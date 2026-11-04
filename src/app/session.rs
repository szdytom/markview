//! Versioned browsing metadata; document contents stay in their source files.
use super::{App, tabs::Tabs};
use crate::{layout::LayoutSnapshot, state::ReaderSession};
use serde::{Deserialize, Serialize};
use std::{
	collections::BTreeMap,
	fs,
	io::Write,
	path::{Path, PathBuf},
	sync::Arc,
	time::{Duration, Instant},
};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Position {
	/// Digest of the input bytes this position was captured on.
	#[serde(default)]
	pub(super) input: u64,
	/// Front-end and parse-option version that produced that input.
	#[serde(default)]
	pub(super) frontend: u64,
	/// Structural child-index path from the document root to the block.
	#[serde(default)]
	path: Vec<u32>,
	/// The semantic field inside that block, absent above text.
	#[serde(default)]
	field: Option<u32>,
	/// UTF-8 offset inside the field named by `field`.
	byte: usize,
	local: f32,
	/// Absolute pixel fallback for an input that no longer matches.
	fallback: f32,
	/// Scroll fraction fallback, which survives height changes.
	#[serde(default)]
	fallback_ratio: f32,
}
impl Position {
	fn capture(
		snapshot: &LayoutSnapshot,
		document: Option<&crate::loaded::LoadedDocument>,
		scroll: f32,
	) -> Self {
		let mut position = Self {
			fallback: scroll,
			fallback_ratio: if snapshot.height > 0.0 {
				scroll / snapshot.height
			} else {
				0.0
			},
			..Self::default()
		};
		if let Some(document) = document {
			position.input = document.input_fingerprint();
			position.frontend = document.frontend_version();
		}
		let index = snapshot
			.blocks
			.partition_point(|b| b.y <= scroll)
			.saturating_sub(1);
		if let Some((index, block)) =
			snapshot.blocks.iter().enumerate().nth(index)
		{
			if let Some(document) = document
				&& let Some(path) = document.path_of(block.id)
			{
				position.path = path.iter().map(|i| *i as u32).collect();
			}
			position.local = scroll - block.y;
			if let Some((node, cluster, rect)) = block
				.clusters()
				.filter(|(_, _, rect)| rect.y + rect.h >= position.local)
				.min_by(|(_, _, a), (_, _, b)| {
					(a.y - position.local)
						.abs()
						.total_cmp(&(b.y - position.local).abs())
				}) {
				if let Some(semantic) = snapshot.semantic_at(
					index,
					node,
					cluster.range.start,
					markview_layout::text::Affinity::Before,
				) {
					position.field = Some(semantic.field.get());
					position.byte = semantic.offset;
				}
				position.local -= rect.y;
			}
		}
		position
	}
	fn resolve(
		&self,
		snapshot: &LayoutSnapshot,
		document: Option<&crate::loaded::LoadedDocument>,
	) -> Option<f32> {
		let document = document?;
		if document.input_fingerprint() != self.input
			|| document.frontend_version() != self.frontend
			|| self.path.is_empty()
		{
			return None;
		}
		let path: Vec<usize> = self.path.iter().map(|i| *i as usize).collect();
		let block = document.block_at_path(&path)?;
		let placed = snapshot.blocks.iter().find(|b| b.id == block.id())?;
		let y = match self.field {
			Some(field) => {
				let semantic = markview_document::SemanticPosition {
					projection: None,
					revision: document.document().revision(),
					block: block.id(),
					field: markview_document::FieldId::from_index(field),
					offset: self.byte,
					affinity: markview_document::Affinity::Before,
				};
				let geometry = snapshot.resolve_position(semantic)?;
				let c = placed
					.layout
					.text
					.get(geometry.node)?
					.clusters
					.iter()
					.filter(|c| c.range.start <= geometry.offset)
					.max_by_key(|c| c.range.start)?;
				placed
					.rect(c.command, c.rect)
					.map(|r| r.y)
					.or_else(|| placed.visible_ancestor_y(c.command))?
			}
			None => 0.0,
		};
		Some(placed.y + y + self.local)
	}
}

/// Collapse choices saved as structural paths, not as allocated block ids.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct SavedDetails(Vec<(Vec<u32>, bool)>);
impl SavedDetails {
	#[cfg(test)]
	pub(super) fn is_empty(&self) -> bool {
		self.0.is_empty()
	}
}
impl From<Vec<(Vec<u32>, bool)>> for SavedDetails {
	fn from(details: Vec<(Vec<u32>, bool)>) -> Self {
		Self(details)
	}
}
impl Serialize for SavedDetails {
	fn serialize<S: serde::Serializer>(
		&self,
		serializer: S,
	) -> Result<S::Ok, S::Error> {
		self.0.serialize(serializer)
	}
}
impl<'de> Deserialize<'de> for SavedDetails {
	fn deserialize<D: serde::Deserializer<'de>>(
		deserializer: D,
	) -> Result<Self, D::Error> {
		#[derive(Deserialize)]
		#[serde(untagged)]
		enum Repr {
			Paths(Vec<(Vec<u32>, bool)>),
			Ids(BTreeMap<String, bool>),
		}
		Ok(match Repr::deserialize(deserializer)? {
			Repr::Paths(paths) => Self(paths),
			// Older sessions named allocated ids; their version migration clears
			// these, because an id can point at another node after a restart.
			Repr::Ids(ids) => {
				let _ = ids;
				Self::default()
			}
		})
	}
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Reading {
	pub(super) position: Position,
	pub(super) details: SavedDetails,
}
impl Reading {
	pub(crate) fn capture(session: &ReaderSession) -> Self {
		if let Some(saved) = &session.saved_reading {
			let details = session
				.document
				.as_deref()
				.map(|document| details_of(document, &session.details_open))
				.unwrap_or_else(|| saved.details.clone());
			return Self {
				details,
				..saved.clone()
			};
		}
		Self {
			position: Position::capture(
				&session.snapshot,
				session.document.as_deref(),
				session.scrolling.offset,
			),
			details: session
				.document
				.as_deref()
				.map(|document| details_of(document, &session.details_open))
				.unwrap_or_default(),
		}
	}
}

