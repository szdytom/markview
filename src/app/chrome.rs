//! Reader chrome built from borrowed display state, with no window or worker access.
pub(super) mod components;
mod controls;
mod export;
mod footer;
#[cfg(test)]
mod gpu_tests;
mod icons;
mod modal;
mod styles;
mod tabs;
use super::{BOTTOM, Button, TOP};
use crate::{
	layout::{Draw, Paint, Rect, Scrollbar, TextShaper},
	settings::{ExportSettings, ReaderSettings},
	state::{
		Command, InteractionState, ReaderSession, ReaderTab, ScrollbarAxis,
	},
};
pub(super) use components::panel_rect;
use controls::{draw_controls, toolbar_controls};
use footer::draw_footer;
use markview_core::style::{ColorField as C, Condition, TextAppearance};
use std::time::Instant;
pub(super) use styles::styles_rect;
use styles::{StylesTarget, draw_styles, style_controls};

/// Height of the remote-image notice strip below the tab bar.
pub(super) const BANNER: f32 = 34.0;

/// Top of the document area; the notice strip pushes it down.
pub(in crate::app) fn content_top(notice: bool) -> f32 {
	TOP + if notice { BANNER } else { 0.0 }
}

fn banner_rect(width: f32) -> Rect {
	Rect {
		x: 0.0,
		y: TOP,
		w: width,
		h: BANNER,
	}
}

fn ui_appearance(shaper: &TextShaper) -> TextAppearance {
	shaper
		.stylesheet
		.text(&TextAppearance::default(), Condition::Ui)
}

fn banner_buttons(shaper: &mut TextShaper, width: f32) -> Vec<Button> {
	let old = shaper.appearance.clone();
	shaper.appearance = ui_appearance(shaper);
	let dismiss = shaper.text_width("Dismiss", 13.0) + 22.0;
	let load = shaper.text_width("Load all", 13.0) + 22.0;
	shaper.appearance = old;
	let y = TOP + (BANNER - 22.0) / 2.0;
	vec![
		Button {
			label: "Dismiss",
			icon: None,
			active: false,
			kind: Default::default(),
			enabled: true,
			action: Command::RemoteDismiss,
			rect: Rect {
				x: width - 16.0 - dismiss - load - 8.0,
				y,
				w: dismiss,
				h: 22.0,
			},
		},
		Button {
			label: "Load all",
			icon: None,
			active: false,
			kind: Default::default(),
			enabled: true,
			action: Command::RemoteLoadAll,
			rect: Rect {
				x: width - 16.0 - load,
				y,
				w: load,
				h: 22.0,
			},
		},
	]
}

fn draw_banner(
	shaper: &mut TextShaper,
	width: f32,
	deferred: usize,
	interaction: &InteractionState,
) -> Vec<Draw> {
	let rect = banner_rect(width);
	shaper.appearance = shaper
		.stylesheet
		.text(&ui_appearance(shaper), Condition::Statusbar);
	let mut out = vec![
		Draw::Rect(rect, Paint::Styled(Condition::Statusbar, C::Background)),
		Draw::Rect(
			Rect {
				x: 0.0,
				y: TOP + BANNER - 1.0,
				w: width,
				h: 1.0,
			},
			Paint::Styled(Condition::Statusbar, C::BorderColor),
		),
	];
	let buttons = banner_buttons(shaper, width);
	let available = buttons.first().map_or(width - 32.0, |b| b.rect.x - 16.0);
	let label = shaper.fit(
		&format!("{deferred} remote images were not loaded."),
		12.0,
		available,
	);
	out.extend(shaper.label(
		&label,
		12.0,
		16.0,
		TOP + BANNER / 2.0 + 5.0,
		Paint::Styled(Condition::Statusbar, C::Color),
	));
	for mut button in buttons {
		if button.action == Command::RemoteLoadAll {
			button.kind = components::ButtonKind::Primary;
		}
		out.extend(components::draw_button(shaper, interaction, &button, true));
	}
	out
}

fn empty_button(width: f32, height: f32) -> Button {
	let mut b = components::button(
		"Open file…",
		Command::Open,
		Rect {
			x: ((width - 400.0) / 2.0).max(24.0),
			y: (height * 0.4).max(110.0) + 64.0,
			w: 128.0,
			h: 32.0,
		},
	);
	b.kind = components::ButtonKind::Primary;
	b
}

