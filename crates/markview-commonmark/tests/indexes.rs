//! Every container reaches the semantic indexes and the collection passes.
use markview_commonmark as commonmark;
use markview_commonmark::Dialect;
use markview_document::{
	AnchorId, BlockKind, Document, InlineKind, visit_fields,
};

const SOURCE: &str = r#"---
title: Indexed
---

# Heading One

![front](a.png)

> [!note] Custom title
> Body with ![callout](b.png)
>
> ```rust
> let x = 1;
> ```

> [!tip]- Folded
> Folded body <a id="folded-anchor"></a>

<figure>
<img src="c.png" alt="figure">
<figcaption>Figure caption</figcaption>
</figure>

| Head |
| --- |
| Cell with ![cell](d.png) |

<details open>
<summary>Summary</summary>

Body <a id="details-anchor"></a>

```python
print(1)
```

</details>

Text[^note]

[^note]: Note body <a id="note-anchor"></a>
"#;

fn fixture() -> Document {
	commonmark::parse_with(
		SOURCE,
		Dialect {
			ofm: true,
			..Dialect::default()
		},
	)
	.document
}

#[test]
fn one_fixture_covers_the_outline_collections_and_enclosing_indexes() {
	let document = fixture();
	let outline = document.outline();
	let headings: Vec<_> =
		outline.iter().map(|entry| entry.text.as_str()).collect();
	assert_eq!(headings, ["Heading One"]);
	assert_eq!(outline[0].anchor, "heading-one");

	let mut images = Vec::new();
	for block in document.blocks() {
		block.images(&mut images);
	}
	let addresses: Vec<_> = images
		.iter()
		.map(|image| image.address().to_owned())
		.collect();
	assert_eq!(addresses, ["a.png", "b.png", "c.png", "d.png"]);

	let mut languages = Vec::new();
	for block in document.blocks() {
		block.for_each_code_block(&mut |language, _| {
			languages.push(language.to_owned())
		});
	}
	assert_eq!(languages, ["yaml", "rust", "python"]);

	// The reader override never reaches the document: only the state a
	// disclosure declared in its source is indexed.
	let mut declared = Vec::new();
	for block in document.blocks() {
		if let Some(open) = document.details_declared(block.id()) {
			declared.push(open);
		}
	}
	assert_eq!(declared, [false, true]);

	let enclosing = |anchor: &str| {
		document
			.details_enclosing(AnchorId::Fragment(anchor.to_owned()))
			.into_iter()
			.filter_map(|id| document.block(id))
			.filter_map(|block| match block.kind() {
				BlockKind::Details { default_open, .. } => Some(*default_open),
				_ => None,
			})
			.collect::<Vec<_>>()
	};
	// A body anchor opens the disclosure that hides it and the folded callout
	// whose title is that disclosure's summary.
	assert_eq!(enclosing("details-anchor"), [true]);
	assert_eq!(enclosing("folded-anchor"), [false]);
	assert!(enclosing("note-anchor").is_empty());
}

#[test]
fn every_field_reports_its_owning_block() {
	let document = fixture();
	let mut fields = 0;
	for block in document.blocks() {
		visit_fields(
			block,
			&mut Vec::new(),
			&|| false,
			&mut |owner, _, text, _| {
				fields += 1;
				assert!(document.block(owner).is_some());
				assert!(!text.is_empty());
			},
		);
	}
	assert!(fields >= 12, "only {fields} fields were visited");
}

#[test]
fn every_parsed_block_has_a_source_span() {
	let parsed = commonmark::parse(SOURCE);
	for block in parsed.blocks() {
		assert!(parsed.block_source(block).is_some(), "a parsed block maps");
	}
}

#[test]
fn anchors_inside_the_containers_are_registered() {
	let document = fixture();
	let mut fragments = Vec::new();
	for block in document.blocks() {
		fragments.extend(block.anchors().iter().cloned());
		visit_fields(
			block,
			&mut Vec::new(),
			&|| false,
			&mut |_, _, text, _| {
				if let markview_document::FieldText::Rich(text) = text {
					for inline in text {
						if let InlineKind::Anchor(anchor) = &inline.kind {
							fragments.push(anchor.clone());
						}
					}
				}
			},
		);
	}
	for anchor in ["folded-anchor", "details-anchor", "note-anchor"] {
		assert!(fragments.iter().any(|found| found == anchor), "{anchor}");
	}
}

