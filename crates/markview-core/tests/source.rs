use markview_core::{
	document,
	fonts::FontConfig,
	layout::{LayoutEngine, LayoutOptions},
	source::{SourceIndex, byte_to_utf16, utf16_to_byte},
};
use std::sync::Arc;

fn options() -> LayoutOptions {
	LayoutOptions {
		width: 220.,
		fonts: FontConfig::from_faces(
			0x736f75726365,
			[
				include_bytes!("fonts/NotoSerif-Regular-subset.otf").as_slice(),
				include_bytes!("fonts/NotoSerifCJKsc-Regular-subset.otf")
					.as_slice(),
				include_bytes!("fonts/NotoSans-Regular-subset.otf").as_slice(),
				include_bytes!("fonts/NotoSansMono-Regular-subset.otf")
					.as_slice(),
			]
			.into_iter()
			.map(|data| parley::fontique::Blob::new(Arc::new(data)))
			.collect(),
		),
		..Default::default()
	}
}

#[test]
fn original_unicode_coordinates_preserve_crlf_and_surrogate_boundaries() {
	let source = "中文😀e\u{301}\r\nlast";
	for (byte, _) in source
		.char_indices()
		.chain(std::iter::once((source.len(), '\0')))
	{
		assert_eq!(utf16_to_byte(source, byte_to_utf16(source, byte)), byte);
	}
	assert_eq!(utf16_to_byte(source, 3), "中文".len());
	assert_eq!(byte_to_utf16(source, "中文😀e\u{301}\r\n".len()), 8);
}

#[test]
fn scroll_anchor_batches_cover_visible_lines_and_complete_atomic_ranges() {
	let svg = "<svg width=\"80\" height=\"1200\">\r\n<rect/>\r\n</svg>";
	let source = format!(
		"# 中文😀\r\n\r\n{}\r\n\r\n{svg}\r\n\r\n<details><summary>closed</summary>\r\n\r\nhidden body\r\n\r\n</details>\r\n\r\n---\r\n\r\nAfter 😀",
		"word ".repeat(160)
	);
	let doc = document::parse(source.clone());
	let index = SourceIndex::new(&doc);
	let mut images = Vec::new();
	for block in &doc.blocks {
		block.images(&mut images);
	}
	let mut resources = markview_core::image::ImageSnapshot::default();
	resources.entries.insert(
		images[0].src.clone(),
		markview_core::image::ImageInfo {
			version: 1,
			size: Some((80, 1200)),
			error: None,
		},
	);
	let snapshot =
		LayoutEngine::new().layout_with_images(&doc, &options(), &resources);
	let anchors = index.scroll_anchors(&snapshot, &Default::default(), 0);
	for text in ["中文😀", "word", svg, "After 😀", "---"] {
		let byte = source.find(text).unwrap();
		let offset = byte_to_utf16(&source, byte);
		let anchor = anchors
			.iter()
			.find(|anchor| anchor.source.contains(&offset))
			.unwrap();
		let point = index
			.source_to_preview(&snapshot, &Default::default(), byte)
			.unwrap();
		assert!(anchor.top <= point.rect.y + 0.01, "{text}: {anchor:?}");
		assert!(
			anchor.bottom >= point.rect.y + point.rect.h - 0.01,
			"{text}: {anchor:?}"
		);
		if text == svg {
			assert_eq!(
				anchor.source,
				offset..byte_to_utf16(&source, byte + svg.len())
			);
			assert!(anchor.bottom - anchor.top >= 1200.);
		}
		if text == "word" {
			assert!(anchor.bottom - anchor.top > 200.);
		}
	}
	let hidden = byte_to_utf16(&source, source.find("hidden body").unwrap());
	assert!(!anchors.iter().any(|anchor| anchor.source.contains(&hidden)));
	assert!(
		anchors
			.windows(2)
			.all(|pair| pair[0].source.end < pair[1].source.start)
	);
}

