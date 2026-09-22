//! Downloadable-font UI state and operations, independent of the application.
use std::{
	collections::{HashMap, HashSet},
	sync::{Arc, Mutex},
};

pub(super) mod view;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
	Download,
	DownloadOne(usize),
	RedownloadOne(usize),
	Cancel(usize),
	OpenFolder,
	SourceFilter,
	StatusFilter,
}

pub(super) enum Message {
	Progress(Box<crate::fonts::Progress>),
	Settled(Box<crate::fonts::Summary>),
}

#[derive(Default)]
pub(super) struct FontPanel {
	// Built only when a page needs it; catalogue scans must not delay launch.
	font_catalog: Vec<crate::fonts::Family>,
	font_jobs: HashMap<String, crate::fonts::Progress>,
	font_cancel: Arc<Mutex<HashSet<String>>>,
	font_note: Option<String>,
	font_source_filter: Option<String>,
	font_status_filter: Option<crate::fonts::State>,
	scroll: f32,
}

pub(super) struct View<'a> {
	pub(super) catalog: &'a [crate::fonts::Family],
	pub(super) shown: Vec<usize>,
	pub(super) jobs: &'a HashMap<String, crate::fonts::Progress>,
	pub(super) scroll: f32,
	pub(super) note: Option<&'a str>,
	pub(super) source_filter: Option<&'a str>,
	pub(super) status_filter: Option<crate::fonts::State>,
}

