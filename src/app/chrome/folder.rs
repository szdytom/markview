//! The folder sidebar uses the same clipped rows for painting and input.
use super::{components, icons, list::List};
use crate::{
	app::{Button, folder::Browser},
	lang::Lang,
	layout::{Draw, Paint, Rect, TextShaper},
	state::{Command, InteractionState},
};
use markview_layout::{
	fonts::FontConfig,
	style::{ColorField as C, Condition, Stylesheet},
};
use std::{collections::HashMap, path::Path, sync::Arc};

const HEADER_BUTTON: f32 = 28.;
const RIGHT_INSET: f32 = 8.;
const ROW_ICON: f32 = 20.;

/// Bounded, position-independent filename labels for scrolling and hovering.
#[derive(Default)]
pub(in crate::app) struct Labels {
	sheet: Option<Arc<Stylesheet>>,
	fonts: Option<FontConfig>,
	width: f32,
	rows: HashMap<markview_ui_core::WidgetId, Vec<Draw>>,
}

impl Labels {
	fn prepare(&mut self, ui: &TextShaper, width: f32) {
		if self.width != width
			|| self.fonts.as_ref() != Some(ui.font_config())
			|| self
				.sheet
				.as_ref()
				.is_none_or(|sheet| !Arc::ptr_eq(sheet, &ui.stylesheet))
		{
			self.rows.clear();
			self.width = width;
			self.sheet = Some(ui.stylesheet.clone());
			self.fonts = Some(ui.font_config().clone());
		}
	}

	fn draw(
		&mut self,
		ui: &mut TextShaper,
		out: &mut Vec<Draw>,
		action: Command,
		name: &str,
		rect: Rect,
	) {
		let id = action.widget_id();
		if self.rows.len() >= 128 && !self.rows.contains_key(&id) {
			self.rows.clear();
		}
		let draws = self.rows.entry(id).or_insert_with(|| {
			components::label(
				ui,
				name,
				13.,
				Rect {
					x: 0.,
					y: 0.,
					..rect
				},
				C::Color,
			)
		});
		out.extend(draws.iter().cloned().map(|mut draw| {
			draw.translate(rect.x, rect.y);
			draw
		}));
	}
}

pub(in crate::app) fn docked(width: f32, phone: bool) -> bool {
	!phone && width >= 760.
}

pub(in crate::app) fn rect(
	width: f32,
	height: f32,
	top: f32,
	bottom: f32,
	preferred_width: f32,
) -> Rect {
	Rect {
		x: 0.,
		y: top,
		w: preferred_width.min(
			(width - if docked(width, false) { 440. } else { 48. }).max(0.),
		),
		h: (height - top - bottom).max(0.),
	}
}

pub(in crate::app) fn surface(drawer: Rect, height: f32) -> Rect {
	Rect {
		y: 0.,
		h: height,
		..drawer
	}
}

pub(in crate::app) fn list(drawer: Rect, rows: usize, scroll: f32) -> List {
	List::new(
		drawer,
		Rect {
			y: drawer.y + 66.,
			h: (drawer.h - 66. - super::BOTTOM).max(0.),
			..drawer
		},
		0.,
		28.,
		rows,
		scroll,
	)
}

pub(in crate::app) fn animated_rect(mut drawer: Rect, reveal: f32) -> Rect {
	drawer.x = -drawer.w * (1. - reveal);
	drawer
}

