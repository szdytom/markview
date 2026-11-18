//! Native floating host for the shared document view; it owns no session or I/O.
use crate::layout::{LayoutSnapshot, Rect};
use markview_interaction::Host;
use std::collections::HashMap;

mod image;

#[derive(Clone, Copy)]
pub(super) struct FrameStyle {
	pub(super) max_height: f32,
	pub(super) right_centered: bool,
	pub(super) picture: bool,
	pub(super) padding: [f32; 4],
	pub(super) metrics: markview_layout::scene::ScrollbarMetrics,
}

impl Default for FrameStyle {
	fn default() -> Self {
		Self {
			max_height: 320.0,
			right_centered: false,
			picture: false,
			padding: Default::default(),
			metrics: Default::default(),
		}
	}
}

/// A reusable rendered scene anchored to document geometry.
pub(super) struct FloatingDocument {
	pub(super) anchor: Rect,
	pub(super) rect: Rect,
	pub(super) snapshot: LayoutSnapshot,
	pub(super) padding: [f32; 4],
	pub(super) revision: markview_document::DocumentRevision,
	pub(super) scroll: markview_interaction::ScrollSurface,
	pub(super) horizontal: HashMap<(usize, usize), f32>,
	pub(super) state: markview_interaction::ViewState,
	pub(super) metrics: markview_layout::scene::ScrollbarMetrics,
	gutter: f32,
	picture: bool,
}

