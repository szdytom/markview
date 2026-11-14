//! An embeddable document viewport and its independent input state.
use crate::{
	DocumentInteraction, Drag, Grain, Host, Hover, Modifiers, Point, Selection,
};
use markview_layout::{
	scene::{
		LayoutSnapshot, LinkTarget, Rect, Scrollbar, ScrollbarMetrics, Viewport,
	},
	text::TextSelection,
};
use web_time::Instant;

/// Maps host logical coordinates to a document surface. DPI belongs to the host.
#[derive(Clone, Copy)]
pub struct DocumentView<'a> {
	pub context: DocumentInteraction<'a>,
	origin: Point,
	scale: f32,
	scrollbar_track: Option<Rect>,
}
impl<'a> From<DocumentInteraction<'a>> for DocumentView<'a> {
	fn from(context: DocumentInteraction<'a>) -> Self {
		Self {
			context,
			origin: Point::default(),
			scale: 1.0,
			scrollbar_track: None,
		}
	}
}
impl<'a> DocumentView<'a> {
	pub fn embedded(
		snapshot: &'a LayoutSnapshot,
		rect: Rect,
		scale: f32,
		scroll: f32,
		horizontal: &'a crate::Horizontal,
		revision: u64,
	) -> Self {
		assert!(scale > 0.0 && scale.is_finite());
		Self {
			context: DocumentInteraction {
				snapshot,
				viewport: Viewport {
					width: rect.w / scale,
					height: rect.h / scale,
					left: 0.0,
					top: 0.0,
					bottom: 0.0,
					scroll,
				},
				horizontal,
				revision,
			},
			origin: Point::new(rect.x, rect.y),
			scale,
			scrollbar_track: None,
		}
	}
	/// Places the vertical track independently of the padded content viewport.
	pub fn with_scrollbar_track(mut self, track: Rect) -> Self {
		self.scrollbar_track = Some(track);
		self
	}
	pub fn local(&self, point: Point) -> Point {
		Point::new(
			(point.x - self.origin.x) / self.scale,
			(point.y - self.origin.y) / self.scale,
		)
	}
	pub fn window_rect(&self, rect: Rect) -> Rect {
		Rect {
			x: self.origin.x + rect.x * self.scale,
			y: self.origin.y + rect.y * self.scale,
			w: rect.w * self.scale,
			h: rect.h * self.scale,
		}
	}
	pub fn bounds(&self) -> Rect {
		self.window_rect(self.context.viewport.clip())
	}
	pub fn contains(&self, point: Point) -> bool {
		self.context.contains(self.local(point))
			|| (self.max_scroll() > 0.0
				&& self
					.scrollbar_track
					.is_some_and(|r| r.contains(point.x, point.y)))
	}
	pub fn link(&self, point: Point) -> Option<LinkTarget> {
		self.context
			.link_ref(self.local(point))
			.map(|link| link.target.clone())
	}
	pub fn image(&self, point: Point) -> Option<(String, Rect)> {
		self.context
			.image(self.local(point))
			.map(|(src, rect, _)| (src.to_owned(), rect))
	}
	pub fn hover(&self, point: Point, holding: bool) -> Hover {
		self.context.hover(self.local(point), holding, true)
	}
	/// Starts the shared selection machine on any host, including the main reader.
	pub fn press_selection(&self, host: &mut impl Host, now: Instant) {
		let point = self.local(host.cursor());
		if !self.context.contains(point) {
			return;
		}
		let link = self.context.link_ref(point).map(|link| link.target.clone());
		let Some(position) = self.context.position(point) else {
			if let Some(link) = link {
				host.begin_link_press(link);
			}
			return;
		};
		let count = if host.modifiers().shift_key() {
			host.reset_clicks();
			1
		} else {
			host.click_count(now)
		};
		let count = if matches!(link, Some(LinkTarget::Disclosure(_))) {
			1
		} else {
			count
		};
		let grain = match count {
			2 => Some((
				Grain::Word,
				self.context.snapshot.select_word_at(position),
			)),
			3 => Some((
				Grain::Block,
				self.context.snapshot.select_block_at(position),
			)),
			_ => None,
		};
		if !grain.is_some_and(|(grain, selection)| {
			host.begin_grain_selection(selection, grain)
		}) {
			host.begin_selection(position, link);
		}
	}
	pub fn move_selection(&self, host: &mut impl Host) -> bool {
		let mut point = self.local(host.cursor());
		let clip = self.context.viewport.clip();
		point.x = point.x.clamp(clip.x, clip.x + clip.w);
		point.y = point.y.clamp(clip.y, clip.y + clip.h);
		host.move_selection(self.context.position(point), self.context.snapshot)
	}
	pub fn max_scroll(&self) -> f32 {
		(self.context.snapshot.height - self.context.viewport.clip().h).max(0.0)
	}
	pub fn scrollbar(&self, metrics: ScrollbarMetrics) -> Option<Scrollbar> {
		if let Some(track) = self.scrollbar_track {
			return Scrollbar::vertical(
				track,
				self.context.viewport.scroll,
				self.context.snapshot.height,
				self.context.viewport.clip().h,
				metrics,
			);
		}
		Self::vertical_scrollbar(
			self.bounds(),
			self.context.viewport.scroll,
			self.context.snapshot.height,
			self.context.viewport.clip().h,
			metrics,
		)
	}

