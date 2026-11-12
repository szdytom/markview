use markview_commonmark::{Dialect, MarkdownParse, parse, parse_with};
use markview_document::{BlockKind, FieldText, InlineKind, visit_fields};
use proptest::prelude::*;

fn assert_last_text_source(doc: &MarkdownParse, text: &str) {
	assert_eq!(doc.map.validate(&doc.source, &doc.document), Ok(()));
	let start = doc.source.rfind(text).unwrap();
	let mut actual = None;
	for block in doc.blocks() {
		visit_fields(
			block,
			&mut Vec::new(),
			&|| false,
			&mut |_, _, value, _| {
				if let FieldText::Rich(rich) = value {
					for inline in rich {
						if matches!(&inline.kind, InlineKind::Text(value) if value == text)
						{
							actual = doc.inline_source(block, inline);
						}
					}
				}
			},
		);
	}
	assert_eq!(
		actual,
		Some(start..start + text.len()),
		"{text:?} in {:?}",
		doc.source
	);
}

#[test]
fn reference_definitions_do_not_shift_image_sources() {
	let source = "[BAR]: /url\n![foo][bai]\n![foo][bar]";
	let doc = parse(source);
	let block = &doc.blocks()[0];
	let BlockKind::Paragraph(rich) = block.kind() else {
		panic!("expected paragraph");
	};
	let image = rich
		.iter()
		.find(|inline| matches!(inline.kind, InlineKind::Image(_)))
		.unwrap();
	let start = source.rfind("![foo][bar]").unwrap();
	assert_eq!(doc.inline_source(block, image), Some(start..source.len()));
	assert_eq!(
		doc.block_source(block),
		Some(source.find("![foo][bai]").unwrap()..source.len())
	);
	assert_eq!(doc.map.validate(source, &doc.document), Ok(()));
}

#[test]
fn partially_consumed_tabs_keep_byte_columns() {
	let doc = parse(">\t>a\n>\t**b**\n");
	assert_last_text_source(&doc, "b");
}

#[test]
fn escaped_table_pipes_do_not_truncate_the_last_character() {
	for newline in ["\n", "\r\n", "\r"] {
		let cell = "A \\| escaped pipe";
		let source =
			format!("| head |{newline}| --- |{newline}| {cell} |{newline}");
		let doc = parse(source.clone());
		let block = &doc.blocks()[0];
		let BlockKind::Table(table) = block.kind() else {
			panic!("expected table");
		};
		let BlockKind::Paragraph(rich) =
			table.groups[1].rows[0].cells[0].blocks[0].kind()
		else {
			panic!("expected cell paragraph");
		};
		assert_eq!(markview_document::plain_text(rich), "A | escaped pipe");
		let start = source.find(cell).unwrap();
		assert_eq!(
			doc.inline_source(block, &rich[0]),
			Some(start..start + cell.len())
		);
		assert_eq!(doc.map.validate(&source, &doc.document), Ok(()));
	}
}

#[test]
fn multiline_footnotes_keep_their_inline_sources() {
	for newline in ["\n", "\r\n", "\r"] {
		let doc = parse(format!(
			"[^n]: first{newline}    **中é😀**{newline}{newline}After[^n]."
		));
		assert_last_text_source(&doc, "中é😀");
		assert_last_text_source(&doc, "After");
	}
}

#[test]
fn unclosed_fences_include_the_last_content_line() {
	for newline in ["\n", "\r\n", "\r"] {
		for ending in ["", newline] {
			let source = format!("```text{newline}first{newline}中é😀{ending}");
			let doc = parse(source.clone());
			assert_eq!(doc.map.validate(&source, &doc.document), Ok(()));
			assert_eq!(
				doc.block_source(&doc.blocks()[0]),
				Some(0..source.trim_end_matches(['\r', '\n']).len())
			);
			let BlockKind::Code { text, .. } = doc.blocks()[0].kind() else {
				panic!("expected code");
			};
			assert_eq!(text.trim_end(), "first\n中é😀");
		}
	}
}

#[test]
fn code_span_line_endings_normalize_without_shifting_following_text() {
	for newline in ["\n", "\r\n", "\r"] {
		let doc = parse(format!("`first{newline}second` **中é😀**"));
		let BlockKind::Paragraph(rich) = doc.blocks()[0].kind() else {
			panic!("expected paragraph");
		};
		assert_eq!(&rich[0].kind, &InlineKind::Text("first second".into()));
		assert!(rich[0].style.code);
		assert_eq!(
			doc.inline_source(&doc.blocks()[0], &rich[0]),
			Some(0.."`first".len() + newline.len() + "second`".len())
		);
		assert_last_text_source(&doc, "中é😀");
	}
}

#[test]
fn block_ids_remain_literal_in_both_dialects() {
	for ofm in [false, true] {
		let doc = parse_with(
			"hello ^block-id",
			Dialect {
				ofm,
				..Dialect::default()
			},
		);
		assert_last_text_source(&doc, "hello ^block-id");
	}
}

proptest! {
	#[test]
	fn definitions_preserve_unicode_inline_byte_ranges(
		text in "[a-z中é😀]{1,20}",
		newline in prop::sample::select(vec!["\n", "\r\n", "\r"]),
		definition in prop::sample::select(vec!["[r]: /url", "[r]: /url\n  \"title\"", "[r]:\n  /url"]),
		prefix in prop::sample::select(vec!["", "> ", "- "]),
	) {
		let continuation = if prefix == "- " { "  " } else { prefix };
		let mut lines: Vec<_> = definition.lines().map(str::to_owned).collect();
		lines.push(format!("[r] **{text}**"));
		let source = lines.iter().enumerate().map(|(i, line)| {
			format!("{}{line}", if i == 0 { prefix } else { continuation })
		}).collect::<Vec<_>>().join(newline);
		assert_last_text_source(&parse(source), &text);
	}
}
