use crate::state::{Command, Modal};
use crate::{
	document,
	layout::LayoutEngine,
	render::{Renderer, Theme, View},
};
use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;

use super::*;
#[test]
#[ignore = "requires a GPU; writes artifacts/refactor-ui.png"]
fn settings_and_selection_frame() -> Result<()> {
	for (width, height, theme, panel_open, filename) in [
		(800.0, 600.0, Theme::Light, true, "refactor-ui.png"),
		(800.0, 600.0, Theme::Dark, true, "settings-dark.png"),
		(500.0, 300.0, Theme::Light, true, "settings-compact.png"),
		(800.0, 600.0, Theme::Light, false, "reader-chrome.png"),
		(
			500.0,
			300.0,
			Theme::Dark,
			false,
			"reader-chrome-compact.png",
		),
	] {
		let settings = ReaderSettings {
			theme,
			..Default::default()
		};
		let document = document::parse(
			"# Reading selections\n\nSelect **English**, 中文 and $x^2$ across lines.\n\n```rust\n\tlet answer = 42;\n```\n\n| A | B |\n|---|---|\n| one | two |\n",
		);
		let snapshot = LayoutEngine::new().layout(
			&document,
			&settings.layout_options(
				width,
				false,
				&crate::test_support::fonts(),
			),
		);
		let interaction = InteractionState {
			panel_open,
			focus_visible: true,
			focus: Some(if panel_open {
				Command::Larger
			} else {
				Command::Settings
			}),
			..Default::default()
		};
		let counts = markview_core::text::TextCounts::of(
			&snapshot.extract_text(snapshot.select_all(1).unwrap(), 1),
		);
		let mut overlay = vec![
			Draw::Rect(
				Rect {
					x: 0.0,
					y: 0.0,
					w: width,
					h: TOP,
				},
				Paint::Background,
			),
			Draw::Rect(
				Rect {
					x: 0.0,
					y: TOP - 1.0,
					w: width,
					h: 1.0,
				},
				Paint::Border,
			),
		];
		overlay.extend(draw_footer(
			&mut crate::test_support::shaper(),
			Some(counts),
			Some(counts),
			None,
			"",
			width,
			height,
		));
		overlay.extend(draw_controls(
			&mut crate::test_support::shaper(),
			&settings,
			&interaction,
			width,
			height,
		));
		let mut renderer = pollster::block_on(Renderer::new(None))?;
		let horizontal = HashMap::new();
		let view = View {
			hovered_link: None,
			held_overflow: None,
			hovered_overflow: None,
			width: (width * 1.25) as u32,
			height: (height * 1.25) as u32,
			scale: 1.25,
			left: 20.0,
			top: TOP + 10.0,
			bottom: BOTTOM + 10.0,
			scroll: 0.0,
			theme: settings.theme,
			horizontal: &horizontal,
			selection: snapshot.select_all(1),
			revision: 1,
		};
		let target = renderer.offscreen(view.width, view.height);
		let submission = renderer.render(
			&snapshot,
			&view,
			&overlay,
			&target.create_view(&Default::default()),
		)?;
		renderer.wait(Some(submission))?;
		let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("artifacts")
			.join(filename);
		std::fs::create_dir_all(output.parent().unwrap())?;
		renderer.save_png(&target, &output)?;
		if panel_open {
			let mut settings = settings.clone();
			settings.style = Some(vec!["paper".into(), "dark".into()]);
			let mut entries =
				crate::stylesheet::catalog(None, settings.style.as_deref());
			let paper = entries.iter_mut().find(|e| e.id == "paper").unwrap();
			paper.name = "纸与墨".into();
			paper.source = "/example/styles/paper.mvss.toml".into();
			paper.error = None;
			entries.push(crate::stylesheet::Entry {
				id: "invalid".into(),
				name: "Invalid stylesheet".into(),
				source: "/example/styles/invalid.mvss.toml".into(),
				error: Some("em.font: must not be empty".into()),
				urls: vec![
					"https://example.invalid/NotoSerif-Regular.ttf".into(),
				],
			});
			let overlay = draw_styles(
				&mut crate::test_support::shaper(),
				super::styles::StylesTarget::Reader,
				settings.style.as_deref(),
				&interaction,
				&entries,
				&crate::fonts::Status::default(),
				0,
				width,
				height,
			);
			let submission = renderer.render(
				&snapshot,
				&view,
				&overlay,
				&target.create_view(&Default::default()),
			)?;
			renderer.wait(Some(submission))?;
			renderer.save_png(
				&target,
				&output.with_file_name(format!("styles-{filename}")),
			)?;
		}
	}
	Ok(())
}

