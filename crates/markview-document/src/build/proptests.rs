use super::*;
use crate::{
	CellKind, DefinitionItem, FieldText, Inline, InlineKind, ListItem, Table,
	TableCell, TableGroup, TableGroupKind, TableRow, TextStyle, visit_blocks,
	visit_fields,
};
use proptest::prelude::*;

fn rich(text: &str) -> Vec<Inline> {
	vec![Inline {
		link: None,
		kind: InlineKind::Text(text.to_owned()),
		style: TextStyle::default(),
	}]
}

fn wrap(kind: u8, child: Block, text: &str) -> BlockKind {
	wrap_rich(kind, child, rich(text))
}

fn wrap_rich(kind: u8, child: Block, text: Vec<Inline>) -> BlockKind {
	let blocks = vec![child];
	match kind {
		0 => BlockKind::Quote {
			callout: None,
			blocks,
		},
		1 => BlockKind::Details {
			default_open: false,
			summary: text,
			blocks,
		},
		2 => BlockKind::FrontMatter {
			default_open: false,
			blocks,
		},
		3 => BlockKind::Figure {
			blocks,
			caption: Some(text),
		},
		4 => BlockKind::Footnote {
			id: crate::FootnoteId::new("note"),
			number: 1,
			blocks,
		},
		5 => BlockKind::List {
			start: Some(1),
			reversed: false,
			tight: false,
			items: vec![ListItem {
				checked: None,
				number: None,
				blocks,
			}],
		},
		6 => BlockKind::DefinitionList(vec![DefinitionItem {
			terms: vec![text],
			definitions: vec![blocks],
		}]),
		7 => BlockKind::Table(Table {
			caption: Some(text),
			columns: vec![crate::TableColumn { align: None }],
			groups: vec![TableGroup {
				kind: TableGroupKind::Body,
				rows: vec![TableRow {
					cells: vec![TableCell {
						kind: CellKind::Data,
						rowspan: 1,
						colspan: 1,
						align: None,
						blocks,
					}],
				}],
			}],
		}),
		_ => BlockKind::Quote {
			callout: Some(crate::Callout {
				kind: crate::CalloutKind::Note,
				name: "note".to_owned(),
				title: Some(text),
				fold: None,
			}),
			blocks,
		},
	}
}

fn image(slot: u8) -> crate::ImageSpec {
	use crate::image::{Diagram, ImageSource, ResourceRef};
	crate::ImageSpec {
		source: match slot % 3 {
			0 => ImageSource::Resource(ResourceRef::document(format!(
				"image-{slot}"
			))),
			1 => ImageSource::Resource(ResourceRef::vault(format!(
				"image-{slot}"
			))),
			_ => ImageSource::Diagram(Diagram {
				language: "mermaid".to_owned(),
				source: format!("graph TD; A{slot}-->B"),
			}),
		},
		alt: format!("alt-{slot}"),
		title: format!("title-{slot}"),
		width: Some(u32::from(slot)),
		height: None,
	}
}

fn image_text(slot: u8) -> Vec<Inline> {
	vec![Inline {
		kind: InlineKind::Image(Box::new(image(slot))),
		style: TextStyle::default(),
		link: None,
	}]
}

