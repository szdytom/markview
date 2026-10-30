//! Versioned browsing metadata; document contents stay in their source files.
use super::{App, tabs::Tabs};
use crate::{layout::LayoutSnapshot, state::ReaderSession};
use serde::{Deserialize, Serialize};
use std::{
	collections::BTreeMap,
	fs,
	io::Write,
	path::{Path, PathBuf},
	time::{Duration, Instant},
};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Position {
	block: u64,
	occurrence: usize,
	source: usize,
	node: Option<usize>,
	byte: usize,
	local: f32,
	fallback: f32,
}
impl Position {
	fn capture(snapshot: &LayoutSnapshot, scroll: f32) -> Self {
		let mut position = Self {
			fallback: scroll,
			..Self::default()
		};
		let index = snapshot
			.blocks
			.partition_point(|b| b.y <= scroll)
			.saturating_sub(1);
		if let Some(block) = snapshot.blocks.get(index) {
			position.block = block.id;
			position.source = block.source.start;
			position.occurrence = snapshot.blocks[..index]
				.iter()
				.filter(|b| b.id == block.id)
				.count();
			position.local = scroll - block.y;
			if let Some((node, cluster, rect)) = block
				.clusters()
				.filter(|(_, _, rect)| rect.y + rect.h >= position.local)
				.min_by(|(_, _, a), (_, _, b)| {
					(a.y - position.local)
						.abs()
						.total_cmp(&(b.y - position.local).abs())
				}) {
				position.node = Some(node);
				position.byte = cluster.range.start;
				position.local -= rect.y;
			}
		}
		position
	}
	fn resolve(&self, snapshot: &LayoutSnapshot) -> Option<f32> {
		let block = snapshot
			.blocks
			.iter()
			.filter(|b| b.id == self.block)
			.nth(self.occurrence)?;
		let y = match self.node {
			Some(node) => {
				let c = block
					.layout
					.text
					.get(node)?
					.clusters
					.iter()
					.find(|c| c.range.contains(&self.byte))?;
				block
					.rect(c.command, c.rect)
					.map(|r| r.y)
					.or_else(|| block.visible_ancestor_y(c.command))?
			}
			None => 0.0,
		};
		Some(block.y + y + self.local)
	}
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Reading {
	pub(super) content: u64,
	position: Position,
	pub(super) details: BTreeMap<u64, bool>,
}
impl Reading {
	pub(crate) fn capture(session: &ReaderSession) -> Self {
		if let Some(saved) = &session.saved_reading {
			return Self {
				details: (*session.details_open).clone(),
				..saved.clone()
			};
		}
		Self {
			content: session.accepted_content_id,
			position: Position::capture(
				&session.snapshot,
				session.scrolling.offset,
			),
			details: (*session.details_open).clone(),
		}
	}
}
impl ReaderSession {
	pub(crate) fn restore_reading(&mut self, viewport: f32) {
		let Some(saved) = &self.saved_reading else {
			return;
		};
		let target = saved.position.resolve(&self.snapshot);
		let target = match target {
			Some(y)
				if self.snapshot_complete
					|| y + viewport <= self.snapshot.height =>
			{
				y
			}
			_ if self.snapshot_complete => saved.position.fallback,
			_ => return,
		};
		self.scrolling.offset =
			target.clamp(0.0, self.snapshot.scroll_limit(viewport));
		self.scrolling.target = None;
		self.saved_reading = None;
	}
}

fn legacy_origin() -> crate::security::Origin {
	crate::security::Origin::Local(crate::security::Trust::Trusted)
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct SavedSearch {
	#[serde(default)]
	open: bool,
	#[serde(default)]
	query: String,
	#[serde(default)]
	case_sensitive: bool,
	#[serde(default)]
	whole_word: bool,
}
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct SavedTab {
	#[serde(default = "legacy_origin")]
	origin: crate::security::Origin,
	path: PathBuf,
	reading: Reading,
	#[serde(default)]
	search: SavedSearch,
}
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct Session {
	#[serde(default)]
	folder: Option<PathBuf>,
	#[serde(default)]
	folder_open: bool,
	version: u32,
	active: usize,
	tabs: Vec<SavedTab>,
	#[serde(default)]
	recent: Vec<usize>,
}
impl Session {
	fn read(path: &Path) -> anyhow::Result<Self> {
		let session: Self = serde_json::from_slice(&fs::read(path)?)?;
		anyhow::ensure!(
			session.version == 1,
			"Unsupported session version {}",
			session.version
		);
		for tab in &session.tabs {
			anyhow::ensure!(
				tab.reading.position.local.is_finite()
					&& tab.reading.position.fallback.is_finite(),
				"Invalid session position"
			);
		}
		Ok(session)
	}
	fn write(&self, path: &Path) -> anyhow::Result<()> {
		let parent = path.parent().unwrap();
		fs::create_dir_all(parent)?;
		let mut temp = tempfile::NamedTempFile::new_in(parent)?;
		serde_json::to_writer(&mut temp, self)?;
		temp.flush()?;
		temp.as_file().sync_all()?;
		temp.persist(path)?;
		if let Ok(dir) = fs::File::open(parent) {
			let _ = dir.sync_all();
		}
		Ok(())
	}
}

#[derive(Default)]
pub(super) struct Persistence {
	pub(super) path: Option<PathBuf>,
	pub(super) initialized: bool,
	pub(super) enabled: bool,
	pub(super) signature: Option<u64>,
	pub(super) deadline: Option<Instant>,
}
impl Tabs {
	fn capture_session(&self, paste: &Path) -> Session {
		let paste = paste.canonicalize().unwrap_or_else(|_| paste.to_owned());
		let mut active: usize = 0;
		let mut tabs = Vec::new();
		let mut saved = vec![None; self.entries().len()];
		for (index, tab) in self.entries().iter().enumerate() {
			if tab.path.starts_with(&paste) {
				continue;
			}
			if index <= self.active() {
				active += 1;
			}
			let session = if index == self.active() {
				&self.session
			} else {
				&tab.session
			};
			saved[index] = Some(tabs.len());
			tabs.push(SavedTab {
				origin: session.security.origin.clone(),
				path: tab.path.clone(),
				reading: Reading::capture(session),
				search: SavedSearch {
					open: session.search.open,
					query: if session.search.needs_query {
						session.search.query.clone()
					} else {
						session.search.input.text().to_owned()
					},
					case_sensitive: session.search.options.case_sensitive,
					whole_word: session.search.options.whole_word,
				},
			});
		}
		let recent = self
			.recent_order()
			.into_iter()
			.filter_map(|id| {
				let index =
					self.entries().iter().position(|tab| tab.id == id)?;
				saved[index]
			})
			.collect();
		Session {
			folder: None,
			folder_open: false,
			version: 1,
			active: active.saturating_sub(1),
			tabs,
			recent,
		}
	}
	fn signature(&self) -> u64 {
		use std::hash::{Hash, Hasher};
		let mut hash = std::hash::DefaultHasher::new();
		self.active().hash(&mut hash);
		self.recent_order().hash(&mut hash);
		for (index, tab) in self.entries().iter().enumerate() {
			tab.path.hash(&mut hash);
			let session = if index == self.active() {
				&self.session
			} else {
				&tab.session
			};
			session.scrolling.offset.to_bits().hash(&mut hash);
			session.details_open.hash(&mut hash);
			session.security.origin.hash(&mut hash);
			session.accepted_content_id.hash(&mut hash);
			session.search.open.hash(&mut hash);
			session.search.input.text().hash(&mut hash);
			session.search.options.case_sensitive.hash(&mut hash);
			session.search.options.whole_word.hash(&mut hash);
		}
		hash.finish()
	}
	fn restore_session(&mut self, session: Session) {
		let selected = session.active;
		let count = session.tabs.len();
		let recent = session.recent;
		let mut restored = vec![None; count];
		let mut closest = None;
		for (index, tab) in session.tabs.into_iter().enumerate() {
			let Ok(path) = fs::canonicalize(&tab.path) else {
				continue;
			};
			if !path.is_file()
				|| crate::file::open_regular(&path).is_err()
				|| self.find_origin(&path, &tab.origin).is_some()
			{
				continue;
			}
			let search = tab.search;
			let origin = tab.origin;
			let entry = self.restore_tab(path, tab.reading);
			let session = self.session_mut(entry);
			session.security.origin = origin;
			session.search.open = search.open;
			session.search.query = search.query;
			session.search.options.case_sensitive = search.case_sensitive;
			session.search.options.whole_word = search.whole_word;
			session.search.needs_query = true;
			restored[index] = Some(entry);
			if closest
				.is_none_or(|(distance, _)| index.abs_diff(selected) < distance)
			{
				closest = Some((index.abs_diff(selected), entry));
			}
		}
		let order: Vec<_> = recent
			.into_iter()
			.filter_map(|index| {
				let entry = (*restored.get(index)?)?;
				Some(self.entries()[entry].id)
			})
			.collect();
		let first_recent = order.first().copied();
		self.set_recent(order);
		let chosen = restored
			.get(selected)
			.and_then(|entry| *entry)
			.or_else(|| {
				first_recent.and_then(|id| {
					self.entries().iter().position(|tab| tab.id == id)
				})
			})
			.or_else(|| closest.map(|(_, entry)| entry));
		if let Some(index) = chosen {
			self.select(index, Instant::now());
		}
		self.remember_active();
	}
}

impl<P> App<P> {
	fn owns_session(&self) -> bool {
		markview_platform::touch_layout()
			|| self
				.instance
				.as_ref()
				.is_some_and(|instance| instance.owns_session)
	}
	pub(super) fn flush_session(&mut self) {
		self.persistence.deadline = None;
		if !self.persistence.initialized
			|| !self.preferences.values.restore_session
			|| !self.owns_session()
		{
			return;
		}
		let Some(path) = &self.persistence.path else {
			return;
		};
		let mut session = self.readers.capture_session(self.paste_dir.path());
		session.folder = self.folder.root().cloned();
		session.folder_open = self.folder.open;
		if let Err(error) = session.write(path) {
			log::warn!("Cannot save browsing session: {error:#}");
		}
	}
}
impl<P: super::SendEvent> App<P> {
	pub(super) fn restore_session(&mut self) {
		if self.persistence.initialized {
			return;
		}
		self.persistence.initialized = true;
		self.persistence.enabled = self.preferences.values.restore_session;
		if !self.persistence.enabled || !self.owns_session() {
			return;
		}
		if let Some(path) = &self.persistence.path {
			match Session::read(path) {
				Ok(session) => {
					let folder = session.folder.clone();
					self.folder.open = session.folder_open;
					self.readers.restore_session(session);
					if let Some(root) = folder {
						self.open_folder(root, false);
					}
				}
				Err(error) if !path.exists() => {
					log::debug!("No saved browsing session: {error:#}")
				}
				Err(error) => {
					log::warn!("Ignoring browsing session: {error:#}")
				}
			}
		}
		self.restore_search();
		if self.readers.session.path.is_some() {
			self.observe_document();
			self.request(false);
		}
	}
	pub(super) fn sync_session_settings(&mut self) {
		self.register_instance();
		let enabled = self.preferences.values.restore_session;
		if self.persistence.enabled
			&& !enabled
			&& self.owns_session()
			&& let Some(path) = &self.persistence.path
			&& let Err(error) = fs::remove_file(path)
			&& error.kind() != std::io::ErrorKind::NotFound
		{
			log::warn!("Cannot clear browsing session: {error:#}");
		}
		self.persistence.enabled = enabled;
		self.persistence.signature = None;
		self.persistence.deadline = None;
	}
	pub(super) fn session_tick(&mut self, now: Instant) {
		if !self.persistence.initialized
			|| !self.preferences.values.restore_session
			|| !self.owns_session()
		{
			return;
		}
		let signature = {
			use std::hash::{Hash, Hasher};
			let mut hash = std::hash::DefaultHasher::new();
			self.readers.signature().hash(&mut hash);
			self.folder.root().hash(&mut hash);
			self.folder.open.hash(&mut hash);
			hash.finish()
		};
		if self.persistence.signature != Some(signature) {
			self.persistence.signature = Some(signature);
			self.persistence
				.deadline
				.get_or_insert(now + Duration::from_secs(1));
		}
		if self.persistence.deadline.is_some_and(|d| d <= now) {
			self.flush_session();
		}
	}
}

#[cfg(test)]
mod tests;
