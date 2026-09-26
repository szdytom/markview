//! Reader chrome built from borrowed display state, with no window or worker access.
pub(super) mod components;
mod controls;
mod export;
use super::font_panel::view as fonts;
mod footer;
#[cfg(test)]
mod gpu_tests;
pub(super) mod icons;
pub(in crate::app) mod list;
mod modal;
pub(in crate::app) mod outline;
pub(in crate::app) mod styles;
mod tabs;
use super::{BOTTOM, Button, TOP};
use crate::{
	lang::Lang,
	layout::{Draw, Paint, Rect, Scrollbar, TextShaper},
	settings::{ExportSettings, ReaderSettings},
	state::{
		Command, InteractionState, ReaderSession, ReaderTab, ScrollbarAxis,
	},
};
pub(super) use components::panel_rect;
use controls::{draw_controls, toolbar_controls};
use fonts::draw_fonts;
use footer::draw_footer;
use markview_core::style::{ColorField as C, Condition, TextAppearance};
use std::time::Instant;
use styles::{StylesTarget, draw_styles, style_controls, style_rows};

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

fn banner_buttons(
	shaper: &mut TextShaper,
	width: f32,
	lang: Lang,
) -> Vec<Button> {
	let old = shaper.appearance.clone();
	shaper.appearance = ui_appearance(shaper);
	let dismiss = shaper.text_width(lang.notice_dismiss(), 13.0) + 22.0;
	let load = shaper.text_width(lang.notice_load_all(), 13.0) + 22.0;
	shaper.appearance = old;
	let y = TOP + (BANNER - 22.0) / 2.0;
	vec![
		Button {
			label: lang.notice_dismiss(),
			icon: None,
			marker: None,
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
			label: lang.notice_load_all(),
			icon: None,
			marker: None,
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
	lang: Lang,
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
	let buttons = banner_buttons(shaper, width, lang);
	let available = buttons.first().map_or(width - 32.0, |b| b.rect.x - 16.0);
	let label =
		shaper.fit(&lang.notice_remote_images(deferred), 12.0, available);
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

fn empty_button(width: f32, height: f32, lang: Lang) -> Button {
	let mut b = components::button(
		lang.empty_open_file(),
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

/// The Styles page's controls: its fixed header and footer, and the list's
/// row buttons clipped to the rows the frame shows.
fn style_page_buttons(
	target: StylesTarget,
	selected: Option<&[String]>,
	entries: &[crate::stylesheet::Entry],
	scroll: f32,
	preview: bool,
	size: (f32, f32),
	lang: Lang,
) -> Vec<Button> {
	let (width, height) = size;
	let list = styles::list(width, height, entries.len(), scroll);
	let mut buttons =
		style_controls(target, selected, preview, width, height, lang);
	buttons.extend(list.hit(style_rows(target, selected, entries, list, lang)));
	buttons
}

pub(super) struct Chrome<'a> {
	pub(super) input_draws: Vec<Draw>,
	pub(super) backend: Option<wgpu::Backend>,
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
	/// The Styles page's list offset.
	pub(super) style_scroll: f32,
	pub(super) fonts: super::font_panel::View<'a>,
	pub(super) width: f32,
	pub(super) height: f32,
	pub(super) scrollbar: Option<Scrollbar>,
	pub(super) warning: Option<std::borrow::Cow<'a, str>>,
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
	/// The open option list, measured against the page that holds its row.
	pub(in crate::app) fn dropdown_menu(
		&mut self,
		open: &mut crate::state::Dropdown,
	) -> Option<components::Menu> {
		if !self.interaction.panel_open() {
			return None;
		}
		let form = controls::settings_form(
			self.ui,
			self.settings,
			self.interaction,
			self.width,
			self.height,
			self.backend,
		);
		form.menu(open, (self.width, self.height))
	}

	pub(super) fn form(&mut self) -> Option<components::Form> {
		if !self.interaction.panel_open()
			|| self.interaction.modal.is_some()
			|| self.interaction.styles_open()
			|| self.interaction.fonts_open()
			|| self.interaction.export_styles_open()
		{
			return None;
		}
		Some(if self.interaction.export_open() {
			export::form(
				self.ui,
				self.export,
				self.interaction.export_scroll,
				self.width,
				self.height,
				self.settings.lang(),
			)
		} else {
			controls::settings_form(
				self.ui,
				self.settings,
				self.interaction,
				self.width,
				self.height,
				self.backend,
			)
			.preview(self.interaction.settings_preview)
		})
	}

	pub(super) fn buttons(&mut self) -> Vec<Button> {
		let (width, height, _) = (self.width, self.height, 1.0);
		if self.interaction.modal.is_some() {
			modal::modal_buttons(
				self.ui,
				self.interaction,
				width,
				height,
				self.settings.lang(),
			)
		} else if self.interaction.panel_open()
			&& self.interaction.export_styles_open()
		{
			style_page_buttons(
				StylesTarget::Export,
				Some(self.export.style.as_slice()),
				self.style_entries,
				self.style_scroll,
				false,
				(width, height),
				self.settings.lang(),
			)
		} else if self.interaction.panel_open()
			&& self.interaction.export_open()
		{
			export::form(
				self.ui,
				self.export,
				self.interaction.export_scroll,
				width,
				height,
				self.settings.lang(),
			)
			.visible_buttons()
		} else if self.interaction.panel_open() && self.interaction.fonts_open()
		{
			fonts::buttons(
				&self.fonts,
				self.interaction.settings_preview,
				width,
				height,
				self.settings.lang(),
			)
		} else if self.interaction.panel_open()
			&& self.interaction.styles_open()
		{
			style_page_buttons(
				StylesTarget::Reader,
				self.settings.style.as_deref(),
				self.style_entries,
				self.style_scroll,
				self.interaction.settings_preview,
				(width, height),
				self.settings.lang(),
			)
		} else if self.interaction.panel_open() {
			let form = controls::settings_form(
				self.ui,
				self.settings,
				self.interaction,
				width,
				height,
				self.backend,
			)
			.without_header()
			.preview(self.interaction.settings_preview)
			.visible_buttons();
			let mut buttons = form;
			buttons.extend(components::settings_header_controls(
				components::panel_rect(width, height),
				if self.interaction.panel
					== crate::state::PanelPage::Settings(
						crate::state::PanelTab::About,
					) {
					crate::state::PanelTab::About
				} else {
					crate::state::PanelTab::Generic
				},
				self.interaction.settings_preview,
				self.settings.lang(),
			));
			buttons
		} else {
			let mut buttons = toolbar_controls(
				width,
				self.interaction.outline_open,
				self.settings.lang(),
			);
			if self.session.path.is_none()
				&& self.session.snapshot.blocks.is_empty()
			{
				// One `Open` command owns keyboard focus; both regions answer the pointer.
				buttons.push(empty_button(width, height, self.settings.lang()));
			}

			if self.remote_notice.is_some() {
				buttons.extend(banner_buttons(
					self.ui,
					width,
					self.settings.lang(),
				));
			}
			if self.interaction.outline_open {
				let lang = self.settings.lang();
				buttons.extend(outline::header_buttons(
					self.outline_drawer(),
					lang,
				));
				buttons.extend(outline::buttons(
					self.outline_drawer(),
					self.session.outline_entries(),
					&self.session.outline_tree,
					self.interaction.outline_scroll,
					lang,
				));
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
		out.extend(controls::draw_toolbar(
			self.ui,
			self.interaction,
			width,
			self.settings.lang(),
		));
		if let Some(deferred) = self.remote_notice {
			out.extend(draw_banner(
				self.ui,
				width,
				deferred,
				self.interaction,
				self.settings.lang(),
			));
		}
		let warning = if self.error
			&& self
				.status_until
				.is_none_or(|until| until <= Instant::now())
		{
			Some(self.status)
		} else {
			self.warning.as_deref()
		};
		if !self.session.search.open {
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
				(width, height),
				self.settings.lang(),
			));
		}
		if self.session.snapshot.blocks.is_empty() {
			let button = empty_button(width, height, self.settings.lang());
			let y = button.rect.y - 64.0;
			let t = self.settings.lang();
			let (title, detail) = if self.session.path.is_none() {
				(t.empty_open_title(), t.empty_open_detail())
			} else if self.error {
				(t.empty_unreadable_title(), t.empty_unreadable_detail())
			} else if self.session.layout_pending
				|| self.session.document.is_none()
			{
				(t.empty_opening_title(), t.empty_opening_detail())
			} else {
				(t.empty_blank_title(), t.empty_blank_detail())
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
		if self.interaction.outline_open {
			out.extend(outline::draw(
				self.ui,
				self.interaction,
				self.session.outline_entries(),
				&self.session.outline_tree,
				self.session.current_outline(),
				self.outline_drawer(),
				self.settings.lang(),
			));
		}
		if self.interaction.panel_open()
			&& self.interaction.export_styles_open()
		{
			out.extend(draw_styles(
				self.ui,
				StylesTarget::Export,
				Some(&self.export.style),
				self.interaction,
				self.style_entries,
				self.style_scroll,
				false,
				width,
				height,
				self.settings.lang(),
			));
		} else if self.interaction.panel_open()
			&& self.interaction.export_open()
		{
			let document = self
				.session
				.path
				.as_deref()
				.and_then(|path| path.file_name())
				.map(|name| name.to_string_lossy().into_owned())
				.unwrap_or_else(|| self.settings.lang().tabs_untitled().into());
			out.extend(export::draw_export(
				self.ui,
				self.export,
				self.interaction,
				&document,
				self.watching,
				(width, height),
				self.settings.lang(),
			));
		} else if self.interaction.panel_open() && self.interaction.fonts_open()
		{
			out.extend(draw_fonts(
				self.ui,
				self.interaction,
				&self.fonts,
				width,
				height,
				self.settings.lang(),
			));
		} else if self.interaction.panel_open()
			&& self.interaction.styles_open()
		{
			out.extend(draw_styles(
				self.ui,
				StylesTarget::Reader,
				self.settings.style.as_deref(),
				self.interaction,
				self.style_entries,
				self.style_scroll,
				self.interaction.settings_preview,
				width,
				height,
				self.settings.lang(),
			));
		} else if self.interaction.panel_open() {
			out.extend(draw_controls(
				self.ui,
				self.settings,
				self.interaction,
				width,
				height,
				self.backend,
			));
		}
		out.append(&mut self.input_draws);
		// A confirmation owns the frame; nothing behind it is interactive.
		if self.interaction.modal.is_some() {
			out.extend(modal::draw_modal(
				self.ui,
				self.interaction,
				width,
				height,
				self.settings.lang(),
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

	/// The outline drawer's rectangle for this window and notice strip.
	pub(super) fn outline_drawer(&self) -> Rect {
		outline::rect_above(
			self.width,
			self.height,
			content_top(self.remote_notice.is_some()),
			super::search::bottom(self.session.search.open),
		)
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
			let buttons = banner_buttons(&mut shaper, width, Lang::En);
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
