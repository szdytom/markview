mod anchor;
mod chrome;
mod document;
mod dropdown;
mod export;
pub(crate) mod font_panel;
mod fonts_command;
mod gestures;
mod icon;
mod interaction;
mod launch;
mod lifecycle;
mod open_document;
mod outline;
mod painting;
mod pointer;
mod preferences;
pub(crate) mod search;
mod tab_metrics;
mod tab_navigation;
mod tab_strip;
mod tabs;
mod text_input;
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
use markview_core::fonts::FontConfig;
use std::{path::PathBuf, sync::Arc, time::Instant};
use winit::{event_loop::EventLoopProxy, window::Window};

/// How the reader hands an event to its own loop.
///
/// `EventLoopProxy` is what the application uses; a test drives the same
/// handlers without a window server by handing them a stub, which is why the
/// application names this rather than the concrete type. It carries no error:
/// a loop that has stopped is a loop nobody is drawing.
trait SendEvent: Clone + Send + 'static {
	fn send(&self, event: Event);
}
impl SendEvent for EventLoopProxy<Event> {
	fn send(&self, event: Event) {
		let _ = self.send_event(event);
	}
}

const TOP: f32 = 40.0;
const BOTTOM: f32 = 28.0;
/// Logical pixels one line of a discrete scroll travels, before the reader's
/// speed multiplier and the desktop's lines-per-notch choice.
const LINE_STEP: f32 = 42.0;

pub fn run() -> Result<()> {
	launch::run()
}

