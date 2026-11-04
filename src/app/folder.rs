//! A read-only folder tree shared by native entry points and the sidebar.
pub(super) mod observe;
use super::{App, Event, SendEvent};
use crate::state::ChromeCommand;
use crate::state::{Command, DrawerAnimation, Row, RowSlide};
use markview_selection::Scroll;
use observe::Observer;
use std::{
	collections::{BTreeMap, HashMap, HashSet},
	fs,
	path::{Path, PathBuf},
	sync::{
		Arc,
		atomic::{AtomicU64, Ordering},
	},
	time::Instant,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Child {
	path: PathBuf,
	directory: bool,
	excluded: bool,
}

fn excluded(path: &Path, directory: bool) -> bool {
	let name = path.file_name().unwrap().to_string_lossy();
	name.starts_with('.')
		|| (directory
			&& ["node_modules", "target", "vendor", "__pycache__"]
				.contains(&name.as_ref()))
}

fn list(
	path: &Path,
	cancelled: &dyn Fn() -> bool,
	show: bool,
) -> anyhow::Result<Vec<Child>> {
	let mut children = Vec::new();
	for child in fs::read_dir(path)?.flatten() {
		anyhow::ensure!(!cancelled(), "Folder scan cancelled");
		let Ok(kind) = child.file_type() else {
			continue;
		};
		let path = child.path();
		let directory = kind.is_dir();
		let excluded = excluded(&path, directory);
		if (!excluded || show)
			&& (directory
				|| (kind.is_file()
					&& (markdown(&path) || crate::file::is_image(&path))))
		{
			children.push(Child {
				path,
				directory,
				excluded,
			});
		}
	}
	children.sort_by_key(|child| {
		(
			!child.directory,
			child
				.path
				.file_name()
				.unwrap()
				.to_string_lossy()
				.to_lowercase(),
			child.path.clone(),
		)
	});
	Ok(children)
}

/// Stable slots survive sibling insertions; removed identities are never reused.
#[derive(Debug)]
pub(super) struct Entries(BTreeMap<usize, Entry>);
impl Entries {
	pub fn iter(&self) -> impl Iterator<Item = &Entry> {
		self.0.values()
	}
	pub fn get(&self, id: usize) -> Option<&Entry> {
		self.0.get(&id)
	}
	#[cfg(test)]
	pub fn len(&self) -> usize {
		self.0.len()
	}
	pub fn is_empty(&self) -> bool {
		self.0.is_empty()
	}
}
impl std::ops::Index<usize> for Entries {
	type Output = Entry;
	fn index(&self, id: usize) -> &Entry {
		&self.0[&id]
	}
}

#[derive(Debug)]
pub(super) struct Entry {
	pub id: usize,
	pub path: PathBuf,
	pub depth: usize,
	pub directory: bool,
	pub excluded: bool,
	pub file: Option<Arc<super::file_search::File>>,
}

#[derive(Debug)]
pub(super) struct Tree {
	pub root: PathBuf,
	pub name: String,
	pub entries: Entries,
	children: HashMap<PathBuf, Vec<usize>>,
	paths: HashMap<PathBuf, usize>,
	next_id: usize,
	pub show_excluded: bool,
	pub error: Option<String>,
	expanded: HashSet<PathBuf>,
	visible: Vec<usize>,
}

impl Tree {
	#[cfg(test)]
	pub fn read(root: PathBuf) -> anyhow::Result<Self> {
		Self::read_until(root, || false)
	}

	fn read_until(
		root: PathBuf,
		cancelled: impl Fn() -> bool,
	) -> anyhow::Result<Self> {
		let root = fs::canonicalize(root)?;
		anyhow::ensure!(root.is_dir(), "Not a folder: {}", root.display());
		let name = markview_platform::folder_name(&root);
		Self::scan(root, name, cancelled, |path, cancelled| {
			Ok(list(path, cancelled, false)?
				.into_iter()
				.map(|child| (child.path, child.directory))
				.collect())
		})
	}

	fn scan(
		root: PathBuf,
		name: String,
		cancelled: impl Fn() -> bool,
		mut list: impl FnMut(
			&Path,
			&dyn Fn() -> bool,
		) -> anyhow::Result<Vec<(PathBuf, bool)>>,
	) -> anyhow::Result<Self> {
		let mut entries = Vec::new();
		let mut pending = vec![(root.clone(), 0, true)];
		while let Some((path, depth, directory)) = pending.pop() {
			anyhow::ensure!(!cancelled(), "Folder scan cancelled");
			if path != root {
				entries.push(Entry {
					file: (!directory).then(|| {
						super::file_search::File::new(
							entries.len(),
							&root,
							&path,
						)
					}),
					directory,
					path: path.clone(),
					depth: depth - 1,
					id: entries.len(),
					excluded: excluded(&path, directory),
				});
			}
			if !directory {
				continue;
			}
			let mut children = match list(&path, &cancelled) {
				Ok(children) => children,
				Err(error) if path == root || cancelled() => return Err(error),
				Err(error) => {
					log::warn!(
						"Cannot read folder {}: {error}",
						path.display()
					);
					continue;
				}
			};
			children.sort_by_key(|(path, directory)| {
				(
					!*directory,
					path.file_name().unwrap().to_string_lossy().to_lowercase(),
					path.clone(),
				)
			});
			// Initial identities follow the directory traversal order.
			for (path, directory) in children.into_iter().rev() {
				pending.push((path, depth + 1, directory));
			}
		}
		let next_id = entries.len();
		let mut children: HashMap<PathBuf, Vec<usize>> = HashMap::new();
		let paths = entries
			.iter()
			.map(|entry| (entry.path.clone(), entry.id))
			.collect();
		for entry in &entries {
			children
				.entry(entry.path.parent().unwrap().to_owned())
				.or_default()
				.push(entry.id);
		}
		let mut tree = Self {
			root,
			name,
			entries: Entries(
				entries.into_iter().map(|entry| (entry.id, entry)).collect(),
			),
			children,
			paths,
			next_id,
			show_excluded: false,
			error: None,
			expanded: HashSet::new(),
			visible: Vec::new(),
		};
		tree.rebuild_rows();
		Ok(tree)
	}

	pub fn rows(&self) -> &[usize] {
		&self.visible
	}

	fn append_rows(&self, path: &Path, out: &mut Vec<usize>) {
		if let Some(children) = self.children.get(path) {
			for id in children {
				out.push(*id);
				if self.entries[*id].directory
					&& self.expanded.contains(&self.entries[*id].path)
				{
					self.append_rows(&self.entries[*id].path, out);
				}
			}
		}
	}

	fn rebuild_rows(&mut self) {
		let mut rows = Vec::new();
		self.append_rows(&self.root, &mut rows);
		self.visible = rows;
	}

	fn remove(&mut self, id: usize) {
		let entry = self.entries.0.remove(&id).unwrap();
		self.paths.remove(&entry.path);
		self.expanded.remove(&entry.path);
		if let Some(children) = self.children.remove(&entry.path) {
			for child in children {
				self.remove(child);
			}
		}
	}

	fn patch(&mut self, path: &Path, children: Vec<Child>) {
		if path != self.root && !self.paths.contains_key(path) {
			return;
		}
		let depth = self
			.paths
			.get(path)
			.map_or(0, |id| self.entries[*id].depth + 1);
		// Capture the old visible range before removing descendants.
		let range = if path == self.root {
			Some(0..self.visible.len())
		} else {
			self.paths
				.get(path)
				.and_then(|id| self.visible.iter().position(|row| row == id))
				.map(|start| {
					let mut end = start + 1;
					while end < self.visible.len()
						&& self.entries[self.visible[end]].depth >= depth
					{
						end += 1;
					}
					start + 1..end
				})
		};
		let previous = self.children.remove(path).unwrap_or_default();
		let mut remaining: HashMap<_, _> = previous
			.into_iter()
			.map(|id| (self.entries[id].path.clone(), id))
			.collect();
		let mut ids = Vec::new();
		for child in children {
			let id = if let Some(id) = remaining.remove(&child.path) {
				if self.entries[id].directory == child.directory {
					id
				} else {
					self.remove(id);
					self.insert(child, depth)
				}
			} else {
				self.insert(child, depth)
			};
			ids.push(id);
		}
		for id in remaining.into_values() {
			self.remove(id);
		}
		self.children.insert(path.to_owned(), ids);
		if let Some(range) = range {
			let mut rows = Vec::new();
			if path == self.root || self.expanded.contains(path) {
				self.append_rows(path, &mut rows);
			}
			self.visible.splice(range, rows);
		}
	}

	fn insert(&mut self, child: Child, depth: usize) -> usize {
		let id = self.next_id;
		self.next_id += 1;
		self.paths.insert(child.path.clone(), id);
		self.entries.0.insert(
			id,
			Entry {
				file: (!child.directory).then(|| {
					super::file_search::File::new(id, &self.root, &child.path)
				}),
				id,
				path: child.path,
				depth,
				directory: child.directory,
				excluded: child.excluded,
			},
		);
		id
	}

	pub fn collapsed(&self, index: usize) -> bool {
		self.entries[index].directory
			&& !self.expanded.contains(&self.entries[index].path)
	}

	pub fn collapse_all(&mut self) {
		self.expanded.clear();
		self.rebuild_rows();
	}

	pub fn toggle(&mut self, index: usize) {
		let path = &self.entries[index].path;
		if !self.expanded.remove(path) {
			self.expanded.insert(path.clone());
		}
		self.rebuild_rows();
	}

	/// Open every collapsed ancestor of `path`, and return its entry index.
	pub fn expand_to(&mut self, path: &Path) -> Option<usize> {
		let index = *self.paths.get(path)?;
		let mut parent = path.parent();
		while let Some(path) = parent.filter(|path| *path != self.root) {
			self.expanded.insert(path.to_owned());
			parent = path.parent();
		}
		self.rebuild_rows();
		Some(index)
	}

	pub fn initial(&self) -> Option<PathBuf> {
		self.entries
			.iter()
			.filter(|e| !e.directory && markdown(&e.path))
			.min_by_key(|e| {
				(
					e.path.parent() != Some(self.root.as_path()),
					!e.path
						.file_name()
						.unwrap()
						.eq_ignore_ascii_case("README.md"),
				)
			})
			.map(|e| e.path.clone())
	}

	pub fn resolve(&self, target: &str, current: &Path) -> Option<PathBuf> {
		if target.is_empty() || url::Url::parse(target).is_ok() {
			return None;
		}
		let decoded = percent_encoding::percent_decode_str(target)
			.decode_utf8()
			.ok()?;
		let name = Path::new(decoded.as_ref());
		if name.is_absolute() {
			return None;
		}
		let candidates = [current.join(name), self.root.join(name)];
		for path in candidates {
			let mut suffixed = path.as_os_str().to_owned();
			suffixed.push(".md");
			let paths = [path.clone(), PathBuf::from(suffixed)];
			for candidate in paths {
				if candidate.exists() {
					return Some(candidate.canonicalize().unwrap_or(candidate));
				}
			}
		}
		let with_extension = if markdown(name) {
			name.to_owned()
		} else {
			let mut suffixed = name.as_os_str().to_owned();
			suffixed.push(".md");
			PathBuf::from(suffixed)
		};
		let mut matches = self
			.entries
			.iter()
			.filter(|e| !e.directory && e.path.ends_with(&with_extension));
		let first = matches.next()?;
		matches.next().is_none().then(|| first.path.clone())
	}
}

pub(super) fn markdown(path: &Path) -> bool {
	path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
		["md", "markdown", "mdown"]
			.iter()
			.any(|x| e.eq_ignore_ascii_case(x))
	})
}