	fn bar_at(
		&self,
		point: Point,
		metrics: ScrollbarMetrics,
	) -> Option<(Scrollbar, Option<(usize, usize)>)> {
		if let Some(bar) = self
			.scrollbar(metrics)
			.filter(|bar| bar.hit(point.x, point.y))
		{
			return Some((bar, None));
		}
		let (block, overflow, local) =
			self.context.overflow_bar_at(self.local(point), metrics)?;
		let item =
			&self.context.snapshot.blocks[block].layout.overflow[overflow];
		let bar = Scrollbar::horizontal(
			self.window_rect(local.track),
			self.context
				.horizontal
				.get(&(block, overflow))
				.copied()
				.unwrap_or(0.0),
			item.content_width,
			item.rect.w,
			metrics,
		)?;
		Some((bar, Some((block, overflow))))
	}
	pub fn vertical_scrollbar(
		bounds: Rect,
		scroll: f32,
		content: f32,
		visible: f32,
		metrics: ScrollbarMetrics,
	) -> Option<Scrollbar> {
		Scrollbar::vertical(
			Rect {
				x: bounds.x + bounds.w - metrics.thickness,
				y: bounds.y,
				w: metrics.thickness,
				h: bounds.h,
			},
			scroll,
			content,
			visible,
			metrics,
		)
	}
}

#[derive(Clone, Debug)]
pub enum ViewAction {
	Link(LinkTarget),
	Image {
		source: String,
		rect: Rect,
	},
	ScrollTo(f32),
	HorizontalTo {
		block: usize,
		overflow: usize,
		offset: f32,
	},
}
#[derive(Clone, Copy)]
pub enum ViewInput {
	Move(Point),
	Press {
		point: Point,
		modifiers: Modifiers,
		at: Instant,
	},
	Release(Point),
	Wheel {
		point: Point,
		delta: f32,
	},
	Cancel,
	SelectAll,
}
#[derive(Default)]
pub struct ViewUpdate {
	pub consumed: bool,
	pub repaint: bool,
	pub action: Option<ViewAction>,
}

impl ViewUpdate {
	fn handled(action: Option<ViewAction>) -> Self {
		Self {
			consumed: true,
			repaint: true,
			action,
		}
	}
}

struct BarDrag {
	geometry: Scrollbar,
	grab: f32,
	overflow: Option<(usize, usize)>,
}
impl BarDrag {
	fn action(&self, point: Point) -> ViewAction {
		let offset = self.geometry.scroll_for(point.x, point.y, self.grab);
		match self.overflow {
			Some((block, overflow)) => ViewAction::HorizontalTo {
				block,
				overflow,
				offset,
			},
			None => ViewAction::ScrollTo(offset),
		}
	}
}

