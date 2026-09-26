use super::*;

#[test]
fn editing_unicode_and_undo_preserve_selection_and_single_line_content() {
	let mut ui = TextShaper::new();
	let mut input = TextInput::default();
	input.insert(&mut ui, "你好 e\u{301} 👩‍💻", EditKind::Typing);
	input.move_cursor(&mut ui, Motion::Left, true);
	assert_eq!(input.selected_text(), Some("👩‍💻"));
	input.insert(&mut ui, "Rust\r\ntext\t!\u{2028}ok\0", EditKind::Separate);
	assert_eq!(input.text(), "你好 e\u{301} Rust text ! ok");
	input.undo(&mut ui, false);
	assert_eq!(input.selected_text(), Some("👩‍💻"));
	input.undo(&mut ui, true);
	assert_eq!(input.text(), "你好 e\u{301} Rust text ! ok");
	input.move_cursor(&mut ui, Motion::Start, false);
	input.move_cursor(&mut ui, Motion::Right, true);
	assert_eq!(input.selected_text(), Some("你"));
	input.delete(&mut ui, false, false);
	assert_eq!(input.text(), "好 e\u{301} Rust text ! ok");
}

#[test]
fn ime_replaces_selection_atomically_and_cancellation_restores_it() {
	let mut ui = TextShaper::new();
	let mut input = TextInput::default();
	input.set_text(&mut ui, "Original");
	input.select_all(&mut ui);
	input.preedit(&mut ui, "ni", Some((2, 2)));
	input.preedit(&mut ui, "你好", None);
	assert_eq!(input.text(), "Original");
	assert!(input.editor.cursor_geometry(1.0).is_none());
	input.cancel_compose(&mut ui);
	assert_eq!(input.selected_text(), Some("Original"));
	input.preedit(&mut ui, "ni", Some((2, 2)));
	input.preedit(&mut ui, "", None);
	input.commit(&mut ui, "你好");
	assert_eq!(input.text(), "你好");
	input.undo(&mut ui, false);
	assert_eq!(input.text(), "Original");
	assert_eq!(input.selected_text(), Some("Original"));
	input.undo(&mut ui, true);
	assert_eq!(input.text(), "你好");
	input.preedit(&mut ui, "hidden", None);
	input.set_text(&mut ui, "reset");
	assert!(input.editor.cursor_geometry(1.0).is_some());
}

#[test]
fn typed_runs_merge_but_motion_and_paste_split_history() {
	let mut ui = TextShaper::new();
	let mut input = TextInput::default();
	for text in ["a", "b", "c"] {
		input.insert(&mut ui, text, EditKind::Typing);
	}
	input.undo(&mut ui, false);
	assert_eq!(input.text(), "");
	input.undo(&mut ui, true);
	input.move_cursor(&mut ui, Motion::WordLeft, false);
	input.insert(&mut ui, "x", EditKind::Typing);
	input.insert(&mut ui, "paste", EditKind::Separate);
	input.undo(&mut ui, false);
	assert_eq!(input.text(), "xabc");
	input.undo(&mut ui, false);
	assert_eq!(input.text(), "abc");
	input.delete(&mut ui, false, true);
	assert_eq!(input.text(), "");
}

#[test]
fn layout_drives_bidi_hit_testing_and_long_input_scroll() {
	let mut ui = TextShaper::new();
	let mut input = TextInput::default();
	let rect = Rect {
		x: 20.,
		y: 30.,
		w: 120.,
		h: 32.,
	};
	input.set_text(
		&mut ui,
		"abc שלום 你好 👩‍💻 text that extends beyond the field",
	);
	let area = input.ime_area(&mut ui, rect);
	assert!(rect.contains(area.x, area.y));
	assert!(input.scroll > 0.0);
	input.move_cursor(&mut ui, Motion::Start, false);
	input.ime_area(&mut ui, rect);
	assert_eq!(input.scroll, 0.0);
	input.point(&mut ui, rect, (rect.x + 9., rect.y + 16.), false, 2);
	assert_eq!(input.selected_text(), Some("abc"));
	input.select_all(&mut ui);
	let draws = input.draw(&mut ui, rect, true, true, "");
	assert!(draws.iter().any(|d| matches!(d,Draw::Clipped{draws,..} if draws.iter().any(|d|matches!(d,Draw::Glyph(_))))));
	input.move_cursor(&mut ui, Motion::End, false);
	input.point(&mut ui, rect, (rect.x - 10., rect.y + 16.), true, 1);
	assert!(input.selected_text().is_some());
}
