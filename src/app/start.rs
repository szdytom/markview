//! Workspace history and shared asynchronous file-search orchestration.
use super::{
	App, Event, SendEvent,
	file_search::{File, Snippet, find},
};
use markview_layout::text_input::TextInput;
use markview_selection::Scroll;
use nucleo_matcher::{Config, Matcher};
use serde::{Deserialize, Serialize};
use std::{
	collections::HashMap,
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
	catalog: Arc<[Arc<File>]>,
	root: Option<PathBuf>,
	sequence: u64,
	worker: Option<Worker>,
}
impl Search {
	pub fn has_files(&self) -> bool {
		!self.catalog.is_empty()
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
	pub(super) fn refresh_start_files(&mut self) {
		let tree = self.folder.tree.as_ref();
		let root = tree.map(|tree| tree.root.clone());
		let catalog = super::file_search::catalog(
			tree.into_iter()
				.flat_map(|tree| tree.entries.iter())
				.filter_map(|entry| entry.file.clone()),
			self.readers.entries().iter().map(|tab| tab.path.as_path()),
		);
		let start_changed = self.folder.start.apply_catalog(
			root.clone(),
			catalog.clone(),
			&mut self.ui,
		);
		if start_changed {
			self.start_search_changed();
		}
		if self.interaction.overlays.quick_open.open()
			&& self.interaction.overlays.quick_open.search.apply_catalog(
				root,
				catalog,
				&mut self.ui,
			) {
			self.quick_open_search_changed();
		}
	}
	pub(super) fn start_search_changed(&mut self) {
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
		mode: markview_selection::WheelMode,
	) -> bool {
		if self.readers.session.path.is_some()
			|| !self.readers.session.snapshot.blocks.is_empty()
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
		assert!(app.input_geometry(TextField::Files).is_some());
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
		assert!(
			app.interaction
				.overlays
				.quick_open
				.search
				.matches
				.is_empty()
		);
		assert_eq!(app.interaction.overlays.quick_open.search.catalog.len(), 2);
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
				ModifiersState::SUPER
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
		assert_eq!(app.interaction.overlays.quick_open.search.matches.len(), 3);
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
