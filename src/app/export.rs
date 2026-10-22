//! The reader's export panel actions and the background jobs they start.
//!
//! Every export runs off the event loop: read, parse and layout happen on a
//! spawned thread, and only the PNG strips come back to the main thread, where
//! the one GPU device lives. Nothing here changes the reader's layout options.
use super::{App, Event, ExportOutcome};
#[cfg(test)]
use crate::state::Command;
use crate::{
	export::{self, draw_tile, write_png},
	layout::LayoutSnapshot,
	settings::{ExportFormat, ExportSettings, FontDefOverride},
};
use markview_layout::{
	fonts::FontConfig,
	paginate::PT_PER_PX,
	style::{CjkType, PageStyle, Stylesheet},
};
use std::{
	path::{Path, PathBuf},
	sync::Arc,
	time::{Duration, Instant},
};

/// The paper presets the panel offers. Any other size can be typed into
/// `settings.toml` and still validates.
pub(super) const PAPER_PRESETS: [(&str, &str); 4] = [
	("A4", "a4"),
	("A5", "a5"),
	("Letter", "letter"),
	("Legal", "legal"),
];

/// Margin presets, top/right/bottom/left in millimetres. The first is the
/// print sheet's own default, so the panel can return to it.
pub(super) const MARGIN_PRESETS: [([f32; 4], &str); 4] = [
	(PageStyle::DEFAULT_MARGIN_MM, "22/20"),
	([15.0; 4], "15 mm"),
	([20.0; 4], "20 mm"),
	([25.0; 4], "25 mm"),
];

/// PNG device pixels per layout pixel.
pub(super) const SCALE_PRESETS: [(f32, &str); 2] = [(1.0, "1×"), (2.0, "2×")];

/// Where one job writes to.
enum Destination {
	/// Ask the reader asynchronously before exporting.
	Ask {
		stem: String,
		directory: Option<PathBuf>,
	},
	/// A watch rebuild writes where the first export did.
	Path(PathBuf),
}

/// A PNG layout waiting to be drawn, one strip per frame.
pub(super) struct PngExport {
	snapshot: LayoutSnapshot,
	path: PathBuf,
	/// The stylesheet the strips render with, so the reading view's own colors
	/// are never borrowed.
	stylesheet: Arc<Stylesheet>,
	plan: export::PngPlan,
	scale: f32,
	/// Paper's left margin in layout pixels, where the text column starts.
	left: f32,
	rgba: Vec<u8>,
	next: usize,
	/// Keep following the document once this image lands.
	watch: bool,
	/// A live rebuild does not reopen the file it writes again.
	rebuild: bool,
}

/// The export bookkeeping: the one job in flight, the file a watch keeps
/// rewriting, and the PNG strips waiting to be drawn.
///
/// The app owns the host I/O that starts and finishes a job; this type owns
/// the state that spans events, so `App` carries one field instead of eight.
#[derive(Default)]
pub(super) struct Session {
	running: bool,
	thread: Option<std::thread::JoinHandle<()>>,
	source: Option<(PathBuf, crate::security::Origin)>,
	png: Option<PngExport>,
	watch: Option<super::WatchExport>,
	watch_at: Option<Instant>,
	rebuild: bool,
	watch_request: bool,
}

impl Session {
	/// Whether a pending or in-flight export follows `path`.
	fn watches_source(&self, path: &Path) -> bool {
		self.watch
			.as_ref()
			.is_some_and(|watch| watch.source == path)
			|| (self.running
				&& self
					.source
					.as_ref()
					.is_some_and(|(source, _)| source == path)
				&& (self.watch_request
					|| self.png.as_ref().is_some_and(|job| job.watch)))
	}

	/// Remembers a document change for an active or in-flight watch export.
	pub(super) fn schedule_watch_export(&mut self, path: &Path) {
		if self.watches_source(path) {
			self.watch_at = Some(Instant::now() + Duration::from_millis(250));
		}
	}

	/// Stops a live export; the file it wrote stays where it is.
	pub(super) fn disarm_watch(&mut self) {
		self.watch = None;
		self.watch_at = None;
	}

