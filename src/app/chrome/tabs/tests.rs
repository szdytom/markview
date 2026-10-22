use super::*;
use crate::app::tab_metrics::TabMetrics;

#[test]
fn hovered_labels_mix_current_theme_colors_and_keep_the_background_highlight() {
	use markview_layout::style::Stylesheet;
	let mut ui = crate::test_support::shaper();
	let strip = TabStrip::default();
	let tabs: Vec<_> = (0..3)
		.map(|i| ReaderTab::new(format!("{i}.md").into()))
		.collect();
	let widths = vec![(100.0, 50.0); tabs.len()];
	let custom = Stylesheet::parse("format_version=2\nversion=1\n[[rule]]\nwhen=['ui','toolbar']\nmuted='#20406080'\ncolor='#80A0C0FF'\n").unwrap();
	for sheet in [
		Stylesheet::bundled(false),
		Stylesheet::bundled(true),
		std::sync::Arc::new(custom),
	] {
		ui.set_stylesheet(sheet);
		let muted = ui.stylesheet.color(Condition::Toolbar, C::Muted);
		let active = ui.stylesheet.color(Condition::Toolbar, C::Color);
		let mixed = std::array::from_fn::<_, 4, _>(|i| {
			muted[i] * 0.3 + active[i] * 0.7
		});
		let mut bar = TabBar {
			ui: &mut ui,
			strip: &strip,
			widths: &widths,
			tabs: &tabs,
			active_tab: 0,
			cursor: (0.0, 0.0),
			style: TabStyle::Underline,
			viewport: Rect {
				x: 10.0,
				y: 4.0,
				w: 700.0,
				h: 36.0,
			},
		};
		for style in [TabStyle::Underline, TabStyle::Connected] {
			bar.style = style;
			let layout = bar.layout();
			for hovered in [None, Some(0), Some(1)] {
				bar.cursor = hovered
					.map_or((0.0, 0.0), |i| (layout.rects[i].x + 20.0, 20.0));
				let draws = bar.draw_tabs();
				let Draw::Clipped { draws, .. } = &draws[0] else {
					unreachable!()
				};
				for (i, rect) in layout.rects.iter().enumerate() {
					let expected = if i == 0 {
						active
					} else if hovered == Some(i) {
						mixed
					} else {
						muted
					};
					let glyphs: Vec<_> = draws
						.iter()
						.filter_map(|draw| match draw {
							Draw::Glyph(glyph)
								if rect.contains(glyph.x, glyph.y) =>
							{
								Some(glyph)
							}
							_ => None,
						})
						.collect();
					assert!(!glyphs.is_empty());
					for glyph in glyphs {
						let actual = bar.ui.stylesheet.paint(glyph.paint);
						assert!(actual.iter().zip(expected).all(|(a, b)| {
							(a - b).abs() <= 0.5 / 255.0 + 0.00001
						}));
					}
				}
				let hover_fills = draws
					.iter()
					.filter(|draw| {
						matches!(
							draw,
							Draw::Rect(
								_,
								Paint::Styled(
									Condition::Button,
									C::HoverBackground
								)
							)
						)
					})
					.count();
				assert_eq!(hover_fills > 0, hovered == Some(1));
			}
		}
	}
}

#[test]
fn cached_tab_end_follows_scroll_theme_and_document_changes() {
	let mut ui = crate::test_support::shaper();
	let mut metrics = TabMetrics::default();
	let mut tabs: Vec<_> = (0..20)
		.map(|i| ReaderTab::new(format!("中文文档{i}.md").into()))
		.collect();
	for count in [20, 2, 1, 0] {
		tabs.truncate(count);
		for size in [1.0, 1.5] {
			let sheet = markview_layout::style::Stylesheet::parse(&format!(
				"format_version=2\nversion=1\n[[rule]]\nwhen=['ui','toolbar']\nsize={size}"
			))
			.unwrap();
			ui.set_stylesheet(std::sync::Arc::new(sheet));
			metrics.sync(&mut ui, &tabs);
			for width in [150.0, 500.5, 1200.0] {
				for scroll in [-10.0, 0.0, 25.5, 10000.0] {
					let viewport = Rect {
						x: 10.0,
						y: 4.0,
						w: width,
						h: 36.0,
					};
					let layout =
						TabLayout::new(viewport, &metrics.widths, scroll);
					let end = layout
						.rects
						.last()
						.map_or(viewport.x, |rect| rect.x + rect.w);
					assert!(
						(metrics.end(viewport, scroll) - end).abs() < 0.001
					);
				}
			}
		}
	}
}