#[test]
#[ignore = "requires a GPU; writes artifacts/notice-*.png and artifacts/confirm-modal*.png"]
fn notice_strip_and_confirmation_frames() -> Result<()> {
	let (width, height) = (800.0_f32, 600.0_f32);
	let directory =
		std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("artifacts");
	std::fs::create_dir_all(&directory)?;
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	for (theme, dark) in [(Theme::Light, false), (Theme::Dark, true)] {
		let settings = ReaderSettings {
			theme,
			stylesheet: markview_core::style::Stylesheet::bundled(dark),
			..Default::default()
		};
		renderer.set_stylesheet(settings.stylesheet.clone());
		let document = document::parse(
			"# Remote images\n\nThe loader requested a bounded number of these.\n\n![one](https://example.com/one.png)\n\nText continues below the notice strip.\n",
		);
		let snapshot = LayoutEngine::new().layout_with_images(
			&document,
			&settings.layout_options(
				width,
				false,
				&crate::test_support::fonts(),
			),
			&Default::default(),
		);
		let mut ui = crate::test_support::shaper();
		ui.set_stylesheet(settings.stylesheet.clone());
		let toolbar = |ui: &mut TextShaper| -> Vec<Draw> {
			vec![
				Draw::Rect(
					Rect {
						x: 0.0,
						y: 0.0,
						w: width,
						h: TOP,
					},
					Paint::Styled(Condition::Toolbar, C::Background),
				),
				Draw::Rect(
					Rect {
						x: 0.0,
						y: TOP - 1.0,
						w: width,
						h: 1.0,
					},
					Paint::Styled(Condition::Toolbar, C::BorderColor),
				),
				Draw::Rect(
					Rect {
						x: 0.0,
						y: height - BOTTOM,
						w: width,
						h: BOTTOM,
					},
					Paint::Styled(Condition::Toolbar, C::Background),
				),
				Draw::Rect(
					Rect {
						x: 0.0,
						y: height - BOTTOM,
						w: width,
						h: 1.0,
					},
					Paint::Styled(Condition::Toolbar, C::BorderColor),
				),
			]
			.into_iter()
			.chain(draw_footer(ui, None, None, None, "", width, height))
			.collect()
		};
		let horizontal = HashMap::new();
		let view = |top: f32| View {
			width: (width * 1.25) as u32,
			height: (height * 1.25) as u32,
			scale: 1.25,
			left: 20.0,
			top,
			bottom: BOTTOM + 10.0,
			scroll: 0.0,
			theme,
			horizontal: &horizontal,
			selection: None,
			revision: 1,
			hovered_link: None,
			hovered_overflow: None,
			held_overflow: None,
		};
		let target =
			renderer.offscreen((width * 1.25) as u32, (height * 1.25) as u32);
		let suffix = if dark { "dark" } else { "light" };
		// The notice strip reserves its own band above the document.
		let mut overlay = toolbar(&mut ui);
		overlay.extend(draw_banner(
			&mut ui,
			width,
			37,
			&InteractionState::default(),
		));
		let submission = renderer.render(
			&snapshot,
			&view(content_top(true) + 10.0),
			&overlay,
			&target.create_view(&Default::default()),
		)?;
		renderer.wait(Some(submission))?;
		renderer.save_png(
			&target,
			&directory.join(format!("notice-strip-{suffix}.png")),
		)?;
		// The confirmation owns the frame; "Open folder" is focused. The target
		// sits under the open document, so the short relative form is shown.
		let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("artifacts");
		let targets = root.join("manual-test/targets");
		let relative = InteractionState {
			modal: Some(Modal::OpenLocal {
				path: targets.join("payload.desktop"),
				dir: targets.clone(),
				document_dir: Some(root.join("manual-test")),
			}),
			focus_visible: true,
			focus: Some(Command::ModalOpenFolder),
			cursor: (620.0, 300.0),
			..Default::default()
		};
		for (interaction, name) in [
			(&relative, "confirm-modal"),
			// A far target has no short relative form; its front is elided.
			(
				&InteractionState {
					modal: Some(Modal::OpenLocal {
						path: PathBuf::from(
							"/srv/data/archive/2026/exports/nightly/release-candidates/marketing/payload.desktop",
						),
						dir: PathBuf::from(
							"/srv/data/archive/2026/exports/nightly",
						),
						document_dir: Some(root.join("manual-test")),
					}),
					focus_visible: true,
					focus: Some(Command::ModalOpenFolder),
					cursor: (620.0, 300.0),
					..Default::default()
				},
				"confirm-modal-long",
			),
		] {
			let mut overlay = toolbar(&mut ui);
			overlay.extend(modal::draw_modal(
				&mut ui,
				interaction,
				width,
				height,
			));
			let submission = renderer.render(
				&snapshot,
				&view(TOP + 10.0),
				&overlay,
				&target.create_view(&Default::default()),
			)?;
			renderer.wait(Some(submission))?;
			renderer.save_png(
				&target,
				&directory.join(format!("{name}-{suffix}.png")),
			)?;
		}
	}
	Ok(())
}