	/// Points the live export at a file that was just written.
	fn arm_watch(&mut self, output: &Path) {
		let Some((source, origin)) = self.source.clone() else {
			return;
		};
		self.watch = Some(super::WatchExport {
			origin,
			source,
			output: output.to_path_buf(),
		});
		// Keep a rebuild queued by a save that arrived while this export was
		// in flight; it still needs to run after the watch is armed.
	}

	fn tiles(&self) -> usize {
		self.png.as_ref().map_or(0, |job| job.plan.tiles.len())
	}

	/// Whether the watch follows this document and origin.
	pub(super) fn watches_document(
		&self,
		source: &Path,
		origin: &crate::security::Origin,
	) -> bool {
		self.watch.as_ref().is_some_and(|watch| {
			watch.source == source && &watch.origin == origin
		})
	}

	/// Whether an export is being prepared or written.
	pub(super) fn is_running(&self) -> bool {
		self.running
	}

	/// Whether a live export keeps rewriting its file.
	pub(super) fn watching(&self) -> bool {
		self.watch.is_some()
	}

	/// When a watched document change becomes a rebuild, if one is queued.
	pub(super) fn watch_deadline(&self) -> Option<Instant> {
		self.watch_at
	}

	/// Consumes a queued rebuild once its deadline passes.
	pub(super) fn take_due_watch(&mut self, now: Instant) -> bool {
		if self.watch_at.is_some_and(|due| due <= now) {
			self.watch_at = None;
			true
		} else {
			false
		}
	}

	/// Joins the export thread. The app's `Drop` calls this after cancelling
	/// the services and shutting the workers down.
	pub(super) fn shutdown(&mut self) {
		if let Some(thread) = self.thread.take() {
			let _ = thread.join();
		}
	}

	/// Installs a joinable thread for the shutdown-order test.
	#[cfg(test)]
	pub(super) fn install_thread(
		&mut self,
		thread: std::thread::JoinHandle<()>,
	) {
		self.thread = Some(thread);
	}
}

impl<P: super::SendEvent> App<P> {
	/// Shows a transient status line; a zero `seconds` leaves it without a
	/// deadline.
	pub(super) fn notify(&mut self, message: &str, error: bool, seconds: u64) {
		self.status.set(message, error, seconds);
		self.redraw();
	}

	/// Asks for a destination, then starts the export on a worker thread.
	/// `watch` keeps rewriting the chosen file whenever the document changes.
	pub(super) fn start_export(&mut self, watch: bool) {
		if self.export.is_running() || self.dialog_open {
			self.notify(
				self.preferences.values.lang().status_export_running(),
				true,
				4,
			);
			return;
		}
		let Some(path) = self.readers.session.path.clone() else {
			self.notify(
				self.preferences.values.lang().status_open_first(),
				true,
				4,
			);
			return;
		};
		let settings = self.preferences.export.clone();
		if let Err(error) = settings.validate() {
			self.notify(
				&self
					.preferences
					.values
					.lang()
					.status_export_settings_invalid(error),
				true,
				6,
			);
			return;
		}
		// The panel closes so the document and the status line stay visible
		// while the file is written.
		self.interaction.show_panel(crate::state::PanelPage::Closed);
		self.interaction.widgets.focus = None;
		self.interaction.pointer.release();
		self.dialog_open = true;
		self.export.rebuild = false;
		self.export.watch_request = watch;
		self.export.running = true;
		self.notify(
			self.preferences.values.lang().status_exporting(),
			false,
			3600,
		);
		self.refresh_hover();

		let directory = path.parent().map(Path::to_path_buf);
		let stem = path
			.file_stem()
			.map(|stem| stem.to_string_lossy().into_owned())
			.filter(|stem| !stem.is_empty())
			.unwrap_or_else(|| "document".into());
		self.spawn_export(path, Destination::Ask { stem, directory }, settings);
	}

