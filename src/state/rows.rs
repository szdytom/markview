//! Row insertion and collapse motion shared by the folder and outline lists.
use super::{Easing, Tween};
use std::{
	collections::{BTreeSet, HashMap},
	time::{Duration, Instant},
};

const DURATION: Duration = Duration::from_millis(180);

/// One row the drawer paints while a tree expands or collapses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Row {
	pub index: usize,
	/// The row's vertical slot, fractional while it moves.
	pub slot: f32,
	/// The share of the row's height still visible in `0..=1`.
	pub band: f32,
}

impl Row {
	pub(crate) fn sequential(rows: &[usize]) -> Vec<Self> {
		rows.iter()
			.enumerate()
			.map(|(row, index)| Self {
				index: *index,
				slot: row as f32,
				band: 1.0,
			})
			.collect()
	}
}

#[derive(Clone, Copy)]
struct Shift {
	index: usize,
	from: f32,
	to: f32,
	from_end: f32,
	to_end: f32,
}

/// Remembers the rows shown before a tree change and slides them into the new
/// layout. A row that leaves shrinks and drifts toward its parent; a row that
/// arrives grows out of it, so every toggle reads as one accordion.
#[derive(Default)]
pub(crate) struct RowSlide {
	target: Vec<usize>,
	shifts: Vec<Shift>,
	tween: Option<Tween>,
}

impl RowSlide {
	/// Starts a transition from `from` to `to`, continuing any running slide.
	pub(crate) fn retarget(
		&mut self,
		from: &[usize],
		to: &[usize],
		now: Instant,
	) {
		if self.target == to {
			return;
		}
		let current = self.current(from, now);
		let target: Vec<(usize, f32)> = to
			.iter()
			.copied()
			.enumerate()
			.map(|(row, index)| (index, row as f32))
			.collect();
		self.shifts = merge(&current, &target);
		self.target = to.to_vec();
		self.tween = Some(Tween::new(0.0, 1.0, now, DURATION, Easing::ExpoOut));
	}

	/// Drops the motion and shows `rows` where they belong.
	pub(crate) fn reset(&mut self, rows: &[usize]) {
		self.target = rows.to_vec();
		self.shifts.clear();
		self.tween = None;
	}

	pub(crate) fn animating(&self) -> bool {
		self.tween.is_some()
	}

	pub(crate) fn advance(&mut self, now: Instant) {
		if self
			.tween
			.as_ref()
			.is_some_and(|tween| tween.is_finished(now))
		{
			self.tween = None;
		}
	}

	/// The rows in motion, or `None` while the target is not `current`.
	pub(crate) fn animated(
		&self,
		current: &[usize],
		now: Instant,
	) -> Option<Vec<Row>> {
		let tween = self.tween.as_ref().filter(|_| self.target == current)?;
		let mut rows: Vec<Row> = self
			.shifts
			.iter()
			.filter(|shift| {
				!tween.is_finished(now)
					|| self.target.binary_search(&shift.index).is_ok()
			})
			.map(|shift| {
				let t = tween.progress(now);
				Row {
					index: shift.index,
					slot: shift.from + (shift.to - shift.from) * t,
					band: shift.from_end + (shift.to_end - shift.from_end) * t,
				}
			})
			.collect();
		rows.sort_by(|a, b| {
			a.slot
				.total_cmp(&b.slot)
				.then_with(|| a.index.cmp(&b.index))
		});
		for i in 0..rows.len() {
			let end = rows[i].band;
			let limit = rows.get(i + 1).map_or(end, |next| next.slot.min(end));
			rows[i].band = (limit - rows[i].slot).clamp(0.0, 1.0);
		}
		Some(rows)
	}

	/// Current slots and ends of the rows a running slide paints, or `from`
	/// at rest. The end keeps a partial band continuous across a retarget.
	fn current(&self, from: &[usize], now: Instant) -> Vec<(usize, f32, f32)> {
		self.animated(from, now).map_or_else(
			|| {
				from.iter()
					.copied()
					.enumerate()
					.map(|(row, index)| (index, row as f32, row as f32 + 1.0))
					.collect()
			},
			|rows| {
				rows.into_iter()
					.map(|row| (row.index, row.slot, row.slot + row.band))
					.collect()
			},
		)
	}

	#[cfg(test)]
	pub(crate) fn freeze(&mut self, elapsed: Duration) {
		self.tween
			.as_mut()
			.expect("the row slide is animating")
			.freeze(elapsed);
	}
}

