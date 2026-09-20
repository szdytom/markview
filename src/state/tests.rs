use super::*;
use markview_core::text::{Affinity, TextPosition};
use winit::event::TouchPhase;
fn position(offset: usize) -> TextPosition {
	TextPosition {
		revision: 1,
		block: 0,
		node: 0,
		offset,
		affinity: Affinity::Before,
	}
}
fn position_in(block: usize, offset: usize) -> TextPosition {
	TextPosition {
		block,
		..position(offset)
	}
}
fn snapshot_of(source: &str) -> LayoutSnapshot {
	let document = Arc::new(document::parse(source));
	crate::layout::LayoutEngine::new()
		.layout(&document, &crate::test_support::options())
}
#[test]
fn click_opens_only_on_release_and_drag_never_opens_link() {
	let snapshot = LayoutSnapshot::default();
	let mut interaction = InteractionState::default();
	interaction
		.begin_selection(position(0), Some("https://example.com".into()));
	assert!(interaction.pointer_down.is_some());
	assert_eq!(
		interaction.finish_selection(Some("https://example.com")),
		Some("https://example.com".into())
	);
	interaction
		.begin_selection(position(0), Some("https://example.com".into()));
	interaction.cursor = (8.0, 0.0);
	interaction.move_selection(Some(position(4)), &snapshot);
	interaction.cursor = (0.0, 0.0);
	interaction.move_selection(Some(position(0)), &snapshot);
	assert!(
		interaction
			.finish_selection(Some("https://example.com"))
			.is_none()
	);
	interaction
		.begin_selection(position(0), Some("https://example.com".into()));
	assert!(interaction.finish_selection(None).is_none());
}
#[test]
fn word_and_block_drag_extend_from_the_multi_click_base() {
	let snapshot = snapshot_of("测试 中文 一下");
	let base = snapshot.select_word_at(position(7)).unwrap();
	assert_eq!(snapshot.extract_text(base, 1), "中文");
	let mut interaction = InteractionState {
		cursor: (0.0, 0.0),
		..Default::default()
	};
	assert!(interaction.begin_grain_selection(Some(base), Grain::Word));
	// Below the drag threshold the double-clicked word stays selected.
	interaction.cursor = (2.0, 0.0);
	interaction.move_selection(Some(position(17)), &snapshot);
	assert_eq!(interaction.selection.unwrap(), base);
	// Right of the base word, the far edge follows the word under the pointer.
	interaction.cursor = (60.0, 0.0);
	interaction.move_selection(Some(position(17)), &snapshot);
	assert_eq!(
		snapshot.extract_text(interaction.selection.unwrap(), 1),
		"中文 一下"
	);
	// Left of the base word, the anchor moves and the base end stays fixed.
	interaction.move_selection(Some(position(0)), &snapshot);
	assert_eq!(
		snapshot.extract_text(interaction.selection.unwrap(), 1),
		"测试 中文"
	);
	// Triple-click drags whole blocks.
	let blocks = snapshot_of("First paragraph.\n\nSecond paragraph.");
	let base = blocks.select_block_at(position(2)).unwrap();
	let mut interaction = InteractionState {
		cursor: (0.0, 0.0),
		..Default::default()
	};
	assert!(interaction.begin_grain_selection(Some(base), Grain::Block));
	interaction.cursor = (60.0, 0.0);
	interaction.move_selection(Some(position_in(1, 2)), &blocks);
	assert_eq!(
		blocks.extract_text(interaction.selection.unwrap(), 1),
		"First paragraph.\n\nSecond paragraph."
	);
}
#[test]
fn shift_extends_original_anchor_and_reload_clears_gesture() {
	let mut interaction = InteractionState::default();
	interaction.begin_selection(position(2), None);
	interaction.finish_selection(None);
	interaction.modifiers = ModifiersState::SHIFT;
	interaction
		.begin_selection(position(10), Some("https://example.com".into()));
	assert_eq!(interaction.selection.unwrap().anchor.offset, 2);
	assert_eq!(interaction.selection.unwrap().focus.offset, 10);
	assert!(
		interaction
			.finish_selection(Some("https://example.com"))
			.is_none()
	);
	interaction.clear_selection();
	assert!(interaction.selection.is_none());
	assert!(interaction.drag_at.is_none());
}
#[test]
fn repeated_presses_within_the_interval_cycle_word_and_block_click() {
	let mut interaction = InteractionState {
		cursor: (40.0, 40.0),
		..Default::default()
	};
	let start = Instant::now();
	assert_eq!(interaction.click_count(start), 1);
	assert_eq!(
		interaction.click_count(start + Duration::from_millis(120)),
		2
	);
	assert_eq!(
		interaction.click_count(start + Duration::from_millis(240)),
		3
	);
	assert_eq!(
		interaction.click_count(start + Duration::from_millis(360)),
		1
	);
	// A pause, a pointer move or a toolbar press starts a new single click.
	assert_eq!(interaction.click_count(start + Duration::from_secs(2)), 1);
	interaction.cursor = (60.0, 40.0);
	assert_eq!(
		interaction.click_count(
			start + Duration::from_secs(2) + Duration::from_millis(100)
		),
		1
	);
	interaction.reset_clicks();
	assert_eq!(
		interaction.click_count(
			start + Duration::from_secs(2) + Duration::from_millis(200)
		),
		1
	);
}
#[test]
fn identical_content_with_a_new_version_keeps_the_selection() {
	let document = Arc::new(document::parse("Hello"));
	let mut engine = crate::layout::LayoutEngine::new();
	let mut session = ReaderSession::default();
	let layout = engine.layout(&document, &crate::test_support::options());
	assert!(session.accept(
		crate::worker::ReaderSnapshot {
			document: document.clone(),
			layout: layout.clone(),
			content_version: 1,
			complete: true,
			remote_deferred: 0,
		},
		300.0,
		None,
	));
	// A file event re-reads the same bytes: a new revision, same content.
	assert!(!session.accept(
		crate::worker::ReaderSnapshot {
			document,
			layout,
			content_version: 2,
			complete: true,
			remote_deferred: 0,
		},
		300.0,
		None,
	));
	assert_eq!(session.accepted_revision, 2);
}