	/// Re-exports a watched document to the file its first export chose.
	pub(super) fn start_watch_export(&mut self) {
		if self.export.is_running() || self.dialog_open {
			// The save that arrived mid-export is not lost; it becomes the
			// next rebuild once the current one is done.
			self.export.watch_at =
				Some(Instant::now() + Duration::from_millis(250));
			return;
		}
		let Some(watch) = &self.export.watch else {
			return;
		};
		let (path, output) = (watch.source.clone(), watch.output.clone());
		if self.readers.session.path.as_ref() != Some(&path)
			|| self.readers.session.security.origin != watch.origin
		{
			return;
		}
		let settings = self.preferences.export.clone();
		if settings.validate().is_err() {
			return;
		}
		self.export.rebuild = true;
		self.export.watch_request = true;
		self.export.running = true;
		self.notify(
			self.preferences.values.lang().status_reexporting(),
			false,
			3600,
		);
		self.spawn_export(path, Destination::Path(output), settings);
	}

	/// Spawns the export thread. It owns everything the job needs, including
	/// the stylesheet the export renders with.
	fn spawn_export(
		&mut self,
		path: PathBuf,
		destination: Destination,
		settings: ExportSettings,
	) {
		let proxy = self.proxy.clone();
		let offline = self.args.offline;
		let security = self.readers.session.security.clone();
		self.export.source = Some((path.clone(), security.origin.clone()));
		// The reader's own font set, so the export shapes with the personal
		// download directory exactly like the display does.
		let fonts = self.fonts_config.clone();
		let metadata = export::MetadataOverrides::with_title(
			self.readers.session.export_title.text(),
		);
		let cjk = self.preferences.values.cjk_type;
		let overrides = self.preferences.values.fontdef_overrides.clone();
		let services = self.services.clone();
		let output = match &destination {
			Destination::Ask { stem, directory } => Some(wait_for_output(
				&services.handle,
				choose_output(
					stem,
					&settings,
					directory.as_deref(),
					self.window.as_deref(),
				),
			)),
			Destination::Path(_) => None,
		};
		if let Some(thread) = self.export.thread.take() {
			let _ = thread.join();
		}
		self.export.thread = Some(std::thread::spawn(move || {
			let outcome = match destination {
				Destination::Ask { .. } => {
					match output.unwrap().recv().ok().flatten() {
						Some(output) => run(
							&path, &output, &settings, metadata, fonts, cjk,
							&overrides, offline, services, security,
						),
						None => ExportOutcome::Cancelled,
					}
				}
				Destination::Path(output) => run(
					&path, &output, &settings, metadata, fonts, cjk,
					&overrides, offline, services, security,
				),
			};
			proxy.send(Event::Exported(Box::new(outcome)));
		}));
	}

	/// Receives one export result on the event loop.
	pub(super) fn export_finished(&mut self, outcome: ExportOutcome) {
		self.dialog_open = false;
		let rebuild = std::mem::take(&mut self.export.rebuild);
		let watch = std::mem::take(&mut self.export.watch_request);
		match outcome {
			ExportOutcome::Written { path, detail } => {
				self.export.running = false;
				let lang = self.preferences.values.lang();
				let message = lang.status_exported(
					detail,
					path.display(),
					if rebuild || watch {
						lang.status_export_watching()
					} else {
						""
					},
				);
				if rebuild {
					if let Err(error) = markview_platform::android::call_string(
						"publishExport",
						&path.to_string_lossy(),
					) {
						log::error!("Export handoff: {error:#}");
					}
					self.notify(&message, false, 6);
				} else {
					self.open_export(&path, message);
				}
				if watch {
					self.export.arm_watch(&path);
				} else {
					self.export.disarm_watch();
				}
			}
			ExportOutcome::PngReady {
				snapshot,
				path,
				stylesheet,
				scale,
			} => {
				self.start_png_export(
					*snapshot, path, stylesheet, scale, watch, rebuild,
				);
			}
			ExportOutcome::Failed(error) => {
				self.export.running = false;
				self.notify(
					&self.preferences.values.lang().status_export_failed(error),
					true,
					8,
				);
			}
			ExportOutcome::Cancelled => {
				self.export.running = false;
				self.notify("", false, 0);
			}
		}
	}

