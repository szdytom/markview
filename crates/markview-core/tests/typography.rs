use markview_core::{
	document,
	fonts::FontConfig,
	layout::{LayoutEngine, LayoutOptions},
	scene::Draw,
	style::{
		Condition, Media, MediaContext, StyleTarget, Stylesheet,
		StylesheetSource, TextAppearance,
	},
};
use std::{path::Path, sync::Arc};

fn options(sheet: Stylesheet) -> LayoutOptions {
	LayoutOptions {
		width: 320.0,
		stylesheet: Arc::new(sheet),
		fonts: FontConfig {
			ignore_system_fonts: true,
			directories: vec![
				Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts"),
			],
			..Default::default()
		},
		..Default::default()
	}
}

#[test]
fn nested_list_tails_leave_one_gap_before_the_next_block() {
	let opts = options((*Stylesheet::bundled(false)).clone());
	for marker in ["-", "1."] {
		let mut flat_gap = None;
		for depth in 0..5 {
			let mut source = String::new();
			for level in 0..depth {
				source += &format!("{}{marker} parent\n", "    ".repeat(level));
			}
			source +=
				&format!("{}{marker} leaf\n\nAfter.\n", "    ".repeat(depth));
			let layout = LayoutEngine::new()
				.layout(&document::parse(source), &opts)
				.flattened();
			let leaf = layout.blocks[0]
				.layout
				.text
				.iter()
				.find(|node| node.text == "leaf")
				.unwrap();
			let bottom = leaf
				.clusters
				.iter()
				.map(|c| c.rect.y + c.rect.h)
				.reduce(f32::max)
				.unwrap();
			let gap = layout.blocks[1].y - bottom;
			let expected = *flat_gap.get_or_insert(gap);
			assert!(
				(gap - expected).abs() < 0.1,
				"{marker}, depth {depth}: {gap} versus {expected}"
			);
		}
	}
}

#[test]
fn phone_list_columns_reflow_cached_layout_and_fit_wide_numbers() {
	let source = document::parse(
		"- parent\n    - child\n        - grandchild\n            - leaf\n\n123456789. numbered\n\n- [x] checked\n",
	);
	let sheet = (*Stylesheet::bundled(false)).clone();
	let mut engine = LayoutEngine::new();
	let desktop = engine.layout(&source, &options(sheet.clone())).flattened();
	let phone = sheet.for_media(MediaContext::new(
		StyleTarget::Ui,
		Media::Phone,
		Some(Media::Android),
	));
	let narrow = engine.layout(&source, &options(phone)).flattened();
	assert_eq!(narrow.reused, 0);
	let leaf_x = |layout: &markview_core::layout::LayoutSnapshot| {
		layout.blocks[0]
			.layout
			.text
			.iter()
			.find(|node| node.text == "leaf")
			.unwrap()
			.clusters[0]
			.rect
			.x
	};
	assert!(leaf_x(&desktop) - leaf_x(&narrow) > 60.0);
	let numbered = &narrow.blocks[1].layout.text;
	let number_end = numbered[0]
		.clusters
		.iter()
		.map(|c| c.rect.x + c.rect.w)
		.reduce(f32::max)
		.unwrap();
	assert!(numbered[1].clusters[0].rect.x > number_end);
	assert!(narrow.blocks[2].layout.text[0].clusters[0].rect.x >= 18.0);
}

#[test]
fn narrow_marker_columns_keep_aligned_markers_before_item_text() {
	for (source, marker_size) in
		[("- item", 0.1), ("- [x] item", 0.1), ("1. item", 0.2)]
	{
		for align in ["left", "center", "right"] {
			let custom = Stylesheet::parse(&format!(
				"format_version=2\nversion=1\n\
			[[rule]]\nwhen=['list']\nmarker_width=0\n\
			[[rule]]\nwhen=['enum']\nmarker_width=0\nalign='{align}'\n\
			[[rule]]\nwhen=['marker']\nsize={marker_size}\nalign='{align}'\n\
			[[rule]]\nwhen=['task_marker']\nalign='{align}'"
			))
			.unwrap();
			let mut sheet = (*Stylesheet::builtin()).clone();
			sheet.merge(&custom);
			let opts = options(sheet);
			let layout = LayoutEngine::new()
				.layout(&document::parse(source), &opts)
				.flattened();
			let block = &layout.blocks[0].layout;
			let text_start = block.text.last().unwrap().clusters[0].rect.x;
			let marker_right = if source.starts_with("1.") {
				block.text[0]
					.clusters
					.iter()
					.map(|c| c.rect.x + c.rect.w)
					.reduce(f32::max)
					.unwrap()
			} else {
				block
					.draws
					.iter()
					.filter_map(|draw| match draw {
						Draw::Polygon { center, points, .. } => points
							.iter()
							.map(|p| center[0] + p[0])
							.reduce(f32::max),
						_ => None,
					})
					.reduce(f32::max)
					.unwrap()
			};
			assert!(
				marker_right < text_start,
				"{align}, {source}: marker ends at {marker_right}, text starts at {text_start}"
			);
		}
	}
}

#[test]
fn marker_width_validates_scope_and_invalidates_layout() {
	for (role, value, valid) in [
		("list", "1.0", true),
		("enum", "0", true),
		("p", "1.0", false),
		("marker", "1.0", false),
		("list", "-1", false),
		("enum", "nan", false),
	] {
		let parsed = Stylesheet::parse(&format!(
			"format_version=2\nversion=1\n[[rule]]\nwhen=['{role}']\nmarker_width={value}"
		));
		assert_eq!(parsed.is_ok(), valid, "{role}: {value}");
		if let Ok(sheet) = parsed {
			let mut baseline = (*Stylesheet::builtin()).clone();
			let previous = baseline.layout_key();
			baseline.merge(&sheet);
			assert_ne!(baseline.layout_key(), previous);
		}
	}
}

#[test]
fn bundled_lower_headings_keep_their_theme_font_and_descending_scale() {
	for (id, target) in [
		("celadon", StyleTarget::Ui),
		("rosewood", StyleTarget::Ui),
		("qibaishi", StyleTarget::Pdf),
		("vangogh", StyleTarget::Pdf),
		("mondrian", StyleTarget::Pdf),
	] {
		let mut source = (*StylesheetSource::builtin()).clone();
		if target == StyleTarget::Pdf {
			source.merge(&StylesheetSource::named_rules("print").unwrap());
		}
		source.merge(&StylesheetSource::named_rules(id).unwrap());
		for device in [Media::Desktop, Media::Phone] {
			let sheet = Arc::new(source.clone())
				.resolve(MediaContext::new(target, device, None));
			let body = sheet.text(&TextAppearance::default(), Condition::Body);
			let third = sheet.text(&body, Condition::H3);
			let mut previous_size = third.size;
			for role in [Condition::H4, Condition::H5, Condition::H6] {
				let heading = sheet.text(&body, role);
				assert_eq!(heading.font, third.font, "{id}: {role:?}");
				assert!(heading.size < previous_size, "{id}: {role:?}");
				assert_eq!(heading.weight, third.weight, "{id}: {role:?}");
				previous_size = heading.size;
			}
		}
	}
}
