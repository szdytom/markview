//! Field bindings survive the semantic traversal, search, source mapping and
//! cached geometry reuse, without a memory-address lookup.
#![cfg(feature = "font-directories")]

use markview_document::{
	Affinity, Block, BlockKind, BlockSource, Callout, CalloutKind, CellKind,
	DefinitionItem, DocumentBuilder, DocumentRevision, FieldId, FieldSource,
	FieldText, FootnoteId, Inline, InlineKind, RichText, SemanticPosition,
	SourceMap, SourceSpan, Table, TableCell, TableColumn, TableGroup,
	TableGroupKind, TableRow, TextStyle, plain_text, visit_fields,
};
use markview_layout::{
	fonts::FontConfig,
	layout::{LayoutEngine, LayoutOptions},
	search::{SearchIndex, SearchOptions},
	source::SourceIndex,
};
use std::path::Path;

fn options() -> LayoutOptions {
	LayoutOptions {
		width: 420.0,
		force_open: true,
		fonts: FontConfig {
			ignore_system_fonts: true,
			directories: vec![
				Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts"),
			],
			..Default::default()
		},
		..Default::default()
	}
}

fn rich(text: &str) -> RichText {
	vec![Inline {
		link: None,
		kind: InlineKind::Text(text.into()),
		style: TextStyle::default(),
	}]
}

fn paragraph(builder: &mut DocumentBuilder, text: &str) -> Block {
	builder.block(Vec::new(), BlockKind::Paragraph(rich(text)))
}

fn cell(
	builder: &mut DocumentBuilder,
	text: &str,
	kind: CellKind,
) -> TableCell {
	TableCell {
		kind,
		rowspan: 1,
		colspan: 1,
		align: None,
		blocks: vec![paragraph(builder, text)],
	}
}

fn fixture() -> markview_document::Document {
	let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
	let mut blocks = Vec::new();

	let yaml = builder.block(
		Vec::new(),
		BlockKind::Code {
			language: "yaml".into(),
			text: "title: Front matter".into(),
		},
	);
	blocks.push(builder.block(
		Vec::new(),
		BlockKind::FrontMatter {
			default_open: true,
			blocks: vec![yaml],
		},
	));
	blocks.push(builder.block(
		vec!["heading".into()],
		BlockKind::Heading {
			level: 1,
			text: rich("Heading text"),
			anchor: "heading".into(),
		},
	));
	blocks.push(paragraph(&mut builder, "Paragraph text"));
	blocks.push(builder.block(
		Vec::new(),
		BlockKind::Code {
			language: "rust".into(),
			text: "let code = 1;".into(),
		},
	));

	let head = TableGroup {
		kind: TableGroupKind::Head,
		rows: vec![TableRow {
			cells: vec![
				cell(&mut builder, "Head cell", CellKind::Header),
				cell(&mut builder, "Head value", CellKind::Header),
			],
		}],
	};
	let body = TableGroup {
		kind: TableGroupKind::Body,
		rows: vec![TableRow {
			cells: vec![
				cell(&mut builder, "Body cell", CellKind::Data),
				cell(&mut builder, "Body value", CellKind::Data),
			],
		}],
	};
	blocks.push(builder.block(
		Vec::new(),
		BlockKind::Table(Table {
			caption: Some(rich("Table caption")),
			columns: vec![
				TableColumn { align: None },
				TableColumn { align: None },
			],
			groups: vec![head, body],
		}),
	));

	let definition = paragraph(&mut builder, "Definition body");
	blocks.push(builder.block(
		Vec::new(),
		BlockKind::DefinitionList(vec![DefinitionItem {
			terms: vec![rich("Definition term")],
			definitions: vec![vec![definition]],
		}]),
	));

	let callout_body = paragraph(&mut builder, "Callout body");
	blocks.push(builder.block(
		Vec::new(),
		BlockKind::Quote {
			callout: Some(Callout {
				kind: CalloutKind::Note,
				name: "note".into(),
				title: Some(rich("Callout summary")),
				fold: None,
			}),
			blocks: vec![callout_body],
		},
	));

	let details_body = paragraph(&mut builder, "Details body");
	blocks.push(builder.block(
		Vec::new(),
		BlockKind::Details {
			default_open: true,
			summary: rich("Details summary"),
			blocks: vec![details_body],
		},
	));

	let note_body = paragraph(&mut builder, "Footnote body");
	blocks.push(builder.block(
		Vec::new(),
		BlockKind::Footnote {
			id: FootnoteId::new("note"),
			number: 1,
			blocks: vec![note_body],
		},
	));

	builder.finish(blocks)
}

#[derive(Clone)]
struct Expected {
	block: markview_document::BlockId,
	field: FieldId,
	text: String,
	source: std::ops::Range<usize>,
}

