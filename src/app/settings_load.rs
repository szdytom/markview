//! Settings catalogues are built in the CPU service and published as snapshots.
use super::{App, Event, SendEvent, font_panel::Choices};
use crate::{fonts::FaceInfo, state::Command};
use markview_core::{fonts::FontConfig, style::StyleTarget};
use std::{
	path::PathBuf,
	sync::{Arc, Mutex},
	time::{Instant, SystemTime},
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
	Reader,
	Export,
	Catalog,
	Choices,
}
impl Kind {
	fn index(self) -> usize {
		self as usize
	}
}
#[derive(Clone, PartialEq, Eq)]
struct Input {
	selected: Option<Vec<String>>,
	fonts: Option<FontConfig>,
	epoch: u64,
}
#[derive(Default)]
pub(super) enum Status {
	#[default]
	Idle,
	Loading,
	Ready,
	Failed(String),
}
#[derive(Default)]
pub(super) struct Load {
	pub status: Status,
	pub cached: bool,
	pub displayed: bool,
	version: u64,
	input: Option<Input>,
	cancel: CancellationToken,
}
impl Load {
	fn begin(
		&mut self,
		input: Input,
		check: bool,
	) -> Option<(u64, CancellationToken)> {
		if self.input.as_ref() == Some(&input)
			&& (matches!(self.status, Status::Loading)
				|| (!check && matches!(self.status, Status::Ready)))
		{
			return None;
		}
		self.cancel.cancel();
		self.cancel = CancellationToken::new();
		self.version += 1;
		self.input = Some(input);
		self.status = Status::Loading;
		Some((self.version, self.cancel.clone()))
	}
	pub fn blocked(&self) -> bool {
		!self.displayed || !matches!(self.status, Status::Ready)
	}
	pub fn message(&self, lang: crate::lang::Lang) -> Option<String> {
		if !self.displayed && !matches!(self.status, Status::Failed(_)) {
			return Some(lang.panel_loading().into());
		}
		match &self.status {
			Status::Loading => Some(
				if self.cached {
					lang.panel_refreshing()
				} else {
					lang.panel_loading()
				}
				.into(),
			),
			Status::Failed(error) => Some(lang.panel_load_failed(error)),
			_ => None,
		}
	}
}

type Stamp = (PathBuf, u64, Option<SystemTime>);
fn files(
	dir: Option<&std::path::Path>,
	accept: impl Fn(&std::path::Path) -> bool,
) -> Vec<Stamp> {
	let mut files: Vec<_> = dir
		.into_iter()
		.filter_map(|dir| std::fs::read_dir(dir).ok())
		.flatten()
		.flatten()
		.filter_map(|entry| {
			let path = entry.path();
			if !accept(&path) {
				return None;
			}
			let meta = path.metadata().ok()?;
			meta.is_file()
				.then(|| (path, meta.len(), meta.modified().ok()))
		})
		.collect();
	files.sort_by(|a, b| a.0.cmp(&b.0));
	files
}
#[derive(Default)]
struct StyleCache {
	key: Option<(Vec<Stamp>, Option<Vec<String>>)>,
	entries: Vec<crate::stylesheet::Entry>,
}
impl StyleCache {
	fn read(
		&mut self,
		dir: Option<&std::path::Path>,
		selected: Option<&[String]>,
		target: StyleTarget,
	) -> Vec<crate::stylesheet::Entry> {
		let stamps = files(dir, |p| {
			p.file_name()
				.is_some_and(|n| n.to_string_lossy().ends_with(".mvss.toml"))
		});
		let key = (stamps, selected.map(<[String]>::to_vec));
		if self.key.as_ref() != Some(&key) {
			self.entries =
				crate::stylesheet::catalog_for(dir, selected, target);
			self.key = Some(key);
		}
		self.entries.clone()
	}
}
#[derive(Default)]
struct FaceCache {
	entries: Vec<(Stamp, Option<FaceInfo>)>,
}
impl FaceCache {
	fn read(&mut self, dir: Option<&std::path::Path>) -> Vec<FaceInfo> {
		let stamps = files(dir, markview_core::fonts::is_font_file);
		let mut old = std::mem::take(&mut self.entries);
		self.entries = stamps
			.into_iter()
			.map(|stamp| {
				let face = if let Some(index) =
					old.iter().position(|(key, _)| *key == stamp)
				{
					old.swap_remove(index).1
				} else {
					crate::fonts::describe_one(&stamp.0)
				};
				(stamp, face)
			})
			.collect();
		self.entries
			.iter()
			.filter_map(|(_, face)| face.clone())
			.collect()
	}
}
#[derive(Default)]
struct Cache {
	styles: [StyleCache; 2],
	faces: FaceCache,
}

