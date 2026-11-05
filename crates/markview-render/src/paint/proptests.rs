use super::*;
use proptest::prelude::*;

proptest! {
	#[test]
	fn image_and_rule_alignment_follow_the_device_grid_with_minimum_pixel_sizes(
		x in -256i16..257, y in -256i16..257, width in 1u16..1025, height in 1u16..1025, scale in 0usize..8,
	) {
		let scale = [0.75, 1.0, 1.25, 1.5, 2.0, 2.5, 3.0, 4.0][scale];
		let original = Rect { x: f32::from(x) / 8.0, y: f32::from(y) / 8.0, w: f32::from(width) / 8.0, h: f32::from(height) / 8.0 };
		let image = device_aligned(original, scale);
		let rule = device_aligned_rule(original, scale);
		// Inputs stay below 1024 device pixels; the tolerance is 1/1000 pixel.
		let integer = |value: f32| (value - value.floor()).abs().min((value.ceil() - value).abs()) <= 0.001;
		for aligned in [image, rule] {
			for coordinate in [aligned.x, aligned.y, aligned.w, aligned.h] { prop_assert!(integer(coordinate * scale)); }
			prop_assert!((aligned.x - original.x).abs() * scale <= 0.501);
			prop_assert!((aligned.y - original.y).abs() * scale <= 0.501);
			prop_assert!(aligned.w * scale >= 0.999 && aligned.h * scale >= 0.999);
		}
		for (original, aligned) in [(original.w, image.w), (original.h, image.h)] {
			if original * scale >= 0.5 { prop_assert!((aligned - original).abs() * scale <= 0.501); }
			else { prop_assert!((aligned * scale - 1.0).abs() <= 0.001); }
		}
		prop_assert!(rule.h * scale + 0.001 >= original.h * scale);
		prop_assert!((rule.h - original.h) * scale < 1.001);
		let right = (rule.x + rule.w) * scale;
		let target = (original.x + original.w) * scale;
		prop_assert!(integer(right));
		prop_assert!((right - target).abs() <= 0.501 || (rule.w * scale - 1.0).abs() <= 0.001);
		for coordinate in [original.x, original.y] {
			let snapped = device_snap(coordinate, scale);
			prop_assert!(integer(snapped * scale));
			prop_assert!((snapped - coordinate).abs() * scale <= 0.501);
		}
		for (raw, expected) in [(-1.5, -2.0), (-0.5, -1.0), (0.5, 1.0), (1.5, 2.0)] {
			prop_assert!((device_snap(raw / scale, scale) * scale - expected).abs() <= 0.001);
		}
	}
}
