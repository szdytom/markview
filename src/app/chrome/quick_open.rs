//! A workspace file picker with a rendered document beside its results.
use super::{
	Button, Command, Draw, Lang, Paint, Rect, TextShaper, components, empty,
	list::List,
};
use crate::{
	app::{folder::Browser, quick_open::Model, widgets::Drawn},
	state::{ChromeCommand, TextField},
};
use markview_layout::style::{ColorField as C, Condition};

pub(in crate::app) struct Geometry {
	pub panel: Rect,
	pub input: Rect,
	pub list: List,
	pub footer: Rect,
	pub preview: Option<Rect>,
	pub body: Option<Rect>,
}

fn preview_header(
	ui: &mut TextShaper,
	name: &str,
	path: &str,
	width: f32,
) -> (String, String, f32) {
	let path_width = ui.text_width(path, 12.0).min(width * 0.4);
	let name = ui.fit(name, 13.0, (width - path_width - 16.0).max(0.0));
	let path_x = ui.text_width(&name, 13.0) + 16.0;
	let path = ui.fit(path, 12.0, (width - path_x).max(0.0));
	(name, path, path_x)
}

pub(in crate::app) fn geometry(
	(width, height): (f32, f32),
	rows: usize,
	scroll: f32,
) -> Geometry {
	let w = (width - 32.0).clamp(0.0, 1240.0);
	let split = w >= 800.0;
	let y = (height * 0.10).max(crate::app::TOP + 12.0).min(height);
	let h = (if split {
		720.0
	} else {
		134.0 + rows.clamp(2, 8) as f32 * 60.0
	})
	.min((height - y - 16.0).max(0.0));
	let panel = Rect {
		x: (width - w) / 2.0,
		y,
		w,
		h,
	};
	let left = if split {
		(w * 0.3).clamp(320.0, 380.0).floor()
	} else {
		w
	};
	let input = Rect {
		x: panel.x + 16.0,
		y: y + 46.0,
		w: (left - 32.0).max(0.0),
		h: 40.0,
	};
	let content_y = (y + 98.0).min(y + h);
	let footer = Rect {
		x: panel.x,
		y: (y + h - 34.0).max(content_y),
		w: left,
		h: 34.0_f32.min((h - 98.0).max(0.0)),
	};
	let viewport = Rect {
		x: panel.x + 8.0,
		y: content_y,
		w: (left - 16.0).max(0.0),
		h: (footer.y - content_y).max(0.0),
	};
	let preview = split.then_some(Rect {
		x: panel.x + left,
		y: y + 1.0,
		w: w - left,
		h: (h - 2.0).max(0.0),
	});
	let body = preview.map(|rect| Rect {
		x: rect.x + 24.0,
		y: (rect.y + 56.0).min(rect.y + rect.h),
		w: (rect.w - 48.0).max(0.0),
		h: (rect.h - 72.0).max(0.0),
	});
	Geometry {
		panel,
		input,
		list: List::new(
			Rect { w: left, ..panel },
			viewport,
			0.0,
			60.0,
			rows,
			scroll,
		),
		footer,
		preview,
		body,
	}
}

pub(super) fn buttons(
	browser: &Browser,
	picker: &Model,
	size: (f32, f32),
	lang: Lang,
) -> Vec<Button> {
	let g = geometry(size, picker.search.matches.len(), picker.scroll());
	if !browser.file_search_available() {
		return g.list.hit(vec![components::button(
			lang.empty_open_folder(),
			Command::Chrome(ChromeCommand::OpenFolder),
			Rect {
				x: g.list.viewport.x + 16.0,
				y: g.list.viewport.y + 44.0,
				w: (g.list.viewport.w - 32.0).max(0.0),
				h: 40.0,
			},
		)]);
	}
	let mut buttons = vec![components::button(
		lang.empty_search_placeholder(),
		Command::Chrome(ChromeCommand::FocusInput(TextField::QuickOpen)),
		g.input,
	)];
	if picker.search.pending {
		return buttons;
	}
	let overflows = g.list.max_scroll() > 0.0;
	buttons.extend(
		g.list.hit(
			g.list
				.visible()
				.map(|index| {
					let file = &picker.search.matches[index];
					let mut rect = g.list.row_rect(index);
					if overflows {
						rect.w = (rect.w - 16.0).max(0.0);
					}
					let mut button = components::button(
						file.name.clone(),
						Command::Chrome(ChromeCommand::QuickOpenFile(file.id)),
						rect,
					);
					button.kind = components::ButtonKind::Quiet;
					button.enabled = !picker.search.pending;
					button.active = index == picker.selected();
					button
				})
				.collect(),
		),
	);
	buttons
}

