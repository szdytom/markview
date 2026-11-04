use super::*;
use proptest::prelude::*;

fn invalidation(n: u8) -> Invalidation {
	[
		Invalidation::None,
		Invalidation::Paint,
		Invalidation::Layout,
	][n as usize]
}
fn update(n: u8, effects: Vec<i16>) -> Update<i16> {
	Update {
		invalidation: invalidation(n),
		effects,
	}
}
#[derive(Default)]
struct Recorder(Vec<i16>);
impl Component for Recorder {
	type Message = (u8, Vec<i16>);
	type Effect = i16;
	type View<'a> = &'a [i16];
	fn update(&mut self, (n, effects): Self::Message) -> Update<i16> {
		self.0.extend(&effects);
		update(n, effects)
	}
	fn view(&self) -> &[i16] {
		&self.0
	}
}
proptest! {
	#[test]
	fn merge_and_map_preserve_order(parts in prop::collection::vec((0u8..3, prop::collection::vec(any::<i16>(), 0..8)), 0..16)) {
		let expected: Vec<_> =
			parts.iter().flat_map(|(_, e)| e.iter().copied()).collect();
		let level = parts
			.iter()
			.map(|(n, _)| invalidation(*n))
			.max()
			.unwrap_or_default();
		let mut all = Update::new(Invalidation::None);
		for (n, effects) in parts {
			all.merge(update(n, effects));
		}
		all.merge(Update::new(Invalidation::None));
		prop_assert_eq!(all.invalidation, level);
		prop_assert_eq!(&all.effects, &expected);
		let mapped = all.map(i32::from);
		prop_assert_eq!(mapped.invalidation, level);
		prop_assert_eq!(
			mapped.effects,
			expected.into_iter().map(i32::from).collect::<Vec<_>>()
		);
	}
	#[test]
	fn scheduling_matches_fifo(ops in prop::collection::vec((any::<bool>(), 0u8..3, any::<i16>()), 0..64)) {
		let mut runtime = Runtime::new(Recorder::default());
		let mut pending = Vec::new();
		let mut processed = Vec::new();
		let mut level = Invalidation::None;
		for (flush, n, value) in ops.into_iter().chain([(true, 0, 0)]) {
			if flush {
				let result = runtime.flush();
				prop_assert_eq!(result.invalidation, level);
				prop_assert_eq!(&result.effects, &pending);
				processed.append(&mut pending);
				level = Invalidation::None;
				prop_assert_eq!(runtime.view(), processed.as_slice());
			} else {
				runtime.send((n, vec![value]));
				pending.push(value);
				level = level.max(invalidation(n));
			}
		}
	}
}

proptest! {
	#[test]
	fn regrouping_updates_and_flush_batches_is_associative(parts in prop::collection::vec((0u8..3, prop::collection::vec(any::<i16>(),0..5),any::<bool>()),0..24)) {
		let mut one = Runtime::new(Recorder::default());
		let mut batches = Runtime::new(Recorder::default());
		let mut merged = Update::new(Invalidation::None);
		for (n, e, flush) in parts {
			one.send((n, e.clone()));
			batches.send((n, e));
			if flush {
				merged.merge(batches.flush());
			}
		}
		merged.merge(batches.flush());
		let once = one.flush();
		prop_assert_eq!(merged.effects, once.effects);
		prop_assert_eq!(merged.invalidation, once.invalidation);
		prop_assert_eq!(one.view(), batches.view());
	}
}
