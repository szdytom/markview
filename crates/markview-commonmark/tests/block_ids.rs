use markview_commonmark::{
	Dialect, MarkdownParse, parse, parse_incremental, parse_prefix_with,
	parse_with,
};
use markview_document::{
	Block, BlockKind, FieldText, plain_text, visit_blocks, visit_fields,
};
use proptest::prelude::*;
use std::sync::Arc;

fn ofm() -> Dialect {
	Dialect {
		ofm: true,
		..Default::default()
	}
}

fn targets(doc: &MarkdownParse) -> Vec<(String, Block)> {
	let mut targets = Vec::new();
	for root in doc.blocks() {
		visit_blocks(root, &mut |block| {
			for anchor in block.anchors() {
				targets.push((anchor.clone(), block.clone()));
			}
		});
	}
	targets
}

fn reading(doc: &MarkdownParse) -> String {
	let mut reading = String::new();
	for block in doc.blocks() {
		visit_fields(
			block,
			&mut Vec::new(),
			&|| false,
			&mut |_, _, text, _| match text {
				FieldText::Rich(text) => reading.push_str(&plain_text(text)),
				FieldText::Code(text) => reading.push_str(text),
			},
		);
	}
	reading
}

#[test]
fn markers_label_blocks_without_entering_the_reading_text() {
	for (source, expected, owner) in [
		("hello ^My-ID\n", "hello", "paragraph"),
		("%%hidden%% ^My-ID\n", "", "paragraph"),
		("- %%hidden%% ^My-ID\n", "", "paragraph"),
		("> %%hidden%% ^My-ID\n", "", "quote"),
		("# Title ^My-ID\n", "Title", "heading"),
		("> one ^My-ID\n", "one", "quote"),
		("> one ^My-ID\n>\n> two\n", "onetwo", "paragraph"),
		("> outer\n>\n>> inner ^My-ID\n", "outerinner", "quote"),
		("- a ^My-ID\n- b\n", "ab", "paragraph"),
		("- [ ] a ^My-ID\n- b\n", "ab", "paragraph"),
		("- outer\n  - inner ^My-ID\n", "outerinner", "paragraph"),
		("- first\n\n  last ^My-ID\n", "firstlast", "paragraph"),
		("- first\n    ^My-ID\n- two\n", "firsttwo", "paragraph"),
		("- first\n\n    ^My-ID\n- two\n", "firsttwo", "paragraph"),
		("- a\n- b\n\n^My-ID\n", "ab", "list"),
		("> - a\n> - b\n>\n> ^My-ID\n", "ab", "list"),
		("| A |\n| - |\n| y ^My-ID |\n", "Ay", "table"),
		("> [!note]- Title\n> Body ^My-ID\n", "TitleBody", "quote"),
		("one ^first\ntwo ^My-ID\n", "one ^first two", "paragraph"),
	] {
		let doc = parse_with(source, ofm());
		assert_eq!(reading(&doc), expected, "{source}");
		let registered = targets(&doc);
		assert_eq!(registered.len(), 1, "{source}: {registered:?}");
		assert_eq!(registered[0].0, "^My-ID");
		let kind = match registered[0].1.kind() {
			BlockKind::Paragraph(_) => "paragraph",
			BlockKind::Heading { anchor, .. } => {
				assert_eq!(anchor, "title");
				"heading"
			}
			BlockKind::Quote { .. } => "quote",
			BlockKind::Table(_) => "table",
			BlockKind::List { .. } => "list",
			other => panic!("unexpected owner: {other:?}"),
		};
		assert_eq!(kind, owner, "{source}");
		if source.starts_with("- first") {
			let BlockKind::Paragraph(text) = registered[0].1.kind() else {
				unreachable!();
			};
			assert_eq!(plain_text(text), "first");
		}
		assert_eq!(doc.map.validate(&doc.source, &doc.document), Ok(()));
		let gfm = parse(source);
		assert!(targets(&gfm).is_empty());
		assert!(reading(&gfm).contains("^My-ID"), "{source}");
	}
}

