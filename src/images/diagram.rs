//! Mermaid diagrams, rendered to SVG on the image workers.
//!
//! Parsing a diagram does not depend on how it is drawn, so the two are cached
//! apart: one parse per source, and one SVG per source and theme. Switching
//! themes therefore lays out and renders again, but never parses again.
//!
//! `mermaid-rs-renderer` discovers system fonts lazily on its first text
//! measurement, so importing it costs nothing until a diagram is rendered.
use anyhow::{Result, anyhow, bail};
use markview_core::style::{Color, Stylesheet};
use std::{
	borrow::Borrow,
	collections::{HashMap, VecDeque},
	hash::Hash,
	sync::{Arc, Mutex, OnceLock},
};

/// Diagrams kept rendered per process. Diagram source is small, so a count cap
/// bounds the cached SVGs and parsed graphs while a reflow or a reopened file
/// never re-renders. Every theme shares both caps, so alternating two themes
/// through a large document re-renders rather than growing without bound.
const CACHE_CAPACITY: usize = 32;

/// Node, edge and subgraph budget for one diagram. The layout recurses once
/// per node along a path (its strongly-connected-component DFS, for example),
/// so a chain of `N` nodes costs about `N` frames. Measured in a debug build
/// on a default 2 MiB stack, a 2000-node chain fit in 512 KiB but overflowed
/// 256 KiB, putting one frame between 131 and 262 bytes and the exhaustion
/// point near 10,000 nodes. A node and an edge count equally here, so a simple
/// path spends half the budget on nodes and recurses at most 256 times, under
/// 70 KiB. Subgraphs share the budget because nesting is walked recursively
/// too.
pub(super) const MAX_GRAPH_ELEMENTS: usize = 512;

/// Source cap checked before parsing, so an oversized fence costs nothing and
/// a pathological one cannot abort a worker. It is also the ultimate bound on
/// the dependency's recursion: a recursive descent consumes at least one byte
/// per frame to terminate, so no traversal can be deeper than the source is
/// long. [`RENDER_STACK_BYTES`] is sized for that whole range rather than for
/// one grammar's nesting cost.
pub(super) const MAX_SOURCE_BYTES: usize = 8 * 1024;

/// Brace-nesting cap for label markup. The text normalizer recurses once per
/// `{...}` group it rewrites, so this bounds that recursion directly and turns
/// a pathological label into a readable error before any layout runs. Real
/// diagrams nest a handful of groups; the byte cap would otherwise allow
/// thousands.
pub(super) const MAX_LABEL_NESTING: usize = 64;

/// Stack for the thread that runs the parse and layout stages.
///
/// The source cap is the ultimate bound on every recursive descent in the
/// dependency: a frame consumes at least one byte before it can return, so no
/// traversal is deeper than the 8192 bytes the cap allows. A debug build (the
/// largest frames) was measured at about 1.8 KiB per nesting level, so the
/// worst case a one-byte-per-frame recursion could reach is under 15 MiB;
/// 32 MiB covers that with room for frames up to 4 KiB. The graph traversals
/// are separately bounded by [`MAX_GRAPH_ELEMENTS`] at a few hundred bytes per
/// frame, and [`MAX_LABEL_NESTING`] rejects deep brace markup before this
/// backstop is needed at all.
const RENDER_STACK_BYTES: usize = 32 * 1024 * 1024;

/// A stylesheet's resolved diagram theme. Two of these are the same theme
/// exactly when their fingerprints match, so the scheduler can tell a color
/// change from a repeat without comparing every field.
pub(super) struct DiagramTheme {
	render: mermaid_rs_renderer::Theme,
	/// The same theme with the reader's Han faces in front, which is what a
	/// diagram carrying Han text is drawn with.
	han: mermaid_rs_renderer::Theme,
	fingerprint: u64,
}

impl DiagramTheme {
	pub(super) fn fingerprint(&self) -> u64 {
		self.fingerprint
	}
	/// The theme for the diagram whose source is `code`.
	fn for_source(&self, code: &str) -> &mermaid_rs_renderer::Theme {
		if markview_core::needs_cjk_faces(code) {
			&self.han
		} else {
			&self.render
		}
	}
}

