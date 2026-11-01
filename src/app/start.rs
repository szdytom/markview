//! Workspace history and cancellable file matching for the start page.
use super::{App, Event, SendEvent};
use markview_layout::text_input::TextInput;
use nucleo_matcher::{
	Config, Matcher, Utf32String,
	pattern::{CaseMatching, Normalization, Pattern},
};
use serde::{Deserialize, Serialize};
use std::{
	collections::HashMap,
	fs,
	io::Write,
	ops::Range,
	path::{Path, PathBuf},
	sync::{
		Arc, Condvar, Mutex,
		atomic::{AtomicU64, Ordering},
	},
	time::{SystemTime, UNIX_EPOCH},
};
use unicode_segmentation::UnicodeSegmentation;

const RECENTS: usize = 8;
const RESULTS: usize = 64;

#[derive(Debug)]
pub(super) struct File {
	pub id: usize,
	pub name: Arc<str>,
	pub parent: Arc<str>,
	text: Utf32String,
	depth: usize,
}
impl File {
	pub fn new(id: usize, root: &Path, path: &Path) -> Arc<Self> {
		let relative = path.strip_prefix(root).unwrap();
		Arc::new(Self {
			id,
			name: path.file_name().unwrap().to_string_lossy().as_ref().into(),
			parent: relative
				.parent()
				.unwrap()
				.to_string_lossy()
				.as_ref()
				.into(),
			text: relative
				.to_string_lossy()
				.replace('\\', "/")
				.as_str()
				.into(),
			depth: relative.components().count(),
		})
	}
	fn relative(&self) -> String {
		if self.parent.is_empty() {
			self.name.to_string()
		} else {
			format!("{}/{}", self.parent, self.name).replace('\\', "/")
		}
	}
}

