use super::*;
use crate::app::tab_metrics::TabMetrics;
use crate::state::ReaderTab;
use std::time::Duration;

/// The strip labels of tabs that carry no front-end title.
fn labels(tabs: &[ReaderTab]) -> Vec<String> {
	tabs.iter()
		.map(|tab| {
			tab.path
				.file_name()
				.unwrap_or(tab.path.as_os_str())
				.to_string_lossy()
				.into_owned()
		})
		.collect()
}

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
		let labels = labels(&tabs);
		let mut bar = TabBar {
			ui: &mut ui,
			strip: &strip,
			widths: &widths,
			labels: &labels,
			active_tab: 0,
			cursor: (0.0, 0.0),
			viewport: Rect {
				x: 10.0,
				y: 4.0,
				w: 700.0,
				h: 36.0,
			},
		};
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

#[test]
fn hovering_the_close_icon_brightens_it_like_any_other_icon() {
	let mut ui = crate::test_support::shaper();
	let strip = TabStrip::default();
	let tabs = vec![ReaderTab::new("0.md".into())];
	let widths = vec![(100.0, 50.0)];
	let names = labels(&tabs);
	let mut bar = TabBar {
		ui: &mut ui,
		strip: &strip,
		widths: &widths,
		labels: &names,
		active_tab: 0,
		cursor: (0.0, 0.0),
		viewport: Rect {
			x: 10.0,
			y: 4.0,
			w: 700.0,
			h: 36.0,
		},
	};
	let rect = bar.layout().rects[0];
	fn close_ink(bar: &mut TabBar) -> Option<C> {
		let draws = bar.draw_tabs();
		let Draw::Clipped { draws, .. } = &draws[0] else {
			unreachable!()
		};
		draws.iter().find_map(|draw| match draw {
			Draw::Icon {
				paint: Paint::Styled(Condition::Toolbar, color),
				..
			} => Some(*color),
			_ => None,
		})
	}
	bar.cursor = (rect.x + 20.0, 20.0);
	assert_eq!(close_ink(&mut bar), Some(C::Muted));
	bar.cursor = (rect.x + rect.w - 20.0, 20.0);
	assert_eq!(close_ink(&mut bar), Some(C::Color));
}

#[test]
fn a_growing_tab_clips_only_its_label_and_controls() {
	let mut ui = crate::test_support::shaper();
	let tabs: Vec<_> = (0..4)
		.map(|i| ReaderTab::new(format!("{i}.md").into()))
		.collect();
	let widths = vec![(100.0, 50.0); tabs.len()];
	let viewport = Rect {
		x: 10.0,
		y: 4.0,
		w: 700.0,
		h: 36.0,
	};
	let before = TabLayout::new(viewport, &widths[..3], 0.0);
	let mut strip = TabStrip::default();
	strip.begin_open(&before, 1);
	strip.freeze_slide(Duration::ZERO);
	let slot = Rect {
		w: 0.0,
		..before.rects[1]
	};
	let body = body(slot);
	// A zero-width tab paints its caps from the widened body, so the surface
	// does not collapse onto its slot.
	let widened = painted_body(slot);
	// The active cap reaches a radius left of the body to join it seamlessly.
	for (active_tab, left) in [(0, widened.x), (1, widened.x - RADIUS)] {
		let mut bar = TabBar {
			ui: &mut ui,
			strip: &strip,
			widths: &widths,
			labels: &labels(&tabs),
			active_tab,
			cursor: (0.0, 0.0),
			viewport,
		};
		let draws = bar.draw_tabs();
		let Draw::Clipped { draws, .. } = &draws[0] else {
			panic!("tab clip missing")
		};
		let groups: Vec<_> = draws
			.iter()
			.filter_map(|draw| match draw {
				Draw::Clipped { rect, draws } => Some((*rect, draws)),
				_ => None,
			})
			.collect();
		assert_eq!(groups.len(), 1, "only the growing tab clips its content");
		assert_eq!(groups[0].0, body, "the clip is the growing tab's body");
		let clip = draws
			.iter()
			.position(|draw| matches!(draw, Draw::Clipped { .. }))
			.expect("the growing tab clips its content");
		let caps: Vec<_> = draws[clip - 2..clip]
			.iter()
			.filter_map(|draw| match draw {
				Draw::Icon { x, .. } => Some(*x),
				_ => None,
			})
			.collect();
		assert_eq!(
			caps,
			[left, widened.x + widened.w - RADIUS],
			"exactly the two caps of the growing tab, its right cap starting \
			 at the slot, with active {active_tab}"
		);
		let content = groups[0].1;
		assert!(content.iter().any(|draw| matches!(draw, Draw::Glyph(_))));
		assert_eq!(
			content
				.iter()
				.filter(|draw| matches!(draw, Draw::Icon { .. }))
				.count(),
			1,
			"the close icon travels with the label"
		);
	}
}

