//! Native tests for the publication bookkeeping and the selection-length
//! cache. These run on the host with no browser, because the state they cover
//! names no browser type.

use crate::selection::Pointer;
use crate::state::{Published, SelectionLength};
use markview_core::layout::LayoutSnapshot;
use markview_core::scene::{BlockLayout, PlacedBlock};
use markview_core::text::{TextPosition, TextSelection};
use markview_selection::Host;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

/// A one-block snapshot whose single text node reads `text`.
fn snapshot(text: &str) -> LayoutSnapshot {
	LayoutSnapshot {
		blocks: vec![PlacedBlock {
			flow: Default::default(),
			id: 1,
			source: 0..0,
			y: 0.0,
			layout: Arc::new(BlockLayout {
				height: 100.0,
				text: vec![markview_core::text::TextNode::new(
					text.to_string(),
					"\n",
				)],
				..Default::default()
			}),
		}],
		height: 100.0,
		..Default::default()
	}
}

/// A `Pointer` whose selection the test set directly, with the revision the
/// test's snapshots were stamped at.
fn pointer_with(selection: Option<TextSelection>) -> Pointer {
	let mut pointer = Pointer::default();
	if let Some(selection) = selection {
		pointer.set_selection(Some(selection));
	}
	pointer
}

fn selection_over(start: usize, end: usize) -> TextSelection {
	TextSelection {
		anchor: TextPosition {
			revision: 1,
			block: 0,
			node: 0,
			offset: start,
			affinity: markview_core::text::Affinity::Before,
		},
		focus: TextPosition {
			revision: 1,
			block: 0,
			node: 0,
			offset: end,
			affinity: markview_core::text::Affinity::After,
		},
	}
}

#[test]
fn a_selection_reads_text_from_the_snapshot() {
	let snapshot = snapshot("hello world");
	let selection = selection_over(0, 5);
	let pointer = pointer_with(Some(selection));
	assert_eq!(pointer.selected_text(&snapshot, 1), "hello");
}

// --- SelectionLength ---------------------------------------------------------

#[test]
fn the_cached_length_extracts_at_most_once() {
	let cache = SelectionLength::default();
	let extractions = Rc::new(RefCell::new(0u32));
	let counter = extractions.clone();
	let extract = move || {
		*counter.borrow_mut() += 1;
		"hello".to_string()
	};
	assert_eq!(cache.get(extract), 5);
	for _ in 0..10 {
		assert_eq!(cache.get(|| panic!("the cache must answer")), 5);
	}
	assert_eq!(*extractions.borrow(), 1);
}

#[test]
fn forgetting_lets_the_next_read_extract_again() {
	let cache = SelectionLength::default();
	assert_eq!(cache.get(|| "abc".to_string()), 3);
	cache.forget();
	assert_eq!(cache.get(|| "de".to_string()), 2);
	assert_eq!(cache.extractions(), 2);
}

#[test]
fn the_length_counts_utf16_units_not_characters() {
	let cache = SelectionLength::default();
	// U+1F600 is one character, two UTF-16 code units.
	assert_eq!(cache.get(|| "\u{1F600}".to_string()), 2);
}

// --- Published ---------------------------------------------------------------

/// A published first snapshot at revision 1, with the selection a reader made
/// on it, as a real handle has before any second publication. The source is
/// returned too, because `continues` compares it by pointer identity.
fn published_and_selected(
	text: &str,
	start: usize,
	end: usize,
) -> (Published, Pointer, Arc<str>) {
	let mut pointer = Pointer::default();
	let mut published = Published::default();
	let source: Arc<str> = Arc::from("doc");
	published.accept(snapshot(text), source.clone(), None, &mut pointer);
	// The reader selects over the snapshot now on screen.
	let selection = TextSelection {
		anchor: TextPosition {
			revision: published.revision,
			block: 0,
			node: 0,
			offset: start,
			affinity: markview_core::text::Affinity::Before,
		},
		focus: TextPosition {
			revision: published.revision,
			block: 0,
			node: 0,
			offset: end,
			affinity: markview_core::text::Affinity::After,
		},
	};
	pointer.set_selection(Some(selection));
	(published, pointer, source)
}

#[test]
fn accept_moves_the_selection_onto_the_new_revision() {
	let (mut published, mut pointer, _) = published_and_selected("hello", 0, 5);
	let from = published.revision;
	published.accept(snapshot("hello"), Arc::from("a"), None, &mut pointer);
	assert_eq!(published.revision, from + 1);
	let selection = pointer.selection().expect("the same text keeps it");
	assert_eq!(selection.anchor.revision, published.revision);
	assert_eq!(
		pointer.selected_text(&published.snapshot, published.revision),
		"hello"
	);
}

#[test]
fn accept_drops_a_selection_whose_text_changed() {
	let (mut published, mut pointer, _) = published_and_selected("hello", 0, 5);
	assert!(pointer.selection().is_some());
	published.accept(snapshot("goodbye"), Arc::from("b"), None, &mut pointer);
	// "hello" → "goodbye" replaces the selected span, so nothing survives.
	assert!(pointer.selection().is_none());
}

#[test]
fn accept_drops_a_selection_when_there_was_no_snapshot() {
	// A selection made before any publication cannot be rebased: there is no
	// previous snapshot to vouch for its text.
	let mut pointer = pointer_with(Some(selection_over(0, 5)));
	let mut published = Published::default();
	published.accept(snapshot("hello"), Arc::from("a"), None, &mut pointer);
	assert!(pointer.selection().is_none());
}

