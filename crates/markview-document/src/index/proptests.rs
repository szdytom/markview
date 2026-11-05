use crate::*;
use proptest::prelude::*;
use std::collections::HashMap;

#[derive(Clone, Debug)]
enum Tree {
	Heading(u8, String, u8),
	Container(u8, bool, Vec<Tree>),
}

fn tree() -> impl Strategy<Value = Tree> {
	(1u8..7, ".{0,12}", 0u8..4)
		.prop_map(|(level, text, anchor)| Tree::Heading(level, text, anchor))
		.prop_recursive(3, 24, 3, |child| {
			(0u8..4, any::<bool>(), prop::collection::vec(child, 0..4))
				.prop_map(|(kind, open, children)| {
					Tree::Container(kind, open, children)
				})
		})
}

fn rich(text: &str) -> RichText {
	vec![Inline {
		kind: InlineKind::Text(text.to_owned()),
		style: TextStyle::default(),
		link: None,
	}]
}

struct Record {
	id: BlockId,
	path: Vec<usize>,
	enclosing: Vec<BlockId>,
	declared: Option<bool>,
	heading: Option<(u8, String, String)>,
	field: Option<(Vec<usize>, String)>,
}

fn build(
	tree: &Tree,
	builder: &mut DocumentBuilder,
	path: Vec<usize>,
	enclosing: Vec<BlockId>,
	records: &mut Vec<Record>,
) -> Block {
	let block = builder.block(Vec::new(), BlockKind::Rule);
	let id = block.id();
	let mut record = Record {
		id,
		path: path.clone(),
		enclosing: enclosing.clone(),
		declared: None,
		heading: None,
		field: None,
	};
	let kind = match tree {
		Tree::Heading(level, text, slot) => {
			let anchor = format!("slot-{slot}");
			record.heading = Some((*level, text.clone(), anchor.clone()));
			record.field = Some((path, text.clone()));
			BlockKind::Heading {
				level: *level,
				text: rich(text),
				anchor,
			}
		}
		Tree::Container(kind, open, children) => {
			let mut child_enclosing = enclosing;
			if *kind == 0 {
				child_enclosing.push(id);
				record.declared = Some(*open);
				record.field = Some((path.clone(), "summary".to_owned()));
			} else if *kind == 2 {
				let mut after_children = path.clone();
				after_children.push(usize::MAX);
				record.field = Some((after_children, "caption".to_owned()));
			}
			let blocks: Vec<_> = children
				.iter()
				.enumerate()
				.map(|(i, child)| {
					let mut child_path = path.clone();
					child_path.push(i);
					build(
						child,
						builder,
						child_path,
						child_enclosing.clone(),
						records,
					)
				})
				.collect();
			match kind {
				0 => BlockKind::Details {
					default_open: *open,
					summary: rich("summary"),
					blocks,
				},
				1 => BlockKind::Quote {
					callout: None,
					blocks,
				},
				2 => BlockKind::Figure {
					blocks,
					caption: Some(rich("caption")),
				},
				_ => BlockKind::List {
					start: None,
					reversed: false,
					tight: false,
					items: blocks
						.into_iter()
						.map(|block| ListItem {
							checked: None,
							number: None,
							blocks: vec![block],
						})
						.collect(),
				},
			}
		}
	};
	records.push(record);
	block
		.with_kind(kind)
		.with_anchors(vec![format!("node-{id}")])
}