	/// Hands a written export to the operating system, so the reader sees the
	/// result. A platform that cannot start a viewer does not undo the file,
	/// so its failure is reported beside the export's own status.
	fn open_export(&mut self, path: &Path, message: String) {
		match markview_platform::open_external(path) {
			Ok(()) => self.notify(&message, false, 6),
			Err(error) => self.notify(
				&self
					.preferences
					.values
					.lang()
					.status_open_failed(message, error.to_string()),
				true,
				8,
			),
		}
	}

	/// Turns a laid-out PNG into strips and a destination for the frame loop.
	fn start_png_export(
		&mut self,
		snapshot: LayoutSnapshot,
		path: PathBuf,
		stylesheet: Arc<Stylesheet>,
		scale: f32,
		watch: bool,
		rebuild: bool,
	) {
		let (geometry, max_tile) = match (
			&self.renderer,
			markview_layout::paginate::PageGeometry::from_style(
				stylesheet.page(),
			),
		) {
			(Some(renderer), Ok(geometry)) => {
				(geometry, renderer.max_texture_dimension_2d())
			}
			(None, _) => {
				let reason =
					self.preferences.values.lang().status_gpu_not_ready();
				self.fail_png_export(reason);
				return;
			}
			(_, Err(error)) => {
				self.fail_png_export(&format!("{error:#}"));
				return;
			}
		};
		let plan =
			match export::plan(&geometry, snapshot.height, scale, max_tile) {
				Ok(plan) => plan,
				Err(error) => {
					self.fail_png_export(&format!("{error:#}"));
					return;
				}
			};
		let bytes = plan.width_px as usize * plan.height_px as usize * 4;
		self.export.png = Some(PngExport {
			snapshot,
			path,
			stylesheet,
			scale,
			left: geometry.margin_pt[3] / PT_PER_PX,
			rgba: vec![0; bytes],
			next: 0,
			watch,
			rebuild,
			plan,
		});
		self.status.set_timed(
			self.preferences
				.values
				.lang()
				.status_exporting_png(0, self.export.tiles()),
			3600,
		);
		self.redraw();
	}

	fn fail_png_export(&mut self, reason: &str) {
		self.export.png = None;
		self.export.running = false;
		self.notify(
			&self.preferences.values.lang().status_export_failed(reason),
			true,
			8,
		);
	}

	/// Draws one strip of a waiting PNG. Called once per frame so the window
	/// stays responsive and the status line can count the strips.
	pub(super) fn advance_png_export(&mut self) {
		let Some(mut job) = self.export.png.take() else {
			return;
		};
		if job.next >= job.plan.tiles.len() {
			self.finish_png_export(job);
			return;
		}
		let Some(renderer) = &mut self.renderer else {
			self.export.png = Some(job);
			let reason = self.preferences.values.lang().status_gpu_not_ready();
			self.fail_png_export(reason);
			return;
		};
		let tile = job.plan.tiles[job.next];
		let theme = self.preferences.values.theme;
		if let Err(error) = draw_tile(
			renderer,
			&job.snapshot,
			&job.plan,
			&job.stylesheet,
			tile,
			job.scale,
			job.left,
			theme,
			&mut job.rgba,
		) {
			self.export.png = Some(job);
			self.fail_png_export(&format!("{error:#}"));
			return;
		}
		job.next += 1;
		if job.next >= job.plan.tiles.len() {
			self.finish_png_export(job);
		} else {
			self.status.set_timed(
				self.preferences
					.values
					.lang()
					.status_exporting_png(job.next, job.plan.tiles.len()),
				3600,
			);
			self.export.png = Some(job);
			self.redraw();
		}
	}

