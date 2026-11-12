//! Workspace history and shared asynchronous file-search orchestration.
use super::{
	App, Event, SendEvent,
	file_search::{File, Snippet, find},
};
use markview_interaction::Scroll;
use markview_layout::text_input::TextInput;
use nucleo_matcher::{Config, Matcher};
use serde::{Deserialize, Serialize};
use std::{
	collections::{HashMap, HashSet},
	fs,
	io::Write,
	path::{Path, PathBuf},
	sync::{
		Arc, Condvar, Mutex,
		atomic::{AtomicU64, Ordering},
	},
	time::{Instant, SystemTime, UNIX_EPOCH},
};

const RECENTS: usize = 8;
/// Recently visited documents kept for the start page.
const VISITED: usize = 12;
/// Rows a blank start-page query fills.
const START_FILES: usize = 3;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Recent {
	pub path: PathBuf,
	pub opened: u64,
	#[serde(default)]
	pub name: String,
}
/// One independent query over the shared workspace catalog.
#[derive(Default)]
pub(super) struct Search {
	pub input: TextInput,
	pub matches: Arc<[Arc<File>]>,
	pub snippets: HashMap<usize, Snippet>,
	pub pending: bool,
	pub scroll: Scroll,
	/// Rows that open one specific tab, keyed by their file id.
	tabs: HashMap<usize, usize>,
	catalog: Arc<[Arc<File>]>,
	root: Option<PathBuf>,
	sequence: u64,
	worker: Option<Worker>,
}
impl Search {
	pub fn has_files(&self) -> bool {
		!self.catalog.is_empty()
	}
	/// Whether the query should list local rows instead of searching.
	fn blank_query(&self) -> bool {
		self.input.text().trim().is_empty()
	}
	/// Replaces the results with a list computed on the UI thread, dropping
	/// whatever a worker may still deliver for an earlier query.
	fn set_list(
		&mut self,
		matches: Arc<[Arc<File>]>,
		tabs: HashMap<usize, usize>,
	) {
		if let Some(worker) = &self.worker {
			self.sequence = worker.cancel();
		}
		self.pending = false;
		self.matches = matches;
		self.tabs = tabs;
		self.snippets.clear();
		self.scroll.reset();
	}
	/// Returns whether the catalog changed and a fresh query is required.
	fn apply_catalog(
		&mut self,
		root: Option<PathBuf>,
		catalog: Arc<[Arc<File>]>,
		ui: &mut crate::layout::TextShaper,
	) -> bool {
		let changed_root = self.root != root;
		if changed_root {
			self.root = root;
			self.input.set_text(ui, "");
			self.scroll.reset();
			self.matches = Arc::default();
			self.snippets.clear();
		}
		if !changed_root
			&& self
				.catalog
				.iter()
				.map(|file| (file.id, &file.path))
				.eq(catalog.iter().map(|file| (file.id, &file.path)))
		{
			return false;
		}
		self.catalog = catalog;
		true
	}
}

#[derive(Default)]
pub(super) struct State {
	pub search: Search,
	pub recent: Vec<Recent>,
	/// Recently visited documents, most recent first.
	visited: Vec<PathBuf>,
	history: Option<PathBuf>,
	saved: Arc<AtomicU64>,
	writer: Arc<Mutex<()>>,
}
impl std::ops::Deref for State {
	type Target = Search;
	fn deref(&self) -> &Search {
		&self.search
	}
}
impl std::ops::DerefMut for State {
	fn deref_mut(&mut self) -> &mut Search {
		&mut self.search
	}
}
impl State {
	pub fn visited(&self) -> &[PathBuf] {
		&self.visited
	}
	/// Records `path` as the most recently visited document.
	pub fn visit(&mut self, path: PathBuf) {
		self.visited.retain(|visited| *visited != path);
		self.visited.insert(0, path);
		self.visited.truncate(VISITED);
	}
	pub fn load(path: Option<PathBuf>) -> Self {
		let recent = path
			.as_ref()
			.and_then(|path| match fs::read(path) {
				Ok(bytes) => serde_json::from_slice::<Vec<Recent>>(&bytes).ok(),
				Err(_) => None,
			})
			.unwrap_or_default();
		let mut state = Self {
			history: path,
			..Self::default()
		};
		for mut entry in recent.into_iter().take(RECENTS) {
			if entry.name.is_empty() {
				entry.name = entry
					.path
					.file_name()
					.unwrap_or(entry.path.as_os_str())
					.to_string_lossy()
					.into_owned();
			}
			if !state.recent.iter().any(|saved| saved.path == entry.path) {
				state.recent.push(entry);
			}
		}
		state
	}
	pub fn record(&mut self, path: PathBuf, name: String) {
		self.recent.retain(|recent| recent.path != path);
		self.recent.insert(
			0,
			Recent {
				path,
				name,
				opened: now(),
			},
		);
		self.recent.truncate(RECENTS);
		let Some(path) = self.history.clone() else {
			return;
		};
		let recent = self.recent.clone();
		let saved = self.saved.clone();
		let revision = saved.fetch_add(1, Ordering::Relaxed) + 1;
		let writer = self.writer.clone();
		std::thread::spawn(move || {
			let _write = writer.lock().unwrap();
			if saved.load(Ordering::Relaxed) != revision {
				return;
			}
			if let Err(error) = save_history(&path, &recent) {
				log::warn!("Cannot save workspace history: {error:#}");
			}
		});
	}
}

