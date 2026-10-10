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
		self.scrolling.offset = target.clamp(
			0.0,
			crate::state::scroll_limit(self.snapshot.height, viewport),
		);
		self.scrolling.target = None;
		self.saved_reading = None;
	}
}

fn legacy_origin() -> crate::security::Origin {
	crate::security::Origin::Local(crate::security::Trust::Trusted)
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct SavedTab {
	#[serde(default = "legacy_origin")]
	origin: crate::security::Origin,
	path: PathBuf,
	reading: Reading,
}
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct Session {
	version: u32,
	active: usize,
	tabs: Vec<SavedTab>,
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
		let tabs = self
			.entries()
			.iter()
			.enumerate()
			.filter_map(|(index, tab)| {
				if tab.path.starts_with(&paste) {
					return None;
				}
				if index <= self.active() {
					active += 1;
				}
				let session = if index == self.active() {
					&self.session
				} else {
					&tab.session
				};
				Some(SavedTab {
					origin: session.security.origin.clone(),
					path: tab.path.clone(),
					reading: Reading::capture(session),
				})
			})
			.collect();
		Session {
			version: 1,
			active: active.saturating_sub(1),
			tabs,
		}
	}
	fn signature(&self) -> u64 {
		use std::hash::{Hash, Hasher};
		let mut hash = std::hash::DefaultHasher::new();
		self.active().hash(&mut hash);
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
		}
		hash.finish()
	}
	fn restore_session(&mut self, session: Session) {
		let selected = session.active;
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
			let entry = self.restore_tab(path, tab.reading);
			self.session_mut(entry).security.origin = tab.origin;
			if closest
				.is_none_or(|(distance, _)| index.abs_diff(selected) < distance)
			{
				closest = Some((index.abs_diff(selected), entry));
			}
		}
		if let Some((_, index)) = closest {
			self.select(index, Instant::now());
		}
	}
}

impl<P> App<P> {
	fn owns_session(&self) -> bool {
		cfg!(target_os = "android") || self.instance.is_some()
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
		let session = self.readers.capture_session(self.paste_dir.path());
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
				Ok(session) => self.readers.restore_session(session),
				Err(error) if !path.exists() => {
					log::debug!("No saved browsing session: {error:#}")
				}
				Err(error) => {
					log::warn!("Ignoring browsing session: {error:#}")
				}
			}
		}
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
		let signature = self.readers.signature();
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
