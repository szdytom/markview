//! Immutable drawing and hit-test geometry shared by layout and rendering.
use crate::{
	math::MathBox,
	text::{TextCluster, TextNode},
};
use parley::FontData;
pub use ratex_types::PathCommand;
use std::{collections::HashMap, ops::Range, sync::Arc};
mod flow;
pub use flow::{FlowIndex, Scene};
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
	/// The renderer emboldens the outline of a non-bold face.
	pub synthetic_bold: bool,
	pub paint: Paint,
}

pub use markview_geometry::Rect;

/// One figure of a vector icon, in a unit box.
///
/// The box spans `0..=1` on both axes, with the origin at the top-left and `y`
/// growing downward like SVG. The drawn size is applied when the icon is
/// painted, so the same buffer serves every scale.
#[derive(Clone, Copy, Debug)]
pub struct IconPath {
	/// Flattened outlines, already transformed into the unit box.
	pub commands: &'static [PathCommand],
	/// Fill the outline instead of stroking it.
	pub fill: bool,
	/// Stroke width in unit-box units; ignored when `fill`.
	pub stroke_width: f32,
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
	/// A filled rectangle whose resolved color is scaled by `opacity`.
	FadedRect {
		rect: Rect,
		paint: Paint,
		opacity: f32,
	},
	Box {
		rect: Rect,
		/// The condition chain the box was laid out in.
		chain: u128,
		/// The element the box belongs to; its rules own the box colors.
		condition: crate::style::Condition,
		radius: f32,
		border: f32,
		left_only: bool,
		decoration: Option<BoxDecoration>,
	},
	Math {
		math: Arc<MathBox>,
		paint: Paint,
		x: f32,
		y: f32,
	},
	/// A filled polygon, such as a list bullet. `points` are relative to
	/// `center`, so moving the shape never rebuilds them.
	Polygon {
		center: [f32; 2],
		points: Arc<[[f32; 2]]>,
		paint: Paint,
	},
	/// A vector icon drawn into a square box whose top-left corner is `(x, y)`
	/// and whose side is `size`. Figures are stroked round-capped.
	Icon {
		paths: &'static [IconPath],
		paint: Paint,
		x: f32,
		y: f32,
		size: f32,
	},
}

/// Explicit box edges and corners, in logical pixels. Legacy boxes keep their fast path.
#[derive(Clone, Copy, Debug)]
pub struct BoxDecoration {
	pub edges: [f32; 4],
	pub corners: [f32; 4],
}

impl BoxDecoration {
	pub fn from_rule(
		rule: &crate::style::Rule,
		left_only: bool,
	) -> Option<Self> {
		(rule.border_edges.is_some() || rule.corner_radii.is_some()).then(
			|| Self {
				edges: rule.border_edges.unwrap_or_else(|| {
					let width = rule.border_width.unwrap_or(0.0);
					if left_only {
						[0.0, 0.0, 0.0, width]
					} else {
						[width; 4]
					}
				}),
				corners: rule
					.corner_radii
					.unwrap_or([rule.radius.unwrap_or(0.0); 4]),
			},
		)
	}
}

/// Scale all corners together so adjacent arcs cannot overlap.
pub fn fit_corners(rect: Rect, corners: [f32; 4]) -> [f32; 4] {
	let [tl, tr, br, bl] = corners;
	let factor = [
		(rect.w, tl + tr),
		(rect.w, bl + br),
		(rect.h, tl + bl),
		(rect.h, tr + br),
	]
	.into_iter()
	.filter(|(_, sum)| *sum > 0.0)
	.map(|(side, sum)| side / sum)
	.fold(1.0_f32, f32::min)
	.max(0.0);
	corners.map(|r| r * factor)
}

