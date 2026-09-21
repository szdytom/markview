mod anchor;
mod chrome;
mod document;
mod export;
mod fonts_command;
mod icon;
mod interaction;
mod launch;
mod lifecycle;
mod outline;
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
use crate::cli::{LaunchOptions, Mode};
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
	Changed(PathBuf),
	SettingsChanged,
	StylesChanged,
	Open(Option<PathBuf>),
	DeviceLost,
	Exported(Box<ExportOutcome>),
	Fonts(Box<crate::fonts::Progress>),
	/// A whole download finished, with what it stored.
	FontsSettled(Box<crate::fonts::Summary>),
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

/// The reader's font sources: the configured set plus the personal download
/// directory. Exports are built from `args.options.fonts`, which never gains
/// that directory.
fn reader_fonts(args: &LaunchOptions, personal: Option<PathBuf>) -> FontConfig {
	let mut fonts = args.options.fonts.clone();
	if args.mode == Mode::Window
		&& !fonts.ignore_system_fonts
		&& let Some(dir) = personal
	{
		fonts.directories.push(dir);
	}
	fonts
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

struct App {
	interaction: InteractionState,
	readers: tabs::Tabs,
	tab_strip: tab_strip::TabStrip,
	tab_metrics: tab_metrics::TabMetrics,
	args: LaunchOptions,
	/// The reader's own font sources: the configured directories plus the
	/// personal download directory. Exports keep using `args.options.fonts`,
	/// so a download can never change reproducible output.
	fonts_config: FontConfig,
	proxy: EventLoopProxy<Event>,
	window: Option<Arc<Window>>,
	renderer: Option<Renderer>,
	worker: Worker,
	watch: Option<FileWatch>,
	_settings_watch: Option<FileWatch>,
	_styles_watch: Option<FileWatch>,
	ui: TextShaper,
	preferences: preferences::Preferences,
	/// What one wheel notch travels on this desktop, read once at startup:
	/// nothing reports the desktop setting changing afterwards.
	wheel_notch: crate::platform::WheelNotch,
	/// The downloadable families the shown stylesheets declare, with what is
	/// already on disk.
	font_catalog: Vec<crate::fonts::Family>,
	/// The families being downloaded right now, by id.
	font_jobs: std::collections::HashMap<String, crate::fonts::Progress>,
	/// Families the reader has asked to stop.
	font_cancel: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
	/// A quiet note about the last download: offline, or nothing to do.
	font_note: Option<String>,
	/// The Fonts tab's filters: a declaring sheet id, and a state.
	font_source_filter: Option<String>,
	font_status_filter: Option<crate::fonts::State>,
	/// How many files this process has stored, which is what tells a finished
	/// download that the reader's own font set changed.
	font_stored: usize,
	clipboard: crate::platform::Clipboard,
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
impl App {
	pub(super) fn new(
		args: LaunchOptions,
		proxy: EventLoopProxy<Event>,
	) -> Self {
		let personal = crate::fonts::directory().filter(|dir| dir.is_dir());
		let fonts_config = reader_fonts(&args, personal);
		let done = proxy.clone();
		let worker = Worker::with_images(
			args.offline,
			fonts_config.clone(),
			move |update| {
				let _ = done.send_event(Event::Ready(Box::new(update)));
			},
		);
		let mut ui = TextShaper::with_fonts(fonts_config.clone());
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
		let builtin = markview_core::style::Stylesheet::builtin();
		let font_catalog = crate::fonts::catalog(
			std::iter::once(("builtin", builtin.font_families.as_slice()))
				.chain(preferences.style_entries.iter().map(|entry| {
					(entry.id.as_str(), entry.font_families.as_slice())
				})),
			crate::fonts::directory().as_deref(),
			&args.options.fonts,
		);
		Self {
			interaction: InteractionState::default(),
			readers: tabs::Tabs::default(),
			tab_strip: Default::default(),
			tab_metrics: Default::default(),
			args,
			fonts_config,
			proxy,
			window: None,
			renderer: None,
			worker,
			watch: None,
			_settings_watch: settings_watch,
			_styles_watch: styles_watch,
			ui,
			preferences,
			wheel_notch: crate::platform::wheel_notch(),
			font_catalog,
			font_jobs: std::collections::HashMap::new(),
			font_cancel: Arc::new(std::sync::Mutex::new(
				std::collections::HashSet::new(),
			)),
			font_note: None,
			font_source_filter: None,
			font_status_filter: None,
			font_stored: 0,
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

	/// The catalogue positions the Fonts page shows, filters applied.
	pub(super) fn shown_fonts(&self) -> Vec<usize> {
		self.font_catalog
			.iter()
			.enumerate()
			.filter(|(_, entry)| {
				self.font_source_filter
					.as_ref()
					.is_none_or(|source| entry.owners.contains(source))
			})
			.filter(|(_, entry)| {
				self.font_status_filter
					.is_none_or(|state| entry.state == state)
			})
			.map(|(index, _)| index)
			.collect()
	}

	/// The family id behind one shown position.
	pub(super) fn shown_font_id(&self, index: usize) -> Option<String> {
		self.shown_fonts()
			.get(index)
			.map(|position| self.font_catalog[*position].family.id.clone())
	}

	/// Every family the Fonts page shows.
	pub(super) fn shown_font_ids(&self) -> Vec<String> {
		self.shown_fonts()
			.iter()
			.map(|position| self.font_catalog[*position].family.id.clone())
			.collect()
	}

	/// The declaring sheets the Fonts page cycles its source filter through.
	pub(super) fn font_sources(&self) -> Vec<String> {
		let mut out: Vec<String> = Vec::new();
		for entry in &self.font_catalog {
			for owner in &entry.owners {
				if !out.contains(owner) {
					out.push(owner.clone());
				}
			}
		}
		out
	}

	/// Advances the source filter to the next declaring sheet, or to all.
	pub(super) fn next_font_source(&mut self) {
		let sources = self.font_sources();
		let next = match &self.font_source_filter {
			None => sources.first().cloned(),
			Some(current) => {
				let at = sources.iter().position(|source| source == current);
				at.and_then(|at| sources.get(at + 1)).cloned()
			}
		};
		self.font_source_filter = next;
		self.interaction.fonts_scroll = 0.0;
	}

	/// Advances the status filter through missing, provided, downloaded, all.
	pub(super) fn next_font_status(&mut self) {
		self.font_status_filter = match self.font_status_filter {
			None => Some(crate::fonts::State::Missing),
			Some(crate::fonts::State::Missing) => {
				Some(crate::fonts::State::Provided)
			}
			Some(crate::fonts::State::Provided) => {
				Some(crate::fonts::State::Downloaded)
			}
			Some(crate::fonts::State::Downloaded) => None,
		};
		self.interaction.fonts_scroll = 0.0;
	}

	/// Rebuilds the catalogue the Fonts page shows.
	///
	/// The builtin recommendations come first and every catalogued sheet
	/// follows, so a sheet that redefines a builtin family replaces it whole.
	pub(super) fn refresh_font_catalog(&mut self) {
		let builtin = markview_core::style::Stylesheet::builtin();
		let sheets =
			std::iter::once(("builtin", builtin.font_families.as_slice()))
				.chain(self.preferences.style_entries.iter().map(|entry| {
					(entry.id.as_str(), entry.font_families.as_slice())
				}));
		let dir = crate::fonts::directory();
		self.font_catalog = crate::fonts::catalog(
			sheets,
			dir.as_deref(),
			&self.args.options.fonts,
		);
	}

	/// Starts downloading the named families, or reports why nothing can run.
	///
	/// Every family comes from the catalogued stylesheets and the builtin
	/// recommendations, so a download is exactly that set; nothing here runs on
	/// its own. A family already being downloaded is left to the run that owns
	/// it, so two runs never write the same files.
	pub(super) fn download_fonts(
		&mut self,
		ids: &[String],
		scope: crate::fonts::Scope,
	) {
		let busy = ids.iter().any(|id| self.font_jobs.contains_key(id));
		let ids: Vec<String> = ids
			.iter()
			.filter(|id| !self.font_jobs.contains_key(*id))
			.cloned()
			.collect();
		let missing: Vec<markview_core::style::FontFamily> =
			crate::fonts::select(&self.font_catalog, &ids, scope)
				.into_iter()
				.cloned()
				.collect();
		if missing.is_empty() {
			// A family already being downloaded is not "downloaded", so the
			// note has to tell the two apart.
			self.font_note = Some(
				if busy {
					"Those families are already downloading"
				} else {
					"Everything selected is already downloaded"
				}
				.into(),
			);
			self.redraw();
			return;
		}
		if self.args.offline {
			self.font_note =
				Some("Offline: font downloads are unavailable".into());
			self.notify("Offline: cannot download fonts", true, 4);
			return;
		}
		let Some(dir) = crate::fonts::directory() else {
			self.font_note = Some("No user configuration directory".into());
			self.redraw();
			return;
		};
		for family in &missing {
			self.font_jobs.insert(
				family.id.clone(),
				crate::fonts::Progress::queued(&family.id),
			);
			if let Ok(mut cancel) = self.font_cancel.lock() {
				cancel.remove(&family.id);
			}
		}
		self.font_note = None;
		let proxy = self.proxy.clone();
		let cancels = self.font_cancel.clone();
		std::thread::spawn(move || {
			let transport = match crate::images::Downloader::new() {
				Ok(transport) => transport,
				Err(error) => {
					let reason = format!("{error:#}");
					let failed = missing
						.iter()
						.map(|family| (family.id.clone(), reason.clone()))
						.collect();
					let _ = proxy.send_event(Event::FontsSettled(Box::new(
						crate::fonts::Summary {
							requested: missing
								.iter()
								.map(|family| family.id.clone())
								.collect(),
							failed,
							..Default::default()
						},
					)));
					return;
				}
			};
			let cancel: Arc<dyn Fn(&str) -> bool + Send + Sync> =
				Arc::new(move |id: &str| {
					cancels.lock().is_ok_and(|cancel| cancel.contains(id))
				});
			let summary = crate::fonts::run(
				&missing,
				&dir,
				&transport,
				crate::fonts::DEFAULT_JOBS,
				cancel,
				&mut |progress| {
					let _ = proxy.send_event(Event::Fonts(Box::new(progress)));
				},
			);
			let _ = proxy.send_event(Event::FontsSettled(Box::new(summary)));
		});
		self.redraw();
	}

	/// Asks one family's running download to stop.
	pub(super) fn cancel_font(&mut self, id: &str) {
		if let Ok(mut cancel) = self.font_cancel.lock() {
			cancel.insert(id.to_owned());
		}
		self.redraw();
	}

	/// Opens the download directory, creating it when it does not exist yet.
	pub(super) fn open_fonts_folder(&mut self) {
		let result = crate::fonts::directory()
			.ok_or_else(|| anyhow::anyhow!("No user configuration directory"))
			.and_then(|dir| {
				std::fs::create_dir_all(&dir)?;
				open::that_detached(dir)?;
				Ok(())
			});
		if let Err(error) = result {
			self.font_note = Some(format!("{error:#}"));
		}
		self.redraw();
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
	/// faces up without a restart. Exports keep `args.options.fonts`, which
	/// never gains the directory.
	pub(super) fn register_fonts(&mut self) {
		if self.fonts_config.ignore_system_fonts {
			return;
		}
		let stored = std::mem::take(&mut self.font_stored);
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

	/// Exports are built from the same configuration the command line parsed,
	/// so a personal download can never reach one.
	#[test]
	fn the_personal_directory_joins_the_reader_but_not_the_export_config() {
		let dir = PathBuf::from("/tmp/markview-fonts");
		let args = LaunchOptions::default();
		let reader = reader_fonts(&args, Some(dir.clone()));
		assert_eq!(reader.directories, vec![dir.clone()]);
		assert!(args.options.fonts.directories.is_empty());

		// A reproducible run keeps its pinned set in the window too.
		let pinned = LaunchOptions {
			options: LayoutOptions {
				fonts: FontConfig {
					ignore_system_fonts: true,
					..Default::default()
				},
				..Default::default()
			},
			..Default::default()
		};
		let reader = reader_fonts(&pinned, Some(dir));
		assert!(reader.directories.is_empty());
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
