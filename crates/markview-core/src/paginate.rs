//! Page breaking for the PDF export.
//!
//! Layout produces one continuous column. Paper needs pages, so this module
//! collects each block's drawn lines into *bands* and distributes the bands
//! over fixed-height regions. The model follows Typst's flow layout: a band
//! records the space it needs together with the lines widow and orphan control
//! refuses to separate from it, and a region is only finished when that need
//! fits nowhere in the current page.
//!
//! Everything here is window- and GPU-independent, and every distance is a
//! layout pixel unless a name says `pt`.
mod furniture;
#[cfg(test)]
mod tests;
use crate::{
	document::{Block, BlockKind, Document, InlineKind},
	layout::{BlockLayout, Draw, LayoutSnapshot},
	style::PageStyle,
};
use std::collections::HashMap;
/// Millimetres to PDF points.
const MM_TO_PT: f32 = 72.0 / 25.4;
/// A layout pixel is a ninety-sixth of an inch; a PDF point a seventy-second.
pub const PT_PER_PX: f32 = 0.75;
/// A printed page never shrinks a block below this much.
const MIN_SCALE: f32 = 0.25;
/// Below this, a scaled block is reported as unreadable rather than merely
/// smaller than the measure it was laid out for.
const READABLE_SCALE: f32 = 0.6;

/// The physical page, in PDF points, with the margins already applied.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageGeometry {
	pub width_pt: f32,
	pub height_pt: f32,
	/// Top, right, bottom, left.
	pub margin_pt: [f32; 4],
}
impl PageGeometry {
	pub fn from_style(style: &PageStyle) -> anyhow::Result<Self> {
		let (width_mm, height_mm) = style
			.paper_mm()
			.ok_or_else(|| anyhow::anyhow!("page.size: unsupported paper"))?;
		let margin_mm =
			style.margin_mm().unwrap_or(PageStyle::DEFAULT_MARGIN_MM);
		let geometry = Self {
			width_pt: width_mm * MM_TO_PT,
			height_pt: height_mm * MM_TO_PT,
			margin_pt: margin_mm.map(|value| value * MM_TO_PT),
		};
		let [top, right, bottom, left] = geometry.margin_pt;
		if geometry.width_pt - left - right < 60.0
			|| geometry.height_pt - top - bottom < 60.0
		{
			anyhow::bail!("page.margin: leaves no room for text");
		}
		Ok(geometry)
	}

	/// The text area as `[left, top, width, height]`, in points.
	pub fn text_pt(&self) -> [f32; 4] {
		let [top, right, bottom, left] = self.margin_pt;
		[
			left,
			top,
			self.width_pt - left - right,
			self.height_pt - top - bottom,
		]
	}

	/// The text area's width and height in layout pixels.
	pub fn text_px(&self) -> (f32, f32) {
		let [_, _, width, height] = self.text_pt();
		(width / PT_PER_PX, height / PT_PER_PX)
	}

	pub fn px_to_pt(px: f32) -> f32 {
		px * PT_PER_PX
	}
}

/// One indivisible run of drawn lines inside a block, in block-local y.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Band {
	top: f32,
	bottom: f32,
}
impl Band {
	fn height(self) -> f32 {
		(self.bottom - self.top).max(0.0)
	}
	fn overlap(self, top: f32, bottom: f32) -> f32 {
		self.bottom.min(bottom) - self.top.max(top)
	}
}

/// One block, ready to be distributed over pages.
struct Prepared {
	bands: Vec<Band>,
	/// Space between the block's own top edge and its first line: its leading
	/// gap and its container's top padding.
	lead: f32,
	/// Space between the block's last line and its bottom edge: the container's
	/// bottom padding and the block's trailing gap.
	trail: f32,
	/// Per band, the height that has to fit for the band to be placed: its own
	/// lines plus whatever widow and orphan control groups with it.
	need: Vec<f32>,
	/// Uniform shrink factor for a block too wide or too tall for the page.
	scale: f32,
	/// A heading travels with the block that follows it.
	keep_with_next: bool,
}