pub(super) fn buttons(
	browser: &Browser,
	drawer: Rect,
	lang: Lang,
) -> Vec<Button> {
	let Some(tree) = &browser.tree else {
		return Vec::new();
	};
	let mut buttons = Vec::new();
	for (i, (label, action, icon)) in [
		(
			lang.folder_refresh(),
			Command::FolderRefresh,
			icons::REDOWNLOAD,
		),
		(
			lang.folder_collapse_all(),
			Command::FolderCollapseAll,
			icons::COLLAPSE_ALL,
		),
	]
	.into_iter()
	.enumerate()
	{
		let mut b = components::button(
			label,
			action,
			Rect {
				x: drawer.x + drawer.w
					- RIGHT_INSET - (2 - i) as f32 * HEADER_BUTTON,
				y: drawer.y + (38. - HEADER_BUTTON) / 2.,
				w: HEADER_BUTTON,
				h: HEADER_BUTTON,
			},
		);
		b.id = crate::app::widgets::row_id(0x666f6c646572, &action.widget_id());
		b.kind = components::ButtonKind::Quiet;
		b.icon = Some(icon);
		b.clip = Some(drawer);
		buttons.push(b);
	}
	let mut root = components::button(
		std::sync::Arc::<str>::from(tree.name.as_str()),
		Command::FolderRoot,
		Rect {
			y: drawer.y + 38.,
			h: 28.,
			..drawer
		},
	);
	root.clip = Some(drawer);
	buttons.push(root);
	let rows = tree.rows();
	let list = list(drawer, rows.len(), browser.scroll.offset());
	let row_buttons = list
		.visible()
		.map(|row| {
			let index = rows[row];
			let e = &tree.entries[index];
			let mut b = components::button(
				std::sync::Arc::<str>::from(
					e.path.file_name().unwrap().to_string_lossy().as_ref(),
				),
				Command::FolderEntry(index),
				list.row_rect(row),
			);
			if list.max_scroll() > 0. {
				b.rect.w = (b.rect.w - 16.).max(0.);
			}
			b.kind = components::ButtonKind::Quiet;
			b
		})
		.collect();
	buttons.extend(list.hit(row_buttons));
	buttons
}

pub(super) struct Pane {
	pub body: Rect,
	pub surface: Rect,
}

pub(super) fn draw(
	ui: &mut TextShaper,
	browser: &Browser,
	active: Option<&Path>,
	interaction: &InteractionState,
	pane: Pane,
	lang: Lang,
) -> Vec<Draw> {
	let Pane {
		body: drawer,
		surface,
	} = pane;
	let Some(tree) = &browser.tree else {
		return Vec::new();
	};
	components::appearance(ui);
	let mut labels = browser.labels.borrow_mut();
	labels.prepare(ui, drawer.w);
	let background = components::mix(
		ui,
		Paint::Styled(Condition::Panel, C::Background),
		Paint::Styled(Condition::Panel, C::Muted),
		0.08,
	);
	let mut out = vec![
		Draw::Rect(surface, background),
		Draw::Rect(
			Rect {
				x: surface.x + surface.w - 1.,
				w: 1.,
				..surface
			},
			Paint::Styled(Condition::Panel, C::BorderColor),
		),
	];
	ui.appearance.weight = 650;
	out.extend(components::label(
		ui,
		&lang.folder_files().to_uppercase(),
		12.,
		Rect {
			x: drawer.x + 16.,
			y: drawer.y,
			w: (drawer.w - 88.).max(0.),
			h: 38.,
		},
		C::Muted,
	));
	components::appearance(ui);
	let root_rect = Rect {
		y: drawer.y + 38.,
		h: 28.,
		..drawer
	};
	let mut header = Vec::new();
	Row {
		rect: root_rect,
		action: Command::FolderRoot,
		depth: 0,
		collapsed: Some(tree.root_collapsed()),
		name: &tree.name,
		selected: false,
	}
	.draw(ui, &mut labels, &mut header, interaction);
	out.push(Draw::Clipped {
		rect: drawer,
		draws: header,
	});
	for b in buttons(browser, drawer, lang).into_iter().filter(|b| {
		matches!(
			b.action,
			Command::FolderCollapseAll | Command::FolderRefresh
		)
	}) {
		out.extend(components::draw_button(ui, interaction, &b, true));
	}
	let rows = tree.rows();
	let list = list(drawer, rows.len(), browser.scroll.offset());
	let mut body = Vec::new();
	if tree.entries.is_empty() && !tree.root_collapsed() {
		body.extend(components::label(
			ui,
			lang.folder_empty(),
			12.,
			Rect {
				x: drawer.x + 16.,
				w: (drawer.w - 32.).max(0.),
				h: 56.,
				..list.viewport
			},
			C::Muted,
		));
	}
	for row in list.visible() {
		let index = rows[row];
		let e = &tree.entries[index];
		Row {
			rect: list.row_rect(row),
			action: Command::FolderEntry(index),
			depth: e.depth + 1,
			collapsed: e.directory.then(|| tree.collapsed(index)),
			name: &e.path.file_name().unwrap().to_string_lossy(),
			selected: active == Some(e.path.as_path()),
		}
		.draw(ui, &mut labels, &mut body, interaction);
	}
	out.push(list.clip(body));
	list.draw_bar(&mut out, ui, interaction);
	out
}