/// Whether a tab clips its content follows its painted width alone, so any tab
/// left too narrow by an animation clips, not just the one an open started.
#[test]
fn a_narrow_tab_clips_its_content_and_a_resting_one_does_not() {
	let mut ui = crate::test_support::shaper();
	let tabs: Vec<_> = (0..3)
		.map(|i| ReaderTab::new(format!("{i}.md").into()))
		.collect();
	let widths = vec![(100.0, 50.0); tabs.len()];
	let viewport = Rect {
		x: 10.0,
		y: 4.0,
		w: 700.0,
		h: 36.0,
	};
	let before = TabLayout::new(viewport, &widths[..2], 0.0);
	let after = TabLayout::new(viewport, &widths, 0.0);
	let clips = |ui: &mut TextShaper, strip: &TabStrip| {
		let mut bar = TabBar {
			ui,
			strip,
			widths: &widths,
			labels: &labels(&tabs),
			active_tab: 0,
			cursor: (0.0, 0.0),
			viewport,
		};
		let draws = bar.draw_tabs();
		let Draw::Clipped { draws, .. } = &draws[0] else {
			panic!("tab clip missing")
		};
		draws
			.iter()
			.filter_map(|draw| match draw {
				Draw::Clipped { rect, .. } => Some(*rect),
				_ => None,
			})
			.collect::<Vec<_>>()
	};
	let mut strip = TabStrip::default();
	strip.begin_open(&before, 1);
	// The probes follow `TAB_MOTION` 220 ms under `CubicOut`, so they assert
	// phases either side of `CLOSE_INSET`, not exact widths.
	strip.freeze_slide(Duration::from_millis(10));
	let shown = strip.rects(&after).expect("the open slide is in flight");
	assert!(
		shown[1].w > 0.0 && shown[1].w < CLOSE_INSET,
		"the growing tab is far narrower than its label"
	);
	assert_eq!(
		clips(&mut ui, &strip),
		[body(shown[1])],
		"only the overflowing tab clips its label and close icon"
	);

	// Past the close control the tab is still too narrow for its label alone.
	let mut strip = TabStrip::default();
	strip.begin_open(&before, 1);
	strip.freeze_slide(Duration::from_millis(35));
	let shown = strip.rects(&after).expect("the open slide is in flight");
	assert!(
		shown[1].w > CLOSE_INSET && shown[1].w < widths[1].0,
		"the growing tab outgrew its close control without reaching rest"
	);
	assert!(
		ui.text_width("1.md", 12.0) > shown[1].w - TAB_CHROME,
		"the full label no longer fits its space"
	);
	assert_eq!(
		clips(&mut ui, &strip),
		[body(shown[1])],
		"the overflowing label alone makes a tab clip"
	);

	let resting = TabStrip::default();
	assert!(
		clips(&mut ui, &resting).is_empty(),
		"tabs wide enough for their content allocate no clip"
	);
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
	for (dark, name) in [(false, "light"), (true, "dark")] {
		let sheet = markview_layout::style::Stylesheet::bundled(dark);
		ui.set_stylesheet(sheet.clone());
		renderer.set_stylesheet(sheet);
		let labels = labels(&tabs);
		let mut bar = TabBar {
			ui: &mut ui,
			strip: &strip,
			widths: &widths,
			labels: &labels,
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
				moving: true,
				slot: Some(5),
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
				labels: bar.labels,
				active_tab: 0,
				viewport: bar.viewport,
			};
			overlay.truncate(1);
			overlay.extend(drag_bar.draw_tabs());
			let image =
				capture(&mut renderer, &Default::default(), &view, &overlay)?;
			baselines.record(&format!("drag-{name}-{scale}x"), &image)?;
			let active_fill = Paint::Background;
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
			metrics.sync(&mut ui, &labels(&tabs));
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
fn tab_layout_preserves_widths_scroll_and_hit_targets() {
	let mut ui = crate::test_support::shaper();
	let tabs: Vec<_> = (0..30)
		.map(|i| ReaderTab::new(format!("document{i}.md").into()))
		.collect();
	let mut metrics = TabMetrics::default();
	metrics.sync(&mut ui, &labels(&tabs));
	let widths = metrics.widths.clone();
	let mut strip = TabStrip::default();
	strip.scroll = 25.0.into();
	let names = labels(&tabs);
	let mut bar = TabBar {
		ui: &mut ui,
		strip: &strip,
		widths: &widths,
		labels: &names,
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
	let layout = bar.layout();
	for rect in &layout.rects {
		assert_eq!(rect.h, 36.0);
	}
	let active = layout.rects[1];
	assert_eq!(layout.hit(active.x + active.w / 2.0, 20.0), Some(1));
	assert_eq!(layout.hit(active.x + active.w - 16.0, 20.0), Some(1));
	let draws = bar.draw_tabs();
	let Draw::Clipped { draws, .. } = &draws[0] else {
		panic!("tab clip missing")
	};
	assert!(!draws.iter().any(|draw| {
		matches!(
			draw,
			Draw::Rect(_, Paint::Styled(Condition::Toolbar, C::Accent))
		)
	}));
	metrics.sync(bar.ui, &labels(&tabs));
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
		let (fitted, overflowed) = fit_label(&mut ui, name, width);
		assert_eq!(fitted, prefix);
		assert!(!overflowed, "a prefix of exactly its own width fits");
		let (fitted, overflowed) = fit_label(&mut ui, name, width - 0.5);
		assert_eq!(fitted, prefix);
		assert!(overflowed, "the two-grapheme fallback still overflows");
		assert!(ui.text_width(&fitted, 12.0) > width - 0.5);
	}
}

#[test]
fn measured_tabs_fit_minimum_window_and_styles_invalidate_widths() {
	let mut ui = crate::test_support::shaper();
	let mut metrics = TabMetrics::default();
	let tabs: Vec<_> = (0..30)
		.map(|i| ReaderTab::new(format!("中文文档{i}.md").into()))
		.collect();
	metrics.sync(&mut ui, &labels(&tabs));
	let old = metrics.widths.clone();
	let strip = TabStrip::default();
	let names = labels(&tabs);
	let mut bar = TabBar {
		ui: &mut ui,
		strip: &strip,
		widths: &metrics.widths,
		labels: &names,
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
	metrics.sync(&mut ui, &labels(&tabs));
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
		let labels = labels(&tabs);
		let mut bar = TabBar {
			ui: &mut ui,
			strip: &strip,
			widths: &widths,
			labels: &labels,
			active_tab: 0,
			cursor: (0.0, 0.0),
			viewport: Rect {
				x: 10.0,
				y: 4.0,
				w: 900.0,
				h: 36.0,
			},
		};
		let layout = bar.layout();
		for active in 0..count {
			bar.active_tab = active;
			for hovered in std::iter::once(None).chain((0..count).map(Some)) {
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
							Paint::Styled(Condition::Toolbar, C::BorderColor),
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
							&& *i + 1 != active
							&& hovered != Some(*i)
							&& hovered != Some(*i + 1)
					})
					.map(|(_, pair)| (pair[0].x + pair[0].w + pair[1].x) / 2.0)
					.collect();
				assert_eq!(
					separators, expected,
					"active {active}, hovered {hovered:?} of {count}"
				);
			}
		}
	}
}