/// Builds a source map in field order, so source navigation can be checked
/// against exactly the fields the traversal produced.
fn fixture_map(
	document: &markview_document::Document,
) -> (String, SourceMap, Vec<Expected>) {
	let mut source = String::new();
	let mut blocks = Vec::new();
	let mut expected = Vec::new();
	for block in document.blocks() {
		let start = source.len();
		let mut fields = Vec::new();
		visit_fields(
			block,
			&mut Vec::new(),
			&|| false,
			&mut |_, field, text, _| {
				let text = match text {
					FieldText::Rich(rich) => plain_text(rich),
					FieldText::Code(code) => code.to_owned(),
				};
				let field_start = source.len();
				source.push_str(&text);
				source.push('\n');
				let range = field_start..field_start + text.len();
				fields.push(FieldSource {
					field,
					spans: vec![SourceSpan {
						semantic: 0..text.len(),
						source: range.clone(),
						exact: true,
					}],
					covers: Vec::new(),
				});
				expected.push(Expected {
					block: block.id(),
					field,
					text,
					source: range,
				});
			},
		);
		blocks.push(BlockSource {
			block: block.id(),
			source: Some(start..source.len()),
			fields,
		});
	}
	let map = SourceMap::new(document.revision(), blocks);
	(source, map, expected)
}

#[test]
fn every_field_resolves_after_address_keyed_bindings_are_gone() {
	let document = fixture();
	let (source, map, expected) = fixture_map(&document);
	let snapshot = LayoutEngine::new().layout(&document, &options());
	let epoch = snapshot.revision.get();
	let index = SearchIndex::new(&document);
	let source_index = SourceIndex::new(source.into(), &document, &map);

	for entry in &expected {
		assert!(
			snapshot.blocks.iter().any(|block| {
				block.id == entry.block
					&& block
						.layout
						.text
						.iter()
						.any(|node| node.search_field == Some(entry.field))
			}),
			"layout binds field {} of {}",
			entry.field,
			entry.block
		);

		let geometry = snapshot.resolve_position(SemanticPosition {
			projection: None,
			revision: document.revision(),
			block: entry.block,
			field: entry.field,
			offset: 0,
			affinity: Affinity::Before,
		});
		assert!(
			geometry.is_some(),
			"selection geometry resolves field {} of {}",
			entry.field,
			entry.block
		);

		let hits = index.find(&entry.text, SearchOptions::default());
		let hit = hits
			.iter()
			.find(|hit| hit.block == entry.block && hit.field == entry.field)
			.unwrap_or_else(|| {
				panic!(
					"search finds field {} of {}: {hits:?}",
					entry.field, entry.block
				)
			});
		let selection = snapshot
			.search_selection(hit, epoch)
			.expect("a current search hit selects its field");
		assert_eq!(snapshot.extract_text(selection, epoch), entry.text);
		let mut stale = hit.clone();
		stale.revision =
			markview_document::DocumentRevision::new(hit.revision.get() + 1);
		assert!(
			snapshot.search_selection(&stale, epoch).is_none(),
			"a search hit from another revision is rejected"
		);

		assert!(
			source_index
				.source_to_preview(
					&snapshot,
					&Default::default(),
					entry.source.start
				)
				.is_some(),
			"source navigation finds field {} of {}",
			entry.field,
			entry.block
		);
	}
}

#[test]
fn semantic_selections_survive_cache_reuse_reflow_and_reject_old_revisions() {
	let parsed = markview_commonmark::parse(
		"Repeated 中文 👋 [one **two**](same).\n\nRepeated 中文 👋 [one **two**](same).\n\n| A | B |\n|---|---|\n| 中文 👋 | tail |\n\n<details open>\n<summary>Outer</summary>\n\n<details open>\n<summary>Inner</summary>\n\nNested 中文 👋.\n\n</details>\n</details>\n",
	);
	let document = &parsed.document;
	let mut engine = LayoutEngine::new();
	let wide = engine.layout(document, &options());
	let reused = engine.layout(document, &options());
	assert!(reused.reused > 0);
	let narrow = engine.layout(
		document,
		&LayoutOptions {
			width: 180.,
			..options()
		},
	);
	let index = SearchIndex::new(document);
	for hit in index.find("中文 👋", SearchOptions::default()) {
		let selection = wide.search_selection(&hit, 1).unwrap();
		assert_eq!(reused.extract_text(selection, 2), "中文 👋");
		assert_eq!(narrow.extract_text(selection, 3), "中文 👋");
		assert!(
			!narrow
				.selection_rects(selection, &Default::default(), 3)
				.is_empty()
		);
		let (x, y) = {
			let rect =
				narrow.selection_rects(selection, &Default::default(), 3)[0];
			(rect.x + 1., rect.y + rect.h / 2.)
		};
		let position =
			narrow.hit_test_text(x, y, &Default::default(), 3).unwrap();
		assert_eq!((position.block, position.field), (hit.block, hit.field));
	}
	let hit = index.find("Repeated", SearchOptions::default()).remove(1);
	let selection = wide.search_selection(&hit, 1).unwrap();
	let replacement = markview_commonmark::parse("Repeated 中文 👋.");
	let replacement = engine.layout(&replacement.document, &options());
	assert!(replacement.extract_text(selection, 1).is_empty());
	assert!(replacement.resolve_position(selection.anchor).is_none());
}