/// Identity of a resolved theme, which covers every field, including a
/// `font_family` that a font definition can move without touching the
/// `[mermaid]` table. The Han variant is covered with it.
fn fingerprint(
	render: &mermaid_rs_renderer::Theme,
	han: &mermaid_rs_renderer::Theme,
) -> u64 {
	crate::document::fingerprint(&(format!("{render:?}"), format!("{han:?}")))
}

/// Resolves the `[mermaid]` table: the named preset, then every field it sets.
pub(super) fn resolve(sheet: &Stylesheet) -> DiagramTheme {
	let style = &sheet.mermaid;
	let mut render = mermaid_rs_renderer::Theme::from_name(style.preset())
		.unwrap_or_else(mermaid_rs_renderer::Theme::modern);
	// A `font_family` whose definitions are all unavailable keeps the preset's
	// own list.
	let families = font_families(sheet);
	if !families.is_empty() {
		render.font_family = families;
	}
	macro_rules! set {
		($($field:ident),*) => {
			$(if let Some(value) = style.$field { render.$field = hex(value); })*
		};
	}
	set!(
		primary_color,
		primary_text_color,
		primary_border_color,
		line_color,
		secondary_color,
		tertiary_color,
		edge_label_background,
		cluster_background,
		cluster_border,
		background,
		sequence_actor_fill,
		sequence_actor_border,
		sequence_actor_line,
		sequence_note_fill,
		sequence_note_border,
		sequence_activation_fill,
		sequence_activation_border,
		text_color,
		git_commit_label_color,
		git_commit_label_background,
		git_tag_label_color,
		git_tag_label_background,
		git_tag_label_border,
		pie_title_text_color,
		pie_section_text_color,
		pie_legend_text_color,
		pie_stroke_color,
		pie_outer_stroke_color
	);
	macro_rules! set_palette {
		($($field:ident),*) => {
			$(if let Some(values) = &style.$field { render.$field = values.map(hex); })*
		};
	}
	set_palette!(
		git_colors,
		git_inv_colors,
		git_branch_label_colors,
		pie_colors
	);
	macro_rules! set_number {
		($($field:ident),*) => {
			$(if let Some(value) = style.$field { render.$field = value; })*
		};
	}
	set_number!(
		font_size,
		pie_title_text_size,
		pie_section_text_size,
		pie_legend_text_size,
		pie_stroke_width,
		pie_outer_stroke_width,
		pie_opacity
	);
	// The rasterizer resolves one base face per text element and falls back,
	// with a warning, for every cluster that face cannot draw. Neither the
	// renderer's presets nor a theme's own list carries a Han face, so a
	// diagram with Han text leads with the sheet's.
	let mut han = render.clone();
	let han_faces = sheet.cjk_families();
	if !han_faces.is_empty() {
		han.font_family = han_first(&render.font_family, &han_faces);
	}
	DiagramTheme {
		fingerprint: fingerprint(&render, &han),
		render,
		han,
	}
}

/// The renderer's font list with the Han faces `faces` in front of `base`,
/// without repeating a family the list already names.
fn han_first(base: &str, faces: &[&str]) -> String {
	let existing: Vec<&str> = base
		.split(',')
		.map(|name| name.trim().trim_matches(['"', '\'']))
		.collect();
	let mut out = String::new();
	for name in faces {
		if existing.contains(&name.trim()) {
			continue;
		}
		if !out.is_empty() {
			out.push_str(", ");
		}
		out.push_str(&quote(name));
	}
	if out.is_empty() {
		return base.to_owned();
	}
	format!("{out}, {base}")
}

/// The renderer's font list for the sheet's `[mermaid] font_family`.
///
/// The renderer reads the system's own font database, so a definition only
/// satisfied by `--fonts` or a downloaded file is not visible here and falls
/// back like any other unavailable candidate.
fn font_families(sheet: &Stylesheet) -> String {
	sheet
		.mermaid_font_families()
		.into_iter()
		.map(quote)
		.collect::<Vec<_>>()
		.join(", ")
}

