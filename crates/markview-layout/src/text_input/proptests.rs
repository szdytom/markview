use super::*;
use proptest::prelude::*;

fn text() -> impl Strategy<Value = String> {
	prop::collection::vec(
		prop::sample::select(vec![
			"a",
			" ",
			"你",
			"é",
			"e\u{301}",
			"👩‍💻",
			"שלום",
		]),
		1..5,
	)
	.prop_map(|parts| parts.concat())
}

proptest! {
	#[test]
	fn ime_commit_cancel_and_directed_selections_follow_committed_history(
		operations in prop::collection::vec((0u8..6, 0usize..80, 0usize..80, "[a-z中é😀]{1,5}"), 1..25),
	) {
		// These scalars are independent graphemes with stable byte boundaries.
		let mut ui = TextShaper::new();
		let mut input = TextInput::default();
		let mut state = (String::new(), 0, 0);
		let mut undo = Vec::new();
		let mut redo = Vec::new();
		for (op, a, b, value) in operations {
			match op {
				0 | 1 => {
					let before = state.clone();
					for preedit in ["temporary", value.as_str()] {
						let boundaries: Vec<_> = preedit.char_indices().map(|(i, _)| i).chain([preedit.len()]).collect();
						let cursor = (boundaries[a % boundaries.len()], boundaries[b % boundaries.len()]);
						input.preedit(&mut ui, preedit, Some(cursor));
						prop_assert_eq!(input.text(), state.0.as_str());
						prop_assert!(input.is_composing());
						let start = state.1.min(state.2);
						let displayed = format!("{}{preedit}{}", &state.0[..start], &state.0[state.1.max(state.2)..]);
						prop_assert_eq!(input.editor.raw_text(), displayed);
						let selection = input.editor.raw_selection();
						prop_assert_eq!((selection.anchor().index(), selection.focus().index()), (start + cursor.0, start + cursor.1));
					}
					if op == 0 {
						input.commit(&mut ui, &value);
						let (start, end) = (state.1.min(state.2), state.1.max(state.2));
						state.0.replace_range(start..end, &value);
						state.1 = start + value.len(); state.2 = state.1;
						if state.0 != before.0 { undo.push(before); redo.clear(); }
					} else { input.cancel_compose(&mut ui); }
				}
				2 => {
					let boundaries: Vec<_> = state.0.char_indices().map(|(i, _)| i).chain([state.0.len()]).collect();
					state.1 = boundaries[a % boundaries.len()]; state.2 = boundaries[b % boundaries.len()];
					ui.input_driver(&mut input.editor).select_byte_range(state.1, state.2);
				}
				3 | 4 => {
					input.undo(&mut ui, op == 4);
					let next = if op == 4 { redo.pop() } else { undo.pop() };
					if let Some(next) = next {
						if op == 4 { undo.push(state); } else { redo.push(state); }
						state = next;
					}
				}
				_ => {
					let end = a % 2 == 0;
					let extend = b % 2 == 0;
					input.move_cursor(&mut ui, if end { Motion::End } else { Motion::Start }, extend);
					state.2 = if end { state.0.len() } else { 0 };
					if !extend { state.1 = state.2; }
				}
			}
			prop_assert!(!input.is_composing());
			prop_assert_eq!(input.text(), state.0.as_str());
			let selection = input.editor.raw_selection();
			prop_assert_eq!((selection.anchor().index(), selection.focus().index()), (state.1, state.2));
			prop_assert_eq!(input.selected_text(), (state.1 != state.2).then(|| &state.0[state.1.min(state.2)..state.1.max(state.2)]));
		}
	}

	#[test]
	fn normalization_preserves_text_and_spaces_and_converts_line_controls(
		parts in prop::collection::vec(prop_oneof![
			text().prop_map(|text| (text.clone(), text)),
			prop::sample::select(vec![
				("\r\n", " "), ("\r", " "), ("\n", " "), ("\t", " "),
				("\u{85}", " "), ("\u{2028}", " "), ("\u{2029}", " "),
				("\0", ""), ("\u{7f}", ""), ("\u{1b}", ""), ("  ", "  "),
			]).prop_map(|(a, b)| (a.to_owned(), b.to_owned())),
		], 0..32),
	) {
		let input = parts.iter().map(|(a, _)| a.as_str()).collect::<Vec<_>>().join("_");
		let expected = parts.iter().map(|(_, b)| b.as_str()).collect::<Vec<_>>().join("_");
		prop_assert_eq!(single_line(&input), expected.as_str());
		prop_assert_eq!(single_line(&expected), expected);
	}

	#[test]
	fn separate_edits_undo_redo_reset_and_ime_follow_snapshot_history(
		operations in prop::collection::vec((0u8..7, text()), 0..40),
	) {
		let mut ui = TextShaper::new();
		let mut input = TextInput::default();
		let mut state = (String::new(), 0, 0);
		let mut undo = Vec::new();
		let mut redo = Vec::new();
		for (op, value) in operations {
			match op {
				0 | 1 => {
					if op == 0 {
						input.move_cursor(&mut ui, Motion::End, false);
						state.1 = state.0.len();
						state.2 = state.0.len();
					} else {
						input.select_all(&mut ui);
						state.1 = 0;
						state.2 = state.0.len();
					}
					let before = state.clone();
					input.insert(&mut ui, &value, EditKind::Separate);
					if op == 0 { state.0.push_str(&value); } else { state.0 = value; }
					state.1 = state.0.len();
					state.2 = state.0.len();
					if before.0 != state.0 {
						undo.push(before);
						redo.clear();
					}
				}
				2 => {
					input.undo(&mut ui, false);
					if let Some(previous) = undo.pop() {
						redo.push(state);
						state = previous;
					}
				}
				3 => {
					input.undo(&mut ui, true);
					if let Some(next) = redo.pop() {
						undo.push(state);
						state = next;
					}
				}
				4 => {
					input.set_text(&mut ui, &value);
					state = (value.clone(), value.len(), value.len());
					undo.clear();
					redo.clear();
				}
				5 => {
					for preedit in [&value, "temporary"] {
						input.preedit(&mut ui, preedit, None);
						prop_assert!(input.is_composing());
						prop_assert_eq!(input.text(), state.0.as_str());
					}
					input.cancel_compose(&mut ui);
					input.cancel_compose(&mut ui);
				}
				_ => {
					input.select_all(&mut ui);
					state.1 = 0;
					state.2 = state.0.len();
				}
			}
			prop_assert!(!input.is_composing());
			prop_assert_eq!(input.text(), state.0.as_str());
			let selection = input.editor.raw_selection();
			prop_assert_eq!((selection.anchor().index(), selection.focus().index()), (state.1, state.2));
			let expected = (state.1 != state.2).then(|| &state.0[state.1.min(state.2)..state.1.max(state.2)]);
			prop_assert_eq!(input.selected_text(), expected);
		}
	}

	#[test]
	fn surrounding_deletion_counts_scalar_or_utf16_units_and_undo_restores_selection(
		left in text(), selected in text(), right in text(),
		before in 0usize..24, after in 0usize..24, code_points in any::<bool>(), reverse in any::<bool>(),
	) {
		let mut ui = TextShaper::new();
		let mut input = TextInput::default();
		let original = format!("{left}{selected}{right}");
		input.set_text(&mut ui, &original);
		let (a, b) = (left.len(), left.len() + selected.len());
		let (anchor, focus) = if reverse { (b, a) } else { (a, b) };
		ui.input_driver(&mut input.editor).select_byte_range(anchor, focus);
		// Prefix sums count complete scalars, rounding an interior UTF-16 unit outward.
		let keep = |text: &str, count: usize, backwards: bool| {
			let mut chars: Vec<_> = text.chars().collect();
			if backwards { chars.reverse(); }
			let mut units = vec![0];
			for c in &chars {
				units.push(units.last().unwrap() + if code_points { 1 } else { c.len_utf16() });
			}
			let removed = units.iter().position(|&n| n >= count).unwrap_or(chars.len());
			let mut kept = chars[removed..].to_vec();
			if backwards { kept.reverse(); }
			kept.into_iter().collect::<String>()
		};
		let prefix = keep(&left, before, true);
		let suffix = keep(&right, after, false);
		input.delete_surrounding(&mut ui, before, after, code_points);
		prop_assert_eq!(input.text(), format!("{prefix}{selected}{suffix}"));
		input.undo(&mut ui, false);
		prop_assert_eq!(input.text(), original.as_str());
		let selection = input.editor.raw_selection();
		prop_assert_eq!((selection.anchor().index(), selection.focus().index()), (anchor, focus));
	}
	#[test]
	fn surrounding_deletion_preserves_selection_when_a_combining_mark_is_exposed(
		prefix in "[a-z]{1,8}", selected in "[a-z]{1,8}", reverse in any::<bool>(),
	) {
		let mut ui = TextShaper::new();
		let mut input = TextInput::default();
		input.set_text(&mut ui, &format!("{prefix}{selected}e\u{301}"));
		let (a, b) = (prefix.len(), prefix.len() + selected.len());
		let (anchor, focus) = if reverse { (b, a) } else { (a, b) };
		ui.input_driver(&mut input.editor).select_byte_range(anchor, focus);
		input.delete_surrounding(&mut ui, 0, 1, false);
		prop_assert_eq!(input.text(), format!("{prefix}{selected}\u{301}"));
		let expanded = format!("{selected}\u{301}");
		prop_assert_eq!(input.selected_text(), Some(expanded.as_str()));
		let selection = input.editor.raw_selection();
		prop_assert_eq!(selection.anchor().index() > selection.focus().index(), reverse);
		input.undo(&mut ui, false);
		prop_assert_eq!(input.selected_text(), Some(selected.as_str()));
		input.undo(&mut ui, true);
		prop_assert_eq!(input.selected_text(), Some(expanded.as_str()));
	}
}
