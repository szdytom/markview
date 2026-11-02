//! The responsive start page shares its geometry with input and scrolling.
use super::{
	Button, Command, Draw, Lang, Paint, Rect, TextShaper, components, icons,
	ui_appearance,
};
use crate::{
	app::{folder::Browser, start},
	state::{ChromeCommand, DocumentCommand, TextField},
};
use markview_layout::style::{ColorField as C, Condition};

#[derive(Clone, Copy)]
pub(in crate::app) struct View<'a> {
	pub rect: Rect,
	pub phone: bool,
	pub browser: Option<&'a Browser>,
}
impl View<'_> {
	fn workspace(self) -> bool {
		self.browser
			.is_some_and(|browser| browser.file_search_available())
	}
	fn rows(self) -> usize {
		self.browser.map_or(0, |browser| {
			if self.workspace() {
				browser.start.matches.len()
			} else {
				browser.start.recent.len()
			}
		})
	}
}

pub(in crate::app) struct Geometry {
	pub clip: Rect,
	pub columns: bool,
	pub local: Rect,
	pub quick: Rect,
	pub web: Rect,
	pub search: Rect,
	pub url: Rect,
	pub url_open: Rect,
	pub max_scroll: f32,
	pub scroll: f32,
	rows_y: f32,
}
pub(in crate::app) fn geometry(view: View<'_>) -> Geometry {
	let columns = view.rect.w >= 760.0 && !view.phone;
	let w = (view.rect.w - 48.0).clamp(0.0, 900.0);
	let local_w = if columns { (w - 64.0) * 0.43 } else { w };
	let quick_w = if columns { w - local_w - 64.0 } else { w };
	let header = if view.workspace() { 122.0 } else { 52.0 };
	let quick_h = if view.workspace() || view.rows() > 0 {
		header + view.rows().max(1) as f32 * 60.0
	} else {
		0.0
	};
	let content_h = if columns {
		320.0_f32.max(quick_h)
	} else {
		180.0 + quick_h + 112.0
	};
	let max_scroll = (content_h + 48.0 - view.rect.h).max(0.0);
	let scroll = view
		.browser
		.map_or(0.0, |browser| browser.start.scroll)
		.clamp(0.0, max_scroll);
	let x = view.rect.x + (view.rect.w - w) / 2.0;
	let y = view.rect.y
		+ if columns {
			((view.rect.h - content_h) / 2.0).max(24.0)
		} else {
			24.0
		} - scroll;
	let quick = Rect {
		x: if columns { x + local_w + 64.0 } else { x },
		y: if columns { y } else { y + 180.0 },
		w: quick_w,
		h: quick_h,
	};
	let web = Rect {
		x,
		y: if columns {
			y + 236.0
		} else {
			quick.y + quick.h + 32.0
		},
		w: local_w,
		h: 72.0,
	};
	let web_w = 64.0_f32.min(web.w / 3.0);
	Geometry {
		clip: view.rect,
		columns,
		local: Rect {
			x,
			y,
			w: local_w,
			h: 204.0,
		},
		web,
		url: Rect {
			x: web.x,
			y: web.y + 28.0,
			w: (web.w - web_w - 8.0).max(0.0),
			h: 40.0,
		},
		url_open: Rect {
			x: web.x + web.w - web_w,
			y: web.y + 28.0,
			w: web_w,
			h: 40.0,
		},
		search: Rect {
			x: quick.x,
			y: quick.y + 66.0,
			w: quick.w,
			h: 40.0,
		},
		rows_y: quick.y + header,
		quick,
		max_scroll,
		scroll,
	}
}

