use markview_core::document::{
	BlockKind, InlineKind, parse, parse_prefix, reparse,
};

#[test]
fn svg_elements_are_atomic_images_with_original_source_ranges() {
	for newline in ["\n", "\r\n"] {
		let svg = [
			"<svg width=\"120\" height=\"80\">",
			"<!-- </svg> -->",
			"",
			"<svg><text><![CDATA[<svg> 中文😀]]></text></svg>",
			"</svg>",
		]
		.join(newline);
		for source in [
			format!("{svg}\n\n# after"),
			format!("before {svg} after"),
			format!(
				"> {}\n\n# after",
				svg.replace(newline, &format!("{newline}> "))
			),
			format!(
				"- {}\n\n# after",
				svg.replace(newline, &format!("{newline}  "))
			),
		] {
			let doc = parse(source.as_str());
			let mut images = Vec::new();
			fn visit<'a>(
				blocks: &'a [markview_core::document::Block],
				out: &mut Vec<&'a markview_core::document::Inline>,
			) {
				for block in blocks {
					match &block.kind {
						BlockKind::Paragraph(rich) => {
							out.extend(rich.iter().filter(|i| {
								matches!(i.kind, InlineKind::Image(_))
							}))
						}
						BlockKind::Quote { blocks, .. } => visit(blocks, out),
						BlockKind::List { items, .. } => {
							for item in items {
								visit(&item.blocks, out);
							}
						}
						_ => {}
					}
				}
			}
			visit(&doc.blocks, &mut images);
			assert_eq!(images.len(), 1, "{source}");
			let inline = images[0];
			let InlineKind::Image(image) = &inline.kind else {
				unreachable!()
			};
			assert_eq!(image.width, Some(120));
			let xml = percent_encoding::percent_decode_str(
				image.src.strip_prefix("data:image/svg+xml,").unwrap(),
			)
			.decode_utf8()
			.unwrap();
			assert!(xml.contains("xmlns=\"http://www.w3.org/2000/svg\""));
			assert!(xml.contains("中文😀"));
			assert!(!xml.contains("\n>"));
			let raw = &source[inline.source.clone()];
			assert!(raw.starts_with("<svg"));
			assert!(raw.ends_with("</svg>"));
			assert!(!raw.contains("# after"));
		}
	}
}

#[test]
fn editing_and_prefix_parsing_keep_svg_atomic() {
	let source = std::sync::Arc::<str>::from(
		"# Before\n\n<svg>\n\n<text>中文😀</text>\n\n</svg>\n\n# After\n",
	);
	assert!(parse_prefix(&source, source.find("<text>").unwrap()).is_none());
	let old = parse(source.clone());
	let changed = source.replace("中文😀", "Updated 中文😀");
	let edited = reparse(&old, std::sync::Arc::from(changed.as_str()));
	let full = parse(changed);
	assert_eq!(edited.blocks, full.blocks);
}

#[test]
fn svg_siblings_html_wrappers_and_unclosed_fallback_preserve_content() {
	let source = "<svg/><svg/>\n\n# after\n\n<p>before <svg viewBox=\"0 0 10 10\"><rect width=\"10\" height=\"10\"/></svg> after</p>";
	let doc = parse(source);
	assert_eq!(doc.outline().len(), 1);
	let mut images = Vec::new();
	for block in &doc.blocks {
		block.images(&mut images);
	}
	assert_eq!(images.len(), 3);
	let last = doc.blocks.last().unwrap();
	assert!(
		matches!(&last.kind,BlockKind::Paragraph(rich) if rich.iter().any(|i| matches!(i.kind,InlineKind::Image(_))))
	);
	let incomplete = parse("<svg>\n<rect/>\n");
	let mut images = Vec::new();
	for block in &incomplete.blocks {
		block.images(&mut images);
	}
	assert!(images.is_empty());
	assert!(matches!(incomplete.blocks[0].kind, BlockKind::Code { .. }));
}

#[test]
fn inline_svg_keeps_text_in_overlapping_markdown_nodes() {
	for (source, expected) in [
		(
			"before <svg><text>`a\nb</text></svg>  after ` tail",
			"before   after  tail",
		),
		(
			"before <svg><text>`a\r\nb</text></svg> 中文😀 after` tail",
			"before  中文😀 after tail",
		),
		(
			"before <svg><text>`hi</text></svg> after` tail",
			"before  after tail",
		),
		(
			"before <svg><text>*hi</text></svg> after* tail",
			"before  after tail",
		),
		(
			"before <svg><text>**hi</text></svg> after** tail",
			"before  after tail",
		),
		(
			"before <svg><text>*hi</text></svg> 中文😀 &amp; after* tail",
			"before  中文😀 & after tail",
		),
		(
			"before <svg><text>*hi</text></svg> **after** tail* end",
			"before  after tail end",
		),
	] {
		let doc = parse(source);
		let BlockKind::Paragraph(rich) = &doc.blocks[0].kind else {
			panic!("expected paragraph")
		};
		assert_eq!(
			markview_core::document::plain_text(rich),
			expected,
			"{source}"
		);
		assert_eq!(
			rich.iter()
				.filter(|i| matches!(i.kind, InlineKind::Image(_)))
				.count(),
			1
		);
		let after = rich
			.iter()
			.find(
				|i| matches!(&i.kind, InlineKind::Text(t) if t.contains("after")),
			)
			.unwrap();
		assert!(source[after.source.clone()].contains("after"));
	}
}

#[test]
fn thousands_of_svg_siblings_parse_without_recursive_suffixes() {
	for prefix in ["", "> > "] {
		let svg = format!("{prefix}<svg>\n{prefix}<rect/>\n{prefix}</svg>\n");
		let source = format!("{}\n# after", svg.repeat(3000));
		let doc = parse(source.as_str());
		let blocks = if prefix.is_empty() {
			&doc.blocks
		} else {
			let BlockKind::Quote { blocks, .. } = &doc.blocks[0].kind else {
				panic!("expected quote")
			};
			let BlockKind::Quote { blocks, .. } = &blocks[0].kind else {
				panic!("expected nested quote")
			};
			blocks
		};
		assert_eq!(blocks.iter().filter(|b| matches!(&b.kind, BlockKind::Paragraph(rich) if rich.iter().any(|i| matches!(i.kind, InlineKind::Image(_))))).count(), 3000);
		for block in blocks.iter().take(3000) {
			assert_eq!(
				&source[block.source.clone()],
				svg.trim_start_matches(prefix).trim_end()
			);
		}
		assert_eq!(doc.outline()[0].text, "after");
	}
}

#[test]
fn inline_svg_clips_overlapping_math_literals_and_source_ranges() {
	for source in [
		"before <svg><text>$x</text></svg> after$ tail",
		"before <svg><text>$x\ny</text></svg> after$ tail",
	] {
		let doc = parse(source);
		let BlockKind::Paragraph(rich) = &doc.blocks[0].kind else {
			panic!("expected paragraph")
		};
		assert_eq!(
			markview_core::document::plain_text(rich),
			"before  after tail"
		);
		let math = rich
			.iter()
			.find(|i| matches!(i.kind, InlineKind::Math { .. }))
			.unwrap();
		let InlineKind::Math { latex, .. } = &math.kind else {
			unreachable!()
		};
		assert_eq!(latex, " after");
		assert_eq!(&source[math.source.clone()], " after$");
	}
}