#[test]
fn pending_pages_accumulate_reverse_and_resolve_without_blank_frames() {
	let mut session = ReaderSession {
		layout_pending: true,
		..Default::default()
	};
	session.snapshot.height = 700.;
	for _ in 0..3 {
		session.scroll_by(540., 600.);
	}
	assert_eq!(session.scroll, 0.);
	assert_eq!(session.pending_scroll, Some(1620.));
	session.snapshot.height = 1500.;
	session.resolve_scroll(600.);
	assert_eq!(session.scroll, 0.);
	session.snapshot.height = 2300.;
	session.resolve_scroll(600.);
	assert_eq!(session.scroll, 1620.);
	assert_eq!(session.pending_scroll, None);
	session.scroll_by(f32::INFINITY, 600.);
	assert_eq!(session.scroll, 1620.);
	session.scroll_by(-540., 600.);
	assert_eq!(session.scroll, 1080.);
	session.scroll_by(5400., 600.);
	session.scroll_by(f32::NEG_INFINITY, 600.);
	assert_eq!(session.scroll, 0.);
	assert_eq!(session.pending_scroll, None);
	session.scroll_by(f32::INFINITY, 600.);
	session.scroll_by(0., 600.);
	assert_eq!(session.pending_scroll, Some(f32::INFINITY));
	session.layout_pending = false;
	session.resolve_scroll(600.);
	// The document ends two thirds of a page above the viewport bottom.
	assert_eq!(session.scroll, 2300. - 200.);
}

#[test]
fn scrolling_past_the_end_keeps_two_thirds_of_a_page_blank() {
	let page = 600.;
	let mut session = ReaderSession::default();
	session.snapshot.height = 2000.;
	session.scroll_by(f32::INFINITY, page);
	assert_eq!(session.scroll, 2000. - page / 3.);
	// Scrolling further only repeats the limit.
	session.scroll_by(400., page);
	assert_eq!(session.scroll, 1800.);
	// A document shorter than the page still lifts its end off the bottom.
	session.snapshot.height = 500.;
	session.scroll_by(f32::INFINITY, page);
	assert_eq!(session.scroll, 500. - page / 3.);
	// An end already inside the top third has nowhere to go.
	session.snapshot.height = 150.;
	session.scroll_by(f32::INFINITY, page);
	assert_eq!(session.scroll, 0.);
	assert_eq!(session.pending_scroll, None);
}

