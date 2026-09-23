//! Shared, concrete chrome components. Geometry owns painting and input alike.
use super::icons;
use crate::{
	app::Button,
	lang::Lang,
	layout::{Draw, Paint, Rect, Scrollbar, TextShaper},
	state::{Command, Dropdown, DropdownId, InteractionState, PanelTab},
};
use markview_core::style::{Color, ColorField as C, Condition, TextAppearance};

pub(in crate::app) const CONTROL: f32 = 32.0;
pub(super) const INSET: f32 = 24.0;
const TAB_Y: f32 = 52.0;
const ROW: f32 = 44.0;
const SECTION: f32 = 32.0;
/// The trailing marker on a button that opens a list.
const MARKER: f32 = 14.0;
/// The pitch of one option in an open list, and the list's own padding.
pub(super) const OPTION: f32 = 30.0;
const MENU_PAD: f32 = 4.0;
/// How far an open list stands off the control it belongs to.
const MENU_GAP: f32 = 4.0;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::app) enum ButtonKind {
	#[default]
	Standard,
	Quiet,
	Link,
	Primary,
}

pub(super) fn button(
	label: &'static str,
	action: Command,
	rect: Rect,
) -> Button {
	Button {
		label,
		action,
		rect,
		icon: match action {
			Command::CopyDiagnostics => Some(icons::COPY),
			Command::Smaller
			| Command::Narrower
			| Command::ScrollSpeed(-1)
			| Command::ExportSize(-1) => Some(icons::MINUS),
			Command::Larger
			| Command::Wider
			| Command::ScrollSpeed(1)
			| Command::ExportSize(1) => Some(icons::PLUS),
			_ => None,
		},
		marker: None,
		active: false,
		kind: ButtonKind::Standard,
		enabled: true,
	}
}

pub(super) fn appearance(ui: &mut TextShaper) {
	ui.appearance = ui.stylesheet.text(
		&ui.stylesheet
			.text(&TextAppearance::default(), Condition::Ui),
		Condition::Panel,
	);
}

pub(in crate::app) fn panel_rect(width: f32, height: f32) -> Rect {
	let w = 600.0_f32.min((width - 32.0).max(0.0));
	let top = crate::app::TOP + 8.0;
	let h = 620.0_f32.min((height - top - 16.0).max(0.0));
	Rect {
		x: (width - w) / 2.0,
		y: ((height - h) / 2.0).max(top),
		w,
		h,
	}
}

pub(in crate::app) fn line(rect: Rect, condition: Condition, color: C) -> Draw {
	Draw::Rect(rect, Paint::Styled(condition, color))
}

/// The settings panel's pages, as one row of tabs below its title.
pub(in crate::app) fn tab_controls(
	rect: Rect,
	current: PanelTab,
	lang: Lang,
) -> Vec<Button> {
	let tabs = [
		(lang.tabs_generic(), PanelTab::Generic),
		(lang.tabs_styles(), PanelTab::Styles),
		(lang.tabs_fonts(), PanelTab::Fonts),
		(lang.tabs_about(), PanelTab::About),
	];
	let step = ((rect.w - INSET * 2.0) / tabs.len() as f32).min(110.0);
	let mut out = Vec::new();
	for (index, (label, tab)) in tabs.iter().enumerate() {
		let mut b = button(
			label,
			Command::SettingsTab(*tab),
			Rect {
				x: rect.x + INSET + index as f32 * step,
				y: rect.y + TAB_Y,
				w: (step - 6.0).max(0.0),
				h: CONTROL,
			},
		);
		b.kind = ButtonKind::Quiet;
		b.active = *tab == current;
		out.push(b);
	}
	out
}

/// Whether a page is previewing the document, and so recedes behind it.
///
/// The header is drawn after this, so the exit control stays legible.
pub(in crate::app) const PREVIEW_OPACITY: f32 = 0.25;

/// Draws the tab row, in the place a page's heading would occupy.
pub(in crate::app) fn draw_tabs(
	ui: &mut TextShaper,
	interaction: &InteractionState,
	rect: Rect,
	current: PanelTab,
	lang: Lang,
) -> Vec<Draw> {
	let mut out = Vec::new();
	for b in tab_controls(rect, current, lang) {
		out.extend(draw_button(ui, interaction, &b, true));
		if b.active {
			out.push(line(
				Rect {
					x: b.rect.x,
					y: b.rect.y + b.rect.h - 2.0,
					w: b.rect.w,
					h: 2.0,
				},
				Condition::Button,
				C::Accent,
			));
		}
	}
	out
}