#[test]
fn literal_carets_and_unsupported_standalone_markers_remain_text() {
	for source in [
		"hello^tight\n",
		"hello ^trail  \n",
		"hello \\^escaped\n",
		"hello ^bad_id\n",
		"hello ^中文\n",
		"^orphan\n",
		"hello\n^next\n",
		"hello\n\n^separate\n",
		"> quote\n\n^separate\n",
		"- list\n^unindented\n",
		"- list\n\n\\^escaped\n",
		"- list\n\n^bad_id\n",
		"- list\n\n^trailing  \n",
		"*hello ^em*\n",
		"[hello ^label](https://example.com)\n",
		"`code ^id`\n",
		"```\ncode ^id\n```\n",
	] {
		let doc = parse_with(source, ofm());
		assert!(targets(&doc).is_empty(), "{source}");
		assert!(reading(&doc).contains('^'), "{source}");
	}
}

#[test]
fn block_wikilinks_preserve_the_marker_and_case() {
	let source = "[[#^My-ID]] [[Note %41#^My-ID|Alias]] [[Note#Heading#^My-ID]] [plain](#^My-ID) [encoded](note.md#%5EMy-ID) [[Note#A Heading]]";
	for dialect in [ofm(), Dialect::default()] {
		let doc = parse_with(source, dialect);
		let BlockKind::Paragraph(text) = doc.blocks()[0].kind() else {
			panic!("expected links");
		};
		assert_eq!(
			text.iter()
				.filter_map(|inline| inline.link_url())
				.collect::<Vec<_>>(),
			[
				"#^My-ID",
				"Note %2541#^My-ID",
				"Note#^My-ID",
				"#^My-ID",
				"note.md#%5EMy-ID",
				"Note#a-heading"
			]
		);
		assert!(plain_text(text).contains("Alias"));
		assert_eq!(doc.map.validate(source, &doc.document), Ok(()));
	}
}

#[test]
fn heading_ids_preserve_code_whitespace_and_existing_heading_links() {
	for prefix in ["", "> "] {
		for title in ["`  code  `", "**Title**", "![image](a.png)"] {
			let plain = parse_with(format!("{prefix}# {title}"), ofm());
			let labelled =
				parse_with(format!("{prefix}# {title} ^Heading-ID"), ofm());
			assert_eq!(reading(&plain), reading(&labelled));
			assert_eq!(plain.outline()[0].anchor, labelled.outline()[0].anchor);
			assert_eq!(
				labelled.map.validate(&labelled.source, &labelled.document),
				Ok(())
			);
		}
	}
}

#[test]
fn repeated_marker_names_stay_on_their_distinct_blocks() {
	let doc = parse_with("> first ^same\n>\n> last ^same\n", ofm());
	let targets = targets(&doc);
	assert_eq!(targets.len(), 2);
	assert!(matches!(targets[0].1.kind(), BlockKind::Quote { .. }));
	assert!(matches!(targets[1].1.kind(), BlockKind::Paragraph(_)));
}

#[test]
fn footnote_paragraphs_and_grid_tables_register_each_marker_once() {
	for (source, table) in [
		("Reference[^n]\n\n[^n]: First\n\n    Last ^id\n", false),
		("+------------+\n| A ^id      |\n+------------+\n", true),
	] {
		let doc = parse_with(source, ofm());
		let targets = targets(&doc);
		assert_eq!(targets.len(), 1, "{source}: {targets:?}");
		assert_eq!(targets[0].0, "^id");
		assert_eq!(matches!(targets[0].1.kind(), BlockKind::Table(_)), table);
		assert_eq!(doc.map.validate(&doc.source, &doc.document), Ok(()));
	}
}

#[test]
fn markers_stay_hidden_in_depth_fallbacks() {
	let source = format!(
		"{}Visible ^secret\n\n{}",
		"<details><summary>S</summary>\n\n".repeat(300),
		"</details>\n\n".repeat(300)
	);
	let doc = parse_with(source, ofm());
	assert!(!reading(&doc).contains("^secret"));
	assert_eq!(doc.map.validate(&doc.source, &doc.document), Ok(()));
}