mod builder_fixture {
	use markview_document::{
		Block, BlockKind, Callout, CalloutKind, CellKind, DefinitionItem,
		Document, DocumentBuilder, DocumentRevision, FootnoteId, ImageSpec,
		Inline, InlineKind, LinkOccurrenceId, ResourceRef, RichText, Table,
		TableCell, TableColumn, TableGroup, TableGroupKind, TableRow,
		TextStyle, image::ImageSource,
	};

	fn rich(text: &str) -> RichText {
		vec![Inline {
			link: None,
			kind: InlineKind::Text(text.into()),
			style: TextStyle::default(),
		}]
	}

	fn image(address: &str) -> Inline {
		Inline {
			link: None,
			kind: InlineKind::Image(Box::new(ImageSpec {
				source: ImageSource::Resource(ResourceRef::document(address)),
				alt: address.into(),
				title: String::new(),
				width: None,
				height: None,
			})),
			style: TextStyle::default(),
		}
	}

	fn with_image(builder: &mut DocumentBuilder, address: &str) -> Block {
		builder.block(
			Vec::new(),
			BlockKind::Paragraph(vec![
				Inline {
					link: None,
					kind: InlineKind::Text("see ".into()),
					style: TextStyle::default(),
				},
				image(address),
			]),
		)
	}

	fn code(builder: &mut DocumentBuilder, language: &str) -> Block {
		builder.block(
			Vec::new(),
			BlockKind::Code {
				language: language.into(),
				text: "code".into(),
			},
		)
	}

	fn cell(
		builder: &mut DocumentBuilder,
		address: &str,
		kind: CellKind,
	) -> TableCell {
		TableCell {
			kind,
			rowspan: 1,
			colspan: 1,
			align: None,
			blocks: vec![with_image(builder, address)],
		}
	}

	/// One document holding every container, each with an image or a code
	/// block, so a collection pass cannot silently skip a variant.
	pub(super) fn document() -> Document {
		let mut builder = DocumentBuilder::new(DocumentRevision::new(7));
		let mut blocks = Vec::new();
		let yaml = code(&mut builder, "yaml");
		blocks.push(builder.block(
			Vec::new(),
			BlockKind::FrontMatter {
				default_open: false,
				blocks: vec![yaml],
			},
		));
		blocks.push(builder.block(
			vec!["heading".into()],
			BlockKind::Heading {
				level: 1,
				text: vec![
					Inline {
						link: None,
						kind: InlineKind::Text("Title".into()),
						style: TextStyle::default(),
					},
					image("heading.png"),
				],
				anchor: "heading".into(),
			},
		));
		let callout_body = with_image(&mut builder, "callout-body.png");
		let callout_code = code(&mut builder, "rust");
		blocks.push(builder.block(
			Vec::new(),
			BlockKind::Quote {
				callout: Some(Callout {
					kind: CalloutKind::Note,
					name: "note".into(),
					title: Some(vec![
						Inline {
							link: None,
							kind: InlineKind::Text("Custom".into()),
							style: TextStyle::default(),
						},
						image("callout-title.png"),
					]),
					fold: None,
				}),
				blocks: vec![callout_body, callout_code],
			},
		));
		let folded_body = with_image(&mut builder, "folded.png");
		let folded = builder.block(
			Vec::new(),
			BlockKind::Details {
				default_open: false,
				summary: rich("Folded"),
				blocks: vec![folded_body],
			},
		);
		blocks.push(builder.block(
			Vec::new(),
			BlockKind::Quote {
				callout: Some(Callout {
					kind: CalloutKind::Tip,
					name: "tip".into(),
					title: None,
					fold: Some(false),
				}),
				blocks: vec![folded],
			},
		));
		let definition = with_image(&mut builder, "definition.png");
		blocks.push(builder.block(
			Vec::new(),
			BlockKind::DefinitionList(vec![DefinitionItem {
				terms: vec![vec![
					Inline {
						link: None,
						kind: InlineKind::Text("Term".into()),
						style: TextStyle::default(),
					},
					image("term.png"),
				]],
				definitions: vec![vec![definition]],
			}]),
		));
		let figure_body = with_image(&mut builder, "figure.png");
		let figure_code = code(&mut builder, "python");
		blocks.push(builder.block(
			Vec::new(),
			BlockKind::Figure {
				blocks: vec![figure_body, figure_code],
				caption: Some(vec![
					Inline {
						link: None,
						kind: InlineKind::Text("Caption".into()),
						style: TextStyle::default(),
					},
					image("caption.png"),
				]),
			},
		));
		let head = cell(&mut builder, "head.png", CellKind::Header);
		let data = cell(&mut builder, "cell.png", CellKind::Data);
		blocks.push(builder.block(
			Vec::new(),
			BlockKind::Table(Table {
				caption: Some(vec![
					Inline {
						link: None,
						kind: InlineKind::Text("Table".into()),
						style: TextStyle::default(),
					},
					image("table-caption.png"),
				]),
				columns: vec![TableColumn { align: None }],
				groups: vec![
					TableGroup {
						kind: TableGroupKind::Head,
						rows: vec![TableRow { cells: vec![head] }],
					},
					TableGroup {
						kind: TableGroupKind::Body,
						rows: vec![TableRow { cells: vec![data] }],
					},
				],
			}),
		));
		let details_body = builder.block(
			vec!["details-anchor".into()],
			BlockKind::Paragraph(vec![
				Inline {
					link: None,
					kind: InlineKind::Anchor("details-anchor".into()),
					style: TextStyle::default(),
				},
				image("details.png"),
			]),
		);
		blocks.push(builder.block(
			Vec::new(),
			BlockKind::Details {
				default_open: true,
				summary: vec![
					Inline {
						link: None,
						kind: InlineKind::Text("Summary".into()),
						style: TextStyle::default(),
					},
					image("summary.png"),
				],
				blocks: vec![details_body],
			},
		));
		let note_body = builder.block(
			vec!["note-anchor".into()],
			BlockKind::Paragraph(vec![
				Inline {
					link: None,
					kind: InlineKind::Anchor("note-anchor".into()),
					style: TextStyle::default(),
				},
				image("note.png"),
			]),
		);
		blocks.push(builder.block(
			Vec::new(),
			BlockKind::Footnote {
				id: FootnoteId::new("note"),
				number: 1,
				blocks: vec![note_body],
			},
		));
		let reference = builder.block(
			Vec::new(),
			BlockKind::Paragraph(vec![Inline {
				link: None,
				kind: InlineKind::FootnoteRef(
					markview_document::FootnoteReference {
						definition: FootnoteId::new("note"),
						number: 1,
						occurrence: LinkOccurrenceId::new(1).unwrap(),
					},
				),
				style: TextStyle::default(),
			}]),
		);
		blocks.push(reference);
		builder.finish(blocks)
	}

