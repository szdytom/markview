//! Downloadable-font UI state and operations, independent of the application.
use std::{collections::HashMap, sync::Arc};

pub(super) mod view;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
	DownloadMissing,
	DownloadAll,
	DownloadOne(usize),
	RedownloadOne(usize),
	Cancel(usize),
	OpenFolder,
	StatusFilter(Option<crate::fonts::State>),
	/// Switches the page to its chooser rows, away from the catalogue.
	Choosers,
}

pub(super) enum Message {
	Progress(Box<crate::fonts::Progress>),
	Settled(Box<crate::fonts::Summary>),
}

#[derive(Clone, Default)]
pub(super) struct Choices {
	pub(super) generation: u64,
	config: Option<markview_core::fonts::FontConfig>,
	latin: Arc<[Arc<str>]>,
	han: Arc<[Arc<str>]>,
}
impl Choices {
	pub(super) fn refresh(
		&mut self,
		config: &markview_core::fonts::FontConfig,
	) {
		if self.config.as_ref() == Some(config) {
			return;
		}
		self.generation += 1;
		self.latin = markview_core::fonts::families(config, false);
		self.han = markview_core::fonts::families(config, true);
		self.config = Some(config.clone());
	}
	fn families(&self, role: crate::settings::FontRole) -> &[Arc<str>] {
		if role.han() { &self.han } else { &self.latin }
	}
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Selection {
	pub catalog_generation: u64,
	pub index: usize,
}

#[derive(Default)]
pub(super) struct FontPanel {
	choices: Choices,
	// Built only when a page needs it; catalogue scans must not delay launch.
	font_catalog: Vec<crate::fonts::Family>,
	font_jobs: HashMap<String, crate::fonts::Progress>,
	font_cancel: HashMap<String, tokio_util::sync::CancellationToken>,
	services: Option<crate::services::Handle>,
	font_note: Option<String>,
	font_status_filter: Option<crate::fonts::State>,
	/// Whether the page shows its chooser rows rather than the catalogue.
	font_choosers: bool,
	scroll: f32,
}

pub(super) struct View<'a> {
	pub(super) choices: Choices,
	pub(super) catalog: &'a [crate::fonts::Family],
	pub(super) shown: Vec<usize>,
	pub(super) jobs: &'a HashMap<String, crate::fonts::Progress>,
	pub(super) scroll: f32,
	pub(super) note: Option<&'a str>,
	pub(super) status_filter: Option<crate::fonts::State>,
	pub(super) choosers: bool,
}

