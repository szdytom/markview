mod anchor;
pub(crate) mod android;
mod background;
mod chrome;
mod dm;
mod document;
mod dropdown;
mod export;
mod export_panel;
mod folder;
pub(crate) mod font_panel;
mod font_service;
mod fonts_command;
pub(crate) mod frame;
mod gestures;
mod interaction;
mod launch;
mod lifecycle;
mod open_document;
mod outline;
mod painting;
mod pointer;
mod preferences;
mod preview;
pub(crate) mod search;
mod security;
pub(crate) mod session;
mod settings_load;
mod settings_panel;
pub(crate) mod sidebar;
mod single_instance;
mod status;
mod style_edit;
mod surface;
mod tab_metrics;
mod tab_navigation;
mod tab_strip;
mod tabs;
mod text_input;
mod ui;
mod ui_model;
mod viewport;
mod web_page;
pub(crate) mod widgets;
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
use markview_layout::fonts::FontConfig;
use std::{path::PathBuf, sync::Arc, time::Instant};
use winit::{event_loop::EventLoopProxy, window::Window};

/// How the reader hands an event to its own loop.
///
/// `EventLoopProxy` is what the application uses; a test drives the same
/// handlers without a window server by handing them a stub, which is why the
/// application names this rather than the concrete type. Internal wake-ups
/// can ignore a closed loop; IPC uses `try_send` to acknowledge delivery.
trait SendEvent: Clone + Send + 'static {
	fn try_send(&self, event: Event) -> bool;
	fn send(&self, event: Event) {
		self.try_send(event);
	}
}
impl SendEvent for EventLoopProxy<Event> {
	fn try_send(&self, event: Event) -> bool {
		self.send_event(event).is_ok()
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
	NativeAssetsChanged,
	SystemThemeChanged(Theme),
	SettingsLoaded(Box<settings_load::Completion>),
	ImageSelected {
		document: Option<PathBuf>,
		revision: u64,
		source: String,
		file: Option<PathBuf>,
	},
	WebLoaded {
		url: String,
		path: PathBuf,
		result: anyhow::Result<crate::web_page::Article>,
	},
	FrameFeedback,
	Ready(Box<Update>),
	SearchReady(search::Result),
	Parsed {
		path: PathBuf,
		content_version: u64,
		document: Arc<markview_document::Document>,
	},
	Changed(PathBuf),
	SettingsChanged,
	StylesChanged,
	Open(Option<PathBuf>),
	FolderLoaded {
		generation: u64,
		select: bool,
		result: anyhow::Result<folder::Tree>,
	},
	Activate(Option<PathBuf>),
	DeviceLost,
	AndroidBack,
	AndroidConfiguration,
	AndroidOpenUrl(String),
	AndroidShared(PathBuf),
	#[cfg(debug_assertions)]
	AndroidInspect(std::sync::mpsc::Sender<String>),
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
		snapshot: Box<crate::layout::LayoutSnapshot>,
		path: PathBuf,
		/// What the strips render with; the shared renderer borrows it and
		/// then takes the reading view's sheet back.
		stylesheet: Arc<markview_layout::style::Stylesheet>,
		scale: f32,
	},
	Failed(String),
	Cancelled,
}

/// One live export: the document it follows and the file it rewrites.
pub(super) struct WatchExport {
	pub(super) origin: crate::security::Origin,
	pub(super) source: PathBuf,
	pub(super) output: PathBuf,
}
#[derive(Clone, Debug)]
enum Label {
	Static(&'static str),
	Shared(Arc<str>),
}
impl std::ops::Deref for Label {
	type Target = str;
	fn deref(&self) -> &str {
		match self {
			Self::Static(text) => text,
			Self::Shared(text) => text,
		}
	}
}
impl AsRef<str> for Label {
	fn as_ref(&self) -> &str {
		self
	}
}
impl<T: AsRef<str>> PartialEq<T> for Label {
	fn eq(&self, other: &T) -> bool {
		self.as_ref() == other.as_ref()
	}
}
impl Eq for Label {}
impl From<&'static str> for Label {
	fn from(text: &'static str) -> Self {
		Self::Static(text)
	}
}
impl From<Arc<str>> for Label {
	fn from(text: Arc<str>) -> Self {
		Self::Shared(text)
	}
}

