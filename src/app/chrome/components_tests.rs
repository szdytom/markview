use super::*;
use crate::{
	app::chrome::{controls, export},
	lang::Lang,
	settings::{ExportFormat, ExportSettings, ReaderSettings},
};

#[test]
fn every_form_action_is_reachable_without_clicking_through_the_clip() {
	let mut ui = crate::test_support::shaper();
	for (width, height) in [(500.0, 300.0), (820.0, 600.0), (1200.0, 800.0)] {
		for exporting in [false, true] {
			let build = |ui: &mut TextShaper, scroll| {
				if exporting {
					export::form(
						ui,
						&ExportSettings {
							format: ExportFormat::Png,
							..Default::default()
						},
						scroll,
						width,
						height,
						Lang::En,
					)
				} else {
					controls::form(
						ui,
						&ReaderSettings::default(),
						scroll,
						width,
						height,
					)
				}
			};
			let initial = build(&mut ui, 0.0);
			// The panel is capped at 620 logical pixels whatever the window, so
			// both pages scroll in a short one. In a tall one the export page
			// still fits whole; the General page does not any more, now that it
			// carries the Interface row beside the rest.
			assert_eq!(initial.max_scroll > 0.0, height < 800.0 || !exporting);
			for button in &initial.buttons {
				assert_eq!(button.rect.h, CONTROL);
				let revealed = build(&mut ui, initial.reveal(button.action));
				let visible = revealed.visible_buttons();
				let b = visible
					.iter()
					.find(|b| b.action == button.action)
					.expect("focus reveals the entire control");
				assert_eq!(b.rect.h, CONTROL);
				assert!(revealed.rect.contains(b.rect.x, b.rect.y));
				assert!(
					revealed
						.rect
						.contains(b.rect.x + b.rect.w, b.rect.y + b.rect.h)
				);
				for (i, a) in visible.iter().enumerate() {
					for b in &visible[i + 1..] {
						assert!(
							a.rect.intersect(b.rect).is_none(),
							"overlapping commands: {:?}, {:?}",
							a.action,
							b.action
						);
					}
				}
			}
			let scrolled = build(&mut ui, f32::MAX);
			assert_eq!(scrolled.scroll, scrolled.max_scroll);
			let close = scrolled
				.visible_buttons()
				.into_iter()
				.find(|b| b.label == "Close")
				.unwrap();
			assert_eq!(close.rect.y, initial.buttons[0].rect.y);
		}
	}
}

#[test]
fn button_feedback_distinguishes_hover_press_selection_and_keyboard_focus() {
	for dark in [false, true] {
		let mut ui = crate::test_support::shaper();
		ui.set_stylesheet(markview_core::style::Stylesheet::bundled(dark));
		for (kind, selected) in [
			(ButtonKind::Standard, false),
			(ButtonKind::Standard, true),
			(ButtonKind::Primary, false),
			(ButtonKind::Quiet, false),
		] {
			let mut b = button(
				"On",
				Command::Hyphens,
				Rect {
					x: 20.0,
					y: 30.0,
					w: 120.0,
					h: CONTROL,
				},
			);
			b.kind = kind;
			b.active = selected;
			let mouse = InteractionState {
				focus: Some(b.action),
				..Default::default()
			};
			let hover = InteractionState {
				cursor: (24.0, 34.0),
				focus: Some(b.action),
				..Default::default()
			};
			let held = InteractionState {
				cursor: hover.cursor,
				pressed: Some(b.action),
				focus: Some(b.action),
				..Default::default()
			};
			let mut fills = Vec::new();
			for state in [&InteractionState::default(), &mouse, &hover, &held] {
				let draws = draw_button(&mut ui, state, &b, true);
				let fill = draws.iter().find_map(|draw| match draw {
					Draw::Rect(rect, paint)
						if rect.h == b.rect.h && rect.w == b.rect.w =>
					{
						Some(ui.stylesheet.paint(*paint))
					}
					_ => None,
				});
				fills.push(fill);
				assert!(!draws.iter().any(|draw| matches!(
					draw,
					Draw::Rect(
						_,
						Paint::Styled(Condition::Button, C::FocusColor)
					)
				)));
			}
			assert_eq!(
				fills[0], fills[1],
				"mouse focus must not leave a visual ring"
			);
			assert_ne!(
				fills[1], fills[2],
				"selected and primary controls also respond to hover"
			);
			assert_ne!(
				fills[2], fills[3],
				"held controls have a distinct fill"
			);
			let keyboard = InteractionState {
				focus: Some(b.action),
				focus_visible: true,
				..Default::default()
			};
			let draws = draw_button(&mut ui, &keyboard, &b, true);
			let edges: Vec<_> = draws
				.iter()
				.filter_map(|d| match d {
					Draw::Rect(r, _) if r.w < b.rect.w || r.h < b.rect.h => {
						Some(r)
					}
					_ => None,
				})
				.collect();
			assert_eq!(
				edges.len(),
				4,
				"focus replaces the border with one outline"
			);
			assert!(edges.iter().all(|r| r.x == b.rect.x
				|| r.y == b.rect.y
				|| r.x + r.w == b.rect.x + b.rect.w
				|| r.y + r.h == b.rect.y + b.rect.h));
			b.enabled = false;
			let idle =
				draw_button(&mut ui, &InteractionState::default(), &b, true);
			let held = draw_button(&mut ui, &held, &b, true);
			let rects = |draws: Vec<Draw>| {
				draws
					.into_iter()
					.filter_map(|d| {
						if let Draw::Rect(r, p) = d {
							Some((r.x, r.y, r.w, r.h, p))
						} else {
							None
						}
					})
					.collect::<Vec<_>>()
			};
			assert_eq!(
				rects(idle),
				rects(held),
				"disabled controls ignore hover and press"
			);
		}
	}
}