#[test]
fn heading_anchors_queue_until_their_heading_is_laid_out() {
	let mut engine = crate::layout::LayoutEngine::new();
	let options = crate::test_support::options();
	let document = Arc::new(document::parse(
		"# Intro\n\nParagraph.\n\n# Details\n\nMore.\n",
	));
	let layout = engine.layout(&document, &options);
	let details = layout.anchor_y("details").unwrap();
	let mut session = ReaderSession::default();
	session.accept(
		crate::worker::ReaderSnapshot {
			document: document.clone(),
			layout: layout.clone(),
			content_version: 1,
			complete: true,
			remote_deferred: 0,
		},
		300.,
		None,
	);
	session.pending_anchor = Some("details".into());
	assert_eq!(session.resolve_anchor(300.), Some(Ok(())));
	assert_eq!(session.pending_anchor, None);
	let max = (layout.height - 300.).max(0.);
	assert_eq!(session.scroll, details.clamp(0., max));
	// A heading the finished document lacks is reported once.
	session.pending_anchor = Some("missing".into());
	assert_eq!(session.resolve_anchor(300.), Some(Err("missing".into())));
	assert_eq!(session.pending_anchor, None);
	// An unfinished prefix keeps the anchor queued.
	let mut prefix = layout.clone();
	prefix.blocks.truncate(1);
	prefix.height = layout.blocks[1].y;
	session.accept(
		crate::worker::ReaderSnapshot {
			document,
			layout: prefix,
			content_version: 2,
			complete: false,
			remote_deferred: 0,
		},
		300.,
		None,
	);
	session.pending_anchor = Some("details".into());
	assert_eq!(session.resolve_anchor(300.), None);
	assert_eq!(session.pending_anchor.as_deref(), Some("details"));
	// A deliberate scroll abandons the queued anchor.
	session.scroll_by(40., 300.);
	assert_eq!(session.pending_anchor, None);
}

#[test]
fn a_jump_into_a_collapsed_body_expands_its_containers_first() {
	let document = Arc::new(document::parse(
		"# Intro\n\n<details>\n<summary>More</summary>\n\n## Hidden\n\n</details>\n",
	));
	let id = document.blocks[1].id;
	let collapsed = crate::layout::LayoutEngine::new()
		.layout(&document, &crate::test_support::options());
	// A collapsed body registers no anchor: the heading is not laid out.
	assert!(collapsed.anchor_y("hidden").is_none());
	let mut session = ReaderSession::default();
	session.accept(
		crate::worker::ReaderSnapshot {
			document: document.clone(),
			layout: collapsed,
			content_version: 1,
			complete: true,
			remote_deferred: 0,
		},
		300.,
		None,
	);
	// The jump opens the enclosing disclosure and reports the change, so the
	// caller requests the reflow.
	assert!(session.open_enclosing_details("hidden"));
	assert_eq!(session.details_open.get(&id), Some(&true));
	// The reflow lays the heading out, and the queued anchor scrolls to it.
	let mut options = crate::test_support::options();
	options.details_open = session.details_open.clone();
	let expanded =
		crate::layout::LayoutEngine::new().layout(&document, &options);
	let at = expanded
		.anchor_y("hidden")
		.expect("the opened body lays the heading out");
	session.accept(
		crate::worker::ReaderSnapshot {
			document,
			layout: expanded,
			content_version: 1,
			complete: true,
			remote_deferred: 0,
		},
		300.,
		None,
	);
	session.pending_anchor = Some("hidden".into());
	assert_eq!(session.resolve_anchor(300.), Some(Ok(())));
	assert_eq!(
		session.scroll,
		at.clamp(0.0, (session.snapshot.height - 300.0).max(0.0))
	);
	// An anchor outside every disclosure needs no reflow, and a second jump
	// into the same body is a no-op now that it is open.
	assert!(!session.open_enclosing_details("intro"));
	assert!(!session.open_enclosing_details("hidden"));
	assert!(!session.open_enclosing_details("missing"));
}

#[test]
fn partial_reload_waits_for_anchor_and_keeps_the_old_snapshot() {
	let mut engine = crate::layout::LayoutEngine::new();
	let options = crate::test_support::options();
	let document = Arc::new(document::parse("Paragraph.\n\n".repeat(100)));
	let full = engine.layout(&document, &options);
	let mut session = ReaderSession::default();
	session.accept(
		crate::worker::ReaderSnapshot {
			document: document.clone(),
			layout: full.clone(),
			content_version: 1,
			complete: true,
			remote_deferred: 0,
		},
		600.,
		None,
	);
	session.scroll = 1800.;
	let mut partial = full.clone();
	partial.blocks.truncate(5);
	partial.height = full.blocks[5].y;
	let reader = crate::worker::ReaderSnapshot {
		document,
		layout: partial,
		content_version: 2,
		complete: false,
		remote_deferred: 0,
	};
	assert!(!session.can_display(&reader, 600.));
	assert_eq!(session.snapshot.blocks.len(), 100);
	assert_eq!(session.scroll, 1800.);
}

