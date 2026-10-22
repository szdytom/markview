use super::*;
use crate::test_support::render_goldens::{Baselines, capture};
use crate::{
	render::{Renderer, Theme, View},
	state::{PanelPage, PanelTab},
};
use anyhow::{Result, ensure};
use markview_layout::style::{CjkType, Stylesheet};
use std::{collections::HashMap, fs, path::Path, sync::Arc};

#[test]
fn ui_styles_match_rendering_baselines() -> Result<()> {
	if !markview_platform::test_support::desktop_goldens() {
		return Ok(());
	}
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	let mut baselines = Baselines::new(
		root.join("tests/goldens/ui"),
		root.join("artifacts/ui-goldens"),
	)?;
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	ensure!(
		renderer.adapter_name.starts_with("llvmpipe")
			&& renderer.adapter_name.ends_with("(Vulkan, Cpu)"),
		"UI baselines require Lavapipe"
	);
	for theme in Stylesheet::READER_THEMES
		.iter()
		.copied()
		.chain(std::iter::once("custom"))
	{
		let mut sheet = (*Stylesheet::builtin()).clone();
		sheet.merge(
			&Stylesheet::named_rules(if theme == "custom" {
				"light"
			} else {
				theme
			})
			.unwrap(),
		);
		if theme == "custom" {
			sheet.merge(&Stylesheet::parse(&fs::read_to_string(
				root.join("tests/fixtures/ui.mvss.toml"),
			)?)?);
		}
		sheet.set_cjk_type(CjkType::Sc);
		let sheet = Arc::new(sheet);
		renderer.set_stylesheet(sheet.clone());
		for (lang, scale) in [(Lang::En, 1.), (Lang::ZhHans, 1.25)] {
			let name = format!(
				"{theme}-{}",
				if lang == Lang::En { "en" } else { "zh" }
			);
			let settings = ReaderSettings {
				lang: Some(lang),
				stylesheet: sheet.clone(),
				..Default::default()
			};
			let interaction = InteractionState {
				panel: PanelPage::Settings(PanelTab::Generic),
				pointer: crate::app::pointer::State::at((-1., -1.)),
				..Default::default()
			};
			let mut ui = crate::test_support::shaper();
			ui.set_stylesheet(sheet.clone());
			let mut draws = draw_controls(
				&mut ui,
				&crate::app::widgets::Drawn::new(&interaction, None),
				&crate::app::settings_panel::Model::new(settings.clone()),
				(800., 1000.),
				None,
				false,
				true,
			);
			draws.extend(draw_footer(
				&mut ui,
				(None, None),
				None,
				"reading.md",
				(800., 1000.),
				0.,
				lang,
			));
			for (i, state) in [
				"normal", "hover", "pressed", "selected", "disabled", "focus",
			]
			.iter()
			.enumerate()
			{
				let rect = Rect {
					x: 20. + i as f32 * 126.,
					y: 900.,
					w: 116.,
					h: 36.,
				};
				let mut b = components::button(*state, Command::Settings, rect);
				b.active = *state == "selected";
				b.enabled = *state != "disabled";
				let state = InteractionState {
					widgets: markview_ui_core::Interaction {
						captured: (*state == "pressed").then_some(b.id),
						focus: (*state == "focus").then_some(b.id),
						focus_visible: *state == "focus",
						..Default::default()
					},
					pointer: crate::app::pointer::State::at(
						if matches!(*state, "hover" | "pressed") {
							(rect.x + 5., rect.y + 5.)
						} else {
							(-1., -1.)
						},
					),
					..Default::default()
				};
				draws.extend(components::draw_button(
					&mut ui,
					&crate::app::widgets::Drawn::new(&state, None),
					&b,
					true,
				));
			}
			draws.extend(ui.label(
				"Warning 提示",
				14.,
				20.,
				964.,
				Paint::Styled(Condition::Ui, C::Warning),
			));
			for (i, expanded) in [false, true].into_iter().enumerate() {
				let bar = Scrollbar::vertical(
					Rect {
						x: 762. + i as f32 * 18.,
						y: 20.,
						w: 14.,
						h: 150.,
					},
					40.,
					500.,
					150.,
					sheet.scrollbar_metrics(),
				)
				.unwrap();
				let (track, thumb) = bar.bars(expanded);
				draws.push(Draw::Rect(
					track,
					Paint::Styled(Condition::Scrollbar, C::Track),
				));
				draws.push(Draw::Rect(
					thumb,
					Paint::Styled(
						Condition::Scrollbar,
						if expanded { C::ThumbHover } else { C::Thumb },
					),
				));
			}
			for (i, paint) in
				[Paint::Shadow, Paint::Scrim].into_iter().enumerate()
			{
				draws.push(Draw::Rect(
					Rect {
						x: 520. + i as f32 * 110.,
						y: 954.,
						w: 90.,
						h: 20.,
					},
					paint,
				));
			}
			draws.extend(ui.label(
				"Error 错误",
				14.,
				250.,
				964.,
				Paint::Styled(Condition::Ui, C::Error),
			));
			let horizontal = HashMap::new();
			let view = View {
				width: (800. * scale) as u32,
				height: (1000. * scale) as u32,
				scale,
				left: 0.,
				top: 0.,
				bottom: 0.,
				scroll: 0.,
				theme: Theme::Light,
				horizontal: &horizontal,
				selection: None,
				revision: 1,
				hovered_link: None,
				hovered_overflow: None,
				held_overflow: None,
			};
			let actual =
				capture(&mut renderer, &Default::default(), &view, &draws)?;
			baselines.record(&name, &actual)?;
		}
	}
	baselines.finish()
}

