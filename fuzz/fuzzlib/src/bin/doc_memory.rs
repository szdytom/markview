//! Before/after evidence: `doc_memory <output-directory> [expected-directory]`.
use markview_document::{Block, BlockKind, Inline, InlineKind};
use markview_layout::{
	layout::LayoutOptions,
	scene::{Draw, Glyph},
	text::TextCluster,
};
use mvfuzz::{allocator::GLOBAL, behavior, pipeline};
use serde_json::json;
use std::{fs, path::PathBuf, time::Instant};
fn main() {
	let mut args = std::env::args().skip(1);
	let first = args.next().expect("output directory");
	let records_only = first == "--records-only";
	let out = PathBuf::from(if records_only {
		args.next().expect("output directory")
	} else {
		first
	});
	let expected = args.next().map(PathBuf::from);
	fs::create_dir_all(&out).unwrap();
	if records_only {
		pipeline::warmup();
		let differences = write_records(&out, expected.as_deref());
		if !differences.is_empty() {
			eprintln!("{}", differences.join("\n"));
			std::process::exit(1);
		}
		return;
	}
	let fonts = pipeline::pinned_fonts();
	let mut font_files: Vec<_> = fs::read_dir(&fonts.directories[0])
		.unwrap()
		.map(|e| e.unwrap().path())
		.collect();
	font_files.sort();
	let manifest = json!({"format":1,"toolchain":String::from_utf8(std::process::Command::new("rustc").arg("-Vv").output().unwrap().stdout).unwrap(),"lock":markview_document::stable_fingerprint(&fs::read(concat!(env!("CARGO_MANIFEST_DIR"),"/../Cargo.lock")).unwrap()),"fonts":font_files.iter().map(|p|json!([p.file_name().unwrap().to_str().unwrap(),markview_document::stable_fingerprint(&fs::read(p).unwrap())])).collect::<Vec<_>>(),"width":720,"resize_width":660,"first_screen_height":800,"theme":"bundled light; CJK SC","highlight":"synchronous","images":"unloaded"});
	fs::write(
		out.join("environment.json"),
		serde_json::to_vec_pretty(&manifest).unwrap(),
	)
	.unwrap();
	drop(manifest);
	drop(font_files);
	pipeline::warmup();

	let mut report = Vec::new();
	for (name, source) in behavior::corpora() {
		let mut sheet =
			(*markview_layout::style::Stylesheet::bundled(false)).clone();
		sheet.set_cjk_type(markview_layout::style::CjkType::Sc);
		let options = LayoutOptions {
			stylesheet: std::sync::Arc::new(sheet),
			width: 720.,
			fonts: pipeline::pinned_fonts(),
			..Default::default()
		};
		for iteration in 0..5 {
			let base = GLOBAL.open_window();
			let count = GLOBAL.allocations();
			let base_live_allocations = GLOBAL.live_allocations();
			let start = Instant::now();
			let doc = markview_commonmark::parse(source.clone());
			let parse_us = start.elapsed().as_micros();
			let document_bytes = GLOBAL.current() - base;
			let mut engine = pipeline::differential_engine();
			let start = Instant::now();
			let mut first_screen_us = None;
			let old = engine
				.layout_progressive(
					&doc,
					&options,
					&Default::default(),
					|snapshot| {
						if first_screen_us.is_none()
							&& (snapshot.height >= 800.
								|| snapshot.blocks.len() == doc.blocks().len())
						{
							first_screen_us = Some(start.elapsed().as_micros());
						}
						true
					},
				)
				.unwrap();
			let cold_us = start.elapsed().as_micros();
			let first_screen_us = first_screen_us.unwrap_or(cold_us);
			let cold_live = GLOBAL.current() - base;
			let cold_peak = GLOBAL.window_peak() - base;
			let cold_allocations = GLOBAL.allocations() - count;
			GLOBAL.open_window();
			let start = Instant::now();
			let warm = engine.layout(&doc, &options);
			let warm_us = start.elapsed().as_micros();
			let coexist = GLOBAL.current() - base;
			let coexist_peak = GLOBAL.window_peak() - base;
			drop(old);
			let after_old_drop = GLOBAL.current() - base;
			let live_allocations =
				GLOBAL.live_allocations() - base_live_allocations;
			GLOBAL.open_window();
			let start = Instant::now();
			let resized = engine.layout(
				&doc,
				&LayoutOptions {
					width: 660.,
					..options.clone()
				},
			);
			let resize_us = start.elapsed().as_micros();
			let resize_coexist_bytes = GLOBAL.current() - base;
			let resize_peak_bytes = GLOBAL.window_peak() - base;
			drop(resized);
			let storage = storage_counts(&warm);
			let draw_storage = draw_storage(&warm);

			drop(warm);
			engine.release_document();
			drop(engine);
			let markview_commonmark::MarkdownParse {
				document,
				source: original,
				map,
				..
			} = doc;
			let before = GLOBAL.current();
			drop(document);
			let semantic_tree_index_text_bytes = before - GLOBAL.current();
			let before = GLOBAL.current();
			drop(map);
			let source_map_bytes = before - GLOBAL.current();
			let before = GLOBAL.current();
			drop(original);
			let original_source_bytes = before - GLOBAL.current();
			let after_release_bytes = GLOBAL.current().saturating_sub(base);
			report.push(json!({"corpus":name,"iteration":iteration,"source_bytes":source.len(),"semantic_tree_index_text_bytes":semantic_tree_index_text_bytes,"source_map_bytes":source_map_bytes,"original_source_bytes":original_source_bytes,"storage_counts":storage,"draw_storage":draw_storage,"document_bytes":document_bytes,"cold_live_bytes":cold_live,"cold_peak_bytes":cold_peak,"cold_allocations":cold_allocations,"coexist_bytes":coexist,"coexist_peak_bytes":coexist_peak,"after_old_drop_bytes":after_old_drop,"after_release_bytes":after_release_bytes,"live_allocations":live_allocations,"parse_us":parse_us,"cold_us":cold_us,"first_screen_us":first_screen_us,"warm_us":warm_us,"resize_us":resize_us,"resize_coexist_bytes":resize_coexist_bytes,"resize_peak_bytes":resize_peak_bytes}));
		}
	}
	fs::write(
		out.join("breakdown.json"),
		serde_json::to_vec_pretty(&profile()).unwrap(),
	)
	.unwrap();
	// Export only after every allocator window has closed.
	let differences = write_records(&out, expected.as_deref());
	let sizes = json!({"inline":size_of::<Inline>(),"inline_kind":size_of::<InlineKind>(),"block":size_of::<Block>(),"block_kind":size_of::<BlockKind>(),"draw":size_of::<Draw>(),"glyph":size_of::<Glyph>(),"cluster":size_of::<TextCluster>()});
	fs::write(
		out.join("memory.json"),
		serde_json::to_vec_pretty(
			&json!({"format":1,"sizes":sizes,"candidate_sizes":candidates::sizes(),"samples":report}),
		)
		.unwrap(),
	)
	.unwrap();
	if !differences.is_empty() {
		eprintln!("{}", differences.join("\n"));
		std::process::exit(1);
	}
}