#[test]
fn extend_appends_only_new_blocks_and_keeps_positions() {
	let (mut published, mut pointer, source) =
		published_and_selected("first", 0, 5);
	// Mark the publication as the output of a pass, so the next prefix of the
	// same pass may extend it.
	published.pass = Some(7);
	assert!(published.continues(&source, 7));
	assert!(published.extend(&two_block_prefix(), &mut pointer));
	assert_eq!(published.snapshot.blocks.len(), 2);
	assert_eq!(published.snapshot.height, 200.0);
	let selection = pointer.selection().expect("retag keeps it");
	assert_eq!(selection.anchor.revision, published.revision);
	assert_eq!(
		pointer.selected_text(&published.snapshot, published.revision),
		"first"
	);
}

#[test]
fn extend_refuses_a_prefix_shorter_than_published() {
	let (mut published, mut pointer, _) = published_and_selected("text", 0, 4);
	let empty = LayoutSnapshot::default();
	assert!(!published.extend(&empty, &mut pointer));
	assert_eq!(published.snapshot.blocks.len(), 1);
}

#[test]
fn continues_names_only_the_same_source_and_pass() {
	let (mut published, _, source) = published_and_selected("text", 0, 4);
	assert!(!published.continues(&source, 7));
	published.pass = Some(7);
	assert!(published.continues(&source, 7));
	let other = Arc::from("other");
	assert!(!published.continues(&other, 7));
}

#[test]
fn disclosure_publication_invalidates_coordinates_and_retags_selection() {
	use markview_core::{
		document,
		fonts::FontConfig,
		layout::{LayoutEngine, LayoutOptions},
		source::SourceIndex,
	};
	use std::{collections::BTreeMap, time::Duration};

	let document = document::parse(
		"<details>\n<summary>Summary</summary>\n\nHidden paragraph.\n\n</details>\n\nFollowing paragraph.\n",
	);
	let source_index = SourceIndex::new(&document);
	let options = LayoutOptions {
		fonts: FontConfig::from_faces(
			0x776562,
			vec![parley::fontique::Blob::new(Arc::new(
				include_bytes!(
					"../../../markview-core/tests/fonts/NotoSerif-Regular-subset.otf"
				)
				.as_slice(),
			))],
		),
		..Default::default()
	};
	let mut engine = LayoutEngine::new();
	let mut pass =
		engine.begin_layout(&document, &options, &Default::default());
	engine.advance(&mut pass, &document, Duration::MAX);
	let mut pointer = Pointer::default();
	let mut published = Published::default();
	published.accept(
		pass.snapshot().clone(),
		document.source.clone(),
		Some(pass.pass_id()),
		&mut pointer,
	);
	pointer.set_selection(published.snapshot.select_all(published.revision));
	let geometry = published.snapshot.blocks[0].layout.clone();
	let old_revision = published.revision;
	let closed = source_index.scroll_anchors(
		&published.snapshot,
		&Default::default(),
		0,
	);
	let open = BTreeMap::from([(document.blocks[0].id, true)]);
	published.present_disclosures(&open, false, &mut pointer);
	assert_eq!(published.revision, old_revision + 1);
	assert_eq!(published.pass, None);
	assert!(!published.continues(&document.source, pass.pass_id()));
	assert!(Arc::ptr_eq(&geometry, &published.snapshot.blocks[0].layout));
	assert_eq!(
		pointer.selection().unwrap().anchor.revision,
		published.revision
	);
	assert!(
		pointer
			.selected_text(&published.snapshot, published.revision)
			.contains("Following")
	);
	let opened = source_index.scroll_anchors(
		&published.snapshot,
		&Default::default(),
		0,
	);
	assert!(opened.len() > closed.len());
	assert!(opened.last().unwrap().top > closed.last().unwrap().top);
	let pass_id = pass.pass_id();
	pass.set_disclosures(Arc::new(open), false);
	engine.advance(&mut pass, &document, Duration::MAX);
	assert_eq!(pass.pass_id(), pass_id);
	published.accept(
		pass.into_snapshot(),
		document.source.clone(),
		Some(pass_id),
		&mut pointer,
	);
	assert!(Arc::ptr_eq(&geometry, &published.snapshot.blocks[0].layout));
	assert!(published.continues(&document.source, pass_id));
}

/// The prefix of a second block, as a resumable pass reports it.
fn two_block_prefix() -> LayoutSnapshot {
	LayoutSnapshot {
		blocks: vec![
			PlacedBlock {
				flow: Default::default(),
				id: 1,
				source: 0..0,
				y: 0.0,
				layout: Arc::new(BlockLayout {
					height: 100.0,
					text: vec![markview_core::text::TextNode::new(
						"first".to_string(),
						"\n",
					)],
					..Default::default()
				}),
			},
			PlacedBlock {
				flow: Default::default(),
				id: 2,
				source: 0..0,
				y: 100.0,
				layout: Arc::new(BlockLayout {
					height: 100.0,
					text: vec![markview_core::text::TextNode::new(
						"second".to_string(),
						"\n",
					)],
					..Default::default()
				}),
			},
		],
		height: 200.0,
		..Default::default()
	}
}