proptest! {
	#[test]
	fn resource_and_code_collectors_preserve_duplicates_and_nested_container_contents(
		layers in prop::collection::vec((0u8..9, 0u8..6, ".{0,16}"), 0..6),
	) {
		for outer in 0u8..9 {
			let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
			let mut root = builder.block(Vec::new(), BlockKind::Paragraph(image_text(0)));
			let mut expected_images = vec![image(0)];
			let mut expected_code = Vec::new();
			let mut expected_ids = vec![root.id()];
			for (kind, slot, source) in layers.iter().cloned().chain([(outer, 0, String::new())]) {
				let language = format!("language-{slot}");
				let code = builder.block(Vec::new(), BlockKind::Code { language: language.clone(), text: source.clone() });
				let sibling = builder.block(Vec::new(), BlockKind::Paragraph(image_text(slot)));
				let sibling_ids = [code.id(), sibling.id()];
				let body = builder.block(Vec::new(), BlockKind::Quote { callout: None, blocks: vec![root, code, sibling] });
				expected_ids.insert(0, body.id());
				expected_ids.extend(sibling_ids);
				root = builder.block(Vec::new(), wrap_rich(kind, body, image_text(slot)));
				expected_ids.insert(0, root.id());
				expected_code.push((language, source));
				expected_images.push(image(slot));
				if kind == 2 { expected_images.clear(); }
				if [1, 3, 6, 7, 8].contains(&kind) { expected_images.push(image(slot)); }
				let mut images = Vec::new();
				root.images(&mut images);
				let counts = |images: Vec<crate::ImageSpec>| {
					let mut counts = std::collections::HashMap::new();
					for image in images { *counts.entry(image).or_insert(0usize) += 1; }
					counts
				};
				prop_assert_eq!(counts(images.into_iter().cloned().collect()), counts(expected_images.clone()));
				let mut code = vec![("existing", "prefix")];
				root.code_blocks(&mut code);
				let expected: Vec<_> = std::iter::once(("existing", "prefix")).chain(expected_code.iter().map(|(language, text)| (language.as_str(), text.as_str()))).collect();
				prop_assert_eq!(code, expected);
				let mut callbacks = Vec::new();
				root.for_each_code_block(&mut |language, text| callbacks.push((language, text)));
				prop_assert_eq!(callbacks, expected_code.iter().map(|(language, text)| (language.as_str(), text.as_str())).collect::<Vec<_>>());
				let mut ids = Vec::new();
				visit_blocks(&root, &mut |block| ids.push(block.id()));
				prop_assert_eq!(&ids, &expected_ids);
			}
		}
	}

	#[test]
	fn table_cell_budget_counts_all_groups_and_rows(
		rows in prop::collection::vec(prop::collection::vec(0usize..5, 0..5), 0..5),
	) {
		let count: usize = rows.iter().flatten().sum();
		let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
		let table = builder.block(Vec::new(), BlockKind::Table(Table {
			caption: None, columns: vec![crate::TableColumn { align: None }; 4],
			groups: rows.iter().map(|rows| TableGroup {
				kind: TableGroupKind::Body,
				rows: rows.iter().map(|&cells| TableRow { cells: (0..cells).map(|_| TableCell {
					kind: CellKind::Data, rowspan: 1, colspan: 1, align: None, blocks: Vec::new(),
				}).collect() }).collect(),
			}).collect(),
		}));
		for limit in [count.saturating_sub(1), count, count + 1] {
			let result = DocumentBuilder::new(DocumentRevision::new(1)).with_limits(DocumentLimits {
				max_table_cells: limit, ..DocumentLimits::default()
			}).try_finish(vec![table.clone()]);
			prop_assert_eq!(result.is_ok(), count <= limit);
			if let Err(error) = result {
				prop_assert_eq!(error, DocumentLimit::Table { cells: count, limit });
			}
		}
	}

	#[test]
	fn flat_document_budgets_accept_the_threshold_and_reject_one_less(
		texts in prop::collection::vec(".{0,24}", 0..12),
	) {
		let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
		let blocks: Vec<_> = texts.iter().enumerate().map(|(i, text)| {
			builder.block(Vec::new(), if i % 2 == 0 {
				BlockKind::Paragraph(rich(text))
			} else { BlockKind::Code { language: String::new(), text: text.clone() } })
		}).collect();
		let bytes: usize = texts.iter().map(String::len).sum();
		for limit in [bytes.saturating_sub(1), bytes, bytes + 1] {
			let result = DocumentBuilder::new(DocumentRevision::new(2)).with_limits(DocumentLimits {
				max_text_bytes: limit, ..DocumentLimits::default()
			}).try_finish(blocks.clone());
			prop_assert_eq!(result.is_ok(), bytes <= limit);
			if let Err(error) = result {
				prop_assert!(matches!(error, DocumentLimit::Text { bytes, limit: found } if bytes > found && found == limit), "unexpected text budget error: {error:?}");
			}
		}
		for limit in [blocks.len().saturating_sub(1), blocks.len(), blocks.len() + 1] {
			let result = DocumentBuilder::new(DocumentRevision::new(2)).with_limits(DocumentLimits {
				max_blocks: limit, ..DocumentLimits::default()
			}).try_finish(blocks.clone());
			prop_assert_eq!(result.is_ok(), blocks.len() <= limit);
			if let Err(error) = result {
				prop_assert_eq!(error, DocumentLimit::Blocks { blocks: limit + 1, limit });
			}
		}
	}

	#[test]
	fn nested_containers_preserve_paths_fields_disclosures_and_budgets(
		kinds in prop::collection::vec(0u8..8, 0..12), text in ".{0,24}",
	) {
		// Visit every container category even with a small sample budget.
		for outer in 0u8..8 {
			let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
			let mut root = builder.block(vec!["leaf".to_owned()], BlockKind::Paragraph(rich(&text)));
			let leaf = root.id();
			let mut ids = vec![leaf];
			let mut fields = vec![(leaf, Vec::<BlockId>::new())];
			let mut disclosures = Vec::new();
			for kind in kinds.iter().copied().chain([outer]) {
				root = builder.block(Vec::new(), wrap(kind, root, &text));
				let id = root.id();
				ids.insert(0, id);
				if matches!(kind, 1 | 2) {
					disclosures.insert(0, id);
					for (_, enclosing) in &mut fields { enclosing.insert(0, id); }
				}
				match kind {
					1 | 6 => fields.insert(0, (id, Vec::new())),
					3 | 7 => fields.push((id, Vec::new())),
					_ => {},
				}
			}
			let block_count = ids.len();
			let text_bytes = fields.len() * text.len();
			let depth = block_count - 1;
			let blocks = vec![root];
			let document = builder.finish(blocks.clone());
			for (level, id) in ids.iter().enumerate() {
				let path = vec![0; level + 1];
				prop_assert_eq!(document.path_of(*id), Some(path.as_slice()));
				prop_assert_eq!(document.block_at_path(&path).unwrap().id(), *id);
				prop_assert_eq!(document.block(*id).unwrap().id(), *id);
			}
			prop_assert!(document.block_at_path(&[]).is_none());
			prop_assert!(document.block_at_path(&vec![0; block_count + 1]).is_none());
			prop_assert_eq!(document.fragment_enclosing("leaf"), disclosures);
			let mut visited = Vec::new();
			visit_blocks(&document.blocks()[0], &mut |block| visited.push(block.id()));
			prop_assert_eq!(visited, ids);
			let mut actual = Vec::new();
			let mut enclosing = Vec::new();
			visit_fields(&document.blocks()[0], &mut enclosing, &|| false, &mut |id, field, value, open| {
				let projected = match value { FieldText::Rich(t) => crate::plain_text(t), FieldText::Code(t) => t.to_owned() };
				actual.push((id, field.get(), projected, open.to_vec()));
			});
			let expected: Vec<_> = fields.iter().enumerate().map(|(i, (id, open))| (*id, i as u32, text.clone(), open.clone())).collect();
			prop_assert_eq!(actual, expected);
			prop_assert!(enclosing.is_empty());
			for (limits, fits) in [
				(DocumentLimits { max_blocks: block_count, max_depth: depth, max_text_bytes: text_bytes, ..DocumentLimits::default() }, true),
				(DocumentLimits { max_blocks: block_count - 1, ..DocumentLimits::default() }, false),
				(DocumentLimits { max_depth: depth - 1, ..DocumentLimits::default() }, false),
			] {
				prop_assert_eq!(DocumentBuilder::new(DocumentRevision::new(2)).with_limits(limits).try_finish(blocks.clone()).is_ok(), fits);
			}
			if text_bytes > 0 {
				let limits = DocumentLimits { max_text_bytes: text_bytes - 1, ..DocumentLimits::default() };
				prop_assert!(matches!(DocumentBuilder::new(DocumentRevision::new(2)).with_limits(limits).try_finish(blocks.clone()), Err(DocumentLimit::Text { .. })), "nested text budget was not enforced");
			}
		}
	}
}
