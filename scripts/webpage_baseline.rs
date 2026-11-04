//! Measurement adapter for the unmodified pre-refactor model and layout.
use anyhow::{Result, bail};
use dom_smoothie::{Config, Readability, TextMode};
use markview_document::{
	Block, BlockKind, Document, InlineKind, RichText, plain_text,
};
use markview_layout::layout::{LayoutEngine, LayoutOptions};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
	collections::BTreeMap,
	process::Command,
	sync::atomic::Ordering,
	time::{Duration, Instant},
};
const WARMUP: usize = 3;
const ITERATIONS: usize = 20;
const URL: &str = "https://example.org/article";
mod file {
	pub const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
}
#[allow(unsafe_code)]
mod counting {
	use std::alloc::{GlobalAlloc, Layout, System};
	use std::sync::atomic::{AtomicU64, Ordering};

	pub static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
	pub static BYTES: AtomicU64 = AtomicU64::new(0);

	pub struct Allocator;

	// SAFETY: every method forwards to the system allocator unchanged; the
	// counters only observe the request, they do not change its semantics.
	unsafe impl GlobalAlloc for Allocator {
		unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
			ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
			BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
			// SAFETY: forwards the caller's valid layout unchanged.
			unsafe { System.alloc(layout) }
		}

		unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
			// SAFETY: forwards the caller's allocation and layout unchanged.
			unsafe { System.dealloc(ptr, layout) }
		}

		unsafe fn realloc(
			&self,
			ptr: *mut u8,
			layout: Layout,
			new_size: usize,
		) -> *mut u8 {
			ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
			BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
			// SAFETY: forwards the caller's allocation, layout and new size
			// unchanged.
			unsafe { System.realloc(ptr, layout, new_size) }
		}
	}
}

#[global_allocator]
static ALLOCATOR: counting::Allocator = counting::Allocator;

