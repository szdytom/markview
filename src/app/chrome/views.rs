//! Borrowed input groups for the chrome painter.
//!
//! `Chrome` keeps one field per group, so a drawing routine reads a named view
//! instead of a flat list of borrowed values.

use crate::{
	app::{
		folder, font_panel, frame, preview, settings_load, settings_panel,
		status, tab_strip,
	},
	layout::Scrollbar,
	settings::ExportSettings,
	state::{ReaderSession, ReaderTab},
	stylesheet,
};
use markview_layout::fonts::FontConfig;
use std::borrow::Cow;

/// Window geometry, the frame layout and the paint-only products.
pub(in crate::app) struct FrameView {
	pub(in crate::app) layout: frame::Layout,
	pub(in crate::app) width: f32,
	pub(in crate::app) height: f32,
	pub(in crate::app) scrollbar: Option<Scrollbar>,
	pub(in crate::app) scrollbar_opacity: f32,
	pub(in crate::app) remote_notice: Option<usize>,
	pub(in crate::app) backend: Option<wgpu::Backend>,
}

impl FrameView {
	pub(in crate::app) fn new(
		layout: frame::Layout,
		(width, height): (f32, f32),
		scrollbar: Option<Scrollbar>,
		remote_notice: Option<usize>,
		backend: Option<wgpu::Backend>,
	) -> Self {
		Self {
			layout,
			width,
			height,
			scrollbar,
			scrollbar_opacity: 1.0,
			remote_notice,
			backend,
		}
	}
	pub(in crate::app) fn with_scrollbar_opacity(
		mut self,
		opacity: f32,
	) -> Self {
		self.scrollbar_opacity = opacity;
		self
	}
}

/// The document session, its tabs and the drawers they drive.
pub(in crate::app) struct SessionView<'a> {
	pub(in crate::app) doc: &'a ReaderSession,
	pub(in crate::app) tabs: &'a [ReaderTab],
	pub(in crate::app) active_tab: usize,
	pub(in crate::app) strip: &'a tab_strip::TabStrip,
	pub(in crate::app) tab_widths: &'a [(f32, f32)],
	/// Each tab's readable title, as the strip and the drawer show it.
	pub(in crate::app) tab_labels: &'a [String],
	pub(in crate::app) preview: Option<&'a preview::Open>,
	pub(in crate::app) folder: Option<&'a folder::Browser>,
	pub(in crate::app) now: std::time::Instant,
}

impl<'a> SessionView<'a> {
	#[expect(
		clippy::too_many_arguments,
		reason = "one borrowed field per reader surface the chrome draws"
	)]
	pub(in crate::app) fn new(
		doc: &'a ReaderSession,
		tabs: &'a [ReaderTab],
		active_tab: usize,
		strip: &'a tab_strip::TabStrip,
		tab_widths: &'a [(f32, f32)],
		tab_labels: &'a [String],
		preview: Option<&'a preview::Open>,
		folder: Option<&'a folder::Browser>,
	) -> Self {
		Self {
			doc,
			tabs,
			active_tab,
			strip,
			tab_widths,
			tab_labels,
			preview,
			folder,
			now: std::time::Instant::now(),
		}
	}

	pub(in crate::app) fn with_now(mut self, now: std::time::Instant) -> Self {
		self.now = now;
		self
	}

	/// The readable title of the tab at `index`; the active tab's session is
	/// `doc`, every other tab carries its own.
	pub(in crate::app) fn label(&self, index: usize) -> String {
		crate::app::tabs::tab_label(self.tabs, self.active_tab, self.doc, index)
	}

	/// Every tab's readable title, in strip order.
	pub(in crate::app) fn labels(&self) -> Vec<String> {
		crate::app::tabs::tab_labels(self.tabs, self.active_tab, self.doc)
	}

	/// The open document's name: a web page's readable label, else its file
	/// name. `None` when the session has no document to name.
	pub(in crate::app) fn document_name(&self) -> Option<Cow<'a, str>> {
		if matches!(self.doc.security.origin, crate::security::Origin::Web(_)) {
			return Some(self.label(self.active_tab).into());
		}
		self.doc
			.path
			.as_deref()?
			.file_name()
			.map(|name| name.to_string_lossy())
	}
}

/// Reader preferences and the resources the panel pages draw from.
pub(in crate::app) struct SettingsView<'a> {
	pub(in crate::app) model: &'a settings_panel::Model,
	pub(in crate::app) restart_pending: bool,
	pub(in crate::app) font_config: &'a FontConfig,
	pub(in crate::app) export: &'a ExportSettings,
	pub(in crate::app) style_entries: &'a [stylesheet::Entry],
	pub(in crate::app) style_scroll: f32,
	pub(in crate::app) resource_load: Option<&'a settings_load::Load>,
	pub(in crate::app) fonts: font_panel::View<'a>,
	pub(in crate::app) watching: bool,
}

impl<'a> SettingsView<'a> {
	#[expect(
		clippy::too_many_arguments,
		reason = "one settings view's borrowed inputs"
	)]
	pub(in crate::app) fn new(
		model: &'a settings_panel::Model,
		restart_pending: bool,
		font_config: &'a FontConfig,
		export: &'a ExportSettings,
		style_entries: &'a [stylesheet::Entry],
		style_scroll: f32,
		resource_load: Option<&'a settings_load::Load>,
		fonts: font_panel::View<'a>,
		watching: bool,
	) -> Self {
		Self {
			model,
			restart_pending,
			font_config,
			export,
			style_entries,
			style_scroll,
			resource_load,
			fonts,
			watching,
		}
	}
}

/// The footer's transient line, its warning and the hover hint.
pub(in crate::app) struct StatusView<'a> {
	pub(in crate::app) line: &'a status::Status,
	pub(in crate::app) warning: Option<Cow<'a, str>>,
	pub(in crate::app) hover_hint: Option<&'a str>,
}

impl<'a> StatusView<'a> {
	pub(in crate::app) fn new(
		line: &'a status::Status,
		warning: Option<Cow<'a, str>>,
		hover_hint: Option<&'a str>,
	) -> Self {
		Self {
			line,
			warning,
			hover_hint,
		}
	}
}
