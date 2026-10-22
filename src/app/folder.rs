//! A read-only folder tree shared by native entry points and the sidebar.
use super::{App, Event, SendEvent};
use crate::{
	scroll::ScrollArea,
	state::{Command, DrawerAnimation},
};
use std::{
	collections::HashSet,
	fs,
	path::{Path, PathBuf},
	sync::{
		Arc,
		atomic::{AtomicU64, Ordering},
	},
	time::Instant,
};

#[derive(Debug)]
pub(super) struct Entry {
	pub path: PathBuf,
	pub depth: usize,
	pub directory: bool,
	/// The first entry after this directory's descendants.
	subtree_end: usize,
}

#[derive(Debug)]
pub(super) struct Tree {
	pub root: PathBuf,
	pub name: String,
	pub entries: Vec<Entry>,
	expanded: HashSet<PathBuf>,
	root_collapsed: bool,
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
		let mut entries = Vec::new();
		let mut pending = vec![(root.clone(), 0)];
		while let Some((path, depth)) = pending.pop() {
			anyhow::ensure!(!cancelled(), "Folder scan cancelled");
			let directory = path.is_dir();
			if path != root {
				entries.push(Entry {
					directory,
					path: path.clone(),
					depth: depth - 1,
					subtree_end: 0,
				});
			}
			if !directory {
				continue;
			}
			let mut children = Vec::new();
			let directory = match fs::read_dir(&path) {
				Ok(directory) => directory,
				Err(error) if path == root => return Err(error.into()),
				Err(error) => {
					log::warn!(
						"Cannot read folder {}: {error}",
						path.display()
					);
					continue;
				}
			};
			for child in directory.flatten() {
				anyhow::ensure!(!cancelled(), "Folder scan cancelled");
				let Ok(kind) = child.file_type() else {
					continue;
				};
				if child.file_name().to_string_lossy().starts_with('.') {
					continue;
				}
				if kind.is_dir() || (kind.is_file() && markdown(&child.path()))
				{
					children.push(Entry {
						path: child.path(),
						depth,
						directory: kind.is_dir(),
						subtree_end: 0,
					});
				}
			}
			children.sort_by_key(|e| {
				(
					!e.directory,
					e.path
						.file_name()
						.unwrap()
						.to_string_lossy()
						.to_lowercase(),
					e.path.clone(),
				)
			});
			// A directory's descendants follow it in the flat tree.
			for child in children.into_iter().rev() {
				pending.push((child.path, child.depth + 1));
			}
		}
		let mut tree = Self {
			root,
			name,
			entries,
			expanded: HashSet::new(),
			root_collapsed: false,
			visible: Vec::new(),
		};
		tree.index_subtrees();
		tree.rebuild_rows();
		Ok(tree)
	}

	pub fn rows(&self) -> &[usize] {
		if self.root_collapsed() {
			&[]
		} else {
			&self.visible
		}
	}

	fn index_subtrees(&mut self) {
		let mut parents = Vec::<usize>::new();
		for index in 0..self.entries.len() {
			while parents.last().is_some_and(|parent| {
				self.entries[*parent].depth >= self.entries[index].depth
			}) {
				self.entries[parents.pop().unwrap()].subtree_end = index;
			}
			if self.entries[index].directory {
				parents.push(index);
			}
		}
		for parent in parents {
			self.entries[parent].subtree_end = self.entries.len();
		}
	}

	fn rebuild_rows(&mut self) {
		self.visible.clear();
		let mut index = 0;
		while let Some(entry) = self.entries.get(index) {
			self.visible.push(index);
			index = if self.collapsed(index) {
				entry.subtree_end
			} else {
				index + 1
			};
		}
	}

	pub fn collapsed(&self, index: usize) -> bool {
		self.entries[index].directory
			&& !self.expanded.contains(&self.entries[index].path)
	}

	pub fn root_collapsed(&self) -> bool {
		self.root_collapsed
	}

	pub fn toggle_root(&mut self) {
		self.root_collapsed = !self.root_collapsed;
	}

	pub fn collapse_all(&mut self) {
		self.expanded.clear();
		self.root_collapsed = true;
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
		let index = self.entries.iter().position(|e| e.path == path)?;
		let depth = self.entries[index].depth;
		self.expanded.extend(
			(0..index)
				.filter(|j| {
					self.entries[*j].directory
						&& self.entries[*j].depth < depth
						&& index < self.entries[*j].subtree_end
				})
				.map(|j| self.entries[j].path.clone()),
		);
		self.root_collapsed = false;
		self.rebuild_rows();
		Some(index)
	}

	pub fn initial(&self) -> Option<PathBuf> {
		self.entries
			.iter()
			.filter(|e| !e.directory)
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
	pub tree: Option<Tree>,
	pub open: bool,
	pub scroll: ScrollArea,
	pub grab: Option<f32>,
	/// The drawer's slide when it opens or closes.
	animation: Option<DrawerAnimation>,
	pub labels: std::cell::RefCell<super::chrome::folder::Labels>,
	pub navigation_revision: u64,
	pending_root: Option<PathBuf>,
	scan_navigation_revision: u64,
	generation: Arc<AtomicU64>,
}

impl Browser {
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
	pub(super) fn reveal(&self) -> f32 {
		DrawerAnimation::reveal(&self.animation, self.open, Instant::now())
	}