/// A family name as a font list spells it. A name with a space is single
/// quoted; the renderer strips the quotes again for the SVG, where a double
/// quote would end the `font-family` attribute.
fn quote(name: &str) -> String {
	if name.contains(' ') && !name.contains('\'') {
		format!("'{name}'")
	} else {
		name.to_owned()
	}
}

/// The `#RRGGBB` (or `#RRGGBBAA`) spelling the renderer reads. An opaque color
/// stays six digits, which every SVG reader understands.
fn hex(color: Color) -> String {
	if color.0 & 255 == 255 {
		format!("#{:06x}", color.0 >> 8)
	} else {
		format!("#{:08x}", color.0)
	}
}

/// A map that forgets its oldest insertion, so every cache here is bounded by
/// a count rather than by the documents a reader happened to open.
#[derive(Default)]
struct Bounded<K, V> {
	entries: HashMap<K, V>,
	order: VecDeque<K>,
}

impl<K: Eq + Hash + Clone, V> Bounded<K, V> {
	fn get<Q>(&self, key: &Q) -> Option<&V>
	where
		K: Borrow<Q>,
		Q: Hash + Eq + ?Sized,
	{
		self.entries.get(key)
	}

	fn insert(&mut self, key: K, value: V) {
		if self.entries.insert(key.clone(), value).is_none() {
			self.order.push_back(key);
		}
		while self.order.len() > CACHE_CAPACITY {
			if let Some(oldest) = self.order.pop_front() {
				self.entries.remove(&oldest);
			}
		}
	}
}

/// Parsed diagrams by source, which is what a theme change reuses.
type ParsedCache = Bounded<String, Arc<mermaid_rs_renderer::ParseOutput>>;

/// Rendered SVGs by theme and source.
type SvgCache = Bounded<(u64, String), Arc<str>>;

fn parsed_cache() -> &'static Mutex<ParsedCache> {
	static CACHE: OnceLock<Mutex<ParsedCache>> = OnceLock::new();
	CACHE.get_or_init(|| Mutex::new(Bounded::default()))
}

fn svg_cache() -> &'static Mutex<SvgCache> {
	static CACHE: OnceLock<Mutex<SvgCache>> = OnceLock::new();
	CACHE.get_or_init(|| Mutex::new(Bounded::default()))
}

/// Whether a parsed diagram of `elements` nodes, edges and subgraphs is small
/// enough for the layout's recursive traversals.
pub(super) fn within_graph_budget(elements: usize) -> bool {
	elements <= MAX_GRAPH_ELEMENTS
}

/// Whether `code`'s `{...}` nesting stays inside the normalizer's budget. The
/// scan is over the raw source, so it also catches markup assembled inside a
/// quoted label; `}` is allowed to close more than it opened so a stray one
/// never underflows.
pub(super) fn within_nesting_budget(code: &str) -> bool {
	let mut depth = 0usize;
	let mut deepest = 0usize;
	for byte in code.bytes() {
		match byte {
			b'{' => {
				depth += 1;
				deepest = deepest.max(depth);
			}
			b'}' => depth = depth.saturating_sub(1),
			_ => {}
		}
	}
	deepest <= MAX_LABEL_NESTING
}

/// Runs `f` on a thread whose stack covers the worst case the byte cap allows.
/// The thread is joined before this returns, so none is left behind.
fn on_render_stack<T: Send>(f: impl FnOnce() -> Result<T> + Send) -> Result<T> {
	std::thread::scope(|scope| {
		std::thread::Builder::new()
			.name("markview-mermaid".into())
			.stack_size(RENDER_STACK_BYTES)
			.spawn_scoped(scope, f)
			.expect("start Mermaid render")
			.join()
			.unwrap_or_else(|_| Err(anyhow!("Mermaid: renderer panicked")))
	})
}