#[test]
fn phone_sidebar_match_rendering_baselines() -> Result<()> {
	use crate::{
		app::folder::{Browser, Tree},
		state::{ReaderTab, SidebarTab},
	};
	if !markview_platform::test_support::desktop_goldens() {
		return Ok(());
	}
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	let mut baselines = Baselines::new(
		root.join("tests/goldens/phone-sidebar"),
		root.join("artifacts/phone-sidebar-goldens"),
	)?;
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	ensure!(
		renderer.adapter_name.starts_with("llvmpipe")
			&& renderer.adapter_name.ends_with("(Vulkan, Cpu)"),
		"Phone sidebar baselines require Lavapipe"
	);
	let dir = tempfile::tempdir()?;
	let folder = dir.path().join("Research notes");
	fs::create_dir_all(folder.join("Projects"))?;
	for i in 0..24 {
		fs::write(folder.join(format!("chapter-{i:02}.md")), "# Note")?;
	}
	let mut browser = Browser::default();
	browser.tree = Some(Tree::read(folder.clone())?);
	let tabs: Vec<_> = (0..20)
		.map(|i| ReaderTab::new(format!("notes/chapter-{i:02}.md").into()))
		.collect();
	let horizontal = HashMap::new();
	for dark in [false, true] {
		let mut sheet = (*Stylesheet::bundled(dark)).clone();
		sheet.set_cjk_type(CjkType::Sc);
		let sheet = Arc::new(sheet);
		for lang in [Lang::En, Lang::ZhHans] {
			let settings = ReaderSettings {
				lang: Some(lang),
				stylesheet: sheet.clone(),
				..Default::default()
			};
			renderer.set_stylesheet(settings.stylesheet.clone());
			let mut ui = crate::test_support::shaper();
			ui.set_stylesheet(settings.stylesheet.clone());
			for (page, tab) in [
				("tabs", SidebarTab::Tabs),
				("folders", SidebarTab::Folder),
				("empty", SidebarTab::Folder),
			] {
				let interaction = InteractionState {
					panel: PanelPage::Tabs,
					sidebar_tab: tab,
					pointer: crate::app::pointer::State::at((-1., -1.)),
					..Default::default()
				};
				let draws = if tab == SidebarTab::Tabs {
					tab_drawer::draw(
						&mut ui,
						&tabs,
						8,
						20.,
						&crate::app::widgets::Drawn::new(&interaction, None),
						(360., 760.),
						&settings,
					)
				} else {
					tab_drawer::draw_folder(
						&mut ui,
						(page != "empty").then_some(&browser),
						Some(&folder.join("chapter-02.md")),
						&crate::app::widgets::Drawn::new(&interaction, None),
						(360., 760.),
						&settings,
					)
				};
				let view = View {
					width: 360,
					height: 760,
					scale: 1.,
					left: 0.,
					top: 0.,
					bottom: 0.,
					scroll: 0.,
					theme: if dark { Theme::Dark } else { Theme::Light },
					horizontal: &horizontal,
					selection: None,
					revision: 1,
					hovered_link: None,
					hovered_overflow: None,
					held_overflow: None,
				};
				let actual =
					capture(&mut renderer, &Default::default(), &view, &draws)?;
				baselines.record(
					&format!(
						"{page}-{}-{}",
						if dark { "dark" } else { "light" },
						if lang == Lang::En { "en" } else { "zh" }
					),
					&actual,
				)?;
			}
		}
	}
	baselines.finish()
}
