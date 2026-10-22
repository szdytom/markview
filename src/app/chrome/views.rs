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
			remote_notice,
			backend,
		}
	}
}

/// The document session, its tabs and the drawers they drive.
pub(in crate::app) struct SessionView<'a> {
	pub(in crate::app) doc: &'a ReaderSession,
	pub(in crate::app) tabs: &'a [ReaderTab],
	pub(in crate::app) active_tab: usize,
	pub(in crate::app) strip: &'a tab_strip::TabStrip,
	pub(in crate::app) tab_widths: &'a [(f32, f32)],
	pub(in crate::app) preview: Option<&'a preview::Open>,
	pub(in crate::app) folder: Option<&'a folder::Browser>,
}

impl<'a> SessionView<'a> {
	pub(in crate::app) fn new(
		doc: &'a ReaderSession,
		tabs: &'a [ReaderTab],
		active_tab: usize,
		strip: &'a tab_strip::TabStrip,
		tab_widths: &'a [(f32, f32)],
		preview: Option<&'a preview::Open>,
		folder: Option<&'a folder::Browser>,
	) -> Self {
		Self {
			doc,
			tabs,
			active_tab,
			strip,
			tab_widths,
			preview,
			folder,
		}
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
