use markview_core::{
	document,
	fonts::FontConfig,
	layout::{LayoutEngine, LayoutOptions},
	search::{SearchIndex, SearchOptions},
};
use std::{collections::HashMap, sync::Arc};
fn options() -> LayoutOptions {
	LayoutOptions {
		width: 320.0,
		fonts: FontConfig {
			ignore_system_fonts: true,
			directories: vec![
				std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
					.join("tests/fonts"),
			],
			..Default::default()
		},
		..Default::default()
	}
}
#[test]
fn fields_survive_disclosure_reflow_and_repeated_semantic_blocks() {
	let doc = document::parse(
		"repeat **needle**\n\nrepeat **needle**\n\n<details>\n<summary>needle</summary>\n\n<details>\n<summary>inner</summary>\n\nhidden needle\n\n</details>\n</details>\n\n| needle | needle |\n|---|---|\n| needle | needle |\n",
	);
	let index = SearchIndex::new(&doc);
	let hits = index.find("needle", SearchOptions::default());
	assert_eq!(hits.len(), 8);
	let mut opts = options();
	let mut engine = LayoutEngine::new();
	let collapsed = engine.layout(&doc, &opts);
	assert!(collapsed.search_selection(&hits[3], 7).is_none());
	for id in hits[3].enclosing.iter() {
		Arc::make_mut(&mut opts.details_open).insert(*id, true);
	}
	let expanded = engine.layout(&doc, &opts);
	for hit in &hits {
		let selection = expanded.search_selection(hit, 7).unwrap();
		assert_eq!(expanded.extract_text(selection, 7), "needle");
		assert!(
			!expanded
				.selection_rects(selection, &HashMap::new(), 7)
				.is_empty()
		);
	}
	assert_ne!(
		expanded.search_selection(&hits[0], 7),
		expanded.search_selection(&hits[1], 7)
	);
	opts.width = 180.0;
	let narrow = engine.layout(&doc, &opts);
	for hit in &hits {
		assert_eq!(
			narrow.extract_text(narrow.search_selection(hit, 8).unwrap(), 8),
			"needle"
		);
	}
	let new_doc = document::parse(format!("unrelated\n\n{}", doc.source));
	let new_hits =
		SearchIndex::new(&new_doc).find("needle", SearchOptions::default());
	let shifted = engine.layout(
		&new_doc,
		&LayoutOptions {
			force_open: true,
			..opts
		},
	);
	for hit in &new_hits {
		assert_eq!(
			shifted.extract_text(shifted.search_selection(hit, 9).unwrap(), 9),
			"needle"
		);
	}
}
#[test]
fn semantic_projection_excludes_diagnostics_and_preserves_math_and_alt_text() {
	let doc = document::parse(
		"text $\\unknowncommand$ tail\n\n![needle](missing.png)\n\nabc $$x+y$$ def\n\n[link **needle**](https://hidden)\n",
	);
	let index = SearchIndex::new(&doc);
	assert!(
		index
			.find("Math error", SearchOptions::default())
			.is_empty()
	);
	assert!(
		index
			.find("missing.png", SearchOptions::default())
			.is_empty()
	);
	assert!(
		index
			.find("https://hidden", SearchOptions::default())
			.is_empty()
	);
	let snapshot = LayoutEngine::new().layout(&doc, &options());
	for query in ["\\unknowncommand", "needle", "x+y"] {
		for hit in index.find(query, SearchOptions::default()) {
			let selection = snapshot.search_selection(&hit, 1).unwrap();
			assert!(
				!snapshot
					.selection_rects(selection, &HashMap::new(), 1)
					.is_empty()
			);
		}
	}
	let mut projected = Vec::new();
	snapshot.visit_search_clusters(
		&HashMap::new(),
		0.0..snapshot.height,
		|bi, field, range, _| projected.push((bi, field, range)),
	);
	let math = index
		.find("\\unknowncommand", SearchOptions::default())
		.remove(0);
	assert!(
		projected
			.iter()
			.filter(|(_, f, _)| *f == math.field)
			.all(|(_, _, r)| r.end <= 25)
	);
	let full = snapshot.select_all(1).unwrap();
	let copied = snapshot.extract_text(full, 1);
	assert!(copied.contains("Math error"));
}
#[test]
fn visible_clusters_survive_multiline_image_placeholders() {
	let doc = document::parse(
		"before ![long alternative text that wraps onto several lines](missing.png) needle after",
	);
	for error in [None, Some("Image could not be loaded".to_owned())] {
		let mut opts = options();
		opts.width = 640.0;
		let mut images = markview_core::image::ImageSnapshot::default();
		images.entries.insert(
			"missing.png".to_owned(),
			markview_core::image::ImageInfo {
				error,
				..Default::default()
			},
		);
		let snapshot =
			LayoutEngine::new().layout_with_images(&doc, &opts, &images);
		assert!(snapshot.blocks.iter().any(|block| {
			block.layout.text.iter().any(|node| {
				node.clusters
					.windows(2)
					.any(|pair| pair[0].rect.y > pair[1].rect.y)
			})
		}));
		let horizontal = HashMap::new();
		let mut all = Vec::new();
		snapshot.visit_search_clusters(
			&horizontal,
			0.0..snapshot.height,
			|bi, field, range, rect| all.push((bi, field, range, rect)),
		);
		for (_, _, _, rect) in &all {
			let visible = rect.y..rect.y + 1.0;
			let expected: Vec<_> = all
				.iter()
				.filter(|(_, _, _, r)| {
					r.y + r.h >= visible.start && r.y <= visible.end
				})
				.map(|(bi, field, range, _)| (*bi, *field, range.clone()))
				.collect();
			let mut actual = Vec::new();
			snapshot.visit_search_clusters(
				&horizontal,
				visible.clone(),
				|bi, field, range, _| actual.push((bi, field, range)),
			);
			assert_eq!(actual, expected, "visible range {visible:?}");
		}
	}
}
#[test]
fn cancellation_and_matches_cross_chunk_boundaries_without_normalization() {
	let text = format!("{}Kneedle{}", "x".repeat(65535), "x".repeat(65536));
	let doc = document::parse(text.as_str());
	let index = SearchIndex::new(&doc);
	assert_eq!(index.find("kneedle", SearchOptions::default()).len(), 1);
	let calls = std::cell::Cell::new(0);
	assert!(
		index
			.find_cancellable("absent", SearchOptions::default(), || {
				calls.set(calls.get() + 1);
				calls.get() > 2
			})
			.is_none()
	);
	assert!(SearchIndex::new_cancellable(&doc, || true).is_none());
}
#[test]
fn contiguous_scan_preserves_field_boundaries_and_literal_order() {
	let paragraphs =
		["ab", "aba", "K", "kΣ", "σ", "aaaaab", "e\u{301}", "中文"];
	let doc = document::parse(paragraphs.join("\n\n"));
	let index = SearchIndex::new(&doc);
	for case_sensitive in [false, true] {
		for whole_word in [false, true] {
			let options = SearchOptions {
				case_sensitive,
				whole_word,
			};
			for query in [
				"aba", "aaab", "aa", "kk", "kσ", "σ", "Σσ", "be", "e\u{301}",
				"文", "missing",
			] {
				let regex = regex::RegexBuilder::new(&regex::escape(query))
					.case_insensitive(!case_sensitive)
					.build()
					.unwrap();
				let mut expected = Vec::new();
				for (block, text) in paragraphs.iter().enumerate() {
					let boundaries: Vec<_> =
						icu_segmenter::WordSegmenter::new_auto(
							Default::default(),
						)
						.segment_str(text)
						.collect();
					for hit in regex.find_iter(text) {
						if !whole_word
							|| (boundaries.contains(&hit.start())
								&& boundaries.contains(&hit.end()))
						{
							expected.push((block, hit.range()));
						}
					}
				}
				let actual: Vec<_> = index
					.find(query, options)
					.into_iter()
					.map(|hit| (hit.block, hit.range))
					.collect();
				assert_eq!(
					actual, expected,
					"query {query:?}, options {options:?}"
				);
			}
		}
	}
	let table = SearchIndex::new(&document::parse(
		"| ab | aba | |\n|---|---|---|\n| K | kΣ | σ |",
	));
	assert_eq!(table.find("aba", SearchOptions::default()).len(), 1);
	assert!(table.find("Σσ", SearchOptions::default()).is_empty());
	let code = SearchIndex::new(&document::parse("```\nab\naba\n```"));
	assert_eq!(code.find("ab\naba", SearchOptions::default()).len(), 1);
}

