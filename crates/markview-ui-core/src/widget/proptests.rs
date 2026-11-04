use super::*;
use proptest::prelude::*;

fn widget(key: usize, x: f32, enabled: bool, focusable: bool) -> Widget<u8> {
	Widget {
		id: WidgetId::new(0, key as u64),
		rect: Rect {
			x,
			y: 0.,
			w: 8.,
			h: 8.,
		},
		clip: None,
		enabled,
		focusable,
		message: 7,
	}
}
fn inside(r: Rect, p: (f32, f32)) -> bool {
	r.w > 0.
		&& r.h > 0.
		&& p.0 >= r.x
		&& p.0 <= r.x + r.w
		&& p.1 >= r.y
		&& p.1 <= r.y + r.h
}
proptest! {
	#[test]
	fn hits_and_focus_follow_layout_order(data in prop::collection::vec((-20i16..20,any::<bool>(),any::<bool>(),any::<bool>()),0..12), point in (-30i16..30,-10i16..20)) {
		let layout = Layout {
			widgets: data
				.iter()
				.enumerate()
				.map(|(i, &(x, e, f, clip))| {
					let mut w = widget(i, x as f32, e, f);
					w.clip = clip.then_some(Rect {
						x: 0.,
						y: 0.,
						w: 20.,
						h: 4.,
					});
					w
				})
				.collect(),
		};
		let p = (point.0 as f32, point.1 as f32);
		let expected = layout.widgets.iter().find(|w| {
			inside(w.rect, p)
				&& w.clip.is_none_or(|c| inside(c, p))
				&& w.clip.is_none_or(|c| {
					w.rect.x < c.x + c.w && w.rect.x + w.rect.w > c.x
				})
		});
		prop_assert_eq!(layout.hit(p).map(|w| w.id), expected.map(|w| w.id));
		let exact = layout.hit(p);
		let touch = layout.hit_touch(p, 16.);
		if let Some(exact) = exact {
			prop_assert_eq!(touch.map(|w| w.id), Some(exact.id));
		} else {
			let candidates: Vec<_> = layout
				.widgets
				.iter()
				.filter(|w| {
					let expanded = Rect {
						x: w.rect.x - 4.,
						y: -4.,
						w: 16.,
						h: 16.,
					};
					inside(expanded, p)
						&& w.clip.is_none_or(|c| inside(c, p))
						&& w.clip.is_none_or(|c| {
							w.rect.x < c.x + c.w && w.rect.x + w.rect.w > c.x
						})
				})
				.collect();
			prop_assert_eq!(touch.is_some(), !candidates.is_empty());
			if let Some(hit) = touch {
				let distance =
					|w: &&Widget<u8>| (p.0 - w.rect.x - 4.).hypot(p.1 - 4.);
				prop_assert!(
					candidates.iter().all(|w| distance(&hit) <= distance(w))
				);
			}
		}
		let ids: Vec<_> = layout
			.widgets
			.iter()
			.filter(|w| w.enabled && w.focusable)
			.map(|w| w.id)
			.collect();
		for backward in [false, true] {
			let mut current = None;
			for i in 0..ids.len() * 2 + 1 {
				let next = layout.next_focus(current, backward);
				let expected = if ids.is_empty() {
					None
				} else {
					Some(
						ids[if backward {
							ids.len() - 1 - i % ids.len()
						} else {
							i % ids.len()
						}],
					)
				};
				prop_assert_eq!(next, expected);
				current = next;
			}
		}
	}
	#[test]
	fn capture_tracks_identity_through_layout_changes(ops in prop::collection::vec((0u8..10,0usize..3,any::<u8>(),any::<bool>()),0..64)) {
		let mut layout = Layout {
			widgets: (0..3)
				.map(|i| widget(i, i as f32 * 20., true, true))
				.collect(),
		};
		let mut state = Interaction::default();
		let mut captured = None;
		for (step, (op, key, message, current_position)) in
			ops.into_iter().enumerate()
		{
			let x = if current_position {
				layout
					.widgets
					.iter()
					.find(|w| w.id.key == key as u64)
					.map_or(key as f32 * 20., |w| w.rect.x)
			} else {
				key as f32 * 20.
			};
			let point = (x + 4., 4.);
			let mut expected = None;
			let input = match op {
				0 => {
					captured = layout
						.widgets
						.iter()
						.find(|w| inside(w.rect, point) && w.enabled)
						.map(|w| w.id);
					Input::Press(point)
				}
				1 => {
					let hit = layout
						.widgets
						.iter()
						.find(|w| inside(w.rect, point) && w.enabled);
					expected =
						hit.filter(|w| Some(w.id) == captured).map(|w| w.message);
					captured = None;
					Input::Release(point)
				}
				2 => {
					captured = None;
					Input::Cancel
				}
				3 => {
					captured = None;
					Input::Blur
				}
				4 => {
					layout.widgets.retain(|w| w.id.key != key as u64);
					Input::Move(point)
				}
				5 => {
					for w in &mut layout.widgets {
						if w.id.key == key as u64 {
							w.enabled = false;
						}
					}
					Input::Move(point)
				}
				6 => {
					for w in &mut layout.widgets {
						if w.id.key == key as u64 {
							w.rect.x += 60.;
						}
					}
					Input::Move(point)
				}
				7 => {
					for w in &mut layout.widgets {
						if w.id.key == key as u64 {
							w.message = message;
						}
					}
					Input::Move(point)
				}
				_ => Input::Move(point),
			};
			captured = captured.filter(|id| {
				layout.widgets.iter().any(|w| w.id == *id && w.enabled)
			});
			prop_assert_eq!(
				state.handle(&layout, input),
				expected,
				"step={}",
				step
			);
			prop_assert_eq!(state.captured, captured, "step={}", step);
		}
	}
}