pub(super) struct Chrome<'a> {
	pub(super) ui: &'a mut TextShaper,
	pub(super) session: &'a ReaderSession,
	pub(super) tabs: &'a [ReaderTab],
	pub(super) active_tab: usize,
	pub(super) tab_strip: &'a super::tab_strip::TabStrip,
	pub(super) tab_widths: &'a [(f32, f32)],
	pub(super) settings: &'a ReaderSettings,
	/// The export panel's own settings, drawn but never applied to the reader.
	pub(super) export: &'a ExportSettings,
	pub(super) interaction: &'a InteractionState,
	pub(super) style_entries: &'a [crate::stylesheet::Entry],
	pub(super) style_page: usize,
	/// The stylesheets' font download, as the Styles panel last left it.
	pub(super) fonts: &'a crate::fonts::Status,
	pub(super) width: f32,
	pub(super) height: f32,
	pub(super) scrollbar: Option<Scrollbar>,
	pub(super) warning: Option<&'a str>,
	pub(super) status: &'a str,
	pub(super) status_until: Option<Instant>,
	pub(super) error: bool,
	/// The footer's hover hint: a link target, or an image title.
	pub(super) hover_hint: Option<&'a str>,
	/// Number of remote image sources the loader deferred, if any.
	pub(super) remote_notice: Option<usize>,
	/// Whether an export is rewriting its file on every document change.
	pub(super) watching: bool,
}
impl Chrome<'_> {
	pub(super) fn form(&mut self) -> Option<components::Form> {
		if !self.interaction.panel_open
			|| self.interaction.modal.is_some()
			|| self.interaction.styles_open
			|| self.interaction.export_styles_open
		{
			return None;
		}
		Some(if self.interaction.export_open {
			export::form(
				self.ui,
				self.export,
				self.interaction.export_scroll,
				self.width,
				self.height,
			)
		} else {
			controls::form(
				self.ui,
				self.settings,
				self.interaction.settings_scroll,
				self.width,
				self.height,
			)
			.preview(self.interaction.settings_preview)
		})
	}

	pub(super) fn buttons(&mut self) -> Vec<Button> {
		let (width, height, _) = (self.width, self.height, 1.0);
		if self.interaction.modal.is_some() {
			modal::modal_buttons(self.ui, self.interaction, width, height)
		} else if self.interaction.export_styles_open {
			style_controls(
				StylesTarget::Export,
				Some(&self.export.style),
				self.style_entries,
				self.fonts,
				self.style_page,
				width,
				height,
			)
		} else if self.interaction.export_open {
			export::form(
				self.ui,
				self.export,
				self.interaction.export_scroll,
				width,
				height,
			)
			.visible_buttons()
		} else if self.interaction.styles_open {
			style_controls(
				StylesTarget::Reader,
				self.settings.style.as_deref(),
				self.style_entries,
				self.fonts,
				self.style_page,
				width,
				height,
			)
		} else if self.interaction.panel_open {
			controls::form(
				self.ui,
				self.settings,
				self.interaction.settings_scroll,
				width,
				height,
			)
			.preview(self.interaction.settings_preview)
			.visible_buttons()
		} else {
			let mut buttons = toolbar_controls(width);
			if self.session.path.is_none()
				&& self.session.snapshot.blocks.is_empty()
			{
				// One `Open` command owns keyboard focus; both regions answer the pointer.
				buttons.push(empty_button(width, height));
			}

			if self.remote_notice.is_some() {
				buttons.extend(banner_buttons(self.ui, width));
			}
			buttons
		}
	}
	pub(super) fn overlay(&mut self) -> Vec<Draw> {
		let (width, height, _) = (self.width, self.height, 1.0);
		let mut out = vec![
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
		];
		out.extend(self.tab_bar().draw_tabs());
		out.extend(controls::draw_toolbar(self.ui, self.interaction, width));
		if let Some(deferred) = self.remote_notice {
			out.extend(draw_banner(self.ui, width, deferred, self.interaction));
		}
		let warning = if self.error
			&& self
				.status_until
				.is_none_or(|until| until <= Instant::now())
		{
			Some(self.status)
		} else {
			self.warning
		};
		out.extend(draw_footer(
			self.ui,
			(!self.session.layout_pending).then_some(self.session.counts),
			self.interaction.selection_counts.map(|(_, counts)| counts),
			warning,
			if self
				.status_until
				.is_some_and(|until| until > Instant::now())
			{
				self.status
			} else {
				self.hover_hint.unwrap_or("")
			},
			width,
			height,
		));
		if self.session.snapshot.blocks.is_empty() {
			let button = empty_button(width, height);
			let y = button.rect.y - 64.0;
			let (title, detail) = if self.session.path.is_none() {
				(
					"Open a Markdown file",
					"Open a Markdown file, or drop one into this window.",
				)
			} else if self.error {
				(
					"Unable to read this file",
					"Check the file path and access permissions.",
				)
			} else if self.session.layout_pending
				|| self.session.document.is_none()
			{
				("Opening document…", "Preparing the first page…")
			} else {
				(
					"The document is empty",
					"Content will appear here when the file changes.",
				)
			};
			self.ui.appearance = ui_appearance(self.ui);
			self.ui.appearance.weight = 700;
			out.extend(self.ui.label(
				title,
				26.0,
				button.rect.x,
				y,
				Paint::Styled(Condition::Ui, C::Color),
			));
			self.ui.appearance = ui_appearance(self.ui);
			let detail =
				self.ui.fit(detail, 13.0, width - button.rect.x - 24.0);
			out.extend(self.ui.label(
				&detail,
				13.0,
				button.rect.x,
				y + 32.0,
				Paint::Styled(Condition::Ui, C::Muted),
			));
			if self.session.path.is_none() {
				out.extend(components::draw_button(
					self.ui,
					self.interaction,
					&button,
					true,
				));
			}
		}

		if let Some(bar) = self.scrollbar {
			let held = self
				.interaction
				.scrollbar
				.is_some_and(|drag| drag.target == ScrollbarAxis::Document);
			let (x, y) = self.interaction.cursor;
			// Hovering anywhere on the bar thickens it; only the thumb itself
			// takes the hover color.
			let (track, thumb) = bar.bars(held || bar.hit(x, y));
			out.push(Draw::Rect(
				track,
				Paint::Styled(Condition::Scrollbar, C::Track),
			));
			out.push(Draw::Rect(
				thumb,
				Paint::Styled(
					Condition::Scrollbar,
					if held || bar.on_thumb(x, y) {
						C::ThumbHover
					} else {
						C::Thumb
					},
				),
			));
		}
		if self.interaction.export_styles_open {
			out.extend(draw_styles(
				self.ui,
				StylesTarget::Export,
				Some(&self.export.style),
				self.interaction,
				self.style_entries,
				self.fonts,
				self.style_page,
				width,
				height,
			));
		} else if self.interaction.export_open {
			let document = self
				.session
				.path
				.as_deref()
				.and_then(|path| path.file_name())
				.map(|name| name.to_string_lossy().into_owned())
				.unwrap_or_else(|| "Untitled".into());
			out.extend(export::draw_export(
				self.ui,
				self.export,
				self.interaction,
				&document,
				self.watching,
				width,
				height,
			));
		} else if self.interaction.styles_open {
			out.extend(draw_styles(
				self.ui,
				StylesTarget::Reader,
				self.settings.style.as_deref(),
				self.interaction,
				self.style_entries,
				self.fonts,
				self.style_page,
				width,
				height,
			));
		} else if self.interaction.panel_open {
			out.extend(draw_controls(
				self.ui,
				self.settings,
				self.interaction,
				width,
				height,
			));
		}
		// A confirmation owns the frame; nothing behind it is interactive.
		if self.interaction.modal.is_some() {
			out.extend(modal::draw_modal(
				self.ui,
				self.interaction,
				width,
				height,
			));
		}
		out
	}

	pub(super) fn tab_bar(&mut self) -> tabs::TabBar<'_> {
		tabs::TabBar {
			ui: self.ui,
			strip: self.tab_strip,
			widths: self.tab_widths,
			tabs: self.tabs,
			active_tab: self.active_tab,
			cursor: self.interaction.cursor,
			width: self.width,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn content_top_reserves_the_notice_strip() {
		assert_eq!(content_top(false), TOP);
		assert_eq!(content_top(true), TOP + BANNER);
	}
	#[test]
	fn banner_buttons_fit_between_the_toolbar_and_the_document() {
		let mut shaper = crate::test_support::shaper();
		for width in [420.0, 500.0, 1200.0] {
			let buttons = banner_buttons(&mut shaper, width);
			assert_eq!(buttons.len(), 2);
			assert_eq!(buttons[0].action, Command::RemoteDismiss);
			assert_eq!(buttons[1].action, Command::RemoteLoadAll);
			for button in &buttons {
				assert!(button.rect.x >= 0.0);
				assert!(button.rect.x + button.rect.w <= width);
				assert!(button.rect.y >= TOP);
				assert!(button.rect.y + button.rect.h <= TOP + BANNER);
			}
			assert!(buttons[0].rect.x + buttons[0].rect.w < buttons[1].rect.x);
		}
	}
}