#[test]
#[ignore = "requires a GPU; writes artifacts/tab-bar/*.png"]
fn tab_strip_frames_clip_overflow_at_fractional_dpi() -> Result<()> {
	use crate::app::{
		tab_metrics::TabMetrics,
		tab_strip::{TabDrag, TabStrip},
	};
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("artifacts/tab-bar");
	std::fs::create_dir_all(&directory)?;
	for (count, scroll, dragging, dark, filename) in [
		(3, 0.0, false, false, "normal.png"),
		(5, 0.0, false, false, "compressed.png"),
		(30, 25.0, false, false, "overflow.png"),
		(30, 125.0, true, true, "drag-dark.png"),
	] {
		let settings = ReaderSettings {
			theme: if dark { Theme::Dark } else { Theme::Light },
			stylesheet: markview_core::style::Stylesheet::bundled(dark),
			..Default::default()
		};
		renderer.set_stylesheet(settings.stylesheet.clone());
		let mut ui = crate::test_support::shaper();
		ui.set_stylesheet(settings.stylesheet.clone());
		let entries: Vec<_> = (0..count)
			.map(|i| {
				ReaderTab::new(
					format!("{}文档{i}.md", ["中文", "开发", "阅读"][i % 3])
						.into(),
				)
			})
			.collect();
		let mut metrics = TabMetrics::default();
		metrics.sync(&mut ui, &entries);
		let strip = TabStrip {
			scroll,
			drag: dragging.then_some(TabDrag {
				index: 3,
				start: 100.0,
				grab: 20.0,
				last: 150.0,
				moving: true,
			}),
			..Default::default()
		};
		let width = if count == 3 { 800.0 } else { 500.0 };
		let mut bar = tabs::TabBar {
			ui: &mut ui,
			strip: &strip,
			widths: &metrics.widths,
			tabs: &entries,
			active_tab: 3.min(count - 1),
			cursor: (150.0, 20.0),
			width,
		};
		let viewport = bar.layout().viewport;
		let tabs = bar.draw_tabs();
		let background = Draw::Rect(
			Rect {
				x: 0.0,
				y: 0.0,
				w: width,
				h: TOP,
			},
			Paint::Styled(Condition::Toolbar, C::Background),
		);
		let controls = draw_controls(
			&mut ui,
			&settings,
			&InteractionState::default(),
			width,
			100.0,
		);
		let horizontal = HashMap::new();
		let view = View {
			width: (width * 1.25) as u32,
			height: 125,
			scale: 1.25,
			left: 20.0,
			top: 50.0,
			bottom: 10.0,
			scroll: 0.0,
			theme: settings.theme,
			horizontal: &horizontal,
			selection: None,
			revision: 0,
			hovered_link: None,
			hovered_overflow: None,
			held_overflow: None,
		};
		let snapshot = Default::default();
		let target = renderer.offscreen(view.width, view.height);
		let mut images = Vec::new();
		for visible in [false, true] {
			let mut overlay = vec![background.clone()];
			if visible {
				overlay.extend(tabs.clone());
			}
			overlay.extend(controls.clone());
			let submission = renderer.render(
				&snapshot,
				&view,
				&overlay,
				&target.create_view(&Default::default()),
			)?;
			renderer.wait(Some(submission))?;
			let output = directory.join(if visible {
				filename.to_string()
			} else {
				format!("baseline-{filename}")
			});
			renderer.save_png(&target, &output)?;
			images.push(image::open(output)?.to_rgba8());
		}
		for y in 5..45 {
			for x in 0..view.width {
				if x < (viewport.x * view.scale).floor() as u32
					|| x >= ((viewport.x + viewport.w) * view.scale).ceil()
						as u32
				{
					assert_eq!(
						images[0].get_pixel(x, y),
						images[1].get_pixel(x, y),
						"tab escaped its clip at {x},{y}: {filename}"
					);
				}
			}
		}
	}
	Ok(())
}