#[test]
fn whole_list_ids_stay_hidden_at_the_depth_limit() {
	for (depth, marker) in [(255, "^whole\n\n"), (256, ""), (256, "^whole\n\n")]
	{
		let source = format!(
			"{}- First\n- Second\n\n{marker}{}",
			"<details>\n\n".repeat(depth),
			"</details>\n\n".repeat(depth)
		);
		let doc = parse_with(source, ofm());
		let visible = reading(&doc);
		assert!(
			visible.contains("First") && visible.contains("Second"),
			"{depth}"
		);
		assert!(!visible.contains("^whole"), "{depth}");
		assert_eq!(doc.map.validate(&doc.source, &doc.document), Ok(()));
	}
}

#[test]
fn standalone_item_ids_survive_the_markdown_reparse_after_svg() {
	use markview_document::embed::{
		Expansion, Request, Source, SourceId, Target, expand,
	};
	use std::collections::HashMap;

	let host = parse_with("![[B#^first]]", ofm());
	for separator in ["\n\n", "\n"] {
		let leaf = parse_with(
			format!(
				"<svg/>{separator}- First marker\n    ^first\n\n- Separate marker\n"
			),
			ofm(),
		);
		assert!(
			leaf.blocks()
				.iter()
				.any(|block| matches!(block.kind(), BlockKind::List { .. }))
		);
		let source = Source {
			id: SourceId(0),
			document: &host.document,
		};
		let targets = HashMap::from([(
			Request {
				from: SourceId(0),
				note: "B".into(),
			},
			Target::Loaded(Source {
				id: SourceId(1),
				document: &leaf.document,
			}),
		)]);
		let Expansion::Ready(expanded) = expand(&source, &targets) else {
			panic!("unexpected target lookup");
		};
		let BlockKind::List { items, .. } =
			expanded.document.blocks()[0].kind()
		else {
			panic!("expected the embedded list item after {separator:?}");
		};
		assert_eq!(items.len(), 1);
		assert!(matches!(
			items[0].blocks[0].kind(),
			BlockKind::Paragraph(content) if plain_text(content) == "First marker"
		));
		assert!(leaf.anchor_block("^first").is_some());
		assert!(!reading(&leaf).contains("^first"));
		assert_eq!(leaf.map.validate(&leaf.source, &leaf.document), Ok(()));
	}
}

proptest! {
	#[test]
	fn marker_edits_and_prefixes_match_full_parses(
		ids in prop::collection::vec(prop::option::of("[A-Za-z0-9-]{1,20}"), 3..7),
		text in "[a-zé你]{1,20}",
		newline in prop::sample::select(vec!["\n", "\r\n", "\r"]),
	) {
		let mut previous = parse_with("before\n\nText\n\nafter\n", ofm());
		for id in ids {
			let marker = id.map(|id| format!(" ^{id}")).unwrap_or_default();
			let source: Arc<str> = format!("before{newline}{newline}{text}{marker}{newline}{newline}after{newline}").into();
			let full = parse_with(source.clone(), ofm());
			let next = parse_incremental(&previous, source.clone(), ofm()).expect("leaf edit uses incremental parsing");
			prop_assert_eq!(next.blocks(), full.blocks());
			prop_assert_eq!(next.content_key(), full.content_key());
			prop_assert_eq!(next.map.validate(&source, &next.document), Ok(()));
			let BlockKind::Paragraph(rich) = next.blocks()[1].kind() else {
				return Err(TestCaseError::fail("expected paragraph"));
			};
			prop_assert_eq!(plain_text(rich), text.clone());
			let range = next.inline_source(&next.blocks()[1], &rich[0]).unwrap();
			prop_assert_eq!(&source[range], plain_text(rich));
			let cut = source.find("after").unwrap();
			let prefix = parse_prefix_with(&source, cut, ofm()).unwrap();
			prop_assert_eq!(prefix.blocks(), &full.blocks()[..prefix.blocks().len()]);
			prop_assert_eq!(prefix.map.validate(&source, &prefix.document), Ok(()));
			previous = next;
		}
	}
}
