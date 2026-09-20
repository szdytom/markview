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
	document::{Block, BlockKind, Document},
	math::MathEngine,
	style::Condition,
};
pub use anchor::anchored_scroll;
use mapping::{Prepared, expand_tabs_mapped};
use std::{collections::HashMap, ops::Range, sync::Arc};

/// The share of a viewport a document may lift its last line by, so the end of
/// the text never sits flush against the bottom edge. Every limit on the scroll
/// offset goes through [`scroll_limit`], which applies it, so no path can
/// refuse to scroll into the blank it reserves.
const SCROLL_TAIL: f32 = 1.0 / 3.0;

/// The furthest a document of `height` scrolls in `viewport`: its last line can
/// be lifted to [`SCROLL_TAIL`] of a page below the top, leaving the rest blank,
/// and a document that already ends higher does not scroll.
pub fn scroll_limit(height: f32, viewport: f32) -> f32 {
	(height - viewport * SCROLL_TAIL).max(0.0)
}
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

/// Everything outside a block's own content that its geometry depends on:
/// which images are decoded, and which of its code blocks already carry syntax
/// colors. Both arrive asynchronously, so a change to either must invalidate
/// exactly the blocks that use it rather than the whole layout cache.
fn external_key(
	block: &Block,
	images: &crate::image::ImageSnapshot,
	highlights: &highlights::Highlights,
	theme: Option<&str>,
	options: &LayoutOptions,
) -> u64 {
	let mut specs = Vec::new();
	block.images(&mut specs);
	let mut code = Vec::new();
	block.code_blocks(&mut code);
	// A `<details>` body is part of its container's geometry, so the resolved
	// state of every disclosure in the subtree is an external input: toggling
	// a nested element must invalidate each ancestor that frames it.
	let mut disclosures = Vec::new();
	disclosure_states(block, options, &mut disclosures);
	crate::document::fingerprint(&(
		specs
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
		code.iter()
			.map(|(language, text)| {
				let key = highlights::key(language, text, theme);
				(key, highlights.results().contains_key(&key))
			})
			.collect::<Vec<_>>(),
		disclosures,
	))
}

/// Appends the identity and resolved collapse state of every `<details>` in
/// `block`'s subtree, in reading order. The tree is enough; no laid-out child
/// is consulted, so the key is available before the block is measured. The
/// identity is part of the key because the placed geometry binds each
/// summary's hit URL to its own block id: two distinct elements must never
/// share geometry whose links point at one of them.
fn disclosure_states(
	block: &Block,
	options: &LayoutOptions,
	out: &mut Vec<(u64, bool)>,
) {
	match &block.kind {
		BlockKind::Details { open, blocks, .. } => {
			out.push((block.id, options.details_expanded(block.id, *open)));
			for block in blocks {
				disclosure_states(block, options, out);
			}
		}
		BlockKind::Quote { blocks, .. }
		| BlockKind::Footnote { blocks, .. } => {
			for block in blocks {
				disclosure_states(block, options, out);
			}
		}
		BlockKind::List { items, .. } => {
			for item in items {
				for block in &item.blocks {
					disclosure_states(block, options, out);
				}
			}
		}
		_ => {}
	}
}