/// A vector icon keeps its optical centre inside its button at any device
/// phase. The icon bakes its subpixel position into the raster, so it must
/// stay centred on a button that sits on a half device pixel instead of
/// snapping to the grid, which leaves it visibly off centre at 125% DPI.
///
/// The Settings icon is the probe because it is symmetric: a correctly placed
/// raster puts its coverage centroid on the box centre.
#[test]
#[ignore = "requires a GPU; writes artifacts/icon-centre-*.png"]
fn icons_keep_their_optical_centre_at_fractional_dpi() -> Result<()> {
	const SCALE: f32 = 1.25;
	const HEIGHT: f32 = 300.0;
	let settings = ReaderSettings {
		stylesheet: markview_core::style::Stylesheet::bundled(false),
		..Default::default()
	};
	let horizontal = HashMap::new();
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	renderer.set_stylesheet(settings.stylesheet.clone());
	// `800.4` puts the buttons half a device pixel right of `800`, the worst
	// case for an icon that snaps to the grid.
	for width in [800.0_f32, 800.4] {
		let mut ui = crate::test_support::shaper();
		ui.set_stylesheet(settings.stylesheet.clone());
		let view = View {
			width: (width * SCALE) as u32,
			height: (HEIGHT * SCALE) as u32,
			scale: SCALE,
			left: 20.0,
			top: TOP + 10.0,
			bottom: BOTTOM + 10.0,
			scroll: 0.0,
			theme: Theme::Light,
			horizontal: &horizontal,
			selection: None,
			revision: 0,
			hovered_link: None,
			hovered_overflow: None,
			held_overflow: None,
		};
		let overlay = draw_controls(
			&mut ui,
			&settings,
			&InteractionState::default(),
			width,
			HEIGHT,
		);
		let target = renderer.offscreen(view.width, view.height);
		let submission = renderer.render(
			&Default::default(),
			&view,
			&overlay,
			&target.create_view(&Default::default()),
		)?;
		renderer.wait(Some(submission))?;
		let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("artifacts")
			.join(format!("icon-centre-{width}.png"));
		std::fs::create_dir_all(output.parent().unwrap())?;
		renderer.save_png(&target, &output)?;
		let image = image::open(output)?.to_rgba8();
		// Both buttons center their 20 px icon box four pixels inside their
		// rectangle; Settings is the second button, so it sits at `width - 40`.
		let left = (width - 40.0) * SCALE;
		let top = 10.0 * SCALE;
		let centre = [left + 10.0 * SCALE, top + 10.0 * SCALE];
		let (x, y) = coverage_centroid(&image, left, top, 20.0 * SCALE);
		let offset = [x - f64::from(centre[0]), y - f64::from(centre[1])];
		assert!(
			offset[0].abs() < 0.25 && offset[1].abs() < 0.25,
			"the icon is off centre at {width}: {:+.3}, {:+.3} px",
			offset[0],
			offset[1]
		);
	}
	Ok(())
}

/// The coverage centroid of whatever is drawn inside a square region, in
/// device pixels. The button fill is sampled at the region's corner.
fn coverage_centroid(
	image: &image::RgbaImage,
	x: f32,
	y: f32,
	side: f32,
) -> (f64, f64) {
	let (x0, y0) = (x.floor() as u32, y.floor() as u32);
	let (x1, y1) = ((x + side).ceil() as u32, (y + side).ceil() as u32);
	let fill = image.get_pixel(x0 + 1, y0 + 1).0;
	let (mut weight, mut sx, mut sy) = (0.0, 0.0, 0.0);
	for py in y0..y1 {
		for px in x0..x1 {
			let pixel = image.get_pixel(px, py).0;
			let delta: f64 =
				(0..3).map(|i| f64::from(pixel[i].abs_diff(fill[i]))).sum();
			if delta > 8.0 {
				weight += delta;
				sx += delta * (f64::from(px) + 0.5);
				sy += delta * (f64::from(py) + 0.5);
			}
		}
	}
	(sx / weight, sy / weight)
}