#[test]
fn completing_a_prefix_preserves_scroll_and_selection_and_finishes_counts() {
	let document = Arc::new(document::parse("Paragraph.\n\n".repeat(100)));
	let mut engine = crate::layout::LayoutEngine::new();
	let mut prefix = None;
	let layout = engine
		.layout_progressive(
			&document,
			&crate::test_support::options(),
			&Default::default(),
			|p| {
				if p.blocks.len() == 10 {
					prefix = Some(p.clone());
				}
				true
			},
		)
		.unwrap();
	let mut session = ReaderSession::default();
	session.accept(
		crate::worker::ReaderSnapshot {
			document: document.clone(),
			layout: prefix.unwrap(),
			content_version: 1,
			complete: false,
			remote_deferred: 0,
		},
		100.,
		None,
	);
	session.scroll_by(100., 100.);
	let selection = session.snapshot.select_all(1).unwrap();
	let text = session.snapshot.extract_text(selection, 1);
	let reader = crate::worker::ReaderSnapshot {
		document,
		layout,
		content_version: 1,
		complete: true,
		remote_deferred: 0,
	};
	assert!(session.extends_prefix(&reader));
	let rebased = session
		.snapshot
		.rebase_selection(&reader.layout, selection, 1, 1)
		.unwrap();
	assert_eq!(reader.layout.extract_text(rebased, 1), text);
	assert_eq!(session.counts, TextCounts::default());
	let full = reader.layout.select_all(1).unwrap();
	let counts = TextCounts::of(&reader.layout.extract_text(full, 1));
	session.accept(reader, 100., Some(counts));
	assert_eq!(session.counts, counts);
	assert_eq!(session.scroll, 100.);
	assert!(session.snapshot_complete);
	assert!(!session.layout_pending);
	assert!(session.counts.chars > text.len());
}
#[test]
fn text_and_layout_are_accepted_together_and_reflow_is_not_new_content() {
	let document = Arc::new(document::parse("Hello"));
	let mut engine = crate::layout::LayoutEngine::new();
	let mut session = ReaderSession::default();
	let reader = crate::worker::ReaderSnapshot {
		document: document.clone(),
		layout: engine.layout(&document, &crate::test_support::options()),
		content_version: 1,
		complete: true,
		remote_deferred: 0,
	};
	let counts = TextCounts { chars: 5, words: 1 };
	assert!(session.accept(reader.clone(), 300.0, Some(counts)));
	assert_eq!(session.counts, counts);
	// A reflow of the same content arrives without counts and keeps them.
	assert!(!session.accept(reader, 300.0, None));
	assert_eq!(session.counts, counts);
	assert!(Arc::ptr_eq(session.document.as_ref().unwrap(), &document));
}

#[test]
fn the_remote_notice_is_per_session_and_per_content() {
	// Every freshly opened tab starts at revision 1, so a shared "dismissed"
	// flag keyed by revision once hid the notice in every new document.
	let mut a = ReaderSession {
		remote_deferred: 12,
		..Default::default()
	};
	assert_eq!(a.remote_notice(), Some(12));
	a.remote_notice_dismissed = true;
	assert_eq!(a.remote_notice(), None);
	// Another tab is unaffected by what this one answered.
	let b = ReaderSession {
		remote_deferred: 3,
		..Default::default()
	};
	assert_eq!(b.remote_notice(), Some(3));
	// So is this one after its content changes.
	let mut c = ReaderSession {
		remote_deferred: 5,
		remote_notice_dismissed: true,
		..Default::default()
	};
	assert_eq!(c.remote_notice(), None);
	c.remote_notice_dismissed = false;
	assert_eq!(c.remote_notice(), Some(5));
	// Nothing deferred means no notice, whatever was answered before.
	c.remote_deferred = 0;
	assert_eq!(c.remote_notice(), None);
}

#[test]
fn a_footnote_returns_to_the_reference_it_was_opened_from() {
	let mut session = ReaderSession {
		jump_origin: Some(("fn:2".into(), 640.)),
		..Default::default()
	};
	assert_eq!(session.footnote_return("2"), Some(640.));
	// Another note, and a heading jump, have no reference to return to.
	assert_eq!(session.footnote_return("1"), None);
	session.jump_origin = Some(("details".into(), 120.));
	assert_eq!(session.footnote_return("2"), None);
	// Releasing the heavy layout drops the origin with it.
	session.jump_origin = Some(("fn:2".into(), 640.));
	session.release_heavy();
	assert_eq!(session.footnote_return("2"), None);
}

