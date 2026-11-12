//! HTML to semantic document extraction for native web reading.
//!
//! The DOM is lowered directly into [`markview_document::Document`]; no
//! intermediate HTML, Markdown or plain-text serialization is produced for the
//! article body.
mod language;
mod lower;
mod metadata;
use markview_document::Document;
pub use metadata::ArticleMetadata;

/// Extraction limits and readability configuration for the native reader.
#[derive(Clone, Debug)]
pub struct Profile {
	/// Largest accepted HTML input, before DOM parsing.
	pub max_input_bytes: usize,
	/// dom_smoothie's element-scan limit.
	pub max_elements: usize,
	/// Total DOM nodes (elements, text and comments) after extraction.
	pub max_dom_nodes: usize,
	/// Semantic block and inline recursion limit.
	pub max_semantic_depth: usize,
	/// Total semantic text bytes.
	pub max_text_bytes: usize,
	/// Total table rows across all tables.
	pub max_table_rows: usize,
	/// Table columns per table.
	pub max_table_columns: usize,
	/// Table cells per table.
	pub max_table_cells: usize,
	/// Total semantic blocks.
	pub max_blocks: usize,
	/// Classes preserved through Readability cleanup.
	pub preserved_classes: &'static [&'static str],
}

/// Version of the extraction profile and lowering semantics.
pub const PROFILE_VERSION: u64 = 2;

/// The settings this reader's HTML whitespace, tables and formulas assume.
pub static PROFILE_V1: Profile = Profile {
	max_input_bytes: 32 * 1024 * 1024,
	max_elements: 100_000,
	max_dom_nodes: 400_000,
	max_semantic_depth: 256,
	max_text_bytes: 32 * 1024 * 1024,
	max_table_rows: 16_384,
	max_table_columns: 256,
	max_table_cells: 131_072,
	max_blocks: 100_000,
	preserved_classes: &[
		"math", "inline", "display", "katex", "MathJax", "mathjax",
	],
};

/// Why a page could not be converted into a document.
#[derive(Debug)]
pub enum Error {
	InputTooLarge { bytes: usize, limit: usize },
	TooManyElements { elements: usize, limit: usize },
	TooManyDomNodes { nodes: usize, limit: usize },
	TooManyBlocks { blocks: usize, limit: usize },
	TooManyText { bytes: usize, limit: usize },
	TableTooLarge,
	DepthExceeded,
	NoReadableContent,
	UnsupportedUrl(String),
	Extraction(String),
}

impl std::fmt::Display for Error {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::InputTooLarge { bytes, limit } => write!(
				f,
				"web page is {bytes} bytes, above the {limit}-byte input limit"
			),
			Self::TooManyElements { elements, limit } => write!(
				f,
				"web page has {elements} elements, above the {limit}-element limit"
			),
			Self::TooManyDomNodes { nodes, limit } => write!(
				f,
				"web page has {nodes} DOM nodes, above the {limit}-node limit"
			),
			Self::TooManyBlocks { blocks, limit } => write!(
				f,
				"web page has {blocks} blocks, above the {limit}-block limit"
			),
			Self::TooManyText { bytes, limit } => write!(
				f,
				"web page has {bytes} text bytes, above the {limit}-byte limit"
			),
			Self::TableTooLarge => f.write_str(
				"web page contains a table above the supported size",
			),
			Self::DepthExceeded => {
				f.write_str("web page nesting exceeds the supported depth")
			}
			Self::NoReadableContent => {
				f.write_str("No readable article found on this page")
			}
			Self::UnsupportedUrl(url) => {
				write!(f, "web page base is not an HTTP(S) URL: {url}")
			}
			Self::Extraction(message) => f.write_str(message),
		}
	}
}

impl std::error::Error for Error {}

impl From<dom_smoothie::ReadabilityError> for Error {
	fn from(error: dom_smoothie::ReadabilityError) -> Self {
		Self::Extraction(format!("{error}"))
	}
}

/// A converted web article: the semantic document and its metadata.
#[derive(Clone, Debug)]
pub struct WebArticle {
	pub document: Document,
	pub metadata: ArticleMetadata,
	/// Deterministic digest of the HTML input that produced this article.
	pub input_fingerprint: u64,
}