pub fn fit_edges(rect: Rect, mut edges: [f32; 4]) -> [f32; 4] {
	for (a, b, extent) in [(0, 2, rect.h), (1, 3, rect.w)] {
		let sum = edges[a] + edges[b];
		// A non-positive extent holds no border, so there is nothing to fit
		// and the `0 / denormal` scale must not run.
		if sum > extent && sum > 0.0 && extent > 0.0 {
			edges[a] *= extent / sum;
			edges[b] *= extent / sum;
		}
	}
	edges
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
			| Self::FadedRect { rect: r, .. }
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
			Self::Icon { x: ix, y: iy, .. } => {
				*ix += x;
				*iy += y;
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

/// The occurrence a link fragment belongs to. Fragments of one link share it,
/// while separate links with the same address keep different ids.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LinkId {
	/// The occurrence of a styled or footnote link, shared by every fragment
	/// the link produces across paragraph splits.
	Inline(markview_document::LinkOccurrenceId),
	/// A summary or backlink whose link is not part of the inline text.
	Special(usize),
}

/// What activating one link fragment does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkTarget {
	/// An address the application resolves, including fragments.
	Url(Arc<str>),
	/// A footnote definition, addressed by its semantic identity.
	FootnoteDefinition(markview_document::FootnoteId),
	/// A footnote definition's number returning to the reference it was opened
	/// from, or to its first reference.
	FootnoteReturn(markview_document::FootnoteId),
	/// A `<details>` summary toggling its own block.
	Disclosure(markview_document::BlockId),
}

impl LinkTarget {
	/// The legacy address form, used only at display and serialization
	/// boundaries. Activation uses the typed variant.
	pub fn display_url(&self) -> Arc<str> {
		match self {
			Self::Url(url) => url.clone(),
			Self::FootnoteDefinition(id) => {
				Arc::from(format!("#{}", crate::footnote::anchor(id.as_str())))
			}
			Self::FootnoteReturn(id) => {
				Arc::from(format!("#{}", crate::footnote::back(id.as_str())))
			}
			Self::Disclosure(id) => Arc::from(format!("details:{id}")),
		}
	}
}

/// One clickable link fragment, in block-local coordinates.
#[derive(Clone, Debug)]
pub struct LinkRect {
	/// The draw commands this fragment paints, in block-local order. Hover
	/// marks exactly this range for the matching occurrence.
	pub commands: Range<usize>,
	/// The occurrence shared by every fragment of this one link.
	pub id: LinkId,
	pub rect: Rect,
	/// Shared with the style the run came from, so one address costs one
	/// allocation however many line fragments point at it.
	pub url: Arc<str>,
	pub target: LinkTarget,
}

/// A link under a point: its address, owning block and occurrence id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkRef<'a> {
	pub url: &'a str,
	pub target: &'a LinkTarget,
	/// The semantic block identity, which stays stable across reflows and
	/// distinguishes blocks with identical text.
	pub block: markview_document::BlockId,
	pub id: LinkId,
}

/// A document anchor and its retained node-local y.
#[derive(Clone, Debug, PartialEq)]
pub struct HeadingAnchor {
	pub anchor: markview_document::AnchorId,
	pub y: f32,
}

/// Immutable node-local geometry, including hidden disclosure bodies.
/// Use `PlacedBlock` accessors for visible block coordinates.
#[derive(Clone, Debug, Default)]
pub struct BlockLayout {
	pub scene: Scene,
	pub page_constraints: Vec<PageConstraint>,
	pub text: Vec<TextNode>,
	pub draws: Vec<Draw>,
	/// Inline decoration draw indices and their owning text rows, in draw order.
	pub inline_decorations: Vec<(usize, Range<f32>)>,
	/// Height with every disclosure expanded; visible height is `PlacedBlock::height`.
	pub height: f32,
	pub width: f32,
	pub overflow: Vec<Overflow>,
	pub links: Vec<LinkRect>,
	/// Every inline link occurrence prepared in this block, in reading order,
	/// including runs that line breaking discarded. Cache reuse rebinds its
	/// ids from this list rather than from the painted fragments alone.
	pub link_occurrences: Vec<markview_document::LinkOccurrenceId>,
	/// Headings laid out inside this block, in reading order.
	pub anchors: Vec<HeadingAnchor>,
	pub degraded: usize,
	pub math_errors: usize,
}

