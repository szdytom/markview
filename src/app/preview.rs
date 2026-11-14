//! Delayed hover previews shared by every hover target.
//!
//! The card itself is chrome (`chrome::preview`); this module owns what the
//! pointer rests on, when the delay elapses, and which target produced the open
//! card. A target only supplies the card's content, so a new preview source
//! (a link, say) adds a `Target` arm instead of a second popup.
//!
//! A page preview is not a screenshot: the card reserves a body rectangle, and
//! the renderer composes the tab's layout instructions into it at device
//! resolution. Idle tabs share a bounded three-screen cache with tab restoration.
mod input;
mod work;
use super::document_view::{FloatingDocument as Floating, FrameStyle};

use super::tabs;
use super::{
	App, TOP,
	chrome::preview::{self as card, Card},
};
use crate::{
	layout::{Draw, LayoutSnapshot, PlacedBlock, Rect},
	render::Theme,
	state::{DisplayState, ReaderSession},
};
use std::{
	collections::{HashMap, HashSet},
	ops::Range,
	time::{Duration, Instant},
};

/// Speculative layout starts after a short stable hover.
pub(super) const PREPARE_DELAY: Duration = Duration::from_millis(30);
/// The display gate stays independent of background completion.
pub(super) const SHOW_DELAY: Duration = Duration::from_millis(100);

/// Per-tab and shared budgets for the resume/hover cache.
pub(super) const PAGE_BUDGET: usize = 8 * 1024 * 1024;
pub(super) const CACHE_BUDGET: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct CachedSlice {
	pub first: usize,
	pub range: Range<f32>,
	pub bytes: usize,
}

/// Three screenfuls shared by tab restoration and hover previews.
pub(crate) struct Page {
	snapshot: LayoutSnapshot,
	scroll: f32,
	horizontal: HashMap<(usize, usize), f32>,
	slice: CachedSlice,
}

impl Page {
	pub(super) fn bytes(&self) -> usize {
		self.slice.bytes
	}
	pub(super) fn capture(session: &ReaderSession) -> Option<Self> {
		session
			.display
			.capture_page(session.scrolling.offset, session.layout_pending)
	}
	pub(super) fn restore(self, session: &mut ReaderSession) {
		if let Some(scroll) = session.display.restore_page(self) {
			session.scrolling.offset = scroll;
			session.layout_pending = true;
		}
	}
}