	fn finish_png_export(&mut self, job: PngExport) {
		let outcome = write_png(
			&job.path,
			&job.rgba,
			job.plan.width_px,
			job.plan.height_px,
		);
		self.export.running = false;
		let path = job.path.clone();
		match outcome {
			Ok(()) => {
				let lang = self.preferences.values.lang();
				let message = lang.status_exported_png(
					job.plan.width_px,
					job.plan.height_px,
					path.display(),
					if job.rebuild || job.watch {
						lang.status_export_watching()
					} else {
						""
					},
				);
				if job.rebuild {
					if let Err(error) = markview_platform::android::call_string(
						"publishExport",
						&path.to_string_lossy(),
					) {
						log::error!("Export handoff: {error:#}");
					}
					self.notify(&message, false, 6);
				} else {
					self.open_export(&path, message);
				}
				if job.watch {
					self.export.arm_watch(&path);
				} else {
					self.export.disarm_watch();
				}
			}
			Err(error) => self.notify(
				&self
					.preferences
					.values
					.lang()
					.status_export_failed(format!("{error:#}")),
				true,
				8,
			),
		}
	}
}

/// Construct native dialogs on the UI thread; await their result on the I/O service.
fn choose_output(
	stem: &str,
	settings: &ExportSettings,
	directory: Option<&Path>,
	window: Option<&winit::window::Window>,
) -> impl Future<Output = Option<PathBuf>> + Send + use<> {
	let lang = crate::lang::Lang::default();
	let (extension, label) = match settings.format {
		ExportFormat::Pdf => ("pdf", lang.export_pdf()),
		ExportFormat::Png => ("png", lang.export_png()),
	};
	markview_platform::choose_output(
		format!("{stem}.{extension}"),
		extension,
		label,
		directory,
		window,
	)
}

fn wait_for_output(
	services: &crate::services::Handle,
	dialog: impl std::future::Future<Output = Option<PathBuf>> + Send + 'static,
) -> std::sync::mpsc::Receiver<Option<PathBuf>> {
	let (send, recv) = std::sync::mpsc::channel();
	let cancel = services.cancel.clone();
	services.submit(async move {
		let output = tokio::select! {
			biased;
			_ = cancel.cancelled() => None,
			output = dialog => output,
		};
		let _ = send.send(output);
	});
	recv
}

