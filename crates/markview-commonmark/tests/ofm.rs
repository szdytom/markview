use markview_commonmark::{Dialect, parse_with};
use markview_document::{Block, BlockKind, Callout, CalloutKind, plain_text};

fn ofm() -> Dialect {
	Dialect {
		ofm: true,
		..Default::default()
	}
}

fn callout(block: &Block) -> (&Callout, &[Block]) {
	let BlockKind::Quote {
		callout: Some(callout),
		blocks,
		..
	} = &block.kind
	else {
		panic!("expected callout: {block:?}");
	};
	(callout, blocks)
}

fn paragraph(block: &Block) -> String {
	let BlockKind::Paragraph(text) = &block.kind else {
		panic!("expected paragraph: {block:?}");
	};
	plain_text(text)
}

#[test]
fn callouts_use_upstream_headers_and_title_boundaries() {
	let doc = parse_with(
		"> [!My Type|metadata]- **Custom** %%hidden%%\n> Body.\n",
		ofm(),
	);
	let (header, blocks) = callout(&doc.blocks[0]);
	assert_eq!(header.kind, CalloutKind::Note);
	assert_eq!(header.title, "Custom");
	assert_eq!(header.fold, Some(false));
	let BlockKind::Details { blocks, .. } = &blocks[0].kind else {
		panic!("expected disclosure");
	};
	assert_eq!(paragraph(&blocks[0]), "Body.");

	for (source, title) in [
		("> [!My Type]\n> Body.\n", "My type"),
		("> [!note]     code\n> Body.\n", "code"),
		("> [!note] %%hidden%%\n> Body.\n", ""),
	] {
		let doc = parse_with(source, ofm());
		let (header, blocks) = callout(&doc.blocks[0]);
		assert_eq!(header.title, title, "{source}");
		assert_eq!(paragraph(&blocks[0]), "Body.");
	}

	let doc = parse_with("> [!note] *Title\n> *body*\n", ofm());
	let (header, blocks) = callout(&doc.blocks[0]);
	assert_eq!(header.title, "*Title");
	let BlockKind::Paragraph(text) = &blocks[0].kind else {
		panic!("expected body paragraph");
	};
	assert_eq!(plain_text(text), "body");
	assert!(text.iter().all(|inline| inline.style.italic));

	let doc = parse_with(">  [!note] Padded\n> Body.\n", ofm());
	assert!(matches!(
		doc.blocks[0].kind,
		BlockKind::Quote { callout: None, .. }
	));
}

#[test]
fn comments_distinguish_raw_html_from_markdown() {
	// Cases from Obsidian 1.14.4's bundled reading parser.
	for (source, expected) in [
		("<p>A %%literal%% B.</p>", "A %%literal%% B."),
		(
			"<p>A %%literal <b>text</b> literal%% B.</p>",
			"A %%literal text literal%% B.",
		),
		("<div>A %%literal%% B.</div>", "A %%literal%% B."),
		("<div>\n\nA %%hidden%% B.\n\n</div>", "A B."),
		("A <b>%%hidden%%</b> B.", "A B."),
		("A %%hidden <b>text</b> hidden%% B.", "A B."),
	] {
		let doc = parse_with(source, ofm());
		assert_eq!(paragraph(&doc.blocks[0]), expected, "{source}");
	}

	let doc = parse_with(
		"<details><summary>S %%literal <b>text</b> literal%%</summary>\n\nBody %%hidden%%.\n\n</details>",
		ofm(),
	);
	let BlockKind::Details {
		summary, blocks, ..
	} = &doc.blocks[0].kind
	else {
		panic!("expected details");
	};
	assert_eq!(plain_text(summary), "S %%literal text literal%%");
	assert_eq!(paragraph(&blocks[0]), "Body .");
}

#[test]
fn callout_html_keeps_its_container_prefixes_out_of_reading_text() {
	let source = "> [!note] Title\r\n> <details><summary>S %%literal%%</summary>\r\n>\r\n> Body %%hidden%%.\r\n>\r\n> </details>\r\n";
	let doc = parse_with(source, ofm());
	let (_, blocks) = callout(&doc.blocks[0]);
	let BlockKind::Details {
		summary, blocks, ..
	} = &blocks[0].kind
	else {
		panic!("expected nested details");
	};
	assert_eq!(plain_text(summary), "S %%literal%%");
	assert_eq!(paragraph(&blocks[0]), "Body .");
	assert_eq!(&doc.source[blocks[0].source.clone()], "Body %%hidden%%.");
}

#[test]
fn deep_quote_inlines_do_not_recurse_while_recognizing_callouts() {
	for prefix in ["[!note] ", "Ordinary "] {
		let source = format!(
			"> {prefix}{}title{}\n> Body.\n",
			"*a ".repeat(10_000),
			" a*".repeat(10_000)
		);
		let doc = parse_with(source, ofm());
		assert_eq!(doc.blocks.len(), 1);
		if prefix.starts_with("[!") {
			let (header, blocks) = callout(&doc.blocks[0]);
			assert!(header.title.contains("title"));
			assert_eq!(paragraph(&blocks[0]), "Body.");
		}
	}
}