/// One view's state; no window, file, clipboard or document ownership.
#[derive(Default)]
pub struct ViewState {
	pub selection: Option<TextSelection>,
	pub hover: Hover,
	pub focused: bool,
	cursor: Point,
	modifiers: Modifiers,
	drag: Option<Drag>,
	dragged: bool,
	last_click: Option<(Instant, Point, u8)>,
	image: Option<(String, Rect, Point)>,
	bar: Option<BarDrag>,
	pressed: bool,
}
impl ViewState {
	pub fn captured(&self) -> bool {
		self.pressed
			|| self.drag.is_some()
			|| self.image.is_some()
			|| self.bar.is_some()
	}
	pub fn held_overflow(&self) -> Option<(usize, usize)> {
		self.bar.as_ref().and_then(|bar| bar.overflow)
	}
	pub fn scrollbar_captured(&self) -> bool {
		self.bar.is_some()
	}
	pub fn selected_text(&self, view: &DocumentView<'_>) -> Option<String> {
		let text = view.context.snapshot.extract_text(
			self.selection.filter(|s| !s.is_empty())?,
			view.context.revision,
		);
		(!text.is_empty()).then_some(text)
	}
	pub fn handle(
		&mut self,
		view: &DocumentView<'_>,
		input: ViewInput,
		metrics: ScrollbarMetrics,
	) -> ViewUpdate {
		match input {
			ViewInput::Press {
				point,
				modifiers,
				at,
			} => self.press(view, point, modifiers, at, metrics),
			ViewInput::Move(point) => self.pointer_move(view, point),
			ViewInput::Release(point) => self.release(view, point),
			ViewInput::Wheel { point, delta } => {
				if !view.contains(point) {
					return ViewUpdate::default();
				}
				self.cursor = point;
				ViewUpdate::handled(Some(ViewAction::ScrollTo(
					(view.context.viewport.scroll + delta)
						.clamp(0.0, view.max_scroll()),
				)))
			}
			ViewInput::Cancel => {
				self.pressed = false;
				self.drag = None;
				self.bar = None;
				self.image = None;
				self.focused = false;
				self.hover = Hover::default();
				ViewUpdate::handled(None)
			}
			ViewInput::SelectAll => {
				self.selection =
					view.context.snapshot.select_all(view.context.revision);
				ViewUpdate::handled(None)
			}
		}
	}
	fn press(
		&mut self,
		view: &DocumentView<'_>,
		point: Point,
		modifiers: Modifiers,
		at: Instant,
		metrics: ScrollbarMetrics,
	) -> ViewUpdate {
		if !view.contains(point) {
			return ViewUpdate::default();
		}
		self.cursor = point;
		self.modifiers = modifiers;
		self.focused = true;
		self.pressed = true;
		self.dragged = false;
		self.hover = Hover::default();
		if let Some((bar, overflow)) = view.bar_at(point, metrics) {
			let grab = if bar.on_thumb(point.x, point.y) {
				bar.grab(point.x, point.y)
			} else {
				0.0
			};
			let bar = BarDrag {
				geometry: bar,
				grab,
				overflow,
			};
			let action = bar.action(point);
			self.bar = Some(bar);
			return ViewUpdate::handled(Some(action));
		}
		self.image = view
			.image(point)
			.map(|(source, rect)| (source, rect, point));
		view.press_selection(self, at);
		ViewUpdate::handled(None)
	}
	fn pointer_move(
		&mut self,
		view: &DocumentView<'_>,
		point: Point,
	) -> ViewUpdate {
		self.cursor = point;
		if !view.contains(point) && !self.captured() {
			self.hover = Hover::default();
			return ViewUpdate::default();
		}
		let action = if let Some(bar) = &self.bar {
			Some(bar.action(point))
		} else {
			view.move_selection(self);
			None
		};
		if self.image.as_ref().is_some_and(|(_, _, start)| {
			(start.x - point.x).hypot(start.y - point.y) >= 4.0
		}) {
			self.image = None;
		}
		self.hover = view.hover(point, self.captured());
		ViewUpdate::handled(action)
	}
	fn release(&mut self, view: &DocumentView<'_>, point: Point) -> ViewUpdate {
		if !self.captured() {
			return ViewUpdate::default();
		}
		self.pressed = false;
		self.cursor = point;
		let action = if self.bar.take().is_some() {
			None
		} else {
			self.finish_selection(view.link(point).as_ref())
				.map(ViewAction::Link)
				.or_else(|| self.released_image(view, point))
		};
		self.image = None;
		self.hover = view.hover(point, false);
		ViewUpdate::handled(action)
	}
	fn released_image(
		&mut self,
		view: &DocumentView<'_>,
		point: Point,
	) -> Option<ViewAction> {
		if self.dragged {
			return None;
		}
		let (source, rect, _) = self.image.take()?;
		(view.image(point)?.0 == source)
			.then_some(ViewAction::Image { source, rect })
	}
	pub fn auto_scroll(
		&self,
		view: &DocumentView<'_>,
		seconds: f32,
	) -> Option<f32> {
		if self.drag.is_none() || !self.dragged {
			return None;
		}
		let bounds = view.bounds();
		let speed = crate::selection_scroll_speed(
			self.cursor.y,
			bounds.y,
			bounds.y + bounds.h,
			view.context.viewport.scroll,
			view.max_scroll(),
		);
		(speed != 0.0).then(|| {
			(view.context.viewport.scroll + speed * seconds)
				.clamp(0.0, view.max_scroll())
		})
	}
}
impl Host for ViewState {
	fn cursor(&self) -> Point {
		self.cursor
	}
	fn modifiers(&self) -> Modifiers {
		self.modifiers
	}
	fn selection(&self) -> Option<TextSelection> {
		self.selection
	}
	fn set_selection(&mut self, selection: Option<TextSelection>) {
		self.selection = selection;
	}
	fn drag(&self) -> Option<&Drag> {
		self.drag.as_ref()
	}
	fn set_drag(&mut self, drag: Option<Drag>) {
		self.drag = drag;
	}
	fn take_drag(&mut self) -> Option<Drag> {
		self.drag.take()
	}
	fn dragged(&self) -> bool {
		self.dragged
	}
	fn set_dragged(&mut self, dragged: bool) {
		self.dragged = dragged;
	}
	fn last_click(&self) -> Option<(Instant, Point, u8)> {
		self.last_click
	}
	fn set_last_click(&mut self, click: Option<(Instant, Point, u8)>) {
		self.last_click = click;
	}
	fn blur(&mut self) {}
	fn release_pointer(&mut self) {
		self.bar = None;
		self.image = None;
	}
}

#[cfg(test)]
mod tests;
