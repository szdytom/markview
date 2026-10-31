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
fn tab_hover_match_rendering_baselines() -> anyhow::Result<()> {
	use crate::render::{Renderer, Theme, View};
	use crate::test_support::render_goldens::{Baselines, capture};
	if !markview_platform::test_support::desktop_goldens() {
		return Ok(());
	}
	let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
	let mut baselines = Baselines::new(
		root.join("tests/goldens/tabs"),
		root.join("artifacts/tab-goldens"),
	)?;
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	anyhow::ensure!(
		renderer.adapter_name.starts_with("llvmpipe")
			&& renderer.adapter_name.ends_with("(Vulkan, Cpu)"),
		"Tab baselines require Lavapipe"
	);
	let mut ui = crate::test_support::shaper();
	let strip = TabStrip::default();
	let tabs: Vec<_> = (0..5)
		.map(|i| ReaderTab::new(format!("{i}.md").into()))
		.collect();
	let widths = vec![(100.0, 50.0); tabs.len()];
	for (dark, style, name) in [
		(false, TabStyle::Connected, "light"),
		(true, TabStyle::Connected, "dark"),
		(false, TabStyle::Underline, "underline-light"),
		(true, TabStyle::Underline, "underline-dark"),
	] {
		let sheet = markview_layout::style::Stylesheet::bundled(dark);
		ui.set_stylesheet(sheet.clone());
		renderer.set_stylesheet(sheet);
		let mut bar = TabBar {
			style,
			ui: &mut ui,
			strip: &strip,
			widths: &widths,
			tabs: &tabs,
			active_tab: 4,
			cursor: (264.0, 20.0),
			viewport: Rect {
				x: 10.0,
				y: 4.0,
				w: 600.0,
				h: 36.0,
			},
		};
		let draws = bar.draw_tabs();
		for scale in [1.0, 1.25, 2.0] {
			let horizontal = Default::default();
			let view = View {
				width: (620.0 * scale) as u32,
				height: (48.0 * scale) as u32,
				scale,
				left: 0.0,
				top: 0.0,
				bottom: 0.0,
				scroll: 0.0,
				theme: if dark { Theme::Dark } else { Theme::Light },
				horizontal: &horizontal,
				selection: None,
				revision: 0,
				hovered_link: None,
				hovered_overflow: None,
				held_overflow: None,
			};
			let mut overlay = vec![Draw::Rect(
				Rect {
					x: 0.0,
					y: 0.0,
					w: 620.0,
					h: 48.0,
				},
				Paint::Styled(Condition::Toolbar, C::Background),
			)];
			overlay.extend(draws.clone());
			let image =
				capture(&mut renderer, &Default::default(), &view, &overlay)?;
			baselines.record(&format!("{name}-{scale}x"), &image)?;
			if scale == 2.0 {
				let border = bar
					.ui
					.stylesheet
					.paint(Paint::Styled(Condition::Toolbar, C::BorderColor))
					.map(|channel| (channel * 255.0).round() as u8);
				for x in [221, 222] {
					assert_eq!(
						image.get_pixel(x, 40).0,
						border,
						"separator must retain both pixels at 200% DPI"
					);
				}
			}
			let center = (264.0 * scale) as u32;
			let background = image.get_pixel(0, 0);
			// The upper edge includes both separators but stays above the label.
			for y in (4.0 * scale) as u32..(16.0 * scale) as u32 {
				let highlighted: Vec<_> = ((208.0 * scale) as u32
					..(320.0 * scale) as u32)
					.filter(|x| {
						image
							.get_pixel(*x, y)
							.0
							.iter()
							.zip(background.0)
							.any(|(a, b)| a.abs_diff(b) > 8)
					})
					.collect();
				let left = highlighted.first().expect("hover has a left edge");
				let right = highlighted.last().unwrap();
				assert!(
					(left + right + 1).abs_diff(center * 2) <= 1,
					"asymmetric hover bounds {left}..={right} at y={y}, scale {scale}, dark {dark}"
				);
			}
			let mut drag_strip = TabStrip::default();
			drag_strip.drag = Some(crate::app::tab_strip::TabDrag {
				index: 0,
				start: 100.0,
				grab: 20.0,
				last: 150.0,
				moving: true,
			});
			let drag_widths = [
				(280.0, 280.0),
				(60.0, 60.0),
				(60.0, 60.0),
				(60.0, 60.0),
				(60.0, 60.0),
			];
			let mut drag_bar = TabBar {
				strip: &drag_strip,
				widths: &drag_widths,
				ui: &mut *bar.ui,
				cursor: (150.0, 20.0),
				style: bar.style,
				tabs: bar.tabs,
				active_tab: 0,
				viewport: bar.viewport,
			};
			overlay.truncate(1);
			overlay.extend(drag_bar.draw_tabs());
			let image =
				capture(&mut renderer, &Default::default(), &view, &overlay)?;
			baselines.record(&format!("drag-{name}-{scale}x"), &image)?;
			let active_fill = match style {
				TabStyle::Connected => Paint::Background,
				TabStyle::Underline => {
					Paint::Styled(Condition::Panel, C::Background)
				}
			};
			let fill = drag_bar
				.ui
				.stylesheet
				.paint(active_fill)
				.map(|channel| (channel * 255.0).round() as u8);
			for x in (340.0 * scale) as u32..(365.0 * scale) as u32 {
				assert_eq!(
					image.get_pixel(x, (14.0 * scale) as u32).0,
					fill,
					"separator crossed the dragged tab at x={x}, scale {scale}"
				);
			}
		}
	}
	baselines.finish()
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
				for hovered in std::iter::once(None).chain((0..count).map(Some))
				{
					bar.cursor = hovered.map_or((0.0, 0.0), |i| {
						let rect = layout.rects[i];
						(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
					});
					let draws = bar.draw_tabs();
					let Draw::Clipped { draws, .. } = &draws[0] else {
						unreachable!()
					};
					let separators: Vec<_> = draws
						.iter()
						.filter_map(|draw| match draw {
							Draw::Rect(
								rect,
								Paint::Styled(
									Condition::Toolbar,
									C::BorderColor,
								),
							) => Some(rect.x + rect.w / 2.0),
							_ => None,
						})
						.collect();
					let expected: Vec<_> = layout
						.rects
						.windows(2)
						.enumerate()
						.filter(|(i, _)| {
							*i != active
								&& *i + 1 != active && hovered != Some(*i)
								&& hovered != Some(*i + 1)
						})
						.map(|(_, pair)| {
							(pair[0].x + pair[0].w + pair[1].x) / 2.0
						})
						.collect();
					assert_eq!(
						separators, expected,
						"{style:?}: active {active}, hovered {hovered:?} of {count}"
					);
				}
			}
		}
	}
}