/// Whether a button belongs to the settings header, which a page draws once
/// through [`draw_settings_header`] rather than through its control loop.
pub(in crate::app) fn is_settings_header(action: Command) -> bool {
	matches!(
		action,
		Command::SettingsTab(_) | Command::Settings | Command::SettingsPreview
	)
}

pub(in crate::app) fn settings_header_controls(
	rect: Rect,
	current: PanelTab,
	preview: bool,
	lang: Lang,
) -> Vec<Button> {
	let mut out = tab_controls(rect, current, lang);
	let close = button(
		lang.panel_close(),
		Command::Settings,
		Rect {
			x: rect.x + rect.w - INSET - CONTROL,
			y: rect.y + 18.0,
			w: CONTROL,
			h: CONTROL,
		},
	);
	let mut eye = button(
		lang.panel_preview(),
		Command::SettingsPreview,
		Rect {
			x: close.rect.x - CONTROL - 8.0,
			..close.rect
		},
	);
	eye.icon = Some(if preview { icons::EYE_OFF } else { icons::EYE });
	eye.active = preview;
	out.extend([eye, {
		let mut close = close;
		close.icon = Some(icons::CLOSE);
		close
	}]);
	out
}

pub(in crate::app) fn draw_settings_header(
	ui: &mut TextShaper,
	interaction: &InteractionState,
	rect: Rect,
	current: PanelTab,
	preview: bool,
	lang: Lang,
) -> Vec<Draw> {
	appearance(ui);
	let old_weight = ui.appearance.weight;
	ui.appearance.weight = 700;
	let mut out = label(
		ui,
		lang.panel_settings(),
		20.0,
		Rect {
			x: rect.x + INSET,
			y: rect.y + 16.0,
			w: rect.w - INSET * 2.0,
			h: 32.0,
		},
		C::Color,
	);
	ui.appearance.weight = old_weight;
	out.extend(draw_tabs(ui, interaction, rect, current, lang));
	let controls = settings_header_controls(rect, current, preview, lang);
	for control in controls.iter().filter(|b| b.icon.is_some()) {
		out.extend(draw_button(ui, interaction, control, true));
	}
	out
}

pub(in crate::app) fn frame(rect: Rect, width: f32, height: f32) -> Vec<Draw> {
	vec![
		Draw::Rect(
			Rect {
				x: 0.0,
				y: 0.0,
				w: width,
				h: height,
			},
			Paint::Scrim,
		),
		Draw::Box {
			rect,
			chain: Condition::Panel.chain(),
			condition: Condition::Panel,
			radius: 0.0,
			border: 1.0,
			left_only: false,
			decoration: None,
		},
	]
}

pub(super) fn label(
	ui: &mut TextShaper,
	text: &str,
	size: f32,
	rect: Rect,
	color: C,
) -> Vec<Draw> {
	let text = ui.fit(text, size, rect.w);
	ui.label(
		&text,
		size,
		rect.x,
		rect.y + rect.h / 2.0 + size * 0.35,
		Paint::Styled(Condition::Panel, color),
	)
}

pub(in crate::app) fn draw_button(
	ui: &mut TextShaper,
	interaction: &InteractionState,
	b: &Button,
	panel: bool,
) -> Vec<Draw> {
	draw_button_edges(ui, interaction, b, panel, [true, true])
}

/// Draws a segment with shared edges owned by the focused or selected neighbor.
pub(in crate::app) fn draw_segmented_button(
	ui: &mut TextShaper,
	interaction: &InteractionState,
	buttons: &[Button],
	i: usize,
) -> Vec<Draw> {
	let b = &buttons[i];
	let priority = |b: &Button| {
		(
			b.enabled
				&& interaction.focus_visible
				&& interaction.focus == Some(b.action),
			b.enabled && b.active,
		)
	};
	let joined = |left: &Button, right: &Button| {
		left.rect.y == right.rect.y
			&& (left.rect.x + left.rect.w - right.rect.x).abs() < 0.001
	};
	let left = i == 0
		|| !joined(&buttons[i - 1], b)
		|| priority(b) > priority(&buttons[i - 1]);
	let right = i + 1 == buttons.len()
		|| !joined(b, &buttons[i + 1])
		|| priority(b) >= priority(&buttons[i + 1]);
	draw_button_edges(ui, interaction, b, true, [left, right])
}

