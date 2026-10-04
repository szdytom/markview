//! Phone tab presentation over the shared reader sessions and scrolling list.
use super::{
	components::{self, ButtonKind, button},
	icons,
	list::List,
};
use crate::{
	app::Button,
	lang::Lang,
	layout::{Draw, Paint, Rect, TextShaper},
	state::{Command, InteractionState, ReaderTab},
};
use markview_core::style::{Color, ColorField as C, Condition};

pub(in crate::app) fn rect(width: f32, height: f32) -> Rect {
	Rect {
		x: 0.,
		y: 0.,
		w: 320_f32.min((width - 48.).max(0.)),
		h: height,
	}
}
pub(in crate::app) fn list(
	width: f32,
	height: f32,
	count: usize,
	scroll: f32,
) -> List {
	let panel = rect(width, height);
	List::new(
		panel,
		Rect {
			x: 16.,
			y: 64.,
			w: (panel.w - 32.).max(0.),
			h: (height - 128.).max(0.),
		},
		0.,
		56.,
		count,
		scroll,
	)
}
pub(super) fn toggle(lang: Lang) -> Button {
	let mut toggle = button(
		lang.toolbar_tabs(),
		Command::Tabs,
		Rect {
			x: 4.,
			y: 0.,
			w: 44.,
			h: 40.,
		},
	);
	toggle.icon = Some(icons::TABS);
	toggle.kind = ButtonKind::Quiet;
	toggle
}
fn row_buttons(
	ui: &mut TextShaper,
	tabs: &[ReaderTab],
	active: usize,
	list: List,
	lang: Lang,
) -> Vec<Button> {
	let mut rows = Vec::new();
	for index in list.visible() {
		let row = list.row_rect(index);
		let path = &tabs[index].path;
		let name = path
			.file_name()
			.unwrap_or(path.as_os_str())
			.to_string_lossy();
		let name = ui.fit(&name, 14., (row.w - 72.).max(0.));
		let mut select = button(
			std::sync::Arc::<str>::from(name),
			Command::SelectTab(index),
			Rect {
				w: (row.w - 48.).max(0.),
				..row
			},
		);
		select.active = index == active;
		select.kind = ButtonKind::Quiet;
		rows.push(select);
		let mut close = button(
			lang.panel_close(),
			Command::CloseTab(index),
			Rect {
				x: row.x + row.w - 44.,
				y: row.y + 6.,
				w: 44.,
				h: 44.,
			},
		);
		close.icon = Some(icons::CLOSE);
		close.kind = ButtonKind::Quiet;
		rows.push(close);
	}
	rows
}
fn controls(panel: Rect, width: f32, lang: Lang) -> Vec<Button> {
	let mut close = button(
		lang.panel_close(),
		Command::Tabs,
		Rect {
			x: panel.w - 52.,
			y: 6.,
			w: 44.,
			h: 44.,
		},
	);
	close.icon = Some(icons::CLOSE);
	close.kind = ButtonKind::Quiet;
	let mut open = button(
		lang.toolbar_open(),
		Command::Open,
		Rect {
			x: 16.,
			y: panel.h - 54.,
			w: (panel.w - 32.).max(0.),
			h: 44.,
		},
	);
	open.kind = ButtonKind::Quiet;
	// The scrim dismisses the drawer without activating the document below.
	let scrim = button(
		"",
		Command::Tabs,
		Rect {
			x: panel.w,
			y: 0.,
			w: width - panel.w,
			h: panel.h,
		},
	);
	vec![close, open, scrim]
}
pub(super) fn buttons(
	ui: &mut TextShaper,
	tabs: &[ReaderTab],
	active: usize,
	scroll: f32,
	size: (f32, f32),
	lang: Lang,
) -> Vec<Button> {
	components::appearance(ui);
	let list = list(size.0, size.1, tabs.len(), scroll);
	let mut buttons = list.hit(row_buttons(ui, tabs, active, list, lang));
	buttons.extend(controls(list.panel, size.0, lang));
	buttons
}

