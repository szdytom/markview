//! The DM viewport feed folded into pixel-pan phases: pan in contact, the OS
//! inertia tail, a release at idle, and a gesture interrupted by abandon.
use markview_selection::{PanFeed, PanPhase, PanStatus};

#[test]
fn a_pan_in_contact_follows_the_content_transform_exactly() {
	let mut feed = PanFeed::default();
	assert_eq!(
		feed.transform(9.0, 9.0),
		None,
		"a transform outside a gesture is ignored"
	);
	assert_eq!(feed.status(PanStatus::Building), Some(PanPhase::Started));
	// The content rests at its origin between gestures, so the origin's own
	// report carries no travel.
	assert_eq!(feed.transform(0.0, 0.0), Some((0.0, 0.0)));
	assert_eq!(feed.transform(12.0, 4.0), Some((12.0, 4.0)));
	assert_eq!(feed.transform(15.0, 1.0), Some((3.0, -3.0)));
}

#[test]
fn the_first_transform_of_a_gesture_carries_its_full_travel() {
	let mut feed = PanFeed::default();
	// The hand does not wait for the seam: the first pump of a contact may
	// already name travel, and none of it may be swallowed as a baseline.
	assert_eq!(feed.status(PanStatus::Running), Some(PanPhase::Started));
	assert_eq!(feed.transform(20.0, 0.0), Some((20.0, 0.0)));
	assert_eq!(feed.transform(25.0, 0.0), Some((5.0, 0.0)));
}

#[test]
fn the_inertia_tail_keeps_moving_and_idle_releases_exactly_once() {
	let mut feed = PanFeed::default();
	feed.status(PanStatus::Running);
	feed.transform(0.0, 0.0);
	feed.transform(20.0, 0.0);
	// The hand lifted; the OS inertia engine keeps the updates coming and
	// decaying, still as the same gesture.
	assert_eq!(feed.status(PanStatus::Inertia), None);
	assert_eq!(feed.transform(26.0, 1.0), Some((6.0, 1.0)));
	assert_eq!(feed.transform(28.0, 1.0), Some((2.0, 0.0)));
	// A reversal inside the inertia is the same gesture, not a second start.
	assert_eq!(feed.status(PanStatus::Running), None);
	assert_eq!(feed.transform(24.0, 1.0), Some((-4.0, 0.0)));
	// Returning to idle releases once; anything after speaks of nothing.
	assert_eq!(feed.status(PanStatus::Ready), Some(PanPhase::Ended));
	assert_eq!(feed.transform(23.0, 1.0), None);
	assert_eq!(feed.status(PanStatus::Ready), None);
}

#[test]
fn a_gesture_interrupted_by_abandon_cancels_and_stays_quiet() {
	let mut feed = PanFeed::default();
	feed.status(PanStatus::Running);
	feed.transform(0.0, 0.0);
	assert_eq!(feed.abandon(), Some(PanPhase::Cancelled));
	// The viewport is gone; a trailing status or transform reopens nothing.
	assert_eq!(feed.status(PanStatus::Ready), None);
	assert_eq!(feed.transform(5.0, 0.0), None);
	assert_eq!(feed.abandon(), None);
	// A new gesture starts clean, from the origin the reset parks the
	// content at.
	assert_eq!(feed.status(PanStatus::Running), Some(PanPhase::Started));
	assert_eq!(feed.transform(0.0, 0.0), Some((0.0, 0.0)));
	assert_eq!(feed.transform(2.0, 0.0), Some((2.0, 0.0)));
}

#[test]
fn activation_passing_through_building_to_ready_starts_and_releases_once() {
	let mut feed = PanFeed::default();
	assert_eq!(feed.status(PanStatus::Building), Some(PanPhase::Started));
	assert_eq!(feed.status(PanStatus::Ready), Some(PanPhase::Ended));
	assert_eq!(feed.status(PanStatus::Ready), None);
}

#[test]
fn a_disabled_viewport_cancels_a_running_gesture() {
	let mut feed = PanFeed::default();
	feed.status(PanStatus::Running);
	assert_eq!(feed.status(PanStatus::Disabled), Some(PanPhase::Cancelled));
	assert_eq!(feed.status(PanStatus::Running), Some(PanPhase::Started));
}