impl DisplayState {
	fn capture_page(&self, scroll: f32, layout_pending: bool) -> Option<Page> {
		let source = &self.snapshot;
		if source.blocks.is_empty()
			|| (layout_pending && self.cached_slice.is_none())
		{
			return None;
		}
		let viewport = if self.viewport > 0. {
			self.viewport
		} else {
			source.width.max(1.) * card::BODY_RATIO
		};
		let scroll = scroll.max(0.0);
		let mut range = (scroll - viewport).max(0.0)
			..(scroll + viewport * 2.0).min(source.height);
		if let Some(cached) = &self.cached_slice {
			if !cached.covers(scroll, viewport, source.height) {
				return None;
			}
			range.start = range.start.max(cached.range.start);
			range.end = range.end.min(cached.range.end);
		}
		let start = source
			.blocks
			.partition_point(|b| b.y + b.height() < range.start);
		let end = source.blocks.partition_point(|b| b.y <= range.end);
		let blocks = &source.blocks[start..end];
		let mut bytes = 1024;
		for block in blocks {
			// Reject giant blocks before walking their drawing commands.
			if block.layout.draws.storage_bytes() > PAGE_BUDGET {
				return None;
			}
			bytes += block.retained_bytes();
			if bytes > PAGE_BUDGET {
				return None;
			}
		}
		let mut snapshot = LayoutSnapshot {
			presentation_key: source.presentation_key,
			content_id: source.content_id,
			revision: source.revision,
			centered: source.centered,
			document_box: source.document_box.clone(),
			blocks: blocks.to_vec(),
			height: source.height,
			width: source.width,
			..Default::default()
		};
		snapshot.images.generation = source.images.generation;
		bytes += self.capture_images(
			&mut snapshot,
			start,
			&range,
			PAGE_BUDGET - bytes,
		)?;
		let horizontal = self
			.horizontal
			.iter()
			.filter(|&(&(block, _), _)| (start..end).contains(&block))
			.map(|(&(block, command), &offset)| {
				((block - start, command), offset)
			})
			.collect();
		let first =
			start + self.cached_slice.as_ref().map_or(0, |slice| slice.first);
		Some(Page {
			snapshot,
			scroll,
			horizontal,
			slice: CachedSlice {
				first,
				range,
				bytes,
			},
		})
	}
	fn capture_images(
		&self,
		snapshot: &mut LayoutSnapshot,
		start: usize,
		range: &Range<f32>,
		budget: usize,
	) -> Option<usize> {
		let source = &self.snapshot;
		let mut bytes = 0;
		let mut allocations = HashSet::new();
		for (block_index, block) in snapshot.blocks.iter().enumerate() {
			let viewport = Rect {
				x: 0.,
				y: range.start - block.y,
				w: source.width.max(1.),
				h: range.end - range.start,
			};
			for (index, _) in block.draw_commands() {
				let Some(draw) = block.other_draw(index) else {
					continue;
				};
				let Draw::Image {
					src, version, rect, ..
				} = draw
				else {
					continue;
				};
				let Some(mut rect) = block.rect(index, *rect) else {
					continue;
				};
				let (offset, clip) = block.command_view(
					index,
					start + block_index,
					&self.horizontal,
				);
				rect.x -= offset;
				let clip = clip
					.map_or(Some(viewport), |clip| viewport.intersect(clip));
				if clip.and_then(|clip| rect.intersect(clip)).is_none() {
					continue;
				}
				if let Some(info) = source.images.entries.get(src) {
					if !snapshot.images.entries.contains_key(src) {
						bytes += src.len()
							+ info.error.as_ref().map_or(0, String::capacity)
							+ 128;
					}
					snapshot.images.entries.insert(src.clone(), info.clone());
				}
				if let Some(pixels) = source.images.pixels.get(src, *version) {
					bytes += src.len() + 64;
					if allocations.insert(std::sync::Arc::as_ptr(&pixels)) {
						bytes += pixels.rgba.len();
					}
					if bytes > budget {
						return None;
					}
					snapshot.images.pixels.insert(
						src.clone(),
						*version,
						pixels,
					);
				}
			}
		}
		if bytes > budget {
			return None;
		}
		Some(bytes)
	}
	fn restore_page(&mut self, page: Page) -> Option<f32> {
		if self.snapshot.blocks.is_empty() {
			self.snapshot = page.snapshot;
			self.horizontal = page.horizontal;
			self.cached_slice = Some(page.slice);
			Some(page.scroll)
		} else {
			if !page.snapshot.images.entries.is_empty() {
				self.cached_slice = Some(CachedSlice {
					first: 0,
					// Only the captured slice has pinned pixels, even though
					// the whole geometry survives.
					range: page.slice.range,
					bytes: page.slice.bytes,
				});
			}
			self.snapshot.images.pixels = page.snapshot.images.pixels;
			None
		}
	}
}

impl CachedSlice {
	pub(crate) fn target(
		&self,
		old: &LayoutSnapshot,
		new: &LayoutSnapshot,
		scroll: f32,
		viewport: f32,
	) -> Option<f32> {
		if !self.covers(scroll, viewport, old.height) {
			return Some(scroll);
		}
		let index = old
			.blocks
			.partition_point(|b| b.y <= scroll)
			.saturating_sub(1);
		let before = old.blocks.get(index)?;
		// Original indices disambiguate identical blocks outside the cached slice.
		let after = new.blocks.get(self.first + index)?;
		if before.content != after.content {
			return None;
		}
		let local = scroll - before.y;
		// Follow the text under the offset: a picture resized while the tab
		// was idle can move it inside a block whose origin stays put.
		// Loading labels are transient anchors when the geometry is unchanged.
		let relaid = before.height() != after.height()
			|| image_rects(before).ne(image_rects(after));
		if relaid
			&& let Some(y) = markview_layout::layout::intra_block_anchor(
				before, after, local,
			) {
			return Some(after.y + y);
		}
		Some(after.y + local)
	}

