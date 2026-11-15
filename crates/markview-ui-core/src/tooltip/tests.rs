use super::*;
use proptest::prelude::*;

const DELAY: Duration = Duration::from_millis(600);

#[test]
fn keyboard_focus_opens_a_pending_hover_without_reviving_a_dismissed_hint() {
	let now = Instant::now();
	let mut hint = Tooltip::default();
	hint.update(Some(1), now, DELAY);
	assert!(hint.update(Some(1), now + DELAY / 2, Duration::ZERO));
	assert_eq!(hint.visible(), Some(&1));
	assert_eq!(hint.deadline(), None);
	assert!(hint.dismiss());
	assert!(!hint.update(Some(1), now + DELAY, Duration::ZERO));
	assert_eq!(hint.visible(), None);
	assert_eq!(hint.deadline(), None);
}

#[test]
fn motion_preserves_the_delay_and_open_hints_have_no_deadline() {
	let now = Instant::now();
	let mut hint = Tooltip::default();
	assert!(!hint.update(Some(1), now, DELAY));
	for ms in [1, 200, 599] {
		let at = now + Duration::from_millis(ms);
		assert!(!hint.update(Some(1), at, DELAY));
		assert!(!hint.tick(at));
		assert_eq!(hint.deadline(), Some(now + DELAY));
		assert_eq!(hint.visible(), None);
	}
	assert!(hint.tick(now + DELAY));
	assert_eq!(hint.visible(), Some(&1));
	assert_eq!(hint.deadline(), None);
	assert!(!hint.tick(now + DELAY * 2));
}

#[test]
fn switching_leaving_and_dismissal_cancel_pending_hints() {
	let now = Instant::now();
	let mut hint = Tooltip::default();
	hint.update(Some(1), now, DELAY);
	hint.update(Some(2), now + DELAY / 2, DELAY);
	assert!(!hint.tick(now + DELAY));
	assert!(hint.tick(now + DELAY * 2));
	assert_eq!(hint.visible(), Some(&2));
	assert!(hint.dismiss());
	assert!(!hint.update(Some(2), now + DELAY * 3, Duration::ZERO));
	assert!(!hint.tick(now + DELAY * 4));
	assert_eq!(hint.visible(), None);
	assert_eq!(hint.deadline(), None);
	hint.update(None, now, DELAY);
	assert!(hint.update(Some(2), now, Duration::ZERO));
	assert_eq!(hint.visible(), Some(&2));
	assert!(hint.update(None, now, DELAY));
	assert_eq!(hint.visible(), None);
	hint.update(Some(3), now, DELAY);
	assert!(!hint.dismiss());
	assert!(!hint.tick(now + DELAY));
	assert_eq!(hint.deadline(), None);
}

proptest! {
	#[test]
	fn arbitrary_motion_never_opens_early_or_revives_a_dismissed_target(
		events in prop::collection::vec((0u8..6, 0u8..4, 0u64..1000), 0..100),
	) {
		let mut now = Instant::now();
		let mut hint = Tooltip::default();
		let mut target = None;
		let mut entered = now;
		let mut suppressed = false;
		for (action, key, elapsed) in events {
			now += Duration::from_millis(elapsed);
			match action {
				0..=3 => {
					let next = (action != 0).then_some(key);
					if next != target {
						entered = now;
						suppressed = false;
					}
					target = next;
					hint.update(next, now, DELAY);
				}
				4 => { hint.dismiss(); suppressed = true; }
				_ => { hint.tick(now); }
			}
			if target.is_none() || suppressed || now < entered + DELAY {
				prop_assert_eq!(hint.visible(), None);
			}
			if let Some(visible) = hint.visible() {
				prop_assert_eq!(Some(*visible), target);
				prop_assert_eq!(hint.deadline(), None);
			}
			if suppressed || target.is_none() {
				prop_assert_eq!(hint.deadline(), None);
			}
		}
	}
}
