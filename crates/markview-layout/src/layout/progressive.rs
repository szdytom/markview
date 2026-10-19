//! A layout pass that can be suspended between blocks.
//!
//! [`LayoutEngine::layout_progressive`] finishes a document in one call and
//! visits each prefix on the way. A front end that must hand a browser a frame
//! cannot spend that long, so it needs the same pass split across calls: a
//! [`ProgressiveLayout`] keeps the state a pass carries and
//! [`LayoutEngine::advance`] lays out the next blocks under a time budget.
//! Because the pass is never restarted, a step pays only for the blocks it has
//! not reached yet, however long the already-laid-out prefix has grown.

#[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
pub(super) mod parallel;

use super::{
	BlockContext, CacheEntry, CacheKey, LayoutEngine, LayoutOptions,
	blocks::anchor_only, cache, external_key,
};
use crate::{
	document::{Block, Document},
	image::ImageSnapshot,
	scene::{
		BlockLayout, BoxDecoration, Draw, LayoutSnapshot, PlacedBlock, Rect,
	},
	style::{Condition, TextAppearance},
};
use std::{sync::Arc, time::Duration};
use web_time::Instant;

/// One layout pass, carried between calls.
///
/// The snapshot it publishes is a valid prefix of the document at every point:
/// a caller may draw it while the pass is still running. It is only final once
/// [`Self::is_complete`] reports so, because the closing body padding is added
/// with the last block.
pub struct ProgressiveLayout {
	result: LayoutSnapshot,
	pass: Pass,
	/// The next block to lay out.
	index: usize,
	/// How many blocks the document has, fixed when the pass began.
	count: usize,
	/// Positions used by child selectors exclude invisible target blocks.
	visible_index: usize,
	visible_count: usize,
	/// The source of the document this pass belongs to. It is the same `Arc`
	/// the document holds, so the two compare by identity on every call.
	source: Arc<str>,
	#[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
	pending: std::collections::VecDeque<(CacheKey, Arc<BlockLayout>)>,
}

/// What a pass resolves once and uses for every block.
struct Pass {
	#[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
	parallel_start: f32,
	options: LayoutOptions,
	images: ImageSnapshot,
	pad: [f32; 4],
	content_width: f32,
	codeblock_theme: Option<String>,
	codeblock_theme_key: u64,
	style_key: u64,
	/// The body appearance every block's own appearance descends from.
	appearance: TextAppearance,
	/// What the closing body padding and margin add once every block is placed.
	trailing: f32,
	/// The body box, retaken with the height of whatever is laid out so far.
	box_template: Draw,
	/// The pass stamp cache entries carry.
	id: u64,
}

impl ProgressiveLayout {
	/// Changes presentation while continuing the same geometry pass.
	pub fn set_disclosures(
		&mut self,
		open: Arc<std::collections::BTreeMap<u64, bool>>,
		force_open: bool,
	) {
		self.result.set_disclosures(&open, force_open);
		self.pass.options.details_open = open;
		self.pass.options.force_open = force_open;
	}
	/// The blocks laid out so far, as a snapshot a renderer can draw.
	pub fn snapshot(&self) -> &LayoutSnapshot {
		&self.result
	}
	/// Whether every block has been laid out.
	pub fn is_complete(&self) -> bool {
		self.index >= self.count
	}
	/// How many blocks are laid out so far.
	pub fn blocks(&self) -> usize {
		self.index
	}
	/// The finished snapshot.
	pub fn into_snapshot(self) -> LayoutSnapshot {
		self.result
	}
	/// Identifies this pass. Every pass an engine begins has its own id, so a
	/// caller can tell whether two prefixes belong to the same layout run
	/// rather than merely to the same document.
	pub fn pass_id(&self) -> u64 {
		self.pass.id
	}
}