	pub(crate) fn covers(
		&self,
		scroll: f32,
		viewport: f32,
		height: f32,
	) -> bool {
		scroll >= self.range.start
			&& (scroll + viewport).min(height) <= self.range.end
	}
}

fn image_rects(block: &PlacedBlock) -> impl Iterator<Item = Rect> + '_ {
	block.draw_commands().filter_map(|(command, _)| {
		let draw = block.other_draw(command)?;
		let Draw::Image { rect, .. } = draw else {
			return None;
		};
		block.rect(command, *rect)
	})
}

/// Fits a rendered scene to its content, bounded by `options.width`.
#[cfg(test)]
fn layout_scene(
	document: &markview_document::Document,
	node: &markview_document::Block,
	options: &crate::layout::LayoutOptions,
	images: &markview_layout::image::ImageSnapshot,
) -> LayoutSnapshot {
	layout_scene_cancellable(document, node, options, images, &|| false)
		.expect("uninterrupted preview")
}

fn layout_scene_cancellable(
	document: &markview_document::Document,
	node: &markview_document::Block,
	options: &crate::layout::LayoutOptions,
	images: &markview_layout::image::ImageSnapshot,
	cancelled: &(dyn Fn() -> bool + Sync),
) -> Option<LayoutSnapshot> {
	use markview_layout::style::Condition;
	let mut engine = crate::layout::LayoutEngine::new();
	let mut snapshot = engine.layout_subtree_cancellable(
		document, node, options, images, cancelled,
	)?;
	if cancelled() {
		return None;
	}
	let mut right = 0.0_f32;
	let mut padding = 0.0_f32;
	for block in &snapshot.blocks {
		for (_, _, rect) in block.clusters() {
			right = right.max(rect.x + rect.w);
		}
		for (_, rect) in block.links() {
			right = right.max(rect.x + rect.w);
		}
		for (command, _) in block.draw_commands() {
			match block.other_draw(command) {
				Some(
					Draw::Image { rect, .. }
					| Draw::Rect(rect, _)
					| Draw::FadedRect { rect, .. }
					| Draw::Clipped { rect, .. },
				) => {
					if let Some(rect) = block.rect(command, *rect) {
						right = right.max(rect.x + rect.w);
					}
				}
				Some(Draw::Math { math, x, .. }) => {
					if let Some([dx, _]) = block.command_offset(command) {
						right = right.max(x + dx + math.width);
					}
				}
				Some(
					draw @ Draw::Box {
						rect,
						chain,
						condition,
						..
					},
				) => {
					if draw_bottom(draw, &options.stylesheet).is_some()
						&& let Some(rect) = block.rect(command, *rect)
					{
						right = right.max(rect.x + rect.w);
					}
					let rule =
						options.stylesheet.element_rule(*chain, *condition);
					padding = padding.max(
						rule.padding
							.as_ref()
							.map_or(0.0, |p| p.sides()[1] * options.font_size),
					);
				}
				_ => {}
			}
		}
	}
	let body = options.stylesheet.rule(Condition::Body);
	let body_padding = if options.preview {
		0.0
	} else {
		body.padding
			.as_ref()
			.map_or(0.0, |p| p.sides()[1] * options.font_size)
	};
	// Keep the original line breaks and glyph positions; only crop the viewport.
	snapshot.width = (right + padding + body_padding)
		.ceil()
		.clamp(1.0, options.width);
	// Outer paragraph and container spacing is not popup padding.
	snapshot.height = scene_content_bottom(&snapshot, &options.stylesheet)
		.ceil()
		.max(1.0)
		.min(snapshot.height);
	(!cancelled()).then_some(snapshot)
}

/// The last visible mark or decorated box, excluding invisible flow wrappers.
fn scene_content_bottom(
	snapshot: &LayoutSnapshot,
	stylesheet: &markview_layout::style::Stylesheet,
) -> f32 {
	let mut bottom = 0.0_f32;
	for block in &snapshot.blocks {
		for rect in block
			.clusters()
			.map(|(_, _, rect)| rect)
			.chain(block.links().map(|(_, rect)| rect))
		{
			bottom = bottom.max(block.y + rect.y + rect.h);
		}
		for (command, [_, dy]) in block.draw_commands() {
			if let Some(draw) = block.other_draw(command)
				&& let Some(edge) = draw_bottom(draw, stylesheet)
			{
				bottom = bottom.max(block.y + dy + edge);
			}
		}
	}
	bottom
}