/// Extracts a readable article from `html` using the default profile.
pub fn extract(html: &str, url: &str) -> Result<WebArticle, Error> {
	extract_with(html, url, &PROFILE_V1)
}

/// Extracts a readable article from `html` with explicit limits.
pub fn extract_with(
	html: &str,
	url: &str,
	profile: &Profile,
) -> Result<WebArticle, Error> {
	if html.len() > profile.max_input_bytes {
		return Err(Error::InputTooLarge {
			bytes: html.len(),
			limit: profile.max_input_bytes,
		});
	}
	let base = url::Url::parse(url)
		.map_err(|_| Error::UnsupportedUrl(url.to_owned()))?;
	if !matches!(base.scheme(), "http" | "https") || base.host_str().is_none() {
		return Err(Error::UnsupportedUrl(url.to_owned()));
	}
	let config = dom_smoothie::Config {
		candidate_select_mode: dom_smoothie::CandidateSelectMode::Readability,
		char_threshold: 500,
		n_top_candidates: 5,
		min_score_to_adjust: 5.0,
		disable_json_ld: false,
		keep_classes: false,
		max_elements_to_parse: profile.max_elements,
		classes_to_preserve: profile
			.preserved_classes
			.iter()
			.map(|class| (*class).to_owned())
			.collect(),
		readable_min_score: 20.0,
		readable_min_content_length: 140,
		text_mode: dom_smoothie::TextMode::Raw,
	};
	let document = dom_smoothie::dom_query::Document::from(html);
	check_element_budget(&document, profile.max_elements)?;
	let canonical = declared_canonical(&document, &base);
	// The semantic pre-passes walk every node, so the total-node budget must
	// be checked before them rather than only after extraction.
	check_dom_node_budget(&document, profile.max_dom_nodes)?;
	clear_internal_attributes(&document);
	mark_title_headings(&document);
	language::preserve(&document);
	preserve_semantics(&document);
	let extracted = dom_smoothie::Readability::with_document(
		document,
		Some(url),
		Some(config),
	)?
	.extract()?;
	check_dom_node_budget(&extracted.document, profile.max_dom_nodes)?;
	let metadata =
		ArticleMetadata::from_extracted(&extracted, &base, canonical);
	let lower = lower::Lower::new(&extracted, base, profile.clone());
	let (document, diagnostics) =
		lower.run(&metadata.title, &metadata.final_url)?;
	Ok(WebArticle {
		document,
		metadata: metadata.with_diagnostics(diagnostics),
		input_fingerprint: markview_document::stable_fingerprint(
			html.as_bytes(),
		),
	})
}

/// The page's declared canonical URL, resolved against the final URL.
fn declared_canonical(
	document: &dom_smoothie::dom_query::Document,
	base: &url::Url,
) -> Option<String> {
	let link = document.select("link[rel~=\"canonical\"]");
	let href = link.nodes().first()?.attr("href")?;
	base.join(href.trim()).ok().map(|url| url.to_string())
}

/// Rejects a document above `limit` total DOM nodes before any pre-pass scans
/// it, and again after extraction when cleanup may have changed the count.
fn check_dom_node_budget(
	document: &dom_smoothie::dom_query::Document,
	limit: usize,
) -> Result<(), Error> {
	let nodes = document.tree.len();
	if nodes > limit {
		return Err(Error::TooManyDomNodes { nodes, limit });
	}
	Ok(())
}

/// Rejects a document above `limit` elements before any pre-pass scans it.
///
/// Readability checks the same limit, but only after the pre-passes have
/// already walked the whole DOM, so an oversized page must fail first.
fn check_element_budget(
	document: &dom_smoothie::dom_query::Document,
	limit: usize,
) -> Result<(), Error> {
	if limit == 0 {
		return Ok(());
	}
	let elements = document
		.root()
		.descendants_it()
		.filter(|node| node.is_element())
		.count();
	if elements > limit {
		return Err(Error::TooManyElements { elements, limit });
	}
	Ok(())
}

/// Removes attributes from the input that would otherwise be trusted as
/// internal markers read back after extraction.
fn clear_internal_attributes(document: &dom_smoothie::dom_query::Document) {
	for name in [
		"data-mv-title",
		"data-mv-align",
		"data-mv-checked",
		"data-mv-math",
		"data-mv-display",
		"data-mv-latex",
		"data-mv-language",
	] {
		let selector = format!("*[{name}]");
		document.select(&selector).remove_attr(name);
	}
}