pub(super) fn now() -> u64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.unwrap_or_default()
		.as_secs()
}
fn save_history(path: &Path, recent: &[Recent]) -> anyhow::Result<()> {
	let parent = path.parent().unwrap();
	fs::create_dir_all(parent)?;
	let mut temp = tempfile::NamedTempFile::new_in(parent)?;
	serde_json::to_writer(&mut temp, recent)?;
	temp.flush()?;
	temp.persist(path)?;
	Ok(())
}

struct Request {
	sequence: u64,
	catalog: Arc<[Arc<File>]>,
	query: String,
}
pub(super) struct Result {
	sequence: u64,
	matches: Arc<[Arc<File>]>,
	snippets: HashMap<usize, Snippet>,
}
type Inbox = crate::mailbox::Inbox<Request>;
struct Worker {
	inbox: Arc<(Mutex<Inbox>, Condvar)>,
	sequence: Arc<AtomicU64>,
	handle: Option<std::thread::JoinHandle<()>>,
}
impl Worker {
	fn new(done: impl Fn(Result) + Send + 'static) -> Self {
		let inbox = Arc::new((Mutex::new(Inbox::default()), Condvar::new()));
		let shared = inbox.clone();
		let sequence = Arc::new(AtomicU64::new(0));
		let latest = sequence.clone();
		let handle = std::thread::Builder::new()
			.name("markview-files".into())
			.spawn(move || {
				let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
				loop {
					let request = {
						let (lock, wake) = &*shared;
						let mut inbox = Inbox::lock(lock);
						while inbox.pending.is_none() && !inbox.stopped {
							inbox = Inbox::recover(lock, wake.wait(inbox));
						}
						if inbox.stopped {
							break;
						}
						inbox.pending.take().unwrap()
					};
					let cancelled =
						|| latest.load(Ordering::Relaxed) != request.sequence;
					if let Some((matches, snippets)) = find(
						&mut matcher,
						&request.catalog,
						&request.query,
						cancelled,
					) && !cancelled()
					{
						done(Result {
							sequence: request.sequence,
							matches,
							snippets,
						});
					}
				}
			})
			.unwrap();
		Self {
			inbox,
			sequence,
			handle: Some(handle),
		}
	}
	fn submit(&self, catalog: Arc<[Arc<File>]>, query: String) -> u64 {
		let sequence = self.sequence.fetch_add(1, Ordering::Relaxed) + 1;
		let (lock, wake) = &*self.inbox;
		Inbox::lock(lock).pending = Some(Request {
			sequence,
			catalog,
			query,
		});
		wake.notify_one();
		sequence
	}
	/// Invalidates in-flight results without queueing another request.
	fn cancel(&self) -> u64 {
		self.sequence.fetch_add(1, Ordering::Relaxed) + 1
	}
}
impl Drop for Worker {
	fn drop(&mut self) {
		self.sequence.fetch_add(1, Ordering::Relaxed);
		let (lock, wake) = &*self.inbox;
		let mut inbox = Inbox::lock(lock);
		inbox.stopped = true;
		inbox.pending = None;
		wake.notify_one();
		drop(inbox);
		self.handle.take().unwrap().join().unwrap();
	}
}