#[derive(Default)]
pub(super) struct Browser {
	pub start: super::start::State,
	pub tree: Option<Tree>,
	pub open: bool,
	pub scroll: Scroll,
	pub grab: Option<f32>,
	/// The tree scrollbar's linger or fade after pointer motion.
	scrollbar_reveal: super::viewport::ScrollbarReveal,
	/// The drawer's slide when it opens or closes.
	animation: Option<DrawerAnimation>,
	/// The rows sliding when a directory expands or collapses.
	rows: RowSlide,
	pub labels: std::cell::RefCell<super::chrome::folder::Labels>,
	pub navigation_revision: u64,
	pending_root: Option<PathBuf>,
	scan_navigation_revision: u64,
	generation: Arc<AtomicU64>,
	observer: Option<Observer>,
	revision: u64,
	view_revision: u64,
	pending_reveal: Option<PathBuf>,
}

impl Browser {
	pub fn file_search_available(&self) -> bool {
		self.root().is_some() || self.start.has_files()
	}
	pub(super) fn updating(&self) -> bool {
		self.pending_root.is_some()
			|| self
				.observer
				.as_ref()
				.is_some_and(|observer| observer.pending(self.revision))
	}

	pub(super) fn root(&self) -> Option<&PathBuf> {
		self.tree
			.as_ref()
			.map(|tree| &tree.root)
			.or(self.pending_root.as_ref())
	}