struct Row<'a> {
	rect: Rect,
	action: Command,
	depth: usize,
	collapsed: Option<bool>,
	name: &'a str,
	selected: bool,
}

impl Row<'_> {
	fn draw(
		self,
		ui: &mut TextShaper,
		labels: &mut Labels,
		out: &mut Vec<Draw>,
		interaction: &InteractionState,
	) {
		let Self {
			rect,
			action,
			depth,
			collapsed,
			name,
			selected,
		} = self;
		let focused = interaction.widgets.focus == Some(action.widget_id());
		let background = Paint::Styled(Condition::Panel, C::Background);
		let ink = Paint::Styled(Condition::Panel, C::Color);
		if selected
			|| rect.contains(
				interaction.pointer.cursor().0,
				interaction.pointer.cursor().1,
			) {
			out.push(Draw::Rect(
				rect,
				components::mix(
					ui,
					background,
					ink,
					if selected { 0.18 } else { 0.11 },
				),
			));
		}
		if focused && interaction.widgets.focus_visible {
			out.push(Draw::Rect(
				Rect {
					x: rect.x + 1.,
					w: 2.,
					..rect
				},
				Paint::Styled(Condition::Button, C::Accent),
			));
		}
		let indent = (12. + depth as f32 * 16.).min((rect.w - 72.).max(12.));
		if let Some(collapsed) = collapsed {
			out.push(Draw::Polygon {
				center: [rect.x + indent + 4., rect.y + rect.h / 2.],
				points: if collapsed {
					[[-2., -4.], [3., 0.], [-2., 4.]].into()
				} else {
					[[-4., -2.], [4., -2.], [0., 3.]].into()
				},
				paint: Paint::Styled(Condition::Panel, C::Muted),
			});
		}
		out.push(Draw::Icon {
			paths: match collapsed {
				Some(true) => icons::TREE_FOLDER,
				Some(false) => icons::TREE_FOLDER_OPEN,
				None => icons::TREE_MARKDOWN,
			},
			paint: components::mix(ui, background, ink, 0.46),
			x: rect.x + indent + 12.,
			y: rect.y + (rect.h - ROW_ICON) / 2.,
			size: ROW_ICON,
		});
		labels.draw(
			ui,
			out,
			action,
			name,
			Rect {
				x: rect.x + indent + 40.,
				w: (rect.w - indent - 48.).max(0.),
				..rect
			},
		);
	}
}