#[test]
fn the_outline_caches_per_document_and_its_entries_queue_heading_anchors() {
	let document =
		Arc::new(document::parse("# One\n\nBody.\n\n> ## Two\n\n# Three\n"));
	let layout = crate::layout::LayoutEngine::new()
		.layout(&document, &crate::test_support::options());
	let mut session = ReaderSession::default();
	session.accept(
		crate::worker::ReaderSnapshot {
			document,
			layout: layout.clone(),
			content_version: 1,
			complete: true,
			remote_deferred: 0,
		},
		300.,
		None,
	);
	// Nothing walks the document until the drawer asks for the outline.
	assert!(session.outline.is_none());
	session.ensure_outline();
	let anchors: Vec<&str> = session
		.outline_entries()
		.iter()
		.map(|entry| entry.anchor.as_str())
		.collect();
	assert_eq!(anchors, ["one", "two", "three"]);
	// An entry addresses the anchor the link path resolves.
	assert_eq!(session.outline_anchor(1), Some("two"));
	let two = layout.anchor_y("two").unwrap();
	// The reading position selects the last heading at or above the viewport.
	session.scroll = 0.0;
	assert_eq!(session.current_outline(), Some(0));
	session.scroll = two + 1.0;
	assert_eq!(session.current_outline(), Some(1));
	// An entry feeds the very anchor path a `#fragment` link uses.
	let third = session.outline_anchor(2).map(str::to_owned);
	session.pending_anchor = third;
	assert_eq!(session.resolve_anchor(300.0), Some(Ok(())));
	let three = layout.anchor_y("three").unwrap();
	assert_eq!(
		session.scroll,
		three.clamp(0.0, (layout.height - 300.0).max(0.0))
	);
}

#[test]
fn a_footnote_reference_between_headings_keeps_the_later_heading_current() {
	let document = Arc::new(document::parse(
		"# One\n\nBody[^a].\n\n# Two\n\n[^a]: Note.\n",
	));
	let layout = crate::layout::LayoutEngine::new()
		.layout(&document, &crate::test_support::options());
	let mut session = ReaderSession::default();
	session.accept(
		crate::worker::ReaderSnapshot {
			document,
			layout: layout.clone(),
			content_version: 1,
			complete: true,
			remote_deferred: 0,
		},
		300.,
		None,
	);
	session.ensure_outline();
	assert_eq!(session.outline_entries().len(), 2);
	// The reference registers `fnref:1` between the two heading anchors; the
	// scan must not mistake it for an outline entry and stop there.
	session.scroll = layout.anchor_y("two").unwrap();
	assert_eq!(session.current_outline(), Some(1));
}

#[test]
fn the_outline_toggles_closes_and_moves_its_selection() {
	let mut interaction = InteractionState::default();
	assert!(interaction.toggle_outline(3, Some(1)));
	assert!(interaction.outline_open);
	assert_eq!(interaction.outline_selection, Some(1));
	assert!(!interaction.toggle_outline(3, Some(1)));
	assert!(!interaction.outline_open);
	assert_eq!(interaction.outline_selection, None);
	// Escape closes the drawer through the state, not through a panel.
	interaction.toggle_outline(3, Some(0));
	interaction.close_outline();
	assert!(!interaction.outline_open);
	assert_eq!(interaction.outline_selection, None);
	// Up/Down move and clamp the keyboard selection.
	interaction.toggle_outline(3, Some(0));
	assert!(interaction.move_outline(-1, 3));
	assert_eq!(interaction.outline_selection, Some(0));
	assert!(interaction.move_outline(1, 3));
	assert_eq!(interaction.outline_selection, Some(1));
	assert!(interaction.move_outline(9, 3));
	assert_eq!(interaction.outline_selection, Some(2));
	// A headingless document has nothing to select.
	assert!(!interaction.move_outline(1, 0));
	assert_eq!(interaction.outline_selection, None);
	// The drawer's own wheel scroll stays inside its content.
	interaction.scroll_outline(40.0, 100.0);
	assert_eq!(interaction.outline_scroll, 40.0);
	interaction.scroll_outline(500.0, 100.0);
	assert_eq!(interaction.outline_scroll, 100.0);
	interaction.scroll_outline(-500.0, 100.0);
	assert_eq!(interaction.outline_scroll, 0.0);
}