#[test]
#[ignore = "requires a GPU; writes artifacts/export-whole.png"]
fn a_whole_document_png_export_stitches_its_tiles() -> Result<()> {
	let settings = crate::settings::ExportSettings {
		format: crate::settings::ExportFormat::Png,
		scale: 1.0,
		..Default::default()
	};
	let geometry = crate::export::geometry(&settings)?;
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	let sheet = markview_core::style::Stylesheet::bundled_print();
	renderer.set_stylesheet(sheet.clone());
	let document = document::parse(
		"# Exporting\n\nA paragraph with 中文 and **bold** text.\n\n- one\n- two\n\n"
			.repeat(30),
	);
	let options = crate::layout::LayoutOptions {
		width: geometry.text_px().0,
		stylesheet: sheet.clone(),
		fonts: crate::test_support::fonts(),
		..Default::default()
	};
	let snapshot = LayoutEngine::new().layout(&document, &options);
	assert!(
		snapshot.height > 1024.0,
		"the document must need several tiles"
	);
	// A small texture limit forces the strip plan the export uses; it must
	// still clear the page's own width.
	let plan =
		crate::export::plan(&geometry, snapshot.height, settings.scale, 1024)?;
	assert!(plan.tiles.len() > 1, "{plan:?}");
	let stylesheet = std::sync::Arc::new(sheet);
	let mut rgba =
		vec![0; plan.width_px as usize * plan.height_px as usize * 4];
	for tile in plan.tiles.iter().copied() {
		crate::app::export::draw_tile(
			&mut renderer,
			&snapshot,
			&plan,
			&stylesheet,
			tile,
			settings.scale,
			geometry.margin_pt[3] / markview_core::paginate::PT_PER_PX,
			Theme::Light,
			&mut rgba,
		)?;
	}
	let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("artifacts/export-whole.png");
	std::fs::create_dir_all(output.parent().unwrap())?;
	crate::app::export::write_png(
		&output,
		&rgba,
		plan.width_px,
		plan.height_px,
	)?;
	let image = image::open(&output)?.to_rgba8();
	assert_eq!(image.dimensions(), (plan.width_px, plan.height_px));
	// The top margin is the sheet's own background, opaque.
	assert_eq!(image.get_pixel(0, 0).0, [255, 255, 255, 255]);
	Ok(())
}

#[test]
#[ignore = "requires a GPU; writes artifacts/export-panel*.png"]
fn export_panel_frames() -> Result<()> {
	use super::export::draw_export;
	let (width, height) = (1000.0_f32, 700.0_f32);
	let directory =
		std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("artifacts");
	std::fs::create_dir_all(&directory)?;
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	for (settings, dark, name) in [
		(
			crate::settings::ExportSettings::default(),
			false,
			"export-panel.png",
		),
		(
			crate::settings::ExportSettings {
				format: crate::settings::ExportFormat::Png,
				paragraph_indent: 2.0,
				..Default::default()
			},
			true,
			"export-panel-png.png",
		),
	] {
		let theme = if dark { Theme::Dark } else { Theme::Light };
		let sheet = markview_core::style::Stylesheet::bundled(dark);
		renderer.set_stylesheet(sheet.clone());
		let document = document::parse(
			"# Exporting\n\nA paragraph behind the panel.\n\n- one\n- two\n",
		);
		let snapshot = LayoutEngine::new().layout(
			&document,
			&crate::layout::LayoutOptions {
				width: 600.0,
				stylesheet: sheet,
				fonts: crate::test_support::fonts(),
				..Default::default()
			},
		);
		let mut ui = crate::test_support::shaper();
		ui.set_stylesheet(markview_core::style::Stylesheet::bundled(dark));
		let mut overlay = vec![
			Draw::Rect(
				Rect {
					x: 0.0,
					y: 0.0,
					w: width,
					h: TOP,
				},
				Paint::Background,
			),
			Draw::Rect(
				Rect {
					x: 0.0,
					y: TOP - 1.0,
					w: width,
					h: 1.0,
				},
				Paint::Border,
			),
		];
		overlay.extend(draw_export(
			&mut ui,
			&settings,
			&InteractionState {
				panel_open: true,
				export_open: true,
				focus_visible: true,
				focus: Some(Command::ExportRun),
				..Default::default()
			},
			"document.md",
			false,
			width,
			height,
		));
		let view = View {
			selection: None,
			revision: 0,
			width: width as u32,
			height: height as u32,
			scale: 1.0,
			scroll: 0.0,
			left: 200.0,
			top: TOP + 10.0,
			bottom: 10.0,
			theme,
			horizontal: &HashMap::new(),
			hovered_link: None,
			hovered_overflow: None,
			held_overflow: None,
		};
		let target = renderer.offscreen(width as u32, height as u32);
		let submission = renderer.render(
			&snapshot,
			&view,
			&overlay,
			&target.create_view(&Default::default()),
		)?;
		renderer.wait(Some(submission))?;
		renderer.save_png(&target, &directory.join(name))?;
	}
	Ok(())
}

