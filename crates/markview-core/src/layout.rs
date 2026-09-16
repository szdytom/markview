//! Document layout and immutable snapshots, independent of a window or GPU.
mod anchor;
mod blocks;
mod code;
mod highlights;
mod images;
mod inline;
mod mapping;
mod paragraph;
#[cfg(test)]
mod stylesheet_tests;
mod table;
#[cfg(test)]
mod tests;
pub use crate::scene::{
	BlockLayout, Draw, Glyph, HeadingAnchor, LayoutSnapshot, LinkRect,
	Overflow, Paint, PlacedBlock, Rect, SCROLLBAR_GUTTER,
	SCROLLBAR_HOVER_THICKNESS, SCROLLBAR_MIN_THUMB, SCROLLBAR_THICKNESS,
	Scrollbar, ScrollbarMetrics, Viewport,
};
pub use crate::shaping::TextShaper;
use crate::{
	document::{Block, Document},
	math::MathEngine,
	style::Condition,
};
pub use anchor::anchored_scroll;
use mapping::{Prepared, expand_tabs_mapped};
use std::{collections::HashMap, ops::Range, sync::Arc};
fn fitted_range(full: &str, shown: &str, range: Range<usize>) -> Range<usize> {
	let (prefix, full_end, shown_end) = crate::text::changed_span(full, shown);
	let start = if range.start <= prefix {
		range.start
	} else if range.start >= shown_end {
		range.start - shown_end + full_end
	} else {
		prefix
	};
	let end = if range.end <= prefix {
		range.end
	} else if range.end >= shown_end {
		range.end - shown_end + full_end
	} else {
		full_end
	};
	start..end
}

fn image_key(block: &Block, images: &crate::image::ImageSnapshot) -> u64 {
	let mut specs = Vec::new();
	block.images(&mut specs);
	crate::document::fingerprint(
		&specs
			.iter()
			.map(|s| {
				(
					&s.src,
					images
						.entries
						.get(&s.src)
						.map(|i| (i.version, i.size, &i.error)),
				)
			})
			.collect::<Vec<_>>(),
	)
}

#[derive(Clone, Debug)]
pub struct LayoutOptions {
	pub width: f32,
	pub font_size: f32,
	pub justify: bool,
	pub hyphenate: bool,
	/// Indent in multiples of the text size: the opening line of prose
	/// paragraphs, and the whole of a list, markers included. Zero disables it.
	pub paragraph_indent: f32,
	pub greedy: bool,
	pub codeblock_theme_override: Option<String>,
	pub stylesheet: Arc<crate::style::Stylesheet>,
	/// Depth and work budgets; see [`crate::limits::Limits`].
	pub limits: crate::limits::Limits,
}
impl Default for LayoutOptions {
	fn default() -> Self {
		Self {
			width: 760.0,
			font_size: 18.0,
			justify: true,
			hyphenate: true,
			paragraph_indent: 0.0,
			greedy: false,
			codeblock_theme_override: None,
			stylesheet: crate::style::Stylesheet::bundled(false),
			limits: crate::limits::Limits::default(),
		}
	}
}

impl PartialEq for LayoutOptions {
	fn eq(&self, other: &Self) -> bool {
		self.width == other.width
			&& self.font_size == other.font_size
			&& self.justify == other.justify
			&& self.hyphenate == other.hyphenate
			&& self.paragraph_indent == other.paragraph_indent
			&& self.greedy == other.greedy
			&& self.codeblock_theme_override == other.codeblock_theme_override
			&& self.stylesheet.layout_key() == other.stylesheet.layout_key()
			&& self.limits == other.limits
	}
}

impl LayoutOptions {
	/// The indent in logical pixels for content set at `size`, capped so a
	/// character still fits in `width`.
	pub(crate) fn indent(&self, size: f32, width: f32) -> f32 {
		(self.paragraph_indent.max(0.0) * size).min((width - size).max(0.0))
	}
}

struct BlockContext<'a> {
	shaper: &'a mut TextShaper,
	math: &'a mut MathEngine,
	images: &'a crate::image::ImageSnapshot,
	highlight_cache: &'a HashMap<u64, highlights::HighlightResult>,
}
pub struct LayoutEngine {
	shaper: TextShaper,
	math: MathEngine,
	cache: HashMap<CacheKey, Arc<BlockLayout>>,
	highlights: highlights::Highlights,
}

#[derive(Hash, PartialEq, Eq)]
struct CacheKey {
	images: u64,
	content: u64,
	width: u32,
	size: u32,
	justify: bool,
	hyphenate: bool,
	paragraph_indent: u32,
	greedy: bool,
	codeblock_theme_override: Option<String>,
	codeblock_theme: Option<String>,
	highlight_generation: u64,
	style: u64,
}
impl Default for LayoutEngine {
	fn default() -> Self {
		Self::new()
	}
}
impl LayoutEngine {
	pub fn new() -> Self {
		Self {
			shaper: TextShaper::new(),
			math: MathEngine::default(),
			cache: HashMap::new(),
			highlights: highlights::Highlights::new(),
		}
	}
	pub fn clear_document_cache(&mut self) {
		self.cache.clear();
	}
	pub fn validate_stylesheet(
		&mut self,
		stylesheet: &crate::style::Stylesheet,
	) -> anyhow::Result<()> {
		self.shaper.validate_stylesheet(stylesheet)
	}

