use super::*;
use crate::{
	document::{
		DocumentBuilder, DocumentRevision, FootnoteId, FootnoteReference,
		Inline, InlineKind, LinkOccurrenceId,
	},
	scene::{Draw, LinkTarget, PlacedBlock, Rect},
	style::{Color, Stylesheet, chain_set},
};
use std::sync::Arc;

fn positions(block: &PlacedBlock) -> Vec<Rect> {
	block.clusters().map(|(_, _, rect)| rect).collect()
}

#[test]
fn subtree_uses_original_number_columns_and_keeps_the_whole_document_cache() {
	let mut source = String::new();
	for i in 1..=12 {
		source.push_str(&format!("Reference[^{i}].\n\n"));
	}
	for i in 1..=12 {
		source.push_str(&format!("[^{i}]: Note {i}.\n\n"));
	}
	let document = markview_commonmark::parse(source);
	let node = document
		.blocks()
		.iter()
		.find(|block| {
			matches!(block.kind(), BlockKind::Footnote { number: 1, .. })
		})
		.unwrap();
	let options = LayoutOptions::default();
	let mut engine = LayoutEngine::new();
	let full = engine.layout(&document, &options);
	let selected = engine.layout_subtree(&document, node, &options);
	assert_eq!(selected.blocks.len(), 1);
	assert_eq!(selected.blocks[0].id, node.id());
	assert_eq!(selected.revision, document.revision());
	assert_eq!(selected.content_id, document.content_key());
	assert_eq!(
		positions(&selected.blocks[0]),
		positions(
			full.blocks
				.iter()
				.find(|block| block.id == node.id())
				.unwrap()
		)
	);
	assert_eq!(engine.layout(&document, &options).reused, full.blocks.len());
}

#[test]
fn subtree_retains_cross_node_and_cyclic_reference_targets() {
	let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
	let reference = |id: &str, number, occurrence| Inline {
		link: None,
		style: Default::default(),
		kind: InlineKind::FootnoteRef(FootnoteReference {
			definition: FootnoteId::new(id),
			number,
			occurrence: LinkOccurrenceId::new(occurrence).unwrap(),
		}),
	};
	let a_body = builder
		.block(Vec::new(), BlockKind::Paragraph(vec![reference("b", 2, 1)]));
	let b_body = builder
		.block(Vec::new(), BlockKind::Paragraph(vec![reference("a", 1, 2)]));
	let a = builder.block(
		Vec::new(),
		BlockKind::Footnote {
			id: FootnoteId::new("a"),
			number: 1,
			blocks: vec![a_body],
		},
	);
	let b = builder.block(
		Vec::new(),
		BlockKind::Footnote {
			id: FootnoteId::new("b"),
			number: 2,
			blocks: vec![b_body],
		},
	);
	let document = builder.finish(vec![a, b]);
	let mut engine = LayoutEngine::new();
	for (index, target) in [(0, "b"), (1, "a")] {
		let snapshot = engine.layout_subtree(
			&document,
			&document.blocks()[index],
			&LayoutOptions::default(),
		);
		assert_eq!(snapshot.blocks.len(), 1);
		assert!(snapshot.blocks[0].links().any(|(link, _)| *link.target
			== LinkTarget::FootnoteDefinition(FootnoteId::new(target))));
		assert_eq!(snapshot.blocks[0].id, document.blocks()[index].id());
	}
}

#[test]
fn nested_selection_inherits_ancestor_and_original_sibling_styles() {
	let document =
		markview_commonmark::parse("> First.\n>\n> Last.\n\nOutside.\n");
	let BlockKind::Quote { blocks, .. } = document.blocks()[0].kind() else {
		panic!("quote")
	};
	let options = LayoutOptions { stylesheet: Arc::new(Stylesheet::parse("format_version=3\nversion=1\n[[rule]]\nwhen=['blockquote']\nsize=0.8\n[[rule]]\nwhen=['blockquote','p','last_child']\nsize=0.6").unwrap()), ..Default::default() };
	let mut engine = LayoutEngine::new();
	let selected = engine.layout_subtree(&document, &blocks[1], &options);
	let sizes: Vec<f32> = selected.blocks[0]
		.draws()
		.filter_map(|(_, draw, _)| {
			if let Draw::Glyph(g) = draw {
				Some(g.size)
			} else {
				None
			}
		})
		.collect();
	assert!(!sizes.is_empty());
	assert!(
		sizes
			.iter()
			.all(|size| (*size - options.font_size * 0.6).abs() < 0.01)
	);
	let first = engine.layout_subtree(&document, &blocks[0], &options);
	assert_ne!(selected.presentation_key, first.presentation_key);
}