#[test]
fn scroll_anchors_preserve_original_lines_across_all_markdown_line_endings() {
	let documents = ["\r", "\n", "\r\n"].map(|newline| {
		["# 中文😀", "", "Paragraph e\u{301}", "", "# Two"].join(newline)
	});
	for source in documents.into_iter().chain(std::iter::once(
		"# 中文😀\r\rParagraph e\u{301}\r\n\r\n# Two\n\nLast 😀\r".to_owned(),
	)) {
		let doc = document::parse(source.clone());
		let index = SourceIndex::new(&doc);
		let snapshot = LayoutEngine::new().layout(&doc, &options());
		let anchors = index.scroll_anchors(&snapshot, &Default::default(), 0);
		let expected: Vec<_> =
			["# 中文😀", "Paragraph e\u{301}", "# Two", "Last 😀"]
				.into_iter()
				.filter_map(|line| {
					let start = source.find(line)?;
					Some(
						byte_to_utf16(&source, start)
							..byte_to_utf16(&source, start + line.len()),
					)
				})
				.collect();
		assert_eq!(
			anchors
				.iter()
				.map(|anchor| anchor.source.clone())
				.collect::<Vec<_>>(),
			expected
		);
		assert!(anchors.windows(2).all(|pair| pair[0].bottom < pair[1].top));
	}
}

#[test]
fn text_free_collapsed_containers_anchor_only_their_opening_line() {
	for source in [
		"---\r\ntitle: hidden 中文😀\r\nother: body\r\n---\r\n\r\n# After",
		"<details>\n<summary></summary>\n\nhidden 中文😀 body\n\n</details>\n\n# After",
		"> <details>\r> <summary></summary>\r>\r> hidden 中文😀 body\r>\r> </details>\r\r# After",
	] {
		let doc = document::parse(source);
		let index = SourceIndex::new(&doc);
		let mut engine = LayoutEngine::new();
		let mut opts = options();
		let snapshot = engine.layout(&doc, &opts);
		let anchors = index.scroll_anchors(&snapshot, &Default::default(), 0);
		assert_eq!(anchors.len(), 2);
		assert_eq!(
			anchors[0].source,
			0..byte_to_utf16(source, source.find(['\r', '\n']).unwrap())
		);
		let hidden = byte_to_utf16(source, source.find("hidden").unwrap());
		assert!(!anchors.iter().any(|anchor| anchor.source.contains(&hidden)));
		opts.force_open = true;
		let expanded = engine.layout(&doc, &opts);
		assert!(
			index
				.scroll_anchors(&expanded, &Default::default(), 0)
				.iter()
				.any(|anchor| anchor.source.contains(&hidden))
		);
		let closed = engine.layout(&doc, &options());
		assert_eq!(
			index.scroll_anchors(&closed, &Default::default(), 0)[0].source,
			anchors[0].source
		);
	}
}

#[test]
fn scroll_anchors_exclude_relocated_footnotes_but_preserve_direct_navigation() {
	for body in ["Long footnote 中文😀. ".repeat(100), "---".to_owned()] {
		for early in [true, false] {
			let prose = "Main paragraph.\n\n".repeat(20);
			let note = format!("[^note]: {body}\n\n");
			let source = if early {
				format!("# Start[^note]\n\n{note}{prose}")
			} else {
				format!("# Start[^note]\n\n{prose}{note}")
			};
			let doc = document::parse(source.clone());
			let index = SourceIndex::new(&doc);
			let snapshot = LayoutEngine::new().layout(&doc, &options());
			let anchors =
				index.scroll_anchors(&snapshot, &Default::default(), 0);
			let byte = source
				.find(if body == "---" { "[^note]:" } else { &body })
				.unwrap();
			let offset = byte_to_utf16(&source, byte);
			assert_eq!(
				anchors.iter().any(|anchor| anchor.source.contains(&offset)),
				!early
			);
			assert!(anchors.len() >= 21);
			assert!(
				anchors.windows(2).all(|pair| pair[0].bottom <= pair[1].top)
			);
			let point = index
				.source_to_preview(&snapshot, &Default::default(), byte)
				.unwrap();
			assert!(point.source.contains(&byte), "{point:?}");
			let last_prose = index
				.source_to_preview(
					&snapshot,
					&Default::default(),
					source.rfind("Main paragraph").unwrap(),
				)
				.unwrap();
			assert!(point.rect.y > last_prose.rect.y);
		}
	}
}

#[test]
fn incremental_scroll_anchor_batches_match_a_complete_geometry_traversal() {
	let source = "# Start[^note]\n\n[^note]: 中文😀 footnote.\n\n".to_owned()
		+ &"中文😀 paragraph.\n\n".repeat(120);
	let doc = document::parse(source);
	let index = SourceIndex::new(&doc);
	let mut engine = LayoutEngine::new();
	let mut layout = engine.begin_layout(&doc, &options(), &Default::default());
	let mut blocks = 0;
	let mut incremental = Vec::new();
	while !layout.is_complete() {
		engine.advance(&mut layout, &doc, std::time::Duration::from_millis(1));
		incremental.extend(index.scroll_anchors(
			layout.snapshot(),
			&Default::default(),
			blocks,
		));
		blocks = layout.snapshot().blocks.len();
	}
	let complete =
		index.scroll_anchors(layout.snapshot(), &Default::default(), 0);
	incremental.sort_by_key(|anchor| anchor.source.start);
	assert_eq!(incremental.len(), complete.len());
	for (incremental, complete) in incremental.iter().zip(&complete) {
		assert_eq!(incremental.source, complete.source);
		assert_eq!(
			(incremental.top, incremental.bottom),
			(complete.top, complete.bottom)
		);
	}
	assert!(
		index
			.scroll_anchors(layout.snapshot(), &Default::default(), blocks)
			.is_empty()
	);
}

