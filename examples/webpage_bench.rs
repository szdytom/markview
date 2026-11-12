//! Compare the old Markdown round trip with the direct DOM lowering.
//!
//! Run with `cargo build --release --locked --examples && target/release/examples/webpage_bench`; it writes
//! `artifacts/webpage-refactor/{baseline,after}.json`. The old numbers are an
//! explicit replay of the old extraction pipeline on the current model, not a
//! build of the pre-refactor revision.
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

#[path = "webpage_bench/replay.rs"]
mod old_replay;

use markview_document::{
	Block, BlockId, BlockKind, Document, FieldText, Inline, InlineKind,
	OutlineEntry, plain_text, plain_text_len, visit_blocks, visit_fields,
};
use markview_layout::layout::{LayoutEngine, LayoutOptions};
use markview_layout::scene::{
	BlockLayout, HeadingAnchor, Overflow, PageConstraint, PlacedBlock,
};
use markview_layout::text::TextNode;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::io::Write as _;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

const ITERATIONS: usize = 20;
const WARMUP: usize = 3;
const URL: &str = "https://example.org/article";
const COMMAND: &str = "cargo build --release --locked --examples && target/release/examples/webpage_bench";

#[derive(Clone, Copy)]
enum Pipeline {
	OldReplay,
	NewDirect,
}

impl Pipeline {
	fn key(self) -> &'static str {
		match self {
			Self::OldReplay => "old_extraction_pipeline_replay",
			Self::NewDirect => "new_direct_lowering",
		}
	}

	fn from_key(key: &str) -> Option<Self> {
		match key {
			"old_extraction_pipeline_replay" => Some(Self::OldReplay),
			"new_direct_lowering" => Some(Self::NewDirect),
			_ => None,
		}
	}

	fn run(self, html: &str) -> Produced {
		match self {
			Self::OldReplay => {
				let old = old_replay::extract(html, URL).unwrap();
				let parse = markview_commonmark::parse(old.markdown);
				Produced::Old {
					parse,
					title: old.title,
				}
			}
			Self::NewDirect => {
				Produced::New(markview_webpage::extract(html, URL).unwrap())
			}
		}
	}
}

static LIVE_RESULTS: std::sync::atomic::AtomicUsize =
	std::sync::atomic::AtomicUsize::new(0);

impl Drop for Produced {
	fn drop(&mut self) {
		assert_eq!(LIVE_RESULTS.swap(0, Ordering::Relaxed), 1);
	}
}

enum Produced {
	Old {
		parse: markview_commonmark::MarkdownParse,
		title: String,
	},
	New(markview_webpage::WebArticle),
}

impl Produced {
	fn document(&self) -> &Document {
		match self {
			Self::Old { parse, .. } => &parse.document,
			Self::New(article) => &article.document,
		}
	}

	fn title(&self) -> &str {
		match self {
			Self::Old { title, .. } => title,
			Self::New(article) => &article.metadata.title,
		}
	}

	fn original_source_bytes(&self) -> usize {
		match self {
			Self::Old { parse, .. } => parse.source.len(),
			Self::New(_) => 0,
		}
	}