impl LayoutEngine {
	/// Starts a pass on `document` without laying out any block.
	///
	/// The document is passed to every [`Self::advance`] rather than stored, so
	/// a caller that owns it pays nothing to hand it over and nothing to take
	/// it back.
	pub fn begin_layout(
		&mut self,
		document: &Document,
		options: &LayoutOptions,
		images: &ImageSnapshot,
	) -> ProgressiveLayout {
		self.shaper.set_stylesheet(options.stylesheet.clone());
		self.shaper.set_fonts(&options.fonts);
		self.math.set_limits(options.limits);
		self.poll_highlights();
		let body = options.stylesheet.rule(Condition::Body);
		let pad = body
			.padding
			.as_ref()
			.map(|p| p.sides().map(|v| v * options.font_size))
			.unwrap_or([0.; 4]);
		let content_width = (options.width - pad[1] - pad[3]).max(1.);
		crate::profile::span(crate::profile::Stage::Highlights, || {
			self.highlights.prepare(
				document.content_id,
				&document.blocks,
				options,
			)
		});
		// A host with no threads colors `prepare`'s jobs in that very call, so
		// their results are already waiting here; a host with threads leaves
		// this a no-op and picks them up on a later pass. Without this the
		// blocks measured below would miss the colors that just arrived, and
		// edited code would stay uncolored until the next update.
		self.poll_highlights();
		let codeblock_theme =
			options.codeblock_theme_override.clone().or_else(|| {
				options.stylesheet.rule(Condition::CodeBlock).theme.clone()
			});
		let codeblock_theme_key =
			crate::document::fingerprint(&codeblock_theme);
		// The stylesheet is immutable for this pass. Its identity belongs to the
		// document request, not to each block's cache lookup. The fonts are part
		// of it: geometry measured with other faces is stale.
		let style_key = document
			.blocks
			.first()
			.map(|_| {
				crate::document::fingerprint(&(
					options.stylesheet.layout_key(),
					&options.fonts,
					options.limits,
				))
			})
			.unwrap_or_default();
		let opening =
			pad[0] + body.space_before.unwrap_or(0.) * options.font_size;
		let box_template = Draw::Box {
			rect: Rect {
				x: 0.,
				y: 0.,
				w: options.width,
				h: opening,
			},
			chain: Condition::Body.chain(),
			condition: Condition::Body,
			radius: body.radius.unwrap_or(0.),
			border: body.border_width.unwrap_or(0.),
			left_only: false,
			decoration: BoxDecoration::from_rule(body, false),
		};
		self.pass = self.pass.wrapping_add(1);
		// A pass that is abandoned never reaches `close`, so its entries would
		// otherwise pile up. Narrowing the cache here keeps an abandoned pass
		// from accumulating geometry, while the pass before this one and the
		// newest completed one stay reusable: cancelling before the first block
		// must not drop the last document's geometry, and an abandoned pass only
		// touched a few blocks to add.
		let previous = self.pass.wrapping_sub(1);
		// Membership, not last use, says whether an entry belongs to the last
		// completed pass: a pass that only reused geometry and was then
		// abandoned must not evict it. A `None` engine pass matches no entry,
		// so entries that never completed a pass are kept by recency alone.
		let completed = self.completed;
		self.cache.retain(|_, entry| {
			entry.pass == previous
				|| completed.is_some() && entry.completed == completed
		});
		self.nested_cache.retain(|_, entry| {
			entry.pass == previous
				|| completed.is_some() && entry.completed == completed
		});
		let pass = Pass {
			#[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
			parallel_start: self.parallel_start,
			options: options.clone(),
			images: images.clone(),
			pad,
			content_width,
			codeblock_theme,
			codeblock_theme_key,
			style_key,
			appearance: self.shaper.appearance.clone(),
			trailing: pad[2]
				+ body.space_after.unwrap_or(0.) * options.font_size,
			box_template,
			id: self.pass,
		};
		let mut result = LayoutSnapshot {
			presentation_key: crate::document::fingerprint(&(
				&options.details_open,
				options.force_open,
			)),
			content_id: document.content_id,
			images: images.clone(),
			width: options.width,
			height: opening,
			..Default::default()
		};
		result.document_box = Some(pass.box_template.clone());
		let mut layout = ProgressiveLayout {
			result,
			pass,
			index: 0,
			count: document.blocks.len(),
			visible_index: 0,
			visible_count: document
				.blocks
				.iter()
				.filter(|b| !anchor_only(b))
				.count(),
			source: document.source.clone(),
			#[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
			pending: Default::default(),
		};
		// An empty document has nothing to advance, so it closes here.
		if layout.is_complete() {
			self.close(&mut layout);
		}
		layout
	}