#[derive(Clone)]
struct Button {
	id: markview_ui_core::WidgetId,
	clip: Option<Rect>,
	kind: chrome::components::ButtonKind,
	enabled: bool,
	rect: Rect,
	/// Names the button; drawn only when it has no icon.
	label: Label,
	/// Drawn centered in place of the label when set.
	icon: Option<&'static [markview_layout::scene::IconPath]>,
	/// Drawn after the label, which makes room for it. It marks what the
	/// button does beyond naming it, such as opening a list.
	marker: Option<&'static [markview_layout::scene::IconPath]>,
	/// Whether this button is the current choice in its row.
	active: bool,
	action: Command,
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
	surface_insets: [f32; 4],
	frame: frame::State,
	native_frame: Option<markview_platform::NativeFrame>,
	native_tab_end: std::rc::Rc<std::cell::Cell<Option<(f32, f32)>>>,
	touch_frame: Option<markview_platform::TouchFrame>,
	interaction: InteractionState,
	gestures: gestures::GestureState,
	readers: tabs::Tabs,
	folder: folder::Browser,
	tab_strip: tab_strip::TabStrip,
	tab_metrics: tab_metrics::TabMetrics,
	preview: preview::Preview,
	args: LaunchOptions,
	/// The reader's own font sources: the configured directories plus the
	/// personal download directory. The export panel shapes with the same
	/// sources, so an export matches what the reader shows.
	fonts_config: FontConfig,
	proxy: P,
	persistence: session::Persistence,
	instance_path: Option<PathBuf>,
	instance: Option<single_instance::Listener>,
	window: Option<Arc<Window>>,
	desktop_theme: Option<Theme>,
	renderer: Option<Renderer>,
	worker: Worker,
	search_worker: search::Worker,
	watch: Option<FileWatch>,
	_settings_watch: Option<FileWatch>,
	_styles_watch: Option<FileWatch>,
	ui: TextShaper,
	preferences: preferences::Preferences,
	settings_resources: settings_load::Resources,
	/// What one wheel notch travels on this desktop, read once at startup:
	/// nothing reports the desktop setting changing afterwards.
	wheel_notch: markview_platform::WheelNotch,
	/// The window's Direct Manipulation viewport, when the desktop and the
	/// initialization both offered one; `None` keeps the wheel paths.
	dm: Option<dm::DirectManipulation>,
	font_downloads: font_service::Downloads,
	services: Arc<crate::services::Services>,
	clipboard: markview_platform::Clipboard,
	text_input: text_input::InputState,
	url_input: markview_layout::text_input::TextInput,
	paste_dir: tempfile::TempDir,
	paste_serial: u32,
	/// A temporary document path, or `None` while its URL is downloading.
	web_pages: std::collections::HashMap<String, PathBuf>,
	status: status::Status,
	dialog_open: bool,
	reflow_at: Option<Instant>,
	retry_at: Option<Instant>,
	/// When the next glyph prewarm pass is due, while one is still worth
	/// running. Cleared whenever the reader is scrolling through new content.
	prewarm_at: Option<Instant>,
	first_frame: Option<Update>,
	started: Instant,
	fatal: Option<String>,
	/// The one export in flight, the live watch target and the PNG strip loop.
	export: export::Session,
}
impl<P> Drop for App<P> {
	fn drop(&mut self) {
		self.flush_session();
		self.instance.take();
		self.services.handle.cancel.cancel();
		self.worker.shutdown();
		self.search_worker.shutdown();
		self.export.shutdown();
	}
}

