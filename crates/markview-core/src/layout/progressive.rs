//! A layout pass that can be suspended between blocks.
//!
//! [`LayoutEngine::layout_progressive`] finishes a document in one call and
//! visits each prefix on the way. A front end that must hand a browser a frame
//! cannot spend that long, so it needs the same pass split across calls: a
//! [`ProgressiveLayout`] keeps the state a pass carries and
//! [`LayoutEngine::advance`] lays out the next blocks under a time budget.
//! Because the pass is never restarted, a step pays only for the blocks it has
//! not reached yet, however long the already-laid-out prefix has grown.

use super::{
	BlockContext, CacheEntry, CacheKey, LayoutEngine, LayoutOptions,
	blocks::anchor_only, external_key,
};
use crate::{
	document::Document,
	image::ImageSnapshot,
	scene::{BoxDecoration, Draw, LayoutSnapshot, PlacedBlock, Rect},
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
}

/// What a pass resolves once and uses for every block.
struct Pass {
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
		let pass = Pass {
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
		self.advance_cancellable(layout, document, budget, &|| false)
	}

	pub(super) fn advance_cancellable(
		&mut self,
		layout: &mut ProgressiveLayout,
		document: &Document,
		budget: Duration,
		cancelled: &dyn Fn() -> bool,
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
			self.block(layout, document, cancelled);
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

	/// Lays out the block at the pass's cursor.
	fn block(
		&mut self,
		layout: &mut ProgressiveLayout,
		document: &Document,
		cancelled: &dyn Fn() -> bool,
	) {
		{
			let index = layout.index;
			let child_index = layout.visible_index;
			let child_count = layout.visible_count;
			let visible = !anchor_only(&document.blocks[index]);
			layout.visible_index += usize::from(visible);
			let ProgressiveLayout { result, pass, .. } = layout;
			let block = &document.blocks[index];
			let key = CacheKey {
				position: if visible
					&& pass.options.stylesheet.has_child_rules()
				{
					u8::from(child_index == 0)
						| (u8::from(child_index + 1 == child_count) << 1)
				} else {
					0
				},
				external: external_key(
					block,
					&pass.images,
					&self.highlights,
					pass.codeblock_theme.as_deref(),
					&pass.options,
				),
				content: block.content_key,
				width: pass.options.width.to_bits(),
				size: pass.options.font_size.to_bits(),
				justify: pass.options.justify,
				hyphenate: pass.options.hyphenate,
				justification: pass.options.justification.bits(),
				paragraph_indent: pass.options.paragraph_indent.to_bits(),
				greedy: pass.options.greedy,
				codeblock_wrap: pass.options.codeblock_wrap,
				codeblock_theme: pass.codeblock_theme_key,
				style: pass.style_key,
			};
			let cached = self.cache.get_mut(&key).map(|entry| {
				entry.pass = pass.id;
				entry.layout.clone()
			});
			let geometry = if let Some(cached) = cached {
				result.reused += 1;
				cached
			} else {
				let measured = crate::profile::measure(
					crate::profile::Stage::Blocks,
					|| {
						let mut out = crate::scene::BlockLayout::default();
						self.shaper.appearance = if visible {
							pass.options.stylesheet.child(
								&pass.appearance,
								child_index,
								child_count,
							)
						} else {
							pass.appearance.clone()
						};
						BlockContext {
							cancelled,
							search_fields: crate::search::layout_fields(block),
							shaper: &mut self.shaper,
							math: &mut self.math,
							images: &pass.images,
							highlight_cache: self.highlights.results(),
							marker_depth: 0,
							enum_depth: 0,
						}
						.block(
							block,
							pass.pad[3],
							0.0,
							pass.content_width,
							&pass.options,
							&mut out,
						);
						out.seal_scene();
						Arc::new(out)
					},
				);
				if cancelled() {
					return;
				}
				self.cache.insert(
					key,
					CacheEntry {
						layout: measured.clone(),
						pass: pass.id,
						completed: None,
					},
				);
				measured
			};
			let flow = Arc::new(geometry.resolve_flow(
				&pass.options.details_open,
				pass.options.force_open,
			));
			result.height += flow.height;
			result.blocks.push(PlacedBlock {
				id: block.id,
				source: block.source.clone(),
				y: result.height - flow.height,
				flow,
				layout: geometry.clone(),
			});
			result.degraded += geometry.degraded;
			result.math_errors += geometry.math_errors;
			// The body box is the document's background, so it must cover every
			// block the prefix has published, this one included; leaving it at
			// the height from before the block would paint a prefix without a
			// background behind its newest block.
			if let Some(Draw::Box { rect, .. }) = &mut result.document_box {
				rect.h = result.height;
			}
		}
		layout.index += 1;
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
		self.completed = Some(id);
	}
}