pub(super) fn draw(
	ui: &mut TextShaper,
	interaction: &Drawn<'_>,
	picker: &Model,
	browser: &Browser,
	size: (f32, f32),
	lang: Lang,
) -> Vec<Draw> {
	let g = geometry(size, picker.search.matches.len(), picker.scroll());
	ui.appearance = super::ui_appearance(ui);
	let mut out = components::frame(g.panel, size.0, size.1);
	let title = ui.fit(lang.empty_quick_access(), 16.0, g.input.w);
	out.extend(ui.label(
		&title,
		16.0,
		g.panel.x + 16.0,
		g.panel.y + 28.0,
		Paint::Styled(Condition::Panel, C::Color),
	));
	out.push(Draw::Rect(
		Rect { h: 1.0, ..g.footer },
		Paint::Styled(Condition::Panel, C::BorderColor),
	));
	let hint =
		ui.fit(lang.quick_open_hint(), 11.0, (g.footer.w - 64.0).max(0.0));
	for (index, paths) in [super::icons::UP, super::icons::DOWN]
		.into_iter()
		.enumerate()
	{
		out.push(Draw::Icon {
			paths,
			paint: Paint::Styled(Condition::Panel, C::Muted),
			x: g.panel.x + 16.0 + index as f32 * 14.0,
			y: g.panel.y + g.panel.h - 23.0,
			size: 12.0,
		});
	}
	out.extend(ui.label(
		&hint,
		11.0,
		g.panel.x + 48.0,
		g.panel.y + g.panel.h - 12.0,
		Paint::Styled(Condition::Panel, C::Muted),
	));
	let mut rows = Vec::new();
	for button in buttons(browser, picker, size, lang) {
		if matches!(
			button.action,
			Command::Chrome(ChromeCommand::QuickOpenFile(_))
		) {
			empty::draw_row(
				ui,
				&mut rows,
				interaction,
				&button,
				empty::View {
					rect: g.panel,
					phone: false,
					browser: Some(browser),
				},
				&picker.search,
				lang,
			);
		} else if button.action == Command::Chrome(ChromeCommand::OpenFolder) {
			rows.extend(components::draw_button(
				ui,
				interaction,
				&button,
				true,
			));
		}
	}
	let message = if !browser.file_search_available() {
		Some(lang.quick_open_no_workspace())
	} else if picker.search.pending {
		Some(lang.empty_searching())
	} else if picker.search.matches.is_empty() {
		Some(lang.empty_no_files())
	} else {
		None
	};
	if let Some(message) = message {
		let message =
			ui.fit(message, 13.0, (g.list.viewport.w - 32.0).max(0.0));
		rows.extend(ui.label(
			&message,
			13.0,
			g.list.viewport.x + 16.0,
			g.list.viewport.y + 28.0,
			Paint::Styled(Condition::Panel, C::Muted),
		));
	}
	out.push(g.list.clip(rows));
	if !picker.search.pending {
		g.list.draw_bar(&mut out, ui, interaction);
	}
	if let Some(preview) = g.preview {
		out.push(Draw::Rect(
			Rect {
				w: (preview.w - 1.0).max(0.0),
				..preview
			},
			Paint::Styled(Condition::Preview, C::Background),
		));
		out.push(Draw::Rect(
			Rect { w: 1.0, ..preview },
			Paint::Styled(Condition::Panel, C::BorderColor),
		));
		out.push(Draw::Rect(
			Rect {
				x: preview.x + 24.0,
				y: preview.y + 46.0,
				w: (preview.w - 48.0).max(0.0),
				h: 1.0,
			},
			Paint::Styled(Condition::Panel, C::BorderColor),
		));
		if let Some(file) = picker.file() {
			let (name, path, path_x) = preview_header(
				ui,
				&file.name,
				if file.parent.is_empty() {
					lang.empty_workspace_root()
				} else {
					&file.parent
				},
				(preview.w - 48.0).max(0.0),
			);
			out.extend(ui.label(
				&name,
				13.0,
				preview.x + 24.0,
				preview.y + 28.0,
				Paint::Styled(Condition::Panel, C::Color),
			));
			out.extend(ui.label(
				&path,
				12.0,
				preview.x + 24.0 + path_x,
				preview.y + 28.0,
				Paint::Styled(Condition::Panel, C::Muted),
			));
		}
		if let Some(body) = g.body {
			let message =
				picker.file().and_then(|_| picker.preview.message(lang));
			if let Some(message) = message {
				let message = ui.fit(message, 13.0, body.w);
				out.extend(ui.label(
					&message,
					13.0,
					body.x,
					body.y + 20.0,
					Paint::Styled(Condition::Panel, C::Muted),
				));
			}
		}
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{
		app::file_search,
		render::{Renderer, Theme, View},
		state::InteractionState,
	};
	use nucleo_matcher::{Config, Matcher};
	use proptest::prelude::*;

	#[test]
	fn preview_header_keeps_names_and_paths_on_one_line_without_overlap() {
		let mut ui = crate::test_support::shaper();
		for width in [160.0, 300.0, 432.0, 770.0] {
			for (name, path) in [
				("reading.md", "docs"),
				(
					"a-very-long-document-name-for-clipping.md",
					"notes/research/a/deeply/nested/directory",
				),
				("阅读笔记.md", "docs/阅读"),
			] {
				let (name, path, path_x) =
					preview_header(&mut ui, name, path, width);
				assert!(ui.text_width(&name, 13.0) + 15.9 <= path_x);
				assert!(path_x + ui.text_width(&path, 12.0) <= width + 0.01);
			}
		}
		let (name, path, _) =
			preview_header(&mut ui, "reading.md", "docs", 432.0);
		assert_eq!(name, "reading.md");
		assert_eq!(path, "docs");
	}

	proptest! {
		#[test]
		fn picker_regions_stay_separate(width in 320.0f32..2600.0, height in 240.0f32..1800.0, rows in 0usize..100, scroll in 0.0f32..10000.0) {
			let g = geometry((width, height), rows, scroll);
			prop_assert!(g.panel.x >= 0.0 && g.panel.y + g.panel.h <= height);
			prop_assert!(g.list.viewport.y + g.list.viewport.h <= g.footer.y + 0.001);
			if let Some(preview) = g.preview {
				prop_assert!(g.input.x + g.input.w <= preview.x);
				prop_assert!(g.footer.x + g.footer.w <= preview.x);
				prop_assert!((preview.y + preview.h - (g.panel.y + g.panel.h - 1.0)).abs() < 0.001);
				let body = g.body.unwrap();
				prop_assert!(body.x >= preview.x && body.x + body.w <= preview.x + preview.w);
				prop_assert!(body.y + body.h <= preview.y + preview.h);
			} else {
				prop_assert!(g.body.is_none());
			}
		}
	}

	#[test]
	fn quick_open_frames() -> anyhow::Result<()> {
		let dir = crate::test_support::TestDir::new();
		std::fs::create_dir(dir.path().join("docs"))?;
		for name in ["README.md", "docs/reading.md", "docs/阅读.md"] {
			std::fs::write(dir.path().join(name), "# Reading")?;
		}
		let mut browser = Browser::default();
		browser.tree = Some(crate::app::folder::Tree::read(dir.path().into())?);
		let files: Vec<_> = browser
			.tree
			.as_ref()
			.unwrap()
			.entries
			.iter()
			.filter_map(|entry| entry.file.clone())
			.collect();
		let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
		let mut picker = Model::default();
		(picker.search.matches, picker.search.snippets) =
			file_search::find(&mut matcher, &files, "read", || false).unwrap();
		let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("artifacts/quick-open");
		std::fs::create_dir_all(&output)?;
		let mut renderer = pollster::block_on(Renderer::new(None))?;
		for (lang, dark, width, height, background) in [
			(Lang::En, false, 1200, 800, None),
			(Lang::ZhHans, true, 1200, 800, None),
			(Lang::En, true, 480, 320, None),
			(Lang::ZhHans, false, 360, 480, None),
			(Lang::En, false, 1200, 800, Some("#78a0c080")),
			(Lang::ZhHans, true, 1200, 800, Some("#78a0c080")),
		] {
			let mut ui = crate::test_support::shaper();
			let mut sheet =
				(*markview_layout::style::Stylesheet::bundled(dark)).clone();
			if let Some(background) = background {
				sheet.merge(&markview_layout::style::Stylesheet::parse(
					&format!(
						"format_version=3\nversion=1\n[[rule]]\nwhen=['preview']\nbackground='{background}'"
					),
				)?);
			}
			let sheet = std::sync::Arc::new(sheet);
			ui.set_stylesheet(sheet.clone());
			renderer.set_stylesheet(sheet);
			let interaction = InteractionState::default();
			let drawn = Drawn::new(&interaction, None);
			let size = (width as f32, height as f32);
			let g = geometry(size, picker.search.matches.len(), 0.0);
			picker.preview.complete(g.body.map(|body| {
				let document = markview_commonmark::parse("# Reading notes\n\nA quiet place for **important ideas** and clear typography.\n\n## Keep reading\n\n> Let the page stay quiet.\n\n```rust\nfn reading_time(words: usize) -> usize {\n    words.div_ceil(240)\n}\n```\n\nFollow the ideas at your own pace.");
				let mut options = crate::test_support::options();
				options.width = body.w;
				options.preview = true;
				options.stylesheet = ui.stylesheet.clone();
				crate::layout::LayoutEngine::new()
					.layout(&document.document, &options)
			}));
			picker.search.input.set_text(&mut ui, "read");
			let mut overlay =
				draw(&mut ui, &drawn, &picker, &browser, size, lang);
			let g = geometry(size, picker.search.matches.len(), 0.0);
			overlay.extend(picker.search.input.draw(
				&mut ui,
				g.input,
				true,
				true,
				lang.empty_search_placeholder(),
			));
			let view = View {
				width,
				height,
				scale: 1.0,
				left: 0.0,
				top: 0.0,
				bottom: 0.0,
				scroll: 0.0,
				theme: if dark { Theme::Dark } else { Theme::Light },
				horizontal: &Default::default(),
				selection: None,
				revision: 0,
				hovered_link: None,
				held_overflow: None,
			};
			let scene = g
				.body
				.and_then(|body| picker.preview.scene(body, view.theme));
			let image = crate::test_support::render_goldens::capture_scenes(
				&mut renderer,
				&Default::default(),
				&view,
				&overlay,
				scene.as_slice(),
			)?;
			if let Some(preview) = g.preview {
				let body = g.body.unwrap();
				let pixel =
					|x: f32, y: f32| *image.get_pixel(x as u32, y as u32);
				let background = pixel(preview.x + 8.0, preview.y + 8.0);
				assert_eq!(
					pixel(body.x + body.w - 4.0, body.y + 4.0),
					background
				);
				assert_eq!(
					pixel(body.x + body.w - 4.0, body.y + body.h - 4.0),
					background
				);
				assert_eq!(
					pixel(preview.x + 8.0, preview.y + preview.h - 8.0),
					background
				);
			}
			let suffix = if background.is_some() {
				"-translucent"
			} else {
				""
			};
			image.save(output.join(format!(
				"{lang:?}-{dark}-{width}-{height}{suffix}.png"
			)))?;
			assert!(g.panel.x >= 0.0 && g.panel.y + g.panel.h <= height as f32);
			assert!(
				buttons(&browser, &picker, size, lang)
					.iter()
					.all(|button| button.widget().hit_rect().is_some())
			);
			assert!(geometry(size, 20, 0.0).list.scrollbar(&ui).is_some());
			assert!(geometry(size, 1, 0.0).list.scrollbar(&ui).is_none());
		}
		Ok(())
	}
}