#[test]
fn enter_follows_the_outline_selection_after_a_click_and_a_step() {
	let mut interaction = InteractionState::default();
	assert!(interaction.toggle_outline(3, Some(0)));
	// A press on the first row leaves button focus there, as the pointer path
	// does, and Enter activates it.
	interaction.focus = Some(Command::OutlineGoto(0));
	let visible = || [Command::OutlineGoto(0), Command::OutlineGoto(1)];
	assert_eq!(
		interaction.enter_action(visible().into_iter()),
		Some(Command::OutlineGoto(0))
	);
	// Down moves the visible selection, and button focus follows it, so Enter
	// no longer jumps back to the row that was clicked.
	assert!(interaction.move_outline(1, 3));
	assert_eq!(interaction.outline_selection, Some(1));
	assert_eq!(interaction.focus, Some(Command::OutlineGoto(1)));
	assert_eq!(
		interaction.enter_action(visible().into_iter()),
		Some(Command::OutlineGoto(1))
	);
	// With no button focus, the selection is still what Enter activates.
	interaction.focus = None;
	assert_eq!(
		interaction.enter_action(visible().into_iter()),
		Some(Command::OutlineGoto(1))
	);
	// With nothing to select, the drawer leaves the focused button alone.
	interaction.outline_selection = None;
	interaction.focus = Some(Command::Outline);
	assert_eq!(
		interaction.enter_action([Command::Outline].into_iter()),
		Some(Command::Outline)
	);
}

#[test]
fn tabbing_through_the_drawer_marks_the_row_focus_lands_on() {
	let mut interaction = InteractionState::default();
	assert!(interaction.toggle_outline(3, Some(0)));
	let buttons = [
		Command::Outline,
		Command::OutlineGoto(0),
		Command::OutlineGoto(1),
		Command::OutlineGoto(2),
	];
	// Tab from the toolbar toggle lands on the first row and marks it.
	interaction.focus = Some(Command::Outline);
	assert_eq!(
		interaction.tab_focus(&buttons, false),
		Some(Command::OutlineGoto(0))
	);
	assert_eq!(interaction.outline_selection, Some(0));
	// Tabbing onward moves the visible selection with the focused row.
	assert_eq!(
		interaction.tab_focus(&buttons, false),
		Some(Command::OutlineGoto(1))
	);
	assert_eq!(interaction.outline_selection, Some(1));
	assert_eq!(
		interaction.tab_focus(&buttons, false),
		Some(Command::OutlineGoto(2))
	);
	assert_eq!(interaction.outline_selection, Some(2));
	// Shift+Tab walks back over the same rows.
	assert_eq!(
		interaction.tab_focus(&buttons, true),
		Some(Command::OutlineGoto(1))
	);
	assert_eq!(interaction.outline_selection, Some(1));
	// Tabbing onto a toolbar button leaves the drawer's selection where it is.
	assert_eq!(
		interaction.tab_focus(&buttons, true),
		Some(Command::OutlineGoto(0))
	);
	assert_eq!(
		interaction.tab_focus(&buttons, true),
		Some(Command::Outline)
	);
	assert_eq!(interaction.outline_selection, Some(0));
}

#[test]
fn a_panel_or_a_confirmation_stops_enter_from_reaching_the_outline() {
	let mut interaction = InteractionState::default();
	assert!(interaction.toggle_outline(3, Some(2)));
	// Ctrl+T opens Styles and clears button focus, leaving the selection
	// behind the panel intact.
	interaction.focus = None;
	let visible = || [Command::Styles, Command::Settings];
	// With only the drawer open, Enter still jumps to its selection.
	assert_eq!(
		interaction.enter_action(visible().into_iter()),
		Some(Command::OutlineGoto(2))
	);
	// A panel owns input: Enter without a focused panel button does nothing
	// instead of scrolling the document behind the panel.
	interaction.panel_open = true;
	assert!(!interaction.outline_owns_input());
	assert_eq!(interaction.enter_action(visible().into_iter()), None);
	// A visible panel button still answers Enter while the panel is open.
	interaction.focus = Some(Command::Settings);
	assert_eq!(
		interaction.enter_action(visible().into_iter()),
		Some(Command::Settings)
	);
	// A confirmation owns input in the same way.
	interaction.focus = None;
	interaction.panel_open = false;
	interaction.modal = Some(Modal::OpenLocal {
		path: "local.bin".into(),
		dir: ".".into(),
		document_dir: None,
	});
	assert_eq!(interaction.enter_action(visible().into_iter()), None);
	// With both gone, Enter reaches the drawer again.
	interaction.modal = None;
	assert_eq!(
		interaction.enter_action(visible().into_iter()),
		Some(Command::OutlineGoto(2))
	);
}