fn write_records(
	out: &std::path::Path,
	expected: Option<&std::path::Path>,
) -> Vec<String> {
	let mut differences = Vec::new();
	for (name, record) in behavior::records() {
		fs::write(
			out.join(format!("{name}.json")),
			serde_json::to_vec_pretty(&record).unwrap(),
		)
		.unwrap();
		if let Some(expected) = expected {
			let previous = serde_json::from_slice(
				&fs::read(expected.join(format!("{name}.json"))).unwrap(),
			)
			.unwrap();
			if let Some(diff) = behavior::difference(&previous, &record, &name)
			{
				fs::write(out.join(format!("{name}.diff")), &diff).unwrap();
				differences.push(diff);
			}
		}
	}
	differences
}

/// Unique retained array lengths/capacities, excluding resources and scene-tree internals.
fn storage_counts(
	snapshot: &markview_layout::scene::LayoutSnapshot,
) -> [usize; 6] {
	let mut seen_draws = std::collections::HashSet::new();
	let mut seen_text = std::collections::HashSet::new();
	let mut counts = [0; 6];
	for block in &snapshot.blocks {
		let b = &block.layout;
		if seen_draws.insert(b.draws.as_ptr()) {
			counts[0] += b.draws.len();
			counts[1] += b.draws.capacity();
		}
		if seen_text.insert(b.text.as_ptr()) {
			counts[2] += b.text.len();
			counts[3] += b.text.capacity();
			for node in &b.text {
				counts[4] += node.clusters.len();
				counts[5] += node.clusters.capacity();
			}
		}
	}
	counts
}

// The pre-optimization checkout stores unpacked `Vec<Draw>` slots.
#[allow(dead_code)]
trait LegacyDrawingStorage {
	fn packed_counts(&self) -> Option<(usize, usize, usize)> {
		None
	}
	fn storage_bytes(&self) -> usize;
}
impl LegacyDrawingStorage for Vec<Draw> {
	fn storage_bytes(&self) -> usize {
		self.capacity() * size_of::<Draw>()
	}
}
fn draw_storage(
	snapshot: &markview_layout::scene::LayoutSnapshot,
) -> [usize; 4] {
	let mut seen = std::collections::HashSet::new();
	let mut counts = [0; 4];
	for block in &snapshot.blocks {
		let draws = &block.layout.draws;
		if seen.insert(draws.as_ptr()) {
			let (commands, glyphs, styles) =
				draws.packed_counts().unwrap_or((draws.len(), 0, 0));
			counts[0] += commands;
			counts[1] += glyphs;
			counts[2] += styles;
			counts[3] += draws.storage_bytes();
		}
	}
	counts
}