#[test]
fn mapping_follows_wrapped_lines_nested_cells_and_reused_geometry() {
	let source = format!(
		"# repeat\r\n\r\n{}\r\n\r\n> nested **target** text\r\n\r\n| first | second |\r\n|---|---|\r\n| cell | final |\r\n\r\n```rust\r\nfirst\r\nsecond\r\nthird\r\n```\r\n",
		"word ".repeat(160)
	);
	let doc = document::parse(source.clone());
	let index = SourceIndex::new(&doc);
	let mut engine = LayoutEngine::new();
	let opts = options();
	let snapshot = engine.layout(&doc, &opts);
	let early = index
		.source_to_preview(
			&snapshot,
			&Default::default(),
			source.find("word").unwrap(),
		)
		.unwrap();
	let late_offset = source.rfind("word").unwrap();
	let late = index
		.source_to_preview(&snapshot, &Default::default(), late_offset)
		.unwrap();
	assert!(late.rect.y > early.rect.y + 200.);
	let reverse = index
		.preview_to_source(&snapshot, &Default::default(), late.rect.y + 1.)
		.unwrap();
	assert!(reverse.source.start > source.find("word").unwrap() + 500);
	for text in ["target", "second |", "final", "second\r\n", "third"] {
		let offset = source.find(text).unwrap();
		assert!(
			index
				.source_to_preview(&snapshot, &Default::default(), offset)
				.unwrap()
				.source
				.contains(&offset),
			"{text}"
		);
	}
	let first = index
		.source_to_preview(
			&snapshot,
			&Default::default(),
			source.find("first\r\n").unwrap(),
		)
		.unwrap();
	let third = index
		.source_to_preview(
			&snapshot,
			&Default::default(),
			source.find("third").unwrap(),
		)
		.unwrap();
	assert!(third.rect.y > first.rect.y + 20.);
	let new_source = format!("prefix\n\n{source}");
	let new_doc = document::parse(new_source.clone());
	let new_index = SourceIndex::new(&new_doc);
	let new_snapshot = engine.layout(&new_doc, &opts);
	assert!(new_snapshot.reused > 0);
	let offset = new_source.find("target").unwrap();
	assert!(
		new_index
			.source_to_preview(&new_snapshot, &Default::default(), offset)
			.unwrap()
			.source
			.contains(&offset)
	);
}

#[test]
fn progressive_targets_wait_and_disclosures_use_visible_content() {
	let source = "# same\n\n<details>\n<summary>closed</summary>\n\n### 中文\n\nhidden body\n\n</details>\n\n# same\n\nend";
	let doc = document::parse(source);
	let index = SourceIndex::new(&doc);
	let opts = options();
	let mut engine = LayoutEngine::new();
	let pending = engine.begin_layout(&doc, &opts, &Default::default());
	assert!(
		index
			.source_to_preview(
				pending.snapshot(),
				&Default::default(),
				source.find("end").unwrap()
			)
			.is_none()
	);
	let snapshot = engine.layout(&doc, &opts);
	let hidden = index
		.source_to_preview(
			&snapshot,
			&Default::default(),
			source.find("hidden").unwrap(),
		)
		.unwrap();
	assert!(hidden.source.end <= source.find("hidden").unwrap());
	let outline = doc.outline();
	assert_eq!(outline.len(), 3);
	assert_eq!(outline[0].anchor, "same");
	assert_eq!(outline[2].anchor, "same-1");
	assert_eq!(&source[outline[1].source.clone()], "### 中文");
	assert!(document::parse("plain").outline().is_empty());
}

