//! Reader chrome built from borrowed display state, with no window or worker access.
pub(super) mod components;
mod controls;
mod empty;
mod export;
use super::font_panel::view as fonts;
pub(in crate::app) mod folder;
mod footer;
mod frame;
#[cfg(test)]
mod golden;
#[cfg(test)]
mod gpu_tests;
pub(super) mod icons;
pub(in crate::app) mod list;
mod modal;
pub(in crate::app) mod outline;
pub(in crate::app) mod preview;
pub(in crate::app) mod styles;
pub(in crate::app) mod tab_drawer;
mod tabs;
mod viewer;
use super::{BOTTOM, Button, TOP};
#[cfg(test)]
use crate::settings::ReaderSettings;
#[cfg(test)]
use crate::state::InteractionState;
use crate::{
	lang::Lang,
	layout::{Draw, Paint, Rect, Scrollbar, TextShaper},
	settings::ExportSettings,
	state::{Command, ReaderSession, ReaderTab, ScrollbarAxis},
};
pub(super) use components::panel_rect;
#[cfg(test)]
use controls::draw_controls;
use controls::toolbar_controls_at;
use fonts::draw_fonts;
use footer::draw_footer;
use markview_layout::{
	fonts::FontConfig,
	style::{ColorField as C, Condition, TextAppearance},
};
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
			id: crate::app::widgets::id(Command::RemoteDismiss),
			clip: None,
			label: (lang.notice_dismiss()).into(),
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
			id: crate::app::widgets::id(Command::RemoteLoadAll),
			clip: None,
			label: (lang.notice_load_all()).into(),
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
	left: f32,
	interaction: &crate::app::widgets::Drawn<'_>,
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
	let available =
		buttons.first().map_or(width - 32.0, |b| b.rect.x - 16.0) - left;
	let label =
		shaper.fit(&lang.notice_remote_images(deferred), 12.0, available);
	out.extend(shaper.label(
		&label,
		12.0,
		16.0 + left,
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
	pub folder: Option<&'a super::folder::Browser>,
	pub(super) input_draws: Vec<Draw>,
	pub(super) backend: Option<wgpu::Backend>,
	pub(super) ui: &'a mut TextShaper,
	pub(super) session: &'a ReaderSession,
	pub(super) tabs: &'a [ReaderTab],
	pub(super) active_tab: usize,
	pub(super) tab_strip: &'a super::tab_strip::TabStrip,
	pub(super) tab_widths: &'a [(f32, f32)],
	/// The open hover preview, when one is painted.
	pub(super) preview: Option<&'a super::preview::Open>,
	pub(super) settings: &'a super::settings_panel::Model,
	pub(super) restart_pending: bool,
	/// The fonts the reader shapes with, which the family choosers describe.
	pub(super) font_config: &'a FontConfig,
	/// The export panel's own settings, drawn but never applied to the reader.
	pub(super) export: &'a ExportSettings,
	/// The paint view of the interaction state: its fields plus the placed
	/// control geometry from the last layout.
	pub(super) interaction: crate::app::widgets::Drawn<'a>,
	pub(super) style_entries: &'a [crate::stylesheet::Entry],
	/// The Styles page's list offset.
	pub(super) style_scroll: f32,
	pub(super) resource_load: Option<&'a super::settings_load::Load>,
	pub(super) fonts: super::font_panel::View<'a>,
	pub(super) frame: super::frame::Layout,
	pub(super) width: f32,
	pub(super) height: f32,
	pub(super) scrollbar: Option<Scrollbar>,
	pub(super) warning: Option<std::borrow::Cow<'a, str>>,
	pub(super) status: &'a super::status::Status,
	/// The footer's hover hint: a link target, or an image title.
	pub(super) hover_hint: Option<&'a str>,
	/// Number of remote image sources the loader deferred, if any.
	pub(super) remote_notice: Option<usize>,
	/// Whether an export is rewriting its file on every document change.
	pub(super) watching: bool,
}
impl<'a> Chrome<'a> {
	/// The active document's absolute path, or `None` when its session has no
	/// document to name.
	fn path(&self) -> Option<std::borrow::Cow<'a, str>> {
		self.session
			.path
			.as_deref()
			.map(|path| path.to_string_lossy())
	}

	/// The footer's opted-in document path.
	fn footer_path(&self) -> Option<std::borrow::Cow<'a, str>> {
		if self.settings.footer_path {
			self.path()
		} else {
			None
		}
	}

	/// The footer's trailing label: a live status, a hover hint, or — when
	/// enabled — the open document's absolute path.
	fn secondary(&self) -> std::borrow::Cow<'a, str> {
		if let Some(status) = self.status.current(Instant::now()) {
			status.into()
		} else if let Some(hint) = self.hover_hint {
			hint.into()
		} else {
			self.footer_path().unwrap_or_default()
		}
	}

	/// The footer's path as a control of its own, while the footer shows it: a
	/// press on the absolute path copies it, so the label is its own hit target.
	fn path_button(&mut self) -> Option<Button> {
		if self.session.search.open
			|| self.status.current(Instant::now()).is_some()
			|| self.hover_hint.is_some()
		{
			return None;
		}
		let path = self.footer_path()?;
		let size = (self.width, self.height);
		let (_, used) = footer::counts_label(
			self.ui,
			(!self.session.layout_pending).then_some(self.session.counts),
			self.interaction.selection_counts.map(|(_, counts)| counts),
			self.width - self.footer_inset(),
			self.settings.lang(),
		);
		let (_, rect) =
			footer::trailing(self.ui, &path, used + self.footer_inset(), size)?;
		// The label holds the whole path, so a copy never depends on what fit.
		let label: std::sync::Arc<str> = std::sync::Arc::from(&*path);
		Some(components::button(label, Command::CopyPath, rect))
	}

	/// The open option list, measured against the page that holds its row.
	pub(in crate::app) fn dropdown_menu(
		&mut self,
		open: &mut crate::state::Dropdown,
	) -> Option<components::Menu> {
		if !self.interaction.panel_open() {
			return None;
		}
		// The Fonts page carries its choosers outside any form, so their lists
		// are measured against the page itself; the anchor follows its scroll.
		if self.interaction.fonts_open() {
			return fonts::menu(
				&self.fonts,
				self.settings,
				self.font_config,
				open,
				(self.width, self.height),
			);
		}
		let form = controls::settings_form(
			self.ui,
			&self.interaction,
			self.settings,
			self.width,
			self.height,
			self.backend,
			self.tab_strip.phone,
		);
		form.menu(open, (self.width, self.height))
	}

	pub(super) fn form(&mut self) -> Option<components::Form> {
		if !self.interaction.panel_open()
			|| self.interaction.panel == crate::state::PanelPage::Tabs
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
				self.interaction.panels.export_scroll().offset(),
				self.width,
				self.height,
				self.settings.lang(),
			)
		} else {
			controls::settings_form(
				self.ui,
				&self.interaction,
				self.settings,
				self.width,
				self.height,
				self.backend,
				self.tab_strip.phone,
			)
			.without_header()
			.preview(self.settings.settings_preview)
		})
	}

	pub(super) fn buttons(&mut self) -> Vec<Button> {
		let form = self.form();
		self.buttons_for(form.as_ref())
	}
	pub(super) fn buttons_for(
		&mut self,
		form: Option<&components::Form>,
	) -> Vec<Button> {
		let (width, height, _) = (self.width, self.height, 1.0);
		let mut buttons = if self.interaction.modal.is_some() {
			modal::modal_buttons(
				self.ui,
				&self.interaction,
				width,
				height,
				self.settings.lang(),
			)
		} else if self.interaction.panel == crate::state::PanelPage::Tabs {
			let mut buttons = match self.interaction.sidebar_tab {
				crate::state::SidebarTab::Tabs => tab_drawer::buttons(
					self.ui,
					self.tabs,
					self.active_tab,
					self.tab_strip.drawer_scroll.offset(),
					(width, height),
					self.settings.lang(),
					self.settings.tabs_width,
				),
				crate::state::SidebarTab::Folder => tab_drawer::folder_buttons(
					self.folder,
					tab_drawer::rect(width, height, self.settings.tabs_width),
					width,
					self.settings.lang(),
				),
			};
			tab_drawer::slide_buttons(
				&mut buttons,
				width,
				height,
				self.interaction.tabs_reveal(),
				self.settings.tabs_width,
			);
			buttons
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
			form.expect("export form").visible_buttons()
		} else if self.interaction.panel_open() && self.interaction.fonts_open()
		{
			fonts::buttons(
				&self.fonts,
				self.settings,
				self.font_config,
				self.settings.settings_preview,
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
				self.settings.settings_preview,
				(width, height),
				self.settings.lang(),
			)
		} else if self.interaction.panel_open() {
			let mut buttons = form.expect("settings form").visible_buttons();
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
				self.settings.settings_preview,
				self.settings.lang(),
			));
			buttons
		} else {
			self.reading_buttons()
		};
		self.apply_button_load_state(&mut buttons);
		buttons
	}
	fn folder_toggle(&self) -> Option<Button> {
		Some(folder::toggle(
			self.frame.sidebar_toggle?,
			self.settings.lang(),
		))
	}

	fn footer_inset(&self) -> f32 {
		if !self.tab_strip.phone
			&& self.folder.is_some_and(|b| b.tree.is_some())
		{
			let drawer = self.folder_drawer();
			(drawer.x + drawer.w).max(0.)
		} else {
			0.
		}
	}

	fn folder_drawer(&self) -> Rect {
		let reveal = self
			.folder
			.map_or(0., |b| self.interaction.folder_reveal(b.open));
		folder::animated_rect(
			folder::rect(
				self.width,
				self.height,
				TOP,
				BOTTOM,
				self.settings.folder_width,
			),
			reveal,
		)
	}

	/// The document controls cannot receive input through the folder overlay.
	fn reading_clip(&self) -> Rect {
		let left = self
			.folder
			.filter(|b| !self.tab_strip.phone && b.tree.is_some())
			.map_or(0., |_| {
				let drawer = self.folder_drawer();
				drawer.x + drawer.w
			});
		Rect {
			x: left,
			y: content_top(self.remote_notice.is_some()),
			w: (self.width - left).max(0.),
			h: (self.height
				- content_top(self.remote_notice.is_some())
				- super::search::bottom(self.session.search.open))
			.max(0.),
		}
	}

	fn reading_buttons(&mut self) -> Vec<Button> {
		let (width, height) = (self.width, self.height);
		let mut buttons = toolbar_controls_at(
			self.frame,
			self.interaction.outline_open,
			self.settings.lang(),
		);
		let clip = Rect {
			x: self.frame.left_inset,
			y: 0.,
			w: (width - self.frame.left_inset).max(0.),
			h: self.height,
		};
		for button in &mut buttons {
			button.clip = Some(clip);
		}
		buttons.extend(self.folder_toggle());
		if let Some(browser) =
			self.folder.filter(|b| !self.tab_strip.phone && b.open)
		{
			buttons.extend(folder::buttons(
				browser,
				self.folder_drawer(),
				self.settings.lang(),
			));
		}
		if self.tab_strip.phone {
			buttons.push(tab_drawer::toggle(self.settings.lang()));
		}
		if self.session.path.is_none()
			&& self.session.snapshot.blocks.is_empty()
		{
			// Each `Open` control has its own identity and geometry.
			let clip = self.reading_clip();
			buttons.extend(
				empty::buttons(self.ui, width, height, self.settings.lang())
					.into_iter()
					.filter_map(|mut button| {
						button.rect.intersect(clip)?;
						button.clip = Some(clip);
						Some(button)
					}),
			);
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
			buttons
				.extend(outline::header_buttons(self.outline_drawer(), lang));
			buttons.extend(outline::buttons(
				self.outline_drawer(),
				self.session.outline_entries(),
				&self.session.outline_tree,
				self.interaction.outline_scroll.offset(),
				lang,
			));
		}
		// The footer's path answers a press last: nothing else in the
		// reading view reaches the band it sits in.
		buttons.extend(self.path_button());
		let main = Rect {
			x: self.frame.left_inset,
			y: 0.,
			w: (width - self.frame.left_inset).max(0.),
			h: height,
		};
		buttons.retain_mut(|b| {
			if matches!(
				b.action,
				Command::Folder
					| Command::FolderRoot
					| Command::FolderEntry(_)
					| Command::OpenFolder
					| Command::FolderRefresh
					| Command::FolderCollapseAll
			) {
				return true;
			}
			let Some(clip) = b.clip.unwrap_or(main).intersect(main) else {
				return false;
			};
			b.clip = Some(clip);
			true
		});
		buttons
	}

	fn apply_button_load_state(&self, buttons: &mut Vec<Button>) {
		if let Some(load) = self.resource_load {
			if load.blocked() {
				for button in buttons.iter_mut() {
					if super::settings_load::dependent(button.action) {
						button.enabled = false;
					}
				}
			}
			if matches!(load.status, super::settings_load::Status::Failed(_)) {
				buttons
					.push(self.resource_retry(load.displayed && load.cached));
			}
		}
	}

	#[cfg(test)]
	pub(super) fn overlay(&mut self) -> Vec<Draw> {
		let form = self.form();
		self.overlay_with_form(form.as_ref()).0
	}
	/// Draws the whole chrome and returns where a hover preview's body was
	/// reserved, so the caller can compose the target's scene into it.
	pub(super) fn overlay_with_form(
		&mut self,
		form: Option<&components::Form>,
	) -> (Vec<Draw>, Option<Rect>) {
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
					y: height - BOTTOM,
					w: width,
					h: BOTTOM,
				},
				Paint::Styled(Condition::Toolbar, C::Background),
			),
		];
		if !self.tab_strip.phone
			&& self.settings.tab_style == crate::settings::TabStyle::Underline
		{
			out.push(Draw::Rect(
				Rect {
					x: 0.0,
					y: TOP - 1.0,
					w: width,
					h: 1.0,
				},
				Paint::Styled(Condition::Toolbar, C::BorderColor),
			));
		}
		if self.tab_strip.phone {
			self.ui.appearance = ui_appearance(self.ui);
			let toggle = tab_drawer::toggle(self.settings.lang());
			out.extend(components::draw_button(
				self.ui,
				&self.interaction,
				&toggle,
				true,
			));
			let name = self
				.session
				.path
				.as_deref()
				.and_then(std::path::Path::file_name)
				.map(|name| name.to_string_lossy())
				.unwrap_or_else(|| "Markview".into());
			let name =
				self.ui
					.fit(&name, 12., (self.frame.toolbar_x - 56.).max(0.));
			out.extend(self.ui.label(
				&name,
				12.,
				52.,
				25.,
				Paint::Styled(Condition::Toolbar, C::Color),
			));
		} else {
			out.extend(self.tab_bar().draw_tabs());
		}
		out.extend(controls::draw_toolbar_at(
			self.ui,
			&self.interaction,
			self.frame,
			self.settings.lang(),
		));
		if let Some(deferred) = self.remote_notice {
			out.extend(draw_banner(
				self.ui,
				width,
				deferred,
				self.frame.left_inset,
				&self.interaction,
				self.settings.lang(),
			));
		}
		// A failure that outlived its deadline takes the warning strip; a
		// live line trails the counts instead.
		let warning = self
			.status
			.warning(Instant::now())
			.or(self.warning.as_deref());
		if !self.session.search.open {
			let secondary = self.secondary();
			let inset = self.footer_inset();
			out.extend(draw_footer(
				self.ui,
				(
					(!self.session.layout_pending)
						.then_some(self.session.counts),
					self.interaction.selection_counts.map(|(_, counts)| counts),
				),
				warning,
				&secondary,
				(width, height),
				inset,
				self.settings.lang(),
			));
		}
		self.draw_document_status(&mut out);

		self.draw_drawers(&mut out);
		let body = self.draw_preview(&mut out);
		self.draw_panel_overlay(&mut out, form);
		self.draw_resource_feedback(&mut out);
		out.append(&mut self.input_draws);
		// An open option list floats over whichever page holds its row, so it
		// goes on after everything the page draws. Measuring brings the
		// highlight into the drawn window, on a copy: the state is corrected
		// the next time the input paths measure it.
		if let Some(mut open) = self.interaction.dropdown
			&& let Some(menu) = self.dropdown_menu(&mut open)
		{
			out.extend(components::draw_menu(
				self.ui,
				&self.interaction,
				&menu,
			));
		}
		// A confirmation owns the frame; nothing behind it is interactive.
		if self.interaction.modal.is_some() {
			out.extend(modal::draw_modal(
				self.ui,
				&self.interaction,
				width,
				height,
				self.settings.lang(),
			));
		}
		// The image viewer floats over everything, the modal included.
		if let Some(viewer) = &self.interaction.viewer {
			out.extend(viewer::draw_viewer(
				viewer,
				&self.session.snapshot,
				(width, height),
			));
		}
		out.extend(frame::draw(self.frame));
		(out, body)
	}

	/// Paints the hover preview card below its anchor, returning the body
	/// rectangle the renderer composes the target's scene into.
	fn draw_preview(&mut self, out: &mut Vec<Draw>) -> Option<Rect> {
		let preview = self.preview?;
		// A panel, list, confirmation or search owns the screen; the card must
		// not float above them for the frame before the tick closes it.
		if self.tab_strip.phone
			|| self.interaction.panel_open()
			|| self.session.search.open
			|| self.interaction.modal.is_some()
			|| self.interaction.dropdown.is_some()
			|| self.interaction.viewer.is_some()
		{
			return None;
		}
		let anchor = self.preview_anchor(preview.target)?;
		let layout =
			preview::layout(&preview.card, anchor, (self.width, self.height));
		let body = layout.body;
		out.extend(preview::draw(self.ui, &preview.card, &layout));
		body
	}

	/// The rectangle a preview card hangs under.
	fn preview_anchor(
		&mut self,
		target: super::preview::Target,
	) -> Option<Rect> {
		match target {
			super::preview::Target::Tab(id) => {
				let index = self.tabs.iter().position(|tab| tab.id == id)?;
				let layout = self.tab_bar().layout();
				let tab = *layout.rects.get(index)?;
				tab.intersect(layout.viewport)
			}
		}
	}

	fn resource_viewport(&self) -> Rect {
		if self.interaction.fonts_open() {
			fonts::list(
				self.width,
				self.height,
				self.fonts.shown.len(),
				self.fonts.scroll,
				self.fonts.choosers,
				fonts::roles(self.settings).len(),
			)
			.viewport
		} else {
			styles::list(
				self.width,
				self.height,
				self.style_entries.len(),
				self.style_scroll,
			)
			.viewport
		}
	}
	fn draw_document_status(&mut self, out: &mut Vec<Draw>) {
		let (width, height) = (self.width, self.height);
		if self.session.path.is_none() {
			out.extend(empty::draw(
				self.ui,
				&self.interaction,
				width,
				height,
				self.settings.lang(),
			));
		} else if self.session.snapshot.blocks.is_empty()
			|| self.session.load_error.is_some()
		{
			if self.session.load_error.is_some() {
				out.push(Draw::Rect(
					Rect {
						x: 0.0,
						y: content_top(self.remote_notice.is_some()),
						w: width,
						h: (height
							- content_top(self.remote_notice.is_some())
							- BOTTOM)
							.max(0.0),
					},
					Paint::Background,
				));
			}
			let x = ((width - 400.0) / 2.0).max(24.0);
			let y = (height * 0.4).max(110.0);
			let t = self.settings.lang();
			let (title, detail) =
				if let Some(error) = self.session.load_error.as_deref() {
					(t.empty_unreadable_title(), error)
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
				x,
				y,
				Paint::Styled(Condition::Ui, C::Color),
			));
			self.ui.appearance = ui_appearance(self.ui);
			let available = width - x - 24.0;
			let mut baseline = y + 32.0;
			for paragraph in detail.lines() {
				for line in controls::wrap(self.ui, paragraph, 13.0, available)
				{
					if baseline > height - BOTTOM - 12.0 {
						break;
					}
					out.extend(self.ui.label(
						&line,
						13.0,
						x,
						baseline,
						Paint::Styled(Condition::Ui, C::Muted),
					));
					baseline += 20.0;
				}
			}
		}
	}

	fn draw_drawers(&mut self, out: &mut Vec<Draw>) {
		let (width, height) = (self.width, self.height);
		if let Some(bar) = self.scrollbar {
			let held = self
				.interaction
				.pointer
				.scrollbar()
				.is_some_and(|drag| drag.target == ScrollbarAxis::Document);
			let (x, y) = self.interaction.pointer.cursor();
			let (track, thumb) = bar.bars(false);
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
		if let Some(browser) = self.folder.filter(|b| {
			!self.tab_strip.phone && self.interaction.folder_reveal(b.open) > 0.
		}) {
			out.extend(folder::draw(
				self.ui,
				browser,
				self.session.path.as_deref(),
				&self.interaction,
				folder::Pane {
					body: self.folder_drawer(),
					surface: folder::surface(self.folder_drawer(), height),
				},
				self.settings.lang(),
			));
		}
		if let Some(button) = self.folder_toggle() {
			out.extend(components::draw_button(
				self.ui,
				&self.interaction,
				&button,
				true,
			));
		}
		if self.interaction.outline_reveal() > 0.0 {
			out.extend(outline::draw(
				self.ui,
				&self.interaction,
				self.session.outline_entries(),
				&self.session.outline_tree,
				self.session.current_outline(),
				self.outline_drawer(),
				self.settings.lang(),
			));
		}
		let tabs_reveal = self.interaction.tabs_reveal();
		if tabs_reveal > 0.0 {
			let draws = match self.interaction.sidebar_tab {
				crate::state::SidebarTab::Tabs => tab_drawer::draw(
					self.ui,
					self.tabs,
					self.active_tab,
					self.tab_strip.drawer_scroll.offset(),
					&self.interaction,
					(width, height),
					self.settings,
				),
				crate::state::SidebarTab::Folder => tab_drawer::draw_folder(
					self.ui,
					self.folder,
					self.session.path.as_deref(),
					&self.interaction,
					(width, height),
					self.settings,
				),
			};
			out.extend(draws);
		}
	}

	fn draw_panel_overlay(
		&mut self,
		out: &mut Vec<Draw>,
		form: Option<&components::Form>,
	) {
		let (width, height) = (self.width, self.height);
		if self.interaction.panel_open()
			&& self.interaction.export_styles_open()
		{
			out.extend(draw_styles(
				self.ui,
				StylesTarget::Export,
				Some(&self.export.style),
				&self.interaction,
				self.style_entries,
				self.style_scroll,
				false,
				width,
				height,
				self.settings.lang(),
				self.resource_load,
			));
		} else if self.interaction.panel_open()
			&& self.interaction.export_open()
			&& self.interaction.modal.is_none()
		{
			let document = self
				.session
				.path
				.as_deref()
				.and_then(|path| path.file_name())
				.map(|name| name.to_string_lossy().into_owned())
				.unwrap_or_else(|| self.settings.lang().tabs_untitled().into());
			out.extend(export::draw_export_form(
				self.ui,
				self.export,
				&self.interaction,
				(&document, self.watching),
				(width, height),
				self.settings.lang(),
				form.expect("export form"),
			));
		} else if self.interaction.panel_open() && self.interaction.fonts_open()
		{
			out.extend(draw_fonts(
				self.ui,
				&self.interaction,
				self.settings,
				&self.fonts,
				width,
				height,
				self.resource_load,
			));
		} else if self.interaction.panel_open()
			&& self.interaction.styles_open()
		{
			out.extend(draw_styles(
				self.ui,
				StylesTarget::Reader,
				self.settings.style.as_deref(),
				&self.interaction,
				self.style_entries,
				self.style_scroll,
				self.settings.settings_preview,
				width,
				height,
				self.settings.lang(),
				self.resource_load,
			));
		} else if self.interaction.panel_open()
			&& self.interaction.panel != crate::state::PanelPage::Tabs
			&& self.interaction.modal.is_none()
		{
			out.extend(controls::draw_controls_form(
				self.ui,
				&self.interaction,
				self.settings,
				(width, height),
				self.restart_pending,
				form.expect("settings form"),
			));
		}
	}

	fn resource_retry(&self, cached: bool) -> Button {
		let panel = components::panel_rect(self.width, self.height);
		let viewport = self.resource_viewport();
		components::button(
			self.settings.lang().panel_retry(),
			Command::RetrySettingsLoad,
			Rect {
				x: if cached {
					panel.x + panel.w - 100.
				} else {
					viewport.x + (viewport.w - 80.) / 2.
				},
				y: if cached {
					panel.y + panel.h - 64.
				} else {
					if viewport.h >= 80. {
						viewport.y + viewport.h / 2. + 8.
					} else {
						viewport.y + (viewport.h - 16.).max(0.)
					}
				},
				w: 80.,
				h: if cached || viewport.h < 80. { 16. } else { 32. },
			},
		)
	}
	fn draw_resource_feedback(&mut self, out: &mut Vec<Draw>) {
		let Some(load) = self.resource_load else {
			return;
		};
		let Some(message) = load.message(self.settings.lang()) else {
			return;
		};
		let panel = components::panel_rect(self.width, self.height);
		let viewport = self.resource_viewport();
		let failed =
			matches!(load.status, super::settings_load::Status::Failed(_));
		let cached = load.displayed && load.cached;
		let rect = if cached {
			Rect {
				x: panel.x + 1.,
				y: panel.y + panel.h - 64.,
				w: panel.w - 2.,
				h: 16.,
			}
		} else {
			viewport
		};
		let start = out.len();
		out.push(Draw::Rect(
			rect,
			crate::layout::Paint::Styled(
				markview_layout::style::Condition::Panel,
				markview_layout::style::ColorField::Background,
			),
		));
		let width = (rect.w - if failed { 116. } else { 48. }).max(0.);
		let text = self.ui.fit(&message, 12., width);
		out.extend(self.ui.label(
			&text,
			12.,
			rect.x + 24.,
			if cached || (failed && rect.h < 80.) {
				rect.y + 12.
			} else {
				rect.y + rect.h / 2. - 8.
			},
			crate::layout::Paint::Styled(
				markview_layout::style::Condition::Panel,
				markview_layout::style::ColorField::Muted,
			),
		));
		if failed && rect.h >= 16. {
			let button = self.resource_retry(load.displayed && load.cached);
			out.extend(components::draw_button(
				self.ui,
				&self.interaction,
				&button,
				true,
			));
		}
		if self.settings.settings_preview
			&& (self.interaction.styles_open() || self.interaction.fonts_open())
		{
			components::fade(
				&mut out[start..],
				self.ui,
				components::PREVIEW_OPACITY,
			);
		}
	}
	pub(super) fn tab_bar(&mut self) -> tabs::TabBar<'_> {
		tabs::TabBar {
			style: self.settings.tab_style,
			ui: self.ui,
			strip: self.tab_strip,
			widths: self.tab_widths,
			tabs: self.tabs,
			active_tab: self.active_tab,
			cursor: self.interaction.pointer.cursor(),
			viewport: self.frame.tabs,
		}
	}

	/// The outline drawer's rectangle for this window and notice strip.
	pub(super) fn outline_drawer(&self) -> Rect {
		let inset = if folder::docked(self.width, self.tab_strip.phone)
			&& self.folder.is_some_and(|b| b.open && b.tree.is_some())
		{
			self.folder_drawer().w
		} else {
			0.
		};
		let mut drawer = outline::rect_above(
			self.width,
			self.height,
			content_top(self.remote_notice.is_some()),
			super::search::bottom(self.session.search.open),
			self.settings.outline_width.min(self.width - inset),
		);
		drawer.x += drawer.w * (1.0 - self.interaction.outline_reveal());
		drawer
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
