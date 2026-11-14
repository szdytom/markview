//! Native floating host for the shared document view; it owns no session or I/O.
use crate::layout::{LayoutSnapshot, Rect};
use markview_interaction::Host;
use std::collections::HashMap;

#[derive(Clone, Copy, Default)]
pub(super) struct FrameStyle {
	pub(super) padding: [f32; 4],
	pub(super) metrics: markview_layout::scene::ScrollbarMetrics,
}

/// A reusable rendered scene anchored to document geometry.
pub(super) struct FloatingDocument {
	pub(super) anchor: Rect,
	pub(super) rect: Rect,
	pub(super) snapshot: LayoutSnapshot,
	pub(super) padding: [f32; 4],
	pub(super) revision: markview_document::DocumentRevision,
	pub(super) scroll: f32,
	pub(super) horizontal: HashMap<(usize, usize), f32>,
	pub(super) state: markview_interaction::ViewState,
	pub(super) metrics: markview_layout::scene::ScrollbarMetrics,
	gutter: f32,
}

impl FloatingDocument {
	pub(super) fn new(
		snapshot: LayoutSnapshot,
		revision: markview_document::DocumentRevision,
		anchor: Rect,
		bounds: Rect,
		frame: FrameStyle,
	) -> Self {
		let FrameStyle { padding, metrics } = frame;
		let mut rect = super::chrome::preview::place(
			anchor,
			(
				snapshot.width + padding[1] + padding[3],
				snapshot.height.min(320.0) + padding[0] + padding[2],
			),
			bounds,
			padding,
		);
		let gutter = if rect.h - padding[0] - padding[2] < snapshot.height {
			metrics.thickness
		} else {
			0.0
		};
		if gutter > 0.0 {
			rect = super::chrome::preview::place(
				anchor,
				(
					snapshot.width + padding[1] + padding[3] + gutter,
					snapshot.height.min(320.0) + padding[0] + padding[2],
				),
				bounds,
				padding,
			);
		}

		Self {
			anchor,
			rect,
			snapshot,
			padding,
			revision,
			scroll: 0.0,
			horizontal: HashMap::new(),
			state: Default::default(),
			metrics,
			gutter,
		}
	}

	pub(super) fn focused(&self) -> bool {
		self.state.focused
	}
	pub(super) fn body(&self) -> Rect {
		let [top, right, bottom, left] = self.padding;
		Rect {
			x: self.rect.x + left,
			y: self.rect.y + top,
			w: (self.rect.w - left - right - self.gutter).max(1.0),
			h: (self.rect.h - top - bottom).max(1.0),
		}
	}
	fn scrollbar_track(&self) -> Rect {
		Rect {
			x: self.rect.x + self.rect.w - self.metrics.thickness,
			y: self.rect.y,
			w: self.metrics.thickness,
			h: self.rect.h,
		}
	}
	pub(super) fn view(&self) -> markview_interaction::DocumentView<'_> {
		markview_interaction::DocumentView::embedded(
			&self.snapshot,
			self.body(),
			1.0,
			self.scroll,
			&self.horizontal,
			self.revision.get(),
		)
		.with_scrollbar_track(self.scrollbar_track())
	}
	pub(super) fn scrollbar(
		&self,
	) -> Option<markview_layout::scene::Scrollbar> {
		self.view().scrollbar(self.metrics)
	}

	pub(super) fn input(
		&mut self,
		input: markview_interaction::ViewInput,
	) -> markview_interaction::ViewUpdate {
		let bounds = self.body();
		let view = markview_interaction::DocumentView::embedded(
			&self.snapshot,
			bounds,
			1.0,
			self.scroll,
			&self.horizontal,
			self.revision.get(),
		)
		.with_scrollbar_track(self.scrollbar_track());
		let mut update = self.state.handle(&view, input, self.metrics);
		let scrolling = matches!(
			update.action,
			Some(
				markview_interaction::ViewAction::ScrollTo(_)
					| markview_interaction::ViewAction::HorizontalTo { .. }
			)
		);
		match update.action {
			Some(markview_interaction::ViewAction::ScrollTo(scroll)) => {
				self.scroll = scroll;
				update.action = None;
			}
			Some(markview_interaction::ViewAction::HorizontalTo {
				block,
				overflow,
				offset,
			}) => {
				self.horizontal.insert((block, overflow), offset);
				update.action = None;
			}
			_ => {}
		}
		if scrolling {
			self.refresh_hover();
		}
		update
	}
	fn refresh_hover(&mut self) {
		self.state.hover = self
			.view()
			.hover(self.state.cursor(), self.state.captured());
	}
	pub(super) fn horizontal_by(
		&mut self,
		point: markview_interaction::Point,
		delta: f32,
	) {
		let view = self.view();
		let point = view.local(point);
		let viewport = view.context.viewport;
		markview_interaction::horizontal_by(
			&self.snapshot,
			viewport,
			&mut self.horizontal,
			point,
			delta,
		);
		self.refresh_hover();
	}
	pub(super) fn pan(
		&mut self,
		overflow: Option<(usize, usize)>,
		dx: f32,
		dy: f32,
	) {
		if let Some((block, overflow)) = overflow {
			let item = &self.snapshot.blocks[block].layout.overflow[overflow];
			let offset = self.horizontal.entry((block, overflow)).or_default();
			*offset = (*offset + dx)
				.clamp(0.0, (item.content_width - item.rect.w).max(0.0));
		}
		self.scroll = (self.scroll + dy).clamp(0.0, self.view().max_scroll());
		self.refresh_hover();
	}
	pub(super) fn selected_text(&self) -> Option<String> {
		self.state.selected_text(&self.view())
	}
	pub(super) fn advance_selection(&mut self, seconds: f32) -> bool {
		use markview_interaction::ViewInput;
		let Some(scroll) = self.state.auto_scroll(&self.view(), seconds) else {
			return false;
		};
		self.scroll = scroll;
		self.input(ViewInput::Move(self.state.cursor()));
		true
	}
}
