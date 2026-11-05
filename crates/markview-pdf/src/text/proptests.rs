use super::*;
use proptest::prelude::*;

proptest! {
	#[test]
	fn variable_axis_mapping_is_bounded_monotone_and_matches_endpoint_interpolation(
		minimum in -1000i16..1001, below in 0u16..513, above in 0u16..513, coordinates in prop::collection::vec(any::<i16>(), 0..20),
	) {
		let min = f32::from(minimum);
		let default = min + f32::from(below);
		let max = default + f32::from(above);
		let mut coordinates: Vec<_> = coordinates.into_iter().chain([i16::MIN, -16384, 0, 16384, i16::MAX]).collect();
		coordinates.sort_unstable();
		let mut previous = min;
		for coordinate in coordinates {
			let t = f64::from(coordinate) / 16384.0;
			let weight = t.abs().min(1.0);
			let endpoint = if t < 0.0 { min } else { max };
			let expected = (1.0 - weight) * f64::from(default) + weight * f64::from(endpoint);
			let actual = design_value(min, default, max, coordinate as f32 / 16384.0);
			prop_assert!(actual >= previous && actual >= min && actual <= max);
			prop_assert!((f64::from(actual) - expected).abs() <= 0.001);
			previous = actual;
		}
	}
}