fn draw_bottom(
	draw: &Draw,
	stylesheet: &markview_layout::style::Stylesheet,
) -> Option<f32> {
	let rect = match draw {
		Draw::Box {
			rect,
			chain,
			condition,
			border,
			decoration,
			..
		} => {
			let background = stylesheet
				.element_rule(*chain, *condition)
				.background
				.is_some_and(|color| color.rgba()[3] > 0.0);
			let edges = decoration
				.as_ref()
				.is_some_and(|d| d.edges.iter().any(|edge| *edge > 0.0));
			return (background || *border > 0.0 || edges)
				.then_some(rect.y + rect.h);
		}
		Draw::Image { rect, .. }
		| Draw::Rect(rect, _)
		| Draw::FadedRect { rect, .. }
		| Draw::Clipped { rect, .. } => rect,
		Draw::Math { math, y, .. } => {
			return Some(y + math.ascent + math.descent);
		}
		Draw::Polygon { center, points, .. } => {
			return points
				.iter()
				.map(|point| center[1] + point[1])
				.max_by(f32::total_cmp);
		}
		Draw::Icon { y, size, .. } => return Some(y + size),
		Draw::Glyph(_) => return None,
	};
	Some(rect.y + rect.h)
}

#[derive(Default)]
pub(super) struct Preview {
	hover: Option<Hover>,
	open: Option<Open>,
	serial: u64,
	pending: Option<Pending>,
	worker: work::Worker,
	#[cfg(test)]
	last_build: Duration,
	/// The body rectangle the chrome reserved this frame, if the card has one.
	body: Option<Rect>,
}

/// What the pointer was resting on. Each owner names its targets, so a tab id
/// cannot collide with a link target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target {
	Tab(u64),
	Footnote {
		block: markview_document::BlockId,
		link: markview_layout::scene::LinkId,
	},
}

struct Hover {
	target: Target,
	at: Instant,
	elapsed: bool,
	attempted: bool,
}

struct Pending {
	source: std::sync::Arc<work::Source>,
	ready: Option<work::Completion>,
}

/// The open card's content. The chrome paints it; the application owns it.
pub(super) struct Open {
	pub(super) target: Target,
	pub(super) card: Card,
	pub(super) floating: Option<Floating>,
}

impl Preview {
	fn cancel_work(&mut self) {
		self.serial = self.serial.wrapping_add(1);
		self.worker.cancel();
		self.pending = None;
	}

	fn accept(&mut self, completion: work::Completion) {
		if completion.serial == self.serial
			&& let Some(pending) = &mut self.pending
		{
			pending.ready = Some(completion);
		}
	}
	pub(super) fn floating_mut(&mut self) -> Option<&mut Floating> {
		self.open.as_mut()?.floating.as_mut()
	}
	pub(super) fn floating(&self) -> Option<&Floating> {
		self.open.as_ref()?.floating.as_ref()
	}

	/// Consumes wheel motion over a scene, including at its scroll limits.
	pub(super) fn scroll(&mut self, point: (f32, f32), delta: f32) -> bool {
		let Some(floating) =
			self.open.as_mut().and_then(|open| open.floating.as_mut())
		else {
			return false;
		};
		if !floating.rect.contains(point.0, point.1) {
			return false;
		}
		floating.input(markview_interaction::ViewInput::Wheel {
			point: markview_interaction::Point::new(point.0, point.1),
			delta,
		});
		true
	}

	pub(super) fn open(&self) -> Option<&Open> {
		self.open.as_ref()
	}

	/// Whether a card is open or a hover is waiting for its delay.
	pub(super) fn tracking(&self) -> bool {
		self.hover.is_some() || self.open.is_some()
	}