/// A run of one block's bands placed on one page.
#[derive(Clone, Debug, PartialEq)]
pub struct PageItem {
	pub block: usize,
	/// The block's bands this item covers.
	pub bands: std::ops::Range<usize>,
	/// Uniform shrink factor the item is drawn with.
	pub scale: f32,
	/// Page-local y of the first covered band's top, in layout pixels.
	pub y: f32,
	/// The block-local y span this fragment covers.
	pub top: f32,
	pub bottom: f32,
	/// Whether the item starts, or ends, its block: only these fragments paint
	/// the container's horizontal edges and rounded corners.
	pub first: bool,
	pub last: bool,
}

/// The pages a document breaks into.
#[derive(Clone, Debug, Default)]
pub struct Pagination {
	pub pages: Vec<Vec<PageItem>>,
	/// Heading anchor to the page and page-local y (layout pixels) it names.
	pub anchors: HashMap<String, (usize, f32)>,
}

/// Adds one drawn line to the band list, joining the line it sits in or
/// starting a band of its own.
fn join(bands: &mut Vec<Band>, top: f32, bottom: f32) {
	if !(top.is_finite() && bottom.is_finite()) || bottom <= top {
		return;
	}
	// Bands stay sorted by `top`, so only the neighbors of the insertion point
	// can overlap this interval.
	let at = bands.partition_point(|band| band.top <= top);
	let from = at.saturating_sub(2);
	let to = (at + 2).min(bands.len());
	let best = (from..to)
		.filter(|index| bands[*index].overlap(top, bottom) > 0.0)
		.max_by(|a, b| {
			bands[*a]
				.overlap(top, bottom)
				.total_cmp(&bands[*b].overlap(top, bottom))
		});
	match best {
		Some(index) => {
			bands[index].top = bands[index].top.min(top);
			bands[index].bottom = bands[index].bottom.max(bottom);
		}
		None => bands.insert(at, Band { top, bottom }),
	}
}

/// The drawn lines of one block. Text clusters carry the line boxes, so the
/// bands follow the lines the paragraph optimizer chose; images, rules and
/// formulas join the line they overlap, or become a band of their own.
///
/// Container boxes are skipped: they span the whole block, and the painter
/// clips them to each page fragment instead.
fn bands(layout: &BlockLayout) -> Vec<Band> {
	let mut bands: Vec<Band> = Vec::new();
	let vertical = |draw: &Draw| -> Option<(f32, f32)> {
		match draw {
			Draw::Box { .. } | Draw::Clipped { .. } => None,
			// A label's glyphs have no text cluster; use the em box.
			Draw::Glyph(glyph) => {
				Some((glyph.y - glyph.size * 0.9, glyph.y + glyph.size * 0.25))
			}
			Draw::Rect(rect, _) => Some((rect.y, rect.y + rect.h)),
			Draw::Image { rect, .. } => Some((rect.y, rect.y + rect.h)),
			Draw::Math { math, y, .. } => {
				Some((*y, *y + math.ascent + math.descent))
			}
		}
	};
	for node in &layout.text {
		for cluster in &node.clusters {
			join(&mut bands, cluster.rect.y, cluster.rect.y + cluster.rect.h);
		}
	}
	for draw in &layout.draws {
		if let Some((top, bottom)) = vertical(draw) {
			join(&mut bands, top, bottom);
		}
	}
	bands.sort_by(|a, b| a.top.total_cmp(&b.top));
	if bands.is_empty() && layout.height > 0.5 {
		bands.push(Band {
			top: 0.0,
			bottom: layout.height,
		});
	}
	bands
}

/// The space each band needs together with the lines widow and orphan control
/// groups with it.
///
/// A two-band block never splits, a three-band block moves as a whole rather
/// than leaving one line behind, and a longer block always keeps two lines on
/// each side of a break.
fn needs(bands: &[Band]) -> Vec<f32> {
	let count = bands.len();
	(0..count)
		.map(|index| {
			let last = match (count, index) {
				(3, 0) => 2,
				(_, 0) if count > 1 => 1,
				(_, at) if at + 2 == count => count - 1,
				(_, at) => at,
			};
			bands[last].bottom - bands[index].top
		})
		.collect()
}