#[derive(Clone, Debug)]
pub struct LayoutOptions {
	pub width: f32,
	pub font_size: f32,
	pub justify: bool,
	pub hyphenate: bool,
	/// How far word spacing and letter spacing may move while justifying.
	pub justification: crate::JustificationLimits,
	/// Indent in multiples of the text size: the opening line of prose
	/// paragraphs, and the whole of a list, markers included. Zero disables it.
	pub paragraph_indent: f32,
	pub greedy: bool,
	pub codeblock_theme_override: Option<String>,
	/// Hard-wrap code block lines at the reading column instead of scrolling.
	pub codeblock_wrap: bool,
	/// Reader-chosen collapse state of each `<details>`, keyed by the block's
	/// semantic id. A block absent from the map uses the state its source
	/// declared, so a fresh document starts there.
	pub details_open: Arc<std::collections::BTreeMap<u64, bool>>,
	/// Render every `<details>` expanded regardless of the map. Exports set
	/// this: a printed page has no pointer to open a collapsed body with.
	pub force_open: bool,
	pub stylesheet: Arc<crate::style::Stylesheet>,
	/// Which faces the shaper may use. The default is the host's own fonts.
	pub fonts: crate::fonts::FontConfig,
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
			justification: crate::JustificationLimits::default(),
			paragraph_indent: 0.0,
			greedy: false,
			codeblock_theme_override: None,
			codeblock_wrap: false,
			details_open: Arc::default(),
			force_open: false,
			stylesheet: crate::style::Stylesheet::bundled(false),
			fonts: crate::fonts::FontConfig::default(),
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
			&& self.justification == other.justification
			&& self.paragraph_indent == other.paragraph_indent
			&& self.greedy == other.greedy
			&& self.codeblock_theme_override == other.codeblock_theme_override
			&& self.codeblock_wrap == other.codeblock_wrap
			&& self.details_open == other.details_open
			&& self.force_open == other.force_open
			&& self.stylesheet.layout_key() == other.stylesheet.layout_key()
			// A diagram theme is pixels, not geometry, but a new one must
			// reach the image scheduler, which recognizes work by these
			// options. Its key follows the font definitions the table names,
			// so changing one of those is a new request too.
			&& self.stylesheet.diagram_key() == other.stylesheet.diagram_key()
			&& self.fonts == other.fonts
			&& self.limits == other.limits
	}
}

impl LayoutOptions {
	/// The typographic choices the microtype passes work from. The CJK
	/// convention comes from the stylesheet, which is also what selects the
	/// `[cjk]` font definition, so the two can never disagree.
	pub(crate) fn typography(&self) -> crate::microtype::Typography {
		crate::microtype::Typography {
			limits: self.justification,
			cjk: self.stylesheet.cjk_type(),
		}
	}

	/// The indent in logical pixels for content set at `size`, capped so a
	/// character still fits in `width`.
	pub(crate) fn indent(&self, size: f32, width: f32) -> f32 {
		(self.paragraph_indent.max(0.0) * size).min((width - size).max(0.0))
	}

	/// Whether one `<details>` block shows its body: the reader's choice when
	/// they made one, the source declaration otherwise, and always when an
	/// export forces every block open.
	pub fn details_expanded(&self, id: u64, declared: bool) -> bool {
		self.force_open
			|| self.details_open.get(&id).copied().unwrap_or(declared)
	}
}

struct BlockContext<'a> {
	shaper: &'a mut TextShaper,
	math: &'a mut MathEngine,
	images: &'a crate::image::ImageSnapshot,
	highlight_cache: &'a HashMap<u64, highlights::HighlightResult>,
	/// How many unordered lists enclose the block being laid out. Ordered
	/// levels do not count, so they never advance a marker's shape cycle.
	marker_depth: usize,
	/// How many ordered lists enclose the block being laid out. A numbering
	/// pattern gives each level its own counting symbol.
	enum_depth: usize,
}
pub struct LayoutEngine {
	shaper: TextShaper,
	math: MathEngine,
	cache: HashMap<CacheKey, CacheEntry>,
	/// Increases once per pass; an entry's stamp says which pass last used it.
	pass: u64,
	highlights: highlights::Highlights,
}

/// A cached block geometry. The `Arc` is shared with the snapshot that
/// published it, so retaining every entry costs one pointer, not a copy.
struct CacheEntry {
	layout: Arc<BlockLayout>,
	pass: u64,
}