#[test]
fn overlapping_tabs_hover_the_one_painted_on_top() {
	let rect = Rect {
		x: 10.0,
		y: 4.0,
		w: 100.0,
		h: 32.0,
	};
	let rects = [rect, Rect { x: 50.0, ..rect }];
	assert_eq!(
		hovered_tab(&rects, None, (60.0, 10.0), 1),
		Some(1),
		"the active tab covers the lower one"
	);
	assert_eq!(hovered_tab(&rects, None, (60.0, 10.0), 9), Some(0));
}

#[test]
fn a_shifted_tab_keeps_the_separator_before_it_during_a_drag() {
	use crate::app::tab_strip::{TabDrag, TabLayout, TabStrip};
	// `1 2 3 4` with `2` dragged past `3` shows `1 3 (2) 4`: the joined `1 3`
	// still needs its separator, while the floating tab splits `3` and `4`.
	let mut ui = crate::test_support::shaper();
	let tabs: Vec<_> = (0..4)
		.map(|i| ReaderTab::new(format!("{i}.md").into()))
		.collect();
	let widths = vec![(100.0, 50.0); 4];
	let viewport = Rect {
		x: 10.0,
		y: 4.0,
		w: 700.0,
		h: 32.0,
	};
	let layout = TabLayout::new(viewport, &widths, 0.0);
	let mut strip = TabStrip::default();
	strip.drag = Some(TabDrag {
		index: 1,
		start: layout.rects[1].x,
		grab: 10.0,
		moving: true,
		slot: Some(3),
	});
	strip.retarget_shift(&layout, &widths);
	strip.freeze_shift(Duration::from_secs(1));
	let cursor = (layout.rects[2].x + 20.0, 20.0);
	let mut bar = TabBar {
		ui: &mut ui,
		strip: &strip,
		widths: &widths,
		labels: &labels(&tabs),
		active_tab: 1,
		cursor,
		viewport,
	};
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
	assert_eq!(
		separators,
		[layout.rects[0].x + layout.rects[0].w + GAP / 2.0 - 0.5],
		"the shifted tab keeps the line it shares with its new neighbor"
	);
}