#[test]
fn a_long_outline_resolves_the_reading_position_in_one_pass() {
	let source: String = (0..1000)
		.map(|i| format!("# Heading {i}\n\nBody {i}.\n\n"))
		.collect();
	let document = Arc::new(document::parse(source));
	let layout = crate::layout::LayoutEngine::new()
		.layout(&document, &crate::test_support::options());
	let mut session = ReaderSession::default();
	session.accept(
		crate::worker::ReaderSnapshot {
			document,
			layout: layout.clone(),
			content_version: 1,
			complete: true,
			remote_deferred: 0,
		},
		600.,
		None,
	);
	session.ensure_outline();
	assert_eq!(session.outline_entries().len(), 1000);
	// Above the first heading the first section is the reading position.
	session.scroll = 0.0;
	assert_eq!(session.current_outline(), Some(0));
	// In the middle of a section its own heading holds the position.
	for index in [1_usize, 250, 500, 999] {
		let anchor = format!("heading-{index}");
		session.scroll = layout.anchor_y(&anchor).unwrap();
		assert_eq!(session.current_outline(), Some(index));
	}
}

#[test]
fn a_heading_nested_in_a_container_is_part_of_the_outline() {
	let document = Arc::new(document::parse("- ### Listed\n\n> ## Quoted\n"));
	let mut session = ReaderSession {
		document: Some(document),
		..Default::default()
	};
	session.ensure_outline();
	let entries: Vec<(&str, u8)> = session
		.outline_entries()
		.iter()
		.map(|entry| (entry.text.as_str(), entry.level))
		.collect();
	assert_eq!(entries, [("Listed", 3), ("Quoted", 2)]);
}

#[test]
fn buttons_activate_once_on_matching_release_and_cancel_outside() {
	for hovered in [Some(Command::Hyphens), Some(Command::CodeWrap), None] {
		let mut interaction = InteractionState {
			pressed: Some(Command::Hyphens),
			..Default::default()
		};
		assert_eq!(
			interaction.release_button(hovered),
			(hovered == Some(Command::Hyphens)).then_some(Command::Hyphens)
		);
		assert!(interaction.pressed.is_none());
		assert_eq!(interaction.release_button(Some(Command::Hyphens)), None);
	}
}

#[test]
fn a_diagonal_first_event_cannot_steal_the_gesture() {
	let mut wheel = WheelGesture::default();
	let start = Instant::now();
	// The first event leans sideways, but only by a pixel: it is held.
	assert_eq!(
		wheel.feed(2.0, 1.0, start, false, TouchPhase::Moved),
		WheelStep::Pending
	);
	// The gesture turns out to be vertical, and nothing it travelled is lost.
	assert_eq!(
		wheel.feed(
			1.0,
			9.0,
			start + Duration::from_millis(8),
			false,
			TouchPhase::Moved
		),
		WheelStep::Travel(WheelAxis::Vertical, 3.0, 10.0)
	);
}

#[test]
fn a_gesture_keeps_the_axis_it_started_with() {
	let mut wheel = WheelGesture::default();
	let start = Instant::now();
	assert_eq!(
		wheel.feed(12.0, 1.0, start, false, TouchPhase::Moved),
		WheelStep::Travel(WheelAxis::Horizontal, 12.0, 1.0)
	);
	// A later event that leans vertically does not flip the pan mid-gesture.
	assert_eq!(
		wheel.feed(
			2.0,
			20.0,
			start + Duration::from_millis(8),
			false,
			TouchPhase::Moved
		),
		WheelStep::Travel(WheelAxis::Horizontal, 2.0, 20.0)
	);
}

#[test]
fn a_pause_between_events_starts_a_new_gesture() {
	let mut wheel = WheelGesture::default();
	let start = Instant::now();
	assert_eq!(
		wheel.feed(12.0, 1.0, start, false, TouchPhase::Moved),
		WheelStep::Travel(WheelAxis::Horizontal, 12.0, 1.0)
	);
	assert_eq!(
		wheel.feed(
			1.0,
			12.0,
			start + Duration::from_millis(200),
			false,
			TouchPhase::Moved
		),
		WheelStep::Travel(WheelAxis::Vertical, 1.0, 12.0)
	);
}

#[test]
fn shift_wheel_is_sideways_without_waiting() {
	let mut wheel = WheelGesture::default();
	let start = Instant::now();
	// A wheel has no horizontal delta at all; Shift turns the vertical one
	// into a pan, so the page must not wait for a direction to emerge.
	assert_eq!(
		wheel.feed(0.0, 42.0, start, true, TouchPhase::Moved),
		WheelStep::Travel(WheelAxis::Horizontal, 0.0, 42.0)
	);
}

#[test]
fn motion_below_the_decision_threshold_is_held_then_applied() {
	let mut wheel = WheelGesture::default();
	let start = Instant::now();
	assert_eq!(
		wheel.feed(1.0, 1.0, start, false, TouchPhase::Moved),
		WheelStep::Pending
	);
	// The deciding event reports both events, so a slow start scrolls too.
	assert_eq!(
		wheel.feed(
			5.0,
			0.0,
			start + Duration::from_millis(8),
			false,
			TouchPhase::Moved
		),
		WheelStep::Travel(WheelAxis::Horizontal, 6.0, 1.0)
	);
}