fn draw_entry(
	ui: &mut TextShaper,
	interaction: &InteractionState,
	b: &Button,
) -> Vec<Draw> {
	let mut surface = b.clone();
	surface.label = "".into();
	surface.active = false;
	let mut out = components::draw_button(ui, interaction, &surface, true);
	let open = b.action == Command::Open;
	if open {
		out.push(Draw::Icon {
			paths: icons::OPEN,
			paint: Paint::Styled(Condition::Panel, C::Color),
			x: b.rect.x + 8.,
			y: b.rect.y + (b.rect.h - 20.) / 2.,
			size: 20.,
		});
	}
	let inset = if open { 40. } else { 16. };
	let name = ui.fit(&b.label, 14., (b.rect.w - inset - 8.).max(0.));
	out.extend(ui.label(
		&name,
		14.,
		b.rect.x + inset,
		b.rect.y + b.rect.h / 2. + 5.,
		Paint::Styled(
			Condition::Panel,
			if b.active || open { C::Color } else { C::Muted },
		),
	));
	out
}
pub(super) fn draw(
	ui: &mut TextShaper,
	tabs: &[ReaderTab],
	active: usize,
	scroll: f32,
	interaction: &InteractionState,
	size: (f32, f32),
	lang: Lang,
) -> Vec<Draw> {
	components::appearance(ui);
	let (width, height) = size;
	let list = list(width, height, tabs.len(), scroll);
	let mut out = vec![
		Draw::Rect(
			Rect {
				x: 0.,
				y: 0.,
				w: width,
				h: height,
			},
			Paint::Scrim,
		),
		Draw::Rect(list.panel, Paint::Styled(Condition::Panel, C::Background)),
		Draw::Icon {
			paths: icons::APP,
			paint: Paint::Styled(Condition::Panel, C::Color),
			x: 12.,
			y: 14.,
			size: 28.,
		},
	];
	for r in [
		Rect {
			x: list.panel.w - 1.,
			w: 1.,
			..list.panel
		},
		Rect {
			x: 16.,
			y: 55.,
			w: list.panel.w - 32.,
			h: 1.,
		},
		Rect {
			y: height - 64.,
			h: 1.,
			..list.panel
		},
	] {
		out.push(components::line(r, Condition::Panel, C::BorderColor));
	}
	let title = ui.fit(lang.toolbar_tabs(), 16., (list.panel.w - 132.).max(0.));
	let title_width = ui.text_width(&title, 16.);
	out.extend(ui.label(
		&title,
		16.,
		48.,
		34.,
		Paint::Styled(Condition::Panel, C::Color),
	));
	out.extend(ui.label(
		&format!("· {}", tabs.len()),
		13.,
		48. + title_width + 8.,
		34.,
		Paint::Styled(Condition::Panel, C::Muted),
	));
	let background = ui.stylesheet.color(Condition::Panel, C::Background);
	let accent = ui.stylesheet.color(Condition::Panel, C::Accent);
	let selected =
		Paint::Color(Color(u32::from_be_bytes(std::array::from_fn(|i| {
			((background[i] + (accent[i] - background[i]) * 0.06) * 255.)
				.round() as u8
		}))));
	let mut rows = Vec::new();
	for b in row_buttons(ui, tabs, active, list, lang) {
		if matches!(b.action, Command::SelectTab(_)) {
			if b.active {
				rows.push(Draw::Rect(
					Rect {
						w: list.viewport.w,
						..b.rect
					},
					selected,
				));
			}
			rows.extend(draw_entry(ui, interaction, &b));
			if b.active {
				rows.push(Draw::Rect(
					Rect {
						y: b.rect.y + 16.,
						w: 2.,
						h: 24.,
						..b.rect
					},
					Paint::Styled(Condition::Panel, C::Accent),
				));
			}
		} else {
			rows.extend(components::draw_button(ui, interaction, &b, true));
		}
	}
	out.push(list.clip(rows));
	list.draw_bar(&mut out, ui, interaction);
	for b in controls(list.panel, width, lang) {
		if b.action == Command::Open {
			out.extend(draw_entry(ui, interaction, &b));
		} else if b.rect.x < list.panel.w {
			out.extend(components::draw_button(ui, interaction, &b, true));
		}
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn drawer_labels_follow_partial_scroll_without_recentering() {
		let mut ui = crate::test_support::shaper();
		let tabs: Vec<_> = (0..30)
			.map(|i| ReaderTab::new(format!("{i}-document.md").into()))
			.collect();
		let first_label = |draws: Vec<Draw>| {
			draws
				.into_iter()
				.find_map(|draw| {
					let Draw::Clipped { draws, .. } = draw else {
						return None;
					};
					draws.into_iter().find_map(|draw| match draw {
						Draw::Glyph(glyph) => Some((glyph.x, glyph.y)),
						_ => None,
					})
				})
				.unwrap()
		};
		for dark in [false, true] {
			ui.set_stylesheet(markview_core::style::Stylesheet::bundled(dark));
			let resting = first_label(draw(
				&mut ui,
				&tabs,
				0,
				0.,
				&InteractionState::default(),
				(320., 760.),
				Lang::En,
			));
			let scrolled = first_label(draw(
				&mut ui,
				&tabs,
				0,
				17.,
				&InteractionState::default(),
				(320., 760.),
				Lang::En,
			));
			assert_eq!(resting.0, 32.);
			assert_eq!(scrolled.0, resting.0);
			assert_eq!(resting.1 - scrolled.1, 17.);
			let buttons =
				buttons(&mut ui, &tabs, 0, 17., (320., 760.), Lang::En);
			let select = buttons
				.iter()
				.find(|b| b.action == Command::SelectTab(0))
				.unwrap();
			assert!(select.rect.contains(scrolled.0, scrolled.1));
			assert_eq!(select.rect.y, 64.);
		}
	}

	#[test]
	fn drawer_rows_are_clipped_and_touch_targets_do_not_overlap() {
		let mut ui = crate::test_support::shaper();
		let tabs: Vec<_> = (0..30)
			.map(|i| {
				ReaderTab::new(format!("{i}-中文-long-document.md").into())
			})
			.collect();
		for width in [320., 411., 599.] {
			let list = list(width, 760., tabs.len(), f32::INFINITY);
			assert_eq!(list.scroll, list.max_scroll());
			assert!(list.viewport.w < width - 48.);
			let buttons = buttons(
				&mut ui,
				&tabs,
				29,
				list.scroll,
				(width, 760.),
				Lang::En,
			);
			assert!(!buttons.iter().any(|b| b.action == Command::SelectTab(0)));
			assert!(
				buttons
					.iter()
					.any(|b| b.action == Command::SelectTab(29) && b.active)
			);
			for (i, a) in buttons.iter().enumerate() {
				for b in &buttons[i + 1..] {
					assert!(a.rect.intersect(b.rect).is_none());
				}
				if matches!(
					a.action,
					Command::SelectTab(_) | Command::CloseTab(_)
				) {
					assert!(
						a.rect.y >= list.viewport.y
							&& a.rect.y + a.rect.h
								<= list.viewport.y + list.viewport.h
					);
				}
			}
			assert!(
				draw(
					&mut ui,
					&tabs,
					29,
					list.scroll,
					&InteractionState::default(),
					(width, 760.),
					Lang::En
				)
				.iter()
				.any(|d| matches!(d, Draw::Clipped { .. }))
			);
		}
	}
}
