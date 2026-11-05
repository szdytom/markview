use super::*;
use proptest::prelude::*;

proptest! {
	#[test]
	fn color_unpremultiplication_chooses_the_nearest_straight_channel_and_keeps_alpha(
		pixels in prop::collection::vec(prop::array::uniform4(any::<u8>()), 0..12),
	) {
		let pixels: Vec<_> = [[0, 0, 0, 0], [255, 128, 1, 255], [1, 1, 1, 1], [2, 1, 0, 1]].into_iter().chain(pixels).collect();
		let mut bytes: Vec<_> = pixels.iter().flatten().copied().collect();
		unpremultiply(&mut bytes);
		for (input, output) in pixels.iter().zip(bytes.as_chunks::<4>().0) {
			let alpha = u32::from(input[3]);
			prop_assert_eq!(output[3], input[3]);
			for channel in 0..3 {
				let expected = if alpha == 0 { 0 } else {
					(0u32..256).min_by_key(|&candidate| ((candidate * alpha).abs_diff(u32::from(input[channel]) * 255), std::cmp::Reverse(candidate))).unwrap() as u8
				};
				prop_assert_eq!(output[channel], expected);
			}
		}
	}
}