/// Pagination hints retain local ranges so nested paragraphs and quotes keep their own rules.
#[derive(Clone, Copy, Debug)]
pub struct PageConstraint {
	pub top: f32,
	pub bottom: f32,
	pub orphans: Option<u16>,
	pub widows: Option<u16>,
	pub keep_together: bool,
}

#[derive(Clone, Debug)]
pub struct PlacedBlock {
	pub id: markview_document::BlockId,
	pub content: markview_document::ContentKey,
	pub y: f32,
	pub layout: Arc<BlockLayout>,
	/// Resolved visibility and positions, shared independently of geometry.
	pub flow: Arc<FlowIndex>,
}

#[derive(Clone, Debug, Default)]
pub struct LayoutSnapshot {
	/// Presentation identity, independent of the worker's geometry pass.
	pub presentation_key: u64,
	/// Semantic identity of the reading text; a changed document or edit
	/// changes it, while cached geometry keeps it stable across passes.
	pub content_id: markview_document::ContentKey,
	pub revision: markview_document::DocumentRevision,
	pub images: crate::image::ImageSnapshot,
	pub document_box: Option<Draw>,
	pub blocks: Vec<PlacedBlock>,
	pub height: f32,
	pub width: f32,
	/// Whether a single picture was centered in the viewport, leaving no
	/// scrollable extent below it.
	pub centered: bool,
	pub reused: usize,
	pub degraded: usize,
	pub math_errors: usize,
}

impl LayoutSnapshot {
	/// The furthest this snapshot scrolls in `viewport`. A centered picture
	/// fits by construction, so it never scrolls into the blank tail. If the
	/// reading area has since shrunk, the picture must scroll again.
	pub fn scroll_limit(&self, viewport: f32) -> f32 {
		if self.centered && self.height <= viewport {
			0.0
		} else {
			crate::layout::scroll_limit(self.height, viewport)
		}
	}
	/// The document y a typed anchor should scroll to, once it has been laid
	/// out. A prefix snapshot only answers for anchors it already contains.
	pub fn anchor_target_y(
		&self,
		anchor: &markview_document::AnchorId,
	) -> Option<f32> {
		self.blocks.iter().find_map(|block| {
			block
				.anchor_positions()
				.find(|(name, _)| *name == anchor)
				.map(|(_, y)| block.y + y)
		})
	}

