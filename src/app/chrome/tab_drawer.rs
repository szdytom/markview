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
use markview_core::style::{ColorField as C, Condition};

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
			x: 8.,
			y: 56.,
			w: (panel.w - 28.).max(0.),
			h: (height - 120.).max(0.),
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
pub(super) fn buttons(
	ui: &mut TextShaper,
	tabs: &[ReaderTab],
	active: usize,
	scroll: f32,
	size: (f32, f32),
	lang: Lang,
) -> Vec<Button> {
	let (width, height) = size;
	let list = list(width, height, tabs.len(), scroll);
	let mut rows = Vec::new();
	for index in list.visible() {
		let row = list.row_rect(index);
		let path = &tabs[index].path;
		let name = path
			.file_name()
			.unwrap_or(path.as_os_str())
			.to_string_lossy();
		let name = ui.fit(&name, 13., (row.w - 68.).max(0.));
		let mut select = button(
			std::sync::Arc::<str>::from(name),
			Command::SelectTab(index),
			Rect {
				w: (row.w - 48.).max(0.),
				..row
			},
		);
		select.active = index == active;
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
	let mut buttons = list.hit(rows);
	let mut close = button(
		lang.panel_close(),
		Command::Tabs,
		Rect {
			x: list.panel.w - 52.,
			y: 6.,
			w: 44.,
			h: 44.,
		},
	);
	close.icon = Some(icons::CLOSE);
	close.kind = ButtonKind::Quiet;
	buttons.push(close);
	buttons.push(button(
		lang.toolbar_open(),
		Command::Open,
		Rect {
			x: 12.,
			y: height - 56.,
			w: (list.panel.w - 24.).max(0.),
			h: 44.,
		},
	));
	// The scrim dismisses the drawer without activating the document below.
	buttons.push(button(
		"",
		Command::Tabs,
		Rect {
			x: list.panel.w,
			y: 0.,
			w: width - list.panel.w,
			h: height,
		},
	));
	buttons
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
	];
	out.extend(ui.label(
		&format!("{} · {}", lang.toolbar_tabs(), tabs.len()),
		16.,
		16.,
		33.,
		Paint::Styled(Condition::Panel, C::Color),
	));
	let mut rows = Vec::new();
	for b in buttons(ui, tabs, active, scroll, size, lang) {
		if matches!(b.action, Command::SelectTab(_) | Command::CloseTab(_)) {
			if b.active {
				rows.push(Draw::Rect(
					Rect { w: 3., ..b.rect },
					Paint::Styled(Condition::Panel, C::Accent),
				));
			}
			rows.extend(components::draw_button(ui, interaction, &b, true));
		} else if b.rect.x < list.panel.w {
			out.extend(components::draw_button(ui, interaction, &b, true));
		}
	}
	out.push(list.clip(rows));
	list.draw_bar(&mut out, ui, interaction);
	out
}

#[cfg(test)]
mod tests {
	use super::*;
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