	pub(super) fn animating(&self) -> bool {
		self.animation.is_some()
	}

	pub(super) fn advance_animation(&mut self, now: Instant) {
		if self
			.animation
			.as_ref()
			.is_some_and(|animation| animation.is_finished(now))
		{
			self.animation = None;
		}
	}

	/// Drops the slide where it is, as a page change without animation does.
	pub(super) fn stop_animation(&mut self) {
		self.animation = None;
	}

	#[cfg(test)]
	pub(super) fn freeze_animation(&mut self, elapsed: std::time::Duration) {
		self.animation
			.as_mut()
			.expect("the folder drawer is animating")
			.freeze(elapsed);
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
			|| self.interaction.modal.is_some()
		{
			return false;
		}
		let Some(tree) = &self.folder.tree else {
			return false;
		};
		if self.interaction.widgets.focus
			== Some(Command::FolderRoot.widget_id())
		{
			return self.folder_root_key(key);
		}
		let rows = tree.rows();
		let focused = self.interaction.widgets.focus.and_then(|id| {
			rows.iter()
				.position(|i| Command::FolderEntry(*i).widget_id() == id)
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
		self.interaction.widgets.focus =
			Some(Command::FolderEntry(index).widget_id());
		self.reveal_folder(index);
		self.redraw();
		true
	}

	fn folder_root_key(&mut self, key: &winit::keyboard::Key) -> bool {
		use winit::keyboard::{Key, NamedKey};
		let tree = self.folder.tree.as_ref().unwrap();
		match key {
			Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowRight) => {
				if tree.root_collapsed()
					== matches!(key, Key::Named(NamedKey::ArrowRight))
				{
					self.action(Command::FolderRoot);
				}
			}
			Key::Named(NamedKey::ArrowDown) => {
				if let Some(index) = tree.rows().first() {
					self.interaction.widgets.focus =
						Some(Command::FolderEntry(*index).widget_id());
					self.reveal_folder(*index);
				}
			}
			_ => return false,
		}
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
		let Some(index) =
			self.folder.tree.as_mut().and_then(|t| t.expand_to(&path))
		else {
			return;
		};
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
				if let Some(previous) = &self.folder.tree
					&& previous.root == tree.root
				{
					tree.expanded = previous.expanded.clone();
					tree.root_collapsed = previous.root_collapsed;
					tree.rebuild_rows();
				}
				let initial = tree.initial();
				let reveal =
					select || self.folder.open || self.phone_folder_open();
				self.folder.tree = Some(tree);
				*self.folder.labels.get_mut() = Default::default();
				self.set_folder_open(reveal);
				self.folder.scroll.reset();
				self.folder.grab = None;
				self.clear_folder_rows();
				if reveal && self.folder_inset() == 0. {
					self.interaction.close_outline();
				}
				if select && let Some(path) = initial {
					self.open(path);
				}
				self.set_folder_open(reveal);
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
		self.refresh_hover();
		self.redraw();
	}

	pub(super) fn pick_folder(&mut self) {
		if self.dialog_open {
			return;
		}
		self.dialog_open = true;
		let proxy = self.proxy.clone();
		markview_platform::pick_folder(move |path| {
			proxy.send(Event::Open(path))
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
			self.folder.reveal(),
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
				*id != Command::FolderRoot.widget_id()
					&& id.scope != Command::FolderEntry(0).widget_id().scope
			});
		}
	}

	pub(super) fn pointer_in_folder(&self) -> bool {
		self.folder.reveal() > 0.
			&& self.folder.tree.is_some()
			&& !self.interaction.panel_open()
			&& self.interaction.modal.is_none()
			&& self.folder_surface().contains(
				self.interaction.pointer.cursor().0,
				self.interaction.pointer.cursor().1,
			)
	}

	pub(super) fn folder_bar(&self) -> Option<crate::layout::Scrollbar> {
		let rows = self.folder.tree.as_ref()?.rows().len();
		super::chrome::folder::list(
			self.folder_rect(),
			rows,
			self.folder.scroll.offset(),
		)
		.scrollbar(&self.ui)
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
		let count = self.folder.tree.as_ref().map_or(0, |t| t.rows().len());
		let list = super::chrome::folder::list(
			self.folder_rect(),
			count,
			self.folder.scroll.offset(),
		);
		self.folder.scroll.set_max(list.max_scroll());
		self.folder.scroll.wheel(delta, Instant::now());
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
			tree.toggle(index);
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
		assert_eq!(tree.entries.len(), 8);
		assert_eq!(tree.entries[0].path, root.join("a"));
		assert_eq!(tree.entries[1].depth, 1);
		assert_eq!(tree.rows(), &[0, 3, 6, 7]);
		assert!(tree.collapsed(0));
		tree.toggle(0);
		assert_eq!(tree.rows(), &[0, 1, 2, 3, 6, 7]);
		tree.toggle(0);
		assert_eq!(tree.rows(), &[0, 3, 6, 7]);
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
		assert!(tree.rows().is_empty());
		// A collapsed root reopens for the note it has to show.
		assert_eq!(tree.expand_to(&root.join("Top.md")), Some(6));
		assert!(!tree.root_collapsed());
		assert_eq!(rows(&tree), expected(&["a", "b", "Top.md"]));
		assert!(tree.expand_to(&root.join("absent.md")).is_none());
	}
}