#[test]
#[ignore = "requires a GPU; writes artifacts/ui-redesign/*.png"]
fn redesigned_chrome_frames() -> Result<()> {
	use crate::app::{tab_metrics::TabMetrics, tab_strip::TabStrip};
	let output =
		PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("artifacts/ui-redesign");
	std::fs::create_dir_all(&output)?;
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	let fonts = crate::test_support::fonts();
	let document = document::parse(
		"# Reading, without distractions\n\nA native home for **Markdown**, 中文 and $x^2$.\n\n## A clear view\n\n- Publication-quality typography\n- Fast, lightweight reading\n\n```rust\nlet reader = Markview::open(\"notes.md\");\n```\n",
	);
	let tabs = vec![
		ReaderTab::new("Typography 排版.md".into()),
		ReaderTab::new("A very long document filename for clipping.md".into()),
	];
	let strip = TabStrip::default();
	for dark in [false, true] {
		let sheet = markview_core::style::Stylesheet::bundled(dark);
		let settings = ReaderSettings {
			stylesheet: sheet.clone(),
			theme: if dark { Theme::Dark } else { Theme::Light },
			..Default::default()
		};
		let mut ui = crate::test_support::shaper();
		ui.set_stylesheet(sheet.clone());
		renderer.set_stylesheet(sheet);
		let mut metrics = TabMetrics::default();
		metrics.sync(&mut ui, &tabs);
		let entries = crate::stylesheet::catalog(None, None);
		for (width, height) in [(500.0, 300.0), (820.0, 600.0), (1200.0, 800.0)]
		{
			let snapshot = LayoutEngine::new().layout(
				&document,
				&settings.layout_options(width, false, &fonts),
			);
			let session = ReaderSession {
				path: Some("Typography 排版.md".into()),
				snapshot,
				document: Some(std::sync::Arc::new(document.clone())),
				..Default::default()
			};
			for scale in [1.0, 1.25, 2.0] {
				for page in [
					"reader",
					"preview",
					"settings",
					"export",
					"styles",
					"empty",
					"error",
					"loading",
					"notice",
					"confirmation",
				] {
					// Full DPI coverage for the forms; one scale suffices for the other states.
					if scale != 1.25 && !matches!(page, "settings" | "export") {
						continue;
					}
					let mut interaction = InteractionState {
						panel_open: matches!(
							page,
							"settings" | "preview" | "export" | "styles"
						),
						settings_preview: page == "preview",
						export_open: page == "export",
						styles_open: page == "styles",
						focus_visible: true,
						focus: Some(if page == "export" {
							Command::ExportRun
						} else {
							Command::Larger
						}),
						..Default::default()
					};
					if page == "confirmation" {
						interaction.modal = Some(Modal::OpenLocal {
							path: "/tmp/example.desktop".into(),
							dir: "/tmp".into(),
							document_dir: None,
						});
						interaction.focus = Some(Command::ModalOpenFolder);
					}
					let empty = ReaderSession {
						path: (page != "empty").then(|| "Missing.md".into()),
						layout_pending: page == "loading",
						..Default::default()
					};
					let session =
						if matches!(page, "empty" | "error" | "loading") {
							&empty
						} else {
							&session
						};
					let export = ExportSettings {
						format: crate::settings::ExportFormat::Png,
						..Default::default()
					};
					if page == "settings" {
						let form = controls::form(
							&mut ui, &settings, 0.0, width, height,
						);
						interaction.settings_scroll =
							form.reveal(Command::Larger);
					}
					let fonts = crate::fonts::Status::default();
					let mut chrome = Chrome {
						ui: &mut ui,
						session,
						tabs: &tabs,
						active_tab: 0,
						tab_strip: &strip,
						tab_widths: &metrics.widths,
						settings: &settings,
						export: &export,
						interaction: &interaction,
						style_entries: &entries,
						style_page: 0,
						fonts: &fonts,
						width,
						height,
						scrollbar: None,
						warning: None,
						status: "File not found",
						status_until: None,
						error: page == "error",
						hover_hint: None,
						remote_notice: (page == "notice").then_some(37),
						watching: false,
					};
					let overlay = chrome.overlay();
					assert_eq!(
						overlay
							.iter()
							.filter(|draw| matches!(draw,
                        Draw::Icon { y, .. } if *y == 10.0))
							.count(),
						3,
						"toolbar must stay visible on {page}"
					);
					let horizontal = HashMap::new();
					let view = View {
						width: (width * scale) as u32,
						height: (height * scale) as u32,
						scale,
						left: 24.0,
						top: content_top(page == "notice") + 16.0,
						bottom: BOTTOM + 10.0,
						scroll: 0.0,
						theme: settings.theme,
						horizontal: &horizontal,
						selection: None,
						revision: 1,
						hovered_link: None,
						hovered_overflow: None,
						held_overflow: None,
					};
					let target = renderer.offscreen(view.width, view.height);
					let submission = renderer.render(
						&session.snapshot,
						&view,
						&overlay,
						&target.create_view(&Default::default()),
					)?;
					renderer.wait(Some(submission))?;
					renderer.save_png(
						&target,
						&output.join(format!(
							"{page}-{}-{width}-{scale}.png",
							if dark { "dark" } else { "light" }
						)),
					)?;
				}
			}
		}
	}
	Ok(())
}