pub(super) fn buttons(
	_ui: &mut TextShaper,
	view: View<'_>,
	lang: Lang,
) -> Vec<Button> {
	let g = geometry(view);
	let mut out = Vec::new();
	for (index, label, action) in [
		(
			1,
			lang.empty_open_file(),
			Command::Document(DocumentCommand::Open),
		),
		(
			2,
			lang.empty_open_folder(),
			Command::Chrome(ChromeCommand::OpenFolder),
		),
	] {
		let rect = if g.columns {
			Rect {
				x: g.local.x,
				y: g.local.y + 88.0 + (index - 1) as f32 * 56.0,
				w: g.local.w,
				h: 48.0,
			}
		} else {
			let w = (g.local.w - 12.0).max(0.0) / 2.0;
			Rect {
				x: g.local.x + (index - 1) as f32 * (w + 12.0),
				y: g.local.y + 88.0,
				w,
				h: 48.0,
			}
		};
		let mut b = components::button(label, action, rect);
		b.id = markview_ui_core::WidgetId::new(2, index);
		b.kind = if g.columns {
			components::ButtonKind::Quiet
		} else {
			components::ButtonKind::Standard
		};
		out.push(b);
	}
	out.extend([
		components::button(
			lang.empty_url_placeholder(),
			Command::Chrome(ChromeCommand::FocusInput(TextField::Url)),
			g.url,
		),
		components::button(
			lang.empty_open_url(),
			Command::Document(DocumentCommand::OpenUrl),
			g.url_open,
		),
	]);
	if view.workspace() {
		out.push(components::button(
			lang.empty_search_placeholder(),
			Command::Chrome(ChromeCommand::FocusInput(TextField::Files)),
			g.search,
		));
	}
	if let Some(browser) = view.browser {
		for index in 0..view.rows() {
			let (label, action) = if view.workspace() {
				let file = &browser.start.matches[index];
				(file.name.clone(), ChromeCommand::StartFile(file.id))
			} else {
				let recent = &browser.start.recent[index];
				(
					recent.name.as_str().into(),
					ChromeCommand::RecentWorkspace(index),
				)
			};
			let mut b = components::button(
				label,
				Command::Chrome(action),
				Rect {
					x: g.quick.x,
					y: g.rows_y + index as f32 * 60.0,
					w: g.quick.w,
					h: 60.0,
				},
			);
			b.kind = components::ButtonKind::Quiet;
			b.enabled = !view.workspace() || !browser.start.pending;
			out.push(b);
		}
	}
	out.into_iter()
		.filter_map(|mut b| {
			b.rect.intersect(g.clip)?;
			b.clip = Some(g.clip);
			Some(b)
		})
		.collect()
}

fn label(
	ui: &mut TextShaper,
	out: &mut Vec<Draw>,
	text: &str,
	size: f32,
	at: (f32, f32, f32),
	color: C,
) {
	let (x, y, width) = at;
	let fitted = ui.fit(text, size, width.max(0.0));
	let appearance = ui.appearance.clone();
	out.extend(
		ui.label_with(
			&fitted,
			size,
			x,
			y,
			&appearance,
			Paint::Styled(Condition::Ui, color),
			None,
		)
		.0,
	);
}
fn rule(out: &mut Vec<Draw>, x: f32, y: f32, w: f32) {
	out.push(components::line(
		Rect { x, y, w, h: 1.0 },
		Condition::Panel,
		C::BorderColor,
	));
}
fn background(
	ui: &mut TextShaper,
	out: &mut Vec<Draw>,
	interaction: &crate::app::widgets::Drawn<'_>,
	b: &Button,
) {
	let mut blank = b.clone();
	blank.label = "".into();
	out.extend(components::draw_button(ui, interaction, &blank, true));
}