#[test]
fn a_separator_never_floats_beside_an_open_gap() {
	use crate::app::tab_strip::{TabDrag, TabLayout, TabStrip};
	let mut ui = crate::test_support::shaper();
	let tabs: Vec<_> = (0..3)
		.map(|i| ReaderTab::new(format!("{i}.md").into()))
		.collect();
	let widths = vec![(100.0, 50.0); 3];
	let viewport = Rect {
		x: 10.0,
		y: 4.0,
		w: 500.0,
		h: 32.0,
	};
	let layout = TabLayout::new(viewport, &widths, 0.0);
	let mut strip = TabStrip::default();
	strip.drag = Some(TabDrag {
		index: 0,
		start: layout.rects[0].x,
		grab: 10.0,
		moving: true,
		slot: Some(2),
	});
	strip.retarget_shift(&layout, &widths);
	strip.freeze_shift(Duration::from_secs(1));
	let cursor = (layout.rects[2].x + 20.0, 20.0);
	let mut bar = TabBar {
		ui: &mut ui,
		strip: &strip,
		widths: &widths,
		labels: &labels(&tabs),
		active_tab: 0,
		cursor,
		viewport,
	};
	let draws = bar.draw_tabs();
	let Draw::Clipped { draws, .. } = &draws[0] else {
		unreachable!()
	};
	let separators = draws
		.iter()
		.filter(|draw| {
			matches!(
				draw,
				Draw::Rect(
					_,
					Paint::Styled(Condition::Toolbar, C::BorderColor)
				)
			)
		})
		.count();
	assert_eq!(separators, 0, "the shifted tab has no neighbor to divide");
}