#[test]
#[ignore = "requires a GPU; writes artifacts/ui-feedback/button-states-*.png"]
fn button_feedback_frames() -> Result<()> {
	use super::components::{ButtonKind, appearance, button, draw_button};
	let directory =
		PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("artifacts/ui-feedback");
	std::fs::create_dir_all(&directory)?;
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	for dark in [false, true] {
		let theme = if dark { Theme::Dark } else { Theme::Light };
		let sheet = markview_core::style::Stylesheet::bundled(dark);
		renderer.set_stylesheet(sheet.clone());
		let mut ui = crate::test_support::shaper();
		ui.set_stylesheet(sheet);
		appearance(&mut ui);
		for scale in [1.0, 1.25, 2.0] {
			let mut draws = vec![Draw::Rect(
				Rect {
					x: 0.0,
					y: 0.0,
					w: 940.0,
					h: 380.0,
				},
				Paint::Styled(Condition::Panel, C::Background),
			)];
			draws.extend(ui.label(
				"Button interaction states",
				20.0,
				24.0,
				36.0,
				Paint::Styled(Condition::Ui, C::Color),
			));
			for (column, title) in [
				"Rest / clicked",
				"Hover",
				"Pressed",
				"Keyboard focus",
				"Disabled",
			]
			.into_iter()
			.enumerate()
			{
				let x = 190.0 + column as f32 * 146.0;
				draws.extend(ui.label(
					title,
					12.0,
					x,
					75.0,
					Paint::Styled(Condition::Ui, C::Muted),
				));
				for (row, (title, kind, selected, icon)) in [
					("Regular", ButtonKind::Standard, false, false),
					("Selected", ButtonKind::Standard, true, false),
					("Primary", ButtonKind::Primary, false, false),
					("Quiet", ButtonKind::Quiet, false, false),
					("Icon", ButtonKind::Quiet, false, true),
				]
				.into_iter()
				.enumerate()
				{
					let y = 100.0 + row as f32 * 52.0;
					if column == 0 {
						draws.extend(ui.label(
							title,
							13.0,
							24.0,
							y + 21.0,
							Paint::Styled(Condition::Ui, C::Color),
						));
					}
					let mut b = button(
						if selected {
							"On"
						} else if kind == ButtonKind::Primary {
							"Export…"
						} else {
							"Open…"
						},
						Command::Open,
						Rect {
							x,
							y,
							w: if icon { 32.0 } else { 126.0 },
							h: 32.0,
						},
					);
					b.kind = kind;
					b.active = selected;
					b.enabled = column != 4;
					b.icon = icon.then_some(super::icons::OPEN);
					let interaction = InteractionState {
						cursor: if matches!(column, 1 | 2 | 4) {
							(x + 5.0, y + 5.0)
						} else {
							(0.0, 0.0)
						},
						focus: Some(b.action),
						focus_visible: column == 3,
						pressed: (column == 2).then_some(b.action),
						..Default::default()
					};
					draws.extend(draw_button(&mut ui, &interaction, &b, true));
				}
			}
			let horizontal = HashMap::new();
			let view = View {
				width: (940.0 * scale) as u32,
				height: (380.0 * scale) as u32,
				scale,
				left: 0.0,
				top: 0.0,
				bottom: 0.0,
				scroll: 0.0,
				theme,
				horizontal: &horizontal,
				selection: None,
				revision: 0,
				hovered_link: None,
				hovered_overflow: None,
				held_overflow: None,
			};
			let target = renderer.offscreen(view.width, view.height);
			let submission = renderer.render(
				&Default::default(),
				&view,
				&draws,
				&target.create_view(&Default::default()),
			)?;
			renderer.wait(Some(submission))?;
			renderer.save_png(
				&target,
				&directory.join(format!(
					"button-states-{}-{scale}.png",
					if dark { "dark" } else { "light" }
				)),
			)?;
		}
	}
	Ok(())
}