	pub(super) fn images() -> Vec<String> {
		let document = document();
		let mut found = Vec::new();
		for block in document.blocks() {
			block.images(&mut found);
		}
		let mut addresses: Vec<_> = found
			.iter()
			.map(|image| image.address().to_owned())
			.collect();
		addresses.sort();
		addresses
	}
}

#[test]
fn every_container_contributes_its_images_and_code_blocks() {
	let document = builder_fixture::document();
	let expected = [
		"callout-body.png",
		"callout-title.png",
		"caption.png",
		"cell.png",
		"definition.png",
		"details.png",
		"figure.png",
		"folded.png",
		"head.png",
		"heading.png",
		"note.png",
		"summary.png",
		"table-caption.png",
		"term.png",
	];
	assert_eq!(builder_fixture::images(), expected);

	let mut languages = Vec::new();
	for block in document.blocks() {
		block.for_each_code_block(&mut |language, _| {
			languages.push(language.to_owned())
		});
	}
	languages.sort();
	assert_eq!(languages, ["python", "rust", "yaml"]);

	// A disclosure's declared state is indexed; the foldable callout's
	// summary is the disclosure's own field.
	let mut declared = Vec::new();
	for block in document.blocks() {
		if let Some(open) = document.details_declared(block.id()) {
			declared.push(open);
		}
	}
	assert_eq!(declared, [false, true]);

	let enclosing = |anchor: &str| {
		document
			.details_enclosing(AnchorId::Fragment(anchor.to_owned()))
			.into_iter()
			.filter_map(|id| document.block(id))
			.filter_map(|block| match block.kind() {
				BlockKind::Details { default_open, .. } => Some(*default_open),
				_ => None,
			})
			.collect::<Vec<_>>()
	};
	assert_eq!(enclosing("details-anchor"), [true]);
	assert!(enclosing("note-anchor").is_empty());

	// Every field names a block of this document.
	for block in document.blocks() {
		visit_fields(
			block,
			&mut Vec::new(),
			&|| false,
			&mut |owner, _, _, _| {
				assert!(document.block(owner).is_some());
			},
		);
	}
}
