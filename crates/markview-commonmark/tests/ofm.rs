use markview_commonmark::{Dialect, parse_with};
use markview_document::{
	Block, BlockKind, Callout, CalloutKind, ImageSource, InlineKind,
	image::ResourceBase, plain_text,
};

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
	} = &block.kind()
	else {
		panic!("expected callout: {block:?}");
	};
	(callout, blocks)
}

/// A plain callout owns its title; a foldable one owns it as the nested
/// disclosure's summary.
fn callout_title<'a>(
	callout: &'a Callout,
	blocks: &'a [Block],
) -> &'a [markview_document::Inline] {
	callout.title.as_deref().unwrap_or_else(|| {
		let BlockKind::Details { summary, .. } = &blocks[0].kind() else {
			panic!("a foldable callout has no disclosure: {blocks:?}");
		};
		summary
	})
}

fn paragraph(block: &Block) -> String {
	let BlockKind::Paragraph(text) = &block.kind() else {
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
	let (header, blocks) = callout(&doc.blocks()[0]);
	assert_eq!(header.kind, CalloutKind::Note);
	assert_eq!(plain_text(callout_title(header, blocks)), "Custom");
	assert_eq!(header.fold, Some(false));
	let BlockKind::Details { blocks, .. } = &blocks[0].kind() else {
		panic!("expected disclosure");
	};
	assert_eq!(paragraph(&blocks[0]), "Body.");

	for (source, title) in [
		("> [!My Type]\n> Body.\n", "My type"),
		("> [!note]     code\n> Body.\n", "code"),
		("> [!note] %%hidden%%\n> Body.\n", ""),
	] {
		let doc = parse_with(source, ofm());
		let (header, blocks) = callout(&doc.blocks()[0]);
		assert_eq!(
			plain_text(callout_title(header, blocks)),
			title,
			"{source}"
		);
		assert_eq!(paragraph(&blocks[0]), "Body.");
	}

	let doc = parse_with("> [!note] *Title\n> *body*\n", ofm());
	let (header, blocks) = callout(&doc.blocks()[0]);
	assert_eq!(plain_text(callout_title(header, blocks)), "*Title");
	let BlockKind::Paragraph(text) = &blocks[0].kind() else {
		panic!("expected body paragraph");
	};
	assert_eq!(plain_text(text), "body");
	assert!(text.iter().all(|inline| inline.style.italic));

	let doc = parse_with(">  [!note] Padded\n> Body.\n", ofm());
	assert!(matches!(
		doc.blocks()[0].kind(),
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
		assert_eq!(paragraph(&doc.blocks()[0]), expected, "{source}");
	}

	let doc = parse_with(
		"<details><summary>S %%literal <b>text</b> literal%%</summary>\n\nBody %%hidden%%.\n\n</details>",
		ofm(),
	);
	let BlockKind::Details {
		summary, blocks, ..
	} = doc.blocks()[0].kind()
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
	let (_, blocks) = callout(&doc.blocks()[0]);
	let BlockKind::Details {
		summary, blocks, ..
	} = &blocks[0].kind()
	else {
		panic!("expected nested details");
	};
	assert_eq!(plain_text(summary), "S %%literal%%");
	assert_eq!(paragraph(&blocks[0]), "Body .");
	assert_eq!(
		&doc.source[doc.block_source(&blocks[0]).unwrap()],
		"Body %%hidden%%."
	);
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
		assert_eq!(doc.blocks().len(), 1);
		if prefix.starts_with("[!") {
			let (header, blocks) = callout(&doc.blocks()[0]);
			assert!(
				plain_text(callout_title(header, blocks)).contains("title")
			);
			assert_eq!(paragraph(&blocks[0]), "Body.");
		}
	}
}

#[test]
fn image_embeds_read_as_images_only_in_ofm() {
	let source = "![[images/Photo 1.png|Alt text]] ![[file.PNG]] ![[note]] ![[scan.avif]]\n";
	let doc = parse_with(source, ofm());
	let BlockKind::Paragraph(text) = doc.blocks()[0].kind() else {
		panic!("expected paragraph");
	};
	let images: Vec<_> = text
		.iter()
		.filter_map(|inline| match &inline.kind {
			InlineKind::Image(image) => Some(image),
			_ => None,
		})
		.collect();
	assert_eq!(images.len(), 2);
	assert_eq!(images[0].address(), "images/Photo 1.png");
	assert_eq!(images[0].alt, "Alt text");
	assert_eq!(
		images[0].source.resource().unwrap().base,
		ResourceBase::Vault
	);
	assert_eq!(images[1].address(), "file.PNG");
	assert_eq!(images[1].alt, "");
	assert_eq!(
		images[1].source.resource().unwrap().base,
		ResourceBase::Vault
	);
	assert!(matches!(images[0].source, ImageSource::Resource(_)));
	assert_eq!(plain_text(text), "Alt text  note scan.avif");
	let links: Vec<_> =
		text.iter().filter_map(|inline| inline.link_url()).collect();
	assert_eq!(links, ["note", "scan.avif"]);

	let doc = parse_with(source, Dialect::default());
	let BlockKind::Paragraph(text) = doc.blocks()[0].kind() else {
		panic!("expected paragraph");
	};
	assert!(
		text.iter()
			.all(|inline| !matches!(inline.kind, InlineKind::Image(_)))
	);
}

#[test]
fn table_escaped_pipe_still_supplies_an_embed_label() {
	let source = "| Image |\n| --- |\n| ![[a.png\\|Caption]] |\n";
	let doc = parse_with(source, ofm());
	let BlockKind::Table(table) = doc.blocks()[0].kind() else {
		panic!("expected table: {:?}", doc.blocks());
	};
	let mut images = Vec::new();
	for child in table.cell_blocks() {
		child.images(&mut images);
	}
	let image = images.into_iter().next().expect("expected image");
	assert_eq!(image.alt, "Caption");
}