pub(super) struct Resources {
	loads: [Load; 4],
	epoch: u64,
	cache: Arc<Mutex<Cache>>,
	feedback: Option<(Kind, Instant, bool)>,
	pub export_entries: Vec<crate::stylesheet::Entry>,
}
impl Resources {
	pub fn new() -> Self {
		Self {
			loads: Default::default(),
			epoch: 0,
			cache: Default::default(),
			export_entries: Vec::new(),
			feedback: None,
		}
	}
	pub fn presented(&mut self, visible: Option<Kind>) -> bool {
		if let Some((kind, started, cached)) = self.feedback.take()
			&& visible == Some(kind)
		{
			log::debug!(
				"settings {:?} click→present: {:.2} ms (cached: {})",
				kind,
				started.elapsed().as_secs_f64() * 1000.,
				cached
			);
		}
		let Some(kind) = visible else {
			return false;
		};
		let load = &mut self.loads[kind.index()];
		let first = !load.displayed;
		load.displayed = true;
		first && matches!(load.status, Status::Ready | Status::Failed(_))
	}
	pub fn load(&self, kind: Kind) -> &Load {
		&self.loads[kind.index()]
	}
	pub fn invalidate(&mut self) {
		self.epoch += 1;
		for kind in [Kind::Reader, Kind::Export, Kind::Catalog] {
			let load = &mut self.loads[kind.index()];
			load.cancel.cancel();
			load.version += 1;
			load.input = None;
			load.status = Status::Idle;
		}
	}
}
pub(super) enum Snapshot {
	Styles(Vec<crate::stylesheet::Entry>),
	Catalog(Vec<crate::fonts::Family>),
	Choices(Choices),
}
pub(super) struct Completion {
	kind: Kind,
	version: u64,
	result: anyhow::Result<Snapshot>,
	started: Instant,
}

/// Keeps drawn and interactive button availability in agreement.
pub(super) fn prepare_button(button: &mut super::Button, load: Option<&Load>) {
	if load.is_some_and(Load::blocked) && dependent(button.action) {
		button.enabled = false;
	}
}