	/// Records the target under the pointer and, when the pointer left the
	/// target whose card is open, closes that card.
	pub(super) fn hover(
		&mut self,
		target: Option<Target>,
		now: Instant,
	) -> bool {
		let mut changed = false;
		if self
			.open
			.as_ref()
			.is_some_and(|open| Some(open.target) != target)
		{
			self.open = None;
			self.body = None;
			changed = true;
		}
		if self.hover.as_ref().map(|hover| hover.target) != target {
			self.cancel_work();
			self.hover = target.map(|target| Hover {
				target,
				at: now + SHOW_DELAY,
				elapsed: false,
				attempted: false,
			});
		}
		changed
	}

	/// The target whose hover delay has elapsed, while no card is open.
	#[cfg(test)]
	pub(super) fn due(&self, now: Instant) -> Option<Target> {
		if self.open.is_some() {
			return None;
		}
		self.hover
			.as_ref()
			.filter(|hover| hover.at <= now)
			.map(|hover| hover.target)
	}

	/// When the held hover becomes due, for the event loop's wake-up.
	pub(super) fn deadline(&self) -> Option<Instant> {
		if self.open.is_some() {
			return None;
		}
		let hover = self.hover.as_ref()?;
		if matches!(hover.target, Target::Footnote { .. }) {
			if self.pending.is_none() && !hover.attempted {
				return Some(hover.at - SHOW_DELAY + PREPARE_DELAY);
			}
			if hover.elapsed {
				return None;
			}
		}
		Some(hover.at)
	}

	pub(super) fn show(&mut self, open: Open) {
		self.cancel_work();
		self.open = Some(open);
		self.body = None;
	}

	/// Opens a rendered hover scene; sources supply content and an anchor only.
	fn show_scene(
		&mut self,
		target: Target,
		anchor: Rect,
		bounds: Rect,
		snapshot: LayoutSnapshot,
		revision: markview_document::DocumentRevision,
		frame: FrameStyle,
	) {
		self.show(Open {
			target,
			card: Card {
				title: Vec::new(),
				detail: String::new(),
				body: true,
			},
			floating: Some(Floating::new(
				snapshot, revision, anchor, bounds, frame,
			)),
		});
	}

	/// The rectangle the chrome left for the card's preview body this frame.
	pub(super) fn body(&self) -> Option<Rect> {
		self.open.as_ref()?;
		self.body
	}

	pub(super) fn set_body(&mut self, rect: Option<Rect>) {
		self.body = rect;
	}

	/// Closes the card and forgets the hover, reporting whether a card was
	/// painted.
	pub(super) fn dismiss(&mut self) -> bool {
		self.cancel_work();
		self.hover = None;
		self.body = None;
		self.open.take().is_some()
	}
	/// The page scene the open card shows, for the renderer to compose.
	///
	/// Split from `App` so callers can hold the reader borrow and the renderer
	/// borrow at the same time.
	pub(super) fn scene<'a>(
		&'a self,
		readers: &'a tabs::Tabs,
		theme: Theme,
	) -> Option<crate::render::Preview<'a>> {
		let open = self.open()?;
		let rect = self.body()?;
		if let Some(floating) = open.floating.as_ref() {
			return Some(crate::render::Preview {
				interaction: Some(crate::render::SceneInteraction {
					held_overflow: floating.state.held_overflow(),
					selection: floating.state.selection,
					revision: floating.revision.get(),
					hovered_link: floating
						.state
						.hover
						.link
						.as_ref()
						.map(|link| link.link_ref()),
				}),
				snapshot: &floating.snapshot,
				scroll: floating.scroll,
				rect,
				theme,
				horizontal: &floating.horizontal,
			});
		}
		let Target::Tab(tab) = open.target else {
			return None;
		};

		let index =
			readers.entries().iter().position(|entry| entry.id == tab)?;
		let session = if index == readers.active() {
			&readers.session
		} else {
			&readers.entries()[index].session
		};
		let page = readers.entries()[index].preview.as_ref();
		let (snapshot, scroll, horizontal) = match page {
			Some(page) => (
				&page.snapshot,
				if page.snapshot.centered {
					let b = &page.snapshot.blocks[0];
					(b.y + b.height() / 2.
						- page.snapshot.width * card::BODY_RATIO / 2.)
						.max(0.)
				} else {
					page.scroll
				},
				&page.horizontal,
			),
			None => (
				&session.display.snapshot,
				session.scrolling.offset,
				&session.display.horizontal,
			),
		};
		if snapshot.blocks.is_empty() && page.is_none() {
			return None;
		}
		Some(crate::render::Preview {
			interaction: None,
			snapshot,
			scroll,
			rect,
			theme,
			horizontal,
		})
	}
}

