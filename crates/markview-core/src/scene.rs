//! Immutable drawing and hit-test geometry shared by layout and rendering.
use crate::{math::MathBox, text::TextNode};
use parley::FontData;
use std::{collections::HashMap, ops::Range, sync::Arc};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Paint {
	Color(crate::style::Color),
	Styled(crate::style::Condition, crate::style::ColorField),
	Cascade(u128, crate::style::ColorField),
	/// A box owned by one element inside a chain: only rules naming that
	/// element, or a specialization of it, apply.
	Scoped(u128, crate::style::Condition, crate::style::ColorField),
	#[default]
	Text,
	Muted,
	Accent,
	Panel,
	Glass,
	Scrim,
	Shadow,
	Border,
	Background,
	Error,
}

impl Paint {
	pub fn cascade(
		self,
		condition: crate::style::Condition,
		field: crate::style::ColorField,
	) -> Self {
		let chain = match self {
			Self::Cascade(v, _) => v,
			Self::Styled(c, _) => c as u128 + 1,
			_ => crate::style::Condition::Body as u128 + 1,
		};
		Self::Cascade(crate::style::chain_push(chain, condition), field)
	}
}

#[derive(Clone, Debug)]
pub struct Glyph {
	pub font: FontData,
	pub coords: Arc<[i16]>,
	pub id: u16,
	pub size: f32,
	pub x: f32,
	pub y: f32,
	/// The face carries no italic, so the renderer shears the outline.
	pub synthetic_italic: bool,
	pub paint: Paint,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Rect {
	pub x: f32,
	pub y: f32,
	pub w: f32,
	pub h: f32,
}
impl Rect {
	pub fn intersect(self, other: Self) -> Option<Self> {
		let x = self.x.max(other.x);
		let y = self.y.max(other.y);
		let w = (self.x + self.w).min(other.x + other.w) - x;
		let h = (self.y + self.h).min(other.y + other.h) - y;
		(w > 0.0 && h > 0.0).then_some(Self { x, y, w, h })
	}
	pub fn contains(self, x: f32, y: f32) -> bool {
		x >= self.x
			&& x <= self.x + self.w
			&& y >= self.y
			&& y <= self.y + self.h
	}
}

#[derive(Clone, Debug)]
pub enum Draw {
	/// A group clipped to a local rectangle.
	Clipped {
		rect: Rect,
		draws: Vec<Draw>,
	},
	Image {
		src: String,
		version: u64,
		rect: Rect,
		title: String,
	},
	Glyph(Glyph),
	Rect(Rect, Paint),
	Box {
		rect: Rect,
		/// The condition chain the box was laid out in.
		chain: u128,
		/// The element the box belongs to; its rules own the box colors.
		condition: crate::style::Condition,
		radius: f32,
		border: f32,
		left_only: bool,
	},
	Math {
		math: Arc<MathBox>,
		paint: Paint,
		x: f32,
		y: f32,
	},
	/// A filled convex polygon, such as a list bullet. `points` are relative
	/// to `center`, so moving the shape never rebuilds them.
	Polygon {
		center: [f32; 2],
		points: Arc<[[f32; 2]]>,
		paint: Paint,
	},
}
impl Draw {
	pub fn translate(&mut self, x: f32, y: f32) {
		match self {
			Self::Clipped { rect, draws } => {
				rect.x += x;
				rect.y += y;
				for draw in draws {
					draw.translate(x, y);
				}
			}
			Self::Glyph(g) => {
				g.x += x;
				g.y += y;
			}
			Self::Rect(r, _)
			| Self::Box { rect: r, .. }
			| Self::Image { rect: r, .. } => {
				r.x += x;
				r.y += y;
			}
			Self::Math { x: gx, y: gy, .. } => {
				*gx += x;
				*gy += y;
			}
			Self::Polygon { center, .. } => {
				center[0] += x;
				center[1] += y;
			}
		}
	}
}

#[derive(Clone, Debug)]
pub struct Overflow {
	pub rect: Rect,
	pub content_width: f32,
	pub commands: Range<usize>,
	/// Space reserved below `rect` for this block's horizontal scrollbar, so
	/// the bar never crowds the last line of text.
	pub gutter: f32,
}

/// One clickable link fragment, in block-local coordinates.
#[derive(Clone, Debug)]
pub struct LinkRect {
	pub command: usize,
	pub rect: Rect,
	pub url: String,
}

/// A heading's anchor and the block-local y a link to it should scroll to.
#[derive(Clone, Debug, PartialEq)]
pub struct HeadingAnchor {
	pub anchor: String,
	pub y: f32,
}

#[derive(Debug, Default)]
pub struct BlockLayout {
	pub text: Vec<TextNode>,
	pub draws: Vec<Draw>,
	pub height: f32,
	pub width: f32,
	pub overflow: Vec<Overflow>,
	pub links: Vec<LinkRect>,
	/// Headings laid out inside this block, in reading order.
	pub anchors: Vec<HeadingAnchor>,
	pub degraded: usize,
	pub math_errors: usize,
}

#[derive(Clone, Debug)]
pub struct PlacedBlock {
	pub id: u64,
	pub source: Range<usize>,
	pub y: f32,
	pub layout: Arc<BlockLayout>,
}

#[derive(Clone, Debug, Default)]
pub struct LayoutSnapshot {
	pub images: crate::image::ImageSnapshot,
	pub document_box: Option<Draw>,
	pub blocks: Vec<PlacedBlock>,
	pub height: f32,
	pub width: f32,
	pub reused: usize,
	pub degraded: usize,
	pub math_errors: usize,
}

impl LayoutSnapshot {
	/// The document y a link to a heading anchor should scroll to, once that
	/// heading has been laid out. A prefix snapshot only answers for the
	/// headings it already contains.
	pub fn anchor_y(&self, anchor: &str) -> Option<f32> {
		self.blocks.iter().find_map(|block| {
			block
				.layout
				.anchors
				.iter()
				.find(|a| a.anchor == anchor)
				.map(|a| block.y + a.y)
		})
	}
	pub fn image_title_at(
		&self,
		x: f32,
		y: f32,
		horizontal: &HashMap<(usize, usize), f32>,
	) -> Option<&str> {
		for (bi, b) in self.blocks.iter().enumerate() {
			if y < b.y || y > b.y + b.layout.height {
				continue;
			}
			for (i, d) in b.layout.draws.iter().enumerate() {
				if let Draw::Image { rect, title, .. } = d {
					let (offset, clip) =
						b.layout.command_view(i, bi, horizontal);
					if !title.is_empty()
						&& clip.is_none_or(|r| r.contains(x, y - b.y))
						&& rect.contains(x + offset, y - b.y)
					{
						return Some(title);
					}
				}
			}
		}
		None
	}
	/// The link under a point in document coordinates: `x` from the column's
	/// left edge, `y` from the top of the document including the scroll offset.
	pub fn link_at(
		&self,
		x: f32,
		y: f32,
		horizontal: &HashMap<(usize, usize), f32>,
	) -> Option<&str> {
		for (bi, block) in self.blocks.iter().enumerate() {
			let y = y - block.y;
			if y < 0.0 || y > block.layout.height {
				continue;
			}
			for link in &block.layout.links {
				let (offset, clip) =
					block.layout.command_view(link.command, bi, horizontal);
				let mut rect = Rect {
					x: link.rect.x - offset,
					..link.rect
				};
				if let Some(clip) = clip {
					let Some(clipped) = rect.intersect(clip) else {
						continue;
					};
					rect = clipped;
				}
				if rect.contains(x, y) {
					return Some(&link.url);
				}
			}
		}
		None
	}
}

/// Default painted thickness of a scrollbar at rest, in logical pixels.
pub const SCROLLBAR_THICKNESS: f32 = 8.0;
/// Default painted thickness of a hovered or dragged thumb. The interactive
/// band is at least this wide, so the grab zone is what the reader sees.
pub const SCROLLBAR_HOVER_THICKNESS: f32 = 14.0;
/// Default space an overflowing block reserves below its content for its
/// horizontal scrollbar, keeping the bar clear of the last line of text.
pub const SCROLLBAR_GUTTER: f32 = 8.0;
/// Shortest thumb a very long document may shrink to, so the bar stays
/// grabbable.
pub const SCROLLBAR_MIN_THUMB: f32 = 24.0;

/// The thicknesses one scrollbar is painted at. A stylesheet may set both to
/// the same value, which disables the thickening on hover.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollbarMetrics {
	/// Painted thickness at rest.
	pub thickness: f32,
	/// Painted thickness of the thumb while hovered or dragged.
	pub thickness_hover: f32,
}

