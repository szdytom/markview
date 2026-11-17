use super::*;
use proptest::prelude::*;
use std::assert_matches;

fn bits(c: CompactCluster) -> [u32; 7] {
	[
		c.range.start,
		c.range.end,
		c.command,
		c.row,
		c.x.to_bits(),
		c.width.to_bits(),
		c.flags_spacing,
	]
}
fn sample() -> CompactCluster {
	CompactCluster {
		range: (u32::MAX - 65535..u32::MAX).into(),
		command: u32::MAX,
		row: 16383,
		x: -0.,
		width: f32::from_bits(0x7fc0_0001),
		flags_spacing: 3,
	}
}

#[test]
fn index_boundaries_keep_exact_ranges_and_use_wide_records_when_needed() {
	let c = sample();
	let records = Records::new(vec![c; 100], 1);
	assert_matches!(records, Records::Narrow { .. });
	assert_eq!(records.storage_bytes(), 2000);
	assert_eq!(bits(records.get(99).unwrap()), bits(c));
	assert!(records.get(100).is_none());
	for (c, spacing) in [
		(
			CompactCluster {
				range: (0..65536).into(),
				..c
			},
			1,
		),
		(CompactCluster { row: 16384, ..c }, 1),
		(
			CompactCluster {
				row: 0,
				flags_spacing: 65536,
				..c
			},
			16385,
		),
	] {
		let records = Records::new(vec![c; 100], spacing);
		assert_matches!(records, Records::Wide(_));
		assert_eq!(records.storage_bytes(), 2800);
		assert_eq!(bits(records.get(99).unwrap()), bits(c));
	}
	let c = CompactCluster {
		row: 0,
		flags_spacing: 65535,
		..c
	};
	let records = Records::new(vec![c; 100], 16384);
	assert_matches!(records, Records::Narrow { .. });
	assert_eq!(bits(records.get(0).unwrap()), bits(c));
}

#[test]
fn spacing_counts_at_bit_boundaries_preserve_records_and_storage_size() {
	let cluster = CompactCluster {
		row: 0,
		flags_spacing: 0,
		..sample()
	};
	for spacing in [0, 1, 2, 3, 4, 5, 16384, 16385, usize::MAX] {
		let records = Records::new(vec![cluster; 100], spacing);
		assert_eq!(
			records.storage_bytes(),
			if spacing <= 16384 { 2000 } else { 2800 }
		);
		assert_eq!(bits(records.get(99).unwrap()), bits(cluster));
	}
}

proptest! {
	#![proptest_config(ProptestConfig::with_cases(128))]
	#[test]
	fn source_ranges_commands_rows_spacing_and_float_bits_round_trip(
		spacing_count in 0usize..257,
		input in prop::collection::vec((any::<u32>(), any::<u16>(), any::<u32>(), 0u32..1024, any::<u32>(), any::<u32>(), any::<u32>()), 0..256),
	) {
		let input: Vec<_> = input.into_iter().map(|(start, len, command, row, x, width, flags)| {
			let start = start.min(u32::MAX - u32::from(len));
			CompactCluster { range: (start..start + u32::from(len)).into(), command, row, x: f32::from_bits(x), width: f32::from_bits(width), flags_spacing: flags % (spacing_count.max(1) as u32 * 4) }
		}).collect();
		let records = Records::new(input.clone(), spacing_count);
		prop_assert_eq!(records.len(), input.len());
		for (i, expected) in input.into_iter().enumerate() {
			prop_assert_eq!(bits(records.get(i).unwrap()), bits(expected));
		}
	}
}