pub(in crate::app) fn toggle(rect: Rect, lang: Lang) -> Button {
	let mut button =
		components::button(lang.folder_toggle(), Command::Folder, rect);
	button.id = crate::app::widgets::row_id(
		0x666f6c646572,
		&Command::Folder.widget_id(),
	);
	button.icon = Some(icons::SIDEBAR);
	button.kind = components::ButtonKind::Quiet;
	button
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{
		app::{folder::Tree, frame::Layout},
		settings::WindowLayout,
	};

	#[test]
	fn cached_filenames_pick_up_fonts_installed_in_an_existing_source() {
		let fonts = tempfile::tempdir().unwrap();
		let fixtures = Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("crates/markview-layout/tests/fonts");
		let install = |name: &str| {
			std::fs::copy(fixtures.join(name), fonts.path().join(name))
				.unwrap();
		};
		install("NotoSerif-Regular-subset.otf");
		let mut config = markview_layout::fonts::FontConfig {
			ignore_system_fonts: true,
			directories: vec![fonts.path().into()],
			..Default::default()
		};
		let mut ui = TextShaper::with_fonts(config.clone());
		let dir = tempfile::tempdir().unwrap();
		std::fs::write(dir.path().join("Example.md"), "# Note").unwrap();
		let mut browser = Browser::default();
		browser.tree = Some(Tree::read(dir.path().into()).unwrap());
		let render_font = |ui: &mut TextShaper| {
			let drawer = rect(1200., 800., 40., 24., 260.);
			draw(
				ui,
				&browser,
				None,
				&InteractionState::default(),
				Pane {
					body: drawer,
					surface: drawer,
				},
				Lang::En,
			);
			browser.labels.borrow().rows[&Command::FolderEntry(0).widget_id()]
				.iter()
				.find_map(|draw| match draw {
					Draw::Glyph(glyph) => Some(glyph.font.data.clone()),
					_ => None,
				})
				.unwrap()
		};
		let expected =
			std::fs::read(fixtures.join("NotoSerif-Regular-subset.otf"))
				.unwrap();
		assert!(render_font(&mut ui).as_ref() == expected);
		install("NotoSans-Regular-subset.otf");
		config.revision += 1;
		ui.set_fonts(&config);
		let expected =
			std::fs::read(fixtures.join("NotoSans-Regular-subset.otf"))
				.unwrap();
		assert!(render_font(&mut ui).as_ref() == expected);
	}

	#[test]
	fn header_and_row_icons_keep_their_alignment_while_the_drawer_moves() {
		let dir = tempfile::tempdir().unwrap();
		std::fs::write(dir.path().join("Note.md"), "# Note").unwrap();
		let mut browser = Browser::default();
		browser.tree = Some(Tree::read(dir.path().into()).unwrap());
		let mut ui = crate::test_support::shaper();
		for width in [200., 260., 600.] {
			for reveal in [0., 0.25, 0.5, 0.75, 1.] {
				let drawer =
					animated_rect(rect(1200., 800., 40., 24., width), reveal);
				let buttons = buttons(&browser, drawer, Lang::En);
				assert_eq!(buttons[0].action, Command::FolderRefresh);
				let collapse_all = &buttons[1];
				assert_eq!(collapse_all.action, Command::FolderCollapseAll);
				assert_eq!(
					collapse_all.rect.x + collapse_all.rect.w,
					drawer.x + width - RIGHT_INSET
				);
				if reveal == 1. {
					let frame = Layout::new(
						WindowLayout::Macos,
						false,
						1200.,
						800.,
						false,
						false,
					)
					.with_sidebar(width, reveal);
					assert_eq!(
						collapse_all.rect.x,
						frame.sidebar_toggle.unwrap().x
					);
				}
				let draws = draw(
					&mut ui,
					&browser,
					None,
					&InteractionState::default(),
					Pane {
						body: drawer,
						surface: drawer,
					},
					Lang::En,
				);
				let Draw::Clipped { draws, .. } = &draws[draws.len() - 1]
				else {
					panic!()
				};
				let Draw::Icon { x, y, size, .. } = draws
					.iter()
					.find(|d| matches!(d, Draw::Icon { .. }))
					.unwrap()
				else {
					panic!()
				};
				let row = list(drawer, 1, 0.).row_rect(0);
				assert_eq!(*size, ROW_ICON);
				assert_eq!(y + size / 2., row.y + row.h / 2.);
				assert_eq!(*x - row.x, 40.);
			}
		}
	}
}
