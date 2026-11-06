use markview_commonmark::parse;
use markview_document::{BlockKind, InlineKind, plain_text};

#[test]
fn raw_html_text_decodes_entities_once_after_recognizing_markup() {
	for (body, expected) in [
		("&amp; &#233; &#x1F600; &lt; &quot; &gt;", "& é 😀 < \" >"),
		(
			"&lt;strong&gt;literal&lt;/strong&gt;",
			"<strong>literal</strong>",
		),
		("&amp;lt;em&amp;gt;", "&lt;em&gt;"),
		("a&#32; &#9;b&#10;c", "a b c"),
		("bare & and &unknown;", "bare & and &unknown;"),
	] {
		let source = format!("<p><em>{body}</em></p>\n");
		let parsed = parse(source.clone());
		assert_eq!(parsed.map.validate(&source, &parsed.document), Ok(()));
		let block = &parsed.blocks()[0];
		let BlockKind::Paragraph(text) = block.kind() else {
			panic!("HTML paragraph missing")
		};
		assert_eq!(plain_text(text), expected, "{body:?}");
		for inline in text {
			assert!(matches!(inline.kind, InlineKind::Text(_)));
			assert!(inline.style.italic);
			assert!(!inline.style.bold);
			assert!(
				source
					.get(parsed.inline_source(block, inline).unwrap())
					.is_some()
			);
		}
	}
}

#[test]
fn raw_html_whitespace_preserves_unicode_at_edges_and_across_styles() {
	for (raw, expected) in [
		("\u{a0}", "\u{a0}a\u{a0}\u{a0}b\u{a0}"),
		("&nbsp;", "\u{a0}a\u{a0}\u{a0}b\u{a0}"),
		("&#160;", "\u{a0}a\u{a0}\u{a0}b\u{a0}"),
		("\u{2003}", "\u{2003}a\u{2003}\u{2003}b\u{2003}"),
		("&#x2003;", "\u{2003}a\u{2003}\u{2003}b\u{2003}"),
		("\u{202f}", "\u{202f}a\u{202f}\u{202f}b\u{202f}"),
		(" ", "a b"),
		("\t", "a b"),
		("\n", "a b"),
		("\r", "a b"),
		("\u{c}", "a b"),
		(" \t\r\n\u{c} ", "a b"),
	] {
		let source = format!(
			"<p><a id='start'></a> \t{raw}<em>a{raw}</em><b>{raw}b</b>{raw}\n <a id='end'></a></p>\n"
		);
		let parsed = parse(source.clone());
		assert_eq!(parsed.map.validate(&source, &parsed.document), Ok(()));
		let BlockKind::Paragraph(text) = parsed.blocks()[0].kind() else {
			panic!("HTML paragraph missing")
		};
		assert_eq!(plain_text(text), expected, "{raw:?}");
	}
}