pub(super) fn draw(
	ui: &mut TextShaper,
	interaction: &crate::app::widgets::Drawn<'_>,
	view: View<'_>,
	lang: Lang,
) -> Vec<Draw> {
	let g = geometry(view);
	ui.appearance = ui_appearance(ui);
	let mut out = Vec::new();
	let mut heading = ui.appearance.clone();
	heading.font = ui
		.stylesheet
		.text(&Default::default(), Condition::Body)
		.font;
	heading.weight = 600;
	ui.appearance = heading.clone();
	label(
		ui,
		&mut out,
		lang.empty_open_title(),
		28.0,
		(g.local.x, g.local.y + 28.0, g.local.w),
		C::Color,
	);
	ui.appearance = ui_appearance(ui);
	let hint = lang.empty_paste_hint(if view.phone {
		""
	} else if markview_platform::command_key() {
		"⌘V"
	} else {
		"Ctrl+V"
	});
	label(
		ui,
		&mut out,
		hint.trim(),
		13.0,
		(g.local.x, g.local.y + 60.0, g.local.w),
		C::Muted,
	);
	if g.columns {
		out.push(components::line(
			Rect {
				x: g.quick.x - 32.0,
				y: g.local.y,
				w: 1.0,
				h: g.local.h.max(g.quick.h).max(308.0),
			},
			Condition::Panel,
			C::BorderColor,
		));
	}
	if g.quick.h > 0.0 {
		ui.appearance = heading;
		label(
			ui,
			&mut out,
			if view.workspace() {
				lang.empty_quick_access()
			} else {
				lang.empty_recent_workspaces()
			},
			if g.columns { 26.0 } else { 22.0 },
			(g.quick.x, g.quick.y + 28.0, g.quick.w),
			C::Color,
		);
		ui.appearance = ui_appearance(ui);
		draw_workspace(ui, &mut out, view, &g, lang);
	}
	rule(&mut out, g.web.x, g.web.y - 24.0, g.web.w);
	label(
		ui,
		&mut out,
		lang.empty_web_title(),
		14.0,
		(g.web.x, g.web.y + 12.0, g.web.w),
		C::Color,
	);
	let web_title_w = ui.text_width(lang.empty_web_title(), 14.0);
	label(
		ui,
		&mut out,
		lang.empty_web_experimental(),
		11.0,
		(
			g.web.x + web_title_w + 12.0,
			g.web.y + 12.0,
			g.web.w - web_title_w - 12.0,
		),
		C::Muted,
	);
	for b in buttons(ui, view, lang) {
		match b.action {
			Command::Chrome(ChromeCommand::FocusInput(_)) => {}
			Command::Document(DocumentCommand::Open)
			| Command::Chrome(ChromeCommand::OpenFolder) => {
				draw_open(ui, &mut out, interaction, &b, g.columns)
			}
			Command::Chrome(
				ChromeCommand::StartFile(_) | ChromeCommand::RecentWorkspace(_),
			) => draw_row(
				ui,
				&mut out,
				interaction,
				&b,
				view,
				&view.browser.unwrap().start.search,
				lang,
			),
			_ => out.extend(components::draw_button(ui, interaction, &b, true)),
		}
	}
	vec![Draw::Clipped {
		rect: g.clip,
		draws: out,
	}]
}

fn draw_workspace(
	ui: &mut TextShaper,
	out: &mut Vec<Draw>,
	view: View<'_>,
	g: &Geometry,
	lang: Lang,
) {
	if let Some(browser) = view.browser
		&& browser.file_search_available()
	{
		let name = browser
			.tree
			.as_ref()
			.map_or(lang.quick_open_tabs(), |tree| tree.name.as_str());
		label(
			ui,
			out,
			name,
			12.0,
			(g.quick.x, g.quick.y + 52.0, g.quick.w),
			C::Muted,
		);
		if browser.start.matches.is_empty() {
			let text = if browser.start.pending {
				lang.empty_searching()
			} else {
				lang.empty_no_files()
			};
			label(
				ui,
				out,
				text,
				13.0,
				(g.quick.x + 8.0, g.rows_y + 28.0, g.quick.w - 16.0),
				C::Muted,
			);
		}
	}
}

fn draw_open(
	ui: &mut TextShaper,
	out: &mut Vec<Draw>,
	interaction: &crate::app::widgets::Drawn<'_>,
	b: &Button,
	columns: bool,
) {
	background(ui, out, interaction, b);
	let folder = b.action == Command::Chrome(ChromeCommand::OpenFolder);
	out.push(Draw::Icon {
		paths: if folder {
			icons::TREE_FOLDER_OPEN
		} else {
			icons::DOCUMENT
		},
		paint: Paint::Styled(Condition::Ui, C::Accent),
		x: b.rect.x + 10.0,
		y: b.rect.y + 12.0,
		size: 24.0,
	});
	let shortcut = if !columns {
		""
	} else if markview_platform::command_key() {
		if folder { "⇧⌘O" } else { "⌘O" }
	} else if folder {
		"Ctrl+Shift+O"
	} else {
		"Ctrl+O"
	};
	let shortcut_w = if columns {
		ui.text_width(shortcut, 11.0) + 20.0
	} else {
		0.0
	};
	label(
		ui,
		out,
		&b.label,
		14.0,
		(
			b.rect.x + 44.0,
			b.rect.y + 29.0,
			b.rect.w - 52.0 - shortcut_w,
		),
		C::Color,
	);
	label(
		ui,
		out,
		shortcut,
		11.0,
		(
			b.rect.x + b.rect.w - shortcut_w,
			b.rect.y + 28.0,
			shortcut_w,
		),
		C::Muted,
	);
}