impl<P: SendEvent> App<P> {
	/// The start page's blank-query rows: recently visited documents without
	/// open tabs, then other workspace files that fill the list.
	fn start_files(&self) -> Arc<[Arc<File>]> {
		let catalog = &self.folder.start.search.catalog;
		let open: HashSet<&Path> = self
			.readers
			.entries()
			.iter()
			.map(|tab| tab.path.as_path())
			.collect();
		let mut rows: Vec<Arc<File>> = Vec::with_capacity(START_FILES);
		for path in self.folder.start.visited() {
			if rows.len() == START_FILES {
				break;
			}
			if open.contains(path.as_path())
				|| rows.iter().any(|file| file.path == *path)
			{
				continue;
			}
			if let Some(file) = catalog.iter().find(|file| file.path == *path) {
				rows.push(file.clone());
			}
		}
		for file in catalog.iter() {
			if rows.len() == START_FILES {
				break;
			}
			if !open.contains(file.path.as_path())
				&& !rows.iter().any(|row| row.path == file.path)
			{
				rows.push(file.clone());
			}
		}
		rows.into()
	}
	/// The picker's blank-query rows: every open tab, most recent first, each
	/// opening that exact tab.
	fn picker_files(&self) -> (Arc<[Arc<File>]>, HashMap<usize, usize>) {
		let catalog = &self.interaction.overlays.quick_open.search.catalog;
		let entries = self.readers.entries();
		let mut tabs = HashMap::new();
		let mut rows = Vec::new();
		for index in self.readers.recent_indices() {
			let Some(file) =
				catalog.iter().find(|file| file.path == entries[index].path)
			else {
				continue;
			};
			// Ids beyond the catalog's own range keep same-path tabs apart.
			let id = usize::MAX - index;
			tabs.insert(id, index);
			rows.push(file.with_id(id));
		}
		(rows.into(), tabs)
	}
	pub(super) fn refresh_start_files(&mut self) {
		let tree = self.folder.tree.as_ref();
		let root = tree.map(|tree| tree.root.clone());
		let catalog = super::file_search::catalog(
			tree.into_iter()
				.flat_map(|tree| tree.entries.iter())
				.filter_map(|entry| entry.file.clone()),
			self.readers.local_paths(),
		);
		let start_changed = self.folder.start.apply_catalog(
			root.clone(),
			catalog.clone(),
			&mut self.ui,
		);
		// A blank query lists open tabs and visits, which change without the
		// catalog itself.
		if start_changed || self.folder.start.search.blank_query() {
			self.start_search_changed();
		}
		if self.interaction.overlays.quick_open.open() {
			let picker = &mut self.interaction.overlays.quick_open;
			let changed =
				picker.search.apply_catalog(root, catalog, &mut self.ui);
			if changed || picker.search.blank_query() {
				self.quick_open_search_changed();
			}
		}
	}
	pub(super) fn start_search_changed(&mut self) {
		if self.folder.start.search.blank_query() {
			let rows = self.start_files();
			self.folder.start.search.set_list(rows, HashMap::new());
			self.redraw();
			return;
		}
		let state = &mut self.folder.start.search;
		let worker = state.worker.get_or_insert_with(|| {
			let proxy = self.proxy.clone();
			Worker::new(move |result| proxy.send(Event::FilesReady(result)))
		});
		state.sequence =
			worker.submit(state.catalog.clone(), state.input.text().to_owned());
		state.pending = true;
		state.scroll.reset();
		self.redraw();
	}
	pub(super) fn quick_open_search_changed(&mut self) {
		if self.interaction.overlays.quick_open.search.blank_query() {
			let (rows, tabs) = self.picker_files();
			self.interaction
				.overlays
				.quick_open
				.search
				.set_list(rows, tabs);
			self.interaction.overlays.quick_open.reset();
			self.redraw();
			return;
		}
		let state = &mut self.interaction.overlays.quick_open.search;
		let worker = state.worker.get_or_insert_with(|| {
			let proxy = self.proxy.clone();
			Worker::new(move |result| {
				proxy.send(Event::QuickOpenFilesReady(result))
			})
		});
		state.sequence =
			worker.submit(state.catalog.clone(), state.input.text().to_owned());
		state.pending = true;
		self.interaction.overlays.quick_open.reset();
		self.redraw();
	}
	pub(super) fn quick_open_files_ready(&mut self, result: Result) {
		if result.sequence
			!= self.interaction.overlays.quick_open.search.sequence
		{
			return;
		}
		self.interaction.overlays.quick_open.search.pending = false;
		self.interaction.overlays.quick_open.search.matches = result.matches;
		self.interaction.overlays.quick_open.search.snippets = result.snippets;
		self.interaction.overlays.quick_open.search.tabs.clear();
		self.interaction.overlays.quick_open.reset();
		self.clear_file_capture(true);
		self.refresh_hover();
		self.redraw();
	}
	fn clear_file_capture(&mut self, quick_open: bool) {
		let command = if quick_open {
			crate::state::ChromeCommand::QuickOpenFile(0)
		} else {
			crate::state::ChromeCommand::StartFile(0)
		};
		let scope = crate::state::Command::Chrome(command).widget_id().scope;
		self.interaction.widgets.captured = self
			.interaction
			.widgets
			.captured
			.filter(|id| id.scope != scope);
	}
	pub(super) fn start_files_ready(&mut self, result: Result) {
		if result.sequence != self.folder.start.sequence {
			return;
		}
		self.folder.start.pending = false;
		self.folder.start.matches = result.matches;
		self.folder.start.snippets = result.snippets;
		self.clear_file_capture(false);
		self.refresh_hover();
		self.redraw();
	}
	pub(super) fn open_recent_workspace(&mut self, index: usize) {
		if let Some(recent) = self.folder.start.recent.get(index) {
			self.open_folder(recent.path.clone(), true);
		}
	}
	pub(super) fn open_start_file(&mut self, id: usize) {
		let Some(path) = self
			.folder
			.start
			.matches
			.iter()
			.find(|file| file.id == id)
			.map(|file| file.path.clone())
		else {
			return;
		};
		self.open_start_path(path);
	}
	pub(super) fn open_quick_open_file(&mut self, id: usize) {
		if let Some(index) = self
			.interaction
			.overlays
			.quick_open
			.search
			.tabs
			.get(&id)
			.copied()
		{
			self.select_tab(index);
			return;
		}
		let Some(path) = self
			.interaction
			.overlays
			.quick_open
			.search
			.matches
			.iter()
			.find(|file| file.id == id)
			.map(|file| file.path.clone())
		else {
			return;
		};
		self.open_start_path(path);
	}
	fn open_start_path(&mut self, path: PathBuf) {
		if let Some(index) = self
			.readers
			.entries()
			.iter()
			.position(|tab| tab.path == path)
		{
			self.select_tab(index);
		} else {
			self.open(path);
		}
	}
	pub(super) fn scroll_start(
		&mut self,
		delta: f32,
		mode: markview_interaction::WheelMode,
	) -> bool {
		if self.readers.session.path.is_some()
			|| !self.readers.session.display.snapshot.blocks.is_empty()
		{
			return false;
		}
		let g = self.chrome().start_geometry();
		let scroll = &mut self.folder.start.scroll;
		scroll.set_max(g.max_scroll);
		let before = scroll.offset();
		scroll.wheel(delta, Instant::now(), mode);
		if !scroll.animating() && scroll.offset() == before {
			return true;
		}
		self.clear_input_focus();
		self.interaction.widgets.captured = None;
		self.refresh_hover();
		self.redraw();
		true
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{
		cli::{LaunchOptions, Mode},
		state::{ChromeCommand, Command},
	};
	use std::{
		sync::mpsc,
		time::{Duration, Instant},
	};

	#[derive(Clone)]
	struct Proxy(mpsc::Sender<Event>);
	impl SendEvent for Proxy {
		fn try_send(&self, event: Event) -> bool {
			self.0.send(event).is_ok()
		}
	}
	struct Loop;
	impl crate::app::window::Loop for Loop {
		fn exit(&self) {}
	}
	fn settle(
		app: &mut App<Proxy>,
		events: &mpsc::Receiver<Event>,
		until: impl Fn(&App<Proxy>) -> bool,
	) {
		let deadline = Instant::now() + Duration::from_secs(10);
		while !until(app) {
			let event = events
				.recv_timeout(
					deadline.saturating_duration_since(Instant::now()),
				)
				.expect("start page did not settle");
			app.handle_user_event(&Loop, event);
		}
	}

	#[test]
	fn start_page_and_quick_open_prefer_markdown_in_search_results() {
		let dir = tempfile::tempdir().unwrap();
		let root = dir.path().canonicalize().unwrap();
		for name in ["guide-a.png", "guide-b.md"] {
			fs::write(root.join(name), "# Guide").unwrap();
		}
		let (tx, events) = mpsc::channel();
		let mut app = App::new(
			LaunchOptions {
				mode: Mode::Smoke,
				options: crate::test_support::options(),
				..Default::default()
			},
			Proxy(tx),
		);
		app.ui = crate::test_support::shaper();
		app.folder_loaded(0, false, super::super::folder::Tree::read(root));
		app.folder.start.input.set_text(&mut app.ui, "guide");
		app.start_search_changed();
		app.open_quick_open();
		app.interaction
			.overlays
			.quick_open
			.search
			.input
			.set_text(&mut app.ui, "guide");
		app.quick_open_search_changed();
		settle(&mut app, &events, |app| {
			!app.folder.start.pending
				&& !app.interaction.overlays.quick_open.search.pending
		});
		for search in [
			&app.folder.start.search,
			&app.interaction.overlays.quick_open.search,
		] {
			assert_eq!(search.matches.len(), 2);
			assert_eq!(&*search.matches[0].name, "guide-b.md");
			assert_eq!(&*search.matches[1].name, "guide-a.png");
		}
	}

	#[test]
	fn workspace_search_cancels_old_queries_updates_from_the_tree_and_opens_files()
	 {
		let dir = tempfile::tempdir().unwrap();
		let root = dir.path().canonicalize().unwrap();
		fs::create_dir_all(root.join("docs/笔记")).unwrap();
		fs::create_dir(root.join("node_modules")).unwrap();
		for name in ["README.md", "docs/笔记/阅读.md", "node_modules/hidden.md"]
		{
			fs::write(root.join(name), "# Reading").unwrap();
		}
		let (tx, events) = mpsc::channel();
		let mut app = App::new(
			LaunchOptions {
				mode: Mode::Smoke,
				options: crate::test_support::options(),
				..Default::default()
			},
			Proxy(tx),
		);
		app.ui = crate::test_support::shaper();
		app.folder_loaded(
			0,
			false,
			crate::app::folder::Tree::read(root.clone()),
		);
		settle(&mut app, &events, |app| !app.folder.start.pending);
		assert_eq!(app.folder.start.recent.len(), 1);
		assert_eq!(app.folder.start.matches.len(), 2);
		assert!(app.folder.start.snippets.is_empty());
		app.overlay();
		let first = app.folder.start.matches[0].id;
		app.interaction.widgets.captured =
			Some(Command::Chrome(ChromeCommand::StartFile(first)).widget_id());
		for query in ["no-such-file", "readme", "docs 阅读"] {
			app.folder.start.input.set_text(&mut app.ui, query);
			app.start_search_changed();
		}
		settle(&mut app, &events, |app| !app.folder.start.pending);
		assert_eq!(app.folder.start.matches.len(), 1);
		assert_eq!(&*app.folder.start.matches[0].name, "阅读.md");
		assert!(app.interaction.widgets.captured.is_none());
		app.overlay();
		let controls = app.controls.as_ref().unwrap();
		let files: Vec<_> = controls
			.widgets
			.iter()
			.filter_map(|widget| match widget.message {
				Command::Chrome(ChromeCommand::StartFile(id)) => Some(id),
				_ => None,
			})
			.collect();
		assert_eq!(files, [app.folder.start.matches[0].id]);
		let snippet =
			&app.folder.start.snippets[&app.folder.start.matches[0].id];
		assert_eq!(
			snippet
				.matched
				.iter()
				.map(|range| &snippet.text[range.clone()])
				.collect::<Vec<_>>(),
			["docs", "阅读"]
		);
		let previous = app.folder.start.sequence;
		app.folder.start.input.set_text(&mut app.ui, "README");
		app.start_search_changed();
		app.start_files_ready(Result {
			sequence: previous,
			matches: Arc::default(),
			snippets: HashMap::new(),
		});
		assert!(app.folder.start.pending);
		settle(&mut app, &events, |app| !app.folder.start.pending);
		assert_eq!(&*app.folder.start.matches[0].name, "README.md");
		let snippet =
			&app.folder.start.snippets[&app.folder.start.matches[0].id];
		assert_eq!(snippet.text, "README.md");
		assert_eq!(snippet.matched.len(), 1);
		assert_eq!(snippet.matched[0], 0..6);

		fs::remove_file(root.join("README.md")).unwrap();
		fs::write(root.join("README-new.md"), "# New").unwrap();
		app.refresh_folder();
		settle(&mut app, &events, |app| {
			!app.folder.start.pending
				&& app
					.folder
					.start
					.matches
					.first()
					.is_some_and(|file| &*file.name == "README-new.md")
		});
		let id = app.folder.start.matches[0].id;
		app.action(Command::Chrome(ChromeCommand::StartFile(id)));
		assert_eq!(
			app.readers.session.path.as_ref().unwrap(),
			&root.join("README-new.md")
		);
		assert!(app.input_geometry(crate::state::TextField::Files).is_none());
	}

	#[test]
	fn quick_open_searches_open_tabs_with_and_without_a_workspace() {
		use crate::state::{DocumentCommand, TextField};
		use winit::keyboard::{Key, NamedKey};
		let dir = tempfile::tempdir().unwrap();
		let root = dir.path().canonicalize().unwrap();
		fs::create_dir(root.join("workspace")).unwrap();
		for name in ["outside.md", "another.md", "workspace/inside.md"] {
			fs::write(root.join(name), "# Note").unwrap();
		}
		let (tx, events) = mpsc::channel();
		let mut app = App::new(
			LaunchOptions {
				mode: Mode::Smoke,
				options: crate::test_support::options(),
				..Default::default()
			},
			Proxy(tx),
		);
		app.ui = crate::test_support::shaper();
		app.open(root.join("outside.md"));
		app.open(root.join("another.md"));
		app.action(Command::Document(DocumentCommand::NewPage));
		assert!(app.folder.root().is_none());
		assert!(app.folder.file_search_available());
		assert!(app.input_geometry(TextField::Files).is_none());
		app.folder.start.recent = vec![Recent {
			path: root.join("workspace"),
			name: "workspace".into(),
			opened: now(),
		}];
		let buttons = app.chrome().buttons();
		assert!(buttons.iter().any(|button| {
			button.action == Command::Chrome(ChromeCommand::RecentWorkspace(0))
		}));
		assert!(!buttons.iter().any(|button| {
			matches!(
				button.action,
				Command::Chrome(ChromeCommand::StartFile(_))
			)
		}));
		app.open_quick_open();
		settle(&mut app, &events, |app| {
			!app.interaction.overlays.quick_open.search.pending
		});
		assert_eq!(app.interaction.overlays.quick_open.search.matches.len(), 2);
		app.interaction
			.overlays
			.quick_open
			.search
			.input
			.set_text(&mut app.ui, "outside");
		app.quick_open_search_changed();
		settle(&mut app, &events, |app| {
			!app.interaction.overlays.quick_open.search.pending
		});
		assert_eq!(app.interaction.overlays.quick_open.search.matches.len(), 1);
		app.key_pressed(&Key::Named(NamedKey::Enter));
		assert_eq!(app.readers.session.path, Some(root.join("outside.md")));
		assert_eq!(app.readers.entries().len(), 2);

		app.folder_loaded(
			0,
			false,
			crate::app::folder::Tree::read(root.join("workspace")),
		);
		app.open_quick_open();
		settle(&mut app, &events, |app| {
			!app.interaction.overlays.quick_open.search.pending
		});
		assert_eq!(app.interaction.overlays.quick_open.search.catalog.len(), 3);
		for query in ["inside", "outside", "outside"] {
			app.interaction
				.overlays
				.quick_open
				.search
				.input
				.set_text(&mut app.ui, query);
			app.quick_open_search_changed();
			settle(&mut app, &events, |app| {
				!app.interaction.overlays.quick_open.search.pending
			});
			assert_eq!(
				app.interaction.overlays.quick_open.search.matches.len(),
				1
			);
			let path = app.interaction.overlays.quick_open.search.matches[0]
				.path
				.clone();
			app.key_pressed(&Key::Named(NamedKey::Enter));
			assert_eq!(app.readers.session.path, Some(path));
			assert_eq!(app.readers.entries().len(), 3);
			app.open_quick_open();
			settle(&mut app, &events, |app| {
				!app.interaction.overlays.quick_open.search.pending
			});
			assert_eq!(
				app.interaction.overlays.quick_open.search.catalog.len(),
				3
			);
		}
		app.close_quick_open();
		let outside = app.readers.find(&root.join("outside.md")).unwrap();
		app.close_tab(outside);
		app.open_quick_open();
		settle(&mut app, &events, |app| {
			!app.interaction.overlays.quick_open.search.pending
		});
		// A blank picker lists the open tabs instead of searching.
		assert_eq!(app.interaction.overlays.quick_open.search.matches.len(), 2);
		assert_eq!(
			app.interaction.overlays.quick_open.search.matches[0].path,
			root.join("workspace/inside.md")
		);
		assert_eq!(app.interaction.overlays.quick_open.search.catalog.len(), 2);
	}

	#[test]
	fn a_web_tab_is_never_a_start_page_or_quick_open_row() {
		let dir = tempfile::tempdir().unwrap();
		let root = dir.path().canonicalize().unwrap();
		fs::write(root.join("local.md"), "# Local").unwrap();
		fs::write(root.join("open.md"), "# Open").unwrap();
		let (tx, events) = mpsc::channel();
		let mut app = App::new(
			LaunchOptions {
				mode: Mode::Smoke,
				offline: true,
				options: crate::test_support::options(),
				..Default::default()
			},
			Proxy(tx),
		);
		app.ui = crate::test_support::shaper();
		app.folder_loaded(
			0,
			false,
			crate::app::folder::Tree::read(root.clone()),
		);
		app.open(root.join("open.md"));
		let url = "https://example.org/blog/post";
		app.open_web_page(url.to_owned());
		let web_path = crate::app::web_page::web_path(url);
		assert!(app.readers.entries().iter().any(|tab| tab.path == web_path));
		assert!(
			app.folder
				.start
				.catalog
				.iter()
				.all(|file| file.path != web_path),
			"a web tab must be excluded before asynchronous folder updates"
		);
		settle(&mut app, &events, |app| !app.folder.start.pending);
		assert!(
			app.folder
				.start
				.matches
				.iter()
				.any(|file| &*file.name == "local.md")
		);
		assert!(
			app.folder
				.start
				.matches
				.iter()
				.all(|file| !file.name.contains("web:")),
			"{:?}",
			app.folder
				.start
				.matches
				.iter()
				.map(|file| &*file.name)
				.collect::<Vec<_>>()
		);
		app.open_quick_open();
		settle(&mut app, &events, |app| {
			!app.interaction.overlays.quick_open.search.pending
		});
		assert!(
			app.interaction
				.overlays
				.quick_open
				.search
				.catalog
				.iter()
				.all(|file| file.path != web_path)
		);
		assert!(
			app.interaction
				.overlays
				.quick_open
				.search
				.matches
				.iter()
				.all(|file| !file.name.contains("web:")),
		);
	}

	#[test]
	fn quick_open_opens_a_file_absent_from_the_start_page_results() {
		use winit::keyboard::{Key, NamedKey};
		let dir = tempfile::tempdir().unwrap();
		let root = dir.path().canonicalize().unwrap();
		for index in 0..20 {
			fs::write(root.join(format!("note-{index:02}.md")), "# Note")
				.unwrap();
		}
		let (tx, events) = mpsc::channel();
		let mut app = App::new(
			LaunchOptions {
				mode: Mode::Smoke,
				options: crate::test_support::options(),
				..Default::default()
			},
			Proxy(tx),
		);
		app.ui = crate::test_support::shaper();
		app.folder_loaded(
			0,
			false,
			crate::app::folder::Tree::read(root.clone()),
		);
		settle(&mut app, &events, |app| !app.folder.start.pending);
		assert_eq!(app.folder.start.matches.len(), 3);
		app.open_quick_open();
		app.interaction
			.overlays
			.quick_open
			.search
			.input
			.set_text(&mut app.ui, "note-15");
		app.quick_open_search_changed();
		settle(&mut app, &events, |app| {
			!app.interaction.overlays.quick_open.search.pending
		});
		let id = app.interaction.overlays.quick_open.search.matches[0].id;
		assert!(app.folder.start.matches.iter().all(|file| file.id != id));
		assert!(app.key_pressed(&Key::Named(NamedKey::Enter)));
		assert_eq!(
			app.readers.session.path.as_ref().unwrap(),
			&root.join("note-15.md")
		);
	}

	#[test]
	fn start_page_lists_recent_visits_before_unused_files_and_skips_open_tabs()
	{
		use crate::state::DocumentCommand;
		let dir = tempfile::tempdir().unwrap();
		let root = dir.path().canonicalize().unwrap();
		for index in 0..6 {
			fs::write(root.join(format!("note-{index}.md")), "# Note").unwrap();
		}
		let (tx, events) = mpsc::channel();
		let mut app = App::new(
			LaunchOptions {
				mode: Mode::Smoke,
				options: crate::test_support::options(),
				..Default::default()
			},
			Proxy(tx),
		);
		app.ui = crate::test_support::shaper();
		app.folder_loaded(
			0,
			false,
			crate::app::folder::Tree::read(root.clone()),
		);
		settle(&mut app, &events, |app| !app.folder.start.pending);
		// Nothing visited yet, so workspace files fill the three rows.
		assert_eq!(app.folder.start.matches.len(), 3);

		app.open(root.join("note-3.md"));
		app.open(root.join("note-1.md"));
		app.action(Command::Document(DocumentCommand::NewPage));
		// Both visits are open tabs, so unused files fill the rows instead.
		assert_eq!(app.folder.start.matches.len(), 3);
		assert!(
			app.folder
				.start
				.matches
				.iter()
				.all(|file| file.path != root.join("note-1.md")
					&& file.path != root.join("note-3.md"))
		);

		let older = app.readers.find(&root.join("note-1.md")).unwrap();
		app.close_tab(older);
		// The most recent visit that is no longer a tab leads the list.
		assert_eq!(app.folder.start.matches[0].path, root.join("note-1.md"));
		assert_eq!(app.folder.start.matches.len(), 3);
	}

	#[test]
	fn start_page_records_visits_from_tab_switches() {
		let dir = tempfile::tempdir().unwrap();
		let root = dir.path().canonicalize().unwrap();
		for index in 0..3 {
			fs::write(root.join(format!("note-{index}.md")), "# Note").unwrap();
		}
		let (tx, events) = mpsc::channel();
		let mut app = App::new(
			LaunchOptions {
				mode: Mode::Smoke,
				options: crate::test_support::options(),
				..Default::default()
			},
			Proxy(tx),
		);
		app.ui = crate::test_support::shaper();
		app.folder_loaded(
			0,
			false,
			crate::app::folder::Tree::read(root.clone()),
		);
		settle(&mut app, &events, |app| !app.folder.start.pending);
		app.open(root.join("note-0.md"));
		app.open(root.join("note-1.md"));
		let first = app.readers.find(&root.join("note-0.md")).unwrap();
		app.select_tab(first);
		let second = app.readers.find(&root.join("note-1.md")).unwrap();
		app.close_tab(second);
		app.close_tab(first);
		assert!(app.readers.entries().is_empty());
		// Returning to `note-0.md` made it the most recent visit.
		assert_eq!(app.folder.start.matches[0].path, root.join("note-0.md"));
		assert_eq!(app.folder.start.matches[1].path, root.join("note-1.md"));
	}

	#[test]
	fn start_page_matches_a_visit_opened_with_dotdot() {
		use crate::state::DocumentCommand;
		let dir = tempfile::tempdir().unwrap();
		let root = dir.path().canonicalize().unwrap();
		fs::create_dir(root.join("docs")).unwrap();
		fs::write(root.join("notes.md"), "# Note").unwrap();
		let (tx, events) = mpsc::channel();
		let mut app = App::new(
			LaunchOptions {
				mode: Mode::Smoke,
				options: crate::test_support::options(),
				..Default::default()
			},
			Proxy(tx),
		);
		app.ui = crate::test_support::shaper();
		app.folder_loaded(
			0,
			false,
			crate::app::folder::Tree::read(root.clone()),
		);
		settle(&mut app, &events, |app| !app.folder.start.pending);
		app.open(root.join("docs/../notes.md"));
		app.action(Command::Document(DocumentCommand::NewPage));
		// The visited file is the open tab, so no row is left.
		assert!(app.folder.start.matches.is_empty());
		app.close_tab(0);
		assert_eq!(app.folder.start.matches[0].path, root.join("notes.md"));
	}

	#[test]
	fn start_page_records_the_document_a_close_activates() {
		let dir = tempfile::tempdir().unwrap();
		let root = dir.path().canonicalize().unwrap();
		for index in 0..3 {
			fs::write(root.join(format!("note-{index}.md")), "# Note").unwrap();
		}
		let (tx, events) = mpsc::channel();
		let mut app = App::new(
			LaunchOptions {
				mode: Mode::Smoke,
				options: crate::test_support::options(),
				..Default::default()
			},
			Proxy(tx),
		);
		app.ui = crate::test_support::shaper();
		app.folder_loaded(
			0,
			false,
			crate::app::folder::Tree::read(root.clone()),
		);
		settle(&mut app, &events, |app| !app.folder.start.pending);
		app.open(root.join("note-0.md"));
		app.open(root.join("note-1.md"));
		let second = app.readers.find(&root.join("note-1.md")).unwrap();
		app.close_tab(second);
		let first = app.readers.find(&root.join("note-0.md")).unwrap();
		app.close_tab(first);
		// `note-0.md` was on screen last, after the close selected it.
		assert_eq!(app.folder.start.matches[0].path, root.join("note-0.md"));
		assert_eq!(app.folder.start.matches[1].path, root.join("note-1.md"));
	}

	#[test]
	fn quick_open_lists_same_path_tabs_separately_and_opens_each() {
		use crate::{
			security::{Security, Trust},
			state::DocumentCommand,
		};
		let dir = tempfile::tempdir().unwrap();
		let root = dir.path().canonicalize().unwrap();
		fs::write(root.join("shared.md"), "# Note").unwrap();
		let (tx, events) = mpsc::channel();
		let mut app = App::new(
			LaunchOptions {
				mode: Mode::Smoke,
				options: crate::test_support::options(),
				..Default::default()
			},
			Proxy(tx),
		);
		app.ui = crate::test_support::shaper();
		app.folder_loaded(
			0,
			false,
			crate::app::folder::Tree::read(root.clone()),
		);
		settle(&mut app, &events, |app| !app.folder.start.pending);
		app.open(root.join("shared.md"));
		assert!(app.readers.open_background_secure(
			root.join("shared.md"),
			None,
			Security::local(Trust::Untrusted),
		));
		app.action(Command::Document(DocumentCommand::NewPage));
		app.open_quick_open();
		let search = &app.interaction.overlays.quick_open.search;
		assert_eq!(search.matches.len(), 2);
		assert_ne!(search.matches[0].id, search.matches[1].id);
		let untrusted = search
			.tabs
			.iter()
			.find_map(|(id, index)| (*index == 1).then_some(*id))
			.expect("the second tab needs its own row");
		app.open_quick_open_file(untrusted);
		assert_eq!(
			app.readers.session.security.origin,
			Security::local(Trust::Untrusted).origin
		);
	}

	#[test]
	fn history_keeps_a_bounded_unique_recent_order_and_survives_restart() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("workspaces.json");
		let mut state = State::load(Some(path.clone()));
		for index in 0..12 {
			state.record(
				dir.path().join(format!("workspace-{index}")),
				format!("Workspace {index}"),
			);
		}
		state.record(dir.path().join("workspace-9"), "Workspace 9".into());
		let deadline = Instant::now() + Duration::from_secs(5);
		loop {
			let restored = State::load(Some(path.clone()));
			if restored.recent.len() == RECENTS
				&& restored.recent[0].name == "Workspace 9"
			{
				assert_eq!(restored.recent[1].name, "Workspace 11");
				assert_eq!(
					restored
						.recent
						.iter()
						.filter(|r| r.name == "Workspace 9")
						.count(),
					1
				);
				break;
			}
			assert!(
				Instant::now() < deadline,
				"workspace history was not saved"
			);
			std::thread::sleep(Duration::from_millis(10));
		}
	}

