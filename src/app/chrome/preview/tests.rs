use super::*;
use crate::app::chrome::preview::Card;

fn place(anchor: Rect, size: (f32, f32), bounds: Rect) -> Rect {
	super::place(anchor, size, bounds, [PADDING; 4])
}

fn card(title: &str, body: bool) -> Card {
	Card {
		title: title_lines(&mut crate::test_support::shaper(), title),
		detail: "/home/reader/notes.md".into(),
		body,
	}
}

fn anchor(x: f32, w: f32) -> Rect {
	Rect {
		x,
		y: 4.0,
		w,
		h: 36.0,
	}
}

#[test]
fn the_card_hangs_below_its_anchor_and_stays_inside_the_window() {
	let card = card("notes.md", true);
	for (x, expected_x) in [
		(0.0, 8.0),
		(200.0, 200.0 + 50.0 - WIDTH / 2.0),
		(700.0, 800.0 - WIDTH - 8.0),
	] {
		let layout = super::layout(&card, anchor(x, 100.0), (800.0, 600.0));
		assert_eq!(layout.rect.y, 4.0 + 36.0 + TOP_GAP);
		assert_eq!(layout.rect.w, WIDTH);
		assert!((layout.rect.x - expected_x).abs() < 0.001, "{x}");
		assert!(layout.rect.x >= 8.0);
		assert!(layout.rect.x + layout.rect.w <= 800.0 - 8.0);
	}
	// A window too short for the whole card keeps its top on screen.
	let short = super::layout(&card, anchor(200.0, 100.0), (800.0, 120.0));
	assert!(short.rect.y >= 0.0);
}

#[test]
fn the_title_wraps_to_at_most_two_lines_and_ends_ellipsized() {
	let mut ui = crate::test_support::shaper();
	assert_eq!(super::title_lines(&mut ui, "notes.md"), ["notes.md"]);

	let long =
		"a-very-long-document-name-that-cannot-fit-on-a-single-tab-label.md"
			.repeat(2);
	let lines = super::title_lines(&mut ui, &long);
	assert_eq!(lines.len(), 2);
	assert!(lines[1].ends_with('…'), "{lines:?}");
	assert!(lines[0].len() + lines[1].len() < long.len());
	assert!(ui.text_width(&lines[1], TITLE_SIZE) <= CONTENT_WIDTH + 0.001);

	let chinese =
		"这是一个名字特别长的中文文档用来检查标题最多显示两行".repeat(2);
	let lines = super::title_lines(&mut ui, &chinese);
	assert_eq!(lines.len(), 2);
	for line in &lines {
		assert!(ui.text_width(line, TITLE_SIZE) <= CONTENT_WIDTH + 0.001);
	}
}

#[test]
fn a_card_without_a_body_reserves_no_preview_rectangle() {
	let with = super::layout(
		&card("notes.md", true),
		anchor(200.0, 100.0),
		(800.0, 600.0),
	);
	let without = super::layout(
		&card("notes.md", false),
		anchor(200.0, 100.0),
		(800.0, 600.0),
	);
	assert!(with.body.is_some());
	assert!(without.body.is_none());
	assert_eq!(
		with.rect.h - without.rect.h,
		CONTENT_WIDTH * BODY_RATIO + BODY_GAP
	);
}

#[test]
fn the_rule_between_the_path_and_page_keeps_the_content_padding() {
	let with_body = card("notes.md", true);
	let layout =
		super::layout(&with_body, anchor(200.0, 100.0), (800.0, 600.0));
	let separator = layout.separator.expect("a body has a rule");
	let body = layout.body.expect("a body is reserved");
	assert_eq!(separator.x, body.x);
	assert_eq!(separator.w, body.w);
	assert_eq!(separator.x - layout.rect.x, PADDING);
	assert!(separator.y > layout.detail.y + layout.detail.h);
	assert!(separator.y + separator.h < body.y);

	let without = super::layout(
		&card("notes.md", false),
		anchor(200.0, 100.0),
		(800.0, 600.0),
	);
	assert!(without.separator.is_none());
}

#[test]
fn preview_cards_use_the_theme_radius_without_a_body_frame() {
	let mut ui = crate::test_support::shaper();
	let card = card("notes.md", true);
	let layout = super::layout(&card, anchor(200.0, 100.0), (800.0, 600.0));
	for theme in markview_layout::style::Stylesheet::READER_THEMES {
		let mut stylesheet =
			(*markview_layout::style::Stylesheet::builtin()).clone();
		stylesheet.merge(
			&markview_layout::style::Stylesheet::named_rules(theme).unwrap(),
		);
		for radius in [0.0, 12.0] {
			if radius > 0.0 {
				stylesheet.merge(
					&markview_layout::style::Stylesheet::parse(
						"format_version=3\nversion=1\n[[rule]]\nwhen=['ui','tab_preview']\nradius=12",
					)
					.unwrap(),
				);
			}
			ui.set_stylesheet(std::sync::Arc::new(stylesheet.clone()));
			let draws = draw(&mut ui, &card, &layout);
			assert!(
				matches!(
					draws.first(),
					Some(Draw::Box {
						rect,
						condition: Condition::TabPreview,
						radius: actual,
						border: 1.0,
						..
					}) if *rect == layout.rect && *actual == radius
				),
				"{theme}: {radius}"
			);
			assert!(
				draws[1..].iter().all(|draw| matches!(
					draw,
					Draw::Glyph(_) | Draw::Rect(_, _)
				))
			);
			assert!(draws.len() > 1);
		}
	}
}