pub(super) fn draw_row(
	ui: &mut TextShaper,
	out: &mut Vec<Draw>,
	interaction: &crate::app::widgets::Drawn<'_>,
	b: &Button,
	view: View<'_>,
	search: &start::Search,
	lang: Lang,
) {
	background(ui, out, interaction, b);
	let browser = view.browser.unwrap();
	let (path, recent) = match b.action {
		Command::Chrome(
			ChromeCommand::StartFile(id) | ChromeCommand::QuickOpenFile(id),
		) => {
			let file =
				search.matches.iter().find(|file| file.id == id).unwrap();
			(
				if file.parent.is_empty() {
					lang.empty_workspace_root()
				} else {
					&file.parent
				},
				None,
			)
		}
		Command::Chrome(ChromeCommand::RecentWorkspace(index)) => (
			if markview_platform::touch_layout() {
				lang.empty_local_workspace()
			} else {
				browser.start.recent[index]
					.path
					.to_str()
					.unwrap_or_default()
			},
			Some(&browser.start.recent[index]),
		),
		_ => unreachable!(),
	};
	let icon = if recent.is_some() {
		icons::TREE_FOLDER
	} else if crate::file::is_image(std::path::Path::new(&*b.label)) {
		icons::TREE_IMAGE
	} else {
		icons::TREE_MARKDOWN
	};
	out.push(Draw::Icon {
		paths: icon,
		paint: Paint::Styled(Condition::Ui, C::Color),
		x: b.rect.x + 10.0,
		y: b.rect.y + 18.0,
		size: 22.0,
	});
	let age = recent.map(|recent| start::now().saturating_sub(recent.opened));
	let when = age.map(|age| {
		if age < 3600 {
			lang.empty_recent_now().to_owned()
		} else if age < 86400 {
			lang.empty_recent_hours(age / 3600)
		} else {
			lang.empty_recent_days(age / 86400)
		}
	});
	let time_w = when
		.as_ref()
		.map_or(0.0, |when| ui.text_width(when, 11.0) + 12.0);
	out.push(Draw::Icon {
		paths: icons::RIGHT,
		paint: Paint::Styled(Condition::Ui, C::Muted),
		x: b.rect.x + b.rect.w - 18.0,
		y: b.rect.y + 22.0,
		size: 16.0,
	});
	let text_w = (b.rect.w - 64.0 - time_w).max(0.0);
	label(
		ui,
		out,
		&b.label,
		14.0,
		(b.rect.x + 44.0, b.rect.y + 24.0, text_w),
		C::Color,
	);
	if let Command::Chrome(
		ChromeCommand::StartFile(id) | ChromeCommand::QuickOpenFile(id),
	) = b.action
		&& let Some(snippet) = search.snippets.get(&id)
	{
		draw_snippet(
			ui,
			out,
			snippet,
			(b.rect.x + 44.0, b.rect.y + 44.0, text_w),
		);
	} else {
		label(
			ui,
			out,
			path,
			12.0,
			(b.rect.x + 44.0, b.rect.y + 44.0, text_w),
			C::Muted,
		);
	}
	if let Some(when) = when {
		label(
			ui,
			out,
			&when,
			11.0,
			(b.rect.x + b.rect.w - time_w - 20.0, b.rect.y + 32.0, time_w),
			C::Muted,
		);
	}
	rule(out, b.rect.x, b.rect.y + b.rect.h - 1.0, b.rect.w);
}