/// Profiling is a separate pass, so traversing storage never changes the measured peaks or latency.
fn profile() -> serde_json::Value {
	use std::collections::{BTreeMap, HashSet};
	let mut result = Vec::new();
	for (name, source) in behavior::corpora() {
		let doc = markview_commonmark::parse(source);
		let mut sheet =
			(*markview_layout::style::Stylesheet::bundled(false)).clone();
		sheet.set_cjk_type(markview_layout::style::CjkType::Sc);
		let options = LayoutOptions {
			width: 720.,
			fonts: pipeline::pinned_fonts(),
			stylesheet: std::sync::Arc::new(sheet),
			..Default::default()
		};
		let mut engine = pipeline::differential_engine();
		let first = engine.layout(&doc, &options);
		let warm = engine.layout(&doc, &options);
		let resized = engine.layout(
			&doc,
			&LayoutOptions {
				width: 660.,
				..options
			},
		);
		let mut seen = HashSet::new();
		let mut categories: BTreeMap<&str, [usize; 4]> = BTreeMap::new();
		let mut visit = |identity: usize,
		                 category: &'static str,
		                 bytes: usize,
		                 len: usize,
		                 capacity: usize| {
			if seen.insert(identity) {
				let c = categories.entry(category).or_default();
				c[0] += bytes;
				c[1] += len;
				c[2] += capacity;
				c[3] += 1;
			}
		};
		doc.document.visit_storage(&mut visit);
		doc.map.visit_storage(&mut visit);
		visit(
			doc.source.as_ptr() as usize,
			"original_source",
			doc.source.len() + 2 * size_of::<usize>(),
			doc.source.len(),
			doc.source.len(),
		);
		for snapshot in [&first, &warm, &resized] {
			snapshot.visit_storage(&mut visit);
		}
		engine.visit_storage(&mut visit);
		let mut fonts = HashSet::new();
		let mut font_resource_bytes = 0;
		for snapshot in [&first, &warm, &resized] {
			for block in &snapshot.blocks {
				for draw in &block.layout.draws {
					if let Draw::Glyph(g) = draw
						&& fonts.insert(g.font.data.data().as_ptr())
					{
						font_resource_bytes += g.font.data.data().len();
					}
				}
			}
		}
		result.push(json!({"corpus":name,"categories":categories,"font_resource_bytes":font_resource_bytes,"font_resources":fonts.len(),"decoded_image_bytes":0}));
	}
	json!({"format":1,"units":"categories: [payload bytes, length, capacity, unique allocations]; source + document + engine cache + cold/warm/resize snapshots deduplicated by allocation identity","coverage":"known arrays, strings and shared allocation headers; hash/B-tree buckets, shaper and highlight caches, math/image payloads and allocator bookkeeping are excluded from this breakdown; memory.json allocator totals include all Rust heap allocations","samples":result})
}

// Candidate layouts are measured without adding allocations to production.
#[allow(dead_code)]
mod candidates {
	use markview_document::*;
	macro_rules! block_kind {
		($name:ident,$table:ty,$callout:ty) => {
			enum $name {
				Paragraph(RichText),
				Heading {
					level: u8,
					text: RichText,
					anchor: String,
				},
				Code {
					language: String,
					text: String,
				},
				Quote {
					callout: Option<$callout>,
					blocks: Vec<Block>,
				},
				List {
					start: Option<isize>,
					reversed: bool,
					tight: bool,
					items: Vec<ListItem>,
				},
				Table($table),
				Footnote {
					id: FootnoteId,
					number: u32,
					blocks: Vec<Block>,
				},
				Details {
					default_open: bool,
					summary: RichText,
					blocks: Vec<Block>,
				},
				Rule,
				FrontMatter {
					default_open: bool,
					blocks: Vec<Block>,
				},
				DefinitionList(Vec<DefinitionItem>),
				Figure {
					blocks: Vec<Block>,
					caption: Option<RichText>,
				},
			}
		};
	}

	block_kind!(BoxedTableKind, Box<Table>, Callout);
	block_kind!(BoxedCalloutKind, Table, Box<Callout>);
	block_kind!(BoxedContainerKind, Box<Table>, Box<Callout>);

	enum BoxedReferenceKind {
		Anchor(String),
		Text(String),
		Image(Box<ImageSpec>),
		Math { latex: String, display: bool },
		FootnoteRef(Box<FootnoteReference>),
		LineBreak(LineBreak),
	}
	enum BoxedMathKind {
		Anchor(String),
		Text(String),
		Image(Box<ImageSpec>),
		Math(Box<(String, bool)>),
		FootnoteRef(FootnoteReference),
		LineBreak(LineBreak),
	}

	pub fn sizes() -> serde_json::Value {
		serde_json::json!({"boxed_table_block_kind":size_of::<BoxedTableKind>(),"boxed_callout_block_kind":size_of::<BoxedCalloutKind>(),"boxed_both_block_kind":size_of::<BoxedContainerKind>(),"boxed_reference_inline_kind":size_of::<BoxedReferenceKind>(),"boxed_math_inline_kind":size_of::<BoxedMathKind>(),"table_payload":size_of::<Table>(),"callout_payload":size_of::<Callout>(),"footnote_payload":size_of::<FootnoteReference>()})
	}
}