#[test]
fn a_reported_gesture_boundary_separates_two_quick_gestures() {
	let mut wheel = WheelGesture::default();
	let start = Instant::now();
	// A sideways pan over a wide formula...
	assert_eq!(
		wheel.feed(14.0, 1.0, start, false, TouchPhase::Moved),
		WheelStep::Travel(WheelAxis::Horizontal, 14.0, 1.0)
	);
	// ...ends, and the very next gesture scrolls the page, well inside the
	// pause that would otherwise carry the axis over.
	let ends = start + Duration::from_millis(8);
	assert_eq!(
		wheel.feed(0.0, 0.0, ends, false, TouchPhase::Ended),
		WheelStep::Pending
	);
	assert_eq!(
		wheel.feed(
			1.0,
			14.0,
			ends + Duration::from_millis(8),
			false,
			TouchPhase::Started
		),
		WheelStep::Travel(WheelAxis::Vertical, 1.0, 14.0)
	);
}

#[test]
fn a_boundary_event_still_moves_with_the_gesture_it_ends() {
	let mut wheel = WheelGesture::default();
	let start = Instant::now();
	assert_eq!(
		wheel.feed(2.0, 1.0, start, false, TouchPhase::Moved),
		WheelStep::Pending
	);
	// The last event of a gesture carries its remaining motion.
	assert_eq!(
		wheel.feed(
			0.0,
			9.0,
			start + Duration::from_millis(8),
			false,
			TouchPhase::Ended
		),
		WheelStep::Travel(WheelAxis::Vertical, 2.0, 10.0)
	);
	// The axis did not survive the boundary.
	assert_eq!(
		wheel.feed(
			12.0,
			1.0,
			start + Duration::from_millis(16),
			false,
			TouchPhase::Moved
		),
		WheelStep::Travel(WheelAxis::Horizontal, 12.0, 1.0)
	);
}

#[test]
fn held_motion_does_not_survive_a_reported_boundary() {
	let mut wheel = WheelGesture::default();
	let start = Instant::now();
	// A gesture that never travels far enough to have a direction...
	assert_eq!(
		wheel.feed(2.0, 1.0, start, false, TouchPhase::Moved),
		WheelStep::Pending
	);
	// ...ends, and leaves nothing for the next gesture to inherit.
	assert_eq!(
		wheel.feed(
			0.0,
			0.0,
			start + Duration::from_millis(8),
			false,
			TouchPhase::Ended
		),
		WheelStep::Pending
	);
	// Five pixels of a new gesture are still five pixels, not seven.
	assert_eq!(
		wheel.feed(
			5.0,
			0.0,
			start + Duration::from_millis(16),
			false,
			TouchPhase::Started
		),
		WheelStep::Pending
	);
	assert_eq!(
		wheel.feed(
			1.0,
			0.0,
			start + Duration::from_millis(24),
			false,
			TouchPhase::Moved
		),
		WheelStep::Travel(WheelAxis::Horizontal, 6.0, 0.0)
	);
}

#[test]
fn held_motion_does_not_survive_a_pause_without_reported_boundaries() {
	let mut wheel = WheelGesture::default();
	let start = Instant::now();
	assert_eq!(
		wheel.feed(2.0, 1.0, start, false, TouchPhase::Moved),
		WheelStep::Pending
	);
	// With no boundary reported, the pause is the boundary.
	assert_eq!(
		wheel.feed(
			4.0,
			0.0,
			start + Duration::from_millis(200),
			false,
			TouchPhase::Moved
		),
		WheelStep::Pending
	);
	assert_eq!(
		wheel.feed(
			2.0,
			0.0,
			start + Duration::from_millis(208),
			false,
			TouchPhase::Moved
		),
		WheelStep::Travel(WheelAxis::Horizontal, 6.0, 0.0)
	);
}

#[test]
fn a_reported_gesture_is_not_cut_in_half_by_a_slow_moment() {
	let mut wheel = WheelGesture::default();
	let start = Instant::now();
	assert_eq!(
		wheel.feed(4.0, 1.0, start, false, TouchPhase::Started),
		WheelStep::Pending
	);
	// The fingers never lifted, so a slow moment is not a new gesture and its
	// motion still accumulates.
	assert_eq!(
		wheel.feed(
			2.0,
			0.0,
			start + Duration::from_millis(400),
			false,
			TouchPhase::Moved
		),
		WheelStep::Travel(WheelAxis::Horizontal, 6.0, 1.0)
	);
}