impl FontPanel {
	pub(super) fn with_services(services: crate::services::Handle) -> Self {
		Self {
			services: Some(services),
			..Default::default()
		}
	}
	pub(super) fn view(&self) -> View<'_> {
		View {
			choices: self.choices.clone(),
			catalog: &self.font_catalog,
			shown: self.shown_fonts(),
			jobs: &self.font_jobs,
			scroll: self.scroll,
			note: self.font_note.as_deref(),
			status_filter: self.font_status_filter,
			choosers: self.font_choosers,
		}
	}
	#[cfg(test)]
	pub(super) fn refresh_choices(
		&mut self,
		config: &markview_core::fonts::FontConfig,
	) {
		self.choices.refresh(config);
	}
	pub(super) fn resolve(
		&self,
		role: crate::settings::FontRole,
		selection: Selection,
	) -> Option<String> {
		(selection.catalog_generation == self.choices.generation)
			.then(|| self.choices.families(role).get(selection.index))
			.flatten()
			.map(|name| name.to_string())
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
			self.font_cancel.remove(id);
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
			Command::DownloadMissing => {
				(self.shown_font_ids(), crate::fonts::Scope::Missing)
			}
			Command::DownloadAll => {
				(self.shown_font_ids(), crate::fonts::Scope::Named)
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
			Command::StatusFilter(state) => {
				self.font_status_filter = state;
				self.font_choosers = false;
				self.scroll = 0.0;
				return false;
			}
			Command::Choosers => {
				self.font_choosers = true;
				self.scroll = 0.0;
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

	pub(super) fn set_catalog(&mut self, catalog: Vec<crate::fonts::Family>) {
		self.font_catalog = catalog;
	}
	pub(super) fn set_choices(&mut self, choices: Choices) {
		self.choices = choices;
	}
	pub(super) fn choosers(&self) -> bool {
		self.font_choosers
	}

	/// Starts downloading the named families that still need a download.
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
		mut send: impl FnMut(Message) + Send + 'static,
	) -> bool {
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
		let services =
			self.services.as_ref().expect("font panel services").clone();
		let mut cancels = HashMap::new();
		for family in &missing {
			self.font_jobs.insert(
				family.id.clone(),
				crate::fonts::Progress::queued(&family.id),
			);
			let cancel = services.cancel.child_token();
			self.font_cancel.insert(family.id.clone(), cancel.clone());
			cancels.insert(family.id.clone(), cancel);
		}
		self.font_note = None;
		let ids: Vec<_> =
			missing.iter().map(|family| family.id.clone()).collect();
		let handle = services.clone();
		if !services.submit(async move {
			let transport = crate::net::Downloader::new("Font");
			let summary = crate::fonts::run_async(
				&missing,
				&dir,
				&transport,
				&handle,
				cancels,
				|progress| send(Message::Progress(Box::new(progress))),
			)
			.await;
			send(Message::Settled(Box::new(summary)));
		}) {
			for id in ids {
				self.font_jobs.remove(&id);
				self.font_cancel.remove(&id);
			}
			self.font_note = Some("Font service closed".into());
		}
		false
	}

	/// Asks one family's running download to stop.
	fn cancel_font(&mut self, id: &str) {
		self.font_cancel.entry(id.to_owned()).or_default().cancel();
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
	fn refreshed_catalogues_reject_old_selection_positions() {
		let mut panel = panel();
		let mut config = crate::test_support::fonts();
		panel.refresh_choices(&config);
		let selection = Selection {
			catalog_generation: panel.choices.generation,
			index: 0,
		};
		let selected = panel
			.resolve(crate::settings::FontRole::Serif, selection)
			.unwrap();
		config.revision += 1;
		panel.refresh_choices(&config);
		assert!(
			panel
				.resolve(crate::settings::FontRole::Serif, selection)
				.is_none()
		);
		assert_eq!(
			panel.resolve(
				crate::settings::FontRole::Serif,
				Selection {
					catalog_generation: panel.choices.generation,
					..selection
				}
			),
			Some(selected)
		);
	}

	#[test]
	fn filters_and_row_commands_address_the_same_families() {
		let mut panel = panel();
		panel.set_scroll(72.0);
		panel.command(
			Command::StatusFilter(Some(crate::fonts::State::Missing)),
			true,
			|_| unreachable!(),
		);
		assert_eq!(panel.view().shown, vec![0, 1]);
		assert_eq!(panel.view().scroll, 0.0);
		let id = panel.font_catalog[1].family.id.clone();
		panel.progress(crate::fonts::Progress::queued(&id));
		panel.command(Command::Cancel(1), true, |_| unreachable!());
		assert!(panel.font_cancel[&id].is_cancelled());
		panel.progress(crate::fonts::Progress::queued(
			&panel.font_catalog[0].family.id,
		));
		panel.command(Command::DownloadMissing, true, |_| unreachable!());
		assert!(panel.view().note.is_none());
		panel.command(
			Command::StatusFilter(Some(crate::fonts::State::Downloaded)),
			true,
			|_| unreachable!(),
		);
		assert!(panel.view().shown.is_empty());
		panel.set_scroll(88.0);
		panel.command(Command::StatusFilter(None), true, |_| unreachable!());
		assert_eq!(panel.view().shown, vec![0, 1, 2]);
		assert_eq!(panel.view().scroll, 0.0);
		assert!(panel.view().status_filter.is_none());
		assert!(!panel.view().choosers);
		// The chooser rows are a view of their own: opening one resets the
		// scroll, and any status filter leaves it again.
		panel.set_scroll(72.0);
		panel.command(Command::Choosers, true, |_| unreachable!());
		assert!(panel.view().choosers);
		assert_eq!(panel.view().scroll, 0.0);
		panel.set_scroll(72.0);
		panel.command(
			Command::StatusFilter(Some(crate::fonts::State::Missing)),
			true,
			|_| unreachable!(),
		);
		assert!(!panel.view().choosers);
		assert_eq!(panel.view().scroll, 0.0);
	}

	#[test]
	fn bulk_downloads_distinguish_missing_from_installed_and_downloaded() {
		let mut panel = panel();
		for state in [
			crate::fonts::State::Provided,
			crate::fonts::State::Downloaded,
		] {
			for entry in &mut panel.font_catalog {
				entry.state = state;
			}
			assert!(!panel.command(
				Command::DownloadMissing,
				true,
				|_| unreachable!()
			));
			assert_eq!(
				panel.command(Command::DownloadAll, true, |_| unreachable!()),
				state == crate::fonts::State::Provided
			);
		}
		for entry in &mut panel.font_catalog {
			entry.state = crate::fonts::State::Missing;
		}
		assert!(panel.command(
			Command::DownloadMissing,
			true,
			|_| unreachable!()
		));
		for id in panel.shown_font_ids() {
			panel.progress(crate::fonts::Progress::queued(&id));
		}
		assert!(!panel.command(Command::DownloadAll, true, |_| unreachable!()));
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