/// The parsed diagram for `code`, parsing it on first use.
fn parsed(code: &str) -> Result<Arc<mermaid_rs_renderer::ParseOutput>> {
	if let Some(parsed) = parsed_cache().lock().unwrap().get(code) {
		return Ok(parsed.clone());
	}
	let parsed = Arc::new(on_render_stack(|| {
		mermaid_rs_renderer::parse_mermaid_strict(code)
			.map_err(|e| anyhow!("Mermaid: {e}"))
	})?);
	let mut cache = parsed_cache().lock().unwrap();
	if let Some(existing) = cache.get(code) {
		return Ok(existing.clone());
	}
	cache.insert(code.to_owned(), parsed.clone());
	Ok(parsed)
}

/// Renders through the dependency's stages instead of its `render` wrapper, so
/// the parsed graph can be bounded before layout walks it. A stack overflow
/// cannot be caught, so the bounds run first and the recursion runs on a
/// thread whose stack covers the worst case the byte cap allows.
fn render_bounded(code: &str, theme: &DiagramTheme) -> Result<String> {
	if code.len() > MAX_SOURCE_BYTES {
		bail!(
			"Mermaid: diagram source exceeds {} KiB",
			MAX_SOURCE_BYTES / 1024
		);
	}
	if !within_nesting_budget(code) {
		bail!("Mermaid: label markup nests deeper than {MAX_LABEL_NESTING}");
	}
	let parsed = parsed(code)?;
	let graph = &parsed.graph;
	let elements =
		graph.nodes.len() + graph.edges.len() + graph.subgraphs.len();
	if !within_graph_budget(elements) {
		bail!(
			"Mermaid: diagram has {elements} graph elements, \
			 the limit is {MAX_GRAPH_ELEMENTS}"
		);
	}
	let config = mermaid_rs_renderer::LayoutConfig::default();
	let render = theme.for_source(code);
	on_render_stack(|| {
		let layout =
			mermaid_rs_renderer::compute_layout(graph, render, &config);
		Ok(mermaid_rs_renderer::render_svg(&layout, render, &config))
	})
}

/// The SVG for a diagram source under one theme, rendering it on first use.
/// The render runs outside the lock, so independent diagrams do not serialize
/// on the cache.
pub(super) fn svg(code: &str, theme: &DiagramTheme) -> Result<Arc<str>> {
	let key = (theme.fingerprint(), code.to_owned());
	if let Some(svg) = svg_cache().lock().unwrap().get(&key) {
		return Ok(svg.clone());
	}
	let svg: Arc<str> = render_bounded(code, theme)?.into();
	let mut cache = svg_cache().lock().unwrap();
	if let Some(existing) = cache.get(&key) {
		return Ok(existing.clone());
	}
	cache.insert(key, svg.clone());
	Ok(svg)
}

#[cfg(test)]
mod tests {
	use super::*;

	/// A stylesheet whose `[mermaid]` table is exactly `table`, so a test
	/// resolves the way a theme does, font definitions included.
	fn theme(table: &str) -> DiagramTheme {
		resolve(
			&Stylesheet::parse(&format!(
				"format_version=2\nversion=1\n[mermaid]\n{table}"
			))
			.unwrap(),
		)
	}

	fn default_theme() -> DiagramTheme {
		resolve(&Stylesheet::default())
	}

	fn dark() -> DiagramTheme {
		theme("theme='dark'")
	}

	fn cached_parse(code: &str) -> Arc<mermaid_rs_renderer::ParseOutput> {
		parsed_cache()
			.lock()
			.unwrap()
			.get(code)
			.cloned()
			.expect("the parse is cached")
	}

	#[test]
	fn renders_each_source_once() {
		let code = "graph TD\n X[one]-->Y[two]\n";
		let theme = default_theme();
		let first = svg(code, &theme).unwrap();
		assert!(first.contains("<svg"));
		assert!(Arc::ptr_eq(&first, &svg(code, &theme).unwrap()));
	}