	/// Lays out blocks until `budget` is spent, and returns whether the document
	/// is complete.
	///
	/// A zero budget still lays out exactly one block, so a caller can never
	/// spin without progress, and `budget` is honoured to within the block in
	/// flight: the step always stops at a block boundary.
	pub fn advance(
		&mut self,
		layout: &mut ProgressiveLayout,
		document: &Document,
		budget: Duration,
	) -> bool {
		self.advance_cancellable(layout, document, budget, &|| false, false)
	}

	pub(super) fn advance_cancellable(
		&mut self,
		layout: &mut ProgressiveLayout,
		document: &Document,
		budget: Duration,
		cancelled: &(dyn Fn() -> bool + Sync),
		_prefetch: bool,
	) -> bool {
		// A pass is a position in one document's block list, so advancing it
		// with another document would read past the end or silently lay out a
		// mixture of the two. The source `Arc` is the document's identity, and
		// the check holds in release as well as in debug.
		assert!(
			Arc::ptr_eq(&layout.source, &document.source),
			"a pass must be advanced with the document it began on"
		);
		assert_eq!(
			layout.count,
			document.blocks.len(),
			"a pass must be advanced with the document it began on"
		);
		// The pass also carries the stylesheet, fonts and math limits that were
		// in force when it began, and it re-applies none of them per block: a
		// later pass on this engine would leave the rest of this one measuring
		// against the configuration that replaced it. Only the newest pass owns
		// the engine, and resuming an older one is a caller error rather than a
		// silently mixed layout.
		assert_eq!(
			layout.pass.id, self.pass,
			"a pass must be advanced before another pass begins"
		);
		if layout.is_complete() {
			return true;
		}
		let started = Instant::now();
		loop {
			self.block(layout, document, cancelled, _prefetch);
			if cancelled() {
				return false;
			}
			if layout.is_complete() {
				return true;
			}
			if started.elapsed() >= budget {
				return false;
			}
		}
	}

	/// Measures local geometry, then publishes one block in document order.
	fn block(
		&mut self,
		layout: &mut ProgressiveLayout,
		document: &Document,
		cancelled: &(dyn Fn() -> bool + Sync),
		_prefetch: bool,
	) {
		let block = &document.blocks[layout.index];
		let job = layout.pass.job(
			block,
			layout.visible_index,
			layout.visible_count,
			self.highlights.results(),
		);
		let cached = self.cache.get_mut(&job.key).map(|entry| {
			entry.pass = layout.pass.id;
			entry.layout.clone()
		});
		let geometry = if let Some(cached) = cached {
			layout.result.reused += 1;
			cache::remap_link_ids(cached, block, &layout.pass.options.limits)
		} else {
			#[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
			if _prefetch {
				self.prefetch(layout, document, cancelled);
				if cancelled() {
					return;
				}
			}
			let measured =
				layout.take_prefetched(&job.key).unwrap_or_else(|| {
					Measurement {
						pass: &layout.pass,
						nested: &self.nested_cache,
						highlights: self.highlights.results(),
						cancelled,
					}
					.measure(&job, &mut self.shaper, &mut self.math)
				});
			if cancelled() {
				return;
			}
			self.cache.insert(
				job.key,
				CacheEntry {
					layout: measured.clone(),
					pass: layout.pass.id,
					completed: None,
				},
			);
			measured
		};
		self.retain_children(&geometry, layout.pass.id);
		layout.place(block, geometry);
		if layout.is_complete() {
			self.close(layout);
		}
	}

	/// Adds what only the end of a pass can know.
	fn close(&mut self, layout: &mut ProgressiveLayout) {
		layout.result.height += layout.pass.trailing;
		let mut document_box = layout.pass.box_template.clone();
		if let Draw::Box { rect, .. } = &mut document_box {
			rect.h = layout.result.height;
		}
		layout.result.document_box = Some(document_box);
		// A complete pass visited every block, so an entry it did not touch
		// belongs to a superseded document, option set, or highlight state.
		// Dropping those keeps the cache at one document's worth of geometry.
		let id = layout.pass.id;
		self.cache.retain(|_, entry| entry.pass == id);
		// Every survivor is part of this completing pass, so stamp that
		// membership now: a later reuse must not erase it.
		for entry in self.cache.values_mut() {
			entry.completed = Some(id);
		}
		self.nested_cache.retain(|_, entry| entry.pass == id);
		for entry in self.nested_cache.values_mut() {
			entry.completed = Some(id);
		}
		self.completed = Some(id);
	}
}

