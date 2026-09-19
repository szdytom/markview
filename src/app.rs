mod anchor;
mod chrome;
mod document;
mod export;
mod icon;
mod interaction;
mod launch;
mod lifecycle;
mod painting;
mod pointer;
mod preferences;
mod tab_metrics;
mod tab_navigation;
mod tab_strip;
mod tabs;
mod ui;
mod viewport;
mod window;
use crate::cli::LaunchOptions;
use crate::state::{Command, InteractionState};
use crate::{
	layout::{LayoutOptions, Rect, TextShaper},
	render::{Renderer, Theme},
	watch::FileWatch,
	worker::{Update, Worker},
};
use anyhow::Result;
use std::{path::PathBuf, sync::Arc, time::Instant};
use winit::{event_loop::EventLoopProxy, window::Window};

const TOP: f32 = 40.0;
const BOTTOM: f32 = 28.0;

pub fn run() -> Result<()> {
	launch::run()
}

enum Event {
	Ready(Box<Update>),
	Changed(PathBuf),
	SettingsChanged,
	StylesChanged,
	Open(Option<PathBuf>),
	DeviceLost,
	Exported(Box<ExportOutcome>),
}

/// What one export produced, or why it produced nothing.
enum ExportOutcome {
	/// A PDF is on the disk.
	Written {
		path: PathBuf,
		detail: String,
	},
	/// A PNG layout is ready for the main thread to draw and write.
	PngReady {
		snapshot: crate::layout::LayoutSnapshot,
		path: PathBuf,
		/// What the strips render with; the shared renderer borrows it and
		/// then takes the reading view's sheet back.
		stylesheet: Arc<markview_core::style::Stylesheet>,
	},
	Failed(String),
	Cancelled,
}

/// One live export: the document it follows and the file it rewrites.
pub(super) struct WatchExport {
	pub(super) source: PathBuf,
	pub(super) output: PathBuf,
}
#[derive(Clone)]
struct Button {
	kind: chrome::components::ButtonKind,
	enabled: bool,
	rect: Rect,
	/// Names the button; drawn only when it has no icon.
	label: &'static str,
	/// Drawn centered in place of the label when set.
	icon: Option<&'static [markview_core::scene::IconPath]>,
	/// Whether this button is the current choice in its row.
	active: bool,
	action: Command,
}

/// The desktop preference; `None` when the platform does not report one.
fn system_theme(window: &Window) -> Option<Theme> {
	window.theme().map(|theme| match theme {
		winit::window::Theme::Dark => Theme::Dark,
		_ => Theme::Light,
	})
}

struct App {
	interaction: InteractionState,
	readers: tabs::Tabs,
	tab_strip: tab_strip::TabStrip,
	tab_metrics: tab_metrics::TabMetrics,
	args: LaunchOptions,
	proxy: EventLoopProxy<Event>,
	window: Option<Arc<Window>>,
	renderer: Option<Renderer>,
	worker: Worker,
	watch: Option<FileWatch>,
	_settings_watch: Option<FileWatch>,
	_styles_watch: Option<FileWatch>,
	ui: TextShaper,
	preferences: preferences::Preferences,
	clipboard: crate::platform::Clipboard,
	paste_dir: tempfile::TempDir,
	paste_serial: u32,
	status: String,
	status_until: Option<Instant>,
	error: bool,
	dialog_open: bool,
	reflow_at: Option<Instant>,
	retry_at: Option<Instant>,
	first_frame: Option<Update>,
	started: Instant,
	fatal: Option<String>,
	/// An export is being prepared or written; one at a time.
	export_running: bool,
	/// A PNG layout waiting to be drawn, one strip per frame.
	png_export: Option<export::PngExport>,
	/// The file a live export keeps rewriting, while the watch toggle is on.
	watch_export: Option<WatchExport>,
	/// When a watched document change is due to become a rebuild.
	watch_at: Option<Instant>,
	/// The export in flight is a watch rebuild, which does not reopen the file.
	export_rebuild: bool,
	/// The export in flight was asked to keep watching its file.
	export_watch_request: bool,
}
impl App {
	pub(super) fn new(
		args: LaunchOptions,
		proxy: EventLoopProxy<Event>,
	) -> Self {
		let done = proxy.clone();
		let worker = Worker::with_images(
			args.offline,
			args.options.fonts.clone(),
			move |update| {
				let _ = done.send_event(Event::Ready(Box::new(update)));
			},
		);
		let mut ui = TextShaper::with_fonts(args.options.fonts.clone());
		let preferences = preferences::Preferences::new(&args, &mut ui);
		let settings_watch = preferences.path().map(|path| {
			let proxy = proxy.clone();
			FileWatch::new(path.to_path_buf(), move || {
				let _ = proxy.send_event(Event::SettingsChanged);
			})
		});
		let styles_watch = crate::stylesheet::directory().map(|dir| {
			let proxy = proxy.clone();
			FileWatch::directory(dir, move || {
				let _ = proxy.send_event(Event::StylesChanged);
			})
		});
		Self {
			interaction: InteractionState::default(),
			readers: tabs::Tabs::default(),
			tab_strip: Default::default(),
			tab_metrics: Default::default(),
			args,
			proxy,
			window: None,
			renderer: None,
			worker,
			watch: None,
			_settings_watch: settings_watch,
			_styles_watch: styles_watch,
			ui,
			preferences,
			clipboard: Default::default(),
			paste_dir: tempfile::tempdir()
				.expect("create clipboard paste directory"),
			paste_serial: 0,
			status: String::new(),
			status_until: None,
			error: false,
			dialog_open: false,
			reflow_at: None,
			retry_at: None,
			first_frame: None,
			started: Instant::now(),
			fatal: None,
			export_running: false,
			png_export: None,
			watch_export: None,
			watch_at: None,
			export_rebuild: false,
			export_watch_request: false,
		}
	}
	fn reload_styles(&mut self) {
		if let Some(reflow) = self.preferences.reload_styles(&mut self.ui) {
			if let Some(renderer) = &mut self.renderer {
				renderer
					.set_stylesheet(self.preferences.values.stylesheet.clone());
			}
			if reflow {
				self.request(false);
			}
		}
		if self.interaction.export_styles_open {
			self.preferences.style_entries = crate::stylesheet::catalog_for(
				crate::stylesheet::directory().as_deref(),
				Some(&self.preferences.export.style),
				markview_core::style::StyleTarget::Pdf,
			);
		}
	}
	pub(super) fn dimensions(&self) -> (f32, f32, f32) {
		self.window.as_ref().map_or((1200.0, 800.0, 1.0), |w| {
			let s = w.scale_factor() as f32;
			let size = w.inner_size();
			(size.width as f32 / s, size.height as f32 / s, s)
		})
	}
	pub(super) fn view_geometry(&self) -> markview_core::scene::Viewport {
		let (width, height, _) = self.dimensions();
		markview_core::scene::Viewport {
			width,
			height,
			left: ((width - self.readers.session.snapshot.width) / 2.0)
				.max(20.0),
			top: self.content_top() + 10.0,
			bottom: BOTTOM + 10.0,
			scroll: self.readers.session.scroll,
		}
	}
	pub(super) fn viewport(&self) -> f32 {
		self.view_geometry().clip().h.max(1.0)
	}
	pub(super) fn redraw(&self) {
		if let Some(w) = &self.window {
			w.request_redraw();
		}
	}
	pub(super) fn options(&self) -> LayoutOptions {
		self.preferences.values.layout_options(
			self.dimensions().0,
			self.args.options.greedy,
			&self.args.options.fonts,
		)
	}
}