	#[test]
	fn quick_open_searches_independently_scrolls_selection_and_opens_a_file() {
		use crate::state::TextField;
		use winit::keyboard::{Key, ModifiersState, NamedKey};
		let dir = tempfile::tempdir().unwrap();
		let root = dir.path().canonicalize().unwrap();
		for index in 0..20 {
			fs::write(root.join(format!("note-{index:02}.md")), "# Note")
				.unwrap();
		}
		let (tx, events) = mpsc::channel();
		let mut app = App::new(
			LaunchOptions {
				mode: Mode::Smoke,
				options: crate::test_support::options(),
				..Default::default()
			},
			Proxy(tx),
		);
		app.ui = crate::test_support::shaper();
		app.folder_loaded(
			0,
			false,
			crate::app::folder::Tree::read(root.clone()),
		);
		settle(&mut app, &events, |app| !app.folder.start.pending);
		app.folder.start.input.set_text(&mut app.ui, "note");
		app.start_search_changed();
		settle(&mut app, &events, |app| !app.folder.start.pending);
		let matches = app.folder.start.matches.clone();
		app.open_start_file(matches[0].id);
		app.interaction.pointer.set_modifiers(
			if markview_platform::command_key() {
				ModifiersState::META
			} else {
				ModifiersState::CONTROL
			},
		);
		assert!(app.quick_open_shortcut(&Key::Character("p".into())));
		app.interaction
			.pointer
			.set_modifiers(ModifiersState::empty());
		settle(&mut app, &events, |app| {
			!app.interaction.overlays.quick_open.search.pending
		});
		assert_eq!(app.focused_input(), Some(TextField::QuickOpen));
		assert_eq!(app.folder.start.input.text(), "note");
		assert!(Arc::ptr_eq(&matches, &app.folder.start.matches));
		assert_eq!(app.folder.start.snippets.len(), 20);
		assert_eq!(app.interaction.overlays.quick_open.search.input.text(), "");
		// The blank picker lists the open tabs.
		assert_eq!(app.interaction.overlays.quick_open.search.matches.len(), 1);
		assert_eq!(
			app.interaction.overlays.quick_open.search.matches[0].path,
			root.join("note-00.md")
		);
		app.interaction
			.overlays
			.quick_open
			.search
			.input
			.set_text(&mut app.ui, "note");
		app.quick_open_search_changed();
		settle(&mut app, &events, |app| {
			!app.interaction.overlays.quick_open.search.pending
		});
		assert_eq!(
			app.interaction.overlays.quick_open.search.matches.len(),
			20
		);
		assert!(app.quick_open_scrollbar().is_some());
		assert!(app.input_geometry(TextField::QuickOpen).is_some());
		assert!(app.input_geometry(TextField::Files).is_none());
		assert!(app.input_geometry(TextField::Url).is_none());
		let query = app
			.interaction
			.overlays
			.quick_open
			.search
			.input
			.text()
			.to_owned();
		for _ in 0..12 {
			assert!(app.key_pressed(&Key::Named(NamedKey::ArrowDown)));
		}
		assert_eq!(app.interaction.overlays.quick_open.selected(), 12);
		assert!(app.interaction.overlays.quick_open.scroll() > 0.0);
		assert_eq!(
			app.interaction.overlays.quick_open.search.input.text(),
			query
		);
		assert_eq!(app.folder.start.input.text(), "note");
		assert_eq!(app.folder.start.matches.len(), 20);
		let id = app.interaction.overlays.quick_open.search.matches[12].id;
		app.overlay();
		let rows = &app.controls.as_ref().unwrap().widgets;
		assert!(rows.iter().any(|widget| widget.message
			== Command::Chrome(ChromeCommand::QuickOpenFile(id))));
		assert!(rows.iter().all(|widget| matches!(
			widget.message,
			Command::Chrome(
				ChromeCommand::QuickOpenFile(_)
					| ChromeCommand::FocusInput(TextField::QuickOpen)
			)
		)));
		assert!(app.key_pressed(&Key::Named(NamedKey::Enter)));
		assert_eq!(
			app.readers.session.path.as_ref().unwrap(),
			&root.join("note-12.md")
		);
		assert!(!app.interaction.overlays.quick_open.open());
		assert!(app.input_geometry(TextField::QuickOpen).is_none());
		assert!(app.input_geometry(TextField::Files).is_none());
		assert_eq!(app.folder.start.input.text(), "note");

		app.open_quick_open();
		let opened = app.readers.session.path.clone();
		app.interaction
			.overlays
			.quick_open
			.search
			.input
			.set_text(&mut app.ui, "no-such-note");
		app.quick_open_search_changed();
		app.key_pressed(&Key::Named(NamedKey::Enter));
		assert_eq!(app.readers.session.path, opened);
		assert!(app.interaction.overlays.quick_open.open());
		settle(&mut app, &events, |app| {
			!app.interaction.overlays.quick_open.search.pending
		});
		assert!(
			app.interaction
				.overlays
				.quick_open
				.search
				.matches
				.is_empty()
		);
		assert_eq!(app.interaction.overlays.quick_open.selected(), 0);
		assert_eq!(app.folder.start.input.text(), "note");
		assert_eq!(app.folder.start.matches.len(), 20);
		app.key_pressed(&Key::Named(NamedKey::Enter));
		assert!(app.interaction.overlays.quick_open.open());
		app.key_pressed(&Key::Named(NamedKey::Escape));
		assert!(!app.interaction.overlays.quick_open.open());
		assert_eq!(app.readers.session.path, opened);

		app.open_quick_open();
		app.interaction.pointer.move_to((5.0, 300.0));
		app.pointer_pressed();
		assert!(!app.interaction.overlays.quick_open.open());
		assert!(app.interaction.widgets.captured.is_none());
		app.pointer_released();
		assert_eq!(app.readers.session.path, opened);
	}
}