	#[test]
	fn a_theme_change_reuses_the_parsed_source() {
		let code = "graph TD\n Reuse[keep]-->Parsed[once]\n";
		let light = svg(code, &default_theme()).unwrap();
		let parsed = cached_parse(code);
		let dark_svg = svg(code, &dark()).unwrap();
		assert!(Arc::ptr_eq(&parsed, &cached_parse(code)));
		// The two themes really do draw differently, and both stay cached.
		assert_ne!(light, dark_svg);
		assert!(Arc::ptr_eq(&dark_svg, &svg(code, &dark()).unwrap()));
	}

	#[test]
	fn a_custom_background_reaches_the_svg() {
		let svg = svg(
			"graph TD\n X[one]-->Y[two]\n",
			&theme("background='#202630'"),
		)
		.unwrap();
		assert!(svg.contains("#202630"), "{svg}");
	}

	#[test]
	fn a_font_family_resolves_font_definitions_like_a_rule() {
		let sheet = Stylesheet::parse(
			"format_version=2\nversion=1\n\
			 [[fontdef]]\nid='reading'\nlookfor=['Noto Serif CJK SC', 'serif']\n\
			 [[fontdef]]\nid='emoji'\nemoji=true\nlookfor=['Noto Color Emoji']\n\
			 [mermaid]\nfont_family=['reading', 'emoji', 'monospace']",
		)
		.unwrap();
		let theme = resolve(&sheet);
		assert_eq!(
			theme.render.font_family,
			"'Noto Serif CJK SC', serif, 'Noto Color Emoji', monospace"
		);
		// A `fontdef` the sheet declares but did not select resolves to
		// nothing, as it does for a rule.
		let sheet = Stylesheet::parse(
			"format_version=2\nversion=1\n\
			 [[fontdef]]\nid='serif[cjk]'\ntype='TC'\nlookfor=['Songti TC']\n\
			 [mermaid]\nfont_family=['serif[cjk]', 'monospace']",
		)
		.unwrap();
		assert_eq!(resolve(&sheet).render.font_family, "monospace");
	}

	/// The font list the renderer wrote into the SVG's first text element.
	fn font_family(svg: &str) -> String {
		svg.split("font-family=\"")
			.nth(1)
			.and_then(|rest| rest.split('"').next())
			.unwrap_or_default()
			.to_owned()
	}

	#[test]
	fn a_han_label_leads_with_the_readers_han_faces() {
		let mut sheet = Stylesheet::parse(
			"format_version=2\nversion=1\n\
			 [[fontdef]]\nid='han'\ntype='SC'\nlookfor=['Songti SC', 'serif']\n\
			 [[rule]]\nwhen=['body']\nfont=[{family='han'}]",
		)
		.unwrap();
		sheet.set_cjk_type(markview_core::style::CjkType::Sc);
		let theme = resolve(&sheet);
		// Han text leads with the reader's own Han face, which covers Latin
		// as well; a Latin-only diagram keeps the renderer's preset list. The
		// renderer strips the list's quotes again before writing the SVG.
		let han =
			font_family(&svg("graph TD\n A[草稿]-->B\n", &theme).unwrap());
		assert!(han.starts_with("Songti SC,"), "{han}");
		let latin =
			font_family(&svg("graph TD\n A[Draft]-->B\n", &theme).unwrap());
		assert!(latin.contains("DejaVu Sans"), "{latin}");
		assert!(!latin.contains("Songti"), "{latin}");
	}

	#[test]
	fn malformed_source_is_an_error() {
		assert!(svg("flowchart LR\n--> B\n", &default_theme()).is_err());
	}

	#[test]
	fn over_budget_graph_is_rejected_before_layout() {
		// 300 edges with 301 nodes is over the element budget but well under
		// the source cap, so this exercises the graph check on its own.
		let mut code = String::from("flowchart TD\n");
		for i in 0..300 {
			code.push_str(&format!("N{i}-->N{}\n", i + 1));
		}
		let error = svg(&code, &default_theme()).unwrap_err().to_string();
		assert!(error.contains("graph elements"), "{error}");
	}
}