proptest! {
	#[test]
	fn typed_anchor_indexes_keep_first_registration_and_each_disclosure_ancestry(
		entries in prop::collection::vec((0u8..9, 0u8..4, 1u32..5, 0usize..6), 0..12),
	) {
		let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
		let mut blocks = Vec::new();
		let mut expected = HashMap::new();
		for (kind, slot, occurrence, depth) in [(0, 0, 1, 1), (1, 0, 1, 2), (2, 0, 1, 3), (2, 0, 2, 4)].into_iter().chain(entries) {
			let wrappers: Vec<_> = (0..depth).map(|_| builder.block(Vec::new(), BlockKind::Rule)).collect();
			let ancestry: Vec<_> = wrappers.iter().map(Block::id).collect();
			let name = format!("slot-{slot}");
			let definition = FootnoteId::new(name.clone());
			let occurrence = LinkOccurrenceId::new(occurrence).unwrap();
			let anchor = match kind { 1 => AnchorId::FootnoteDefinition(definition.clone()), 2 => AnchorId::FootnoteReference { definition: definition.clone(), occurrence }, _ => AnchorId::Fragment(name.clone()) };
			expected.entry(anchor).or_insert_with(|| ancestry.clone());
			let text = vec![Inline { kind: if kind == 2 { InlineKind::FootnoteRef(FootnoteReference { definition: definition.clone(), occurrence, number: 1 }) } else { InlineKind::Anchor(name.clone()) }, style: TextStyle::default(), link: None }];
			let leaf = match kind {
				1 => BlockKind::Footnote { id: definition, number: 1, blocks: Vec::new() },
				4 => BlockKind::Heading { level: 2, text: rich("heading"), anchor: name.clone() },
				5 => BlockKind::Figure { blocks: Vec::new(), caption: Some(text) },
				6 => BlockKind::Table(Table { caption: Some(text), columns: Vec::new(), groups: Vec::new() }),
				7 => BlockKind::DefinitionList(vec![DefinitionItem { terms: vec![text], definitions: Vec::new() }]),
				8 => BlockKind::Quote { callout: Some(Callout { kind: CalloutKind::Note, name: "note".to_owned(), title: Some(text), fold: None }), blocks: Vec::new() },
				_ => BlockKind::Paragraph(if kind == 3 { Vec::new() } else { text }),
			};
			let mut root = builder.block(if kind == 3 { vec![name] } else { Vec::new() }, leaf);
			for (layer, wrapper) in wrappers.into_iter().enumerate().rev() {
				let kind = if layer % 2 == 0 {
					let summary = format!("summary-{}", wrapper.id());
					expected.insert(AnchorId::Fragment(summary.clone()), ancestry[..layer].to_vec());
					BlockKind::Details { default_open: false, summary: vec![Inline { kind: InlineKind::Anchor(summary), style: TextStyle::default(), link: None }], blocks: vec![root] }
				} else { BlockKind::FrontMatter { default_open: false, blocks: vec![root] } };
				root = wrapper.with_kind(kind);
			}
			blocks.push(root);
		}
		let document = builder.finish(blocks);
		for (anchor, ancestry) in expected { prop_assert_eq!(document.details_enclosing(&anchor), ancestry); }
		prop_assert!(document.details_enclosing(AnchorId::FootnoteReference { definition: FootnoteId::new("slot-0"), occurrence: LinkOccurrenceId::new(999).unwrap() }).is_empty(), "unknown occurrence must not resolve");
		prop_assert!(document.fragment_enclosing("missing").is_empty());
	}

	#[test]
	fn branching_indexes_and_field_order_match_the_declared_tree(trees in prop::collection::vec(tree(), 0..4)) {
		let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
		let mut records = Vec::new();
		let blocks = trees.iter().enumerate().map(|(i, tree)| build(tree, &mut builder, vec![i], Vec::new(), &mut records)).collect();
		let document = builder.finish(blocks);
		records.sort_by(|a, b| a.path.cmp(&b.path));
		let mut first_anchors = HashMap::new();
		let mut expected_outline = Vec::new();
		for record in &records {
			prop_assert_eq!(document.path_of(record.id), Some(record.path.as_slice()));
			prop_assert_eq!(document.block_at_path(&record.path).unwrap().id(), record.id);
			prop_assert_eq!(document.block(record.id).unwrap().id(), record.id);
			prop_assert_eq!(document.details_declared(record.id), record.declared);
			prop_assert_eq!(document.fragment_enclosing(&format!("node-{}", record.id)), record.enclosing.as_slice());
			let mut invalid = record.path.clone();
			invalid.push(usize::MAX);
			prop_assert!(document.block_at_path(&invalid).is_none());
			if let Some((level, text, anchor)) = &record.heading {
				expected_outline.push((record.id, *level, text.clone(), anchor.clone()));
				first_anchors.entry(anchor.clone()).or_insert_with(|| record.enclosing.clone());
			}
		}
		for (anchor, enclosing) in first_anchors {
			prop_assert_eq!(document.fragment_enclosing(&anchor), enclosing);
		}
		let actual: Vec<_> = document.outline_entries().iter().map(|entry| (entry.block, entry.level, entry.text.clone(), entry.anchor.clone())).collect();
		prop_assert_eq!(actual, expected_outline);
		for (i, root) in document.blocks().iter().enumerate() {
			let mut expected: Vec<_> = records.iter().filter(|record| record.path[0] == i)
				.filter_map(|record| record.field.as_ref().map(|(key, text)| (key, record.id, text, &record.enclosing))).collect();
			expected.sort_by(|a, b| a.0.cmp(b.0));
			let expected: Vec<_> = expected.into_iter().enumerate().map(|(ordinal, (_, owner, text, enclosing))| (owner, ordinal as u32, text.clone(), enclosing.clone())).collect();
			let mut actual = Vec::new();
			let mut enclosing = Vec::new();
			visit_fields(root, &mut enclosing, &|| false, &mut |owner, field, text, open| {
				let FieldText::Rich(text) = text else { unreachable!() };
				actual.push((owner, field.get(), plain_text(text), open.to_vec()));
			});
			prop_assert_eq!(actual, expected);
			prop_assert!(enclosing.is_empty());
			visit_fields(root, &mut enclosing, &|| true, &mut |_, _, _, _| panic!("cancelled traversal visited a field"));
			prop_assert!(enclosing.is_empty());
		}
	}
}