/// The collapse state as a saved session stores it.
fn details_of(
	document: &crate::loaded::LoadedDocument,
	open: &BTreeMap<markview_document::BlockId, bool>,
) -> SavedDetails {
	SavedDetails(
		open.iter()
			.filter_map(|(id, open)| {
				Some((
					document
						.path_of(*id)?
						.iter()
						.map(|index| *index as u32)
						.collect(),
					*open,
				))
			})
			.collect(),
	)
}

/// A saved session's collapse state resolved against the current document.
pub(super) fn saved_details(
	document: &markview_document::Document,
	details: &SavedDetails,
) -> BTreeMap<markview_document::BlockId, bool> {
	details
		.0
		.iter()
		.filter_map(|(path, open)| {
			let path: Vec<usize> = path.iter().map(|i| *i as usize).collect();
			let block = document.block_at_path(&path)?;
			Some((block.id(), *open))
		})
		.collect()
}
impl ReaderSession {
	/// Resolves saved collapse paths against the document they were saved on.
	pub(crate) fn restore_disclosures(
		&mut self,
		document: &crate::loaded::LoadedDocument,
	) -> bool {
		let Some(saved) = self.saved_reading.as_mut() else {
			return false;
		};
		let matches = saved.position.input == document.input_fingerprint()
			&& saved.position.frontend == document.frontend_version();
		let details = if matches {
			saved_details(document.document(), &saved.details)
		} else {
			Default::default()
		};
		let had_saved = !saved.details.0.is_empty();
		saved.details = SavedDetails::default();
		let changed = self.details_open.as_ref() != &details;
		self.details_open = Arc::new(details);
		changed || had_saved
	}

	/// Drops saved disclosure choices when the loaded content no longer
	/// matches what they were made on.
	pub(crate) fn discard_stale_disclosures(
		&mut self,
		input: u64,
		frontend: u64,
	) -> bool {
		let stale = self.saved_reading.as_ref().is_some_and(|saved| {
			saved.position.input != input || saved.position.frontend != frontend
		});
		if !stale || self.details_open.is_empty() {
			return false;
		}
		self.details_open = Default::default();
		if let Some(saved) = self.saved_reading.as_mut() {
			saved.details = SavedDetails::default();
		}
		true
	}

	pub(crate) fn restore_reading(&mut self, viewport: f32) {
		let Some(saved) = &self.saved_reading else {
			return;
		};
		let target = saved
			.position
			.resolve(&self.snapshot, self.document.as_deref());
		let target = match target {
			Some(y)
				if self.snapshot_complete
					|| y + viewport <= self.snapshot.height =>
			{
				y
			}
			_ if self.snapshot_complete => {
				if saved.position.fallback_ratio > 0.0 {
					saved.position.fallback_ratio
						* self.snapshot.scroll_limit(viewport)
				} else {
					saved.position.fallback
				}
			}
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
		let mut session: Self = serde_json::from_slice(&fs::read(path)?)?;
		anyhow::ensure!(
			matches!(session.version, 1..=4),
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
		if session.version < 4 {
			// Older versions stored layout node ordinals and raw block ids.
			// Keep only the scroll fallbacks and discard the stale structure.
			for tab in &mut session.tabs {
				tab.reading.position = Position {
					fallback: tab.reading.position.fallback,
					fallback_ratio: tab.reading.position.fallback_ratio,
					..Position::default()
				};
				tab.reading.details = SavedDetails::default();
			}
			session.version = 4;
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
			version: 4,
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
			let web = match &tab.origin {
				crate::security::Origin::Web(url) => Some(url.clone()),
				_ => None,
			};
			let path = if let Some(url) = &web {
				super::web_page::web_path(url)
			} else {
				let Ok(path) = fs::canonicalize(&tab.path) else {
					continue;
				};
				if !path.is_file()
					|| crate::file::open_regular(&path).is_err()
					|| self.find_origin(&path, &tab.origin).is_some()
				{
					continue;
				}
				path
			};
			if self.find_origin(&path, &tab.origin).is_some() {
				continue;
			}
			let search = tab.search;
			let origin = tab.origin;
			let entry = self.restore_tab(path, tab.reading);
			let session = self.session_mut(entry);
			if let Some(url) = &web {
				session.security = crate::security::Security::web(url.clone());
			}
			session.security.origin = origin;
			session.search.open = search.open;
			session.search.query = search.query;
			session.search.options.case_sensitive = search.case_sensitive;
			session.search.options.whole_word = search.whole_word;
			session.search.needs_query = true;
			if let Some(url) = &web {
				// Only the selected web tab is fetched now, so every other one
				// keeps this label until it is opened.
				self.set_title(entry, super::web_page::web_label(url));
			}
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
					self.visit_active();
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
		if let crate::security::Origin::Web(url) =
			self.readers.session.security.origin.clone()
			&& self.readers.session.document.is_none()
		{
			self.open_web_page(url);
			return;
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