fn merge(current: &[(usize, f32, f32)], target: &[(usize, f32)]) -> Vec<Shift> {
	let current_slots: HashMap<usize, (f32, f32)> = current
		.iter()
		.map(|(index, slot, end)| (*index, (*slot, *end)))
		.collect();
	let target_slots: HashMap<usize, f32> = target.iter().copied().collect();
	let mut shifts = Vec::new();
	let mut from = None;
	for (index, to) in target {
		let (start, start_end) = match current_slots.get(index) {
			Some((slot, end)) => {
				from = Some(*slot);
				(*slot, *end)
			}
			None => {
				let start = from.map_or(0.0, |slot: f32| slot + 1.0);
				(start, start)
			}
		};
		shifts.push(Shift {
			index: *index,
			from: start,
			to: *to,
			from_end: start_end,
			to_end: *to + 1.0,
		});
	}
	let mut to = None;
	for (index, slot, end) in current {
		if target_slots.contains_key(index) {
			to = target_slots.get(index).copied();
			continue;
		}
		let target = to.map_or(0.0, |slot| slot + 1.0);
		shifts.push(Shift {
			index: *index,
			from: *slot,
			to: target,
			from_end: *end,
			to_end: target,
		});
	}
	let mut seen = BTreeSet::new();
	shifts.retain(|shift| seen.insert(shift.index));
	shifts
}

#[cfg(test)]
mod tests {
	use super::*;

	fn rows(slide: &RowSlide, current: &[usize], now: Instant) -> Vec<Row> {
		slide.animated(current, now).unwrap()
	}

	#[test]
	fn expanding_rows_grow_out_of_their_parent_and_keep_the_order() {
		let start = Instant::now();
		let mut slide = RowSlide::default();
		slide.reset(&[0, 2, 3]);
		assert!(!slide.animating());
		slide.retarget(&[0, 2, 3], &[0, 1, 2, 3], start);
		let halfway = rows(&slide, &[0, 1, 2, 3], start + DURATION / 2);
		assert_eq!(
			halfway.iter().map(|row| row.index).collect::<Vec<_>>(),
			[0, 1, 2, 3]
		);
		assert_eq!(halfway[1].slot, 1.0);
		assert!(halfway[2].slot > 1.0 && halfway[2].slot < 2.0);
		assert!(halfway[1].band > 0.0 && halfway[1].band < 1.0);
		let settled = rows(&slide, &[0, 1, 2, 3], start + DURATION);
		assert_eq!(
			settled.iter().map(|row| row.slot).collect::<Vec<_>>(),
			[0.0, 1.0, 2.0, 3.0]
		);
		assert!(settled.iter().all(|row| row.band == 1.0));
	}

	#[test]
	fn reversing_a_slide_keeps_a_partial_band_continuous() {
		let start = Instant::now();
		let mut slide = RowSlide::default();
		slide.reset(&[0, 1]);
		slide.retarget(&[0, 1], &[0], start);
		let mid = start + DURATION / 4;
		let band = |rows: Vec<Row>| {
			rows.into_iter().find(|row| row.index == 1).unwrap().band
		};
		let before = band(slide.animated(&[0], mid).unwrap());
		slide.retarget(&[0], &[0, 1], mid);
		let after = band(slide.animated(&[0, 1], mid).unwrap());
		assert!((before - after).abs() < 1e-6, "{before} -> {after}");
	}

	#[test]
	fn a_terminal_child_grows_and_shrinks_its_band() {
		let start = Instant::now();
		let mut slide = RowSlide::default();
		slide.reset(&[0]);
		slide.retarget(&[0], &[0, 1], start);
		let arriving = rows(&slide, &[0, 1], start + DURATION / 4)[1];
		assert_eq!(arriving.slot, 1.0);
		assert!(arriving.band > 0.0 && arriving.band < 1.0);
		let mut slide = RowSlide::default();
		slide.reset(&[0, 1]);
		slide.retarget(&[0, 1], &[0], start);
		let leaving = rows(&slide, &[0], start + DURATION / 4)[1];
		assert!(leaving.band > 0.0 && leaving.band < 1.0);
	}

	#[test]
	fn collapsing_rows_shrink_toward_their_parent() {
		let start = Instant::now();
		let mut slide = RowSlide::default();
		slide.retarget(&[0, 1, 2, 3], &[0, 2, 3], start);
		let halfway = rows(&slide, &[0, 2, 3], start + DURATION / 2);
		assert!(halfway.iter().any(|row| row.index == 1));
		let leaving = halfway.iter().find(|row| row.index == 1).unwrap();
		assert!(leaving.slot < 1.5);
		let settled = rows(&slide, &[0, 2, 3], start + DURATION);
		assert!(settled.iter().all(|row| row.index != 1));
	}
}
