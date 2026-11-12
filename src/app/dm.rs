//! Windows Direct Manipulation panning.
//!
//! The pump policy — folding viewport statuses and content transforms into
//! pixel-pan phases and deltas — is platform-neutral so it tests everywhere;
//! the COM assembly supplies batches from `markview-platform`.

use markview_platform::event::TouchPhase;

use markview_interaction::{PanFeed, PanPhase, PanStatus};

/// Folds one pump's worth of viewport news into seam events, in logical
/// pixels.
///
/// The statuses of a batch come first, but a transform observed in the same
/// batch belongs to the gesture it closed: its delta is fed before the
/// release, so the last motion of a gesture is not orphaned after its
/// `Ended`.
///
/// Travel finer than one physical pixel cannot change what the reader shows,
/// so it waits in `carry` until it adds up to one — the OS inertia tail
/// decays for seconds through deltas a fraction of a pixel wide, and feeding
/// each one would redraw the page for nothing. A release flushes whatever is
/// left, so the page lands exactly where the viewport did.
pub(crate) fn fold(
	feed: &mut PanFeed,
	statuses: Vec<PanStatus>,
	transform: Option<(f32, f32)>,
	scale: f32,
	carry: &mut (f32, f32),
) -> Vec<(TouchPhase, f32, f32)> {
	let release = statuses
		.iter()
		.position(|s| {
			!matches!(
				s,
				PanStatus::Building | PanStatus::Running | PanStatus::Inertia
			)
		})
		.unwrap_or(statuses.len());
	let (opening, closing) = statuses.split_at(release);
	let mut events: Vec<_> = phases(feed, opening.iter().copied()).collect();
	if let Some((x, y)) = transform
		&& let Some((dx, dy)) = feed.transform(x / scale, y / scale)
	{
		carry.0 += dx;
		carry.1 += dy;
	}
	let quantum = 1.0 / scale;
	if carry.0.abs() >= quantum || carry.1.abs() >= quantum {
		log::debug!("pan delta ({:.1},{:.1})", carry.0, carry.1);
		events.push((TouchPhase::Moved, carry.0, carry.1));
		*carry = (0.0, 0.0);
	}
	if !closing.is_empty() && *carry != (0.0, 0.0) {
		log::debug!("pan delta ({:.1},{:.1})", carry.0, carry.1);
		events.push((TouchPhase::Moved, carry.0, carry.1));
		*carry = (0.0, 0.0);
	}
	events.extend(phases(feed, closing.iter().copied()));
	events
}

/// The phases of a status run, in order.
fn phases(
	feed: &mut PanFeed,
	statuses: impl Iterator<Item = PanStatus>,
) -> impl Iterator<Item = (TouchPhase, f32, f32)> {
	statuses
		.filter_map(|s| feed.status(s).map(touch))
		.map(|phase| (phase, 0.0, 0.0))
}

/// Ends the gesture a held stream was carrying, silencing it: a
/// pointer-driven interaction owns the input, the gesture in flight speaks
/// its `Cancelled` once, and its deltas drop — so nothing stale resumes
/// panning after the owner lets go.
fn hold(feed: &mut PanFeed) -> Vec<(TouchPhase, f32, f32)> {
	feed.abandon()
		.map(|phase| vec![(touch(phase), 0.0, 0.0)])
		.unwrap_or_default()
}

/// The pan lifecycle in the seam's terms; the two spell the same three
/// moments.
fn touch(phase: PanPhase) -> TouchPhase {
	match phase {
		PanPhase::Started => TouchPhase::Started,
		PanPhase::Ended => TouchPhase::Ended,
		PanPhase::Cancelled => TouchPhase::Cancelled,
	}
}