#[derive(Debug)]
pub(super) struct Snippet {
	pub text: String,
	/// UTF-8 byte ranges covering whole matched graphemes.
	pub matched: Vec<Range<usize>>,
}
impl Snippet {
	fn new(path: &str, indices: &[u32]) -> Self {
		let graphemes: Vec<_> = path.graphemes(true).collect();
		let start = (indices[0] as usize).saturating_sub(6);
		let end = (indices.last().unwrap() + 13) as usize;
		let end = end.min(graphemes.len()).min(start + 96);
		let mut text = if start > 0 {
			"…".into()
		} else {
			String::new()
		};
		let mut matched: Vec<Range<usize>> = Vec::new();
		for (index, grapheme) in
			graphemes.iter().enumerate().take(end).skip(start)
		{
			let byte = text.len();
			text.push_str(grapheme);
			if indices.binary_search(&(index as u32)).is_ok() {
				if let Some(previous) = matched.last_mut()
					&& previous.end == byte
				{
					previous.end = text.len();
				} else {
					matched.push(byte..text.len());
				}
			}
		}
		if end < graphemes.len() {
			text.push('…');
		}
		Self { text, matched }
	}
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Recent {
	pub path: PathBuf,
	pub opened: u64,
	#[serde(default)]
	pub name: String,
}
#[derive(Default)]
pub(super) struct State {
	pub input: TextInput,
	pub recent: Vec<Recent>,
	pub matches: Arc<[Arc<File>]>,
	pub snippets: HashMap<usize, Snippet>,
	pub pending: bool,
	pub scroll: f32,
	catalog: Arc<[Arc<File>]>,
	root: Option<PathBuf>,
	sequence: u64,
	worker: Option<Worker>,
	history: Option<PathBuf>,
	saved: Arc<AtomicU64>,
	writer: Arc<Mutex<()>>,
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
type Matches = (Arc<[Arc<File>]>, HashMap<usize, Snippet>);
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

fn find(
	matcher: &mut Matcher,
	catalog: &[Arc<File>],
	query: &str,
	cancelled: impl Fn() -> bool,
) -> Option<Matches> {
	let query = query.replace('\\', "/");
	let pattern = Pattern::new(
		&query,
		CaseMatching::Ignore,
		Normalization::Smart,
		nucleo_matcher::pattern::AtomKind::Fuzzy,
	);
	let mut scored = Vec::new();
	for (index, file) in catalog.iter().enumerate() {
		if index % 128 == 0 && cancelled() {
			return None;
		}
		if let Some(score) = pattern.score(file.text.slice(..), matcher) {
			scored.push((score, index));
		}
	}
	let order = |a: &(u32, usize), b: &(u32, usize)| {
		b.0.cmp(&a.0)
			.then_with(|| catalog[a.1].depth.cmp(&catalog[b.1].depth))
			.then_with(|| catalog[a.1].name.cmp(&catalog[b.1].name))
			.then_with(|| a.1.cmp(&b.1))
	};
	let limit = if query.trim().is_empty() { 3 } else { RESULTS };
	if scored.len() > limit {
		scored.select_nth_unstable_by(limit, order);
		scored.truncate(limit);
	}
	scored.sort_unstable_by(order);
	let mut matches = Vec::with_capacity(scored.len());
	let mut snippets = HashMap::new();
	let mut indices = Vec::new();
	for (_, index) in scored {
		if cancelled() {
			return None;
		}
		let file = &catalog[index];
		if !query.trim().is_empty() {
			indices.clear();
			pattern
				.indices(file.text.slice(..), matcher, &mut indices)
				.unwrap();
			indices.sort_unstable();
			indices.dedup();
			let name_start =
				file.text.len() - file.name.graphemes(true).count();
			let snippet = if indices[0] as usize >= name_start {
				for index in &mut indices {
					*index -= name_start as u32;
				}
				Snippet::new(&file.name, &indices)
			} else {
				Snippet::new(&file.relative(), &indices)
			};
			snippets.insert(file.id, snippet);
		}
		matches.push(file.clone());
	}
	Some((matches.into(), snippets))
}

impl<P: SendEvent> App<P> {
	pub(super) fn refresh_start_files(&mut self) {
		let Some(tree) = &self.folder.tree else {
			return;
		};
		let state = &mut self.folder.start;
		if state.root.as_ref() != Some(&tree.root) {
			state.root = Some(tree.root.clone());
			state.input.set_text(&mut self.ui, "");
			state.scroll = 0.0;
			state.matches = Arc::default();
			state.snippets.clear();
		}
		state.catalog = tree
			.entries
			.iter()
			.filter_map(|entry| entry.file.clone())
			.collect();
		self.start_search_changed();
	}
	pub(super) fn start_search_changed(&mut self) {
		let state = &mut self.folder.start;
		let worker = state.worker.get_or_insert_with(|| {
			let proxy = self.proxy.clone();
			Worker::new(move |result| proxy.send(Event::FilesReady(result)))
		});
		state.sequence =
			worker.submit(state.catalog.clone(), state.input.text().to_owned());
		state.pending = true;
		state.scroll = 0.0;
		self.redraw();
	}
	pub(super) fn start_files_ready(&mut self, result: Result) {
		if result.sequence != self.folder.start.sequence {
			return;
		}
		self.folder.start.pending = false;
		self.folder.start.matches = result.matches;
		self.folder.start.snippets = result.snippets;
		let scope = crate::state::Command::Chrome(
			crate::state::ChromeCommand::StartFile(0),
		)
		.widget_id()
		.scope;
		self.interaction.widgets.captured = self
			.interaction
			.widgets
			.captured
			.filter(|id| id.scope != scope);
		self.refresh_hover();
		self.redraw();
	}
	pub(super) fn open_recent_workspace(&mut self, index: usize) {
		if let Some(recent) = self.folder.start.recent.get(index) {
			self.open_folder(recent.path.clone(), true);
		}
	}
	pub(super) fn open_start_file(&mut self, id: usize) {
		if let Some(file) = self
			.folder
			.tree
			.as_ref()
			.and_then(|tree| tree.entries.get(id))
			.filter(|entry| !entry.directory)
		{
			self.open(file.path.clone());
		}
	}
	pub(super) fn scroll_start(&mut self, delta: f32) -> bool {
		if self.readers.session.path.is_some()
			|| !self.readers.session.snapshot.blocks.is_empty()
		{
			return false;
		}
		let g = self.chrome().start_geometry();
		let next = (g.scroll + delta).clamp(0.0, g.max_scroll);
		self.folder.start.scroll = next;
		if next == g.scroll {
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
		fs::create_dir_all(dir.path().join("docs/笔记")).unwrap();
		fs::create_dir(dir.path().join("node_modules")).unwrap();
		for name in ["README.md", "docs/笔记/阅读.md", "node_modules/hidden.md"]
		{
			fs::write(dir.path().join(name), "# Reading").unwrap();
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
			crate::app::folder::Tree::read(dir.path().into()),
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

		fs::remove_file(dir.path().join("README.md")).unwrap();
		fs::write(dir.path().join("README-new.md"), "# New").unwrap();
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
			&dir.path().join("README-new.md")
		);
		assert!(app.input_geometry(crate::state::TextField::Files).is_none());
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
	fn match_snippets_preserve_graphemes_and_skip_long_unmatched_prefixes() {
		let root = Path::new("workspace");
		let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
		for (name, query, expected) in [
			("cafe\u{301}-notes.md", "cafe", "cafe\u{301}"),
			("阅读-notes.md", "阅读", "阅读"),
			("CHANGELOG.md", "chlg", "CHLG"),
		] {
			let file = File::new(
				0,
				root,
				&root
					.join("a-long-unmatched-parent-directory/nested")
					.join(name),
			);
			let (_, snippets) =
				find(&mut matcher, &[file], query, || false).unwrap();
			let snippet = &snippets[&0];
			assert!(!snippet.text.contains("unmatched-parent"));
			let matched = snippet
				.matched
				.iter()
				.map(|range| &snippet.text[range.clone()])
				.collect::<String>();
			assert_eq!(matched, expected);
			assert!(snippet.text.graphemes(true).count() <= 98);
		}
	}

	#[test]
	#[ignore = "manual file matching performance measurement"]
	fn workspace_search_performance() {
		let root = Path::new("workspace");
		let started = Instant::now();
		let files: Vec<_> = (0..100_000)
			.map(|index| {
				File::new(
					index,
					root,
					&root.join(format!(
						"docs/group-{}/reading-{index}.md",
						index % 100
					)),
				)
			})
			.collect();
		eprintln!("Prepare 100000 paths: {:?}", started.elapsed());
		let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
		for query in
			["", "r", "read", "group-19", "read99999", "no_match_at_all"]
		{
			let started = Instant::now();
			let (result, _) =
				find(&mut matcher, &files, query, || false).unwrap();
			eprintln!(
				"{query:?}: {:?}, {} results",
				started.elapsed(),
				result.len()
			);
			assert!(result.len() <= RESULTS);
		}
		assert!(find(&mut matcher, &files, "read", || true).is_none());
	}
}