	pub fn layout(
		&mut self,
		document: &Document,
		options: &LayoutOptions,
	) -> LayoutSnapshot {
		self.layout_with_images(document, options, &Default::default())
	}

	pub fn layout_with_images(
		&mut self,
		document: &Document,
		options: &LayoutOptions,
		images: &crate::image::ImageSnapshot,
	) -> LayoutSnapshot {
		self.layout_progressive(document, options, images, |_| true)
			.expect("uninterrupted layout")
	}

	/// Visits each completed prefix. Returning false cancels at a block boundary.
	/// Prefixes share immutable block geometry with the final snapshot.
	pub fn layout_progressive(
		&mut self,
		document: &Document,
		options: &LayoutOptions,
		images: &crate::image::ImageSnapshot,
		mut progress: impl FnMut(&LayoutSnapshot) -> bool,
	) -> Option<LayoutSnapshot> {
		self.shaper.set_stylesheet(options.stylesheet.clone());
		self.math.set_limits(options.limits);
		self.poll_highlights();
		let mut result = LayoutSnapshot {
			images: images.clone(),
			width: options.width,
			..Default::default()
		};
		let body = options.stylesheet.rule(Condition::Body);
		let padding = body
			.padding
			.as_ref()
			.map(|p| p.sides().map(|v| v * options.font_size))
			.unwrap_or([0.; 4]);
		let content_width = (options.width - padding[1] - padding[3]).max(1.);
		crate::profile::span(crate::profile::Stage::Highlights, || {
			self.highlights.prepare(&document.blocks, options)
		});
		let codeblock_theme =
			options.codeblock_theme_override.clone().or_else(|| {
				options.stylesheet.rule(Condition::CodeBlock).theme.clone()
			});
		result.height =
			padding[0] + body.space_before.unwrap_or(0.) * options.font_size;
		// The stylesheet is immutable for this pass. Its identity belongs to
		// the document request, not to each block's cache lookup.
		let style_key = document
			.blocks
			.first()
			.map(|_| options.stylesheet.layout_key())
			.unwrap_or_default();
		let previous = std::mem::take(&mut self.cache);
		result.document_box = Some(Draw::Box {
			rect: Rect {
				x: 0.,
				y: 0.,
				w: options.width,
				h: result.height,
			},
			chain: Condition::Body.chain(),
			condition: Condition::Body,
			radius: body.radius.unwrap_or(0.),
			border: body.border_width.unwrap_or(0.),
			left_only: false,
		});
		let mut cached_draws = 0;
		for block in &document.blocks {
			if let Some(Draw::Box { rect, .. }) = &mut result.document_box {
				rect.h = result.height;
			}
			if !progress(&result) {
				return None;
			}
			let key = CacheKey {
				images: image_key(block, images),
				content: block.content_key,
				width: options.width.to_bits(),
				size: options.font_size.to_bits(),
				justify: options.justify,
				hyphenate: options.hyphenate,
				paragraph_indent: options.paragraph_indent.to_bits(),
				greedy: options.greedy,
				codeblock_theme_override: options
					.codeblock_theme_override
					.clone(),
				codeblock_theme: codeblock_theme.clone(),
				highlight_generation: self.highlights.generation(),
				style: style_key,
			};
			let layout = if let Some(cached) = previous.get(&key) {
				result.reused += 1;
				cached.clone()
			} else {
				crate::profile::measure(crate::profile::Stage::Blocks, || {
					let mut out = BlockLayout::default();
					BlockContext {
						shaper: &mut self.shaper,
						math: &mut self.math,
						images,
						highlight_cache: self.highlights.results(),
					}
					.block(
						block,
						padding[3],
						0.0,
						content_width,
						options,
						&mut out,
					);
					Arc::new(out)
				})
			};
			result.blocks.push(PlacedBlock {
				id: block.id,
				source: block.source.clone(),
				y: result.height,
				layout: layout.clone(),
			});
			result.height += layout.height;
			result.degraded += layout.degraded;
			result.math_errors += layout.math_errors;
			cached_draws += layout.draws.len();
			if cached_draws < 100_000 && self.cache.len() < 256 {
				self.cache.insert(key, layout);
			}
		}
		result.height +=
			padding[2] + body.space_after.unwrap_or(0.) * options.font_size;
		result.document_box = Some(Draw::Box {
			rect: Rect {
				x: 0.,
				y: 0.,
				w: options.width,
				h: result.height,
			},
			chain: Condition::Body.chain(),
			condition: Condition::Body,
			radius: body.radius.unwrap_or(0.),
			border: body.border_width.unwrap_or(0.),
			left_only: false,
		});
		Some(result)
	}

	pub fn poll_highlights(&mut self) -> bool {
		let changed = self.highlights.poll();
		if changed {
			self.cache.clear();
		}
		changed
	}
	pub fn label(
		&mut self,
		text: &str,
		size: f32,
		x: f32,
		y: f32,
		paint: Paint,
	) -> Vec<Draw> {
		self.shaper.label(text, size, x, y, paint)
	}
	pub fn fit(&mut self, text: &str, size: f32, max: f32) -> String {
		self.shaper.fit(text, size, max)
	}
	pub fn text_width(&mut self, text: &str, size: f32) -> f32 {
		self.shaper.text_width(text, size)
	}
}