enum Event {
	Ready(Box<Update>),
	SearchReady(search::Result),
	Parsed {
		path: PathBuf,
		content_version: u64,
		document: Arc<markview_core::document::Document>,
	},
	Changed(PathBuf),
	SettingsChanged,
	StylesChanged,
	Open(Option<PathBuf>),
	DeviceLost,
	Exported(Box<ExportOutcome>),
	Fonts(font_panel::Message),
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
	/// Drawn after the label, which makes room for it. It marks what the
	/// button does beyond naming it, such as opening a list.
	marker: Option<&'static [markview_core::scene::IconPath]>,
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

/// Records a finished download in `config`, returning whether it changed.
///
/// A job that stored no file leaves the directories and the revision alone: a
/// fresh revision would only build and cache a collection identical to the one
/// already in use. Otherwise the directory is added once, but the revision
/// always changes, because a later job may have stored new faces in a
/// directory an earlier one already registered, and both the collection cache
/// and `TextShaper::set_fonts` compare whole configurations.
fn register_font_dir(
	config: &mut FontConfig,
	dir: PathBuf,
	stored: usize,
) -> bool {
	if stored == 0 {
		return false;
	}
	if !config.directories.contains(&dir) {
		config.directories.push(dir);
	}
	config.revision = config.revision.wrapping_add(1);
	true
}

struct App<P = EventLoopProxy<Event>> {
	interaction: InteractionState,
	gestures: gestures::GestureState,
	readers: tabs::Tabs,
	tab_strip: tab_strip::TabStrip,
	tab_metrics: tab_metrics::TabMetrics,
	args: LaunchOptions,
	/// The reader's own font sources: the configured directories plus the
	/// personal download directory. The export panel shapes with the same
	/// sources, so an export matches what the reader shows.
	fonts_config: FontConfig,
	proxy: P,
	window: Option<Arc<Window>>,
	renderer: Option<Renderer>,
	worker: Worker,
	search_worker: search::Worker,
	watch: Option<FileWatch>,
	_settings_watch: Option<FileWatch>,
	_styles_watch: Option<FileWatch>,
	ui: TextShaper,
	preferences: preferences::Preferences,
	/// What one wheel notch travels on this desktop, read once at startup:
	/// nothing reports the desktop setting changing afterwards.
	wheel_notch: crate::platform::WheelNotch,
	font_panel: font_panel::FontPanel,
	clipboard: crate::platform::Clipboard,
	text_input: text_input::InputState,
	paste_dir: tempfile::TempDir,
	paste_serial: u32,
	status: String,
	status_until: Option<Instant>,
	error: bool,
	dialog_open: bool,
	reflow_at: Option<Instant>,
	retry_at: Option<Instant>,
	/// When the next glyph prewarm pass is due, while one is still worth
	/// running. Cleared whenever the reader is scrolling through new content.
	prewarm_at: Option<Instant>,
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
impl<P: SendEvent> App<P> {
	pub(super) fn new(args: LaunchOptions, proxy: P) -> Self {
		// Parse already folded the personal download directory into the font
		// set for the drawing modes. This copy is the reader's own, so a
		// download can extend it without touching what the command line named.
		let fonts_config = args.options.fonts.clone();
		let done = proxy.clone();
		let parsed_proxy = proxy.clone();
		let worker = Worker::with_images_and_parsed(
			args.offline,
			fonts_config.clone(),
			move |update| {
				done.send(Event::Ready(Box::new(update)));
			},
			move |path, content_version, document| {
				parsed_proxy.send(Event::Parsed {
					path,
					content_version,
					document,
				})
			},
		);
		let search_proxy = proxy.clone();
		let search_worker = search::Worker::new(move |result| {
			search_proxy.send(Event::SearchReady(result))
		});
		let mut ui = TextShaper::with_fonts(fonts_config.clone());
		let preferences = preferences::Preferences::new(&args, &mut ui);
		let settings_watch = preferences.path().map(|path| {
			let proxy = proxy.clone();
			FileWatch::new(path.to_path_buf(), move || {
				proxy.send(Event::SettingsChanged);
			})
		});
		let styles_watch = crate::stylesheet::directory().map(|dir| {
			let proxy = proxy.clone();
			FileWatch::directory(dir, move || {
				proxy.send(Event::StylesChanged);
			})
		});

		Self {
			interaction: InteractionState::default(),
			gestures: gestures::GestureState::default(),
			readers: tabs::Tabs::default(),
			tab_strip: Default::default(),
			tab_metrics: Default::default(),
			args,
			fonts_config,
			proxy,
			window: None,
			renderer: None,
			worker,
			search_worker,
			watch: None,
			_settings_watch: settings_watch,
			_styles_watch: styles_watch,
			ui,
			preferences,
			wheel_notch: crate::platform::wheel_notch(),
			font_panel: font_panel::FontPanel::default(),
			clipboard: Default::default(),
			text_input: Default::default(),
			paste_dir: tempfile::tempdir()
				.expect("create clipboard paste directory"),
			paste_serial: 0,
			status: String::new(),
			status_until: None,
			error: false,
			dialog_open: false,
			reflow_at: None,
			retry_at: None,
			prewarm_at: None,
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
		if self.interaction.export_styles_open() {
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
			bottom: self.bottom() + 10.0,
			scroll: self.readers.session.scroll,
		}
	}
	pub(super) fn viewport(&self) -> f32 {
		self.viewport_size().1
	}
	/// The reading viewport's logical width and height.
	pub(super) fn viewport_size(&self) -> (f32, f32) {
		let clip = self.view_geometry().clip();
		(clip.w.max(1.0), clip.h.max(1.0))
	}
	pub(super) fn redraw(&self) {
		if let Some(w) = &self.window {
			w.request_redraw();
		}
	}
	pub(super) fn options(&self) -> LayoutOptions {
		let mut options = self.preferences.values.layout_options(
			self.dimensions().0,
			self.args.options.greedy,
			&self.fonts_config,
		);
		options.details_open = self.readers.session.details_open.clone();
		options
	}

	/// Makes the user font directory part of every later layout after a job
	/// that stored at least one file.
	///
	/// A job that failed every request, or found every file already present,
	/// changed nothing, so it must not bump the revision: a new revision would
	/// build and cache a collection identical to the one in use. A job that
	/// wrote nothing also cannot have created the directory, so it is not
	/// added either. `FontConfig` is part of the layout options and of the
	/// collection cache key, so the reflow after a real change picks the new
	/// faces up without a restart. The export panel reads this same
	/// configuration, so an export picks them up too.
	pub(super) fn register_fonts(&mut self, stored: usize) {
		if self.fonts_config.ignore_system_fonts {
			return;
		}
		let Some(dir) = crate::fonts::directory() else {
			return;
		};
		if !register_font_dir(&mut self.fonts_config, dir, stored) {
			return;
		}
		let config = self.fonts_config.clone();
		self.ui.set_fonts(&config);
		self.request(false);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_finished_download_bumps_the_revision_only_after_storing_a_file() {
		let dir = PathBuf::from("/tmp/markview-fonts");
		// A job that stored nothing leaves the configuration untouched, so an
		// identical collection is not built and cached again.
		let mut config = FontConfig::default();
		assert!(!register_font_dir(&mut config, dir.clone(), 0));
		assert!(config.directories.is_empty());
		assert_eq!(config.revision, 0);
		// Storing a file adds the directory and advances the revision.
		assert!(register_font_dir(&mut config, dir.clone(), 1));
		assert_eq!(config.directories, vec![dir.clone()]);
		assert_eq!(config.revision, 1);
		// A later job that stores into the known directory still advances,
		// because the same paths now name new faces.
		assert!(register_font_dir(&mut config, dir.clone(), 2));
		assert_eq!(config.directories, vec![dir]);
		assert_eq!(config.revision, 2);
	}
}
