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
		let snapshot = LayoutEngine::new()
			.layout(&document, &settings.layout_options(width, false));
		let interaction = InteractionState {
			panel_open,
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
			&mut TextShaper::new(),
			Some(counts),
			Some(counts),
			None,
			"",
			width,
			height,
		));
		overlay.extend(draw_controls(
			&mut TextShaper::new(),
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
			});
			let overlay = draw_styles(
				&mut TextShaper::new(),
				&settings,
				&interaction,
				&entries,
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
			&settings.layout_options(width, false),
			&Default::default(),
		);
		let mut ui = TextShaper::new();
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
		overlay.extend(draw_banner(&mut ui, width, 37));
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
		let mut ui = TextShaper::new();
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
