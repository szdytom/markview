//! Downloadable-font UI state and operations, independent of the application.
use markview_ui_core::{Component, Invalidation, Update};
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
	Catalog(Vec<crate::fonts::Family>),
	Choices(Choices),
	Command {
		command: Command,
		offline: bool,
	},
	Progress {
		run: u64,
		progress: Box<crate::fonts::Progress>,
	},
	Settled {
		run: u64,
		summary: Box<crate::fonts::Summary>,
	},
	Failed {
		run: u64,
		ids: Vec<String>,
		note: String,
	},
	Note(String),
}

pub(super) enum Effect {
	Download {
		run: u64,
		families: Vec<markview_layout::style::FontFamily>,
	},
	Cancel(String),
	OpenFolder,
	Offline,
	Completed {
		run: u64,
		stored: usize,
		failure: Option<String>,
	},
}

#[derive(Clone, Default)]
pub(super) struct Choices {
	pub(super) generation: u64,
	config: Option<markview_layout::fonts::FontConfig>,
	latin: Arc<[Arc<str>]>,
	han: Arc<[Arc<str>]>,
}
impl Choices {
	pub(super) fn refresh(
		&mut self,
		config: &markview_layout::fonts::FontConfig,
	) {
		if self.config.as_ref() == Some(config) {
			return;
		}
		self.generation += 1;
		self.latin = markview_layout::fonts::families(config, false);
		self.han = markview_layout::fonts::families(config, true);
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
	job_runs: HashMap<String, u64>,
	next_run: u64,
	font_note: Option<String>,
	font_status_filter: Option<crate::fonts::State>,
	/// Whether the page shows its chooser rows rather than the catalogue.
	font_choosers: bool,
	scroll: markview_selection::Scroll,
}

pub(super) struct View<'a> {
	pub(super) choices: std::borrow::Cow<'a, Choices>,
	pub(super) catalog: &'a [crate::fonts::Family],
	pub(super) shown: Vec<usize>,
	pub(super) jobs: &'a HashMap<String, crate::fonts::Progress>,
	pub(super) scroll: f32,
	pub(super) note: Option<&'a str>,
	pub(super) status_filter: Option<crate::fonts::State>,
	pub(super) choosers: bool,
}