fn draw_button_edges(
	ui: &mut TextShaper,
	interaction: &InteractionState,
	b: &Button,
	panel: bool,
	edges: [bool; 2],
) -> Vec<Draw> {
	let hovered = b.enabled
		&& b.rect.contains(interaction.cursor.0, interaction.cursor.1);
	let pressed = b.enabled && interaction.pressed == Some(b.action) && hovered;
	let primary = b.enabled && b.kind == ButtonKind::Primary;
	let link = b.kind == ButtonKind::Link;
	let quiet = matches!(b.kind, ButtonKind::Quiet | ButtonKind::Link)
		|| b.icon.is_some()
		|| !panel;
	let focused = b.enabled
		&& interaction.focus_visible
		&& interaction.focus == Some(b.action);
	let selected = b.enabled && b.active;
	let styled = |field| Paint::Styled(Condition::Button, field);
	let fill = if primary {
		if pressed || hovered {
			mix(ui, C::Accent, C::Color, if pressed { 0.22 } else { 0.10 })
		} else {
			styled(C::Accent)
		}
	} else if selected && (pressed || hovered) {
		mix(
			ui,
			C::ActiveBackground,
			C::Accent,
			if pressed { 0.22 } else { 0.10 },
		)
	} else if pressed || selected {
		styled(C::ActiveBackground)
	} else if hovered {
		styled(C::HoverBackground)
	} else {
		styled(C::Background)
	};
	let mut out = Vec::new();
	if primary || selected || pressed || hovered || !quiet {
		out.push(Draw::Rect(b.rect, fill));
	}
	// A single edge carries focus, replacing the resting border instead of stacking rings.
	if focused {
		out.extend(outline(
			b.rect,
			if primary { C::Color } else { C::FocusColor },
			2.0,
			edges,
		));
	} else if !quiet && !primary {
		out.extend(outline(
			b.rect,
			if selected { C::Accent } else { C::BorderColor },
			1.0,
			edges,
		));
	}
	let color = if !b.enabled {
		C::DisabledColor
	} else if primary {
		C::Background
	} else if link {
		C::Accent
	} else {
		C::Color
	};

	let old = ui.appearance.clone();
	ui.appearance = ui.stylesheet.text(&old, Condition::Button);
	if let Some(paths) = b.icon {
		out.push(Draw::Icon {
			paths,
			paint: Paint::Styled(Condition::Button, color),
			x: b.rect.x + (b.rect.w - 20.0) / 2.0,
			y: b.rect.y + (b.rect.h - 20.0) / 2.0,
			size: 20.0,
		});
	} else {
		let room = if b.marker.is_some() {
			MARKER + 4.0
		} else {
			0.0
		};
		let text = ui.fit(b.label, 13.0, (b.rect.w - 8.0 - room).max(0.0));
		let width = ui.text_width(&text, 13.0);
		let x = if link {
			b.rect.x
		} else {
			b.rect.x + (b.rect.w - width - room) / 2.0
		};
		if link {
			out.push(line(
				Rect {
					x,
					y: b.rect.y + b.rect.h / 2.0 + 7.0,
					w: width,
					h: 1.0,
				},
				Condition::Button,
				color,
			));
		}
		out.extend(ui.label(
			&text,
			13.0,
			x,
			b.rect.y + b.rect.h / 2.0 + 4.5,
			Paint::Styled(Condition::Button, color),
		));
		if let Some(paths) = b.marker {
			out.push(Draw::Icon {
				paths,
				paint: Paint::Styled(Condition::Button, color),
				x: x + width + 4.0,
				y: b.rect.y + (b.rect.h - MARKER) / 2.0,
				size: MARKER,
			});
		}
	}
	ui.appearance = old;
	out
}