#[test]
#[ignore = "requires a GPU and system CJK fonts; writes artifacts/cjk-weight/*.png"]
fn cjk_ui_weight_comparison() -> Result<()> {
	use markview_core::style::{CjkType, Stylesheet, TextAppearance};
	let directory =
		PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("artifacts/cjk-weight");
	std::fs::create_dir_all(&directory)?;
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	for dark in [false, true] {
		let mut sheet = (*Stylesheet::bundled(dark)).clone();
		sheet.set_cjk_type(CjkType::Sc);
		let sheet = std::sync::Arc::new(sheet);
		renderer.set_stylesheet(sheet.clone());
		for scale in [1.0, 1.25, 2.0] {
			let mut draws = vec![Draw::Rect(
				Rect {
					x: 0.0,
					y: 0.0,
					w: 1200.0,
					h: 350.0,
				},
				Paint::Styled(Condition::Panel, C::Background),
			)];
			for (column, weight) in [400, 450, 500].into_iter().enumerate() {
				let mut trial = (*sheet).clone();
				trial.merge(&Stylesheet::parse(&format!(
					"format_version=2\nversion=1\n[[rule]]\nwhen=['ui']\nfont=[{{family='sans-serif'}},{{family='sans-serif[cjk]',weight={weight}}},{{family='sans-serif[cjk]'}},{{family='emoji',weight=400}}]"
				))?);
				// Use installed fonts: pinned test faces intentionally lack Medium.
				let mut ui = TextShaper::new();
				ui.set_stylesheet(std::sync::Arc::new(trial));
				ui.appearance = ui
					.stylesheet
					.text(&TextAppearance::default(), Condition::Ui);
				let x = 24.0 + column as f32 * 400.0;
				draws.extend(ui.label(
					&format!("CJK {weight} / Latin 400"),
					18.0,
					x,
					36.0,
					Paint::Styled(Condition::Ui, C::Color),
				));
				for (row, size) in [12.0, 14.0, 16.0].into_iter().enumerate() {
					let y = 85.0 + row as f32 * 85.0;
					draws.extend(ui.label(
						"阅读设置 · 导出文档 · 字体选择",
						size,
						x,
						y,
						Paint::Styled(Condition::Ui, C::Color),
					));
					draws.extend(ui.label(
						"简体中文 / 日本語 / 繁體中文",
						size,
						x,
						y + 24.0,
						Paint::Styled(Condition::Ui, C::Muted),
					));
					draws.extend(ui.label(
						"Markdown · 12345 · 保存 PDF",
						size,
						x,
						y + 48.0,
						Paint::Styled(Condition::Ui, C::Color),
					));
				}
			}
			let horizontal = HashMap::new();
			let view = View {
				width: (1200.0 * scale) as u32,
				height: (350.0 * scale) as u32,
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
			let target = renderer.offscreen(view.width, view.height);
			let submission = renderer.render(
				&Default::default(),
				&view,
				&draws,
				&target.create_view(&Default::default()),
			)?;
			renderer.wait(Some(submission))?;
			renderer.save_png(
				&target,
				&directory.join(format!(
					"{}-{scale}.png",
					if dark { "dark" } else { "light" }
				)),
			)?;
		}
	}
	Ok(())
}
