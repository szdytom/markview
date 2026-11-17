use super::*;
use proptest::prelude::*;

fn bits(g: GlyphPosition) -> (u16, u32, u32) {
	(g.id, g.x.to_bits(), g.y.to_bits())
}
fn pack(glyphs: &[GlyphPosition]) -> Positions {
	let mut builder = Builder::default();
	for g in glyphs {
		builder.push(*g);
	}
	builder.finish()
}

#[test]
fn repeated_baselines_share_exact_bits_including_signed_zero_and_nan() {
	let rows = [0, (-0f32).to_bits(), 0x7fc0_0001, 0x7fc0_0002];
	let input: Vec<_> = (0..1000)
		.map(|i| GlyphPosition {
			id: i as u16,
			x: f32::from_bits(i),
			y: f32::from_bits(rows[i as usize % rows.len()]),
		})
		.collect();
	let packed = pack(&input);
	let Positions::Shared { y, .. } = &packed else {
		panic!("repeated coordinates should share storage")
	};
	assert_eq!(y.len(), rows.len());
	assert!(packed.storage_bytes() < size_of_val(&*input) * 7 / 10);
	for (i, g) in input.into_iter().enumerate() {
		assert_eq!(bits(packed.get(i)), bits(g));
	}
}

#[test]
fn distinct_coordinates_and_small_buffers_keep_native_positions() {
	for count in [0, 1, 4, 100, 65_537] {
		let input: Vec<_> = (0..count)
			.map(|i| GlyphPosition {
				id: i as u16,
				x: -(i as f32),
				y: f32::from_bits(i),
			})
			.collect();
		let packed = pack(&input);
		assert!(matches!(packed, Positions::Native(_)));
		assert_eq!(packed.len(), input.len());
		for (i, g) in input.into_iter().enumerate() {
			assert_eq!(bits(packed.get(i)), bits(g));
		}
	}
}

proptest! {
	#![proptest_config(ProptestConfig::with_cases(128))]
	#[test]
	fn arbitrary_float_payloads_and_glyph_ids_round_trip(
		rows in prop::collection::vec(any::<u32>(), 1..16),
		input in prop::collection::vec((any::<u16>(), any::<u32>(), any::<usize>()), 0..512),
	) {
		let input: Vec<_> = input.into_iter().map(|(id, x, row)| GlyphPosition {
			id, x: f32::from_bits(x), y: f32::from_bits(rows[row % rows.len()]),
		}).collect();
		let packed = pack(&input);
		prop_assert_eq!(packed.len(), input.len());
		for (i, g) in input.into_iter().enumerate() {
			prop_assert_eq!(bits(packed.get(i)), bits(g));
		}
	}
}