impl FontPanel {
	pub(super) fn view(&self) -> View<'_> {
		View {
			choices: std::borrow::Cow::Borrowed(&self.choices),
			catalog: &self.font_catalog,
			shown: self.shown_fonts(),
			jobs: &self.font_jobs,
			scroll: self.scroll.offset(),
			note: self.font_note.as_deref(),
			status_filter: self.font_status_filter,
			choosers: self.font_choosers,
		}
	}
	#[cfg(test)]
	pub(super) fn refresh_choices(
		&mut self,
		config: &markview_layout::fonts::FontConfig,
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
	#[cfg(test)]
	pub(super) fn set_scroll(&mut self, scroll: f32) {
		self.scroll.set(scroll);
	}
	pub(super) fn scroll_area(&mut self) -> &mut markview_selection::Scroll {
		&mut self.scroll
	}
	pub(super) fn scroll_area_ref(&self) -> &markview_selection::Scroll {
		&self.scroll
	}
	pub(super) fn progress(&mut self, progress: crate::fonts::Progress) {
		self.font_jobs.insert(progress.id.clone(), progress);
	}
	pub(super) fn settled(&mut self, summary: &crate::fonts::Summary) {
		for id in &summary.requested {
			self.font_jobs.remove(id);
			self.job_runs.remove(id);
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
	fn command(&mut self, command: Command, offline: bool) -> Update<Effect> {
		let (ids, scope) = match command {
			Command::DownloadMissing => {
				(self.shown_font_ids(), crate::fonts::Scope::Missing)
			}
			Command::DownloadAll => {
				(self.shown_font_ids(), crate::fonts::Scope::Named)
			}
			Command::DownloadOne(index) | Command::RedownloadOne(index) => {
				let Some(id) = self.shown_font_id(index) else {
					return Update::new(Invalidation::None);
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
					return Update::new(Invalidation::Paint)
						.effect(Effect::Cancel(id));
				}
				return Update::new(Invalidation::None);
			}
			Command::OpenFolder => {
				return Update::new(Invalidation::None)
					.effect(Effect::OpenFolder);
			}
			Command::StatusFilter(state) => {
				self.font_status_filter = state;
				self.font_choosers = false;
				self.scroll.reset();
				return Update::new(Invalidation::Layout);
			}
			Command::Choosers => {
				self.font_choosers = true;
				self.scroll.reset();
				return Update::new(Invalidation::Layout);
			}
		};
		self.download_fonts(&ids, scope, offline)
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

	pub(super) fn choosers(&self) -> bool {
		self.font_choosers
	}

	fn download_fonts(
		&mut self,
		ids: &[String],
		scope: crate::fonts::Scope,
		offline: bool,
	) -> Update<Effect> {
		let ids: Vec<_> = ids
			.iter()
			.filter(|id| !self.font_jobs.contains_key(*id))
			.cloned()
			.collect();
		let families: Vec<_> =
			crate::fonts::select(&self.font_catalog, &ids, scope)
				.into_iter()
				.cloned()
				.collect();
		if families.is_empty() {
			return Update::new(Invalidation::None);
		}
		if offline {
			self.font_note =
				Some("Offline: font downloads are unavailable".into());
			return Update::new(Invalidation::Paint).effect(Effect::Offline);
		}
		self.next_run += 1;
		let run = self.next_run;
		for family in &families {
			self.font_jobs.insert(
				family.id.clone(),
				crate::fonts::Progress::queued(&family.id),
			);
			self.job_runs.insert(family.id.clone(), run);
		}
		self.font_note = None;
		Update::new(Invalidation::Layout)
			.effect(Effect::Download { run, families })
	}
}

impl Component for FontPanel {
	type Message = Message;
	type Effect = Effect;
	type View<'a> = View<'a>;

	fn view(&self) -> View<'_> {
		self.view()
	}
	fn update(&mut self, message: Message) -> Update<Effect> {
		match message {
			Message::Catalog(catalog) => {
				self.font_catalog = catalog;
				Update::new(Invalidation::Layout)
			}
			Message::Choices(choices) => {
				self.choices = choices;
				Update::new(Invalidation::Layout)
			}
			Message::Command { command, offline } => {
				self.command(command, offline)
			}
			Message::Progress { run, progress } => {
				if self.job_runs.get(&progress.id) != Some(&run) {
					return Update::new(Invalidation::None);
				}
				self.progress(*progress);
				Update::new(Invalidation::Paint)
			}
			Message::Settled { run, summary } => {
				if summary.requested.is_empty()
					|| !summary
						.requested
						.iter()
						.all(|id| self.job_runs.get(id) == Some(&run))
				{
					return Update::new(Invalidation::None);
				}
				self.settled(&summary);
				Update::new(Invalidation::Layout).effect(Effect::Completed {
					run,
					stored: summary.stored,
					failure: summary
						.failed
						.first()
						.map(|(_, reason)| reason.clone()),
				})
			}
			Message::Failed { run, ids, note } => {
				let mut accepted = false;
				for id in ids {
					if self.job_runs.get(&id) == Some(&run) {
						self.job_runs.remove(&id);
						self.font_jobs.remove(&id);
						accepted = true;
					}
				}
				if !accepted {
					return Update::new(Invalidation::None);
				}
				self.font_note = Some(note);
				Update::new(Invalidation::Layout)
			}
			Message::Note(note) => {
				self.font_note = Some(note);
				Update::new(Invalidation::Paint)
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn panel() -> FontPanel {
		let sheet = markview_layout::style::Stylesheet::builtin();
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
		);
		assert_eq!(panel.view().shown, vec![0, 1]);
		assert_eq!(panel.view().scroll, 0.0);
		let id = panel.font_catalog[1].family.id.clone();
		panel.progress(crate::fonts::Progress::queued(&id));
		panel.command(Command::Cancel(1), true);
		assert!(
			matches!(panel.command(Command::Cancel(1), true).effects.as_slice(), [Effect::Cancel(cancelled)] if cancelled == &id)
		);
		panel.progress(crate::fonts::Progress::queued(
			&panel.font_catalog[0].family.id,
		));
		panel.command(Command::DownloadMissing, true);
		assert!(panel.view().note.is_none());
		panel.command(
			Command::StatusFilter(Some(crate::fonts::State::Downloaded)),
			true,
		);
		assert!(panel.view().shown.is_empty());
		panel.set_scroll(88.0);
		panel.command(Command::StatusFilter(None), true);
		assert_eq!(panel.view().shown, vec![0, 1, 2]);
		assert_eq!(panel.view().scroll, 0.0);
		assert!(panel.view().status_filter.is_none());
		assert!(!panel.view().choosers);
		// The chooser rows are a view of their own: opening one resets the
		// scroll, and any status filter leaves it again.
		panel.set_scroll(72.0);
		panel.command(Command::Choosers, true);
		assert!(panel.view().choosers);
		assert_eq!(panel.view().scroll, 0.0);
		panel.set_scroll(72.0);
		panel.command(
			Command::StatusFilter(Some(crate::fonts::State::Missing)),
			true,
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
			assert!(
				panel
					.command(Command::DownloadMissing, true)
					.effects
					.is_empty()
			);
			assert_eq!(
				!panel.command(Command::DownloadAll, true).effects.is_empty(),
				state == crate::fonts::State::Provided
			);
		}
		for entry in &mut panel.font_catalog {
			entry.state = crate::fonts::State::Missing;
		}
		assert!(matches!(
			panel
				.command(Command::DownloadMissing, true)
				.effects
				.as_slice(),
			[Effect::Offline]
		));
		for id in panel.shown_font_ids() {
			panel.progress(crate::fonts::Progress::queued(&id));
		}
		assert!(panel.command(Command::DownloadAll, true).effects.is_empty());
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
		assert!(matches!(
			panel
				.command(Command::DownloadOne(0), true)
				.effects
				.as_slice(),
			[Effect::Offline]
		));
		assert_eq!(
			panel.view().note,
			Some("Offline: font downloads are unavailable")
		);
	}

	#[test]
	fn retry_rejects_progress_and_completion_from_the_previous_run() {
		let mut panel = panel();
		let first = panel.update(Message::Command {
			command: Command::DownloadOne(0),
			offline: false,
		});
		let [Effect::Download { run, families }] = first.effects.as_slice()
		else {
			panic!("download effect");
		};
		let run = *run;
		let id = families[0].id.clone();
		let summary = || {
			Box::new(crate::fonts::Summary {
				requested: vec![id.clone()],
				stored: 1,
				..Default::default()
			})
		};
		assert!(matches!(
			panel
				.update(Message::Settled {
					run,
					summary: summary()
				})
				.effects
				.as_slice(),
			[Effect::Completed { stored: 1, .. }]
		));
		let retry = panel.update(Message::Command {
			command: Command::RedownloadOne(0),
			offline: false,
		});
		let [Effect::Download { run: next, .. }] = retry.effects.as_slice()
		else {
			panic!("retry effect");
		};
		assert_ne!(run, *next);
		assert_eq!(
			panel
				.update(Message::Progress {
					run,
					progress: Box::new(crate::fonts::Progress::queued(&id))
				})
				.invalidation,
			Invalidation::None
		);
		assert!(
			panel
				.update(Message::Settled {
					run,
					summary: summary()
				})
				.effects
				.is_empty()
		);
		assert!(panel.view().jobs.contains_key(&id));
		assert!(matches!(
			panel
				.update(Message::Settled {
					run: *next,
					summary: summary()
				})
				.effects
				.as_slice(),
			[Effect::Completed { stored: 1, .. }]
		));
		assert!(!panel.view().jobs.contains_key(&id));
	}
}