#[test]
fn disclosure_ranges_cover_original_nested_and_quoted_headings() {
	for source in [
		"prefix\n\n<details>\n<summary>repeat</summary>\n\n## repeat\n\n<details><summary>inner</summary>\n### child\n</details>\n</details>\n",
		"prefix\r\n\r\n> <details>\r\n> <summary>repeat</summary>\r\n>\r\n> ## repeat\r\n>\r\n> </details>\r\n",
		"<details><summary>repeat</summary>\n## repeat\n</details><details><summary>next</summary>\n## repeat\n</details>\n",
		"prefix\r\r> <details>\r> <summary>repeat</summary>\r>\r> ## repeat\r>\r> </details>\r",
	] {
		let doc = document::parse(source);
		let outline = doc.outline();
		for entry in outline {
			let expected = if entry.text == "child" {
				"### child"
			} else {
				"## repeat"
			};
			assert_eq!(&source[entry.source.clone()], expected, "{source}");
		}
		let mut options = options();
		let mut engine = LayoutEngine::new();
		for heading in doc.outline() {
			for id in doc.details_enclosing(&heading.anchor) {
				Arc::make_mut(&mut options.details_open).insert(id, true);
			}
		}
		let snapshot = engine.layout(&doc, &options);
		let index = SourceIndex::new(&doc);
		for heading in doc.outline() {
			let offset =
				heading.source.start + if heading.level == 3 { 4 } else { 3 };
			let mapped = index
				.source_to_preview(&snapshot, &Default::default(), offset)
				.unwrap();
			assert!(
				mapped.source.contains(&offset),
				"offset={offset}, heading={heading:?}, mapped={mapped:?}, source={source}"
			);
		}
	}
}

#[test]
fn adjacent_quoted_disclosures_map_unicode_bodies_and_trailing_text() {
	for newline in ["\n", "\r\n", "\r"] {
		let source = [
			"> > <details><summary>first</summary>",
			"> > # first",
			"> > </details>",
			"> > <details><summary>second</summary>",
			"> > # 中文",
			"> > </details>",
			"> > # 中文 tail",
		]
		.join(newline);
		let doc = document::parse(source.as_str());
		let outline = doc.outline();
		assert_eq!(outline.len(), 3);
		for (heading, expected) in
			outline.iter().zip(["# first", "# 中文", "# 中文 tail"])
		{
			assert_eq!(&source[heading.source.clone()], expected);
		}
		let mut opts = options();
		for heading in &outline {
			for id in doc.details_enclosing(&heading.anchor) {
				Arc::make_mut(&mut opts.details_open).insert(id, true);
			}
		}
		let snapshot = LayoutEngine::new().layout(&doc, &opts);
		let index = SourceIndex::new(&doc);
		for heading in outline {
			let offset = heading.source.start + 2;
			let mapped = index
				.source_to_preview(&snapshot, &Default::default(), offset)
				.unwrap();
			assert!(
				mapped.source.contains(&offset),
				"{heading:?} {mapped:?} {source:?}"
			);
		}
	}
}

#[test]
fn empty_alt_images_keep_atomic_source_geometry_across_resource_updates() {
	for source in [
		"before ![](test.png) after",
		"before <svg width=\"100\" height=\"140\"><rect/></svg> after",
		"![first](test.png)![](test.png)![](test.png)",
		"> before ![](test.png) after",
		"| image |\n|---|\n| ![](test.png) |",
		"<details open><summary>images</summary>\n![](test.png)\n</details>",
	] {
		let doc = document::parse(source);
		let index = SourceIndex::new(&doc);
		let opts = options();
		let mut engine = LayoutEngine::new();
		for state in [None, Some((100, 140))] {
			let mut resources = markview_core::image::ImageSnapshot::default();
			let mut images = Vec::new();
			for block in &doc.blocks {
				block.images(&mut images);
			}
			resources.entries.insert(
				images[0].src.clone(),
				markview_core::image::ImageInfo {
					version: 1,
					size: state,
					error: None,
				},
			);
			let snapshot = engine.layout_with_images(&doc, &opts, &resources);
			let ranges: Vec<_> = if let Some(start) = source.find("<svg") {
				std::iter::once(start..source.find("</svg>").unwrap() + 6)
					.collect()
			} else {
				source
					.match_indices("![]")
					.map(|(start, _)| start..start + "![](test.png)".len())
					.collect()
			};
			for range in &ranges {
				let (start, end) = (range.start, range.end);
				for offset in start..end {
					let mapped = index
						.source_to_preview(
							&snapshot,
							&Default::default(),
							offset,
						)
						.unwrap();
					assert_eq!(
						mapped.source,
						start..end,
						"{source}, state={state:?}"
					);
					assert!(
						mapped.rect.w > 80. && mapped.rect.h > 60.,
						"{mapped:?}"
					);
				}
			}
			if source.starts_with("before") {
				let image = index
					.source_to_preview(
						&snapshot,
						&Default::default(),
						ranges[0].start,
					)
					.unwrap();
				let reverse = index
					.preview_to_source(
						&snapshot,
						&Default::default(),
						image.rect.y + 1.,
					)
					.unwrap();
				assert_eq!(reverse.source, image.source);
				let shifted = document::parse(format!("prefix\n\n{source}"));
				let shifted_snapshot =
					engine.layout_with_images(&shifted, &opts, &resources);
				assert!(shifted_snapshot.reused > 0);
				let mapped = SourceIndex::new(&shifted)
					.source_to_preview(
						&shifted_snapshot,
						&Default::default(),
						image.source.start + 8,
					)
					.unwrap();
				assert_eq!(
					mapped.source,
					image.source.start + 8..image.source.end + 8
				);
			}
		}
	}
}