impl FloatingDocument {
	pub(super) fn new(
		mut snapshot: LayoutSnapshot,
		revision: markview_document::DocumentRevision,
		anchor: Rect,
		bounds: Rect,
		frame: FrameStyle,
	) -> Self {
		let FrameStyle {
			max_height,
			mut padding,
			metrics,
			right_centered,
			picture,
		} = frame;
		let picture = picture && {
			let margin = 8.0_f32.min(bounds.w / 2.0).min(bounds.h / 2.0);
			let width = 420.0_f32.min((bounds.w - 2.0 * margin).max(1.0));
			let height = 320.0_f32.min((bounds.h - 2.0 * margin).max(1.0));
			image::fit(&mut snapshot, width, height)
		};
		if picture {
			padding = [0.0; 4];
		}
		let place = |anchor, size, bounds, padding| {
			if right_centered {
				super::chrome::preview::place_right(anchor, size, bounds)
			} else if picture {
				super::chrome::preview::place_image(anchor, size, bounds)
			} else {
				super::chrome::preview::place(anchor, size, bounds, padding)
			}
		};
		let mut rect = place(
			anchor,
			(
				snapshot.width + padding[1] + padding[3],
				snapshot.height.min(max_height) + padding[0] + padding[2],
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
			rect = place(
				anchor,
				(
					snapshot.width + padding[1] + padding[3] + gutter,
					snapshot.height.min(max_height) + padding[0] + padding[2],
				),
				bounds,
				padding,
			);
		}

		Self::embedded(
			snapshot,
			revision,
			anchor,
			rect,
			FrameStyle {
				padding,
				picture,
				..frame
			},
			gutter,
		)
	}

	pub(super) fn picker(
		mut snapshot: LayoutSnapshot,
		body: Rect,
		metrics: markview_layout::scene::ScrollbarMetrics,
		picture: bool,
	) -> Self {
		let picture = picture && image::fit(&mut snapshot, body.w, body.h);
		let revision = snapshot.revision;
		let mut doc = Self::embedded(
			snapshot,
			revision,
			body,
			body,
			FrameStyle {
				metrics,
				picture,
				..Default::default()
			},
			metrics.thickness,
		);
		doc.place_in_picker(body);
		doc
	}

	pub(super) fn place_in_picker(&mut self, body: Rect) {
		self.rect = if self.picture {
			Rect {
				x: body.x + (body.w - self.snapshot.width) / 2.0,
				y: body.y + (body.h - self.snapshot.height) / 2.0,
				w: self.snapshot.width,
				h: self.snapshot.height,
			}
		} else {
			Rect {
				w: body.w + self.gutter,
				..body
			}
		};
		self.scroll.set_max(self.view().max_scroll());
	}

	fn embedded(
		snapshot: LayoutSnapshot,
		revision: markview_document::DocumentRevision,
		anchor: Rect,
		rect: Rect,
		frame: FrameStyle,
		gutter: f32,
	) -> Self {
		let FrameStyle {
			padding,
			picture,
			metrics,
			..
		} = frame;
		let mut floating = Self {
			anchor,
			rect,
			snapshot,
			padding,
			revision,
			scroll: Default::default(),
			horizontal: HashMap::new(),
			state: Default::default(),
			metrics,
			gutter,
			picture,
		};
		floating.scroll.set_max(floating.view().max_scroll());
		floating
	}

	pub(super) fn scene(
		&self,
		rect: Rect,
		theme: crate::render::Theme,
	) -> crate::render::Preview<'_> {
		crate::render::Preview {
			interaction: Some(crate::render::SceneInteraction {
				held_overflow: self.state.held_overflow(),
				selection: self.state.selection,
				revision: self.revision.get(),
				hovered_link: self
					.state
					.hover
					.link
					.as_ref()
					.map(|link| link.link_ref()),
			}),
			snapshot: &self.snapshot,
			scroll: self.scroll.offset(),
			rect,
			theme,
			horizontal: &self.horizontal,
		}
	}
	pub(super) fn focused(&self) -> bool {
		self.state.focused
	}
	pub(super) fn body(&self) -> Rect {
		if self.picture {
			return self.rect;
		}
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
			self.scroll.offset(),
			&self.horizontal,
			self.revision.get(),
		)
		.with_scrollbar_track(self.scrollbar_track())
	}
	pub(super) fn scrollbar(
		&self,
	) -> Option<markview_layout::scene::Scrollbar> {
		if self.picture {
			return None;
		}
		self.view().scrollbar(self.metrics)
	}

	pub(super) fn draw_scrollbar(&self, out: &mut Vec<crate::layout::Draw>) {
		use crate::layout::Draw;
		if let Some(bar) = self.scrollbar() {
			let (track, thumb) = bar.bars();
			out.push(Draw::Rect(
				track,
				crate::layout::Paint::Styled(
					markview_layout::style::Condition::Scrollbar,
					markview_layout::style::ColorField::Track,
				),
			));
			out.push(Draw::Rect(
				thumb,
				crate::layout::Paint::Styled(
					markview_layout::style::Condition::Scrollbar,
					markview_layout::style::ColorField::Thumb,
				),
			));
		}
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
			self.scroll.offset(),
			&self.horizontal,
			self.revision.get(),
		)
		.with_scrollbar_track(self.scrollbar_track());
		let max_scroll = view.max_scroll();
		self.scroll.set_max(max_scroll);
		if matches!(input, markview_interaction::ViewInput::Press { point, .. } if view.contains(point))
		{
			self.scroll.cancel();
		}
		let mut update = self.state.handle(&view, input, self.metrics);
		let scrolling = matches!(
			update.action,
			Some(
				markview_interaction::ViewAction::Scroll(_)
					| markview_interaction::ViewAction::JumpTo(_)
					| markview_interaction::ViewAction::HorizontalTo { .. }
			)
		);
		match update.action.take() {
			Some(markview_interaction::ViewAction::Scroll(request)) => {
				self.scroll.apply(request);
			}
			Some(markview_interaction::ViewAction::JumpTo(scroll)) => {
				self.scroll.jump_to(scroll);
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
			other => update.action = other,
		}
		if scrolling {
			self.refresh_hover();
		}
		update
	}
	pub(super) fn refresh_hover(&mut self) {
		self.state.hover = self
			.view()
			.hover(self.state.cursor(), self.state.captured());
	}
	pub(super) fn horizontal_by(
		&mut self,
		point: markview_interaction::Point,
		delta: f32,
	) -> bool {
		let view = self.view();
		let point = view.local(point);
		let viewport = view.context.viewport;
		let consumed = markview_interaction::horizontal_by(
			&self.snapshot,
			viewport,
			&mut self.horizontal,
			point,
			delta,
		);
		self.refresh_hover();
		consumed
	}
	pub(super) fn pan(
		&mut self,
		overflow: Option<(usize, usize)>,
		dx: f32,
		dy: f32,
	) {
		if let Some((block, overflow)) = overflow {
			let Some(item) = self
				.snapshot
				.blocks
				.get(block)
				.and_then(|block| block.layout.overflow.get(overflow))
			else {
				return;
			};
			let offset = self.horizontal.entry((block, overflow)).or_default();
			*offset = (*offset + dx)
				.clamp(0.0, (item.content_width - item.rect.w).max(0.0));
		}
		self.scroll.set_max(self.view().max_scroll());
		self.scroll.pan_by(dy);
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
		self.scroll.jump_to(scroll);
		self.input(ViewInput::Move(self.state.cursor()));
		true
	}
}