impl<P: super::SendEvent> App<P> {
	/// The hover target under the pointer after it moved, so its card can open
	/// once the delay elapses. Returns whether the painted card changed.
	pub(super) fn hover_preview(&mut self, now: Instant) -> bool {
		if self
			.preview
			.pending
			.as_ref()
			.is_some_and(|pending| !pending.source.matches(self))
		{
			self.preview.cancel_work();
		}
		if let Some(floating) = self.preview.floating() {
			let anchor = floating.anchor;
			let context = self.document_interaction();
			let point = markview_interaction::Point::new(
				anchor.x + anchor.w / 2.0,
				anchor.y + anchor.h / 2.0,
			);
			if context.snapshot.revision != floating.revision
				|| context.link_rect(point) != Some(anchor)
			{
				return self.dismiss_preview();
			}
		}
		let target = if self.preview_allowed() {
			self.target_at_cursor()
		} else {
			None
		};
		self.preview.hover(target, now)
	}

	/// Starts speculative layout at 30 ms and presents no earlier than 100 ms.
	pub(super) fn preview_tick(&mut self, now: Instant) {
		if !self.preview.tracking() {
			return;
		}
		if self.hover_preview(now) {
			self.redraw();
		}
		if !self.preview_allowed() {
			self.dismiss_preview();
			return;
		}
		while let Some(completion) = self.preview.worker.try_recv() {
			self.preview.accept(completion);
		}
		let Some(hover) = self
			.preview
			.hover
			.as_mut()
			.filter(|_| self.preview.open.is_none())
		else {
			return;
		};
		hover.elapsed = now >= hover.at;
		let target = hover.target;
		match target {
			Target::Tab(tab) if hover.elapsed => {
				if !self.open_tab_card(tab) {
					self.preview.dismiss();
				}
				self.redraw();
			}
			Target::Footnote { .. }
				if self.preview.pending.is_none()
					&& now >= hover.at - SHOW_DELAY + PREPARE_DELAY =>
			{
				hover.attempted = true;
				self.start_footnote(target);
			}
			_ => {}
		}
		self.present_footnote();
	}

	fn present_footnote(&mut self) {
		if !self.preview.hover.as_ref().is_some_and(|h| h.elapsed) {
			return;
		}
		let Some(pending) =
			self.preview.pending.as_mut().filter(|p| p.ready.is_some())
		else {
			return;
		};
		let ready = pending.ready.take().unwrap();
		let Some(snapshot) = ready.snapshot else {
			self.dismiss_preview();
			return;
		};
		let source = pending.source.clone();
		log::trace!("Hover layout completed in {:?}", ready.build);
		#[cfg(test)]
		{
			self.preview.last_build = ready.build;
		}
		self.preview.show_scene(
			source.target,
			source.anchor,
			source.bounds,
			snapshot,
			source.revision,
			source.frame,
		);
		self.redraw();
	}

	fn start_footnote(&mut self, target: Target) {
		let Some(source) = self.footnote_source(target) else {
			if self
				.preview
				.hover
				.as_ref()
				.is_some_and(|hover| hover.elapsed)
			{
				self.preview.dismiss();
			}
			return;
		};
		let source = std::sync::Arc::new(source);
		let wake = self.proxy.clone();
		self.preview.worker.submit(
			&self.services.handle,
			self.preview.serial,
			source.clone(),
			move || wake.send(super::Event::PreviewReady),
		);
		self.preview.pending = Some(Pending {
			source,
			ready: None,
		});
	}

	pub(super) fn refresh_open_preview(&mut self) -> bool {
		self.preview.tracking() && self.hover_preview(Instant::now())
	}

	pub(super) fn dismiss_preview(&mut self) -> bool {
		self.preview.dismiss()
	}