fn draw_snippet(
	ui: &mut TextShaper,
	out: &mut Vec<Draw>,
	snippet: &crate::app::file_search::Snippet,
	(x, y, width): (f32, f32, f32),
) {
	let appearance = ui_appearance(ui);
	let mut draws = Vec::new();
	let mut cursor = x;
	let mut append = |text: &str, bold: bool| {
		let mut style = appearance.clone();
		if bold {
			style.weight = style.weight.max(700);
		}
		let (run, advance) = ui.label_with(
			text,
			12.0,
			cursor,
			y,
			&style,
			Paint::Styled(Condition::Ui, C::Muted),
			None,
		);
		draws.extend(run);
		cursor += advance;
	};
	let mut byte = 0;
	for range in &snippet.matched {
		append(&snippet.text[byte..range.start], false);
		append(&snippet.text[range.clone()], true);
		byte = range.end;
	}
	append(&snippet.text[byte..], false);
	let clip = Rect {
		x,
		y: y - 18.0 * appearance.size,
		w: width,
		h: 30.0 * appearance.size,
	};
	if cursor - x > width {
		let (mut ellipsis, advance) = ui.label_with(
			"…",
			12.0,
			0.0,
			y,
			&appearance,
			Paint::Styled(Condition::Ui, C::Muted),
			None,
		);
		let right = x + (width - advance).max(0.0);
		for draw in &mut ellipsis {
			draw.translate(right, 0.0);
		}
		out.push(Draw::Clipped {
			rect: clip,
			draws: vec![
				Draw::Clipped {
					rect: Rect {
						w: (width - advance).max(0.0),
						..clip
					},
					draws,
				},
				Draw::Clipped {
					rect: clip,
					draws: ellipsis,
				},
			],
		});
	} else {
		out.push(Draw::Clipped { rect: clip, draws });
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{
		render::{Renderer, Theme, View as RenderView},
		state::InteractionState,
	};

	fn view(width: f32, height: f32, browser: &Browser) -> View<'_> {
		View {
			rect: Rect {
				x: 0.0,
				y: super::super::TOP,
				w: width,
				h: (height - super::super::TOP - super::super::BOTTOM).max(0.0),
			},
			phone: width < 600.0,
			browser: Some(browser),
		}
	}
	fn browser() -> (tempfile::TempDir, Browser) {
		let dir = tempfile::tempdir().unwrap();
		for name in ["README.md", "CHANGELOG.md", "notes.md"] {
			std::fs::write(dir.path().join(name), "# A document").unwrap();
		}
		let mut browser = Browser::default();
		browser.start.recent = ["Markview", "阅读笔记", "写作"]
			.into_iter()
			.map(|name| start::Recent {
				name: name.into(),
				path: std::path::PathBuf::from(format!("~/Documents/{name}")),
				opened: start::now(),
			})
			.collect();
		(dir, browser)
	}

	#[test]
	fn empty_page_controls_reflow_scroll_and_keep_distinct_hit_targets() {
		let mut ui = crate::test_support::shaper();
		let (dir, mut browser) = browser();
		for workspace in [false, true] {
			if workspace {
				browser.tree = Some(
					crate::app::folder::Tree::read(dir.path().into()).unwrap(),
				);
				browser.start.matches = browser
					.tree
					.as_ref()
					.unwrap()
					.entries
					.iter()
					.filter_map(|entry| entry.file.clone())
					.collect();
			}
			for lang in [Lang::En, Lang::ZhHans, Lang::ZhHant, Lang::Ja] {
				for (width, height) in [
					(1200.0, 800.0),
					(800.0, 600.0),
					(536.0, 300.0),
					(360.0, 760.0),
					(360.0, 480.0),
					(320.0, 200.0),
				] {
					let max =
						geometry(view(width, height, &browser)).max_scroll;
					let mut reached = Vec::new();
					for offset in [0.0, max / 2.0, max] {
						browser.start.scroll = offset;
						let v = view(width, height, &browser);
						let controls = buttons(&mut ui, v, lang);
						let layout = crate::app::widgets::layout(&controls);
						let ids = controls
							.iter()
							.map(|button| button.id)
							.collect::<std::collections::HashSet<_>>();
						assert_eq!(ids.len(), controls.len());
						for b in &controls {
							let hit = b.widget().hit_rect().unwrap();
							assert!(
								hit.x >= 24.0 && hit.x + hit.w <= width - 24.0
							);
							assert!(
								hit.y >= v.rect.y
									&& hit.y + hit.h
										<= v.rect.y + v.rect.h + 0.01
							);
							assert_eq!(
								layout
									.hit((
										hit.x + hit.w / 2.0,
										hit.y + hit.h / 2.0
									))
									.unwrap()
									.id,
								b.id
							);
							reached.push(b.action);
						}
						for (index, b) in controls.iter().enumerate() {
							assert!(controls[index + 1..].iter().all(
								|other| b.rect.intersect(other.rect).is_none()
							));
						}
					}
					for action in [
						Command::Document(DocumentCommand::Open),
						Command::Chrome(ChromeCommand::OpenFolder),
						Command::Chrome(ChromeCommand::FocusInput(
							TextField::Url,
						)),
						Command::Document(DocumentCommand::OpenUrl),
					] {
						assert!(
							reached.contains(&action),
							"{width}×{height}: {action:?}"
						);
					}
					assert_eq!(
						reached.contains(&Command::Chrome(
							ChromeCommand::FocusInput(TextField::Files)
						)),
						workspace
					);
					assert!(reached.iter().any(|action| if workspace {
						matches!(
							action,
							Command::Chrome(ChromeCommand::StartFile(_))
						)
					} else {
						matches!(
							action,
							Command::Chrome(ChromeCommand::RecentWorkspace(_))
						)
					}));
				}
			}
		}
	}

	#[test]
	fn workspace_caption_is_just_its_name_and_match_characters_are_bold() {
		fn weights(
			draws: &[Draw],
			output: &mut std::collections::BTreeSet<u16>,
		) {
			for draw in draws {
				match draw {
					Draw::Glyph(glyph) => {
						let face = ttf_parser::Face::parse(
							glyph.font.data.data(),
							glyph.font.index,
						)
						.unwrap();
						output.insert(face.weight().to_number());
					}
					Draw::Clipped { draws, .. } => weights(draws, output),
					_ => {}
				}
			}
		}
		let mut ui = crate::test_support::shaper();
		let (dir, mut browser) = browser();
		browser.tree =
			Some(crate::app::folder::Tree::read(dir.path().into()).unwrap());
		browser.start.matches = browser
			.tree
			.as_ref()
			.unwrap()
			.entries
			.iter()
			.filter_map(|entry| entry.file.clone())
			.collect();
		let v = view(1200.0, 800.0, &browser);
		let g = geometry(v);
		let mut actual = Vec::new();
		draw_workspace(&mut ui, &mut actual, v, &g, Lang::En);
		let mut expected = Vec::new();
		label(
			&mut ui,
			&mut expected,
			&browser.tree.as_ref().unwrap().name,
			12.0,
			(g.quick.x, g.quick.y + 52.0, g.quick.w),
			C::Muted,
		);
		assert_eq!(actual.len(), expected.len());
		for (text, matched) in
			[("docs/README.md", 5..9), ("docs/阅读.md", 5..11)]
		{
			let snippet = crate::app::file_search::Snippet {
				text: text.into(),
				matched: std::iter::once(matched).collect(),
			};
			for width in [280.0, 52.0] {
				let mut draws = Vec::new();
				draw_snippet(
					&mut ui,
					&mut draws,
					&snippet,
					(24.0, 100.0, width),
				);
				let mut used = std::collections::BTreeSet::new();
				weights(&draws, &mut used);
				assert!(used.contains(&400));
				assert!(used.contains(&700));
				assert!(
					matches!(draws.as_slice(), [Draw::Clipped { rect, .. }] if rect.w == width)
				);
			}
		}
	}

	#[test]
	fn quick_access_rows_use_the_sidebar_icons() {
		let mut ui = crate::test_support::shaper();
		let (dir, mut browser) = browser();
		std::fs::write(dir.path().join("cover.png"), b"png").unwrap();
		browser.tree =
			Some(crate::app::folder::Tree::read(dir.path().into()).unwrap());
		browser.start.matches = browser
			.tree
			.as_ref()
			.unwrap()
			.entries
			.iter()
			.filter_map(|entry| entry.file.clone())
			.collect();
		let v = view(1200.0, 800.0, &browser);
		let interaction = InteractionState::default();
		let drawn = crate::app::widgets::Drawn::new(&interaction, None);
		for (name, expected) in [
			("README.md", icons::TREE_MARKDOWN),
			("cover.png", icons::TREE_IMAGE),
		] {
			let file = browser
				.start
				.matches
				.iter()
				.find(|file| &*file.name == name)
				.unwrap();
			let button = components::button(
				file.name.clone(),
				Command::Chrome(ChromeCommand::StartFile(file.id)),
				Rect {
					x: 0.0,
					y: 0.0,
					w: 400.0,
					h: 60.0,
				},
			);
			let mut out = Vec::new();
			draw_row(
				&mut ui,
				&mut out,
				&drawn,
				&button,
				v,
				&browser.start.search,
				Lang::En,
			);
			let icon = out
				.iter()
				.find_map(|draw| match draw {
					Draw::Icon { paths, .. } => Some(*paths),
					_ => None,
				})
				.unwrap();
			assert_eq!(format!("{icon:?}"), format!("{expected:?}"), "{name}");
		}
	}

	#[test]
	fn empty_page_frames() -> anyhow::Result<()> {
		let output = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
			.join("artifacts/empty-page");
		std::fs::create_dir_all(&output)?;
		let mut renderer = pollster::block_on(Renderer::new(None))?;
		let (dir, mut browser) = browser();
		for page in 0..3 {
			let workspace = page > 0;
			if workspace {
				browser.tree =
					Some(crate::app::folder::Tree::read(dir.path().into())?);
				browser.start.matches = browser
					.tree
					.as_ref()
					.unwrap()
					.entries
					.iter()
					.filter_map(|entry| entry.file.clone())
					.collect();
			}
			if page == 2 {
				browser.start.matches = browser
					.start
					.matches
					.iter()
					.filter(|file| &*file.name == "README.md")
					.cloned()
					.collect();
				let id = browser.start.matches[0].id;
				browser.start.snippets.insert(
					id,
					crate::app::file_search::Snippet {
						text: "README.md".into(),
						matched: std::iter::once(0..4).collect(),
					},
				);
			}
			let suffix = if page == 2 {
				"search"
			} else if workspace {
				"true"
			} else {
				"false"
			};

			for dark in [false, true] {
				let sheet = markview_layout::style::Stylesheet::bundled(dark);
				renderer.set_stylesheet(sheet.clone());
				let mut ui = crate::test_support::shaper();
				ui.set_stylesheet(sheet);
				for lang in [Lang::En, Lang::ZhHans, Lang::ZhHant, Lang::Ja] {
					for (width, height) in [
						(1200.0, 800.0),
						(800.0, 600.0),
						(390.0, 844.0),
						(360.0, 480.0),
					] {
						browser.start.scroll = 0.0;
						let v = view(width, height, &browser);
						let interaction = InteractionState::default();
						let drawn =
							crate::app::widgets::Drawn::new(&interaction, None);
						let mut overlay = draw(&mut ui, &drawn, v, lang);
						for b in buttons(&mut ui, v, lang) {
							if let Command::Chrome(ChromeCommand::FocusInput(
								field,
							)) = b.action
							{
								let placeholder = if field == TextField::Files {
									lang.empty_search_placeholder()
								} else {
									lang.empty_url_placeholder()
								};
								let mut input = markview_layout::text_input::TextInput::default();
								if page == 2 && field == TextField::Files {
									input.set_text(&mut ui, "read");
								}
								overlay.push(Draw::Clipped {
									rect: v.rect,
									draws: input.draw(
										&mut ui,
										b.rect,
										false,
										false,
										placeholder,
									),
								});
							}
						}
						let horizontal = std::collections::HashMap::new();
						let render_view = RenderView {
							width: width as u32,
							height: height as u32,
							scale: 1.0,
							left: 0.0,
							top: 0.0,
							bottom: 0.0,
							scroll: 0.0,
							theme: if dark {
								Theme::Dark
							} else {
								Theme::Light
							},
							horizontal: &horizontal,
							selection: None,
							revision: 0,
							hovered_link: None,
							held_overflow: None,
						};
						let target = renderer
							.offscreen(render_view.width, render_view.height);
						let submission = renderer.render(
							&Default::default(),
							&render_view,
							&overlay,
							&target.create_view(&Default::default()),
						)?;
						renderer.wait(Some(submission))?;
						renderer.save_png(
							&target,
							&output.join(format!(
								"{lang:?}-{dark}-{width}-{height}-{suffix}.png"
							)),
						)?;
					}
				}
			}
		}
		Ok(())
	}
}
