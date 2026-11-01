//! A compact workspace file list above the reading surface.
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
}

pub(in crate::app) fn geometry(
	(width, height): (f32, f32),
	rows: usize,
	scroll: f32,
) -> Geometry {
	let w = (width - 32.0).clamp(0.0, 680.0);
	let y = (height * 0.12).max(crate::app::TOP + 12.0).min(height);
	let h = (134.0 + rows.clamp(2, 8) as f32 * 60.0)
		.min((height - y - 16.0).max(0.0));
	let panel = Rect {
		x: (width - w) / 2.0,
		y,
		w,
		h,
	};
	let input = Rect {
		x: panel.x + 16.0,
		y: y + 46.0,
		w: (w - 32.0).max(0.0),
		h: 40.0,
	};
	let viewport = Rect {
		x: panel.x + 8.0,
		y: y + 98.0,
		w: (w - 16.0).max(0.0),
		h: (h - 132.0).max(0.0),
	};
	Geometry {
		panel,
		input,
		list: List::new(panel, viewport, 0.0, 60.0, rows, scroll),
	}
}

pub(super) fn buttons(
	browser: &Browser,
	picker: &Model,
	size: (f32, f32),
	lang: Lang,
) -> Vec<Button> {
	let g = geometry(size, browser.start.matches.len(), picker.scroll());
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
		Command::Chrome(ChromeCommand::FocusInput(TextField::Files)),
		g.input,
	)];
	if browser.start.pending {
		return buttons;
	}
	buttons.extend(
		g.list.hit(
			g.list
				.visible()
				.map(|index| {
					let file = &browser.start.matches[index];
					let mut button = components::button(
						file.name.clone(),
						Command::Chrome(ChromeCommand::StartFile(file.id)),
						g.list.row_rect(index),
					);
					button.kind = components::ButtonKind::Quiet;
					button.enabled = !browser.start.pending;
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
	browser: &Browser,
	size: (f32, f32),
	lang: Lang,
) -> Vec<Draw> {
	let picker = &interaction.overlays.quick_open;
	let g = geometry(size, browser.start.matches.len(), picker.scroll());
	ui.appearance = super::ui_appearance(ui);
	let mut out = components::frame(g.panel, size.0, size.1);
	let title =
		ui.fit(lang.empty_quick_access(), 16.0, (g.panel.w - 32.0).max(0.0));
	out.extend(ui.label(
		&title,
		16.0,
		g.panel.x + 16.0,
		g.panel.y + 28.0,
		Paint::Styled(Condition::Panel, C::Color),
	));
	let hint =
		ui.fit(lang.quick_open_hint(), 11.0, (g.panel.w - 64.0).max(0.0));
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
		if matches!(button.action, Command::Chrome(ChromeCommand::StartFile(_)))
		{
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
	} else if browser.start.pending {
		Some(lang.empty_searching())
	} else if browser.start.matches.is_empty() {
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

	#[test]
	fn quick_open_frames() -> anyhow::Result<()> {
		let dir = tempfile::tempdir()?;
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
		(browser.start.matches, browser.start.snippets) =
			file_search::find(&mut matcher, &files, "read", || false).unwrap();
		let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("artifacts/quick-open");
		std::fs::create_dir_all(&output)?;
		let mut renderer = pollster::block_on(Renderer::new(None))?;
		for (lang, dark, width, height) in [
			(Lang::En, false, 1200, 800),
			(Lang::ZhHans, true, 1200, 800),
			(Lang::En, true, 480, 320),
			(Lang::ZhHans, false, 360, 480),
		] {
			let mut ui = crate::test_support::shaper();
			let sheet = markview_layout::style::Stylesheet::bundled(dark);
			ui.set_stylesheet(sheet.clone());
			renderer.set_stylesheet(sheet);
			let interaction = InteractionState::default();
			let drawn = Drawn::new(&interaction, None);
			let size = (width as f32, height as f32);
			let mut overlay = draw(&mut ui, &drawn, &browser, size, lang);
			let g = geometry(size, browser.start.matches.len(), 0.0);
			browser.start.input.set_text(&mut ui, "read");
			overlay.extend(browser.start.input.draw(
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
				hovered_overflow: None,
				held_overflow: None,
			};
			let image = crate::test_support::render_goldens::capture(
				&mut renderer,
				&Default::default(),
				&view,
				&overlay,
			)?;
			image.save(
				output.join(format!("{lang:?}-{dark}-{width}-{height}.png")),
			)?;
			assert!(g.panel.x >= 0.0 && g.panel.y + g.panel.h <= height as f32);
			assert!(
				buttons(&browser, &interaction.overlays.quick_open, size, lang)
					.iter()
					.all(|button| button.widget().hit_rect().is_some())
			);
		}
		Ok(())
	}
}
