use crate::*;
use proptest::prelude::*;

proptest! {
	#[test]
	fn supported_link_attributes_decode_entities_and_ignore_css(text in "[a-z中é😀]{1,16}", bold in any::<bool>()) {
		let tag = if bold { "b" } else { "em" };
		let source = format!("<p><a id='attributes' href='https://a.test/?a=1&amp;b=2' href='https://ignored.test' style='color:red'><{tag}>{text}</{tag}></a></p>\n");
		let parsed = parse(source.clone());
		prop_assert_eq!(parsed.map.validate(&source, &parsed.document), Ok(()));
		let block = &parsed.blocks()[0];
		let BlockKind::Paragraph(rich) = block.kind() else { panic!("HTML paragraph missing") };
		prop_assert_eq!(plain_text(rich), text);
		for inline in rich.iter().filter(|inline| matches!(inline.kind, InlineKind::Text(_))) {
			prop_assert_eq!((inline.style.bold, inline.style.italic), (bold, !bold));
			prop_assert!(inline.style.color.is_none());
			prop_assert_eq!(inline.link_url(), Some("https://a.test/?a=1&b=2"));
			let range = parsed.inline_source(block, inline).unwrap();
			prop_assert!(source.get(range).is_some());
		}
		prop_assert!(rich.iter().any(|inline| matches!(&inline.kind, InlineKind::Anchor(anchor) if anchor == "attributes")));
	}

	#[test]
	#[ignore = "Known bug PBT-013: decoded nonbreaking spaces are collapsed; PBT-012 entity decoding is fixed"]
	fn supported_raw_html_entities_and_first_link_attribute_preserve_reading_text(
		entities in prop::collection::vec(0usize..7, 1..12),
		bold in any::<bool>(),
	) {
		let pairs = [("&amp;", "&"), ("&lt;", "<"), ("&gt;", ">"), ("&quot;", "\""), ("&#233;", "é"), ("&#x1F600;", "😀"), ("&nbsp;", "\u{a0}")];
		let encoded = entities.iter().map(|&i| pairs[i].0).collect::<Vec<_>>().join("_");
		let expected = entities.iter().map(|&i| pairs[i].1).collect::<Vec<_>>().join("_");
		let tag = if bold { "b" } else { "em" };
		let source = format!("<p><a id='entities' href='https://a.test/?a=1&amp;b=2' href='https://ignored.test' style='color:red'><{tag}>{encoded}</{tag}></a></p>\n");
		let parsed = parse(source.clone());
		prop_assert_eq!(parsed.map.validate(&source, &parsed.document), Ok(()));
		let BlockKind::Paragraph(text) = parsed.blocks()[0].kind() else { panic!("HTML paragraph missing") };
		prop_assert_eq!(plain_text(text), expected);
		for inline in text.iter().filter(|inline| matches!(inline.kind, InlineKind::Text(_))) {
			prop_assert_eq!(inline.style.bold, bold);
			prop_assert_eq!(inline.style.italic, !bold);
			prop_assert!(inline.style.color.is_none());
			prop_assert_eq!(inline.link_url(), Some("https://a.test/?a=1&b=2"));
		}
		prop_assert!(text.iter().any(|inline| matches!(&inline.kind, InlineKind::Anchor(anchor) if anchor == "entities")));
	}
}

#[test]
fn raw_html_ampersand_entity_reads_as_one_character() {
	let parsed = parse("<p><em>&amp;</em></p>\n");
	let BlockKind::Paragraph(text) = parsed.blocks()[0].kind() else {
		panic!("HTML paragraph missing")
	};
	assert_eq!(plain_text(text), "&");
}

#[test]
#[ignore = "Known bug PBT-013: raw HTML collapses literal nonbreaking spaces"]
fn raw_html_preserves_literal_nonbreaking_spaces() {
	let parsed = parse("<p><em>a\u{a0}b</em></p>\n");
	let BlockKind::Paragraph(text) = parsed.blocks()[0].kind() else {
		panic!("HTML paragraph missing")
	};
	assert_eq!(plain_text(text), "a\u{a0}b");
}