/// The shrink factor of a wide block: a table wider than the measure is scaled
/// until it fits, so no cell is silently cut off.
fn table_scale(block: Option<&Block>, layout: &BlockLayout) -> f32 {
	let width = block
		.map(|block| matches!(block.kind, BlockKind::Table { .. }))
		.unwrap_or(false);
	if !width {
		return 1.0;
	}
	layout
		.overflow
		.iter()
		.filter(|overflow| overflow.content_width > overflow.rect.w)
		.map(|overflow| overflow.rect.w / overflow.content_width)
		.fold(1.0_f32, f32::min)
}

/// Whether a paragraph is nothing but one image, which is then centred and
/// scaled to the page rather than cut.
fn only_image(block: Option<&Block>) -> bool {
	block
		.map(|block| match &block.kind {
			BlockKind::Paragraph(rich) => {
				let mut images = 0;
				for inline in rich {
					match &inline.kind {
						InlineKind::Image(_) => images += 1,
						InlineKind::Text(text) if text.trim().is_empty() => {}
						InlineKind::LineBreak { .. } => {}
						_ => return false,
					}
				}
				images == 1
			}
			_ => false,
		})
		.unwrap_or(false)
}

fn prepare(
	block: Option<&Block>,
	layout: &BlockLayout,
	content: (f32, f32),
) -> Prepared {
	let bands = bands(layout);
	let need = needs(&bands);
	let mut scale = table_scale(block, layout);
	// A lone image taller than the page is scaled down instead of losing its
	// bottom half. Its caption, if any, follows at the same scale.
	if only_image(block)
		&& bands.first().is_some_and(|band| band.bottom > content.1)
	{
		scale = scale.min(content.1 / bands[0].bottom);
	}
	if scale < MIN_SCALE {
		scale = MIN_SCALE;
	}
	if scale < 1.0 {
		let at = block.map(|block| block.source.start).unwrap_or_default();
		log::warn!(
			"Page: block at byte {at} scaled to {:.0}% to fit the page",
			scale * 100.0
		);
		if scale < READABLE_SCALE {
			log::warn!(
				"Page: block at byte {at} is below the readable size after scaling; \
				 split or restyle the source to print it"
			);
		}
	}
	let lead = bands.first().map(|band| band.top).unwrap_or(0.0);
	let trail = bands
		.last()
		.map(|band| (layout.height - band.bottom).max(0.0))
		.unwrap_or(0.0);
	Prepared {
		bands,
		lead,
		trail,
		need,
		scale,
		keep_with_next: block
			.map(|block| matches!(block.kind, BlockKind::Heading { .. }))
			.unwrap_or(false),
	}
}