impl FontPanel {
	pub(super) fn view(&self) -> View<'_> {
		View {
			catalog: &self.font_catalog,
			shown: self.shown_fonts(),
			jobs: &self.font_jobs,
			scroll: self.scroll,
			note: self.font_note.as_deref(),
			source_filter: self.font_source_filter.as_deref(),
			status_filter: self.font_status_filter,
		}
	}
	pub(super) fn set_scroll(&mut self, scroll: f32) {
		self.scroll = scroll;
	}
	pub(super) fn progress(&mut self, progress: crate::fonts::Progress) {
		self.font_jobs.insert(progress.id.clone(), progress);
	}
	pub(super) fn settled(&mut self, summary: &crate::fonts::Summary) {
		for id in &summary.requested {
			self.font_jobs.remove(id);
		}
		self.font_note =
			match (summary.failed.first(), summary.cancelled.first()) {
				(Some((id, reason)), _) => Some(format!("{id}: {reason}")),
				(None, Some(id)) => Some(format!("{id}: cancelled")),
				(None, None) if summary.stored > 0 => Some(format!(
					"{} files stored, {} MiB",
					summary.stored,
					summary.bytes / (1024 * 1024)
				)),
				(None, None) => Some("Nothing to download".into()),
			};
	}
	/// Returns whether an offline notification should be shown.
	pub(super) fn command(
		&mut self,
		command: Command,
		offline: bool,
		send: impl Fn(Message) + Send + 'static,
	) -> bool {
		let (ids, scope) = match command {
			Command::Download => {
				(self.shown_font_ids(), crate::fonts::Scope::Missing)
			}
			Command::DownloadOne(index) | Command::RedownloadOne(index) => {
				let Some(id) = self.shown_font_id(index) else {
					return false;
				};
				let scope = if matches!(command, Command::RedownloadOne(_)) {
					crate::fonts::Scope::All
				} else {
					crate::fonts::Scope::Named
				};
				(vec![id], scope)
			}
			Command::Cancel(index) => {
				if let Some(id) = self.shown_font_id(index) {
					self.cancel_font(&id);
				}
				return false;
			}
			Command::OpenFolder => {
				self.open_fonts_folder();
				return false;
			}
			Command::SourceFilter => {
				self.next_font_source();
				return false;
			}
			Command::StatusFilter => {
				self.next_font_status();
				return false;
			}
		};
		self.download_fonts(&ids, scope, offline, send)
	}
	/// The catalogue positions the Fonts page shows, filters applied.
	fn shown_fonts(&self) -> Vec<usize> {
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
	fn shown_font_id(&self, index: usize) -> Option<String> {
		self.shown_fonts()
			.get(index)
			.map(|position| self.font_catalog[*position].family.id.clone())
	}

	/// Every family the Fonts page shows.
	fn shown_font_ids(&self) -> Vec<String> {
		self.shown_fonts()
			.iter()
			.map(|position| self.font_catalog[*position].family.id.clone())
			.collect()
	}

	/// The declaring sheets the Fonts page cycles its source filter through.
	fn font_sources(&self) -> Vec<String> {
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
	fn next_font_source(&mut self) {
		let sources = self.font_sources();
		let next = match &self.font_source_filter {
			None => sources.first().cloned(),
			Some(current) => {
				let at = sources.iter().position(|source| source == current);
				at.and_then(|at| sources.get(at + 1)).cloned()
			}
		};
		self.font_source_filter = next;
		self.scroll = 0.0;
	}

	/// Advances the status filter through missing, provided, downloaded, all.
	fn next_font_status(&mut self) {
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
		self.scroll = 0.0;
	}

	/// Rebuilds the catalogue the Fonts page shows.
	///
	/// The builtin recommendations come first and every catalogued sheet
	/// follows, so a sheet that redefines a builtin family replaces it whole.
	/// The reader's own configuration is used, so the build shares the shaper's
	/// collection cache.
	pub(super) fn refresh(
		&mut self,
		entries: &[crate::stylesheet::Entry],
		fonts: &markview_core::fonts::FontConfig,
	) {
		let builtin = markview_core::style::Stylesheet::builtin();
		let sheets =
			std::iter::once(("builtin", builtin.font_families.as_slice()))
				.chain(entries.iter().map(|entry| {
					(entry.id.as_str(), entry.font_families.as_slice())
				}));
		let dir = crate::fonts::directory();
		self.font_catalog =
			crate::fonts::catalog(sheets, dir.as_deref(), fonts);
	}

	/// Starts downloading the named families, or reports why nothing can run.
	///
	/// Every family comes from the catalogued stylesheets and the builtin
	/// recommendations, so a download is exactly that set; nothing here runs on
	/// its own. A family already being downloaded is left to the run that owns
	/// it, so two runs never write the same files.
	fn download_fonts(
		&mut self,
		ids: &[String],
		scope: crate::fonts::Scope,
		offline: bool,
		send: impl Fn(Message) + Send + 'static,
	) -> bool {
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
			return false;
		}
		if offline {
			self.font_note =
				Some("Offline: font downloads are unavailable".into());
			return true;
		}
		let Some(dir) = crate::fonts::directory() else {
			self.font_note = Some("No user configuration directory".into());
			return false;
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
		let cancels = self.font_cancel.clone();
		std::thread::spawn(move || {
			let transport = match crate::net::Downloader::new("Font") {
				Ok(transport) => transport,
				Err(error) => {
					let reason = format!("{error:#}");
					let failed = missing
						.iter()
						.map(|family| (family.id.clone(), reason.clone()))
						.collect();
					send(Message::Settled(Box::new(crate::fonts::Summary {
						requested: missing
							.iter()
							.map(|family| family.id.clone())
							.collect(),
						failed,
						..Default::default()
					})));
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
					send(Message::Progress(Box::new(progress)));
				},
			);
			send(Message::Settled(Box::new(summary)));
		});
		false
	}

	/// Asks one family's running download to stop.
	fn cancel_font(&mut self, id: &str) {
		if let Ok(mut cancel) = self.font_cancel.lock() {
			cancel.insert(id.to_owned());
		}
	}

	/// Opens the download directory, creating it when it does not exist yet.
	fn open_fonts_folder(&mut self) {
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
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn panel() -> FontPanel {
		let sheet = markview_core::style::Stylesheet::builtin();
		FontPanel {
			font_catalog: sheet
				.font_families
				.iter()
				.take(3)
				.enumerate()
				.map(|(index, family)| crate::fonts::Family {
					family: family.clone(),
					owners: vec![
						if index == 0 { "builtin" } else { "custom" }.into(),
					],
					state: if index == 2 {
						crate::fonts::State::Provided
					} else {
						crate::fonts::State::Missing
					},
					files: Vec::new(),
					bytes: 0,
				})
				.collect(),
			..Default::default()
		}
	}

	#[test]
	fn filters_and_row_commands_address_the_same_families() {
		let mut panel = panel();
		panel.command(Command::SourceFilter, true, |_| unreachable!());
		panel.command(Command::SourceFilter, true, |_| unreachable!());
		panel.set_scroll(72.0);
		panel.command(Command::StatusFilter, true, |_| unreachable!());
		assert_eq!(panel.view().shown, vec![1]);
		assert_eq!(panel.view().scroll, 0.0);
		let id = panel.font_catalog[1].family.id.clone();
		panel.progress(crate::fonts::Progress::queued(&id));
		panel.command(Command::Cancel(0), true, |_| unreachable!());
		assert!(panel.font_cancel.lock().unwrap().contains(&id));
		panel.command(Command::Download, true, |_| unreachable!());
		assert_eq!(
			panel.view().note,
			Some("Those families are already downloading")
		);
	}

	#[test]
	fn settling_one_download_preserves_other_jobs_and_allows_retry() {
		let mut panel = panel();
		let ids = panel.shown_font_ids();
		for id in &ids {
			panel.progress(crate::fonts::Progress::queued(id));
		}
		panel.settled(&crate::fonts::Summary {
			requested: vec![ids[0].clone()],
			cancelled: vec![ids[0].clone()],
			..Default::default()
		});
		assert!(!panel.view().jobs.contains_key(&ids[0]));
		assert!(panel.view().jobs.contains_key(&ids[1]));
		assert!(panel.command(
			Command::DownloadOne(0),
			true,
			|_| unreachable!()
		));
		assert_eq!(
			panel.view().note,
			Some("Offline: font downloads are unavailable")
		);
	}
}