	/// The document y an HTML fragment anchor should scroll to.
	pub fn anchor_y(&self, anchor: &str) -> Option<f32> {
		self.anchor_target_y(&markview_document::AnchorId::Fragment(
			anchor.to_owned(),
		))
	}
	pub fn image_title_at(
		&self,
		x: f32,
		y: f32,
		horizontal: &HashMap<(usize, usize), f32>,
	) -> Option<&str> {
		self.image_at(x, y, horizontal)
			.map(|(_, _, _, title)| title)
			.filter(|title| !title.is_empty())
	}
	/// The image under a point in document coordinates: its alias, the
	/// version the page drew it with, its rect and its title. The alias and
	/// version name the texture the viewer draws; the rect carries the aspect
	/// the layout chose for it.
	pub fn image_at(
		&self,
		x: f32,
		y: f32,
		horizontal: &HashMap<(usize, usize), f32>,
	) -> Option<(&str, u64, Rect, &str)> {
		for (bi, b) in self.blocks.iter().enumerate() {
			if y < b.y || y > b.y + b.height() {
				continue;
			}
			for (i, d, _) in b.draws() {
				if let Draw::Image {
					src,
					version,
					rect,
					title,
				} = d
				{
					let (offset, clip) = b.command_view(i, bi, horizontal);
					let rect = b.rect(i, *rect).unwrap();
					if clip.is_none_or(|r| r.contains(x, y - b.y))
						&& rect.contains(x + offset, y - b.y)
					{
						return Some((src, *version, rect, title));
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
		self.link_ref_at(x, y, horizontal).map(|link| link.url)
	}
	/// The whole link under a point, including the occurrence the renderer
	/// needs to highlight one link without its same-address neighbors.
	pub fn link_ref_at(
		&self,
		x: f32,
		y: f32,
		horizontal: &HashMap<(usize, usize), f32>,
	) -> Option<LinkRef<'_>> {
		for (bi, block) in self.blocks.iter().enumerate() {
			let y = y - block.y;
			if y < 0.0 || y > block.height() {
				continue;
			}
			for (link, mut rect) in block.links() {
				let (offset, clip) =
					block.command_view(link.commands.start, bi, horizontal);
				rect.x -= offset;
				if let Some(clip) = clip {
					let Some(clipped) = rect.intersect(clip) else {
						continue;
					};
					rect = clipped;
				}
				if rect.contains(x, y) {
					return Some(LinkRef {
						url: link.url.as_ref(),
						target: &link.target,
						block: block.id,
						id: link.id,
					});
				}
			}
		}
		None
	}
}

/// Default painted thickness of a scrollbar, in logical pixels.
pub const SCROLLBAR_THICKNESS: f32 = 8.0;
/// Default space an overflowing block reserves below its content for its
/// horizontal scrollbar, keeping the bar clear of the last line of text.
pub const SCROLLBAR_GUTTER: f32 = 8.0;
/// Shortest thumb a very long document may shrink to, so the bar stays
/// grabbable.
pub const SCROLLBAR_MIN_THUMB: f32 = 24.0;

/// The thickness one scrollbar is painted at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollbarMetrics {
	/// Painted thickness.
	pub thickness: f32,
}

impl Default for ScrollbarMetrics {
	fn default() -> Self {
		Self::DOCUMENT
	}
}

impl ScrollbarMetrics {
	/// The reader's vertical bar.
	pub const DOCUMENT: Self = Self {
		thickness: SCROLLBAR_THICKNESS,
	};
	/// A wide block's horizontal bar.
	pub const OVERFLOW: Self = Self {
		thickness: SCROLLBAR_THICKNESS,
	};
	/// The band a wide block's bar occupies below its content: the reserved
	/// gutter, or at least the bar itself when no gutter is configured.
	pub fn overflow_band(&self, gutter: f32) -> f32 {
		gutter.max(self.thickness)
	}
}

/// A scrollbar track: where the thumb sits, what it maps to, and the
/// thickness it is painted at. Both the reader's document bar and every wide
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
	/// Thickness this bar is painted at.
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

	/// The track and thumb to paint.
	pub fn bars(&self) -> (Rect, Rect) {
		(
			self.paint(self.track, self.metrics.thickness),
			self.paint(self.thumb, self.metrics.thickness),
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

	/// Scroll offset at the end of the thumb's travel.
	pub fn max_scroll(&self) -> f32 {
		self.max_scroll
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
	/// Whether the draw at `command` belongs to the link occurrence `id`.
	pub fn link_hovered(&self, command: usize, url: &str, id: LinkId) -> bool {
		self.links.iter().any(|link| {
			link.id == id
				&& &*link.url == url
				&& link.commands.contains(&command)
		})
	}

	/// Draw spans owned by each text cluster, with its node index and row geometry.
	pub fn text_draw_spans(
		&self,
	) -> impl Iterator<Item = (Range<usize>, usize, &TextCluster)> {
		let mut clusters: Vec<_> = self
			.text
			.iter()
			.enumerate()
			.flat_map(|(node, text)| {
				text.clusters.iter().map(move |c| (node, c))
			})
			.collect();
		clusters.sort_by_key(|(_, c)| c.command);
		let mut end = self.draws.len();
		clusters.into_iter().rev().map(move |(node, cluster)| {
			let span = cluster.command..end;
			end = cluster.command;
			(span, node, cluster)
		})
	}
}

#[cfg(test)]
mod tests;
