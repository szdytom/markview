//! The reader's export panel actions and the background jobs they start.
//!
//! Every export runs off the event loop: read, parse and layout happen on a
//! spawned thread, and only the PNG strips come back to the main thread, where
//! the one GPU device lives. Nothing here changes the reader's layout options.
use super::{App, Event, ExportOutcome};
use crate::{
	export,
	layout::LayoutSnapshot,
	settings::{ExportFormat, ExportSettings, FontDefOverride},
	state::Command,
};
use markview_core::{
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
	/// Ask the reader, on the exporting thread so the window never blocks.
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

impl<P: super::SendEvent> App<P> {
	/// Applies an export-panel change, and reports whether it owned the
	/// command. It never requests a reader layout, so the window cannot move.
	pub(super) fn export_command(&mut self, action: Command) -> bool {
		let mut settings = self.preferences.export.clone();
		match action {
			Command::ExportFormat(format) => settings.format = format,
			Command::ExportSize(delta) => {
				settings.font_size =
					(settings.font_size + f32::from(delta)).clamp(10.0, 40.0)
			}
			Command::ExportIndent(em) => {
				settings.paragraph_indent = f32::from(em)
			}
			Command::ExportPaper(index) => {
				let Some((_, paper)) = PAPER_PRESETS.get(index as usize) else {
					return true;
				};
				settings.paper = (*paper).into();
			}
			Command::ExportOrientation(landscape) => {
				settings.landscape = landscape
			}
			Command::ExportMargin(index) => {
				let Some((margin, _)) = MARGIN_PRESETS.get(index as usize)
				else {
					return true;
				};
				settings.margin = *margin;
			}
			Command::ExportScale(index) => {
				let Some((scale, _)) = SCALE_PRESETS.get(index as usize) else {
					return true;
				};
				settings.scale = *scale;
			}
			_ => return false,
		}
		self.preferences.set_export(settings);
		self.redraw();
		true
	}

	/// Shows a transient status line; a zero `seconds` clears it.
	pub(super) fn notify(&mut self, message: &str, error: bool, seconds: u64) {
		self.status = message.to_owned();
		self.error = error;
		self.status_until = (seconds > 0)
			.then(|| Instant::now() + Duration::from_secs(seconds));
		self.redraw();
	}

	/// Asks for a destination, then starts the export on a worker thread.
	/// `watch` keeps rewriting the chosen file whenever the document changes.
	pub(super) fn start_export(&mut self, watch: bool) {
		if self.export_running || self.dialog_open {
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
		self.interaction.focus = None;
		self.interaction.pointer_down = None;
		self.dialog_open = true;
		self.export_rebuild = false;
		self.export_watch_request = watch;
		self.export_running = true;
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
		if self.export_running || self.dialog_open {
			// The save that arrived mid-export is not lost; it becomes the
			// next rebuild once the current one is done.
			self.watch_at = Some(Instant::now() + Duration::from_millis(250));
			return;
		}
		let Some(watch) = &self.watch_export else {
			return;
		};
		let (path, output) = (watch.source.clone(), watch.output.clone());
		if self.readers.session.path.as_ref() != Some(&path) {
			return;
		}
		let settings = self.preferences.export.clone();
		if settings.validate().is_err() {
			return;
		}
		self.export_rebuild = true;
		self.export_watch_request = true;
		self.export_running = true;
		self.notify(
			self.preferences.values.lang().status_reexporting(),
			false,
			3600,
		);
		self.spawn_export(path, Destination::Path(output), settings);
	}

	/// Remembers a document change while its export is being watched.
	pub(super) fn schedule_watch_export(&mut self, path: &Path) {
		if self
			.watch_export
			.as_ref()
			.is_some_and(|watch| watch.source == path)
		{
			self.watch_at = Some(Instant::now() + Duration::from_millis(250));
		}
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
		// The reader's own font set, so the export shapes with the personal
		// download directory exactly like the display does.
		let fonts = self.fonts_config.clone();
		let metadata = export::MetadataOverrides::with_title(
			self.readers.session.export_title.text(),
		);
		let cjk = self.preferences.values.cjk_type;
		let overrides = self.preferences.values.fontdef_overrides.clone();
		std::thread::spawn(move || {
			let outcome = match destination {
				Destination::Ask { stem, directory } => {
					match choose_output(&stem, &settings, directory.as_deref())
					{
						Some(output) => run(
							&path, &output, &settings, metadata, fonts, cjk,
							&overrides, offline,
						),
						None => ExportOutcome::Cancelled,
					}
				}
				Destination::Path(output) => run(
					&path, &output, &settings, metadata, fonts, cjk,
					&overrides, offline,
				),
			};
			proxy.send(Event::Exported(Box::new(outcome)));
		});
	}

	/// Receives one export result on the event loop.
	pub(super) fn export_finished(&mut self, outcome: ExportOutcome) {
		self.dialog_open = false;
		let rebuild = std::mem::take(&mut self.export_rebuild);
		let watch = std::mem::take(&mut self.export_watch_request);
		match outcome {
			ExportOutcome::Written { path, detail } => {
				self.export_running = false;
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
					self.notify(&message, false, 6);
				} else {
					self.open_export(&path, message);
				}
				if watch {
					self.arm_watch(&path);
				} else {
					self.disarm_watch();
				}
			}
			ExportOutcome::PngReady {
				snapshot,
				path,
				stylesheet,
			} => {
				self.start_png_export(
					snapshot, path, stylesheet, watch, rebuild,
				);
			}
			ExportOutcome::Failed(error) => {
				self.export_running = false;
				self.notify(
					&self.preferences.values.lang().status_export_failed(error),
					true,
					8,
				);
			}
			ExportOutcome::Cancelled => {
				self.export_running = false;
				self.notify("", false, 0);
			}
		}
	}

	/// Stops a live export; the file it wrote stays where it is.
	fn disarm_watch(&mut self) {
		self.watch_export = None;
		self.watch_at = None;
	}

	/// Points the live export at a file that was just written.
	fn arm_watch(&mut self, output: &Path) {
		let Some(source) = self.readers.session.path.clone() else {
			return;
		};
		self.watch_export = Some(super::WatchExport {
			source,
			output: output.to_path_buf(),
		});
		self.watch_at = None;
	}

	/// Hands a written export to the operating system, so the reader sees the
	/// result. A platform that cannot start a viewer does not undo the file,
	/// so its failure is reported beside the export's own status.
	fn open_export(&mut self, path: &Path, message: String) {
		match open::that_detached(path) {
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
		watch: bool,
		rebuild: bool,
	) {
		let settings = self.preferences.export.clone();
		let (geometry, max_tile) =
			match (&self.renderer, export::geometry(&settings)) {
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
		let plan = match export::plan(
			&geometry,
			snapshot.height,
			settings.scale,
			max_tile,
		) {
			Ok(plan) => plan,
			Err(error) => {
				self.fail_png_export(&format!("{error:#}"));
				return;
			}
		};
		let bytes = plan.width_px as usize * plan.height_px as usize * 4;
		self.png_export = Some(PngExport {
			snapshot,
			path,
			stylesheet,
			scale: settings.scale,
			left: geometry.margin_pt[3] / PT_PER_PX,
			rgba: vec![0; bytes],
			next: 0,
			watch,
			rebuild,
			plan,
		});
		self.status = self
			.preferences
			.values
			.lang()
			.status_exporting_png(0, self.tiles());
		self.status_until = Some(Instant::now() + Duration::from_secs(3600));
		self.redraw();
	}

	fn tiles(&self) -> usize {
		self.png_export
			.as_ref()
			.map_or(0, |job| job.plan.tiles.len())
	}

	fn fail_png_export(&mut self, reason: &str) {
		self.png_export = None;
		self.export_running = false;
		self.notify(
			&self.preferences.values.lang().status_export_failed(reason),
			true,
			8,
		);
	}

	/// Draws one strip of a waiting PNG. Called once per frame so the window
	/// stays responsive and the status line can count the strips.
	pub(super) fn advance_png_export(&mut self) {
		let Some(mut job) = self.png_export.take() else {
			return;
		};
		if job.next >= job.plan.tiles.len() {
			self.finish_png_export(job);
			return;
		}
		let Some(renderer) = &mut self.renderer else {
			self.png_export = Some(job);
			let reason = self.preferences.values.lang().status_gpu_not_ready();
			self.fail_png_export(reason);
			return;
		};
		let tile = job.plan.tiles[job.next];
		let theme = self.preferences.values.theme;
		if let Err(error) = export::draw_tile(
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
			self.png_export = Some(job);
			self.fail_png_export(&format!("{error:#}"));
			return;
		}
		job.next += 1;
		if job.next >= job.plan.tiles.len() {
			self.finish_png_export(job);
		} else {
			self.status = self
				.preferences
				.values
				.lang()
				.status_exporting_png(job.next, job.plan.tiles.len());
			self.status_until =
				Some(Instant::now() + Duration::from_secs(3600));
			self.png_export = Some(job);
			self.redraw();
		}
	}

	fn finish_png_export(&mut self, job: PngExport) {
		let outcome = export::write_png(
			&job.path,
			&job.rgba,
			job.plan.width_px,
			job.plan.height_px,
		);
		self.export_running = false;
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
					self.notify(&message, false, 6);
				} else {
					self.open_export(&path, message);
				}
				if job.watch {
					self.arm_watch(&path);
				} else {
					self.disarm_watch();
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

/// The save dialog, on the exporting thread so the window never blocks.
fn choose_output(
	stem: &str,
	settings: &ExportSettings,
	directory: Option<&Path>,
) -> Option<PathBuf> {
	// The format names the export panel offers, so the dialog and the page
	// behind it cannot drift apart.
	let lang = crate::lang::Lang::default();
	let (extension, label) = match settings.format {
		ExportFormat::Pdf => ("pdf", lang.export_pdf()),
		ExportFormat::Png => ("png", lang.export_png()),
	};
	let mut dialog = rfd::FileDialog::new()
		.set_file_name(format!("{stem}.{extension}"))
		.add_filter(label, &[extension]);
	if let Some(directory) = directory.filter(|dir| dir.is_dir()) {
		dialog = dialog.set_directory(directory);
	}
	dialog.save_file()
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
) -> ExportOutcome {
	match settings.format {
		ExportFormat::Pdf => {
			let mut args = match export::pdf_request(
				path.to_path_buf(),
				output.to_path_buf(),
				settings,
				fonts,
				cjk,
				overrides,
				offline,
			) {
				Ok(args) => args,
				Err(error) => {
					return ExportOutcome::Failed(format!("{error:#}"));
				}
			};
			args.metadata = metadata;
			match crate::pdf::export_once(&args) {
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
			let stylesheet = match export::export_stylesheet(
				&settings.style,
				cjk,
				overrides,
			) {
				Ok(stylesheet) => stylesheet,
				Err(error) => {
					return ExportOutcome::Failed(format!("{error:#}"));
				}
			};
			let geometry = match export::geometry(settings) {
				Ok(geometry) => geometry,
				Err(error) => {
					return ExportOutcome::Failed(format!("{error:#}"));
				}
			};
			let options = export::layout_options(
				settings,
				geometry.text_px().0,
				stylesheet.clone(),
				fonts,
			);
			match export::png_snapshot(path, options, offline) {
				Ok(snapshot) => ExportOutcome::PngReady {
					snapshot,
					path: output.to_path_buf(),
					stylesheet,
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