	/// Retargets the drawer's slide between closed and open.
	pub(super) fn animate(&mut self, open: bool) {
		DrawerAnimation::retarget(&mut self.animation, open, self.open);
	}

	/// The drawer's slide reveal at `now`.
	pub(super) fn reveal(&self, now: Instant) -> f32 {
		DrawerAnimation::reveal(&self.animation, self.open, now)
	}

	pub(super) fn scrollbar_opacity_at(&self, now: Instant) -> f32 {
		if self.grab.is_some() {
			1.0
		} else {
			self.scrollbar_reveal.opacity(now)
		}
	}

	pub(super) fn show_scrollbar(&mut self, now: Instant) {
		self.scrollbar_reveal.show(now);
	}

	pub(super) fn hide_scrollbar(&mut self, now: Instant) {
		self.scrollbar_reveal.hide(now);
	}

	pub(super) fn scrollbar_deadline(&self, now: Instant) -> Option<Instant> {
		self.scrollbar_reveal.deadline(now)
	}

	/// Expires the tree bar's linger, or keeps it alive under a drag. Returns
	/// whether paint changed.
	pub(super) fn scrollbar_tick(&mut self, now: Instant) -> bool {
		self.scrollbar_reveal.tick(now, self.grab.is_some())
	}

	pub(super) fn animating(&self) -> bool {
		self.animation.is_some() || self.rows.animating()
	}

	pub(super) fn advance_scrollbar(&mut self, now: Instant) -> bool {
		self.scrollbar_reveal.advance(now)
	}

	pub(super) fn advance_animation(&mut self, now: Instant) {
		if self
			.animation
			.as_ref()
			.is_some_and(|animation| animation.is_finished(now))
		{
			self.animation = None;
		}
		self.rows.advance(now);
	}

	/// Drops the slide where it is, as a page change without animation does.
	pub(super) fn stop_animation(&mut self) {
		self.animation = None;
		self.rows
			.reset(self.tree.as_ref().map_or(&[], |tree| tree.rows()));
	}

	/// Starts the accordion between the tree's rows `from` and its current rows.
	pub(super) fn retarget_rows(&mut self, from: &[usize], now: Instant) {
		let Some(tree) = &self.tree else {
			self.rows.reset(&[]);
			return;
		};
		let from: Vec<_> = from
			.iter()
			.copied()
			.filter(|id| tree.entries.get(*id).is_some())
			.collect();
		self.rows.retarget(&from, tree.rows(), now);
	}

	/// Snaps the rows to a tree that was replaced rather than toggled.
	pub(super) fn reset_rows(&mut self) {
		self.rows
			.reset(self.tree.as_ref().map_or(&[], |tree| tree.rows()));
	}

	/// The rows the list paints: the visible slice at rest, or every moving row.
	pub(super) fn list_rows(
		&self,
		list: &super::chrome::list::List,
		now: Instant,
	) -> Vec<Row> {
		let Some(tree) = &self.tree else {
			return Vec::new();
		};
		self.rows.animated(tree.rows(), now).map_or_else(
			|| {
				list.visible()
					.map(|row| Row {
						index: tree.rows()[row],
						slot: row as f32,
						band: 1.0,
					})
					.collect()
			},
			|rows| {
				rows.into_iter()
					.filter(|row| {
						list.slot_rect(row.slot)
							.intersect(list.viewport)
							.is_some()
					})
					.collect()
			},
		)
	}

	#[cfg(test)]
	pub(super) fn freeze_animation(&mut self, elapsed: std::time::Duration) {
		self.animation
			.as_mut()
			.expect("the folder drawer is animating")
			.freeze(elapsed);
	}

	#[cfg(test)]
	pub(super) fn freeze_rows(&mut self, elapsed: std::time::Duration) {
		self.rows.freeze(elapsed);
	}
}

impl Drop for Browser {
	fn drop(&mut self) {
		self.generation.fetch_add(1, Ordering::Relaxed);
	}
}

impl<P: SendEvent> App<P> {
	pub(super) fn folder_link(
		&self,
		target: &str,
		current: Option<&Path>,
	) -> Option<PathBuf> {
		if !matches!(
			self.readers.session.security.origin,
			crate::security::Origin::Local(_)
		) {
			return None;
		}
		let tree = self.folder.tree.as_ref()?;
		let current = current?;
		if !current.starts_with(&tree.root) {
			return None;
		}
		tree.resolve(target, current)
	}

