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
