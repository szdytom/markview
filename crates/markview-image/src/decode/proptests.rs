use super::*;
use proptest::prelude::*;

proptest! {
	#[test]
	fn ico_directory_selects_the_first_largest_png_payload(
		entries in prop::collection::vec((any::<u8>(), any::<u8>(), any::<bool>()), 0..8),
	) {
		let mut bytes = vec![0, 0, 1, 0];
		bytes.extend_from_slice(&(entries.len() as u16).to_le_bytes());
		let payload_start = 6 + 16 * entries.len();
		let payload = |i, png| {
			let mut data = if png { b"\x89PNG\r\n\x1a\n".to_vec() } else { b"bitmap!!".to_vec() };
			data.push(i as u8);
			data
		};
		for (i, &(w, h, _)) in entries.iter().enumerate() {
			bytes.extend_from_slice(&[w, h, 0, 0, 1, 0, 32, 0]);
			bytes.extend_from_slice(&9u32.to_le_bytes());
			bytes.extend_from_slice(&((payload_start + i * 9) as u32).to_le_bytes());
		}
		for (i, &(_, _, png)) in entries.iter().enumerate() { bytes.extend(payload(i, png)); }
		let area = |w, h| { let side = |v| if v == 0 { 256u32 } else { u32::from(v) }; side(w) * side(h) };
		let best_area = entries.iter().filter(|entry| entry.2).map(|&(w, h, _)| area(w, h)).max();
		let expected = best_area.and_then(|max| entries.iter().position(|&(w, h, png)| png && area(w, h) == max)).map(|i| payload(i, true));
		prop_assert_eq!(ico_png(&bytes), expected.as_deref());
		// A truncated directory or an out-of-bounds payload abandons PNG extraction.
		if !entries.is_empty() {
			prop_assert!(ico_png(&bytes[..payload_start - 1]).is_none());
			bytes[18..22].copy_from_slice(&u32::MAX.to_le_bytes());
			prop_assert!(ico_png(&bytes).is_none());
		}
	}

	#[test]
	fn pixel_budget_and_initial_bitmap_size_follow_the_declared_policy(
		w in any::<u32>(), h in any::<u32>(),
		width_power in 0u32..13, height in 1u16..4097, half_column in 0u16..8193,
	) {
		prop_assert_eq!(dimensions(w, h).is_ok(), w > 0 && h > 0 && u64::from(w) * u64::from(h) <= 16_000_000);
		let width = 1u16 << width_power;
		let numerator = u64::from(half_column.min(width));
		let denominator = u64::from(width);
		let scaled = |value| ((u64::from(value) * numerator * 2 + denominator) / (denominator * 2)).max(1) as u32;
		prop_assert_eq!(initial_bitmap_target((u32::from(width), u32::from(height)), f32::from(half_column) / 2.0), (scaled(width), scaled(height)));
	}

	#[test]
	fn bitmap_target_clamps_each_axis_and_uniform_alpha_survives_resize(
		w in 2u32..9, h in 2u32..9, target_w in 0u32..12, target_h in 0u32..12,
		alpha in prop::sample::select(vec![0u8, 1, 127, 128, 254, 255]),
	) {
		let rgba = image::RgbaImage::from_pixel(w, h, image::Rgba([255, 0, 255, alpha]));
		let mut bytes = Cursor::new(Vec::new());
		rgba.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
		let decoded = decode(bytes.get_ref(), Some((target_w, target_h)), None, None, &[]).unwrap();
		let target = (target_w.clamp(1, w), target_h.clamp(1, h));
		prop_assert_eq!(decoded.intrinsic, (w, h));
		prop_assert!(!decoded.svg);
		prop_assert_eq!((decoded.pixels.width, decoded.pixels.height), target);
		let expected = if alpha == 0 && target != (w, h) { [0, 0, 0, 0] } else { [255, 0, 255, alpha] };
		prop_assert!(decoded.pixels.rgba.as_chunks::<4>().0.iter().all(|pixel| *pixel == expected));
	}
}

#[test]
fn svg_target_accepts_long_thin_images_but_rejects_zero_dimensions() {
	let svg = b"<svg xmlns='http://www.w3.org/2000/svg' width='2' height='1'><rect width='2' height='1'/></svg>";
	for target in [None, Some((3, 2)), Some((8193, 1))] {
		let decoded = decode(svg, target, None, None, &[]).unwrap();
		assert!(decoded.svg);
		assert_eq!(decoded.intrinsic, (2, 1));
		assert_eq!(
			(decoded.pixels.width, decoded.pixels.height),
			target.unwrap_or((2, 1))
		);
	}
	for target in [(0, 1), (1, 0), (0, 0), (4001, 4000)] {
		assert!(decode(svg, Some(target), None, None, &[]).is_err());
	}
}