fn outline(r: Rect, color: C, thickness: f32, edges: [bool; 2]) -> Vec<Draw> {
	[
		Rect { h: thickness, ..r },
		Rect {
			y: r.y + r.h - thickness,
			h: thickness,
			..r
		},
		Rect { w: thickness, ..r },
		Rect {
			x: r.x + r.w - thickness,
			w: thickness,
			..r
		},
	]
	.into_iter()
	.enumerate()
	.filter(|(i, _)| *i < 2 || edges[*i - 2])
	.map(|(_, r)| line(r, Condition::Button, color))
	.collect()
}

/// Derive interaction shades from the effective stylesheet, including custom themes.
fn mix(ui: &TextShaper, from: C, to: C, amount: f32) -> Paint {
	let from = ui.stylesheet.color(Condition::Button, from);
	let to = ui.stylesheet.color(Condition::Button, to);
	Paint::Color(Color(u32::from_be_bytes(std::array::from_fn(|i| {
		((from[i] + (to[i] - from[i]) * amount) * 255.0).round() as u8
	}))))
}

pub(super) struct Action {
	pub label: &'static str,
	pub action: Command,
	pub active: bool,
}
pub(super) fn action(
	label: &'static str,
	active: bool,
	action: Command,
) -> Action {
	Action {
		label,
		active,
		action,
	}
}
pub(super) struct Row {
	icon: Option<&'static [markview_core::scene::IconPath]>,
	pub label: String,
	pub actions: Vec<Action>,
	pub section: Option<&'static str>,
	pub value: Option<String>,
	link: Option<(&'static str, Command)>,
	/// The options this row offers in a list instead of in place.
	menu: Option<RowMenu>,
}
/// A row's options, and the control whose list they belong to.
pub(super) struct RowMenu {
	pub(super) id: DropdownId,
	pub(super) entries: Vec<Action>,
}
impl RowMenu {
	/// The option the row shows while its list is closed: the current one, or
	/// the first when the reader has not chosen any.
	fn chosen(&self) -> Option<&Action> {
		self.entries
			.iter()
			.find(|entry| entry.active)
			.or_else(|| self.entries.first())
	}
}
impl Row {
	pub fn new(label: impl Into<String>, actions: Vec<Action>) -> Self {
		Self {
			icon: None,
			label: label.into(),
			actions,
			section: None,
			value: None,
			link: None,
			menu: None,
		}
	}
	/// Offers `entries` in a list the row's own control opens, showing the
	/// current one where a value would go.
	pub fn menu(mut self, id: DropdownId, entries: Vec<Action>) -> Self {
		self.menu = Some(RowMenu { id, entries });
		self
	}
	pub fn icon(paths: &'static [markview_core::scene::IconPath]) -> Self {
		Self {
			icon: Some(paths),
			..Self::new("", vec![])
		}
	}
	pub fn link(label: &'static str, command: Command) -> Self {
		Self {
			link: Some((label, command)),
			..Self::new("", vec![])
		}
	}
	fn height(&self) -> f32 {
		if self.icon.is_some() {
			72.0
		} else if self.actions.is_empty()
			&& self.value.is_none()
			&& self.menu.is_none()
		{
			26.0
		} else {
			ROW
		}
	}
	pub fn section(mut self, title: &'static str) -> Self {
		self.section = Some(title);
		self
	}
	pub fn value(mut self, value: impl Into<String>) -> Self {
		self.value = Some(value.into());
		self
	}
}

/// A form keeps every control for keyboard traversal and clips only pointer input.
pub(in crate::app) struct Form {
	header: bool,
	preview: bool,
	/// The language its own controls are labelled in.
	lang: Lang,
	pub rect: Rect,
	pub viewport: Rect,
	pub scroll: f32,
	pub max_scroll: f32,
	pub buttons: Vec<Button>,
	body_start: usize,
	body_end: usize,
	rows: Vec<(Row, f32)>,
	/// The control each list row opens its options from.
	menus: Vec<(DropdownId, Rect)>,
}
impl Form {
	pub(super) fn new(
		width: f32,
		height: f32,
		scroll: f32,
		rows: Vec<Row>,
		close: Option<Command>,
		spacious_header: bool,
		lang: Lang,
	) -> Self {
		let rect = panel_rect(width, height);
		let spacious_header = spacious_header && rect.h >= 300.0;
		let viewport = Rect {
			x: rect.x + INSET,
			y: rect.y + if spacious_header { 120.0 } else { 88.0 },
			w: rect.w - INSET * 2.0,
			h: rect.h - if spacious_header { 184.0 } else { 152.0 },
		};
		let content = 16.0
			+ rows.iter().map(Row::height).sum::<f32>()
			+ rows.iter().filter(|r| r.section.is_some()).count() as f32
				* SECTION;
		let max_scroll = (content - viewport.h).max(0.0);
		let scroll = scroll.clamp(0.0, max_scroll);
		let body_start = usize::from(close.is_some());
		let mut buttons = close.map_or_else(Vec::new, |action| {
			let mut close = button(
				lang.panel_close(),
				action,
				Rect {
					x: rect.x + rect.w - INSET - CONTROL,
					y: rect.y + 18.0,
					w: CONTROL,
					h: CONTROL,
				},
			);
			close.icon = Some(icons::CLOSE);
			vec![close]
		});
		let mut y = viewport.y + 8.0 - scroll;
		let mut placed = Vec::new();
		let mut menus = Vec::new();
		for row in rows {
			if row.section.is_some() {
				y += SECTION;
			}
			if let Some((label, command)) = row.link {
				let mut link = button(
					label,
					command,
					Rect {
						x: viewport.x,
						y,
						w: viewport.w,
						h: row.height(),
					},
				);
				link.kind = ButtonKind::Link;
				buttons.push(link);
			}
			let right = viewport.x + viewport.w - 8.0;
			let w = 232.0_f32.min(viewport.w * 0.56);
			// A list row answers with one control the width of the whole
			// control area, showing the option in force.
			if let Some(menu) = &row.menu
				&& let Some(chosen) = menu.chosen()
			{
				let rect = Rect {
					x: right - w,
					y,
					w,
					h: CONTROL,
				};
				let mut b = button(
					chosen.label,
					Command::ToggleDropdown(
						menu.id,
						menu.entries
							.iter()
							.position(|entry| entry.active)
							.unwrap_or(0),
					),
					rect,
				);
				b.marker = Some(icons::CHEVRON);
				buttons.push(b);
				menus.push((menu.id, rect));
			}
			let count = row.actions.len();
			for (i, entry) in row.actions.iter().enumerate() {
				let (x, w) = if row.value.is_some() {
					(if i == 0 { right - w } else { right - CONTROL }, CONTROL)
				} else {
					let slot = w / count as f32;
					(right - w + i as f32 * slot, slot)
				};
				let mut b = button(
					entry.label,
					entry.action,
					Rect {
						x,
						y,
						w,
						h: CONTROL,
					},
				);
				b.active = entry.active;
				buttons.push(b);
			}
			let height = row.height();
			placed.push((row, y));
			y += height;
		}
		let body_end = buttons.len();
		Self {
			header: true,
			preview: false,
			lang,
			rect,
			viewport,
			scroll,
			max_scroll,
			buttons,
			body_start,
			body_end,
			rows: placed,
			menus,
		}
	}
	pub(super) fn preview_control(&mut self) {
		let close = self.buttons[0].rect;
		let mut eye = button(
			self.lang.panel_preview(),
			Command::SettingsPreview,
			Rect {
				x: close.x - CONTROL - 8.0,
				..close
			},
		);
		eye.icon = Some(icons::EYE);
		self.buttons.insert(self.body_start, eye);
		self.body_start += 1;
		self.body_end += 1;
	}
	pub(super) fn without_header(mut self) -> Self {
		self.header = false;
		self
	}
	pub(super) fn preview(mut self, enabled: bool) -> Self {
		self.preview = enabled;
		for b in &mut self.buttons {
			if b.action == Command::SettingsPreview {
				b.active = enabled;
				b.label = if enabled {
					self.lang.panel_exit_preview()
				} else {
					self.lang.panel_preview()
				};
				b.icon =
					Some(if enabled { icons::EYE_OFF } else { icons::EYE });
			}
		}
		self
	}