#[test]
fn settings_and_export_step_controls_draw_icons_without_font_glyphs() {
	let mut ui = crate::test_support::shaper();
	let forms = [
		controls::form(&mut ui, &ReaderSettings::default(), 0., 1200., 800.),
		export::form(
			&mut ui,
			&ExportSettings::default(),
			0.,
			1200.,
			800.,
			Lang::En,
		),
	];
	for command in [
		Command::Smaller,
		Command::Larger,
		Command::Narrower,
		Command::Wider,
		Command::ScrollSpeed(-1),
		Command::ScrollSpeed(1),
		Command::ExportSize(-1),
		Command::ExportSize(1),
	] {
		let button = forms
			.iter()
			.flat_map(|form| &form.buttons)
			.find(|button| button.action == command)
			.unwrap();
		let draws =
			draw_button(&mut ui, &InteractionState::default(), button, true);
		assert!(draws.iter().any(|draw| matches!(draw, Draw::Icon { .. })));
		assert!(!draws.iter().any(|draw| matches!(draw, Draw::Glyph(_))));
	}
}

#[test]
fn segmented_choices_draw_each_shared_edge_once() {
	let mut ui = crate::test_support::shaper();
	for selected in 0..4 {
		for focus in [None, Some(0), Some(1), Some(2), Some(3)] {
			let form = Form::new(
				1200.0,
				800.0,
				0.0,
				vec![Row::new(
					"Indent",
					(0..4)
						.map(|i| {
							action("Choice", i == selected, Command::Indent(i))
						})
						.collect(),
				)],
				None,
				false,
				Lang::En,
			);
			let state = InteractionState {
				focus: focus.map(Command::Indent),
				focus_visible: focus.is_some(),
				..Default::default()
			};
			let draws =
				form.draw(&mut ui, &state, "", "", C::Muted, (1200.0, 800.0));
			let body = draws
				.iter()
				.find_map(|draw| match draw {
					Draw::Clipped { draws, .. } => Some(draws),
					_ => None,
				})
				.unwrap();
			let edges: Vec<_> = body
				.iter()
				.filter_map(|draw| match draw {
					Draw::Rect(
						rect,
						Paint::Styled(Condition::Button, color),
					) if rect.h == CONTROL && rect.w <= 2.0 => Some((rect, color)),
					_ => None,
				})
				.collect();
			assert_eq!(
				edges.len(),
				5,
				"four segments need five vertical edges"
			);
			for pair in form.buttons.windows(2) {
				let boundary = pair[1].rect.x;
				let shared: Vec<_> = edges
					.iter()
					.filter(|(r, _)| r.x >= boundary - 2.0 && r.x <= boundary)
					.collect();
				assert_eq!(shared.len(), 1);
				let focused =
					pair.iter().any(|b| state.focus == Some(b.action));
				let selected = pair.iter().any(|b| b.active);
				assert_eq!(
					*shared[0].1,
					if focused {
						C::FocusColor
					} else if selected {
						C::Accent
					} else {
						C::BorderColor
					}
				);
				assert_eq!(shared[0].0.w, if focused { 2.0 } else { 1.0 });
			}
		}
	}
}