struct BlockJob<'a> {
	block: &'a Block,
	key: CacheKey,
	child_index: usize,
	child_count: usize,
}

impl Pass {
	fn job<'a>(
		&self,
		block: &'a Block,
		child_index: usize,
		child_count: usize,
		highlights: &std::collections::HashMap<
			u64,
			super::highlights::HighlightResult,
		>,
	) -> BlockJob<'a> {
		let visible = !anchor_only(block);
		let options = &self.options;
		let key = CacheKey {
			position: if visible && options.stylesheet.has_child_rules() {
				u8::from(child_index == 0)
					| (u8::from(child_index + 1 == child_count) << 1)
			} else {
				0
			},
			external: external_key(
				block,
				&self.images,
				highlights,
				self.codeblock_theme.as_deref(),
				options,
			),
			content: block.content_key,
			width: options.width.to_bits(),
			size: options.font_size.to_bits(),
			justify: options.justify,
			hyphenate: options.hyphenate,
			justification: options.justification.bits(),
			paragraph_indent: options.paragraph_indent.to_bits(),
			greedy: options.greedy,
			codeblock_wrap: options.codeblock_wrap,
			codeblock_theme: self.codeblock_theme_key,
			style: self.style_key,
		};
		BlockJob {
			block,
			key,
			child_index,
			child_count,
		}
	}
}

struct Measurement<'a> {
	pass: &'a Pass,
	nested: &'a std::collections::HashMap<u64, super::cache::NestedEntry>,
	highlights:
		&'a std::collections::HashMap<u64, super::highlights::HighlightResult>,
	cancelled: &'a (dyn Fn() -> bool + Sync),
}

impl Measurement<'_> {
	fn measure(
		&self,
		job: &BlockJob<'_>,
		shaper: &mut super::TextShaper,
		math: &mut crate::math::MathEngine,
	) -> Arc<BlockLayout> {
		crate::profile::measure(crate::profile::Stage::Blocks, || {
			let mut out = BlockLayout::default();
			shaper.appearance = if anchor_only(job.block) {
				self.pass.appearance.clone()
			} else {
				self.pass.options.stylesheet.child(
					&self.pass.appearance,
					job.child_index,
					job.child_count,
				)
			};
			BlockContext {
				nested: Some(super::cache::NestedCache {
					entries: self.nested,
					base: &job.key,
				}),
				cancelled: self.cancelled,
				search_fields: crate::search::layout_fields(job.block),
				shaper,
				math,
				images: &self.pass.images,
				highlight_cache: self.highlights,
				marker_depth: 0,
				enum_depth: 0,
			}
			.block(
				job.block,
				self.pass.pad[3],
				0.,
				self.pass.content_width,
				&self.pass.options,
				&mut out,
			);
			out.seal_scene();
			Arc::new(out)
		})
	}
}

impl ProgressiveLayout {
	fn take_prefetched(&mut self, key: &CacheKey) -> Option<Arc<BlockLayout>> {
		#[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
		{
			self.pending.pop_front().map(|(prepared, geometry)| {
				assert!(
					prepared == *key,
					"prefetched geometry must match its block"
				);
				geometry
			})
		}
		#[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
		{
			let _ = key;
			None
		}
	}

	fn place(&mut self, block: &Block, geometry: Arc<BlockLayout>) {
		let flow = Arc::new(geometry.resolve_flow(
			&self.pass.options.details_open,
			self.pass.options.force_open,
		));
		let result = &mut self.result;
		result.height += flow.height;
		let y = result.height - flow.height;
		result.degraded += geometry.degraded;
		result.math_errors += geometry.math_errors;
		result.blocks.push(PlacedBlock {
			id: block.id,
			source: block.source.clone(),
			y,
			flow,
			layout: geometry,
		});
		if let Some(Draw::Box { rect, .. }) = &mut result.document_box {
			rect.h = result.height;
		}
		self.index += 1;
		self.visible_index += usize::from(!anchor_only(block));
	}
}
