use super::*;
use proptest::prelude::*;

proptest! {
	#[test]
	fn row_transitions_preserve_shared_slots_and_settle_to_the_target(
		initial in prop::collection::btree_set(0usize..32, 0..16),
		changes in prop::collection::vec((prop::collection::btree_set(0usize..32, 0..16), 0u64..181), 0..16),
	) {
		let mut current: Vec<_> = initial.into_iter().collect();
		let mut slide = RowSlide::default();
		slide.reset(&current);
		let mut now = Instant::now();
		for (target, elapsed) in changes {
			now += Duration::from_millis(elapsed);
			let target: Vec<_> = target.into_iter().collect();
			let before = slide.animated(&current, now).unwrap_or_else(|| Row::sequential(&current));
			slide.retarget(&current, &target, now);
			let rows = slide.animated(&target, now).unwrap_or_else(|| Row::sequential(&target));
			for previous in &before {
				if target.contains(&previous.index) {
					let next = rows.iter().find(|row| row.index == previous.index).unwrap();
					prop_assert!((next.slot - previous.slot).abs() <= 0.0001);
				}
			}
			for at in [now, now + DURATION / 2, now + DURATION] {
				let rows = slide.animated(&target, at).unwrap_or_else(|| Row::sequential(&target));
				let identities: BTreeSet<_> = rows.iter().map(|row| row.index).collect();
				prop_assert_eq!(identities.len(), rows.len());
				for row in &rows {
					prop_assert!(row.slot.is_finite() && row.slot >= 0.0);
					prop_assert!((0.0..=1.0).contains(&row.band));
				}
				prop_assert!(rows.windows(2).all(|pair| pair[0].slot + pair[0].band <= pair[1].slot + 0.0001));
				if at == now + DURATION { prop_assert_eq!(rows, Row::sequential(&target)); }
			}
			current = target;
		}
		slide.advance(now + DURATION);
		prop_assert!(!slide.animating());
		prop_assert!(slide.animated(&current, now + DURATION).is_none());
		slide.reset(&current);
		prop_assert!(!slide.animating());
	}

	#[test]
	#[ignore = "Known bug PBT-007: rapid row retargeting can clip a retained row's partial band immediately"]
	fn rapid_retargeting_preserves_a_retained_rows_visible_band(elapsed in 19u64..180) {
		let start = Instant::now();
		let mut slide = RowSlide::default();
		slide.reset(&[]);
		slide.retarget(&[], &[2], start);
		slide.retarget(&[2], &[0, 1], start);
		let now = start + Duration::from_millis(elapsed);
		let before = slide.animated(&[0, 1], now).unwrap();
		let before = *before.iter().find(|row| row.index == 1).unwrap();
		slide.retarget(&[0, 1], &[1, 2, 3], now);
		let after = slide.animated(&[1, 2, 3], now).unwrap();
		let after = *after.iter().find(|row| row.index == 1).unwrap();
		prop_assert_eq!(after.slot, before.slot);
		prop_assert!((after.band - before.band).abs() <= 0.0001, "band changed from {} to {}", before.band, after.band);
	}
}