#[derive(Hash, PartialEq, Eq)]
struct CacheKey {
	position: u8,
	external: u64,
	content: u64,
	width: u32,
	size: u32,
	justify: bool,
	hyphenate: bool,
	justification: [u32; 4],
	paragraph_indent: u32,
	greedy: bool,
	codeblock_wrap: bool,
	/// Fingerprint of the resolved syntax theme, which is the same for every
	/// block of a pass.
	codeblock_theme: u64,
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
			pass: 0,
			highlights: highlights::Highlights::new(),
		}
	}
	pub fn clear_document_cache(&mut self) {
		self.cache.clear();
	}
	/// Drops geometry and syntax colors for a document that is no longer open,
	/// so an idle reader keeps nothing from it.
	pub fn release_document(&mut self) {
		self.cache.clear();
		self.highlights.clear();
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
		self.shaper.set_fonts(&options.fonts);
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
		let codeblock_theme_key =
			crate::document::fingerprint(&codeblock_theme);
		result.height =
			padding[0] + body.space_before.unwrap_or(0.) * options.font_size;
		// The stylesheet is immutable for this pass. Its identity belongs to
		// the document request, not to each block's cache lookup. The fonts
		// are part of it: geometry measured with other faces is stale.
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
		self.pass = self.pass.wrapping_add(1);
		let pass = self.pass;
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
			decoration: crate::scene::BoxDecoration::from_rule(body, false),
		});
		let body_appearance = self.shaper.appearance.clone();
		for (index, block) in document.blocks.iter().enumerate() {
			if let Some(Draw::Box { rect, .. }) = &mut result.document_box {
				rect.h = result.height;
			}
			if !progress(&result) {
				return None;
			}
			let key = CacheKey {
				position: if options.stylesheet.has_child_rules() {
					u8::from(index == 0)
						| (u8::from(index + 1 == document.blocks.len()) << 1)
				} else {
					0
				},
				external: external_key(
					block,
					images,
					&self.highlights,
					codeblock_theme.as_deref(),
					options,
				),
				content: block.content_key,
				width: options.width.to_bits(),
				size: options.font_size.to_bits(),
				justify: options.justify,
				hyphenate: options.hyphenate,
				justification: options.justification.bits(),
				paragraph_indent: options.paragraph_indent.to_bits(),
				greedy: options.greedy,
				codeblock_wrap: options.codeblock_wrap,
				codeblock_theme: codeblock_theme_key,
				style: style_key,
			};
			let cached = self.cache.get_mut(&key).map(|entry| {
				entry.pass = pass;
				entry.layout.clone()
			});
			let layout = if let Some(cached) = cached {
				result.reused += 1;
				cached
			} else {
				let layout = crate::profile::measure(
					crate::profile::Stage::Blocks,
					|| {
						let mut out = BlockLayout::default();
						self.shaper.appearance = options.stylesheet.child(
							&body_appearance,
							index,
							document.blocks.len(),
						);
						BlockContext {
							shaper: &mut self.shaper,
							math: &mut self.math,
							images,
							highlight_cache: self.highlights.results(),
							marker_depth: 0,
							enum_depth: 0,
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
					},
				);
				self.cache.insert(
					key,
					CacheEntry {
						layout: layout.clone(),
						pass,
					},
				);
				layout
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
		}
		// A complete pass visited every block, so an entry it did not touch
		// belongs to a superseded document, option set, or highlight state.
		// Dropping those keeps the cache at one document's worth of geometry.
		self.cache.retain(|_, entry| entry.pass == pass);
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
			decoration: crate::scene::BoxDecoration::from_rule(body, false),
		});
		Some(result)
	}

	/// Reports whether syntax colors arrived since the last pass. Arrived
	/// colors change the affected blocks' `external` key, so the cache
	/// invalidates them on the next lookup and leaves the rest alone.
	pub fn poll_highlights(&mut self) -> bool {
		self.highlights.poll()
	}

	/// Waits for the background syntax highlighting, then reports whether it
	/// changed the pass.
	///
	/// An export has no event loop to lay out again when a job reports, so it
	/// settles the pass before drawing; the reader keeps polling instead.
	pub fn wait_highlights(&mut self) -> bool {
		self.highlights.settle()
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