pub(super) struct DirectManipulation {
	native: markview_platform::dm::DirectManipulation,
	feed: PanFeed,
	carry: (f32, f32),
}
impl DirectManipulation {
	pub fn new(window: &dyn winit::window::Window) -> Option<Self> {
		Some(Self {
			native: markview_platform::dm::DirectManipulation::new(window)?,
			feed: PanFeed::default(),
			carry: (0.0, 0.0),
		})
	}
	pub fn take_offered_pointers() -> Vec<u32> {
		markview_platform::dm::DirectManipulation::take_offered_pointers()
	}
	pub fn contact(&mut self, pointer: u32) {
		self.native.contact(pointer);
	}
	pub fn resize(&mut self, width: u32, height: u32, scale: f32) {
		self.native.resize(width, height, scale);
	}
	pub fn abandon(&mut self) {
		let _ = self.feed.abandon();
		self.carry = (0.0, 0.0);
		self.native.abandon();
	}
	pub fn deadline(
		&self,
		now: std::time::Instant,
	) -> Option<std::time::Instant> {
		self.native.deadline(now)
	}
	pub fn pump(&mut self, held: bool) -> Vec<(TouchPhase, f32, f32)> {
		let batch = self.native.pump();
		if held {
			self.carry = (0.0, 0.0);
			return hold(&mut self.feed);
		}
		let statuses: Vec<_> = batch
			.statuses
			.into_iter()
			.map(|s| match s {
				markview_platform::dm::Status::Building => PanStatus::Building,
				markview_platform::dm::Status::Running => PanStatus::Running,
				markview_platform::dm::Status::Inertia => PanStatus::Inertia,
				markview_platform::dm::Status::Ready => PanStatus::Ready,
				markview_platform::dm::Status::Disabled => PanStatus::Disabled,
			})
			.collect();
		if statuses.is_empty() && batch.transform.is_none() {
			return Vec::new();
		}
		fold(
			&mut self.feed,
			statuses,
			batch.transform,
			batch.scale,
			&mut self.carry,
		)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn moved(events: &[(TouchPhase, f32, f32)]) -> Vec<(f32, f32)> {
		events
			.iter()
			.filter(|(phase, _, _)| matches!(phase, TouchPhase::Moved))
			.map(|(_, dx, dy)| (*dx, *dy))
			.collect()
	}

	#[test]
	fn a_batch_that_opens_the_gesture_pans_by_the_travel_it_carries() {
		let mut feed = PanFeed::default();
		let mut carry = (0.0, 0.0);
		let statuses = vec![PanStatus::Running];
		// Physical pixels on a 2× display: the contact's first pump may
		// already name travel, and the content rested at its origin, so it
		// pans in full.
		let events =
			fold(&mut feed, statuses, Some((200.0, -60.0)), 2.0, &mut carry);
		assert_eq!(
			events,
			vec![
				(TouchPhase::Started, 0.0, 0.0),
				(TouchPhase::Moved, 100.0, -30.0),
			]
		);
		// The origin's own report carries no travel.
		let events =
			fold(&mut feed, vec![], Some((200.0, -60.0)), 2.0, &mut carry);
		assert!(events.is_empty());
	}

	#[test]
	fn sub_pixel_travel_waits_to_add_up_to_one_shown_pixel() {
		let mut feed = PanFeed::default();
		let mut carry = (0.0, 0.0);
		let statuses = vec![PanStatus::Running];
		// On a 2× display one physical pixel is half a logical one, so a
		// tail decaying through fifth-of-a-pixel deltas lights nothing yet.
		let events =
			fold(&mut feed, statuses, Some((0.4, 0.0)), 2.0, &mut carry);
		assert_eq!(events, vec![(TouchPhase::Started, 0.0, 0.0)]);
		// The tail's residue adds up to a whole pixel and pans in one piece.
		let events = fold(&mut feed, vec![], Some((1.2, 0.0)), 2.0, &mut carry);
		assert_eq!(moved(&events), vec![(0.6, 0.0)]);
		assert_eq!(carry, (0.0, 0.0));
	}

	#[test]
	fn a_release_flushes_the_travel_a_quiet_tail_left_behind() {
		let mut feed = PanFeed::default();
		let mut carry = (0.0, 0.0);
		let statuses = vec![PanStatus::Running];
		fold(&mut feed, statuses, Some((0.4, 0.0)), 2.0, &mut carry);
		// The viewport settles and releases while the residue is still
		// finer than a pixel: the page takes it before the release, so it
		// lands exactly where the viewport did.
		let events =
			fold(&mut feed, vec![PanStatus::Ready], None, 2.0, &mut carry);
		assert_eq!(moved(&events), vec![(0.2, 0.0)]);
		assert_eq!(events.last(), Some(&(TouchPhase::Ended, 0.0, 0.0)));
		assert_eq!(carry, (0.0, 0.0));
	}

	#[test]
	fn the_last_transform_of_a_closing_gesture_precedes_its_release() {
		let mut feed = PanFeed::default();
		let mut carry = (0.0, 0.0);
		let statuses = vec![PanStatus::Running];
		fold(&mut feed, statuses, Some((200.0, -60.0)), 2.0, &mut carry);
		// The lift and the final motion land in one pump.
		let statuses = vec![PanStatus::Ready];
		let events =
			fold(&mut feed, statuses, Some((240.0, -80.0)), 2.0, &mut carry);
		assert_eq!(moved(&events), vec![(20.0, -10.0)]);
		assert_eq!(
			events.last(),
			Some(&(TouchPhase::Ended, 0.0, 0.0)),
			"the release closes the gesture, after its last delta"
		);
	}

	#[test]
	fn the_inertia_tail_keeps_arriving_as_plain_deltas() {
		let mut feed = PanFeed::default();
		let mut carry = (0.0, 0.0);
		let statuses = vec![PanStatus::Running, PanStatus::Inertia];
		fold(&mut feed, statuses, Some((0.0, 0.0)), 1.0, &mut carry);
		let events =
			fold(&mut feed, vec![], Some((30.0, 0.0)), 1.0, &mut carry);
		assert_eq!(moved(&events), vec![(30.0, 0.0)]);
	}

	#[test]
	fn a_reset_transform_between_gestures_moves_nothing() {
		let mut feed = PanFeed::default();
		let mut carry = (0.0, 0.0);
		let statuses = vec![PanStatus::Running];
		fold(&mut feed, statuses, Some((300.0, 0.0)), 1.0, &mut carry);
		let statuses = vec![PanStatus::Ready];
		fold(&mut feed, statuses, Some((300.0, 0.0)), 1.0, &mut carry);
		// The viewport resets its transform to the origin while idle; the
		// next gesture's first transform still only names its origin.
		let statuses = vec![PanStatus::Building];
		let events =
			fold(&mut feed, statuses, Some((0.0, 0.0)), 1.0, &mut carry);
		assert_eq!(events, vec![(TouchPhase::Started, 0.0, 0.0)]);
		let events =
			fold(&mut feed, vec![], Some((25.0, 10.0)), 1.0, &mut carry);
		assert_eq!(moved(&events), vec![(25.0, 10.0)]);
	}

	#[test]
	fn a_held_stream_is_silenced_and_cannot_resume_after_the_owner_lets_go() {
		let mut feed = PanFeed::default();
		let mut carry = (0.0, 0.0);
		let statuses = vec![PanStatus::Running];
		fold(&mut feed, statuses, Some((0.0, 0.0)), 1.0, &mut carry);
		let events =
			fold(&mut feed, vec![], Some((30.0, 0.0)), 1.0, &mut carry);
		assert_eq!(moved(&events), vec![(30.0, 0.0)]);
		// A pointer drag takes the input: the gesture in flight ends once,
		// and the delta the batch carried drops with it.
		let events = hold(&mut feed);
		assert_eq!(events, vec![(TouchPhase::Cancelled, 0.0, 0.0)]);
		// The stale stream keeps arriving while the drag owns the input,
		// and pans nothing.
		let events = hold(&mut feed);
		assert!(events.is_empty());
		// The owner lets go; the stream is still stale and pans nothing.
		let events =
			fold(&mut feed, vec![], Some((50.0, 0.0)), 1.0, &mut carry);
		assert!(events.is_empty(), "a stale stream must not resume panning");
		// A fresh gesture after the drag pans again.
		let statuses = vec![PanStatus::Building];
		let events =
			fold(&mut feed, statuses, Some((0.0, 0.0)), 1.0, &mut carry);
		assert_eq!(events, vec![(TouchPhase::Started, 0.0, 0.0)]);
		let events =
			fold(&mut feed, vec![], Some((12.0, 0.0)), 1.0, &mut carry);
		assert_eq!(moved(&events), vec![(12.0, 0.0)]);
	}

	#[test]
	fn a_disabled_viewport_cancels_what_it_was_panning() {
		let mut feed = PanFeed::default();
		let mut carry = (0.0, 0.0);
		let statuses = vec![PanStatus::Running];
		fold(&mut feed, statuses, Some((0.0, 0.0)), 1.0, &mut carry);
		let statuses = vec![PanStatus::Disabled];
		let events =
			fold(&mut feed, statuses, Some((10.0, 0.0)), 1.0, &mut carry);
		// The teardown still delivers the contact's last motion, then cancels.
		assert_eq!(
			events,
			vec![
				(TouchPhase::Moved, 10.0, 0.0),
				(TouchPhase::Cancelled, 0.0, 0.0)
			]
		);
	}
}
