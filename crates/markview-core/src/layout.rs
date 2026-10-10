//! Document layout and immutable snapshots, independent of a window or GPU.
mod anchor;
mod blocks;
mod cache;
pub(crate) mod code;
mod highlights;
mod images;
mod inline;
mod mapping;
mod paragraph;
mod progressive;
#[cfg(test)]
mod stylesheet_tests;
mod table;
pub use progressive::ProgressiveLayout;
pub(crate) use table::Table;
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
};
pub use anchor::anchored_scroll;
use mapping::{Prepared, expand_tabs_mapped};
use std::{collections::HashMap, ops::Range, sync::Arc, time::Duration};

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
/// which images are decoded, which of its code blocks already carry syntax
/// colors, and — for the front matter — the reader's own front-matter
/// options. The images and colors arrive asynchronously; a change to any of
/// these must invalidate exactly the blocks that use it rather than the
/// whole layout cache.
fn external_key(
	block: &Block,
	images: &crate::image::ImageSnapshot,
	highlights: &HashMap<u64, highlights::HighlightResult>,
	theme: Option<&str>,
	options: &LayoutOptions,
) -> u64 {
	let mut specs = Vec::new();
	block.images(&mut specs);
	let mut code = Vec::new();
	block.code_blocks(&mut code);
	// Disclosure identities bind summary targets; presentation overrides do
	// not belong to the geometry key.
	let mut disclosures = Vec::new();
	disclosure_identities(block, &mut disclosures);
	// The front matter is the only block that reads the reader's own options:
	// it draws the reader's interface label, and an export hides it entirely.
	// Neither belongs to the source, so both belong to the block's key.
	let front_matter = matches!(block.kind, BlockKind::FrontMatter { .. })
		.then(|| (&options.front_matter_label, options.hide_front_matter));
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
				let key = highlights::key(
					language,
					text,
					theme,
					options.limits.highlight_line_bytes,
				);
				(key, highlights.contains_key(&key))
			})
			.collect::<Vec<_>>(),
		disclosures,
		front_matter,
	))
}

/// Binds summary targets and source defaults independently of reader state.
fn disclosure_identities(block: &Block, out: &mut Vec<(u64, bool)>) {
	match &block.kind {
		BlockKind::Details { open, blocks, .. }
		| BlockKind::FrontMatter { open, blocks } => {
			out.push((block.id, *open));
			for block in blocks {
				disclosure_identities(block, out);
			}
		}
		BlockKind::Quote { blocks, .. }
		| BlockKind::Footnote { blocks, .. } => {
			for block in blocks {
				disclosure_identities(block, out);
			}
		}
		BlockKind::List { items, .. } => {
			for item in items {
				for block in &item.blocks {
					disclosure_identities(block, out);
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
	/// declared, so a fresh document starts there. Presentation state does not
	/// invalidate cached geometry.
	pub details_open: Arc<std::collections::BTreeMap<u64, bool>>,
	/// Render every `<details>` expanded regardless of the map. Exports set
	/// this: a printed page has no pointer to open a collapsed body with.
	pub force_open: bool,
	/// The label on a collapsed front-matter block. The core holds no
	/// interface text, so the reader supplies the one it draws.
	pub front_matter_label: String,
	/// Draw no front matter at all. Exports set this: metadata is the reader's
	/// aid, not part of the document's text.
	pub hide_front_matter: bool,
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
			front_matter_label: "Frontmatter".into(),
			hide_front_matter: false,
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
			&& self.front_matter_label == other.front_matter_label
			&& self.hide_front_matter == other.hide_front_matter
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
}

struct BlockContext<'a> {
	nested: Option<cache::NestedCache<'a>>,
	cancelled: &'a dyn Fn() -> bool,
	search_fields: HashMap<usize, crate::search::SearchField>,
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
	nested_cache: HashMap<u64, cache::NestedEntry>,
	/// Increases once per pass; an entry's stamp says which pass last used it.
	pass: u64,
	/// The newest pass that reached `close`. Its geometry stays reusable while
	/// later passes are abandoned before touching every block.
	completed: Option<u64>,
	highlights: highlights::Highlights,
}

/// A cached block geometry. The `Arc` is shared with the snapshot that
/// published it, so retaining every entry costs one pointer, not a copy.
struct CacheEntry {
	layout: Arc<BlockLayout>,
	/// The pass that last used the entry, whether it measured or reused it.
	pass: u64,
	/// The newest pass in which this entry was part of a completing layout.
	/// A reuse must leave it alone, or an abandoned pass would erase the only
	/// evidence that the geometry belongs to the last completed pass.
	completed: Option<u64>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
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
		Self::with_executor(
			crate::background::default_executor(),
			Arc::new(|| {}),
		)
	}
	/// Injects shared CPU capacity and a completion callback.
	pub fn with_executor(
		executor: Arc<dyn crate::background::Executor>,
		wake: crate::background::Wake,
	) -> Self {
		Self {
			shaper: TextShaper::new(),
			math: MathEngine::default(),
			cache: HashMap::new(),
			nested_cache: HashMap::new(),
			pass: 0,
			completed: None,
			highlights: highlights::Highlights::new(executor, wake),
		}
	}
	pub fn clear_document_cache(&mut self) {
		self.cache.clear();
		self.nested_cache.clear();
	}
	/// Drops geometry and syntax colors for a document that is no longer open,
	/// so an idle reader keeps nothing from it.
	pub fn release_document(&mut self) {
		self.cache.clear();
		self.nested_cache.clear();
		self.highlights.clear();
	}
	/// How many block geometries the cache holds, so a test can check that
	/// abandoned passes cannot accumulate.
	#[cfg(test)]
	pub(crate) fn cached_blocks(&self) -> usize {
		self.cache.len()
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
	///
	/// A front end that must return to its event loop between blocks calls
	/// [`Self::begin_layout`] and [`Self::advance`] instead; this is those two
	/// driven to completion, so there is one block loop, not two.
	pub fn layout_progressive(
		&mut self,
		document: &Document,
		options: &LayoutOptions,
		images: &crate::image::ImageSnapshot,
		progress: impl FnMut(&LayoutSnapshot) -> bool,
	) -> Option<LayoutSnapshot> {
		self.layout_progressive_cancellable(
			document,
			options,
			images,
			progress,
			|| false,
		)
	}

	/// Checks cancellation between nested blocks as well as published prefixes.
	pub fn layout_progressive_cancellable(
		&mut self,
		document: &Document,
		options: &LayoutOptions,
		images: &crate::image::ImageSnapshot,
		mut progress: impl FnMut(&LayoutSnapshot) -> bool,
		cancelled: impl Fn() -> bool,
	) -> Option<LayoutSnapshot> {
		let mut layout = self.begin_layout(document, options, images);
		while !layout.is_complete() {
			if cancelled() || !progress(layout.snapshot()) {
				return None;
			}
			// A zero budget lays out exactly one block, so the closure sees
			// every prefix.
			self.advance_cancellable(
				&mut layout,
				document,
				Duration::ZERO,
				&cancelled,
			);
			if cancelled() {
				return None;
			}
		}
		Some(layout.into_snapshot())
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