/// Everything that happens off the event loop for one export.
#[expect(
	clippy::too_many_arguments,
	reason = "Export job captures independent settings, metadata and font configuration"
)]
fn run(
	path: &Path,
	output: &Path,
	settings: &ExportSettings,
	metadata: export::MetadataOverrides,
	fonts: FontConfig,
	cjk: CjkType,
	overrides: &[FontDefOverride],
	offline: bool,
	services: Arc<crate::services::Services>,
	security: crate::security::Security,
) -> ExportOutcome {
	let prepared =
		match export::Config::from_settings(settings, fonts, cjk, overrides)
			.prepare()
		{
			Ok(prepared) => prepared,
			Err(error) => return ExportOutcome::Failed(format!("{error:#}")),
		};
	match settings.format {
		ExportFormat::Pdf => {
			let mut args =
				prepared.pdf_request(path.into(), output.into(), offline);
			args.metadata = metadata;
			args.security = security;
			match crate::pdf::export_with_services(&args, services) {
				Ok(stats) => ExportOutcome::Written {
					path: output.to_path_buf(),
					detail: format!(
						"{} page{}, {} bytes",
						stats.pages,
						if stats.pages == 1 { "" } else { "s" },
						stats.bytes
					),
				},
				Err(error) => ExportOutcome::Failed(format!("{error:#}")),
			}
		}
		ExportFormat::Png => {
			let scale = prepared.scale;
			let stylesheet = prepared.options.stylesheet.clone();
			match export::png_snapshot_with_services(
				path,
				prepared.options,
				offline,
				services,
				security,
			) {
				Ok(snapshot) => ExportOutcome::PngReady {
					snapshot: Box::new(snapshot),
					path: output.to_path_buf(),
					stylesheet,
					scale,
				},
				Err(error) => ExportOutcome::Failed(format!("{error:#}")),
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn panel_size_controls_adjust_across_the_full_export_range() {
		#[derive(Clone)]
		struct Proxy;
		impl super::super::SendEvent for Proxy {
			fn try_send(&self, _: Event) -> bool {
				true
			}
		}
		let mut app = App::new(
			crate::cli::LaunchOptions {
				mode: crate::cli::Mode::Smoke,
				options: crate::test_support::options(),
				..Default::default()
			},
			Proxy,
		);
		for (size, delta, expected) in [
			(72.0, 1, 73.0),
			(72.0, -1, 71.0),
			(8.0, 1, 9.0),
			(8.0, -1, 7.0),
			(1.0, -1, 1.0),
			(1.0, 1, 2.0),
			(200.0, 1, 200.0),
			(200.0, -1, 199.0),
		] {
			app.preferences.export.font_size = size;
			app.preferences.export.validate().unwrap();
			assert!(app.page_command(Command::ExportSize(delta)));
			assert_eq!(app.preferences.export.font_size, expected);
			app.preferences.export.validate().unwrap();
		}
	}

	#[test]
	fn png_jobs_keep_their_page_and_scale_when_preferences_change() {
		#[derive(Clone)]
		struct Proxy;
		impl super::super::SendEvent for Proxy {
			fn try_send(&self, _: Event) -> bool {
				true
			}
		}
		let dir = tempfile::tempdir().unwrap();
		let source = dir.path().join("source.md");
		std::fs::write(&source, "# Snapshot\n\nBody paragraph.").unwrap();
		let settings = ExportSettings {
			format: ExportFormat::Png,
			paper: "a5".into(),
			landscape: true,
			margin: [15.0; 4],
			scale: 3.0,
			..Default::default()
		};
		let outcome = run(
			&source,
			&dir.path().join("snapshot.png"),
			&settings,
			Default::default(),
			crate::test_support::fonts(),
			CjkType::Sc,
			&[],
			true,
			Arc::new(crate::services::Services::new(2)),
			crate::security::Security::local(crate::security::Trust::Trusted),
		);
		assert!(matches!(outcome, ExportOutcome::PngReady { .. }));
		let mut app = App::new(
			crate::cli::LaunchOptions {
				mode: crate::cli::Mode::Smoke,
				options: crate::test_support::options(),
				..Default::default()
			},
			Proxy,
		);
		app.renderer = Some(
			pollster::block_on(crate::render::Renderer::new(None)).unwrap(),
		);
		app.preferences.export = ExportSettings::default();
		app.export_finished(outcome);
		let job = app.export.png.as_ref().unwrap();
		let geometry = export::geometry(&settings).unwrap();
		assert_eq!(job.scale, 3.0);
		assert_eq!(job.left, geometry.margin_pt[3] / PT_PER_PX);
		assert_eq!(
			job.plan.width_px,
			(geometry.width_pt / PT_PER_PX * 3.0).round() as u32
		);
		assert!(job.snapshot.height > 0.0);
	}

	#[test]
	fn closing_the_app_cancels_an_open_save_selection_before_joining() {
		#[derive(Clone)]
		struct Proxy;
		impl super::super::SendEvent for Proxy {
			fn try_send(&self, _: Event) -> bool {
				true
			}
		}
		struct OpenDialog {
			entered: Option<std::sync::mpsc::Sender<()>>,
			dropped: std::sync::mpsc::Sender<()>,
		}
		impl std::future::Future for OpenDialog {
			type Output = Option<PathBuf>;
			fn poll(
				mut self: std::pin::Pin<&mut Self>,
				_: &mut std::task::Context<'_>,
			) -> std::task::Poll<Self::Output> {
				if let Some(entered) = self.entered.take() {
					entered.send(()).unwrap();
				}
				std::task::Poll::Pending
			}
		}
		impl Drop for OpenDialog {
			fn drop(&mut self) {
				self.dropped.send(()).unwrap();
			}
		}
		let mut app = App::new(
			crate::cli::LaunchOptions {
				mode: crate::cli::Mode::Smoke,
				options: crate::test_support::options(),
				..Default::default()
			},
			Proxy,
		);
		let (entered, started) = std::sync::mpsc::channel();
		let (dropped, released) = std::sync::mpsc::channel();
		let output = wait_for_output(
			&app.services.handle,
			OpenDialog {
				entered: Some(entered),
				dropped,
			},
		);
		app.export.thread = Some(std::thread::spawn(move || {
			assert!(output.recv().unwrap().is_none());
		}));
		started.recv_timeout(Duration::from_secs(5)).unwrap();
		drop(app);
		released.recv_timeout(Duration::from_secs(5)).unwrap();
	}

	#[test]
	fn saves_while_a_watch_export_is_in_flight_are_queued() {
		#[derive(Clone)]
		struct Proxy;
		impl super::super::SendEvent for Proxy {
			fn try_send(&self, _: Event) -> bool {
				true
			}
		}
		let mut app = App::new(
			crate::cli::LaunchOptions {
				mode: crate::cli::Mode::Smoke,
				options: crate::test_support::options(),
				..Default::default()
			},
			Proxy,
		);
		let source = PathBuf::from("document.md");
		let output = PathBuf::from("document.pdf");
		let origin =
			crate::security::Origin::Local(crate::security::Trust::Trusted);
		app.export.running = true;
		app.export.watch_request = true;
		app.export.source = Some((source.clone(), origin.clone()));
		app.export.schedule_watch_export(&source);
		let first = app
			.export
			.watch_at
			.expect("a save during the first export must queue a rebuild");
		app.export.arm_watch(&output);
		assert_eq!(
			app.export.watch_at,
			Some(first),
			"arming the watch must not drop a save seen during the export"
		);
		app.export.watch_at = None;
		app.export.schedule_watch_export(&source);
		let rebuild = app
			.export
			.watch_at
			.expect("a save during a watch rebuild must queue another");
		app.export.arm_watch(&output);
		assert_eq!(
			app.export.watch_at,
			Some(rebuild),
			"arming the watch must not drop a save seen during a rebuild"
		);
	}

	#[test]
	fn desktop_pdf_jobs_use_custom_titles_and_empty_titles_fall_back() {
		let dir = tempfile::tempdir().unwrap();
		let source = dir.path().join("source.md");
		std::fs::write(&source, "# Document heading\n\nBody").unwrap();
		for (title, expected) in [
			(" Custom title ", "Custom title"),
			("   ", "Document heading"),
		] {
			let output = dir.path().join("output.pdf");
			let result = run(
				&source,
				&output,
				&ExportSettings::default(),
				export::MetadataOverrides::with_title(title),
				crate::test_support::fonts(),
				CjkType::Sc,
				&[],
				false,
				Arc::new(crate::services::Services::new(4)),
				crate::security::Security::local(
					crate::security::Trust::Trusted,
				),
			);
			assert!(matches!(result, ExportOutcome::Written { .. }));
			let bytes = std::fs::read(&output).unwrap();
			let pdf = String::from_utf8_lossy(&bytes);
			assert!(
				pdf.contains(&format!("/Title({expected})")),
				"PDF metadata must contain the job title: {:?}",
				pdf.find("/Title")
					.map(|i| &pdf[i..(i + 140).min(pdf.len())])
			);
		}
	}
}

#[cfg(test)]
mod security_tests {
	use super::*;
	use crate::security::{Resource, Security, Trust};

	#[test]
	fn desktop_exports_preserve_untrusted_resource_permissions() {
		let dir = tempfile::tempdir().unwrap();
		let source = dir.path().join("document.md");
		let image_path = dir.path().join("picture.png");
		image::RgbaImage::from_pixel(3, 2, image::Rgba([12, 34, 56, 255]))
			.save(&image_path)
			.unwrap();
		std::fs::write(&source, "# Document\n\n![image](picture.png)").unwrap();
		for (security, expected_images) in [
			(Security::local(Trust::Trusted), 1),
			(Security::local(Trust::Untrusted), 0),
			(
				{
					let mut security = Security::local(Trust::Untrusted);
					security.grant(Resource::File(
						std::fs::canonicalize(&image_path).unwrap(),
					));
					security
				},
				1,
			),
		] {
			let output = dir.path().join("document.pdf");
			let outcome = run(
				&source,
				&output,
				&ExportSettings::default(),
				Default::default(),
				crate::test_support::fonts(),
				CjkType::Sc,
				&[],
				true,
				Arc::new(crate::services::Services::new(4)),
				security.clone(),
			);
			if expected_images == 0 {
				assert!(
					matches!(outcome, ExportOutcome::Failed(ref error) if error.contains("Permission required"))
				);
			} else {
				assert!(matches!(outcome, ExportOutcome::Written { .. }));
			}
			let pdf = lopdf::Document::load(&output).unwrap();
			let images = pdf
				.objects
				.values()
				.filter(|object| {
					object
						.as_dict()
						.ok()
						.or_else(|| {
							object.as_stream().ok().map(|stream| &stream.dict)
						})
						.is_some_and(|dict| {
							dict.get(b"Subtype").is_ok_and(|value| {
								value
									.as_name()
									.is_ok_and(|name| name == b"Image")
							})
						})
				})
				.count();
			if expected_images != 0 {
				assert_eq!(images, expected_images);
			}
			let snapshot = export::png_snapshot_with_services(
				&source,
				crate::test_support::options(),
				true,
				Arc::new(crate::services::Services::new(4)),
				security,
			)
			.unwrap();
			assert_eq!(snapshot.images.decoded().len(), expected_images);
		}
	}
	#[test]
	fn live_exports_keep_the_initiating_origin_when_tabs_change() {
		#[derive(Clone)]
		struct Proxy(std::sync::mpsc::Sender<Event>);
		impl super::super::SendEvent for Proxy {
			fn try_send(&self, event: Event) -> bool {
				self.0.send(event).is_ok()
			}
		}
		let dir = tempfile::tempdir().unwrap();
		let source = dir.path().join("document.md");
		std::fs::write(&source, "# Document").unwrap();
		let source = std::fs::canonicalize(source).unwrap();
		let (send, receive) = std::sync::mpsc::channel();
		let mut app = App::new(
			crate::cli::LaunchOptions {
				mode: crate::cli::Mode::Smoke,
				options: crate::test_support::options(),
				..Default::default()
			},
			Proxy(send),
		);
		app.open_document(source.clone(), Security::local(Trust::Untrusted));
		app.export.rebuild = true;
		app.export.watch_request = true;
		app.export.running = true;
		app.spawn_export(
			source.clone(),
			Destination::Path(dir.path().join("document.pdf")),
			ExportSettings::default(),
		);
		app.open(source.clone());
		loop {
			if let Event::Exported(outcome) =
				receive.recv_timeout(Duration::from_secs(10)).unwrap()
			{
				assert!(matches!(*outcome, ExportOutcome::Written { .. }));
				app.export_finished(*outcome);
				break;
			}
		}
		assert_eq!(
			app.export.watch.as_ref().unwrap().origin,
			crate::security::Origin::Local(Trust::Untrusted)
		);
		app.start_watch_export();
		assert!(!app.export.running);
		app.close_tab(app.readers.active());
		assert!(app.export.watch.is_some());
		assert_eq!(
			app.readers.session.security.origin.trust(),
			Trust::Untrusted
		);
		let owner = app.readers.active();
		app.open(source.clone());
		let counterpart = app.readers.active();
		app.select_tab(owner);
		app.close_tab(counterpart);
		assert!(app.export.watch.is_some());
		let output = dir.path().join("document.pdf");
		let original = std::fs::read(&output).unwrap();
		std::fs::write(&source, "# Revised document").unwrap();
		app.export.schedule_watch_export(&source);
		assert!(app.export.watch_at.is_some());
		app.start_watch_export();
		assert!(app.export.running);
		loop {
			if let Event::Exported(outcome) =
				receive.recv_timeout(Duration::from_secs(10)).unwrap()
			{
				assert!(matches!(*outcome, ExportOutcome::Written { .. }));
				app.export_finished(*outcome);
				break;
			}
		}
		assert_ne!(std::fs::read(output).unwrap(), original);
		let owner = app.readers.active();
		app.open(source);
		app.close_tab(owner);
		assert!(app.export.watch.is_none());
		assert!(app.export.watch_at.is_none());
	}
}
