use super::*;
use crate::{
	app::chrome::{controls, export},
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
			assert_eq!(initial.max_scroll > 0.0, height < 800.0);
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