impl Default for ScrollbarMetrics {
	fn default() -> Self {
		Self::DOCUMENT
	}
}

impl ScrollbarMetrics {
	/// The reader's vertical bar: thin at rest, thick while held.
	pub const DOCUMENT: Self = Self {
		thickness: SCROLLBAR_THICKNESS,
		thickness_hover: SCROLLBAR_HOVER_THICKNESS,
	};
	/// A wide block's horizontal bar, which does not thicken by default.
	pub const OVERFLOW: Self = Self {
		thickness: SCROLLBAR_THICKNESS,
		thickness_hover: SCROLLBAR_THICKNESS,
	};
	/// The interactive band: the widest the bar is ever painted.
	pub fn band(&self) -> f32 {
		self.thickness.max(self.thickness_hover)
	}
	/// The band a wide block's bar occupies below its content: the reserved
	/// gutter, or at least the bar itself when no gutter is configured.
	pub fn overflow_band(&self, gutter: f32) -> f32 {
		gutter.max(self.band())
	}
}

/// A scrollbar track: where the thumb sits, what it maps to, and the two
/// thicknesses it is painted at. Both the reader's document bar and every wide
/// block's bar use this, so drawing and pointer handling cannot disagree.
#[derive(Clone, Copy, Debug)]
pub struct Scrollbar {
	/// The interactive track, spanning the full grab thickness.
	pub track: Rect,
	/// The thumb inside `track`, also spanning the full grab thickness.
	pub thumb: Rect,
	/// The bar runs down the window; otherwise it runs across the content.
	vertical: bool,
	/// Scroll offset at the end of the thumb's travel.
	max_scroll: f32,
	/// Thumb travel along the track.
	travel: f32,
	/// Thumb length along the track.
	thumb_len: f32,
	/// Thicknesses this bar is painted at.
	metrics: ScrollbarMetrics,
}