#[test]
fn a_moving_drag_hovers_only_the_tab_it_carries() {
	use crate::app::tab_strip::{TabDrag, TabLayout, TabStrip};
	let mut ui = crate::test_support::shaper();
	let tabs: Vec<_> = (0..3)
		.map(|i| ReaderTab::new(format!("{i}.md").into()))
		.collect();
	let widths = vec![(100.0, 50.0); 3];
	let viewport = Rect {
		x: 10.0,
		y: 4.0,
		w: 400.0,
		h: 32.0,
	};
	let layout = TabLayout::new(viewport, &widths, 0.0);
	let mut strip = TabStrip::default();
	strip.drag = Some(TabDrag {
		index: 0,
		start: layout.rects[0].x,
		grab: 10.0,
		moving: true,
		slot: Some(1),
	});
	let cursor = (layout.rects[1].x + 20.0, 20.0);
	let mut bar = TabBar {
		ui: &mut ui,
		strip: &strip,
		widths: &widths,
		labels: &labels(&tabs),
		active_tab: 0,
		cursor,
		viewport,
	};
	let draws = bar.draw_tabs();
	let Draw::Clipped { draws, .. } = &draws[0] else {
		unreachable!()
	};
	let hover_fills = draws
		.iter()
		.filter(|draw| {
			matches!(
				draw,
				Draw::Rect(
					_,
					Paint::Styled(Condition::Button, C::HoverBackground)
				)
			)
		})
		.count();
	assert_eq!(hover_fills, 0, "the covered tab does not take the hover");
}

#[test]
fn dragging_paints_the_tabs_where_the_drop_will_put_them() {
	use crate::app::tab_strip::{TabDrag, TabLayout, TabStrip};
	let mut ui = crate::test_support::shaper();
	let tabs: Vec<_> = (0..3)
		.map(|i| ReaderTab::new(format!("{i}.md").into()))
		.collect();
	let widths = vec![(100.0, 50.0); 3];
	let viewport = Rect {
		x: 10.0,
		y: 4.0,
		w: 400.0,
		h: 32.0,
	};
	let layout = TabLayout::new(viewport, &widths, 0.0);
	let mut strip = TabStrip::default();
	strip.drag = Some(TabDrag {
		index: 0,
		start: 0.0,
		grab: 0.0,
		moving: true,
		slot: Some(2),
	});
	strip.retarget_shift(&layout, &widths);
	strip.freeze_shift(std::time::Duration::from_secs(1));
	let mut bar = TabBar {
		ui: &mut ui,
		strip: &strip,
		widths: &widths,
		labels: &labels(&tabs),
		active_tab: 0,
		cursor: (0.0, 0.0),
		viewport,
	};
	let draws = bar.draw_tabs();
	let Draw::Clipped { draws, .. } = &draws[0] else {
		unreachable!()
	};
	let mut bodies: Vec<_> = draws
		.iter()
		.filter_map(|draw| match draw {
			Draw::Rect(
				rect,
				Paint::Styled(Condition::Toolbar, C::Background),
			) => Some(rect.x),
			_ => None,
		})
		.collect();
	bodies.sort_by(f32::total_cmp);
	let preview = TabLayout::new(viewport, &[(100.0, 50.0); 3], 0.0);
	assert_eq!(
		bodies,
		[
			preview.rects[0].x - GAP / 2.0 + RADIUS - 1.0,
			preview.rects[2].x - GAP / 2.0 + RADIUS - 1.0
		]
	);
}