#[test]
fn live_tab_style_switch_preserves_widths_scroll_and_hit_targets() {
	let mut ui = crate::test_support::shaper();
	let tabs: Vec<_> = (0..30)
		.map(|i| ReaderTab::new(format!("document{i}.md").into()))
		.collect();
	let mut metrics = TabMetrics::default();
	metrics.sync(&mut ui, &tabs);
	let widths = metrics.widths.clone();
	let mut strip = TabStrip::default();
	strip.scroll = 25.0.into();
	let mut bar = TabBar {
		style: TabStyle::default(),
		ui: &mut ui,
		strip: &strip,
		widths: &widths,
		tabs: &tabs,
		active_tab: 1,
		cursor: (0.0, 0.0),
		viewport: crate::app::frame::Layout::new(
			crate::settings::WindowLayout::Windows,
			false,
			500.0,
			300.0,
			false,
			false,
		)
		.tabs,
	};
	let original = bar.layout();
	for (style, height) in
		[(TabStyle::Underline, 32.0), (TabStyle::Connected, 36.0)]
	{
		bar.style = style;
		let layout = bar.layout();
		assert_eq!(layout.scroll, original.scroll);
		for (rect, before) in layout.rects.iter().zip(&original.rects) {
			assert_eq!((rect.x, rect.w, rect.h), (before.x, before.w, height));
		}
		let active = layout.rects[1];
		assert_eq!(layout.hit(active.x + active.w / 2.0, 20.0), Some(1));
		assert_eq!(layout.hit(active.x + active.w - 16.0, 20.0), Some(1));
		let draws = bar.draw_tabs();
		let Draw::Clipped { draws, .. } = &draws[0] else {
			panic!("tab clip missing")
		};
		let underline = draws.iter().any(|draw| {
			matches!(
				draw,
				Draw::Rect(_, Paint::Styled(Condition::Toolbar, C::Accent))
			)
		});
		assert_eq!(underline, style == TabStyle::Underline);
	}
	metrics.sync(bar.ui, &tabs);
	assert_eq!(metrics.widths, widths);
}

#[test]
fn compact_labels_keep_two_whole_graphemes_and_fit_the_measured_space() {
	let mut ui = crate::test_support::shaper();
	ui.appearance = crate::app::tab_metrics::tab_appearance(&ui);
	for name in ["中文文档.md", "e\u{301}日笔记.md", "👩‍💻🙂notes.md", "a.md"]
	{
		let prefix: String = name.graphemes(true).take(2).collect();
		let width = ui.text_width(&prefix, 12.0);
		let fitted = fit_label(&mut ui, name, width);
		assert_eq!(fitted, prefix);
		assert!(ui.text_width(&fitted, 12.0) <= width + 0.001);
	}
}

#[test]
fn measured_tabs_fit_minimum_window_and_styles_invalidate_widths() {
	let mut ui = crate::test_support::shaper();
	let mut metrics = TabMetrics::default();
	let tabs: Vec<_> = (0..30)
		.map(|i| ReaderTab::new(format!("中文文档{i}.md").into()))
		.collect();
	metrics.sync(&mut ui, &tabs);
	let old = metrics.widths.clone();
	let strip = TabStrip::default();
	let mut bar = TabBar {
		style: TabStyle::default(),
		ui: &mut ui,
		strip: &strip,
		widths: &metrics.widths,
		tabs: &tabs,
		active_tab: 0,
		cursor: (0.0, 0.0),
		viewport: crate::app::frame::Layout::new(
			crate::settings::WindowLayout::Windows,
			false,
			500.0,
			300.0,
			false,
			false,
		)
		.tabs,
	};
	let layout = bar.layout();
	assert!(layout.max_scroll > 0.0);
	for (rect, (_, minimum)) in layout.rects.iter().zip(&metrics.widths) {
		assert!((rect.w - minimum).abs() < 0.001);
	}
	let sheet = markview_layout::style::Stylesheet::parse(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['ui','toolbar']\nsize = 1.5\n",
	)
	.unwrap();
	ui.set_stylesheet(std::sync::Arc::new(sheet));
	metrics.sync(&mut ui, &tabs);
	assert!(metrics.widths[0].1 > old[0].1);
	metrics.sync(&mut ui, &[]);
	assert!(metrics.widths.is_empty());
}

#[test]
fn separators_only_divide_neighboring_inactive_tabs() {
	let mut ui = crate::test_support::shaper();
	let strip = TabStrip::default();
	for count in 1..=5 {
		let tabs: Vec<_> = (0..count)
			.map(|i| ReaderTab::new(format!("{i}.md").into()))
			.collect();
		let widths = vec![(100.0, 50.0); count];
		let mut bar = TabBar {
			ui: &mut ui,
			strip: &strip,
			widths: &widths,
			tabs: &tabs,
			active_tab: 0,
			cursor: (0.0, 0.0),
			style: TabStyle::Underline,
			viewport: Rect {
				x: 10.0,
				y: 4.0,
				w: 900.0,
				h: 36.0,
			},
		};
		for style in [TabStyle::Underline, TabStyle::Connected] {
			bar.style = style;
			let layout = bar.layout();
			for active in 0..count {
				bar.active_tab = active;
				let draws = bar.draw_tabs();
				let Draw::Clipped { draws, .. } = &draws[0] else {
					unreachable!()
				};
				let separators: Vec<_> = draws
					.iter()
					.filter_map(|draw| match draw {
						Draw::Rect(
							rect,
							Paint::Styled(Condition::Toolbar, C::BorderColor),
						) => Some(rect.x),
						_ => None,
					})
					.collect();
				let expected: Vec<_> = layout
					.rects
					.windows(2)
					.enumerate()
					.filter(|(i, _)| *i != active && *i + 1 != active)
					.map(|(_, pair)| pair[0].x + pair[0].w - 1.0)
					.collect();
				assert_eq!(
					separators, expected,
					"{style:?}: active tab {active} of {count}"
				);
			}
		}
	}
}
