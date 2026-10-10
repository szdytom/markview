//! Document hit testing and hover, independent of a host's chrome.
use crate::Point;
use markview_core::{
	layout::LayoutSnapshot,
	scene::{Rect, Scrollbar, ScrollbarMetrics, Viewport},
	text::TextPosition,
};
use std::collections::HashMap;

pub type Horizontal = HashMap<(usize, usize), f32>;

/// The document geometry currently displayed by a host.
#[derive(Clone, Copy)]
pub struct DocumentInteraction<'a> {
	pub snapshot: &'a LayoutSnapshot,
	pub viewport: Viewport,
	pub horizontal: &'a Horizontal,
	pub revision: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Cursor {
	#[default]
	Default,
	Text,
	Pointer,
}
impl Cursor {
	pub fn css(self) -> &'static str {
		match self {
			Self::Default => "default",
			Self::Text => "text",
			Self::Pointer => "pointer",
		}
	}
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Hover {
	pub link: Option<String>,
	pub image_title: Option<String>,
	pub overflow: Option<(usize, usize)>,
	pub cursor: Cursor,
}
impl DocumentInteraction<'_> {
	pub fn contains(&self, point: Point) -> bool {
		self.viewport.clip().contains(point.x, point.y)
	}
	pub fn position(&self, point: Point) -> Option<TextPosition> {
		let (x, y) = self.viewport.document_point(point.x, point.y);
		self.snapshot
			.hit_test_text(x, y, self.horizontal, self.revision)
	}
	pub fn contains_text(&self, point: Point) -> bool {
		if !self.contains(point) {
			return false;
		}
		let (x, y) = self.viewport.document_point(point.x, point.y);
		self.snapshot.contains_text(x, y, self.horizontal)
	}
	pub fn link(&self, point: Point) -> Option<&str> {
		if !self.contains(point) {
			return None;
		}
		let (x, y) = self.viewport.document_point(point.x, point.y);
		self.snapshot.link_at(x, y, self.horizontal)
	}
	pub fn image(&self, point: Point) -> Option<(&str, Rect, &str)> {
		if !self.contains(point) {
			return None;
		}
		let (x, y) = self.viewport.document_point(point.x, point.y);
		self.snapshot
			.image_at(x, y, self.horizontal)
			.map(|(src, _, rect, title)| (src, rect, title))
	}
	pub fn overflow_bar(
		&self,
		block: usize,
		overflow: usize,
		metrics: ScrollbarMetrics,
	) -> Option<Scrollbar> {
		let placed = self.snapshot.blocks.get(block)?;
		let item = placed.layout.overflow.get(overflow)?;
		let rect = placed.overflow_rect(overflow)?;
		let track = self.viewport.window_rect(Rect {
			x: rect.x,
			y: placed.y + rect.y + rect.h,
			w: item.rect.w,
			h: metrics.overflow_band(item.gutter),
		});
		Scrollbar::horizontal(
			track,
			self.horizontal
				.get(&(block, overflow))
				.copied()
				.unwrap_or(0.0),
			item.content_width,
			item.rect.w,
			metrics,
		)
	}
	pub fn overflow_bar_at(
		&self,
		point: Point,
		metrics: ScrollbarMetrics,
	) -> Option<(usize, usize, Scrollbar)> {
		if !self.contains(point) {
			return None;
		}
		let (_, y) = self.viewport.document_point(point.x, point.y);
		for (block, placed) in self.snapshot.blocks.iter().enumerate() {
			if y < placed.y || y > placed.y + placed.height() {
				continue;
			}
			for overflow in 0..placed.layout.overflow.len() {
				if let Some(bar) = self.overflow_bar(block, overflow, metrics)
					&& bar.hit(point.x, point.y)
				{
					return Some((block, overflow, bar));
				}
			}
		}
		None
	}
	pub fn overflow_at(&self, point: Point) -> Option<(usize, usize)> {
		if !self.contains(point) {
			return None;
		}
		let (x, y) = self.viewport.document_point(point.x, point.y);
		self.snapshot.blocks.iter().enumerate().find_map(|(bi, b)| {
			b.layout
				.overflow
				.iter()
				.enumerate()
				.find(|(oi, _)| {
					b.overflow_rect(*oi).is_some_and(|r| r.contains(x, y - b.y))
				})
				.map(|(oi, _)| oi)
				.map(|oi| (bi, oi))
		})
	}
	/// Resolves document cursor priority for the actions this host supports.
	pub fn cursor(
		&self,
		point: Point,
		links_clickable: bool,
		images_clickable: bool,
	) -> Cursor {
		if (links_clickable && self.link(point).is_some())
			|| (images_clickable && self.image(point).is_some())
		{
			Cursor::Pointer
		} else if self.contains_text(point) {
			Cursor::Text
		} else {
			Cursor::Default
		}
	}

	pub fn hover(
		&self,
		point: Point,
		holding: bool,
		images_clickable: bool,
		metrics: ScrollbarMetrics,
	) -> Hover {
		if holding {
			return Hover {
				cursor: if self.contains_text(point) {
					Cursor::Text
				} else {
					Cursor::Default
				},
				..Hover::default()
			};
		}
		let link = self.link(point).map(str::to_owned);
		let image = self.image(point);
		let overflow =
			self.overflow_bar_at(point, metrics).map(|(b, o, _)| (b, o));
		let cursor = if link.is_some() || (images_clickable && image.is_some())
		{
			Cursor::Pointer
		} else if self.contains_text(point) {
			Cursor::Text
		} else {
			Cursor::Default
		};
		Hover {
			link,
			image_title: image
				.map(|(_, _, title)| title)
				.filter(|title| !title.is_empty())
				.map(str::to_owned),
			overflow,
			cursor,
		}
	}
}

/// Pans one overflowing block and reports whether a block consumed the input.
pub fn horizontal_by(
	snapshot: &LayoutSnapshot,
	viewport: Viewport,
	horizontal: &mut Horizontal,
	point: Point,
	delta: f32,
) -> bool {
	let context = DocumentInteraction {
		snapshot,
		viewport,
		horizontal,
		revision: 0,
	};
	let Some((block, overflow)) = context.overflow_at(point) else {
		return false;
	};
	let item = &snapshot.blocks[block].layout.overflow[overflow];
	let offset = horizontal.entry((block, overflow)).or_default();
	*offset = (*offset + delta)
		.clamp(0.0, (item.content_width - item.rect.w).max(0.0));
	true
}