/// Breaks a laid-out document into pages.
pub fn paginate(
	document: &Document,
	snapshot: &LayoutSnapshot,
	geometry: &PageGeometry,
) -> Pagination {
	let content = geometry.text_px();
	let prepared: Vec<Prepared> = snapshot
		.blocks
		.iter()
		.enumerate()
		.map(|(index, placed)| {
			prepare(document.blocks.get(index), &placed.layout, content)
		})
		.collect();
	let mut pages: Vec<Vec<PageItem>> = Vec::new();
	let mut current: Vec<PageItem> = Vec::new();
	let mut used = 0.0_f32;
	for block in 0..prepared.len() {
		let block_prepared = &prepared[block];
		let mut band = 0;
		while band < block_prepared.bands.len() {
			// A block owns its whole vertical extent: from its top edge, across
			// its leading gap, its lines and its trailing gap. A fragment that
			// opens a page therefore starts at the block's top edge, while a
			// continuation starts at its first line: nothing is painted above
			// that line on this page.
			let lead = if band == 0 {
				block_prepared.lead * block_prepared.scale
			} else {
				0.0
			};
			let trail = if band + 1 == block_prepared.bands.len() {
				block_prepared.trail * block_prepared.scale
			} else {
				0.0
			};
			let y = current
				.last()
				.filter(|item| item.block == block && item.bands.end == band)
				.map(|item| {
					item.y
						+ (block_prepared.bands[band].top
							- block_prepared.bands[item.bands.start].top)
							* item.scale
				})
				.unwrap_or(used + lead);
			let height =
				block_prepared.bands[band].height() * block_prepared.scale;
			// The trailing gap is reserved for what follows, not required for
			// the line itself: counting it here would widow the last line.
			let needed = block_prepared.need[band] * block_prepared.scale;
			// A heading and the block it introduces travel together, so a
			// section never opens with a heading alone at the foot of a page.
			// Reserve only the heading's remaining extent, including its gaps;
			// counting already placed lines again can split a fitting heading.
			let followed = if block_prepared.keep_with_next {
				let remaining = block_prepared.bands.last().unwrap().bottom
					- block_prepared.bands[band].top
					+ block_prepared.trail;
				remaining * block_prepared.scale
					+ prepared.get(block + 1).map_or(0.0, |next| {
						(next.lead + next.need.first().copied().unwrap_or(0.0))
							* next.scale
					})
			} else {
				0.0
			};
			let overflows = y + needed.max(followed) > content.1 + 0.5;
			if overflows && !current.is_empty() {
				pages.push(std::mem::take(&mut current));
				used = 0.0;
				continue;
			}
			if y + height > content.1 + 0.5 {
				log::warn!(
					"Page: a band taller than the text area is cut at the page edge"
				);
			}
			used = used.max(y + height + trail);
			match current.last_mut() {
				Some(item) if item.block == block && item.bands.end == band => {
					item.bands.end = band + 1;
					item.bottom = block_prepared.bands[band].bottom;
				}
				_ => {
					let first = block_prepared.bands[band].top;
					let last = block_prepared.bands[band].bottom;
					current.push(PageItem {
						block,
						bands: band..band + 1,
						scale: block_prepared.scale,
						y,
						top: first,
						bottom: last,
						first: false,
						last: false,
					});
				}
			}
			band += 1;
		}
	}
	if !current.is_empty() {
		pages.push(current);
	}
	if pages.is_empty() {
		pages.push(Vec::new());
	}
	// A fragment knows whether it opens or closes its block, which is what
	// decides where a container paints its edges.
	for page in &mut pages {
		for item in page {
			let count = prepared[item.block].bands.len();
			item.first = item.bands.start == 0;
			item.last = item.bands.end == count;
		}
	}
	let anchors = collect_anchors(snapshot, &prepared, &pages);
	Pagination { pages, anchors }
}

/// Maps every heading anchor to the page and page-local y a link reaches.
fn collect_anchors(
	snapshot: &LayoutSnapshot,
	prepared: &[Prepared],
	pages: &[Vec<PageItem>],
) -> HashMap<String, (usize, f32)> {
	let mut out = HashMap::new();
	for (index, placed) in snapshot.blocks.iter().enumerate() {
		for anchor in &placed.layout.anchors {
			// An anchor sits at its container's top edge, which can be above
			// the first drawn line when the heading's own padding is above it;
			// such an anchor belongs to the band that follows it.
			let band = prepared[index]
				.bands
				.iter()
				.position(|band| band.bottom > anchor.y)
				.unwrap_or_else(|| {
					prepared[index].bands.len().saturating_sub(1)
				});
			if prepared[index].bands.is_empty() {
				continue;
			}
			for (page, items) in pages.iter().enumerate() {
				let Some(item) = items.iter().find(|item| {
					item.block == index && item.bands.contains(&band)
				}) else {
					continue;
				};
				let y = item.y
					+ (prepared[index].bands[band].top
						- prepared[index].bands[item.bands.start].top)
						* item.scale;
				out.insert(anchor.anchor.clone(), (page, y));
				break;
			}
		}
	}
	out
}

/// Re-exported so the painter and the stylesheet validator agree on what a
/// page-furniture slot may say.
pub use furniture::{
	Furniture, FurnitureText, SlotSegment, expand_template, page_furniture,
	template_is_valid,
};
