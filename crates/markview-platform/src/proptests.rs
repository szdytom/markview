use super::*;
use proptest::prelude::*;

proptest! {
	#[test]
	fn traffic_light_margins_preserve_space_and_snap_within_the_optical_balance_band(
		height in 1u16..129, gap in 0u16..257, shift in 0u16..257, scale in 0usize..8,
	) {
		let height = f64::from(height) / 4.0;
		let space = f64::from(gap) / 4.0;
		let shift = f64::from(shift) / 4.0;
		let scale = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0][scale];
		let (top, bottom) = traffic_light_margins(height + space, height, scale);
		let low = space / 2.1;
		let high = space / 2.0;
		prop_assert!((top + bottom - space).abs() <= 1e-12);
		prop_assert!(top >= low - 1e-12 && top <= high + 1e-12);
		prop_assert!(bottom + 1e-12 >= top);
		let ideal = space * 19.0 / 39.0;
		prop_assert!((top - ideal).abs() * scale <= 0.5 + 1e-12);
		let device = top * scale;
		let on_grid = (device - device.floor()).min(device.ceil() - device).abs() <= 1e-12;
		prop_assert!(on_grid || (top - low).abs() <= 1e-12 || (top - high).abs() <= 1e-12);
		prop_assert_eq!(traffic_light_margins(height + space + shift, height + shift, scale), (top, bottom));
	}
}