	pub(super) fn folder_key(&mut self, key: &winit::keyboard::Key) -> bool {
		use winit::keyboard::{Key, NamedKey};
		if (!self.folder.open || self.interaction.panel_open())
			&& !self.phone_folder_open()
			|| self.interaction.overlays.modal_open()
		{
			return false;
		}
		let Some(tree) = &self.folder.tree else {
			return false;
		};
		let rows = tree.rows();
		let focused = self.interaction.widgets.focus.and_then(|id| {
			rows.iter().position(|i| {
				Command::Chrome(ChromeCommand::FolderEntry(*i)).widget_id()
					== id
			})
		});
		let Some(row) = focused else {
			return false;
		};
		let next = match key {
			Key::Named(NamedKey::ArrowDown) => (row + 1).min(rows.len() - 1),
			Key::Named(NamedKey::ArrowUp) => row.saturating_sub(1),
			Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowRight)
				if tree.entries[rows[row]].directory =>
			{
				let collapsed = tree.collapsed(rows[row]);
				if collapsed == matches!(key, Key::Named(NamedKey::ArrowRight))
				{
					self.activate_folder_entry(rows[row]);
				}
				return true;
			}
			_ => return false,
		};
		let index = rows[next];
		self.interaction.widgets.focus = Some(
			Command::Chrome(ChromeCommand::FolderEntry(index)).widget_id(),
		);
		self.reveal_folder(index);
		self.redraw();
		true
	}

	/// Expand an open tree onto the document the reader is showing.
	pub(super) fn expand_folder_to_tab(&mut self) {
		if !(self.folder.open || self.phone_folder_open()) {
			return;
		}
		let Some(path) = self.readers.session.path.clone() else {
			return;
		};
		if self.folder.updating() {
			self.folder.pending_reveal = Some(path);
			return;
		}
		self.folder.pending_reveal = None;
		self.expand_folder_to_path(&path);
	}

	fn replay_folder_reveal(&mut self) {
		if let Some(path) = self.folder.pending_reveal.take()
			&& self.readers.session.path.as_ref() == Some(&path)
			&& (self.folder.open || self.phone_folder_open())
		{
			self.expand_folder_to_path(&path);
		}
	}

	fn expand_folder_to_path(&mut self, path: &Path) {
		let before = self
			.folder
			.tree
			.as_ref()
			.map_or_else(Vec::new, |tree| tree.rows().to_vec());
		let Some(index) =
			self.folder.tree.as_mut().and_then(|t| t.expand_to(path))
		else {
			return;
		};
		self.folder.retarget_rows(&before, Instant::now());
		self.reveal_folder(index);
	}

	pub(super) fn reveal_folder(&mut self, index: usize) {
		let Some(tree) = &self.folder.tree else {
			return;
		};
		let rows = tree.rows();
		let Some(row) = rows.iter().position(|i| *i == index) else {
			return;
		};
		let list = super::chrome::folder::list(
			self.folder_rect(),
			rows.len(),
			self.folder.scroll.offset(),
		);
		let top = row as f32 * 28.;
		self.folder.scroll.set_max(list.max_scroll());
		self.folder.scroll.set(
			list.scroll
				.min(top)
				.max(top + 28. - list.viewport.h)
				.clamp(0., list.max_scroll()),
		);
	}

	pub(super) fn open_folder(&mut self, root: PathBuf, select: bool) {
		self.folder.observer = None;
		self.folder.revision = 0;
		self.folder.view_revision = 0;
		self.folder.pending_reveal = None;
		self.sync_instance_folder(Some(&root));
		self.folder.pending_root = Some(root.clone());
		self.folder.scan_navigation_revision = self.folder.navigation_revision;
		let generation =
			self.folder.generation.fetch_add(1, Ordering::Relaxed) + 1;
		let current = self.folder.generation.clone();
		let proxy = self.proxy.clone();
		std::thread::spawn(move || {
			let result = Tree::read_until(root, || {
				current.load(Ordering::Relaxed) != generation
			});
			if current.load(Ordering::Relaxed) != generation {
				return;
			}
			proxy.send(Event::FolderLoaded {
				generation,
				select,
				result,
			})
		});
	}

	pub(super) fn folder_loaded(
		&mut self,
		generation: u64,
		select: bool,
		result: anyhow::Result<Tree>,
	) {
		if generation != self.folder.generation.load(Ordering::Relaxed) {
			return;
		}
		self.folder.pending_root = None;
		let select = select
			&& self.folder.scan_navigation_revision
				== self.folder.navigation_revision;
		match result {
			Ok(mut tree) => {
				let same_root = self
					.folder
					.tree
					.as_ref()
					.is_some_and(|previous| previous.root == tree.root);
				let focus = self.folder_focus();
				if let Some(previous) = &self.folder.tree
					&& previous.root == tree.root
				{
					tree.expanded = previous
						.expanded
						.iter()
						.filter(|path| {
							tree.paths
								.get(*path)
								.is_some_and(|id| tree.entries[*id].directory)
						})
						.cloned()
						.collect();
					tree.rebuild_rows();
				}
				let initial = tree.initial();
				self.folder
					.start
					.record(tree.root.clone(), tree.name.clone());
				let reveal =
					select || self.folder.open || self.phone_folder_open();
				self.folder.tree = Some(tree);
				self.refresh_start_files();
				self.folder.reset_rows();
				*self.folder.labels.get_mut() = Default::default();
				self.set_folder_open(reveal);
				if !same_root {
					self.folder.scroll.reset();
					self.clear_folder_rows();
				} else if let Some((path, row)) = focus {
					self.restore_folder_focus(&path, row);
				}
				self.folder.grab = None;
				self.interaction.widgets.captured =
					self.interaction.widgets.captured.filter(|id| {
						id.scope
							!= Command::Chrome(ChromeCommand::FolderEntry(0))
								.widget_id()
								.scope
					});
				if reveal && self.folder_inset() == 0. {
					self.interaction.close_outline();
				}
				if select && let Some(path) = initial {
					self.open(path);
				}
				self.set_folder_open(reveal);
				if self.folder.pending_reveal.is_some() {
					self.expand_folder_to_tab();
				}
				self.request(false);
			}
			Err(error) => {
				self.status.set_message(
					self.preferences
						.values
						.lang()
						.folder_error(error.to_string()),
				);
				self.status.set_error(true);
			}
		}
		self.start_folder_observer();
		let root = self.folder.root().cloned();
		self.sync_instance_folder(root.as_deref());
		self.refresh_folder_hover();
		self.redraw();
	}

	fn refresh_folder_hover(&mut self) {
		let focus = self.interaction.widgets.focus.filter(|id| {
			id.scope
				== Command::Chrome(ChromeCommand::FolderEntry(0))
					.widget_id()
					.scope && ((self.folder.open && !self.interaction.panel_open())
				|| self.phone_folder_open())
				&& !self.interaction.overlays.modal_open()
		});
		self.refresh_hover();
		if focus.is_some() {
			self.interaction.widgets.focus = focus;
		}
	}

	fn folder_focus(&self) -> Option<(PathBuf, usize)> {
		let focus = self.interaction.widgets.focus?;
		let tree = self.folder.tree.as_ref()?;
		tree.rows()
			.iter()
			.enumerate()
			.find(|(_, id)| {
				Command::Chrome(ChromeCommand::FolderEntry(**id)).widget_id()
					== focus
			})
			.map(|(row, id)| (tree.entries[*id].path.clone(), row))
	}

	fn restore_folder_focus(&mut self, path: &Path, neighbor: usize) {
		let tree = self.folder.tree.as_ref().unwrap();
		let mut ancestor = Some(path);
		let mut target = None;
		while let Some(path) = ancestor {
			target = tree
				.paths
				.get(path)
				.copied()
				.filter(|id| tree.rows().contains(id));
			if target.is_some() {
				break;
			}
			ancestor = path.parent();
		}
		self.interaction.widgets.focus = target
			.or_else(|| {
				tree.rows()
					.get(neighbor.min(tree.rows().len().saturating_sub(1)))
					.copied()
			})
			.map(|id| {
				Command::Chrome(ChromeCommand::FolderEntry(id)).widget_id()
			});
	}

	fn start_folder_observer(&mut self) {
		if self.folder.observer.is_some() {
			return;
		}
		let Some(tree) = &self.folder.tree else {
			return;
		};
		let root = tree.root.clone();
		let show = tree.show_excluded;
		let generation = self.folder.generation.load(Ordering::Relaxed);
		let proxy = self.proxy.clone();
		let observer = Observer::new(root, move |update| {
			proxy.send(Event::FolderUpdated { generation, update });
		});
		// A pending scan can receive view commands before its observer exists.
		if show || self.folder.view_revision != 0 {
			self.folder.view_revision += 1;
			observer.view(
				self.folder.tree.as_ref().unwrap(),
				self.folder.view_revision,
			);
		}
		self.folder.observer = Some(observer);
	}

	pub(super) fn refresh_folder(&mut self) {
		if let Some(observer) = &self.folder.observer {
			observer.refresh();
		}
	}

	pub(super) fn folder_view_changed(&mut self) {
		self.folder.view_revision += 1;
		if let Some(tree) = &mut self.folder.tree {
			let prune: Vec<_> = tree
				.children
				.keys()
				.filter(|path| {
					*path != &tree.root
						&& path
							.strip_prefix(&tree.root)
							.unwrap()
							.components()
							.any(|part| {
								excluded(Path::new(part.as_os_str()), true)
							}) && (!tree.show_excluded
						|| !tree.expanded.contains(*path))
				})
				.cloned()
				.collect();
			for path in prune {
				tree.patch(&path, Vec::new());
			}
			if let Some(observer) = &self.folder.observer {
				observer.view(tree, self.folder.view_revision);
			}
		}
		self.folder.reset_rows();
	}

	pub(super) fn toggle_folder_excluded(&mut self) {
		let focus = self.folder_focus();
		if let Some(tree) = &mut self.folder.tree {
			tree.show_excluded = !tree.show_excluded;
			if !tree.show_excluded {
				let parents: Vec<_> = tree.children.keys().cloned().collect();
				for path in parents {
					let children = tree
						.children
						.get(&path)
						.into_iter()
						.flatten()
						.filter_map(|id| {
							let entry = &tree.entries[*id];
							(!entry.excluded).then(|| Child {
								path: entry.path.clone(),
								directory: entry.directory,
								excluded: false,
							})
						})
						.collect();
					tree.patch(&path, children);
				}
			}
		}
		self.folder_view_changed();
		self.refresh_start_files();
		self.folder.reset_rows();
		if let Some((path, row)) = focus {
			self.restore_folder_focus(&path, row);
		}
		self.scroll_folder(0.);
		self.refresh_folder_hover();
		self.redraw();
	}

	pub(super) fn folder_updated(
		&mut self,
		generation: u64,
		update: observe::Update,
	) {
		if generation != self.folder.generation.load(Ordering::Relaxed)
			|| update.revision <= self.folder.revision
			|| update.view_revision != self.folder.view_revision
		{
			return;
		}
		let focus = self.folder_focus();
		let Some(tree) = &mut self.folder.tree else {
			return;
		};
		self.folder.revision = update.revision;
		if update.directories.is_empty() && tree.error == update.error {
			if self.folder.pending_reveal.is_some() {
				self.replay_folder_reveal();
				self.redraw();
			}
			return;
		}
		if let Some(error) = &tree.error
			&& update.error.is_none()
			&& self.status.message()
				== self.preferences.values.lang().folder_error(error.clone())
		{
			self.status.clear();
		}
		for (path, children) in update.directories {
			tree.patch(&path, children);
		}
		tree.error = update.error;
		if let Some(error) = &tree.error {
			self.status.set_message(
				self.preferences.values.lang().folder_error(error.clone()),
			);
			self.status.set_error(true);
		}
		self.folder.labels.get_mut().retain(&tree.entries);
		self.refresh_start_files();
		self.folder.reset_rows();
		if let Some((path, row)) = focus {
			self.restore_folder_focus(&path, row);
		}
		let scope = Command::Chrome(ChromeCommand::FolderEntry(0))
			.widget_id()
			.scope;
		if self.interaction.widgets.captured.is_some_and(|id| {
			id.scope == scope
				&& !self.folder.tree.as_ref().unwrap().rows().iter().any(
					|row| {
						Command::Chrome(ChromeCommand::FolderEntry(*row))
							.widget_id() == id
					},
				)
		}) {
			self.interaction.widgets.captured = None;
		}
		let count = self.folder.tree.as_ref().unwrap().rows().len();
		let max = super::chrome::folder::list(
			self.folder_rect(),
			count,
			self.folder.scroll.offset(),
		)
		.max_scroll();
		self.folder.scroll.set_max(max);
		if self.folder.pending_reveal.is_some() {
			self.replay_folder_reveal();
		}
		self.refresh_folder_hover();
		self.redraw();
	}

	pub(super) fn pick_folder(&mut self) {
		if self.dialog_open {
			return;
		}
		self.dialog_open = true;
		let proxy = self.proxy.clone();
		markview_platform::pick_folder(move |path| {
			proxy.send(Event::Picked(path))
		});
	}

	pub(super) fn toggle_folder(&mut self) {
		if self.tab_strip.phone {
			self.set_folder_open(!self.phone_folder_open());
			self.refresh_hover();
			self.redraw();
			return;
		}
		if self.folder.tree.is_none() {
			self.pick_folder();
			return;
		}
		self.set_folder_open(!self.folder.open);
		if self.folder.open && self.folder_inset() == 0. {
			self.interaction.close_outline();
		}
		self.interaction.widgets.captured = None;
		self.request(false);
		self.refresh_hover();
		self.redraw();
	}

	pub(super) fn set_folder_open(&mut self, open: bool) {
		if self.tab_strip.phone {
			self.folder.open = false;
			self.folder.stop_animation();
			if open && !self.phone_folder_open() {
				self.switch_sidebar_tab(crate::state::SidebarTab::Folder);
			} else if !open && self.phone_folder_open() {
				self.show_panel(crate::state::PanelPage::Closed);
			}
			return;
		}
		if open
			&& self.readers.session.search.open
			&& !super::chrome::folder::docked(
				self.dimensions().0,
				self.tab_strip.phone,
			) {
			self.close_search();
		}
		self.folder.animate(open);
		self.folder.open = open;
	}

	pub(super) fn folder_rect(&self) -> crate::layout::Rect {
		let (w, h, _) = self.dimensions();
		if self.tab_strip.phone {
			return super::chrome::tab_drawer::folder_rect(
				super::chrome::tab_drawer::animated_rect(
					w,
					h,
					self.tabs_reveal(),
					self.preferences.values.tabs_width,
				),
			);
		}
		super::chrome::folder::animated_rect(
			super::chrome::folder::rect(
				w,
				h,
				super::TOP,
				super::BOTTOM,
				self.preferences.values.folder_width,
			),
			self.folder.reveal(self.frame_now()),
		)
	}

	pub(super) fn folder_surface(&self) -> crate::layout::Rect {
		super::chrome::folder::surface(self.folder_rect(), self.dimensions().1)
	}

	pub(super) fn folder_chrome_inset(&self) -> f32 {
		if self.tab_strip.phone || self.folder.tree.is_none() {
			return 0.;
		}
		let drawer = self.folder_rect();
		(drawer.x + drawer.w).max(0.)
	}

	pub(super) fn folder_inset(&self) -> f32 {
		if self.folder.open
			&& self.folder.tree.is_some()
			&& super::chrome::folder::docked(
				self.dimensions().0,
				self.tab_strip.phone,
			) {
			self.folder_rect().w
		} else {
			0.
		}
	}

	pub(super) fn reconcile_drawers(&mut self, width: f32) {
		if self.folder.open
			&& (self.interaction.overlays.outline.open()
				|| self.readers.session.search.open)
			&& !super::chrome::folder::docked(width, self.tab_strip.phone)
		{
			// Close immediately so the drawers cannot overlap during a resize.
			self.folder.open = false;
			self.folder.stop_animation();
			self.folder.grab = None;
			self.clear_folder_rows();
		}
	}

	fn clear_folder_rows(&mut self) {
		for slot in [
			&mut self.interaction.widgets.focus,
			&mut self.interaction.widgets.captured,
		] {
			*slot = slot.filter(|id| {
				id.scope
					!= Command::Chrome(ChromeCommand::FolderEntry(0))
						.widget_id()
						.scope
			});
		}
	}

	pub(super) fn pointer_in_folder(&self) -> bool {
		self.folder.reveal(self.frame_now()) > 0.
			&& self.folder.tree.is_some()
			&& !self.interaction.panel_open()
			&& !self.interaction.overlays.modal_open()
			&& self.folder_surface().contains(
				self.interaction.pointer.cursor().0,
				self.interaction.pointer.cursor().1,
			)
	}

	pub(super) fn folder_bar(&self) -> Option<crate::layout::Scrollbar> {
		if self.folder.grab.is_none()
			&& self.folder.scrollbar_opacity_at(self.frame_now()) <= 0.0
		{
			return None;
		}
		let rows = self.folder.tree.as_ref()?.rows().len();
		super::chrome::folder::scrollbar(
			&super::chrome::folder::list(
				self.folder_rect(),
				rows,
				self.folder.scroll.offset(),
			),
			&self.ui,
		)
	}

	pub(super) fn folder_scrollbar_tick(&mut self, now: Instant) {
		if self.folder.scrollbar_tick(now) {
			self.refresh_hover();
			self.redraw();
		}
	}

	pub(super) fn begin_folder_drag(&mut self) -> bool {
		let Some(bar) = self.folder_bar() else {
			return false;
		};
		let (x, y) = self.interaction.pointer.cursor();
		if !bar.hit(x, y) {
			return false;
		}
		// The thumb follows the pointer now, not the ease it interrupted.
		self.folder.scroll.cancel();
		let rows = self
			.folder
			.tree
			.as_ref()
			.map_or(0, |tree| tree.rows().len());
		let max = super::chrome::folder::list(
			self.folder_rect(),
			rows,
			self.folder.scroll.offset(),
		)
		.max_scroll();
		self.folder.scroll.set_max(max);
		let grab = if bar.on_thumb(x, y) {
			bar.grab(x, y)
		} else {
			self.folder.scroll.set(bar.scroll_for(x, y, 0.));
			0.
		};
		self.folder.grab = Some(grab);
		self.redraw();
		true
	}

	pub(super) fn drag_folder(&mut self) {
		if let Some(grab) = self.folder.grab
			&& let Some(bar) = self.folder_bar()
		{
			let (x, y) = self.interaction.pointer.cursor();
			self.folder.scroll.set(bar.scroll_for(x, y, grab));
			self.redraw();
		}
	}

	pub(super) fn scroll_folder(&mut self, delta: f32) {
		self.folder.show_scrollbar(Instant::now());
		let count = self.folder.tree.as_ref().map_or(0, |t| t.rows().len());
		let list = super::chrome::folder::list(
			self.folder_rect(),
			count,
			self.folder.scroll.offset(),
		);
		self.folder.scroll.set_max(list.max_scroll());
		self.folder
			.scroll
			.set((list.scroll + delta).clamp(0., list.max_scroll()));
		self.interaction.widgets.captured = None;
		self.redraw();
	}

	/// A wheel step over the tree, eased like the document's own.
	pub(super) fn wheel_folder(&mut self, delta: f32) {
		let now = Instant::now();
		self.folder.show_scrollbar(now);
		let count = self.folder.tree.as_ref().map_or(0, |t| t.rows().len());
		let list = super::chrome::folder::list(
			self.folder_rect(),
			count,
			self.folder.scroll.offset(),
		);
		self.folder.scroll.set_max(list.max_scroll());
		self.folder.scroll.wheel(
			delta,
			now,
			markview_selection::WheelMode::Detent,
		);
		self.interaction.widgets.captured = None;
		self.redraw();
	}

	pub(super) fn activate_folder_entry(&mut self, index: usize) {
		let Some(tree) = &mut self.folder.tree else {
			return;
		};
		let Some(entry) = tree.entries.get(index) else {
			return;
		};
		if entry.directory {
			let before = tree.rows().to_vec();
			let lazy = tree.show_excluded
				&& tree.entries[index]
					.path
					.strip_prefix(&tree.root)
					.unwrap()
					.components()
					.any(|part| excluded(Path::new(part.as_os_str()), true));
			tree.toggle(index);
			if lazy {
				self.folder_view_changed();
			}
			self.folder.retarget_rows(&before, Instant::now());
			self.scroll_folder(0.);
		} else {
			let path = entry.path.clone();
			if self.folder_inset() == 0. {
				self.set_folder_open(false);
			}
			self.open(path);
		}
		self.redraw();
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn folder_entries_keep_non_unicode_names() {
		let root = PathBuf::from("/folder");
		let image =
			root.join(markview_platform::test_support::non_unicode_name());
		let tree = Tree::scan(
			root.clone(),
			"folder".into(),
			|| false,
			|_, _| Ok(vec![(image.clone(), false)]),
		)
		.unwrap();
		assert_eq!(tree.entries.len(), 1);
		assert_eq!(tree.entries[0].path, image);
		assert!(tree.entries[0].path.to_str().is_none());
	}

	#[test]
	fn existing_directories_take_precedence_over_suffixed_notes() {
		let dir = tempfile::tempdir().unwrap();
		let root = dir.path().canonicalize().unwrap();
		for name in
			["assets", "docs/assets", "docs/asset files", "vault-assets"]
		{
			fs::create_dir_all(root.join(name)).unwrap();
			fs::write(root.join(format!("{name}.md")), "# Note").unwrap();
		}
		let tree = Tree::read(root.clone()).unwrap();
		let current = root.join("docs");
		for (target, expected) in [
			("assets", "docs/assets"),
			("assets/", "docs/assets"),
			("asset%20files", "docs/asset files"),
			("vault-assets", "vault-assets"),
		] {
			let path = tree.resolve(target, &current).unwrap();
			assert_eq!(path, root.join(expected), "{target}");
			assert_eq!(
				crate::link::classify(&path),
				crate::link::Target::OsDirect(path)
			);
		}
		assert_eq!(
			tree.resolve("assets.md", &current),
			Some(current.join("assets.md"))
		);
	}

	#[test]
	fn unreadable_descendants_do_not_discard_accessible_notes() {
		if !markview_platform::test_support::special_files() {
			return;
		}
		let dir = tempfile::tempdir().unwrap();
		let root = dir.path().canonicalize().unwrap();
		let locked = root.join("locked");
		fs::create_dir(&locked).unwrap();
		fs::write(root.join("README.md"), "# Readable").unwrap();
		fs::write(locked.join("Hidden.md"), "# Inaccessible").unwrap();
		let permissions = fs::metadata(&locked).unwrap().permissions();
		markview_platform::test_support::set_mode(&locked, 0o000).unwrap();
		if fs::read_dir(&locked).is_ok() {
			fs::set_permissions(&locked, permissions).unwrap();
			eprintln!(
				"Skipping permission test: process can read mode-000 folders"
			);
			return;
		}
		let tree = Tree::read(root.clone());
		let unreadable_root = Tree::read(locked.clone());
		fs::set_permissions(&locked, permissions).unwrap();
		assert!(unreadable_root.is_err());
		let mut tree = tree.unwrap();
		assert_eq!(tree.initial(), Some(root.join("README.md")));
		assert_eq!(tree.entries.len(), 2);
		assert_eq!(tree.entries[0].path, locked);
		tree.toggle(0);
		assert_eq!(tree.rows(), &[0, 1]);
	}

	#[test]
	fn folded_subtrees_preserve_nested_expansion_and_neighboring_rows() {
		let dir = tempfile::tempdir().unwrap();
		for path in ["a/nested", "a/empty", "b/nested", "empty"] {
			fs::create_dir_all(dir.path().join(path)).unwrap();
		}
		for path in
			["a/nested/One.md", "a/Note.md", "b/nested/Two.md", "Last.md"]
		{
			fs::write(dir.path().join(path), "note").unwrap();
		}
		let mut tree = Tree::read(dir.path().into()).unwrap();
		let rows = |tree: &Tree| {
			tree.rows()
				.iter()
				.map(|i| {
					tree.entries[*i]
						.path
						.strip_prefix(&tree.root)
						.unwrap()
						.to_owned()
				})
				.collect::<Vec<_>>()
		};
		let expected = |paths: &[&str]| {
			paths.iter().map(PathBuf::from).collect::<Vec<_>>()
		};
		assert_eq!(rows(&tree), expected(&["a", "b", "empty", "Last.md"]));
		let a = tree
			.entries
			.iter()
			.position(|e| e.path.ends_with("a"))
			.unwrap();
		let nested = tree
			.entries
			.iter()
			.position(|e| e.path.ends_with("a/nested"))
			.unwrap();
		tree.toggle(a);
		tree.toggle(nested);
		let expanded = rows(&tree);
		assert_eq!(
			expanded,
			expected(&[
				"a",
				"a/empty",
				"a/nested",
				"a/nested/One.md",
				"a/Note.md",
				"b",
				"empty",
				"Last.md"
			])
		);
		tree.toggle(a);
		assert_eq!(rows(&tree), expected(&["a", "b", "empty", "Last.md"]));
		tree.toggle(a);
		assert_eq!(rows(&tree), expanded);
	}

	#[test]
	fn superseded_scans_stop_during_a_directory_read() {
		let dir = tempfile::tempdir().unwrap();
		for index in 0..20 {
			fs::write(dir.path().join(format!("{index}.md")), "note").unwrap();
		}
		let checks = std::cell::Cell::new(0);
		let result = Tree::read_until(dir.path().into(), || {
			checks.set(checks.get() + 1);
			checks.get() >= 5
		});
		assert!(result.is_err());
		assert_eq!(checks.get(), 5);
	}

	#[test]
	fn tree_preserves_hierarchy_collapse_and_unambiguous_vault_paths() {
		let dir = tempfile::tempdir().unwrap();
		let root = dir.path().canonicalize().unwrap();
		for name in ["a", "b", ".obsidian", "empty"] {
			fs::create_dir(root.join(name)).unwrap();
		}
		for name in [
			"README.md",
			"a/First note.md",
			"a/Duplicate.md",
			"b/Duplicate.md",
			"b/Version.1.md",
			".obsidian/hidden.md",
			"image.png",
		] {
			fs::write(root.join(name), "content").unwrap();
		}
		markview_platform::test_support::symlink(&root, root.join("a/loop"))
			.unwrap();
		let mut tree = Tree::read(root.clone()).unwrap();
		assert_eq!(tree.initial(), Some(root.join("README.md")));
		assert_eq!(tree.entries.len(), 9);
		assert_eq!(tree.entries[0].path, root.join("a"));
		assert_eq!(tree.entries[1].depth, 1);
		assert_eq!(tree.rows(), &[0, 3, 6, 7, 8]);
		assert!(tree.collapsed(0));
		tree.toggle(0);
		assert_eq!(tree.rows(), &[0, 1, 2, 3, 6, 7, 8]);
		tree.toggle(0);
		assert_eq!(tree.rows(), &[0, 3, 6, 7, 8]);
		assert_eq!(
			tree.resolve("First%20note", &root.join("b")),
			Some(root.join("a/First note.md"))
		);
		assert_eq!(
			tree.resolve("a/First note", &root.join("b")),
			Some(root.join("a/First note.md"))
		);
		assert_eq!(
			tree.resolve("Version.1", &root),
			Some(root.join("b/Version.1.md"))
		);
		assert_eq!(tree.resolve("Duplicate", &root), None);
		assert_eq!(
			tree.resolve("Duplicate", &root.join("a")),
			Some(root.join("a/Duplicate.md"))
		);
		assert_eq!(tree.resolve("https://example.com", &root), None);
		assert_eq!(Tree::read(root.join("empty")).unwrap().initial(), None);
		assert!(Tree::read(root.join("missing")).is_err());
	}

	#[test]
	fn expand_to_opens_only_the_branch_above_a_path() {
		let dir = tempfile::tempdir().unwrap();
		let root = dir.path().canonicalize().unwrap();
		for name in ["a/nested", "b/nested"] {
			fs::create_dir_all(root.join(name)).unwrap();
		}
		for name in ["a/nested/One.md", "b/nested/Two.md", "Top.md"] {
			fs::write(root.join(name), "note").unwrap();
		}
		let mut tree = Tree::read(root.clone()).unwrap();
		let rows = |tree: &Tree| {
			tree.rows()
				.iter()
				.map(|i| {
					tree.entries[*i]
						.path
						.strip_prefix(&tree.root)
						.unwrap()
						.to_owned()
				})
				.collect::<Vec<_>>()
		};
		let expected = |names: &[&str]| {
			names.iter().map(PathBuf::from).collect::<Vec<_>>()
		};
		// Only the branch above the note opens; siblings keep their state.
		let one = root.join("a/nested/One.md");
		let index = tree.expand_to(&one).unwrap();
		assert!(tree.rows().contains(&index));
		assert_eq!(
			rows(&tree),
			expected(&["a", "a/nested", "a/nested/One.md", "b", "Top.md"])
		);
		assert!(
			tree.collapsed(
				tree.entries
					.iter()
					.position(|e| e.path == root.join("b"))
					.unwrap()
			)
		);
		tree.collapse_all();
		assert_eq!(rows(&tree), expected(&["a", "b", "Top.md"]));
		assert_eq!(tree.expand_to(&root.join("Top.md")), Some(6));
		assert_eq!(rows(&tree), expected(&["a", "b", "Top.md"]));
		assert!(tree.expand_to(&root.join("absent.md")).is_none());
	}
}