/// Marks every top-level heading as an article title candidate.
///
/// Readability may rename or nest the title heading it keeps, so lowering
/// recognizes it by this attribute instead of comparing any text.
fn mark_title_headings(document: &dom_smoothie::dom_query::Document) {
	for node in document.root().descendants() {
		if node.is_element()
			&& node
				.node_name()
				.is_some_and(|name| name.eq_ignore_ascii_case("h1"))
		{
			node.set_attr("data-mv-title", "1");
		}
	}
}

/// Keeps a cell's alignment under an attribute Readability preserves.
fn preserve_cell_align(
	node: &dom_smoothie::dom_query::NodeRef<'_>,
	name: &str,
) {
	if (name == "td" || name == "th")
		&& let Some(align) = node.attr("align")
	{
		node.set_attr("data-mv-align", &align);
	}
}

/// Keeps the checklist state of a checkbox's owning list item.
///
/// Readability removes `input` elements, so the lowering cannot read the
/// checkbox afterwards. One walk over the checkboxes marks every owning item,
/// so nested lists cost their depth instead of a scan per list item.
fn preserve_checkbox(checkbox: &dom_smoothie::dom_query::NodeRef<'_>) {
	if !checkbox
		.attr("type")
		.is_some_and(|kind| kind.eq_ignore_ascii_case("checkbox"))
	{
		return;
	}
	let owner = checkbox.ancestors_it(None).find(|ancestor| {
		ancestor
			.node_name()
			.is_some_and(|name| name.eq_ignore_ascii_case("li"))
	});
	let Some(owner) = owner else {
		return;
	};
	// The first checkbox of an item is the one the reader sees.
	if owner.attr("data-mv-checked").is_some() {
		return;
	}
	let checked = if checkbox.attr("checked").is_some() {
		"1"
	} else {
		"0"
	};
	owner.set_attr("data-mv-checked", checked);
}

/// Copies class-driven meaning that Readability otherwise drops.
fn preserve_classes(
	node: &dom_smoothie::dom_query::NodeRef<'_>,
	name: &str,
	classes: &str,
) {
	let math = classes.split_whitespace().any(|class| class == "math");
	if math {
		node.set_attr("data-mv-math", "1");
	}
	if math
		&& classes
			.split_whitespace()
			.any(|class| class == "display" || class == "block")
	{
		node.set_attr("data-mv-display", "1");
	}
	// Readability rewrites bare `div`s into paragraphs and drops their
	// attributes, so keep block formulas in a preserved container until
	// lowering.
	if math && name == "div" {
		node.rename("pre");
	}
}

/// Copies semantic information into attributes Readability keeps.
fn preserve_semantics(document: &dom_smoothie::dom_query::Document) {
	// Preserve code line breaks before Readability rewrites consecutive `br`s.
	for br in document.select("pre br").nodes() {
		br.replace_with(&document.tree.new_text("\n"));
	}
	for node in document.root().descendants() {
		if !node.is_element() {
			continue;
		}
		let name = node
			.node_name()
			.map(|name| name.to_ascii_lowercase())
			.unwrap_or_default();
		if let Some(classes) = node.attr("class") {
			preserve_classes(&node, &name, &classes);
		}
		preserve_cell_align(&node, &name);
		if name == "input" {
			preserve_checkbox(&node);
		}
		if name == "math" {
			let latex = node.descendants().into_iter().find(|child| {
				child.is_element()
					&& child.node_name().is_some_and(|name| {
						name.eq_ignore_ascii_case("annotation")
					})
					&& child.attr("encoding").is_some_and(|encoding| {
						encoding
							.to_ascii_lowercase()
							.contains("application/x-tex")
					})
			});
			if let Some(latex) = latex {
				node.set_attr("data-mv-latex", &latex.text());
			}
		}
		if name == "script"
			&& let Some(kind) = node.attr("type")
			&& kind.to_ascii_lowercase().starts_with("math/tex")
		{
			let latex = node.text().to_string();
			node.rename("span");
			node.set_attr("data-mv-math", "1");
			node.set_attr("data-mv-latex", &latex);
			if kind.to_ascii_lowercase().contains("mode=display") {
				node.set_attr("data-mv-display", "1");
			}
		}
	}
}