#[test]
fn source_geometry_follows_horizontal_code_and_table_panning() {
	let line = "0123456789".repeat(200);
	for source in [
		format!("```text\n{line}\n```"),
		format!("| {line} |\n|---|"),
	] {
		let doc = document::parse(source.as_str());
		let snapshot = LayoutEngine::new().layout(&doc, &options());
		let index = SourceIndex::new(&doc);
		let block = &snapshot.blocks[0];
		let overflow = &block.layout.overflow[0];
		let node = &block.layout.text[0];
		let horizontal = std::collections::HashMap::from([((0, 0), 700.)]);
		let start = source.find(&line).unwrap();
		let first = node
			.clusters
			.iter()
			.find(|c| c.rect.x + c.rect.w > overflow.rect.x + 700.)
			.unwrap();
		let reverse = index
			.preview_to_source(
				&snapshot,
				&horizontal,
				block.y + first.rect.y + 1.,
			)
			.unwrap();
		assert_eq!(reverse.source.start, start + first.range.start);
		assert_eq!(
			index
				.source_to_preview(&snapshot, &horizontal, start)
				.unwrap()
				.source,
			reverse.source
		);
		let target = node
			.clusters
			.iter()
			.find(|c| c.rect.x > overflow.rect.x + 750.)
			.unwrap();
		let offset = start + target.range.start;
		let mapped = index
			.source_to_preview(&snapshot, &horizontal, offset)
			.unwrap();
		assert!(mapped.source.contains(&offset));
		assert!((mapped.rect.x - (target.rect.x - 700.)).abs() < 0.01);
		assert!(
			mapped.rect.x >= overflow.rect.x
				&& mapped.rect.x + mapped.rect.w
					<= overflow.rect.x + overflow.rect.w
		);
		let reset = index
			.preview_to_source(
				&snapshot,
				&Default::default(),
				reverse.rect.y + 1.,
			)
			.unwrap();
		assert_eq!(reset.source.start, start);
	}
}

#[test]
fn atomic_image_source_geometry_uses_the_table_scroll_offset() {
	let source = format!(
		"| {} | ![](test.png) |\n|---|---|",
		"0123456789".repeat(100)
	);
	let doc = document::parse(source.as_str());
	let mut images = markview_core::image::ImageSnapshot::default();
	images.entries.insert(
		"test.png".into(),
		markview_core::image::ImageInfo {
			version: 1,
			size: Some((100, 140)),
			error: None,
		},
	);
	let snapshot =
		LayoutEngine::new().layout_with_images(&doc, &options(), &images);
	let block = &snapshot.blocks[0];
	let overflow = &block.layout.overflow[0];
	let rect = block
		.layout
		.draws
		.iter()
		.find_map(|draw| match draw {
			markview_core::scene::Draw::Image { rect, .. } => Some(rect),
			_ => None,
		})
		.unwrap();
	let pan = (rect.x - overflow.rect.x - 50.)
		.min(overflow.content_width - overflow.rect.w);
	let horizontal = std::collections::HashMap::from([((0, 0), pan)]);
	let offset = source.find("![]").unwrap();
	let index = SourceIndex::new(&doc);
	let mapped = index
		.source_to_preview(&snapshot, &horizontal, offset)
		.unwrap();
	assert_eq!(mapped.source, offset..offset + "![](test.png)".len());
	assert!((mapped.rect.x - (rect.x - pan)).abs() < 0.01);
	assert_eq!(mapped.rect.w, rect.w);
	assert_eq!(
		index
			.preview_to_source(&snapshot, &horizontal, mapped.rect.y + 1.)
			.unwrap()
			.source,
		mapped.source
	);
}