#[test]
fn preview_context_is_scoped_and_does_not_pollute_normal_layout() {
	let document = markview_commonmark::parse("A paragraph.\n");
	let stylesheet = Arc::new(Stylesheet::parse("format_version=3\nversion=1\n[[rule]]\nwhen=['preview']\nbackground='#ffeecc'\npadding=0.5\nradius=8\nborder_color='#abcdef'\nborder_width=2\n[[rule]]\nwhen=['preview','p']\ncolor='#123456'\nsize=0.7\n[[rule]]\nwhen=['ui','tab_preview']\nradius=12").unwrap());
	let options = LayoutOptions {
		stylesheet: stylesheet.clone(),
		..Default::default()
	};
	let mut engine = LayoutEngine::new();
	let normal = engine.layout(&document, &options);
	let preview = engine.layout_subtree(
		&document,
		&document.blocks()[0],
		&LayoutOptions {
			preview: true,
			..options.clone()
		},
	);
	let glyph = preview.blocks[0]
		.draws()
		.find_map(|(_, draw, _)| {
			if let Draw::Glyph(g) = draw {
				Some(g)
			} else {
				None
			}
		})
		.unwrap();
	assert_eq!(stylesheet.paint(glyph.paint), Color(0x123456ff).rgba());
	assert!((glyph.size - options.font_size * 0.7).abs() < 0.01);
	let Draw::Box {
		condition, chain, ..
	} = preview.document_box.unwrap()
	else {
		panic!("preview surface")
	};
	assert_eq!(condition, Condition::Preview);
	assert_eq!(
		stylesheet.paint(crate::scene::Paint::Scoped(
			chain,
			condition,
			crate::style::ColorField::Background
		)),
		Color(0xffeeccff).rgba()
	);
	let again = engine.layout(&document, &options);
	assert_eq!(positions(&normal.blocks[0]), positions(&again.blocks[0]));
	for (_, draw, _) in again.blocks[0].draws() {
		if let Draw::Glyph(glyph) = draw
			&& let crate::scene::Paint::Cascade(chain, _) = glyph.paint
		{
			assert!(!chain_set(chain).contains(Condition::Preview));
		}
	}
}

#[test]
#[should_panic(expected = "the layout root must belong to the source document")]
fn subtree_rejects_a_node_from_another_document_with_the_same_local_id() {
	let first = markview_commonmark::parse("First.");
	let second = markview_commonmark::parse("Second.");
	LayoutEngine::new().layout_subtree(
		&first,
		&second.blocks()[0],
		&LayoutOptions::default(),
	);
}

proptest::proptest! {
	#[test]
	fn selecting_borrowed_siblings_preserves_original_geometry(count in 1usize..10, selected in 0usize..30, width in 140.0f32..700.0) {
		let source = (0..count).map(|i| format!("Paragraph {i} with **formatted** content.\n\n")).collect::<String>();
		let document = markview_commonmark::parse(source);
		let node = &document.blocks()[selected % count];
		let options = LayoutOptions { width, ..Default::default() };
		let mut engine = LayoutEngine::new();
		let full = engine.layout(&document, &options);
		let part = engine.layout_subtree(&document, node, &options);
		proptest::prop_assert_eq!(positions(&part.blocks[0]), positions(&full.blocks[selected % count]));
		proptest::prop_assert_eq!(part.blocks[0].id, node.id());
	}
}

#[test]
fn selected_table_cell_inherits_its_original_row_style() {
	let document =
		markview_commonmark::parse("| Header |\n|---|\n| Content |\n");
	let BlockKind::Table(table) = document.blocks()[0].kind() else {
		panic!("table")
	};
	let node = &table.groups[0].rows[0].cells[0].blocks[0];
	let sheet = Stylesheet::parse("format_version=3\nversion=1\n[[rule]]\nwhen=['table','cell','first_child']\nsize=0.55").unwrap();
	let options = LayoutOptions {
		stylesheet: Arc::new(sheet),
		..Default::default()
	};
	let snapshot =
		LayoutEngine::new().layout_subtree(&document, node, &options);
	let size = snapshot.blocks[0]
		.draws()
		.find_map(|(_, draw, _)| {
			if let Draw::Glyph(glyph) = draw {
				Some(glyph.size)
			} else {
				None
			}
		})
		.unwrap();
	assert!((size - options.font_size * 0.55).abs() < 0.01);
}

#[test]
fn subtree_cancellation_before_start_does_not_begin_a_pass() {
	let document = markview_commonmark::parse("Paragraph.");
	let options = LayoutOptions::default();
	let mut engine = LayoutEngine::new();
	assert!(
		engine
			.layout_subtree_cancellable(
				&document,
				&document.blocks()[0],
				&options,
				&Default::default(),
				|| true
			)
			.is_none()
	);
	let resumed =
		engine.layout_subtree(&document, &document.blocks()[0], &options);
	assert_eq!(resumed.blocks[0].id, document.blocks()[0].id());
}

#[test]
fn cancellation_inside_a_footnote_stops_and_preserves_future_geometry() {
	use std::sync::atomic::{AtomicUsize, Ordering};
	let source = format!(
		"Reference[^n].\n\n[^n]: Start.\n\n{}",
		"    A paragraph inside the same footnote.\n\n".repeat(80)
	);
	let document = markview_commonmark::parse(source);
	let note = document
		.blocks()
		.iter()
		.find(|block| matches!(block.kind(), BlockKind::Footnote { .. }))
		.unwrap();
	let options = LayoutOptions::default();
	let calls = AtomicUsize::new(0);
	let mut engine = LayoutEngine::new();
	assert!(
		engine
			.layout_subtree_cancellable(
				&document,
				note,
				&options,
				&Default::default(),
				|| calls.fetch_add(1, Ordering::Relaxed) >= 8
			)
			.is_none()
	);
	assert!(calls.load(Ordering::Relaxed) >= 9);
	let resumed = engine.layout_subtree(&document, note, &options);
	let fresh = LayoutEngine::new().layout_subtree(&document, note, &options);
	assert_eq!(positions(&resumed.blocks[0]), positions(&fresh.blocks[0]));
	assert_eq!(resumed.blocks[0].id, note.id());
}