	pub(super) fn footer(
		&mut self,
		ui: &mut TextShaper,
		entries: &[(&'static str, Command, ButtonKind)],
	) {
		let mut right = self.rect.x + self.rect.w - INSET;
		for &(text, action, kind) in entries.iter().rev() {
			let w = if action == Command::CopyDiagnostics {
				CONTROL
			} else {
				super::controls::button_width(ui, text, 13.0) + 6.0
			};
			let mut b = button(
				text,
				action,
				Rect {
					x: right - w,
					y: self.rect.y + self.rect.h - 48.0,
					w,
					h: CONTROL,
				},
			);
			b.kind = kind;
			self.buttons.insert(self.body_end, b);
			right -= w + 8.0;
		}
	}
	pub fn visible_buttons(&self) -> Vec<Button> {
		self.buttons
			.iter()
			.enumerate()
			.filter_map(|(i, b)| {
				if !self.header
					&& matches!(
						b.action,
						Command::Settings | Command::SettingsPreview
					) {
					return None;
				}
				if !b.enabled {
					return None;
				}
				let mut b = b.clone();
				if (self.body_start..self.body_end).contains(&i) {
					b.rect = b.rect.intersect(self.viewport)?;
				}
				Some(b)
			})
			.collect()
	}
	pub fn reveal(&self, action: Command) -> f32 {
		let Some(b) = self.buttons[self.body_start..self.body_end]
			.iter()
			.find(|b| b.action == action)
		else {
			return self.scroll;
		};
		let dy = if b.rect.y < self.viewport.y {
			b.rect.y - self.viewport.y
		} else {
			(b.rect.y + b.rect.h - self.viewport.y - self.viewport.h).max(0.0)
		};
		(self.scroll + dy).clamp(0.0, self.max_scroll)
	}
	pub fn scrollbar(&self, ui: &TextShaper) -> Option<Scrollbar> {
		Scrollbar::vertical(
			Rect {
				x: self.rect.x + self.rect.w - 16.0,
				w: 12.0,
				..self.viewport
			},
			self.scroll,
			self.viewport.h + self.max_scroll,
			self.viewport.h,
			ui.stylesheet.scrollbar_metrics(),
		)
	}
	pub(super) fn draw(
		&self,
		ui: &mut TextShaper,
		interaction: &InteractionState,
		title: &str,
		detail: &str,
		detail_color: C,
		size: (f32, f32),
	) -> Vec<Draw> {
		appearance(ui);
		let mut out = if self.preview {
			vec![line(self.rect, Condition::Panel, C::Background)]
		} else {
			frame(self.rect, size.0, size.1)
		};
		let text_rect = Rect {
			x: self.rect.x + INSET,
			y: self.rect.y + 16.0,
			w: self.rect.w
				- 2.0 * INSET
				- (self.body_start as f32 * 40.0 + 4.0),
			h: 32.0,
		};
		let weight = ui.appearance.weight;
		ui.appearance.weight = 700;
		if !title.is_empty() {
			out.extend(label(ui, title, 20.0, text_rect, C::Color));
		}
		ui.appearance.weight = weight;
		out.extend(label(
			ui,
			detail,
			12.0,
			Rect {
				y: self.viewport.y - 36.0,
				h: 20.0,
				w: self.rect.w - 2.0 * INSET,
				..text_rect
			},
			detail_color,
		));
		for y in [self.viewport.y - 1.0, self.viewport.y + self.viewport.h] {
			out.push(line(
				Rect {
					x: self.rect.x + 1.0,
					y,
					w: self.rect.w - 2.0,
					h: 1.0,
				},
				Condition::Panel,
				C::BorderColor,
			));
		}
		let mut body = Vec::new();
		for (row, y) in &self.rows {
			if y + row.height().max(CONTROL) < self.viewport.y
				|| y - SECTION > self.viewport.y + self.viewport.h
			{
				continue;
			}
			if let Some(paths) = row.icon {
				body.push(Draw::Icon {
					paths,
					paint: Paint::Styled(Condition::Panel, C::Color),
					x: self.viewport.x + (self.viewport.w - 64.0) / 2.0,
					y: *y,
					size: 64.0,
				});
				continue;
			}
			if let Some(section) = row.section {
				ui.appearance.weight = 700;
				body.extend(label(
					ui,
					section,
					12.0,
					Rect {
						y: y - SECTION
							+ if row.height() < ROW { 14.0 } else { 0.0 },
						h: 24.0,
						..self.viewport
					},
					C::Muted,
				));
				ui.appearance.weight = weight;
			}
			let control_width = 232.0_f32.min(self.viewport.w * 0.56);
			body.extend(label(
				ui,
				&row.label,
				13.0,
				Rect {
					y: *y,
					w: if row.actions.is_empty()
						&& row.value.is_none()
						&& row.menu.is_none()
					{
						self.viewport.w
					} else {
						self.viewport.w - control_width - 20.0
					},
					h: CONTROL,
					..self.viewport
				},
				C::Color,
			));
			if let Some(value) = &row.value {
				let r = Rect {
					x: self.viewport.x + self.viewport.w - 8.0 - control_width
						+ CONTROL,
					y: *y,
					w: control_width - CONTROL * 2.0,
					h: CONTROL,
				};
				let fitted = ui.fit(value, 13.0, r.w - 8.0);
				let x = r.x + (r.w - ui.text_width(&fitted, 13.0)) / 2.0;
				body.extend(ui.label(
					&fitted,
					13.0,
					x,
					r.y + 20.5,
					Paint::Styled(Condition::Panel, C::Color),
				));
			}
		}
		let body_interaction = InteractionState {
			cursor: if self
				.viewport
				.contains(interaction.cursor.0, interaction.cursor.1)
			{
				interaction.cursor
			} else {
				(f32::NEG_INFINITY, f32::NEG_INFINITY)
			},
			focus: interaction.focus,
			focus_visible: interaction.focus_visible,
			pressed: interaction.pressed,
			..Default::default()
		};

		for (i, b) in self.buttons.iter().enumerate() {
			if !self.header
				&& matches!(
					b.action,
					Command::Settings | Command::SettingsPreview
				) {
				continue;
			}
			if self.preview && b.action == Command::SettingsPreview {
				continue;
			}
			if (self.body_start..self.body_end).contains(&i) {
				if b.rect.intersect(self.viewport).is_some() {
					body.extend(draw_segmented_button(
						ui,
						&body_interaction,
						&self.buttons[self.body_start..self.body_end],
						i - self.body_start,
					));
				}
			} else {
				out.extend(draw_button(ui, interaction, b, true));
			}
		}
		out.push(Draw::Clipped {
			rect: self.viewport,
			draws: body,
		});
		if let Some(bar) = self.scrollbar(ui) {
			let hovered = bar.hit(interaction.cursor.0, interaction.cursor.1);
			let (track, thumb) =
				bar.bars(hovered || interaction.panel_grab.is_some());
			out.push(line(track, Condition::Scrollbar, C::Track));
			out.push(line(
				thumb,
				Condition::Scrollbar,
				if hovered { C::ThumbHover } else { C::Thumb },
			));
		}
		if self.preview {
			fade(&mut out, ui, 0.25);
			// Keep the exit control legible while the rest of the panel recedes.
			if self.header
				&& let Some(eye) = self
					.buttons
					.iter()
					.find(|b| b.action == Command::SettingsPreview)
			{
				out.extend(draw_button(ui, interaction, eye, true));
			}
		}

		out
	}
}

/// An open option list: where it sits, and the buttons that answer for it.
///
/// The same value carries the geometry, the painting and the hit testing, so
/// the list cannot answer a click somewhere other than where it is drawn.
pub(in crate::app) struct Menu {
	pub(in crate::app) rect: Rect,
	pub(in crate::app) buttons: Vec<Button>,
	/// How many options the row holds, drawn or not.
	pub(in crate::app) options: usize,
	/// The option the keyboard is on, counted over every option the list holds.
	pub(super) highlight: usize,
	/// The first option drawn.
	pub(super) offset: usize,
}
impl Menu {
	/// The command the keyboard would commit.
	pub(in crate::app) fn chosen(&self) -> Option<Command> {
		self.buttons
			.get(self.highlight.saturating_sub(self.offset))
			.map(|button| button.action)
	}
}

impl Form {
	/// The list `dropdown` has open, when this form holds that row.
	///
	/// Measuring is what decides which options are drawn, so it is also where
	/// the highlight is brought into that window. A list opened on its last
	/// option therefore shows that option rather than starting from the top,
	/// and the option under the keyboard is always one the reader can see.
	pub(super) fn menu(
		&self,
		dropdown: &mut Dropdown,
		size: (f32, f32),
	) -> Option<Menu> {
		let anchor = self
			.menus
			.iter()
			.find(|(id, _)| *id == dropdown.id)
			.map(|(_, rect)| *rect)
			// A row the page scrolled away holds no list: its anchor is gone,
			// so the list must not hang from where the control used to be.
			.filter(|rect| rect.intersect(self.viewport).is_some())?;
		let entries = self.rows.iter().find_map(|(row, _)| {
			row.menu
				.as_ref()
				.filter(|menu| menu.id == dropdown.id)
				.map(|menu| menu.entries.as_slice())
		})?;
		let (rect, shown) = menu_rect(anchor, entries.len(), size);
		dropdown.follow(shown);
		let offset = dropdown.offset;
		let buttons = entries
			.iter()
			.skip(offset)
			.take(shown)
			.enumerate()
			.map(|(slot, entry)| {
				let mut b = button(
					entry.label,
					entry.action,
					Rect {
						x: rect.x + MENU_PAD,
						y: rect.y + MENU_PAD + slot as f32 * OPTION,
						w: rect.w - 2.0 * MENU_PAD,
						h: OPTION - 2.0,
					},
				);
				// Options read as a flat list: only the pointer and the current
				// choice give them a fill.
				b.kind = ButtonKind::Quiet;
				b.active = entry.active;
				b
			})
			.collect();
		Some(Menu {
			rect,
			buttons,
			options: entries.len(),
			highlight: dropdown.highlight,
			offset,
		})
	}
}

/// Where an option list sits, and how many of its options are drawn.
///
/// It stands under its control, or above it when the window's bottom edge is
/// the nearer one. It never leaves the window: where neither side holds every
/// option, the longer side shows as many as it can and the rest follow the
/// highlight within that window.
fn menu_rect(anchor: Rect, count: usize, size: (f32, f32)) -> (Rect, usize) {
	let (_, height) = size;
	let room = |available: f32| {
		((available - 2.0 * MENU_PAD) / OPTION).floor().max(0.0) as usize
	};
	let below = room(height - MENU_PAD - (anchor.y + anchor.h + MENU_GAP));
	let above = room(anchor.y - MENU_GAP - MENU_PAD);
	let (shown, downwards) = if below >= count {
		(count, true)
	} else if above >= count {
		(count, false)
	} else if below >= above {
		(below, true)
	} else {
		(above, false)
	};
	let shown = shown.max(1);
	let h = shown as f32 * OPTION + 2.0 * MENU_PAD;
	let y = if downwards {
		anchor.y + anchor.h + MENU_GAP
	} else {
		anchor.y - MENU_GAP - h
	};
	(
		Rect {
			x: anchor.x,
			y,
			w: anchor.w,
			h,
		},
		shown,
	)
}

/// Draws an open option list over the page behind it.
pub(in crate::app) fn draw_menu(
	ui: &mut TextShaper,
	interaction: &InteractionState,
	menu: &Menu,
) -> Vec<Draw> {
	let mut out = vec![Draw::Rect(
		menu.rect,
		Paint::Styled(Condition::Panel, C::Background),
	)];
	out.extend(outline(menu.rect, C::BorderColor, 1.0, [true, true]));
	// A list always has an option under the keyboard, so it wears the focus
	// ring whether the pointer put it there or the arrow keys did.
	let targeted = InteractionState {
		cursor: interaction.cursor,
		pressed: interaction.pressed,
		focus: menu.chosen(),
		focus_visible: true,
		..Default::default()
	};
	for b in &menu.buttons {
		out.extend(draw_button(ui, &targeted, b, true));
	}
	out
}

/// Forms contain only painted vectors and clipped groups; document assets stay outside.
pub(in crate::app) fn fade(draws: &mut [Draw], ui: &TextShaper, opacity: f32) {
	for draw in draws {
		let paint = match draw {
			Draw::Clipped { draws, .. } => {
				fade(draws, ui, opacity);
				continue;
			}
			Draw::Glyph(glyph) => &mut glyph.paint,
			Draw::Rect(_, paint)
			| Draw::Icon { paint, .. }
			| Draw::Polygon { paint, .. }
			| Draw::Math { paint, .. } => paint,
			Draw::Box { .. } | Draw::Image { .. } => continue,
		};
		let mut rgba = ui.stylesheet.paint(*paint);
		rgba[3] *= opacity;
		*paint = Paint::Color(Color(u32::from_be_bytes(
			rgba.map(|channel| (channel * 255.0).round() as u8),
		)));
	}
}

#[cfg(test)]
#[path = "components_tests.rs"]
mod tests;
