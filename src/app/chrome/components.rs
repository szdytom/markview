//! Shared, concrete chrome components. Geometry owns painting and input alike.
use super::icons;
use crate::{
	app::Button,
	layout::{Draw, Paint, Rect, Scrollbar, TextShaper},
	state::{Command, InteractionState, PanelTab},
};
use markview_core::style::{Color, ColorField as C, Condition, TextAppearance};

pub(in crate::app) const CONTROL: f32 = 32.0;
pub(super) const INSET: f32 = 24.0;
const TAB_Y: f32 = 48.0;
const ROW: f32 = 44.0;
const SECTION: f32 = 32.0;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::app) enum ButtonKind {
	#[default]
	Standard,
	Quiet,
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

/// The settings panel's three pages, as one row of tabs below its title.
pub(in crate::app) fn tab_controls(
	rect: Rect,
	current: PanelTab,
) -> Vec<Button> {
	const TABS: [(&str, PanelTab); 3] = [
		("Generic", PanelTab::Generic),
		("Styles", PanelTab::Styles),
		("Fonts", PanelTab::Fonts),
	];
	let mut out = Vec::new();
	for (index, (label, tab)) in TABS.iter().enumerate() {
		let mut b = button(
			label,
			Command::SettingsTab(*tab),
			Rect {
				x: rect.x + INSET + index as f32 * 110.0,
				y: rect.y + TAB_Y,
				w: 104.0,
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
) -> Vec<Draw> {
	let mut out = Vec::new();
	for b in tab_controls(rect, current) {
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
) -> Vec<Button> {
	let mut out = tab_controls(rect, current);
	let close = button(
		"Close",
		Command::Settings,
		Rect {
			x: rect.x + rect.w - INSET - CONTROL,
			y: rect.y + 18.0,
			w: CONTROL,
			h: CONTROL,
		},
	);
	let mut eye = button(
		"Preview document",
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
) -> Vec<Draw> {
	appearance(ui);
	let old_weight = ui.appearance.weight;
	ui.appearance.weight = 700;
	let mut out = label(
		ui,
		"Settings",
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
	out.extend(draw_tabs(ui, interaction, rect, current));
	let controls = settings_header_controls(rect, current, preview);
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
	let hovered = b.enabled
		&& b.rect.contains(interaction.cursor.0, interaction.cursor.1);
	let pressed = b.enabled && interaction.pressed == Some(b.action) && hovered;
	let primary = b.enabled && b.kind == ButtonKind::Primary;
	let quiet = b.kind == ButtonKind::Quiet || b.icon.is_some() || !panel;
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
		));
	} else if !quiet && !primary {
		out.extend(outline(
			b.rect,
			if selected { C::Accent } else { C::BorderColor },
			1.0,
		));
	}
	let color = if !b.enabled {
		C::DisabledColor
	} else if primary {
		C::Background
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
		let text = ui.fit(b.label, 13.0, (b.rect.w - 8.0).max(0.0));
		let x = b.rect.x + (b.rect.w - ui.text_width(&text, 13.0)) / 2.0;
		out.extend(ui.label(
			&text,
			13.0,
			x,
			b.rect.y + b.rect.h / 2.0 + 4.5,
			Paint::Styled(Condition::Button, color),
		));
	}
	ui.appearance = old;
	out
}

fn outline(r: Rect, color: C, thickness: f32) -> Vec<Draw> {
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
	.map(|r| line(r, Condition::Button, color))
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
	pub label: String,
	pub actions: Vec<Action>,
	pub section: Option<&'static str>,
	pub value: Option<String>,
}
impl Row {
	pub fn new(label: impl Into<String>, actions: Vec<Action>) -> Self {
		Self {
			label: label.into(),
			actions,
			section: None,
			value: None,
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
	pub rect: Rect,
	pub viewport: Rect,
	pub scroll: f32,
	pub max_scroll: f32,
	pub buttons: Vec<Button>,
	body_start: usize,
	body_end: usize,
	rows: Vec<(Row, f32)>,
}
impl Form {
	pub(super) fn new(
		width: f32,
		height: f32,
		scroll: f32,
		rows: Vec<Row>,
		close: Option<Command>,
		spacious_header: bool,
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
			+ rows.len() as f32 * ROW
			+ rows.iter().filter(|r| r.section.is_some()).count() as f32
				* SECTION;
		let max_scroll = (content - viewport.h).max(0.0);
		let scroll = scroll.clamp(0.0, max_scroll);
		let body_start = usize::from(close.is_some());
		let mut buttons = close.map_or_else(Vec::new, |action| {
			let mut close = button(
				"Close",
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
		for row in rows {
			if row.section.is_some() {
				y += SECTION;
			}
			let right = viewport.x + viewport.w - 8.0;
			let w = 232.0_f32.min(viewport.w * 0.56);
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
			placed.push((row, y));
			y += ROW;
		}
		let body_end = buttons.len();
		Self {
			header: true,
			preview: false,
			rect,
			viewport,
			scroll,
			max_scroll,
			buttons,
			body_start,
			body_end,
			rows: placed,
		}
	}
	pub(super) fn preview_control(&mut self) {
		let close = self.buttons[0].rect;
		let mut eye = button(
			"Preview document",
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
					"Exit preview"
				} else {
					"Preview document"
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
			let w = super::controls::button_width(ui, text, 13.0) + 6.0;
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
			if y + ROW < self.viewport.y
				|| y - SECTION > self.viewport.y + self.viewport.h
			{
				continue;
			}
			if let Some(section) = row.section {
				ui.appearance.weight = 700;
				body.extend(label(
					ui,
					section,
					12.0,
					Rect {
						y: y - SECTION,
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
					w: self.viewport.w - control_width - 20.0,
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
					body.extend(draw_button(ui, &body_interaction, b, true));
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