	fn preview_allowed(&self) -> bool {
		self.frame.focused
			&& !self.tab_strip.phone
			&& self.interaction.panel == crate::state::PanelPage::Closed
			&& !self.readers.session.search.open
			&& !self.interaction.overlays.modal_open()
			&& !self.interaction.overlays.quick_open.open()
			&& !self.interaction.overlays.dropdown_open()
			&& !self.interaction.overlays.viewer_open()
			&& self.tab_strip.drag.is_none()
			&& self.interaction.pointer.drag().is_none()
	}

	fn target_at_cursor(&mut self) -> Option<Target> {
		let (x, y) = self.interaction.pointer.cursor();
		if self.preview.floating().is_some_and(|floating| {
			floating.focused() || floating.state.captured()
		}) {
			return self.preview.open().map(|open| open.target);
		}
		if y >= TOP {
			if self.pointer_in_outline() || self.pointer_in_folder() {
				return None;
			}
			if let Some(floating) = self.preview.floating() {
				let r = floating.rect;
				let near = Rect {
					x: r.x - 6.0,
					y: r.y - 6.0,
					w: r.w + 12.0,
					h: r.h + 12.0,
				};
				if near.contains(x, y) {
					return self.preview.open().map(|open| open.target);
				}
			}
			let context = self.document_interaction();
			let link =
				context.link_ref(markview_interaction::Point::new(x, y))?;
			return matches!(
				link.target,
				markview_layout::scene::LinkTarget::FootnoteDefinition(_)
			)
			.then_some(Target::Footnote {
				block: link.block,
				link: link.id,
			});
		}
		self.tab_at_cursor()
			.map(|index| Target::Tab(self.readers.entries()[index].id))
	}

	fn footnote_source(&self, target: Target) -> Option<work::Source> {
		let Target::Footnote { block, link } = target else {
			return None;
		};
		let context = self.document_interaction();
		let (x, y) = self.interaction.pointer.cursor();
		let point = markview_interaction::Point::new(x, y);
		let reference = context.link_ref(point)?;
		if reference.block != block || reference.id != link {
			return None;
		}
		let markview_layout::scene::LinkTarget::FootnoteDefinition(id) =
			reference.target
		else {
			return None;
		};
		let anchor = context.link_rect(point)?;
		let bounds = context.viewport.clip();
		let revision = context.snapshot.revision;
		let document = self.readers.session.document.as_ref()?;
		let original_options = self.options();
		let mut options = original_options.clone();
		options.preview = true;
		let style = options.stylesheet.element_rule(
			markview_layout::style::Condition::Preview.chain(),
			markview_layout::style::Condition::Preview,
		);
		let padding = style.padding.as_ref().map_or([10.0; 4], |p| {
			p.sides().map(|side| side * options.font_size)
		});
		let metrics = options.stylesheet.scrollbar_metrics();
		options.width = 420.0_f32.min(
			(bounds.w - 16.0 - padding[1] - padding[3] - metrics.thickness)
				.max(1.0),
		);
		options.viewport_height = 0.0;
		Some(work::Source {
			target,
			anchor,
			bounds,
			revision,
			frame: FrameStyle { padding, metrics },
			document: document.clone(),
			definition: id.clone(),
			original_options,
			options,
			images: self.readers.session.display.snapshot.images.clone(),
			version: self.readers.session.version,
		})
	}

	/// Fills the card from a tab that is still open. Returns whether it could.
	fn open_tab_card(&mut self, tab: u64) -> bool {
		let Some(index) =
			self.readers.entries().iter().position(|e| e.id == tab)
		else {
			return false;
		};
		let (name, detail) = self.readers.card_labels(index);
		let title = card::title_lines(&mut self.ui, &name);
		let detail = card::detail_line(&mut self.ui, &detail);
		let active = index == self.readers.active();
		let session = if active {
			&self.readers.session
		} else {
			&self.readers.entries()[index].session
		};
		// The active page already fills the window, so its card names it only.
		let body = !active
			&& (!session.display.snapshot.blocks.is_empty()
				|| self.readers.entries()[index].preview.is_some());
		self.preview.show(Open {
			target: Target::Tab(tab),
			floating: None,
			card: Card {
				title,
				detail,
				body,
			},
		});
		true
	}
}

#[cfg(test)]
mod tests;
