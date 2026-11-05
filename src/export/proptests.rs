use super::*;
use proptest::prelude::*;

proptest! {
	#[test]
	fn png_strips_cover_the_exact_device_pixel_partition_and_reject_excess_width(
		width in 1u16..513, content in 0u16..2049, top in 0u8..65, bottom in 0u8..65,
		scale in 1u8..33, limit in 0u16..1025,
	) {
		let scale = f32::from(scale) / 4.0;
		let geometry = PageGeometry { width_pt: f32::from(width) * 0.75, height_pt: 600.0, margin_pt: [f32::from(top) * 0.75, 0.0, f32::from(bottom) * 0.75, 0.0] };
		let expected_width = (f32::from(width) * scale).round().max(1.0) as u32;
		let expected_height = (f32::from(u16::from(top) + content + u16::from(bottom)) * scale).round().max(1.0) as u32;
		let result = plan(&geometry, f32::from(content), scale, u32::from(limit));
		let limit = u32::from(limit).max(1);
		if expected_width > limit { prop_assert!(result.is_err()); }
		else {
			let plan = result.unwrap();
			prop_assert_eq!((plan.width_px, plan.height_px), (expected_width, expected_height));
			let expected: Vec<_> = (0..expected_height).step_by(limit as usize).map(|start| (start, (expected_height - start).min(limit))).collect();
			prop_assert_eq!(plan.tiles.iter().map(|tile| (tile.y_px, tile.height_px)).collect::<Vec<_>>(), expected);
			for tile in plan.tiles { prop_assert!(((tile.scroll + f32::from(top)) * scale - tile.y_px as f32).abs() <= 0.002); }
		}
		let excessive = PageGeometry { width_pt: 6000.0, ..geometry };
		prop_assert!(plan(&excessive, 10000.0, 1.0, 8000).is_err(), "the 64-million-pixel limit must apply even when the width fits");
	}

	#[test]
	fn export_page_overrides_preserve_unselected_slots_and_convert_paper_and_margins(
		width in 100u16..601, height in 100u16..601, landscape in any::<bool>(),
		margins in prop::array::uniform4(0u8..11), slots in prop::array::uniform6(prop::option::of("[a-z]{0,10}")),
		value in any::<f32>(),
	) {
		let mut sheet = Stylesheet::default();
		sheet.page.header_left = Some("previous".to_owned());
		sheet.page.footer_right = Some("kept".to_owned());
		let sheet = Arc::new(sheet);
		let before = sheet.page().clone();
		let page = PageOverrides { paper: Some(format!("{width}x{height}")), landscape: Some(landscape), margin: Some(margins.map(f32::from)), header: std::array::from_fn(|index| slots[index].clone()), footer: std::array::from_fn(|index| slots[index + 3].clone()) };
		let styled = config::styled(sheet.clone(), &page).unwrap();
		prop_assert_eq!(sheet.page(), &before);
		let geometry = PageGeometry::from_style(styled.page()).unwrap();
		let (w, h) = if landscape { (height, width) } else { (width, height) };
		for (actual, mm) in [geometry.width_pt, geometry.height_pt].into_iter().zip([w, h]) { prop_assert!((f64::from(actual) - f64::from(mm) * 72.0 / 25.4).abs() < 0.001); }
		for (actual, mm) in geometry.margin_pt.into_iter().zip(margins) { prop_assert!((f64::from(actual) - f64::from(mm) * 72.0 / 25.4).abs() < 0.001); }
		let actual = [&styled.page().header_left, &styled.page().header_center, &styled.page().header_right, &styled.page().footer_left, &styled.page().footer_center, &styled.page().footer_right];
		let previous = [&before.header_left, &before.header_center, &before.header_right, &before.footer_left, &before.footer_center, &before.footer_right];
		for index in 0..6 { prop_assert_eq!(actual[index], &slots[index].clone().or_else(|| previous[index].clone())); }
		for (check, low, high) in [(config::validate_font_size as fn(f32) -> Result<()>, 1.0, 200.0), (config::validate_indent, 0.0, 4.0), (config::validate_scale, 0.1, 8.0)] {
			for value in [value, low, high, low - 1.0, high + 1.0, f32::NAN, f32::INFINITY] { prop_assert_eq!(check(value).is_ok(), value.is_finite() && value >= low && value <= high); }
		}
	}
}