/// Thumb length and its offset along a track, or `None` when the content fits
/// and no scrollbar is shown.
fn thumb_span(
	length: f32,
	scroll: f32,
	content: f32,
	viewport: f32,
) -> Option<(f32, f32)> {
	if content <= viewport || length <= 0.0 {
		return None;
	}
	let thumb = (length * viewport / content)
		.clamp(SCROLLBAR_MIN_THUMB.min(length), length);
	let at = (scroll / (content - viewport)).clamp(0.0, 1.0) * (length - thumb);
	Some((thumb, at))
}

impl Scrollbar {
	/// A vertical bar in `track`, or `None` when nothing scrolls.
	pub fn vertical(
		track: Rect,
		scroll: f32,
		content: f32,
		viewport: f32,
		metrics: ScrollbarMetrics,
	) -> Option<Self> {
		let (thumb_len, at) = thumb_span(track.h, scroll, content, viewport)?;
		Some(Self {
			track,
			thumb: Rect {
				y: track.y + at,
				h: thumb_len,
				..track
			},
			vertical: true,
			max_scroll: content - viewport,
			travel: track.h - thumb_len,
			thumb_len,
			metrics,
		})
	}

	/// A horizontal bar in `track`, or `None` when nothing scrolls.
	pub fn horizontal(
		track: Rect,
		scroll: f32,
		content: f32,
		viewport: f32,
		metrics: ScrollbarMetrics,
	) -> Option<Self> {
		let (thumb_len, at) = thumb_span(track.w, scroll, content, viewport)?;
		Some(Self {
			track,
			thumb: Rect {
				x: track.x + at,
				w: thumb_len,
				..track
			},
			vertical: false,
			max_scroll: content - viewport,
			travel: track.w - thumb_len,
			thumb_len,
			metrics,
		})
	}