impl<P: SendEvent> App<P> {
	pub(super) fn new(args: LaunchOptions, proxy: P) -> Self {
		// Parse already folded the personal download directory into the font
		// set for the drawing modes. This copy is the reader's own, so a
		// download can extend it without touching what the command line named.
		let fonts_config = args.options.fonts.clone();
		let done = proxy.clone();
		let parsed_proxy = proxy.clone();
		let services = Arc::new(crate::services::Services::new(4));
		if args.mode == crate::cli::Mode::Window
			&& markview_platform::portal_theme()
		{
			let proxy = proxy.clone();
			let cancel = services.handle.cancel.clone();
			services.handle.submit(async move {
				tokio::select! {
					_ = cancel.cancelled() => {},
					result = markview_platform::watch_theme(move |theme| {
						proxy.send(Event::SystemThemeChanged(reader_theme(theme)));
					}) => {
						if let Err(error) = result {
							log::debug!("Desktop appearance unavailable: {error}");
						}
					},
				}
			});
		}
		let worker = Worker::with_services_and_parsed(
			services.clone(),
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
		services.handle.configure_http(&preferences.values);
		let instance_path = preferences
			.path()
			.map(|config| config.with_file_name("instance.lock"));
		let settings_watch = preferences.path().map(|path| {
			let proxy = proxy.clone();
			FileWatch::new(path.to_path_buf(), move || {
				proxy.send(Event::SettingsChanged);
			})
		});
		let settings_resources = settings_load::Resources::new();
		let styles_watch = crate::stylesheet::directory().map(|dir| {
			let proxy = proxy.clone();
			FileWatch::directory(dir, move || {
				proxy.send(Event::StylesChanged);
			})
		});

		Self {
			surface_insets: [0.0; 4],
			frame: frame::State::new(preferences.values.window_layout),
			native_frame: None,
			native_tab_end: Default::default(),
			touch_frame: None,
			interaction: InteractionState::default(),
			gestures: gestures::GestureState::default(),
			readers: tabs::Tabs::default(),
			folder: Default::default(),
			tab_strip: Default::default(),
			tab_metrics: Default::default(),
			preview: Default::default(),
			args,
			fonts_config,
			proxy,
			persistence: session::Persistence {
				path: preferences
					.path()
					.map(|p| p.with_file_name("session.json")),
				..Default::default()
			},
			instance_path,
			instance: None,
			window: None,
			desktop_theme: None,
			renderer: None,
			worker,
			search_worker,
			watch: None,
			_settings_watch: settings_watch,
			_styles_watch: styles_watch,
			ui,
			preferences,
			settings_resources,
			wheel_notch: markview_platform::wheel_notch(),
			dm: None,
			font_downloads: Default::default(),
			services,
			clipboard: Default::default(),
			text_input: Default::default(),
			url_input: Default::default(),
			paste_dir: tempfile::tempdir()
				.expect("create clipboard paste directory"),
			paste_serial: 0,
			web_pages: Default::default(),
			status: status::Status::default(),
			dialog_open: false,
			reflow_at: None,
			retry_at: None,
			prewarm_at: None,
			first_frame: None,
			started: Instant::now(),
			fatal: None,
			export: export::Session::default(),
		}
	}
	/// The desktop preference; `None` when the platform does not report one.
	fn system_theme(&self) -> Option<Theme> {
		if markview_platform::portal_theme() {
			self.desktop_theme
		} else {
			self.window.as_ref()?.theme().map(reader_theme)
		}
	}

	fn reload_styles(&mut self) {
		self.update_media();
		if let Some(reflow) = self.preferences.reload_styles(&mut self.ui) {
			if let Some(renderer) = &mut self.renderer {
				renderer
					.set_stylesheet(self.preferences.values.stylesheet.clone());
			}
			if reflow {
				self.request(false);
			}
		}
		if markview_platform::touch_layout() {
			use markview_layout::style::{ColorField, Condition};
			let [r, g, b, _] = self
				.ui
				.stylesheet
				.color(Condition::Toolbar, ColorField::Background);
			let mode = if r * 0.2126 + g * 0.7152 + b * 0.0722 > 0.5 {
				"light"
			} else {
				"dark"
			};
			if let Err(error) =
				markview_platform::android::call_string("systemBars", mode)
			{
				log::warn!("Android system bars: {error:#}");
			}
		}
		self.settings_resources.invalidate();
		self.refresh_settings_resources(false);
	}
	pub(super) fn dimensions(&self) -> (f32, f32, f32) {
		self.window.as_ref().map_or((1200.0, 800.0, 1.0), |w| {
			let s = w.scale_factor() as f32;
			let size = w.inner_size();
			let insets = self.insets();
			(
				(size.width as f32 / s - insets[0] - insets[2]).max(1.0),
				(size.height as f32 / s - insets[1] - insets[3]).max(1.0),
				s,
			)
		})
	}
	fn update_media(&mut self) -> bool {
		use markview_layout::style::{Condition, StyleTarget, TextAppearance};
		let (width, height) =
			self.window.as_ref().map_or((1200.0, 800.0), |window| {
				let size = window.inner_size();
				(size.width as f32, size.height as f32)
			});
		let media = crate::stylesheet::media_context(StyleTarget::Ui)
			.with_size(width, height);
		if self.preferences.values.stylesheet.media() == media {
			return false;
		}
		let sheet = self.preferences.values.stylesheet.for_media(media);
		let changed = sheet.rules != self.preferences.values.stylesheet.rules;
		let sheet = Arc::new(sheet);
		self.preferences.values.stylesheet = sheet.clone();
		if changed {
			self.ui.set_stylesheet(sheet.clone());
			self.ui.appearance =
				sheet.text(&TextAppearance::default(), Condition::Ui);
			if let Some(renderer) = &mut self.renderer {
				renderer.set_stylesheet(sheet);
			}
		}
		changed
	}
	pub(super) fn insets(&self) -> [f32; 4] {
		if markview_platform::touch_layout()
			&& let Some(window) = &self.window
		{
			let size = window.inner_size();
			return markview_platform::android::insets(
				size.width,
				size.height,
				window.scale_factor() as f32,
			);
		}
		[0.0; 4]
	}
	pub(super) fn view_geometry(&self) -> markview_layout::scene::Viewport {
		let (width, height, _) = self.dimensions();
		let inset = if chrome::folder::docked(width, self.tab_strip.phone) {
			self.folder_chrome_inset()
		} else {
			0.
		};
		markview_layout::scene::Viewport {
			width,
			height,
			left: inset
				+ ((width - inset - self.readers.session.snapshot.width) / 2.0)
					.max(20.0),
			top: self.content_top(),
			bottom: self.bottom(),
			scroll: self.readers.session.scrolling.offset,
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
			self.dimensions().0 - self.folder_inset(),
			self.args.options.greedy,
			&self.fonts_config,
		);
		if self.tab_strip.phone {
			options.width = (self.dimensions().0 - 40.0).max(80.0);
		}
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

fn native_event(event: markview_platform::NativeEvent) -> Event {
	use markview_platform::NativeEvent;
	match event {
		NativeEvent::Open(path) => Event::Open(Some(path)),
		NativeEvent::Shared(path) => Event::AndroidShared(path),
		NativeEvent::OpenUrl(url) => Event::AndroidOpenUrl(url),
		NativeEvent::Back => Event::AndroidBack,
		NativeEvent::Configuration => Event::AndroidConfiguration,
		NativeEvent::AssetsChanged => Event::NativeAssetsChanged,
		#[cfg(debug_assertions)]
		NativeEvent::Inspect(send) => Event::AndroidInspect(send),
	}
}

fn reader_theme(theme: winit::window::Theme) -> Theme {
	match theme {
		winit::window::Theme::Dark => Theme::Dark,
		winit::window::Theme::Light => Theme::Light,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn shutdown_releases_instance_ownership_before_joining_an_export() {
		#[derive(Clone)]
		struct Proxy;
		impl SendEvent for Proxy {
			fn try_send(&self, _: Event) -> bool {
				true
			}
		}

		let dir = tempfile::tempdir().unwrap();
		let lock = dir.path().join("instance.lock");
		let single_instance::Start::Primary(primary) =
			single_instance::start(&lock, false, None).unwrap()
		else {
			panic!()
		};
		let mut app = App::new(
			LaunchOptions {
				options: crate::test_support::options(),
				..Default::default()
			},
			Proxy,
		);
		app.instance = Some(primary.listen(|_| true));
		let (tx, rx) = std::sync::mpsc::channel();
		app.export.install_thread(std::thread::spawn(move || {
			let deadline = Instant::now() + std::time::Duration::from_secs(2);
			while Instant::now() < deadline {
				if matches!(
					single_instance::start(&lock, false, None).unwrap(),
					single_instance::Start::Primary(_)
				) {
					tx.send(true).unwrap();
					return;
				}
				std::thread::sleep(std::time::Duration::from_millis(25));
			}
			tx.send(false).unwrap();
		}));
		drop(app);
		assert!(rx.recv().unwrap());
	}

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