mod paste {
	pub(crate) fn looks_like_markdown(text: &str) -> bool {
		let text = text.trim();
		if text.is_empty() || text.len() > crate::file::MAX_FILE_BYTES as usize
		{
			return false;
		}
		if text.chars().filter(|c| *c == '\u{fffd}').count() >= 2 {
			return false;
		}
		text.chars()
			.all(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
	}
}
fn extract(html: &str, url: &str) -> Result<String> {
	let config = Config {
		text_mode: TextMode::Markdown,
		max_elements_to_parse: 100_000,
		..Default::default()
	};
	let document = dom_query::Document::from(html);
	let preserved = preserve_markup(&document);
	let article =
		Readability::with_document(document, Some(url), Some(config))?
			.parse()?;
	if article.text_content.trim().is_empty() {
		bail!("No readable article found on this page");
	}
	// A single plain heading names the temporary tab; source links stay absolute.
	let title = article
		.title
		.split_whitespace()
		.collect::<Vec<_>>()
		.join(" ");
	let title = if title.is_empty() { url } else { &title };
	let mut heading = String::with_capacity(title.len());
	for c in title.chars() {
		if c.is_ascii_punctuation() {
			use std::fmt::Write;
			write!(heading, "&#{};", c as u32).unwrap();
		} else {
			heading.push(c);
		}
	}
	let mut content = literal_delimiters(&article.text_content);
	for (token, replacement) in preserved {
		content = content.replace(&token, &replacement);
	}
	let markdown = format!("# {heading}\n\nSource: <{url}>\n\n{}\n", content);
	if !crate::paste::looks_like_markdown(&markdown) {
		bail!("Extracted web article exceeds the supported Markdown limits");
	}
	Ok(markdown)
}

/// Protect semantic HTML that the Markdown serializer treats as plain text.
fn preserve_markup(document: &dom_query::Document) -> Vec<(String, String)> {
	let text = document.root().text();
	let mut prefix = "MARKVIEWPRESERVED".to_string();
	while text.contains(&prefix) {
		prefix.push('X');
	}
	let mut preserved = Vec::new();
	for node in document
		.select(".math.inline, .math.display, script[type^='math/tex']")
		.nodes()
	{
		if node.ancestors_it(None).any(|parent| parent.is("pre, code")) {
			continue;
		}
		let text = node.text();
		let mut latex = text.trim();
		for (open, close) in
			[(r"\(", r"\)"), (r"\[", r"\]"), ("$$", "$$"), ("$", "$")]
		{
			if let Some(inner) =
				latex.strip_prefix(open).and_then(|s| s.strip_suffix(close))
			{
				latex = inner;
				break;
			}
		}
		let in_table =
			node.ancestors_it(None).any(|parent| parent.is("td, th"));
		// TeX delimiters keep literal pipes from splitting Markdown table cells.
		let latex = if in_table {
			latex.replace(r"\|", r"\Vert ").replace('|', r"\vert ")
		} else {
			latex.to_string()
		};
		let display = node.has_class("display")
			|| node
				.attr("type")
				.is_some_and(|kind| kind.contains("mode=display"));
		let replacement = if display && !in_table {
			format!("\n\n\\[{latex}\\]\n\n")
		} else {
			format!("\\({latex}\\)")
		};
		let token = format!("{prefix}{}END", preserved.len());
		if node.is("script") {
			node.rename("span");
		}
		node.set_text(token.as_str());
		preserved.push((token, replacement));
	}
	// Empty headings otherwise produce an empty, invalid table separator cell.
	for node in document.select("th").nodes() {
		if node.inner_html().trim().is_empty() {
			let token = format!("{prefix}{}END", preserved.len());
			node.set_text(token.as_str());
			preserved.push((token, String::new()));
		}
	}
	preserved
}

/// Keep CommonMark punctuation escapes from becoming Markview LaTeX delimiters.
fn literal_delimiters(markdown: &str) -> String {
	let arena = comrak::Arena::new();
	let root =
		comrak::parse_document(&arena, markdown, &comrak::Options::default());
	let mut lines = vec![0];
	lines.extend(markdown.match_indices('\n').map(|(offset, _)| offset + 1));
	let mut code = root
		.descendants()
		.filter_map(|node| {
			let data = node.data.borrow();
			matches!(
				data.value,
				comrak::nodes::NodeValue::Code(_)
					| comrak::nodes::NodeValue::CodeBlock(_)
			)
			.then(|| {
				let position = data.sourcepos;
				let start =
					lines[position.start.line - 1] + position.start.column - 1;
				let end = (lines[position.end.line - 1] + position.end.column)
					.min(markdown.len());
				start..end
			})
		})
		.peekable();
	let mut output = String::with_capacity(markdown.len());
	let mut offset = 0;
	while offset < markdown.len() {
		if let Some(range) = code.peek()
			&& offset == range.start
		{
			output.push_str(&markdown[range.clone()]);
			offset = range.end;
			code.next();
			continue;
		}
		let tail = &markdown[offset..];
		let mut chars = tail.chars();
		let c = chars.next().unwrap();
		if c == '\\'
			&& let Some(next) = chars.next()
		{
			match next {
				'(' => output.push_str("&#40;"),
				')' => output.push_str("&#41;"),
				'[' => output.push_str("&#91;"),
				']' => output.push_str("&#93;"),
				_ => {
					output.push(c);
					output.push(next);
				}
			}
			offset += c.len_utf8() + next.len_utf8();
		} else {
			output.push(c);
			offset += c.len_utf8();
		}
	}
	output
}

struct Fixture {
	name: &'static str,
	html: String,
	capability: bool,
}

fn read_fixture(name: &'static str, file: &str, capability: bool) -> Fixture {
	Fixture {
		name,
		html: std::fs::read_to_string(format!("tests/fixtures/{file}"))
			.unwrap(),
		capability,
	}
}

fn long_article() -> String {
	let mut body = String::from(
		"<html><head><title>Long article</title></head><body><article><h1>Long article</h1>",
	);
	for i in 0..4000 {
		body.push_str(&format!(
			"<p>Section {i}. Native typography, formulas, links and tables keep their place in the article. Read <a href=\"/note/{i}\">note {i}</a> for the next detail, then continue with ordinary prose that gives the extractor a stable body.</p>\n"
		));
	}
	body.push_str("</article></body></html>");
	body
}

fn fixtures() -> Vec<Fixture> {
	vec![
		read_fixture("small", "web-article.html", false),
		Fixture {
			name: "long",
			html: long_article(),
			capability: false,
		},
		read_fixture("math_dense", "web-math-dense.html", false),
		read_fixture("table_dense", "web-table-dense.html", false),
		read_fixture("repeated_link", "web-repeated-link.html", false),
		read_fixture("complex_table", "web-complex-table.html", true),
	]
}

fn fonts() -> markview_layout::fonts::FontConfig {
	markview_layout::fonts::FontConfig::from_faces(
		0x7765627071,
		[
			include_bytes!(
				"../crates/markview-layout/tests/fonts/NotoSerif-Regular-subset.otf"
			)
			.as_slice(),
			include_bytes!(
				"../crates/markview-layout/tests/fonts/NotoSans-Regular-subset.otf"
			)
			.as_slice(),
			include_bytes!(
				"../crates/markview-layout/tests/fonts/NotoSansMono-Regular-subset.otf"
			)
			.as_slice(),
		]
		.into_iter()
		.map(|data| parley::fontique::Blob::new(std::sync::Arc::new(data)))
		.collect(),
	)
}

fn layout_options() -> LayoutOptions {
	LayoutOptions {
		width: 640.,
		viewport_height: 800.,
		fonts: fonts(),
		..Default::default()
	}
}

fn sha256(bytes: &[u8]) -> String {
	let mut hasher = Sha256::new();
	hasher.update(bytes);
	hasher
		.finalize()
		.iter()
		.map(|byte| format!("{byte:02x}"))
		.collect()
}

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
struct Normalized {
	blocks: Vec<String>,
	fields: Vec<NormalizedField>,
	links: Vec<NormalizedLink>,
	images: Vec<NormalizedImage>,
	tables: Vec<NormalizedTable>,
}

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
struct NormalizedField {
	owner: String,
	kind: &'static str,
	text: String,
	links: Vec<NormalizedLink>,
	images: Vec<NormalizedImage>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
struct NormalizedLink {
	address: String,
	occurrence: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
struct NormalizedImage {
	address: String,
	alt: String,
	title: String,
	width: Option<u32>,
	height: Option<u32>,
}

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
struct NormalizedTable {
	caption: Option<String>,
	columns: Vec<Option<String>>,
	groups: Vec<NormalizedGroup>,
}

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
struct NormalizedGroup {
	kind: &'static str,
	rows: Vec<NormalizedRow>,
}

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
struct NormalizedRow {
	cells: Vec<NormalizedCell>,
}

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
struct NormalizedCell {
	kind: &'static str,
	rowspan: u32,
	colspan: u32,
	align: Option<String>,
	blocks: Vec<String>,
}

fn align_label(align: Option<markview_document::CellAlign>) -> Option<String> {
	// The old Markdown round trip always writes a separator row, so an
	// unspecified HTML table column becomes `Left`. Both render left in this
	// LTR profile; compare the distinction only where it changes the grid.
	align.and_then(|align| match align {
		markview_document::CellAlign::Left => None,
		markview_document::CellAlign::Center => Some("center".to_owned()),
		markview_document::CellAlign::Right => Some("right".to_owned()),
	})
}

fn add_field(
	out: &mut Normalized,
	owner: String,
	kind: &'static str,
	text: String,
	rich: Option<&RichText>,
	ids: &mut BTreeMap<usize, usize>,
) {
	let mut links = Vec::new();
	let mut images = Vec::new();
	if let Some(rich) = rich {
		for inline in rich {
			if let Some(link) = &inline.style.link {
				let next = ids.len();
				let occurrence = *ids.entry(link.origin()).or_insert(next);
				let link = NormalizedLink {
					address: link.as_str().to_owned(),
					occurrence,
				};
				if links.last() != Some(&link) {
					links.push(link.clone());
				}
				if out.links.last() != Some(&link) {
					out.links.push(link);
				}
			}
			if let InlineKind::Image(image) = &inline.kind {
				let image = NormalizedImage {
					address: image.src.clone(),
					alt: image.alt.clone(),
					title: image.title.clone(),
					width: image.width,
					height: image.height,
				};
				images.push(image.clone());
				out.images.push(image);
			}
		}
	}
	out.fields.push(NormalizedField {
		owner,
		kind,
		text,
		links,
		images,
	});
}
fn normalize(document: &Document) -> Normalized {
	let mut out = Normalized {
		blocks: Vec::new(),
		fields: Vec::new(),
		links: Vec::new(),
		images: Vec::new(),
		tables: Vec::new(),
	};
	let mut ids = BTreeMap::new();
	normalize_blocks(&document.blocks[2..], &mut out, &mut ids);
	out
}
fn normalize_blocks(
	blocks: &[Block],
	out: &mut Normalized,
	ids: &mut BTreeMap<usize, usize>,
) {
	for block in blocks {
		match &block.kind {
			BlockKind::Paragraph(text) | BlockKind::Heading { text, .. } => {
				let label = match &block.kind {
					BlockKind::Heading { level, .. } => {
						format!("heading:{level}")
					}
					_ => "paragraph".into(),
				};
				out.blocks.push(label.clone());
				add_field(
					out,
					label,
					"rich",
					plain_text(text),
					Some(text),
					ids,
				);
			}
			BlockKind::Code { language, text } => {
				let label = format!("code:{language}");
				out.blocks.push(label.clone());
				add_field(
					out,
					label,
					"code",
					text.strip_suffix('\n').unwrap_or(text).into(),
					None,
					ids,
				);
			}
			BlockKind::Table { align, rows } => {
				let groups_count = if rows.len() > 1 { 2 } else { 1 };
				out.blocks.push(format!(
					"table:cols={}:groups={groups_count}:rows={}:caption=false",
					align.len(),
					rows.len()
				));
				let mut groups = Vec::new();
				for (i, row) in rows.iter().enumerate() {
					let kind = if i == 0 { "head" } else { "body" };
					let mut cells = Vec::new();
					for text in row {
						out.blocks.push("paragraph".into());
						add_field(
							out,
							"paragraph".into(),
							"rich",
							plain_text(text),
							Some(text),
							ids,
						);
						cells.push(NormalizedCell {
							kind: if i == 0 { "header" } else { "data" },
							rowspan: 1,
							colspan: 1,
							align: None,
							blocks: vec![format!(
								"paragraph[{}]",
								plain_text(text)
							)],
						});
					}
					if groups
						.last()
						.is_none_or(|g: &NormalizedGroup| g.kind != kind)
					{
						groups.push(NormalizedGroup {
							kind,
							rows: Vec::new(),
						});
					}
					groups
						.last_mut()
						.unwrap()
						.rows
						.push(NormalizedRow { cells });
				}
				out.tables.push(NormalizedTable {
					caption: None,
					columns: align
						.iter()
						.map(|a| align_label(Some(*a)))
						.collect(),
					groups,
				});
			}
			BlockKind::List {
				start,
				tight,
				items,
			} => {
				out.blocks.push(format!(
					"list:start={start:?}:reversed=false:tight={tight}:items={}",
					items.len()
				));
				for item in items {
					normalize_blocks(&item.blocks, out, ids);
				}
			}
			BlockKind::Quote { callout, blocks } => {
				let label = if callout.is_some() {
					"quote:callout"
				} else {
					"quote"
				};
				out.blocks.push(label.into());
				if let Some(callout) = callout {
					add_field(
						out,
						label.into(),
						"rich",
						plain_text(&callout.title),
						Some(&callout.title),
						ids,
					);
				}
				normalize_blocks(blocks, out, ids);
			}
			BlockKind::Details {
				open,
				summary,
				blocks,
				..
			} => {
				let label = format!("details:open={open}");
				out.blocks.push(label.clone());
				add_field(
					out,
					label,
					"rich",
					plain_text(summary),
					Some(summary),
					ids,
				);
				normalize_blocks(blocks, out, ids);
			}
			BlockKind::Footnote { label, blocks, .. } => {
				out.blocks.push(format!("footnote:{label}"));
				normalize_blocks(blocks, out, ids);
			}
			BlockKind::FrontMatter { open, blocks } => {
				out.blocks.push(format!("front_matter:open={open}"));
				normalize_blocks(blocks, out, ids);
			}
			BlockKind::Rule => out.blocks.push("rule".into()),
		}
	}
}
fn peak_rss_kb() -> u64 {
	std::fs::read_to_string("/proc/self/status")
		.ok()
		.and_then(|status| {
			status.lines().find_map(|line| {
				line.strip_prefix("VmHWM:")?
					.split_whitespace()
					.next()?
					.parse()
					.ok()
			})
		})
		.unwrap_or(0)
}

fn rich_bytes(text: &RichText) -> usize {
	std::mem::size_of_val(text.as_slice())
		+ text
			.iter()
			.map(|inline| {
				let content = match &inline.kind {
					InlineKind::Text(t) | InlineKind::Math { latex: t, .. } => {
						t.capacity()
					}
					InlineKind::Anchor(a) => a.capacity(),
					InlineKind::Image(i) => {
						i.src.capacity() + i.alt.capacity() + i.title.capacity()
					}
					_ => 0,
				};
				content
					+ inline.style.link.as_ref().map_or(0, |l| l.as_str().len())
			})
			.sum::<usize>()
}
fn node_bytes(blocks: &[Block]) -> usize {
	std::mem::size_of_val(blocks)
		+ blocks
			.iter()
			.map(|b| match &b.kind {
				BlockKind::Paragraph(t)
				| BlockKind::Heading { text: t, .. } => rich_bytes(t),
				BlockKind::Code { language, text } => {
					language.capacity() + text.capacity()
				}
				BlockKind::Quote { blocks, .. }
				| BlockKind::Footnote { blocks, .. }
				| BlockKind::FrontMatter { blocks, .. } => node_bytes(blocks),
				BlockKind::Details {
					summary, blocks, ..
				} => rich_bytes(summary) + node_bytes(blocks),
				BlockKind::List { items, .. } => {
					std::mem::size_of_val(items.as_slice())
						+ items
							.iter()
							.map(|i| node_bytes(&i.blocks))
							.sum::<usize>()
				}
				BlockKind::Table { rows, .. } => {
					rows.iter().flatten().map(rich_bytes).sum()
				}
				BlockKind::Rule => 0,
			})
			.sum::<usize>()
}
fn memory(result: &Iteration) -> Value {
	let mut seen = std::collections::HashSet::new();
	let geometry: usize = result
		.snapshot
		.blocks
		.iter()
		.filter(|b| seen.insert(std::sync::Arc::as_ptr(&b.layout)))
		.map(|b| {
			std::mem::size_of_val(b.layout.draws.as_slice())
				+ std::mem::size_of_val(b.layout.text.as_slice())
				+ b.layout
					.text
					.iter()
					.map(|n| {
						n.text.capacity()
							+ std::mem::size_of_val(n.clusters.as_slice())
					})
					.sum::<usize>()
		})
		.sum();
	let source = result.document.source.len();
	let nodes = node_bytes(&result.document.blocks);
	json!({"original_source_bytes":source,"source_map_bytes_estimate":0,"semantic_nodes_bytes_estimate":nodes,"semantic_index_bytes_estimate":0,"layout_geometry_bytes_estimate":geometry,"total_bytes_estimate":source+nodes+geometry,"method":"lower-bound payload estimates; source ranges live inline in nodes, no separate SourceMap or resident semantic index; geometry excludes private scene storage and Draw payloads; not allocator measurements or RSS"})
}
static LIVE_RESULTS: std::sync::atomic::AtomicUsize =
	std::sync::atomic::AtomicUsize::new(0);
struct Iteration {
	document: Document,
	snapshot: markview_layout::scene::LayoutSnapshot,
	sample: Value,
}
impl Drop for Iteration {
	fn drop(&mut self) {
		assert_eq!(LIVE_RESULTS.swap(0, Ordering::Relaxed), 1);
	}
}
fn run(fixture: &Fixture, options: &LayoutOptions) -> Iteration {
	assert_eq!(
		LIVE_RESULTS.swap(1, Ordering::Relaxed),
		0,
		"previous iteration must be dropped"
	);
	counting::ALLOCATIONS.store(0, Ordering::Relaxed);
	counting::BYTES.store(0, Ordering::Relaxed);
	let start = Instant::now();
	let markdown = extract(&fixture.html, URL).unwrap();
	let document = markview_commonmark::parse(markdown);
	let extract_ms = start.elapsed().as_secs_f64() * 1000.;
	let allocations = counting::ALLOCATIONS.load(Ordering::Relaxed);
	let allocated_bytes = counting::BYTES.load(Ordering::Relaxed);
	let mut engine = LayoutEngine::new();
	let mut layout =
		engine.begin_layout(&document, options, &Default::default());
	while !layout.is_complete() && layout.snapshot().height < 800. {
		engine.advance(&mut layout, &document, Duration::ZERO);
	}
	let first_viewport_layout_ms = start.elapsed().as_secs_f64() * 1000.;
	while !layout.is_complete() {
		engine.advance(&mut layout, &document, Duration::from_secs(60));
	}
	let full_layout_ms = start.elapsed().as_secs_f64() * 1000.;
	Iteration {
		document,
		snapshot: layout.into_snapshot(),
		sample: json!({"extract_convert_ms":extract_ms, "first_viewport_layout_ms":first_viewport_layout_ms, "full_layout_ms":full_layout_ms, "allocations":allocations, "allocated_bytes":allocated_bytes}),
	}
}
fn distribution(samples: &[Value], key: &str) -> Value {
	let mut values: Vec<f64> =
		samples.iter().map(|s| s[key].as_f64().unwrap()).collect();
	values.sort_by(f64::total_cmp);
	json!({"median_ms": (values[9]+values[10])/2., "p95_ms": values[18]})
}
fn main() {
	let args: Vec<_> = std::env::args().collect();
	let fixtures = fixtures();
	let options = layout_options();
	if args.get(1).is_some_and(|s| s == "--child") {
		let fixture = fixtures.iter().find(|f| f.name == args[2]).unwrap();
		for _ in 0..=WARMUP {
			drop(run(fixture, &options));
		}
		println!("{}", peak_rss_kb());
		return;
	}
	let mut rows = Vec::new();
	for fixture in &fixtures {
		for _ in 0..WARMUP {
			drop(run(fixture, &options));
		}
		let mut last = None;
		let mut samples = Vec::new();
		for _ in 0..ITERATIONS {
			drop(last.take());
			let result = run(fixture, &options);
			samples.push(result.sample.clone());
			last = Some(result);
		}
		let result = last.as_ref().unwrap();
		let input = tempfile::NamedTempFile::new().unwrap();
		std::fs::write(input.path(), &fixture.html).unwrap();
		let output = Command::new(
			std::env::current_exe()
				.unwrap()
				.with_file_name("webpage_baseline_rss"),
		)
		.arg(input.path())
		.output()
		.unwrap();
		assert!(
			output.status.success(),
			"{}",
			String::from_utf8_lossy(&output.stderr)
		);
		let rss: u64 = String::from_utf8(output.stdout)
			.unwrap()
			.trim()
			.parse()
			.unwrap();
		rows.push(json!({"fixture":fixture.name, "fixture_sha256":sha256(fixture.html.as_bytes()), "fixture_kind":if fixture.capability {"capability"} else {"equivalence"}, "extract_convert":distribution(&samples,"extract_convert_ms"), "first_viewport_layout":distribution(&samples,"first_viewport_layout_ms"), "full_layout":distribution(&samples,"full_layout_ms"), "allocations_per_iteration":{"median":distribution(&samples,"allocations")["median_ms"]}, "allocated_bytes_per_iteration":{"median":distribution(&samples,"allocated_bytes")["median_ms"]}, "peak_rss_kb":rss, "per_iteration_samples":samples, "body_semantics":normalize(&result.document), "memory":memory(result), "reading_text_sha256":sha256(result.snapshot.full_reading_text().as_bytes())}));
	}
	println!("{}", serde_json::to_string_pretty(&json!({"true_pre_refactor_baseline":true,"revision":"4328b6246ac6deed0243bff3869fef1dcbc74d2b", "warmup":WARMUP,"iterations":ITERATIONS,"viewport":{"width":640,"height":800},"samples":rows})).unwrap());
}