/// Commands that depend on positions or availability in the current catalogue.
pub(super) fn dependent(command: Command) -> bool {
	use super::font_panel::Command as Font;
	matches!(
		command,
		Command::StyleToggle(_)
			| Command::StyleUp(_)
			| Command::StyleDown(_)
			| Command::ExportStyleToggle(_)
			| Command::ExportStyleUp(_)
			| Command::ExportStyleDown(_)
			| Command::SystemTheme
			| Command::FontFamily(_, _)
			| Command::ToggleDropdown(crate::state::DropdownId::Font(_), _)
			| Command::Fonts(
				Font::DownloadMissing
					| Font::DownloadAll
					| Font::DownloadOne(_)
					| Font::RedownloadOne(_)
			)
	)
}
impl<P: SendEvent> App<P> {
	pub(super) fn settings_kind(&self) -> Option<Kind> {
		if self.interaction.export_styles_open() {
			Some(Kind::Export)
		} else if self.interaction.styles_open() {
			Some(Kind::Reader)
		} else if self.interaction.fonts_open() {
			Some(if self.font_panel.choosers() {
				Kind::Choices
			} else {
				Kind::Catalog
			})
		} else {
			None
		}
	}
	pub(super) fn settings_load(&self) -> Option<&Load> {
		self.settings_kind()
			.map(|kind| &self.settings_resources.loads[kind.index()])
	}
	pub(super) fn settings_action_enabled(&self, command: Command) -> bool {
		!dependent(command)
			|| self.settings_load().is_none_or(|load| !load.blocked())
	}
	pub(super) fn refresh_settings_resources(&mut self, check: bool) {
		let Some(kind) = self.settings_kind() else {
			return;
		};
		let input = Input {
			selected: match kind {
				Kind::Export => Some(self.preferences.export.style.clone()),
				Kind::Choices => None,
				_ => self.preferences.values.style.clone(),
			},
			fonts: matches!(kind, Kind::Catalog | Kind::Choices)
				.then(|| self.fonts_config.clone()),
			epoch: if kind == Kind::Choices {
				0
			} else {
				self.settings_resources.epoch
			},
		};
		let load = &mut self.settings_resources.loads[kind.index()];
		// Candidate names are already cached by `FontConfig`; no filesystem check is needed.
		let started = Instant::now();
		let request = load.begin(input.clone(), check && kind != Kind::Choices);
		if check || request.is_some() {
			self.settings_resources.feedback =
				Some((kind, started, load.cached));
		}
		let Some((version, cancel)) = request else {
			return;
		};
		if kind == Kind::Choices {
			self.interaction.dropdown = None;
		}
		let cache = self.settings_resources.cache.clone();
		let proxy = self.proxy.clone();
		let services = self.services.handle.clone();
		let send = self.proxy.clone();
		if !self.services.handle.submit(async move {
			let result = services
				.compute(0, &cancel, move || {
					let snapshot = match kind {
						Kind::Choices => {
							let mut choices = Choices::default();
							choices.refresh(input.fonts.as_ref().unwrap());
							choices.generation = version;
							Snapshot::Choices(choices)
						}
						_ => {
							let mut cache = markview_core::sync::cache(
								&cache,
								"Settings catalogue cache",
							);
							let target = if kind == Kind::Export {
								StyleTarget::Pdf
							} else {
								StyleTarget::Ui
							};
							let entries = cache.styles
								[usize::from(kind == Kind::Export)]
							.read(
								crate::stylesheet::directory().as_deref(),
								input.selected.as_deref(),
								target,
							);
							if kind == Kind::Catalog {
								let faces = cache
									.faces
									.read(crate::fonts::directory().as_deref());
								let builtin =
									markview_core::style::Stylesheet::builtin();
								let sheets = std::iter::once((
									"builtin",
									builtin.font_families.as_slice(),
								))
								.chain(entries.iter().map(|entry| {
									(
										entry.id.as_str(),
										entry.font_families.as_slice(),
									)
								}));
								Snapshot::Catalog(crate::fonts::catalog_faces(
									sheets,
									&faces,
									input.fonts.as_ref().unwrap(),
								))
							} else {
								Snapshot::Styles(entries)
							}
						}
					};
					Ok(snapshot)
				})
				.await;
			proxy.send(Event::SettingsLoaded(Box::new(Completion {
				kind,
				version,
				result,
				started,
			})));
		}) {
			send.send(Event::SettingsLoaded(Box::new(Completion {
				kind,
				version,
				result: Err(anyhow::anyhow!("CPU service closed")),
				started,
			})));
		}
	}
	#[cfg(test)]
	pub(super) fn complete_choices_fixture(&mut self) {
		let mut choices = Choices::default();
		choices.refresh(&self.fonts_config);
		let version =
			self.settings_resources.loads[Kind::Choices.index()].version;
		choices.generation = version;
		self.settings_resources.loads[Kind::Choices.index()].displayed = true;
		self.settings_loaded(Completion {
			kind: Kind::Choices,
			version,
			result: Ok(Snapshot::Choices(choices)),
			started: Instant::now(),
		});
	}
	pub(super) fn settings_loaded(&mut self, completion: Completion) {
		let load = &mut self.settings_resources.loads[completion.kind.index()];
		if load.version != completion.version
			|| !matches!(load.status, Status::Loading)
		{
			return;
		}
		log::debug!(
			"settings {:?} ready in {:.2} ms (cached: {})",
			completion.kind,
			completion.started.elapsed().as_secs_f64() * 1000.,
			load.cached
		);
		match completion.result {
			Ok(snapshot) => {
				match snapshot {
					Snapshot::Styles(entries)
						if completion.kind == Kind::Export =>
					{
						self.settings_resources.export_entries = entries
					}
					Snapshot::Styles(entries) => {
						self.preferences.style_entries = entries
					}
					Snapshot::Catalog(catalog) => {
						self.font_panel.set_catalog(catalog)
					}
					Snapshot::Choices(choices) => {
						self.font_panel.set_choices(choices)
					}
				}
				load.cached = true;
				load.status = Status::Ready;
			}
			Err(error) => load.status = Status::Failed(error.to_string()),
		}
		self.refresh_hover();
		self.redraw();
	}
}

#[cfg(test)]
mod tests;