#[test]
fn floating_scenes_flip_above_and_use_side_space() {
	let bounds = Rect {
		x: 30.0,
		y: 48.0,
		w: 740.0,
		h: 500.0,
	};
	let bottom = Rect {
		x: 700.0,
		y: 510.0,
		w: 10.0,
		h: 18.0,
	};
	let rect = place(bottom, (300.0, 180.0), bounds);
	assert!(rect.y + rect.h < bottom.y);
	let middle = Rect {
		x: 350.0,
		y: 290.0,
		..bottom
	};
	let rect = place(middle, (250.0, 400.0), bounds);
	assert!(rect.intersect(middle).is_none());
	assert!(rect.x > middle.x + middle.w || rect.x + rect.w < middle.x);
}

proptest::proptest! {
	#[test]
	fn floating_placement_stays_within_offset_viewports(
		width in 40.0f32..1800.0, height in 40.0f32..1200.0,
		x in 0.0f32..1.0, y in 0.0f32..1.0,
		w in 1.0f32..900.0, h in 1.0f32..900.0,
	) {
		let bounds = Rect { x: 37.0, y: 48.0, w: width, h: height };
		let anchor = Rect { x: bounds.x + x * (width - 10.0), y: bounds.y + y * (height - 10.0), w: 10.0, h: 10.0 };
		let rect = place(anchor, (w, h), bounds);
		proptest::prop_assert!(rect.x >= bounds.x && rect.y >= bounds.y);
		proptest::prop_assert!(rect.x + rect.w <= bounds.x + width + 0.001);
		proptest::prop_assert!(rect.y + rect.h <= bounds.y + height + 0.001);
		proptest::prop_assert!(rect.w > 0.0 && rect.h > 0.0);
		proptest::prop_assert_eq!(rect, place(anchor, (w, h), bounds));
	}
}

#[test]
fn tall_floating_scenes_shorten_instead_of_covering_the_reference() {
	let bounds = Rect {
		x: 0.0,
		y: 40.0,
		w: 600.0,
		h: 400.0,
	};
	let anchor = Rect {
		x: 300.0,
		y: 240.0,
		w: 12.0,
		h: 18.0,
	};
	let rect = place(anchor, (440.0, 340.0), bounds);
	assert!(rect.intersect(anchor).is_none());
	assert!(rect.h < 340.0);
}

#[test]
fn floating_scenes_align_the_reading_start_with_the_reference() {
	let bounds = Rect {
		x: 0.0,
		y: 40.0,
		w: 1000.0,
		h: 700.0,
	};
	let anchor = Rect {
		x: 100.0,
		y: 200.0,
		w: 100.0,
		h: 20.0,
	};
	let origin = (anchor.x, anchor.y + anchor.h / 2.0);
	for width in [160.0, 420.0] {
		let rect = place(anchor, (width, 120.0), bounds);
		assert!(rect.intersect(anchor).is_none());
		assert!((rect.x + PADDING - origin.0).abs() < 0.01);
		assert!(
			(rect.x + PADDING - origin.0).hypot(rect.y + PADDING - origin.1)
				< 30.0
		);
	}
}

#[test]
fn a_side_preview_aligns_its_first_line_with_the_reference() {
	let bounds = Rect {
		x: 0.0,
		y: 40.0,
		w: 1000.0,
		h: 700.0,
	};
	let anchor = Rect {
		x: 400.0,
		y: 300.0,
		w: 12.0,
		h: 30.0,
	};
	let origin = (anchor.x, anchor.y + anchor.h / 2.0);
	let rect = place(anchor, (250.0, 180.0), bounds);
	assert!(rect.x > anchor.x + anchor.w);
	assert!((rect.y + PADDING - origin.1).abs() < 0.01);
}

#[test]
fn reflowed_preview_surfaces_and_tab_cards_have_separate_styles() {
	let mut ui = crate::test_support::shaper();
	ui.set_stylesheet(std::sync::Arc::new(markview_layout::style::Stylesheet::parse("format_version=3\nversion=1\n[[rule]]\nwhen=['preview']\nbackground='#ffeecc'\nborder_width=3\nradius=9\npadding=1\n[[rule]]\nwhen=['ui','tab_preview']\nradius=4").unwrap()));
	let rect = Rect {
		x: 0.0,
		y: 0.0,
		w: 200.0,
		h: 100.0,
	};
	assert!(matches!(
		&surface(&ui, rect)[0],
		Draw::Box {
			condition: Condition::Preview,
			radius: 9.0,
			border: 3.0,
			..
		}
	));
	let card = card("A tab", false);
	let layout = layout(&card, rect, (800.0, 600.0));
	assert!(matches!(
		&draw(&mut ui, &card, &layout)[0],
		Draw::Box {
			condition: Condition::TabPreview,
			radius: 4.0,
			border: 1.0,
			..
		}
	));
}