	fn source_map_bytes_estimate(&self) -> usize {
		match self {
			Self::Old { parse, .. } => source_map_estimate(&parse.map),
			Self::New(_) => 0,
		}
	}
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

#[path = "webpage_bench/fonts.rs"]
mod bench_fonts;
use bench_fonts::layout_options;

fn sha256(bytes: &[u8]) -> String {
	let mut hasher = Sha256::new();
	hasher.update(bytes);
	hasher
		.finalize()
		.iter()
		.map(|byte| format!("{byte:02x}"))
		.collect()
}

#[derive(Clone, Copy)]
struct Sample {
	extract_ms: f64,
	first_viewport_layout_ms: f64,
	first_viewport_blocks: usize,
	first_viewport_height: f32,
	full_layout_ms: f64,
	allocations: u64,
	allocated_bytes: u64,
}

struct Measured {
	produced: Produced,
	samples: Vec<Sample>,
	snapshot: markview_layout::scene::LayoutSnapshot,
	peak_rss_kb: u64,
}

fn run_iteration(
	fixture: &Fixture,
	pipeline: Pipeline,
	options: &LayoutOptions,
) -> (Produced, Sample, markview_layout::scene::LayoutSnapshot) {
	assert_eq!(
		LIVE_RESULTS.swap(1, Ordering::Relaxed),
		0,
		"previous iteration result must be released"
	);
	counting::ALLOCATIONS.store(0, Ordering::Relaxed);
	counting::BYTES.store(0, Ordering::Relaxed);
	let start = Instant::now();
	let produced = pipeline.run(&fixture.html);
	let extract = start.elapsed();
	let allocations = counting::ALLOCATIONS.load(Ordering::Relaxed);
	let allocated_bytes = counting::BYTES.load(Ordering::Relaxed);
	let images = markview_layout::image::ImageSnapshot::default();
	let mut engine = LayoutEngine::new();
	let mut layout = engine.begin_layout(produced.document(), options, &images);
	while !layout.is_complete()
		&& layout.snapshot().height < options.viewport_height
	{
		engine.advance(&mut layout, produced.document(), Duration::ZERO);
	}
	let first = start.elapsed();
	let first_viewport_blocks = layout.snapshot().blocks.len();
	let first_viewport_height = layout.snapshot().height;
	while !layout.is_complete() {
		engine.advance(
			&mut layout,
			produced.document(),
			Duration::from_secs(60),
		);
	}
	let full = start.elapsed();
	let snapshot = layout.into_snapshot();
	(
		produced,
		Sample {
			extract_ms: extract.as_secs_f64() * 1000.0,
			first_viewport_layout_ms: first.as_secs_f64() * 1000.0,
			first_viewport_blocks,
			first_viewport_height,
			full_layout_ms: full.as_secs_f64() * 1000.0,
			allocations,
			allocated_bytes,
		},
		snapshot,
	)
}

fn measure(
	fixture: &Fixture,
	pipeline: Pipeline,
	options: &LayoutOptions,
) -> Measured {
	for _ in 0..WARMUP {
		let _ = run_iteration(fixture, pipeline, options);
	}
	let mut samples = Vec::with_capacity(ITERATIONS);
	let mut last = None;
	for _ in 0..ITERATIONS {
		drop(last.take());
		let (produced, sample, snapshot) =
			run_iteration(fixture, pipeline, options);
		samples.push(sample);
		last = Some((produced, snapshot));
	}
	let (produced, snapshot) = last.unwrap();
	Measured {
		produced,
		samples,
		snapshot,
		peak_rss_kb: peak_rss_child(fixture, pipeline),
	}
}

fn peak_rss_child(fixture: &Fixture, pipeline: Pipeline) -> u64 {
	let input = tempfile::NamedTempFile::new().unwrap();
	std::fs::write(input.path(), &fixture.html).unwrap();
	let executable =
		std::env::current_exe()
			.unwrap()
			.with_file_name(match pipeline {
				Pipeline::OldReplay => "webpage_replay_rss",
				Pipeline::NewDirect => "webpage_rss",
			});
	let output = Command::new(executable)
		.arg(input.path())
		.arg(pipeline.key())
		.output()
		.expect("spawn peak-RSS child");
	if !output.status.success() {
		panic!(
			"peak-RSS child failed for {} / {}: {}",
			fixture.name,
			pipeline.key(),
			String::from_utf8_lossy(&output.stderr)
		);
	}
	String::from_utf8_lossy(&output.stdout)
		.trim()
		.parse()
		.expect("peak-RSS child prints VmHWM")
}

fn child_main(name: &str, pipeline_key: &str) {
	let fixture = fixtures()
		.into_iter()
		.find(|fixture| fixture.name == name)
		.expect("known child fixture");
	let pipeline =
		Pipeline::from_key(pipeline_key).expect("known child pipeline");
	let options = layout_options();
	// One measured pass after warmup is enough for a process high-water mark.
	let mut last = None;
	for _ in 0..=WARMUP {
		drop(last.take());
		last = Some(run_iteration(&fixture, pipeline, &options));
	}
	drop(last);
	println!("{}", peak_rss_kb());
}

fn distributions(
	samples: &[Sample],
	value: impl Fn(&Sample) -> f64,
) -> (f64, f64) {
	let mut values: Vec<f64> = samples.iter().map(value).collect();
	values.sort_by(f64::total_cmp);
	let median = if values.len().is_multiple_of(2) {
		(values[values.len() / 2 - 1] + values[values.len() / 2]) / 2.0
	} else {
		values[values.len() / 2]
	};
	let rank = (values.len() * 95).div_ceil(100).saturating_sub(1);
	(median, values[rank.min(values.len() - 1)])
}

fn numbers(samples: &[Sample], value: impl Fn(&Sample) -> u64) -> (f64, f64) {
	let mut values: Vec<f64> =
		samples.iter().map(|sample| value(sample) as f64).collect();
	values.sort_by(f64::total_cmp);
	let median = if values.len().is_multiple_of(2) {
		(values[values.len() / 2 - 1] + values[values.len() / 2]) / 2.0
	} else {
		values[values.len() / 2]
	};
	let rank = (values.len() * 95).div_ceil(100).saturating_sub(1);
	(median, values[rank.min(values.len() - 1)])
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

fn block_count(document: &Document) -> usize {
	let mut count = 0;
	for block in document.blocks() {
		visit_blocks(block, &mut |_| count += 1);
	}
	count
}

fn all_block_ids(document: &Document) -> Vec<BlockId> {
	let mut ids = Vec::new();
	for block in document.blocks() {
		visit_blocks(block, &mut |block| ids.push(block.id()));
	}
	ids
}

fn source_map_estimate(map: &markview_document::SourceMap) -> usize {
	let mut bytes = std::mem::size_of::<markview_document::SourceMap>();
	for block in map.blocks() {
		bytes += std::mem::size_of::<markview_document::BlockSource>();
		bytes += block.fields.capacity()
			* std::mem::size_of::<markview_document::FieldSource>();
		for field in &block.fields {
			bytes += field.spans.capacity()
				* std::mem::size_of::<markview_document::SourceSpan>();
			bytes += field.covers.capacity()
				* std::mem::size_of::<markview_document::SourceSpan>();
		}
	}
	// `reverse` and `by_block` duplicate spans and block ids; the reverse
	// index stores one entry per span with three indices.
	let spans: usize = map
		.blocks()
		.iter()
		.flat_map(|block| &block.fields)
		.map(|field| field.spans.len())
		.sum();
	bytes + spans * std::mem::size_of::<(usize, usize, usize, usize)>()
}

fn rich_estimate(text: &markview_document::RichText) -> usize {
	text.capacity() * std::mem::size_of::<Inline>()
		+ text.iter().map(inline_estimate).sum::<usize>()
}

fn inline_estimate(inline: &Inline) -> usize {
	match &inline.kind {
		InlineKind::Text(text) | InlineKind::Anchor(text) => text.capacity(),
		InlineKind::Math { latex, .. } => latex.capacity(),
		InlineKind::Image(image) => {
			let mut bytes = image.alt.capacity()
				+ image.title.capacity()
				+ match &image.source {
					markview_document::ImageSource::Resource(resource) => {
						resource.address.capacity()
					}
					markview_document::ImageSource::Diagram(diagram) => {
						diagram.language.capacity() + diagram.source.capacity()
					}
				};
			bytes += 0;
			bytes
		}
		InlineKind::FootnoteRef(reference) => {
			reference.definition.as_str().len()
		}
		InlineKind::LineBreak(_) => 0,
	}
}

fn blocks_estimate(blocks: &[Block]) -> usize {
	std::mem::size_of_val(blocks)
		+ blocks.iter().map(block_estimate).sum::<usize>()
}

fn block_estimate(block: &Block) -> usize {
	let mut bytes: usize =
		block.anchors().iter().map(|anchor| anchor.capacity()).sum();
	bytes += match block.kind() {
		BlockKind::Paragraph(text) => rich_estimate(text),
		BlockKind::Heading { text, anchor, .. } => {
			rich_estimate(text) + anchor.capacity()
		}
		BlockKind::Code { language, text } => {
			language.capacity() + text.capacity()
		}
		BlockKind::Quote { callout, blocks } => {
			let callout_bytes = callout.as_ref().map_or(0, |callout| {
				callout.name.capacity()
					+ callout.title.as_ref().map_or(0, rich_estimate)
			});
			callout_bytes + blocks_estimate(blocks)
		}
		BlockKind::List { items, .. } => {
			items.capacity()
				* std::mem::size_of::<markview_document::ListItem>()
				+ items
					.iter()
					.map(|item| blocks_estimate(&item.blocks))
					.sum::<usize>()
		}
		BlockKind::Table(table) => table_estimate(table),
		BlockKind::Footnote { id, blocks, .. } => {
			id.as_str().len() + blocks_estimate(blocks)
		}
		BlockKind::Details {
			summary, blocks, ..
		} => rich_estimate(summary) + blocks_estimate(blocks),
		BlockKind::FrontMatter { blocks, .. } => blocks_estimate(blocks),
		BlockKind::DefinitionList(items) => {
			items.capacity()
				* std::mem::size_of::<markview_document::DefinitionItem>()
				+ items
					.iter()
					.map(|item| {
						item.terms.iter().map(rich_estimate).sum::<usize>()
							+ item
								.definitions
								.iter()
								.map(|blocks| blocks_estimate(blocks))
								.sum::<usize>()
					})
					.sum::<usize>()
		}
		BlockKind::Figure { blocks, caption } => {
			blocks_estimate(blocks) + caption.as_ref().map_or(0, rich_estimate)
		}
		BlockKind::Rule => 0,
	};
	bytes
}

fn table_estimate(table: &markview_document::Table) -> usize {
	let mut bytes = table.caption.as_ref().map_or(0, rich_estimate)
		+ table.columns.capacity()
			* std::mem::size_of::<markview_document::TableColumn>()
		+ table.groups.capacity()
			* std::mem::size_of::<markview_document::TableGroup>();
	for group in &table.groups {
		bytes += group.rows.capacity()
			* std::mem::size_of::<markview_document::TableRow>();
		for row in &group.rows {
			bytes += row.cells.capacity()
				* std::mem::size_of::<markview_document::TableCell>();
			for cell in &row.cells {
				bytes += blocks_estimate(&cell.blocks);
			}
		}
	}
	bytes
}

struct DocumentStats {
	fields: usize,
	plain_text_bytes: usize,
	anchor_count: usize,
	footnote_refs: usize,
}

fn document_stats(document: &Document) -> DocumentStats {
	let mut stats = DocumentStats {
		fields: 0,
		plain_text_bytes: 0,
		anchor_count: 0,
		footnote_refs: 0,
	};
	for root in document.blocks() {
		visit_fields(
			root,
			&mut Vec::new(),
			&|| false,
			&mut |_, _, field, _| {
				stats.fields += 1;
				stats.plain_text_bytes += match field {
					FieldText::Rich(text) => plain_text_len(text),
					FieldText::Code(text) => text.len(),
				};
				if let FieldText::Rich(text) = field {
					for inline in text {
						match &inline.kind {
							InlineKind::Anchor(_) => stats.anchor_count += 1,
							InlineKind::FootnoteRef(_) => {
								stats.footnote_refs += 1
							}
							_ => {}
						}
					}
				}
			},
		);
	}
	stats
}

fn semantic_index_estimate(
	document: &Document,
	stats: &DocumentStats,
) -> usize {
	let ids = all_block_ids(document);
	let blocks: Vec<&Block> =
		ids.iter().filter_map(|id| document.block(*id)).collect();
	let path_entries: usize = ids
		.iter()
		.filter_map(|id| document.path_of(*id))
		.map(|path| {
			std::mem::size_of::<(BlockId, Vec<usize>)>()
				+ std::mem::size_of_val(path)
		})
		.sum();
	let outline: usize = document
		.outline_entries()
		.iter()
		.map(|entry: &OutlineEntry| {
			std::mem::size_of::<OutlineEntry>()
				+ entry.text.capacity()
				+ entry.anchor.capacity()
		})
		.sum();
	let anchors: usize = blocks
		.iter()
		.map(|block| {
			block
				.anchors()
				.iter()
				.map(|anchor| {
					std::mem::size_of::<String>()
						+ anchor.capacity()
						+ std::mem::size_of::<Vec<BlockId>>()
						+ std::mem::size_of::<BlockId>()
				})
				.sum::<usize>()
				+ match block.kind() {
					BlockKind::Heading { anchor, .. } => {
						std::mem::size_of::<String>() + anchor.capacity()
					}
					_ => 0,
				}
		})
		.sum();
	let details = blocks
		.iter()
		.filter(|block| {
			matches!(
				block.kind(),
				BlockKind::Details { .. } | BlockKind::FrontMatter { .. }
			)
		})
		.count();
	let anchor_entries = anchors
		+ (stats.anchor_count + stats.footnote_refs)
			* (std::mem::size_of::<String>()
				+ std::mem::size_of::<Vec<BlockId>>());
	// HashMap tables keep spare capacity; the estimate charges 2x their
	// entry payloads and excludes allocator headers.
	(path_entries + outline + anchor_entries) * 2
		+ details * std::mem::size_of::<(BlockId, bool)>() * 2
}

fn layout_geometry_estimate(
	snapshot: &markview_layout::scene::LayoutSnapshot,
) -> usize {
	let mut seen = HashSet::new();
	let mut bytes =
		snapshot.blocks.capacity() * std::mem::size_of::<PlacedBlock>();
	for placed in &snapshot.blocks {
		let pointer = std::sync::Arc::as_ptr(&placed.layout) as usize;
		if !seen.insert(pointer) {
			continue;
		}
		let layout: &BlockLayout = &placed.layout;
		bytes += std::mem::size_of::<BlockLayout>()
			+ layout.page_constraints.capacity()
				* std::mem::size_of::<PageConstraint>()
			+ layout.text.capacity() * std::mem::size_of::<TextNode>()
			+ layout.draws.storage_bytes()
			+ layout.inline_decorations.capacity()
				* std::mem::size_of::<(usize, std::ops::Range<f32>)>()
			+ layout.overflow.capacity() * std::mem::size_of::<Overflow>()
			+ layout.links.storage_bytes()
			+ layout.link_occurrences.capacity()
				* std::mem::size_of::<markview_document::LinkOccurrenceId>()
			+ layout.anchors.capacity() * std::mem::size_of::<HeadingAnchor>();
	}
	bytes
}

fn memory_report(
	produced: &Produced,
	snapshot: &markview_layout::scene::LayoutSnapshot,
	stats: &DocumentStats,
) -> Value {
	let document = produced.document();
	let source = produced.original_source_bytes();
	let source_map = produced.source_map_bytes_estimate();
	let nodes = blocks_estimate(document.blocks());
	let index = semantic_index_estimate(document, stats);
	let layout = layout_geometry_estimate(snapshot);
	json!({
		"original_source_bytes": source,
		"original_source_method": match produced {
			Produced::Old { .. } => "retained MarkdownParse.source length",
			Produced::New(_) => "not retained by Document; input HTML is not part of the loaded document",
		},
		"source_map_bytes_estimate": source_map,
		"semantic_nodes_bytes_estimate": nodes,
		"semantic_index_bytes_estimate": index,
		"plain_text_projection_bytes_logical": stats.plain_text_bytes,
		"plain_text_fields": stats.fields,
		"layout_geometry_bytes_estimate": layout,
		"layout_snapshot_blocks": snapshot.blocks.len(),
		"layout_reused_blocks": snapshot.reused,
		"total_bytes_estimate": source + source_map + nodes + index + layout,
		"methods": {
			"semantic_nodes": "sum of block/container Vec capacities and string capacities, excluding allocator headers",
			"semantic_index": "path, outline and anchor entry payloads with a 2x HashMap allowance; excludes allocator headers",
			"plain_text_projection": "plain_text_len over every field; logical bytes, not a retained second copy",
			"layout_geometry": "published BlockLayout vector capacities and Arc-unique block geometry; excludes Draw payloads and private Scene storage",
		},
	})
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

fn block_label(block: &Block) -> String {
	match block.kind() {
		BlockKind::Paragraph(_) => "paragraph".into(),
		BlockKind::Heading { level, .. } => format!("heading:{level}"),
		BlockKind::Code { language, .. } => {
			format!("code:{language}")
		}
		BlockKind::Quote { callout, .. } => {
			if callout.is_some() {
				"quote:callout".into()
			} else {
				"quote".into()
			}
		}
		BlockKind::List {
			start,
			reversed,
			tight,
			items,
		} => format!(
			"list:start={start:?}:reversed={reversed}:tight={tight}:items={}",
			items.len()
		),
		BlockKind::Table(table) => {
			let rows: usize =
				table.groups.iter().map(|group| group.rows.len()).sum();
			format!(
				"table:cols={}:groups={}:rows={rows}:caption={}",
				table.columns.len(),
				table.groups.len(),
				table.caption.is_some()
			)
		}
		BlockKind::Footnote { id, .. } => {
			format!("footnote:{}", id.as_str())
		}
		BlockKind::Details { default_open, .. } => {
			format!("details:open={default_open}")
		}
		BlockKind::Rule => "rule".into(),
		BlockKind::FrontMatter { default_open, .. } => {
			format!("front_matter:open={default_open}")
		}
		BlockKind::DefinitionList(items) => {
			format!("definition_list:{}", items.len())
		}
		BlockKind::Figure { caption, .. } => {
			format!("figure:caption={}", caption.is_some())
		}
	}
}

fn block_texts(block: &Block) -> Vec<String> {
	let mut texts = Vec::new();
	visit_fields(block, &mut Vec::new(), &|| false, &mut |_, _, field, _| {
		texts.push(match field {
			FieldText::Rich(text) => plain_text(text),
			FieldText::Code(text) => text.to_owned(),
		});
	});
	texts
}

fn block_summary(block: &Block) -> String {
	format!("{}[{}]", block_label(block), block_texts(block).join("|"))
}

fn table_normalized(table: &markview_document::Table) -> NormalizedTable {
	NormalizedTable {
		caption: table.caption.as_ref().map(|caption| plain_text(caption)),
		columns: table
			.columns
			.iter()
			.map(|column| align_label(column.align))
			.collect(),
		groups: table
			.groups
			.iter()
			.map(|group| NormalizedGroup {
				kind: match group.kind {
					markview_document::TableGroupKind::Head => "head",
					markview_document::TableGroupKind::Body => "body",
					markview_document::TableGroupKind::Foot => "foot",
				},
				rows: group
					.rows
					.iter()
					.map(|row| NormalizedRow {
						cells: row
							.cells
							.iter()
							.map(|cell| NormalizedCell {
								kind: match cell.kind {
									markview_document::CellKind::Header => {
										"header"
									}
									markview_document::CellKind::Data => "data",
								},
								rowspan: cell.rowspan,
								colspan: cell.colspan,
								align: align_label(cell.align),
								blocks: cell
									.blocks
									.iter()
									.map(block_summary)
									.collect(),
							})
							.collect(),
					})
					.collect(),
			})
			.collect(),
	}
}

fn collect_blocks(block: &Block, out: &mut Normalized) {
	visit_blocks(block, &mut |block| {
		out.blocks.push(block_label(block));
		if let BlockKind::Table(table) = block.kind() {
			out.tables.push(table_normalized(table));
		}
	});
}

fn envelope(block: &Block, title: &str, url: &str) -> bool {
	let text = match block.kind() {
		BlockKind::Heading { text, .. } | BlockKind::Paragraph(text) => {
			plain_text(text)
		}
		_ => return false,
	};
	let text = text.trim();
	text == title.trim() || text == format!("Source: {url}")
}

fn inline_image(
	image: &markview_document::image::ImageSpec,
) -> NormalizedImage {
	NormalizedImage {
		address: image.address().to_owned(),
		alt: image.alt.clone(),
		title: image.title.clone(),
		width: image.width,
		height: image.height,
	}
}

fn normalize(document: &Document, title: &str) -> Normalized {
	let mut out = Normalized {
		blocks: Vec::new(),
		fields: Vec::new(),
		links: Vec::new(),
		images: Vec::new(),
		tables: Vec::new(),
	};
	let mut occurrences = BTreeMap::new();
	for root in document.blocks() {
		if envelope(root, title, URL) {
			continue;
		}
		collect_blocks(root, &mut out);
		visit_fields(
			root,
			&mut Vec::new(),
			&|| false,
			&mut |owner, _, field, _| {
				let owner_label = document
					.block(owner)
					.map(block_label)
					.unwrap_or_else(|| "unknown".into());
				let (kind, text) = match field {
					FieldText::Rich(text) => ("rich", plain_text(text)),
					FieldText::Code(text) => (
						"code",
						text.strip_suffix('\n').unwrap_or(text).to_owned(),
					),
				};
				let mut field_links = Vec::new();
				let mut field_images = Vec::new();
				if let FieldText::Rich(text) = field {
					for inline in text {
						if let Some(link) = &inline.link {
							let next = occurrences.len();
							let occurrence = *occurrences
								.entry(link.occurrence())
								.or_insert(next);
							let normalized = NormalizedLink {
								address: link.as_str().to_owned(),
								occurrence,
							};
							if field_links.last() != Some(&normalized) {
								field_links.push(normalized.clone());
							}
							if out.links.last() != Some(&normalized) {
								out.links.push(normalized);
							}
						}
						if let InlineKind::Image(image) = &inline.kind {
							let normalized = inline_image(image);
							field_images.push(normalized.clone());
							out.images.push(normalized);
						}
					}
				}
				out.fields.push(NormalizedField {
					owner: owner_label,
					kind,
					text,
					links: field_links,
					images: field_images,
				});
			},
		);
	}
	out
}

fn semantic_diff(old: &Normalized, new: &Normalized) -> Vec<String> {
	let mut diffs = Vec::new();
	let components: [(&str, String, String); 5] = [
		(
			"blocks",
			format!("{:?}", old.blocks),
			format!("{:?}", new.blocks),
		),
		(
			"fields",
			format!("{:?}", old.fields),
			format!("{:?}", new.fields),
		),
		(
			"links",
			format!("{:?}", old.links),
			format!("{:?}", new.links),
		),
		(
			"images",
			format!("{:?}", old.images),
			format!("{:?}", new.images),
		),
		(
			"tables",
			format!("{:?}", old.tables),
			format!("{:?}", new.tables),
		),
	];
	for (name, old_value, new_value) in components {
		if old_value != new_value {
			let old_short: String = old_value.chars().take(1200).collect();
			let new_short: String = new_value.chars().take(1200).collect();
			diffs.push(format!("{name}: old={old_short} new={new_short}"));
		}
	}
	diffs
}

struct Row {
	fixture: String,
	kind: &'static str,
	input_bytes: usize,
	digest: String,
	pipeline: &'static str,
	top_level_blocks: usize,
	total_blocks: usize,
	extract_median: f64,
	extract_p95: f64,
	first_median: f64,
	first_p95: f64,
	full_median: f64,
	full_p95: f64,
	alloc_median: f64,
	alloc_p95: f64,
	bytes_median: f64,
	bytes_p95: f64,
	peak_rss_kb: u64,
	samples: Vec<Sample>,
	memory: Value,
	body_semantics: Value,
	reading_text_sha256: String,
}

fn row_json(row: &Row) -> Value {
	json!({
		"fixture": row.fixture,
		"fixture_kind": row.kind,
		"input_bytes": row.input_bytes,
		"fixture_sha256": row.digest,
		"pipeline": row.pipeline,
		"top_level_blocks": row.top_level_blocks,
		"total_blocks": row.total_blocks,
		"extract_convert": {
			"median_ms": row.extract_median,
			"p95_ms": row.extract_p95,
		},
		"first_viewport_layout": {
			"median_ms": row.first_median,
			"p95_ms": row.first_p95,
		},
		"full_layout": {
			"median_ms": row.full_median,
			"p95_ms": row.full_p95,
		},
		"allocations_per_iteration": {
			"median": row.alloc_median,
			"p95": row.alloc_p95,
		},
		"allocated_bytes_per_iteration": {
			"median": row.bytes_median,
			"p95": row.bytes_p95,
		},
		"peak_rss_kb": row.peak_rss_kb,
		"memory": row.memory,
		"body_semantics": row.body_semantics,
		"reading_text_sha256": row.reading_text_sha256,
		"per_iteration_samples": row.samples.iter().map(|sample| json!({
			"extract_convert_ms": sample.extract_ms,
			"first_viewport_layout_ms": sample.first_viewport_layout_ms,
			"first_viewport_blocks": sample.first_viewport_blocks,
			"first_viewport_height": sample.first_viewport_height,
			"full_layout_ms": sample.full_layout_ms,
			"allocations": sample.allocations,
			"allocated_bytes": sample.allocated_bytes,
		})).collect::<Vec<_>>(),
	})
}

fn build_row(
	fixture: &Fixture,
	pipeline: Pipeline,
	measured: &Measured,
) -> Row {
	let (extract_median, extract_p95) =
		distributions(&measured.samples, |sample| sample.extract_ms);
	let (first_median, first_p95) =
		distributions(&measured.samples, |sample| {
			sample.first_viewport_layout_ms
		});
	let (full_median, full_p95) =
		distributions(&measured.samples, |sample| sample.full_layout_ms);
	let (alloc_median, alloc_p95) =
		numbers(&measured.samples, |sample| sample.allocations);
	let (bytes_median, bytes_p95) =
		numbers(&measured.samples, |sample| sample.allocated_bytes);
	let stats = document_stats(measured.produced.document());
	Row {
		fixture: fixture.name.to_owned(),
		kind: if fixture.capability {
			"capability"
		} else {
			"equivalence"
		},
		input_bytes: fixture.html.len(),
		digest: sha256(fixture.html.as_bytes()),
		pipeline: pipeline.key(),
		top_level_blocks: measured.produced.document().blocks().len(),
		total_blocks: block_count(measured.produced.document()),
		extract_median,
		extract_p95,
		first_median,
		first_p95,
		full_median,
		full_p95,
		alloc_median,
		alloc_p95,
		bytes_median,
		bytes_p95,
		peak_rss_kb: measured.peak_rss_kb,
		samples: measured.samples.clone(),
		memory: memory_report(&measured.produced, &measured.snapshot, &stats),
		body_semantics: serde_json::to_value(normalize(
			measured.produced.document(),
			measured.produced.title(),
		))
		.unwrap(),
		reading_text_sha256: sha256(
			measured.snapshot.full_reading_text().as_bytes(),
		),
	}
}

fn percent_change(old: f64, new: f64) -> f64 {
	if old == 0.0 {
		if new == 0.0 { 0.0 } else { f64::INFINITY }
	} else {
		(new - old) / old * 100.0
	}
}

fn verdict(old: &Row, new: &Row, capability: bool) -> Value {
	let allocation_pass = new.alloc_median <= old.alloc_median
		&& new.bytes_median <= old.bytes_median;
	let rss_pass = new.peak_rss_kb as f64 <= old.peak_rss_kb as f64 * 1.05;
	let extract_pass = new.extract_median <= old.extract_median * 1.05;
	let first_pass = new.first_p95 <= old.first_p95 * 1.10;
	let full_pass = new.full_p95 <= old.full_p95 * 1.10;
	let status = if capability {
		"capability_difference"
	} else if allocation_pass
		&& rss_pass
		&& extract_pass
		&& first_pass
		&& full_pass
	{
		"pass"
	} else {
		"open_finding"
	};
	json!({
		"fixture": old.fixture,
		"fixture_kind": old.kind,
		"status": status,
		"threshold_applicable": !capability,
		"note": if capability {
			"not an equivalence threshold: the old pipeline has no table grid, caption, row groups or spans, so its cost is not the same semantic work"
		} else {
			"equivalence fixture; allocation, RSS and timing thresholds apply"
		},
		"allocation_not_increased": allocation_pass,
		"peak_rss_no_regression_over_5pct": rss_pass,
		"extract_convert_median_no_regression_over_5pct": extract_pass,
		"first_viewport_layout_p95_no_regression_over_10pct": first_pass,
		"full_layout_p95_no_regression_over_10pct": full_pass,
		"peak_rss_change_pct": percent_change(old.peak_rss_kb as f64, new.peak_rss_kb as f64),
		"extract_convert_median_change_pct": percent_change(old.extract_median, new.extract_median),
		"first_viewport_layout_p95_change_pct": percent_change(old.first_p95, new.first_p95),
		"full_layout_p95_change_pct": percent_change(old.full_p95, new.full_p95),
	})
}

fn cpu_model() -> String {
	std::fs::read_to_string("/proc/cpuinfo")
		.ok()
		.and_then(|text| {
			text.lines().find_map(|line| {
				line.strip_prefix("model name")?
					.split_once(':')?
					.1
					.trim()
					.to_owned()
					.into()
			})
		})
		.unwrap_or_else(|| "unknown".into())
}

fn memory_total_kb() -> u64 {
	std::fs::read_to_string("/proc/meminfo")
		.ok()
		.and_then(|text| {
			text.lines().find_map(|line| {
				line.strip_prefix("MemTotal:")?
					.split_whitespace()
					.next()?
					.parse()
					.ok()
			})
		})
		.unwrap_or(0)
}

fn command_text(program: &str, args: &[&str]) -> String {
	Command::new(program)
		.args(args)
		.output()
		.ok()
		.map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
		.unwrap_or_else(|| "unavailable".into())
}

fn git_state() -> (String, bool) {
	let head = Command::new("git")
		.args(["rev-parse", "HEAD"])
		.output()
		.ok()
		.filter(|output| output.status.success())
		.map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
		.unwrap_or_else(|| "unknown".into());
	let dirty = Command::new("git")
		.args(["status", "--porcelain"])
		.output()
		.ok()
		.map(|output| !output.stdout.is_empty())
		.unwrap_or(false);
	(head, dirty)
}

fn environment(fixtures: &[Fixture]) -> Value {
	let (head, dirty) = git_state();
	let font_files = [
		(
			"NotoSerif-Regular-subset.otf",
			include_bytes!(
				"../crates/markview-layout/tests/fonts/NotoSerif-Regular-subset.otf"
			)
			.as_slice(),
		),
		(
			"NotoSans-Regular-subset.otf",
			include_bytes!(
				"../crates/markview-layout/tests/fonts/NotoSans-Regular-subset.otf"
			)
			.as_slice(),
		),
		(
			"NotoSansMono-Regular-subset.otf",
			include_bytes!(
				"../crates/markview-layout/tests/fonts/NotoSansMono-Regular-subset.otf"
			)
			.as_slice(),
		),
	];
	json!({
		"command": COMMAND,
		"rustc": command_text("rustc", &["-V"]),
		"rustc_verbose": command_text("rustc", &["-Vv"]),
		"cargo": command_text("cargo", &["-V"]),
		"profile": if cfg!(debug_assertions) { "dev" } else { "release" },
		"features": if cfg!(feature = "parallel-layout") {
			vec!["default", "parallel-layout"]
		} else {
			vec!["default"]
		},
		"cpu": cpu_model(),
		"memory_total_kb": memory_total_kb(),
		"fonts": font_files.iter().map(|(name, bytes)| json!({
			"name": name,
			"sha256": sha256(bytes),
		})).collect::<Vec<_>>(),
		"fixtures": fixtures.iter().map(|fixture| json!({
			"name": fixture.name,
			"input_bytes": fixture.html.len(),
			"sha256": sha256(fixture.html.as_bytes()),
			"kind": if fixture.capability { "capability" } else { "equivalence" },
		})).collect::<Vec<_>>(),
		"git_head": head,
		"git_dirty": dirty,
	})
}

fn harness_metadata(fixtures: &[Fixture]) -> Value {
	json!({
		"iterations": ITERATIONS,
		"viewport": {"width": 640, "height": 800, "scroll": 0},
		"warmup_iterations": WARMUP,
		"baseline_kind": "old_extraction_pipeline_replay",
		"true_pre_refactor_baseline": false,
		"baseline_note": "The old pipeline is replayed on the current model; it is not a build of the pre-refactor revision.",
		"allocation_counter_scope": "per iteration: reset immediately before extraction and read immediately after extraction, before layout",
		"peak_rss_method": "each fixture/pipeline runs in a minimal webpage_rss child with one input, after warmup and one measured pass; report/normalizer code is excluded; the child reads VmHWM from /proc/self/status, including warmup and allocator retention; the previous iteration result is dropped before each pass",
		"phases": {
			"extract_convert": "HTML input through extraction and Document construction",
			"first_viewport_layout": "extract_convert plus progressive layout until prefix height covers an 800px viewport at scroll 0, or the document is complete; CPU layout readiness, not GPU presentation",
			"full_layout": "extract_convert plus progressive layout until is_complete() is true; closing body padding included",
		},
		"semantic_comparison": "leading generated title/source envelope blocks are excluded; body block kinds, field text, links/occurrences, images and table grids are compared",
		"semantic_normalization": [
			"a code field's single terminal newline is ignored: Markdown fenced blocks carry one, HTML pre/code need not",
			"unspecified and left column alignment compare equal: the old Markdown separator always emits left, while direct HTML lowering keeps unspecified",
			"adjacent inline runs sharing one LinkOccurrenceId count as one link occurrence",
		],
		"environment": environment(fixtures),
	})
}

fn write_json(name: &str, value: &Value) {
	let path = PathBuf::from(format!("artifacts/webpage-refactor/{name}.json"));
	std::fs::create_dir_all(path.parent().unwrap()).unwrap();
	std::fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap();
}

fn main() {
	let args: Vec<String> = std::env::args().collect();
	if args.get(1).is_some_and(|arg| arg == "--child") {
		child_main(&args[2], &args[3]);
		return;
	}
	let fixtures = fixtures();
	let options = layout_options();
	let mut old_rows = Vec::new();
	let mut new_rows = Vec::new();
	let mut semantic = Vec::new();
	let mut verdicts = Vec::new();
	for fixture in &fixtures {
		let old = measure(fixture, Pipeline::OldReplay, &options);
		let old_normalized =
			normalize(old.produced.document(), old.produced.title());
		let old_row = build_row(fixture, Pipeline::OldReplay, &old);
		drop(old);
		let new = measure(fixture, Pipeline::NewDirect, &options);
		let new_normalized =
			normalize(new.produced.document(), new.produced.title());
		let differences = semantic_diff(&old_normalized, &new_normalized);
		let status = if differences.is_empty() {
			"equivalent"
		} else if fixture.capability {
			"capability_difference"
		} else {
			"different"
		};
		semantic.push(json!({
			"fixture": fixture.name,
			"fixture_kind": if fixture.capability { "capability" } else { "equivalence" },
			"status": status,
			"equivalent": differences.is_empty(),
			"differences": differences,
			"note": if fixture.capability && !differences.is_empty() {
				"old pipeline loss recorded separately: the Markdown round trip collapses the complex table into paragraphs and drops its caption, row groups, spans and cell blocks"
			} else {
				"normalized body semantics"
			},
		}));
		let new_row = build_row(fixture, Pipeline::NewDirect, &new);
		verdicts.push(verdict(&old_row, &new_row, fixture.capability));
		old_rows.push(old_row);
		new_rows.push(new_row);
	}
	let metadata = harness_metadata(&fixtures);
	let baseline = json!({
		"harness": "webpage_bench",
		"baseline": "old_extraction_pipeline_replay",
		"true_pre_refactor_baseline": false,
		"metadata": metadata,
		"samples": old_rows.iter().map(row_json).collect::<Vec<_>>(),
	});
	let after = json!({
		"harness": "webpage_bench",
		"pipeline": "new_direct_lowering",
		"metadata": metadata,
		"samples": new_rows.iter().map(row_json).collect::<Vec<_>>(),
		"semantic_comparison": semantic,
		"threshold_verdicts": verdicts,
	});
	write_json("replay", &baseline);
	write_json("after", &after);
	let stdout = std::io::stdout();
	let mut stdout = stdout.lock();
	writeln!(stdout, "{}", serde_json::to_string_pretty(&after).unwrap())
		.unwrap();
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn first_viewport_requires_body_after_the_short_title() {
		let fixture = Fixture {
			name: "viewport",
			html: format!("<html><head><title>Viewport</title></head><body><article><h1>Viewport</h1>{}</article></body></html>", "<p>A short title cannot fill the viewport. Continue reading enough body paragraphs to cover the first screen.</p>".repeat(30)),
			capability: false,
		};
		let (_, sample, _) =
			run_iteration(&fixture, Pipeline::NewDirect, &layout_options());
		assert!(sample.first_viewport_blocks > 1);
		assert!(sample.first_viewport_height >= 800.);
		assert!(sample.first_viewport_layout_ms < sample.full_layout_ms);
	}
}