proptest! {
	#[test]
	fn keyboard_focus_reconciles_before_activation(ops in prop::collection::vec((0u8..6,0usize..3,any::<bool>()),0..64)) {
		let mut layout = Layout {
			widgets: (0..3)
				.map(|i| widget(i, i as f32 * 20., true, true))
				.collect(),
		};
		let mut state = Interaction::default();
		let mut focus = None;
		for (step, (op, key, backward)) in ops.into_iter().enumerate() {
			if op == 0 {
				layout.widgets[key].enabled = !layout.widgets[key].enabled;
			}
			if op == 1 {
				layout.widgets[key].focusable = !layout.widgets[key].focusable;
			}
			let ids: Vec<_> = layout
				.widgets
				.iter()
				.filter(|w| w.enabled && w.focusable)
				.map(|w| w.id)
				.collect();
			focus = focus.filter(|id| ids.contains(id));
			let mut expected = None;
			let input = if op == 2 {
				focus = if ids.is_empty() {
					None
				} else {
					let index =
						focus.and_then(|id| ids.iter().position(|&x| x == id));
					let index = match (index, backward) {
						(Some(i), false) => (i + 1) % ids.len(),
						(Some(i), true) => (i + ids.len() - 1) % ids.len(),
						(None, false) => 0,
						(None, true) => ids.len() - 1,
					};
					Some(ids[index])
				};
				Input::Navigate { backward }
			} else if op == 3 {
				expected = focus.map(|_| 7);
				Input::Activate
			} else if op == 4 {
				focus = None;
				Input::Blur
			} else {
				Input::Cancel
			};
			prop_assert_eq!(
				state.handle(&layout, input),
				expected,
				"step={}",
				step
			);
			prop_assert_eq!(state.focus, focus, "step={}", step);
		}
	}
}

proptest! {
	#[test]
	fn moved_capture_activates_with_current_message(key in 0usize..3, shift in 1u16..100, message in 0u8..7) {
		let mut layout = Layout {
			widgets: (0..3)
				.map(|i| widget(i, i as f32 * 20., true, true))
				.collect(),
		};
		let mut state = Interaction::default();
		let id = layout.widgets[key].id;
		let original = (layout.widgets[key].rect.x + 4., 4.);
		prop_assert_eq!(state.handle(&layout, Input::Press(original)), None);
		layout.widgets[key].rect.x += 60. + shift as f32;
		layout.widgets[key].message = message;
		state.reconcile(&layout);
		prop_assert_eq!(state.captured, Some(id));
		let moved = (layout.widgets[key].rect.x + 4., 4.);
		prop_assert_eq!(
			state.handle(&layout, Input::Release(moved)),
			Some(message)
		);
		prop_assert_eq!(state.handle(&layout, Input::Release(moved)), None);
		prop_assert_eq!(state.captured, None);
	}
}