/// A form holding one list row and `fillers` plain rows around it.
fn list_form_at(
	count: usize,
	fillers: usize,
	scroll: f32,
	menu_first: bool,
	size: (f32, f32),
) -> Form {
	const OPTIONS: [&str; 10] = [
		"one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
		"ten",
	];
	let entries = OPTIONS[..count]
		.iter()
		.enumerate()
		.map(|(index, label)| {
			action(label, index == 0, Command::Indent(index as u8))
		})
		.collect();
	let mut rows: Vec<Row> = (0..fillers)
		.map(|index| Row::new(format!("row {index}"), vec![]))
		.collect();
	let list = Row::new("List", vec![]).menu(DropdownId::Language, entries);
	if menu_first {
		rows.insert(0, list);
	} else {
		rows.push(list);
	}
	Form::new(size.0, size.1, scroll, rows, None, true, Lang::En)
}

/// A form holding one list row, behind `fillers` plain rows.
fn list_form(count: usize, fillers: usize, size: (f32, f32)) -> Form {
	list_form_at(count, fillers, 0.0, false, size)
}

/// A row the page scrolled past holds no list: there is no anchor left to
/// hang it from, so measuring finds nothing instead of placing the list where
/// the control used to be.
#[test]
fn a_list_whose_row_left_the_viewport_is_not_measured() {
	let size = (820.0, 600.0);
	let mut open = Dropdown::new(DropdownId::Language, 0);
	assert!(
		list_form_at(3, 20, 0.0, true, size)
			.menu(&mut open, size)
			.is_some(),
		"the row is on screen, so its list opens"
	);
	assert!(
		list_form_at(3, 20, 100.0, true, size)
			.menu(&mut open, size)
			.is_none(),
		"a row the page scrolled away holds no list"
	);
}

/// An option list floats over the page, so it is the one thing in the chrome
/// that has to be measured against the window rather than against the panel.
#[test]
fn an_open_option_list_stays_inside_the_window() {
	for (width, height) in [(500.0, 300.0), (820.0, 600.0), (1200.0, 800.0)] {
		let size = (width, height);
		let window = Rect {
			x: 0.0,
			y: 0.0,
			w: width,
			h: height,
		};
		for (count, fillers) in [(1, 0), (3, 0), (6, 0), (10, 0), (10, 6)] {
			for highlight in [0, count - 1] {
				let form = list_form(count, fillers, size);
				let where_ = format!(
					"{width}x{height}, {count} options, {fillers} above"
				);
				let anchor = form
					.buttons
					.iter()
					.find(|b| {
						matches!(
							b.action,
							Command::ToggleDropdown(DropdownId::Language, _)
						)
					})
					.expect("the control that opens the list")
					.rect;
				let mut open = Dropdown::new(DropdownId::Language, highlight);
				let menu = form.menu(&mut open, size);
				// A row the page does not show holds no list, so the cases
				// whose anchor is off the page measure nothing.
				if anchor.intersect(form.viewport).is_none() {
					assert!(
						menu.is_none(),
						"{where_}: a list with no visible row measured"
					);
					continue;
				}
				let menu = menu.expect("the row opens a list");
				assert!(
					window.contains(menu.rect.x, menu.rect.y)
						&& window.contains(
							menu.rect.x + menu.rect.w,
							menu.rect.y + menu.rect.h
						),
					"{where_}: the list leaves the window"
				);
				assert!(
					!menu.buttons.is_empty(),
					"{where_}: the list is empty"
				);
				assert!(
					menu.buttons.len() <= count,
					"{where_}: more options drawn than declared"
				);
				for button in &menu.buttons {
					assert!(
						menu.rect.contains(button.rect.x, button.rect.y)
							&& menu.rect.contains(
								button.rect.x + button.rect.w,
								button.rect.y + button.rect.h
							),
						"{where_}: an option leaves the list"
					);
				}
				for (index, a) in menu.buttons.iter().enumerate() {
					for b in &menu.buttons[index + 1..] {
						assert!(
							a.rect.intersect(b.rect).is_none(),
							"{where_}: two options overlap"
						);
					}
				}
				assert!(
					anchor.intersect(menu.rect).is_none(),
					"{where_}: the list covers its own control"
				);
				assert!(
					menu.chosen().is_some(),
					"{where_}: no option under the keyboard"
				);
			}
		}
	}
}