#[test]
#[ignore = "large-document timing and index memory report"]
fn large_document_search_measurement() {
	let source =
		"A paragraph with **needle**, 中文文字 and a link.\n\n".repeat(100000);
	let doc = document::parse(source);
	let start = std::time::Instant::now();
	let index = SearchIndex::new(&doc);
	let build = start.elapsed();
	let start = std::time::Instant::now();
	let matches = index.find("needle", SearchOptions::default());
	assert_eq!(matches.len(), 100000);
	eprintln!(
		"source={} text={} index={} build={:?} query={:?} matches={}",
		doc.source.len(),
		index.text_bytes(),
		index.memory_bytes(),
		build,
		start.elapsed(),
		matches.len()
	);
	for query in ["n", "need", "absent"] {
		for whole_word in [false, true] {
			let start = std::time::Instant::now();
			let matches = index.find(
				query,
				SearchOptions {
					whole_word,
					..Default::default()
				},
			);
			eprintln!(
				"query={query:?} whole_word={whole_word} elapsed={:?} matches={}",
				start.elapsed(),
				matches.len()
			);
		}
	}
	let start = std::time::Instant::now();
	assert!(
		index
			.find_cancellable("needle", SearchOptions::default(), || true)
			.is_none()
	);
	eprintln!("cancel={:?}", start.elapsed());
}