	/// The track and thumb to paint. The track stays at its rest thickness; a
	/// hovered or dragged thumb may thicken, up to the interactive band.
	pub fn bars(&self, expanded: bool) -> (Rect, Rect) {
		let thickness = if expanded {
			self.metrics.thickness_hover
		} else {
			self.metrics.thickness
		};
		(
			self.paint(self.track, self.metrics.thickness),
			self.paint(self.thumb, thickness),
		)
	}

	/// `rect` narrowed to `size` across the bar's cross axis, centred on the
	/// interactive band.
	fn paint(&self, rect: Rect, size: f32) -> Rect {
		if self.vertical {
			let size = size.clamp(0.0, rect.w);
			Rect {
				x: rect.x + (rect.w - size) * 0.5,
				w: size,
				..rect
			}
		} else {
			let size = size.clamp(0.0, rect.h);
			Rect {
				y: rect.y + (rect.h - size) * 0.5,
				h: size,
				..rect
			}
		}
	}

	/// True when the point is inside the interactive track.
	pub fn hit(&self, x: f32, y: f32) -> bool {
		self.track.contains(x, y)
	}

	/// True when the point is on the thumb rather than on the empty track.
	/// The whole grab thickness counts, so a press that looks like it landed
	/// on the bar starts a drag instead of jumping the scroll offset.
	pub fn on_thumb(&self, x: f32, y: f32) -> bool {
		if !self.track.contains(x, y) {
			return false;
		}
		let (position, start) = if self.vertical {
			(y, self.thumb.y)
		} else {
			(x, self.thumb.x)
		};
		position >= start && position <= start + self.thumb_len
	}

	/// The pointer's offset inside the thumb, kept constant during a drag.
	pub fn grab(&self, x: f32, y: f32) -> f32 {
		let (pointer, start, offset) = if self.vertical {
			(y, self.track.y, self.thumb.y - self.track.y)
		} else {
			(x, self.track.x, self.thumb.x - self.track.x)
		};
		(pointer - start - offset).clamp(0.0, self.thumb_len)
	}

	/// The scroll offset for a pointer at `(x, y)` holding the thumb at
	/// `grab`.
	pub fn scroll_for(&self, x: f32, y: f32, grab: f32) -> f32 {
		if self.travel <= 0.0 {
			return 0.0;
		}
		let (pointer, start) = if self.vertical {
			(y, self.track.y)
		} else {
			(x, self.track.x)
		};
		(pointer - grab - start).clamp(0.0, self.travel) / self.travel
			* self.max_scroll
	}
}

/// Logical window coordinates. DPI conversion happens once at the platform edge.
#[derive(Clone, Copy, Debug)]
pub struct Viewport {
	pub width: f32,
	pub height: f32,
	pub left: f32,
	pub top: f32,
	pub bottom: f32,
	pub scroll: f32,
}
impl Viewport {
	pub fn clip(self) -> Rect {
		Rect {
			x: 0.0,
			y: self.top,
			w: self.width,
			h: (self.height - self.top - self.bottom).max(0.0),
		}
	}
	pub fn document_point(self, x: f32, y: f32) -> (f32, f32) {
		(x - self.left, y - self.top + self.scroll)
	}
	pub fn window_rect(self, rect: Rect) -> Rect {
		Rect {
			x: rect.x + self.left,
			y: rect.y + self.top - self.scroll,
			..rect
		}
	}
}
impl BlockLayout {
	/// Shared overflow transform for painting, link hits and text selection.
	pub fn command_view(
		&self,
		command: usize,
		block: usize,
		horizontal: &HashMap<(usize, usize), f32>,
	) -> (f32, Option<Rect>) {
		self.overflow
			.iter()
			.enumerate()
			.find(|(_, o)| o.commands.contains(&command))
			.map(|(oi, o)| {
				(
					horizontal
						.get(&(block, oi))
						.copied()
						.unwrap_or(0.0)
						.clamp(0.0, (o.content_width - o.rect.w).max(0.0)),
					Some(o.rect),
				)
			})
			.unwrap_or((0.0, None))
	}
}

#[cfg(test)]
mod tests;
