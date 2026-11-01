//! Directory snapshots, bounded event batches, and watcher recovery.
use super::{Child, Tree, excluded, list};
use markview_platform::watch::{self, EventKind, TreeWatcher};
use std::{
	collections::{HashMap, HashSet},
	path::{Path, PathBuf},
	sync::{
		Arc, Mutex,
		atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
		mpsc,
	},
	time::{Duration, Instant, SystemTime},
};

const BATCH: Duration = Duration::from_millis(100);
const POLL: Duration = Duration::from_secs(2);
const AUDIT: Duration = Duration::from_secs(30);
const RETRY: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub(in crate::app) struct Update {
	pub revision: u64,
	pub view_revision: u64,
	pub directories: Vec<(PathBuf, Vec<Child>)>,
	pub error: Option<String>,
}

enum Request {
	Event(watch::Result),
	Refresh,
	View {
		revision: u64,
		show: bool,
		opened: HashSet<PathBuf>,
	},
	Stop,
}

#[derive(Default)]
struct Visibility {
	show: bool,
	opened: HashSet<PathBuf>,
}

pub(super) struct Observer {
	tx: mpsc::Sender<Request>,
	stop: Arc<AtomicBool>,
	queued: Arc<AtomicUsize>,
	active: Arc<AtomicBool>,
	latest: Arc<AtomicU64>,
	visible: Arc<Mutex<Visibility>>,
}

impl Observer {
	pub fn new(
		root: PathBuf,
		changed: impl Fn(Update) + Send + 'static,
	) -> Self {
		Self::start(root, false, changed)
	}

	fn start(
		root: PathBuf,
		polling: bool,
		changed: impl Fn(Update) + Send + 'static,
	) -> Self {
		let (tx, rx) = mpsc::channel();
		let stop = Arc::new(AtomicBool::new(false));
		let stopped = stop.clone();
		let queued = Arc::new(AtomicUsize::new(0));
		let queued_worker = queued.clone();
		let active = Arc::new(AtomicBool::new(true));
		let working = active.clone();
		let latest = Arc::new(AtomicU64::new(0));
		let produced = latest.clone();
		let flag = Arc::new(AtomicBool::new(false));
		let visible = Arc::new(Mutex::new(Visibility::default()));
		let events = Events {
			root: root.clone(),
			tx: tx.clone(),
			queued: queued.clone(),
			overflow: flag.clone(),
			visible: visible.clone(),
		};
		let watcher = if polling { None } else { events.watcher() };
		// The worker owns its watcher; cancellation cannot wait on filesystem I/O.
		let _ = std::thread::spawn(move || {
			let mut state = State::new(root, watcher, stopped.clone());
			state.events = (!polling).then_some(events);
			let mut schedule = Schedule::new(state.root.clone());
			while !stopped.load(Ordering::Relaxed) {
				match rx.recv_timeout(
					schedule
						.deadline()
						.saturating_duration_since(Instant::now()),
				) {
					Ok(Request::Stop)
					| Err(mpsc::RecvTimeoutError::Disconnected) => break,
					Ok(request) => {
						queued_worker.fetch_sub(1, Ordering::Relaxed);
						schedule.request(&mut state, request, &flag);
					}
					Err(mpsc::RecvTimeoutError::Timeout) => {}
				}
				let now = Instant::now();
				schedule.tick(&mut state, &flag, now);
				if !schedule.dirty.is_empty() {
					working.store(true, Ordering::Release);
				}
				if schedule.take_due(now) {
					let update =
						state.refresh(std::mem::take(&mut schedule.dirty));
					produced.store(update.revision, Ordering::Release);
					working.store(false, Ordering::Release);
					if !stopped.load(Ordering::Relaxed) {
						changed(update);
					}
				}
			}
		});
		Self {
			tx,
			stop,
			queued,
			active,
			latest,
			visible,
		}
	}

	pub fn pending(&self, revision: u64) -> bool {
		self.queued.load(Ordering::Acquire) != 0
			|| self.active.load(Ordering::Acquire)
			|| self.latest.load(Ordering::Acquire) > revision
	}

	pub fn refresh(&self) {
		self.active.store(true, Ordering::Release);
		self.send(Request::Refresh);
	}
	pub fn view(&self, tree: &Tree, revision: u64) {
		self.active.store(true, Ordering::Release);
		*self.visible.lock().unwrap() = Visibility {
			show: tree.show_excluded,
			opened: tree.expanded.clone(),
		};
		self.send(Request::View {
			revision,
			show: tree.show_excluded,
			opened: tree.expanded.clone(),
		});
	}
	fn send(&self, request: Request) {
		// Control requests preserve their order; only filesystem events may overflow.
		self.queued.fetch_add(1, Ordering::Relaxed);
		if self.tx.send(request).is_err() {
			self.queued.fetch_sub(1, Ordering::Relaxed);
		}
	}
}

impl Drop for Observer {
	fn drop(&mut self) {
		self.stop.store(true, Ordering::Relaxed);
		let _ = self.tx.send(Request::Stop);
	}
}

#[derive(Clone)]
struct Events {
	root: PathBuf,
	tx: mpsc::Sender<Request>,
	queued: Arc<AtomicUsize>,
	overflow: Arc<AtomicBool>,
	visible: Arc<Mutex<Visibility>>,
}

impl Events {
	fn watcher(&self) -> Option<TreeWatcher> {
		let events = self.clone();
		TreeWatcher::new(self.root.clone(), move |event| events.send(event))
	}

	fn relevant(&self, path: &Path) -> bool {
		let Ok(relative) = path.strip_prefix(&self.root) else {
			return false;
		};
		let mut parts = relative.components().peekable();
		while let Some(part) = parts.next() {
			let std::path::Component::Normal(name) = part else {
				return false;
			};
			if !excluded(Path::new(name), true) {
				continue;
			}
			if parts.peek().is_none() {
				return true;
			}
			let visible = self.visible.lock().unwrap();
			return visible.show
				&& visible.opened.iter().any(|opened| {
					path == opened || path.parent() == Some(opened.as_path())
				});
		}
		true
	}

	fn send(&self, event: watch::Result) {
		if event.as_ref().is_ok_and(|event| {
			event.kind == EventKind::Access
				|| (!event.need_rescan()
					&& !event.paths.is_empty()
					&& !event.paths.iter().any(|path| self.relevant(path)))
		}) {
			return;
		}
		if self.queued.fetch_add(1, Ordering::Relaxed) < 256 {
			if self.tx.send(Request::Event(event)).is_err() {
				self.queued.fetch_sub(1, Ordering::Relaxed);
			}
		} else {
			self.queued.fetch_sub(1, Ordering::Relaxed);
			self.overflow.store(true, Ordering::Relaxed);
		}
	}
}

struct Schedule {
	dirty: HashSet<PathBuf>,
	due: Option<Instant>,
	poll: Instant,
	audit: Instant,
	retry: Instant,
}

impl Schedule {
	fn new(root: PathBuf) -> Self {
		let now = Instant::now();
		Self {
			dirty: HashSet::from([root]),
			due: Some(now),
			poll: now + POLL,
			audit: now + AUDIT,
			retry: now + RETRY,
		}
	}

	fn deadline(&self) -> Instant {
		self.due
			.unwrap_or(self.poll)
			.min(self.poll)
			.min(self.retry)
			.min(self.audit)
	}

	fn take_due(&mut self, now: Instant) -> bool {
		if self.due.is_some_and(|due| due <= now) {
			self.due = None;
			true
		} else {
			false
		}
	}

	fn audit(&mut self, state: &State) {
		self.dirty = state.snapshots.keys().cloned().collect();
		self.dirty.insert(state.root.clone());
	}

	fn request(
		&mut self,
		state: &mut State,
		request: Request,
		overflow: &AtomicBool,
	) {
		match request {
			Request::Refresh => {
				self.audit(state);
				self.due = Some(Instant::now());
			}
			Request::View {
				revision,
				show,
				opened,
			} => {
				state.view_revision = revision;
				state.resend = true;
				state.show = show;
				state.opened = opened;
				self.audit(state);
				self.due = Some(Instant::now());
			}
			Request::Event(Ok(event))
				if !event.need_rescan() && !event.paths.is_empty() =>
			{
				self.paths_changed(state, event);
			}
			Request::Event(_) => {
				overflow.store(true, Ordering::Relaxed);
			}
			Request::Stop => unreachable!(),
		}
	}

	fn paths_changed(&mut self, state: &mut State, event: watch::Event) {
		let structural = matches!(
			event.kind,
			EventKind::Create | EventKind::Remove | EventKind::Rename
		);
		for path in event.paths {
			if structural && state.snapshots.contains_key(&path) {
				self.dirty.extend(state.invalidate_watches(&path));
			}
			if path == state.root || state.snapshots.contains_key(&path) {
				self.dirty.insert(path.clone());
			}
			if let Some(parent) = path.parent()
				&& state.snapshots.contains_key(parent)
			{
				self.dirty.insert(parent.to_owned());
			}
		}
	}

	fn tick(&mut self, state: &mut State, overflow: &AtomicBool, now: Instant) {
		if now >= self.poll {
			for path in state.snapshots.keys() {
				if !state.watched.contains(path)
					|| state.stamps.get(path) != Some(&stamp(path))
				{
					self.dirty.insert(path.clone());
				}
			}
			self.poll = now + POLL;
		}
		if now >= self.retry {
			if state.missing || !state.root.is_dir() {
				self.dirty.insert(state.root.clone());
			}
			state.register();
			self.retry = now + RETRY;
		}
		if now >= self.audit
			|| overflow.swap(false, Ordering::Relaxed)
			|| self.dirty.len() > 4096
		{
			self.audit(state);
			self.audit = now + AUDIT;
		}
		if !self.dirty.is_empty() {
			self.due.get_or_insert(now + BATCH);
		}
	}
}

struct State {
	root: PathBuf,
	watcher: Option<TreeWatcher>,
	events: Option<Events>,
	watched: HashSet<PathBuf>,
	snapshots: HashMap<PathBuf, Vec<Child>>,
	stamps: HashMap<PathBuf, Option<SystemTime>>,
	show: bool,
	opened: HashSet<PathBuf>,
	revision: u64,
	missing: bool,
	view_revision: u64,
	resend: bool,
	error: Option<String>,
	#[cfg(test)]
	reads: Vec<PathBuf>,
	stop: Arc<AtomicBool>,
}

fn stamp(path: &Path) -> Option<SystemTime> {
	std::fs::metadata(path).ok()?.modified().ok()
}

impl State {
	fn new(
		root: PathBuf,
		mut watcher: Option<TreeWatcher>,
		stop: Arc<AtomicBool>,
	) -> Self {
		if let Some(watcher) = &mut watcher {
			watcher.watch_parent();
		}
		Self {
			root,
			watcher,
			events: None,
			watched: HashSet::new(),
			snapshots: HashMap::new(),
			stamps: HashMap::new(),
			show: false,
			opened: HashSet::new(),
			revision: 0,
			missing: false,
			view_revision: 0,
			resend: false,
			stop,
			error: None,
			#[cfg(test)]
			reads: Vec::new(),
		}
	}

	fn watch(&mut self, path: &Path) {
		let is_root = path == self.root;
		let Some(watcher) = &mut self.watcher else {
			return;
		};
		let watched = if is_root {
			watcher.watch_root()
		} else {
			watcher.watch_dir(path)
		};
		if watched {
			self.watched.insert(path.to_owned());
		}
	}

	fn unwatch(&mut self, path: &Path) {
		if self.watched.remove(path)
			&& let Some(watcher) = &mut self.watcher
		{
			let _ = watcher.unwatch(path);
		}
	}

	fn read(&mut self, path: &Path) -> Vec<Child> {
		if !self.watched.contains(path) {
			self.watch(path);
		}
		#[cfg(test)]
		self.reads.push(path.to_owned());
		match list(path, &|| self.stop.load(Ordering::Relaxed), self.show) {
			Ok(children) => {
				if path == self.root {
					self.error = None;
				}
				children
			}
			Err(error) => {
				self.unwatch(path);
				if path == self.root {
					self.error = Some(error.to_string());
				} else {
					log::warn!(
						"Cannot read folder {}: {error}",
						path.display()
					);
				}
				Vec::new()
			}
		}
	}

	fn prune(
		&mut self,
		previous: Option<Vec<Child>>,
		children: &[Child],
	) -> Vec<PathBuf> {
		let directories: HashSet<_> = children
			.iter()
			.filter(|child| child.directory)
			.map(|child| &child.path)
			.collect();
		let mut pending: Vec<_> = previous
			.into_iter()
			.flatten()
			.filter(|child| {
				child.directory
					&& (!directories.contains(&child.path)
						|| !self.eligible(&child.path))
			})
			.map(|child| child.path)
			.collect();
		let mut removed = Vec::new();
		while let Some(path) = pending.pop() {
			self.opened.remove(&path);
			if let Some(children) = self.snapshots.remove(&path) {
				pending.extend(
					children
						.into_iter()
						.filter(|child| child.directory)
						.map(|child| child.path),
				);
				self.stamps.remove(&path);
				self.unwatch(&path);
				removed.push(path);
			}
		}
		removed
	}

	fn invalidate_watches(&mut self, path: &Path) -> Vec<PathBuf> {
		let mut pending = vec![path.to_owned()];
		let mut invalidated = Vec::new();
		while let Some(path) = pending.pop() {
			if let Some(children) = self.snapshots.get(&path) {
				pending.extend(
					children
						.iter()
						.filter(|child| child.directory)
						.map(|child| child.path.clone()),
				);
			}
			self.unwatch(&path);
			invalidated.push(path);
		}
		invalidated
	}

	fn eligible(&self, path: &Path) -> bool {
		!path
			.strip_prefix(&self.root)
			.unwrap()
			.components()
			.any(|part| excluded(Path::new(part.as_os_str()), true))
			|| (self.show && self.opened.contains(path))
	}

	fn register(&mut self) {
		if self.watcher.is_none()
			&& let Some(events) = &self.events
		{
			self.watcher = events.watcher();
			if let Some(watcher) = &mut self.watcher {
				watcher.watch_parent();
			}
		}
		if self.watcher.is_some() {
			let paths: Vec<_> = std::iter::once(self.root.clone())
				.chain(self.snapshots.keys().cloned())
				.collect();
			for path in paths {
				if !self.watched.contains(&path) {
					self.watch(&path);
				}
			}
		}
	}

	fn contains_directory(&self, path: &Path) -> bool {
		path == self.root
			|| self.snapshots.get(path.parent().unwrap()).is_some_and(
				|children| {
					children
						.iter()
						.any(|child| child.path == path && child.directory)
				},
			)
	}

	fn refresh(&mut self, dirty: HashSet<PathBuf>) -> Update {
		self.revision += 1;
		let mut update = Update {
			revision: self.revision,
			view_revision: self.view_revision,
			directories: Vec::new(),
			error: None,
		};
		let mut pending: Vec<_> = dirty.into_iter().collect();
		pending
			.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
		let mut visited = HashSet::new();
		while let Some(path) = pending.pop() {
			if self.stop.load(Ordering::Relaxed) {
				break;
			}
			if !visited.insert(path.clone()) || !self.eligible(&path) {
				continue;
			}
			// A removed parent's queued child must not be resurrected.
			if !self.contains_directory(&path) {
				continue;
			}
			let children = self.read(&path);
			let recovered =
				path == self.root && self.missing && self.error.is_none();
			if path == self.root {
				self.missing = self.error.is_some();
			}
			let previous =
				self.snapshots.insert(path.clone(), children.clone());
			if previous.as_ref() != Some(&children) || self.resend || recovered
			{
				update.directories.push((path.clone(), children.clone()));
			}
			self.stamps.insert(path.clone(), stamp(&path));
			for child in children
				.iter()
				.filter(|child| child.directory && self.eligible(&child.path))
			{
				if !self.snapshots.contains_key(&child.path) {
					pending.push(child.path.clone());
				}
			}
			for path in self.prune(previous, &children) {
				update.directories.push((path, Vec::new()));
			}
		}
		update
			.directories
			.sort_by_key(|(path, _)| path.components().count());
		self.opened.retain(|path| self.snapshots.contains_key(path));
		self.resend = false;
		update.error = self.error.clone();
		update
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::fs;

	pub(super) fn state(root: PathBuf) -> State {
		State::new(root, None, Arc::new(AtomicBool::new(false)))
	}

	#[test]
	fn snapshots_read_only_affected_directories_and_expand_exclusions_one_level_at_a_time()
	 {
		let dir = tempfile::tempdir().unwrap();
		let root = dir.path().canonicalize().unwrap();
		for path in ["a/deep", "b", "node_modules/pkg/deep", ".git/objects"] {
			fs::create_dir_all(root.join(path)).unwrap();
		}
		fs::write(root.join("a/Note.md"), "note").unwrap();
		let mut state = state(root.clone());
		let update = state.refresh(HashSet::from([root.clone()]));
		assert_eq!(update.directories.len(), 4);
		assert!(
			!state
				.snapshots
				.keys()
				.any(|path| path.starts_with(root.join("node_modules"))
					|| path.starts_with(root.join(".git")))
		);
		state.reads.clear();
		fs::rename(root.join("a/Note.md"), root.join("a/New.md")).unwrap();
		let update = state.refresh(HashSet::from([root.join("a")]));
		assert_eq!(state.reads, [root.join("a")]);
		assert_eq!(update.directories.len(), 1);
		state.show = true;
		state.refresh(HashSet::from([root.clone()]));
		assert!(
			state.snapshots[&root]
				.iter()
				.any(|child| child.path.ends_with("node_modules"))
		);
		assert!(!state.snapshots.contains_key(&root.join("node_modules")));
		state.opened.insert(root.join("node_modules"));
		state.refresh(HashSet::from([root.clone()]));
		assert!(state.snapshots.contains_key(&root.join("node_modules")));
		assert!(!state.snapshots.contains_key(&root.join("node_modules/pkg")));
		state.opened.insert(root.join("node_modules/pkg"));
		state.refresh(HashSet::from([root.join("node_modules")]));
		assert!(state.snapshots.contains_key(&root.join("node_modules/pkg")));
		state.opened.clear();
		state.refresh(HashSet::from([root.clone()]));
		assert!(!state.snapshots.contains_key(&root.join("node_modules")));
		assert!(!state.snapshots.contains_key(&root.join("node_modules/pkg")));
		fs::rename(root.join("a"), root.join("renamed")).unwrap();
		state.refresh(HashSet::from([root.clone(), root.join("a/deep")]));
		assert!(
			!state
				.snapshots
				.keys()
				.any(|path| path.starts_with(root.join("a")))
		);
		assert!(state.snapshots.contains_key(&root.join("renamed/deep")));
	}

	#[test]
	fn cancelling_an_observer_does_not_wait_for_blocked_worker_calls() {
		let dir = tempfile::tempdir().unwrap();
		let (started, ready) = mpsc::channel();
		let (release, gate) = mpsc::channel::<()>();
		let (finished, done) = mpsc::channel();
		let observer =
			Observer::start(dir.path().to_owned(), true, move |_| {
				let _ = started.send(());
				let _ = gate.recv_timeout(Duration::from_secs(3));
				let _ = finished.send(());
			});
		ready.recv_timeout(Duration::from_secs(3)).unwrap();
		let start = Instant::now();
		drop(observer);
		assert!(
			start.elapsed() < Duration::from_secs(1),
			"observer cancellation waited for a blocked call"
		);
		release.send(()).unwrap();
		done.recv_timeout(Duration::from_secs(3)).unwrap();
	}

	#[test]
	fn excluded_subtree_events_are_dropped_until_opened() {
		let root = PathBuf::from("/root");
		let (tx, _rx) = mpsc::channel();
		let visible = Arc::new(Mutex::new(Visibility::default()));
		let events = Events {
			root: root.clone(),
			tx,
			queued: Arc::default(),
			overflow: Arc::default(),
			visible: visible.clone(),
		};
		assert!(events.relevant(&root.join("Note.md")));
		assert!(events.relevant(&root.join("node_modules")));
		assert!(!events.relevant(&root.join("node_modules/pkg/Note.md")));
		visible.lock().unwrap().show = true;
		visible
			.lock()
			.unwrap()
			.opened
			.insert(root.join("node_modules/pkg"));
		assert!(events.relevant(&root.join("node_modules/pkg/Note.md")));
		assert!(!events.relevant(&root.join("node_modules/other/Note.md")));
		assert!(!events.relevant(&root.join("..")));
	}

	#[test]
	fn replaced_directories_restore_native_watches_for_surviving_paths() {
		let dir = tempfile::tempdir().unwrap();
		let root = dir.path().canonicalize().unwrap();
		let branch = root.join("branch");
		let nested = branch.join("nested");
		fs::create_dir_all(&nested).unwrap();
		let (tx, rx) = mpsc::channel();
		let watcher = TreeWatcher::new(root.clone(), move |event| {
			let _ = tx.send(event);
		})
		.unwrap();
		let mut state = State::new(
			root.clone(),
			Some(watcher),
			Arc::new(AtomicBool::new(false)),
		);
		state.refresh(HashSet::from([root.clone()]));
		fs::remove_dir_all(&branch).unwrap();
		fs::create_dir_all(&nested).unwrap();
		let mut schedule = Schedule::new(root.clone());
		schedule.request(
			&mut state,
			Request::Event(Ok(
				watch::Event::new(EventKind::Remove).add_path(branch.clone())
			)),
			&AtomicBool::new(false),
		);
		assert!(state.watched.contains(&root));
		assert!(!state.watched.contains(&branch));
		assert!(!state.watched.contains(&nested));
		state.refresh(std::mem::take(&mut schedule.dirty));
		assert!(state.watched.contains(&branch));
		assert!(state.watched.contains(&nested));
		while rx.try_recv().is_ok() {}
		let note = nested.join("New.md");
		fs::write(&note, "note").unwrap();
		let deadline = Instant::now() + Duration::from_secs(3);
		loop {
			let event: watch::Result = rx
				.recv_timeout(
					deadline.saturating_duration_since(Instant::now()),
				)
				.expect("replacement directory lost its native watch");
			if event.is_ok_and(|event| event.paths.contains(&note)) {
				break;
			}
		}
	}

	#[test]
	fn recreated_excluded_subtrees_require_explicit_expansion_again() {
		let dir = tempfile::tempdir().unwrap();
		let root = dir.path().canonicalize().unwrap();
		let excluded = root.join("node_modules");
		let package = excluded.join("pkg");
		fs::create_dir_all(&package).unwrap();
		let mut state = state(root.clone());
		state.show = true;
		state.opened.extend([excluded.clone(), package.clone()]);
		state.refresh(HashSet::from([root.clone()]));
		assert!(state.snapshots.contains_key(&package));
		fs::remove_dir_all(&excluded).unwrap();
		state.refresh(HashSet::from([root.clone()]));
		assert!(state.opened.is_empty());
		// A queued view can contain paths removed by an earlier batch.
		let mut schedule = Schedule::new(root.clone());
		schedule.request(
			&mut state,
			Request::View {
				revision: 1,
				show: true,
				opened: HashSet::from([excluded.clone(), package.clone()]),
			},
			&AtomicBool::new(false),
		);
		state.refresh(std::mem::take(&mut schedule.dirty));
		assert!(state.opened.is_empty());

		fs::create_dir_all(&package).unwrap();
		fs::write(package.join("Note.md"), "note").unwrap();
		state.reads.clear();
		state.refresh(HashSet::from([root.clone()]));
		assert_eq!(state.reads.as_slice(), std::slice::from_ref(&root));
		assert!(!state.snapshots.contains_key(&excluded));
		assert!(!state.snapshots.contains_key(&package));
	}

	fn receive_until(
		rx: &mpsc::Receiver<Update>,
		predicate: impl Fn(&Update) -> bool,
	) -> Update {
		let deadline = Instant::now() + Duration::from_secs(8);
		loop {
			let update = rx
				.recv_timeout(
					deadline.saturating_duration_since(Instant::now()),
				)
				.expect("folder update timed out");
			if predicate(&update) {
				return update;
			}
		}
	}

	#[test]
	fn native_events_coalesce_atomic_saves_and_continuous_changes_make_progress()
	 {
		let dir = tempfile::tempdir().unwrap();
		let root = dir.path().canonicalize().unwrap();
		fs::write(root.join("Note.md"), "old").unwrap();
		let (tx, rx) = mpsc::channel();
		let _observer = Observer::new(root.clone(), move |update| {
			let _ = tx.send(update);
		});
		receive_until(&rx, |_| true);
		for index in 0..30 {
			fs::write(root.join(format!("{index}.md")), "note").unwrap();
		}
		fs::write(root.join("save.tmp"), "new").unwrap();
		fs::rename(root.join("save.tmp"), root.join("Note.md")).unwrap();
		let update = receive_until(&rx, |update| {
			update
				.directories
				.iter()
				.any(|(_, children)| children.len() == 31)
		});
		assert_eq!(update.directories.len(), 1);
		let writer_root = root.clone();
		let writer = std::thread::spawn(move || {
			for index in 30..60 {
				fs::write(writer_root.join(format!("{index}.md")), "note")
					.unwrap();
				std::thread::sleep(Duration::from_millis(20));
			}
		});
		let start = Instant::now();
		receive_until(&rx, |update| !update.directories.is_empty());
		assert!(start.elapsed() < Duration::from_secs(1));
		writer.join().unwrap();
	}

	#[test]
	fn polling_recovers_without_native_events_and_reports_root_loss_and_recreation()
	 {
		let dir = tempfile::tempdir().unwrap();
		let root = dir.path().join("root");
		fs::create_dir(&root).unwrap();
		fs::create_dir(root.join("deep")).unwrap();
		let (tx, rx) = mpsc::channel();
		let observer = Observer::start(root.clone(), true, move |update| {
			let _ = tx.send(update);
		});
		receive_until(&rx, |_| true);
		fs::write(root.join("deep/New.md"), "note").unwrap();
		receive_until(&rx, |update| {
			update.directories.iter().any(|(_, children)| {
				children.iter().any(|child| child.path.ends_with("New.md"))
			})
		});
		// An explicit rescan signal also recovers an event with no usable path.
		fs::write(root.join("deep/Lost.md"), "note").unwrap();
		observer.send(Request::Event(Err(watch::Error::new("lost event"))));
		receive_until(&rx, |update| {
			update.directories.iter().any(|(_, children)| {
				children.iter().any(|child| child.path.ends_with("Lost.md"))
			})
		});
		fs::remove_dir_all(&root).unwrap();
		receive_until(&rx, |update| update.error.is_some());
		fs::create_dir(&root).unwrap();
		observer.refresh();
		let recovered = receive_until(&rx, |update| update.error.is_none());
		assert!(
			recovered
				.directories
				.iter()
				.any(|(path, children)| path == &root && children.is_empty())
		);
	}
}

#[cfg(test)]
mod profile {
	use super::{tests::*, *};

	#[test]
	#[ignore = "profiles synthetic local scans and excluded dependency directories"]
	fn folder_incremental_scan_profile() {
		let dir = tempfile::tempdir().unwrap();
		let root = dir.path().canonicalize().unwrap();
		for directory in 0..100 {
			let path = root.join(format!("dir-{directory:03}"));
			std::fs::create_dir(&path).unwrap();
			for note in 0..20 {
				std::fs::write(path.join(format!("{note:02}.md")), "note")
					.unwrap();
			}
		}
		let scan = Instant::now();
		let mut baseline = state(root.clone());
		baseline.refresh(HashSet::from([root.clone()]));
		let without_dependencies = scan.elapsed();
		for directory in 0..100 {
			let path = root.join(format!("node_modules/pkg-{directory:03}"));
			std::fs::create_dir_all(&path).unwrap();
			for note in 0..20 {
				std::fs::write(path.join(format!("{note:02}.md")), "note")
					.unwrap();
			}
		}
		let scan = Instant::now();
		let mut with_dependencies = state(root.clone());
		with_dependencies.refresh(HashSet::from([root.clone()]));
		let with_dependencies_ms = scan.elapsed().as_secs_f64() * 1000.;
		let scan = Instant::now();
		let full = Tree::read(root.clone()).unwrap();
		let tree_ms = scan.elapsed().as_secs_f64() * 1000.;
		with_dependencies.reads.clear();
		std::fs::write(root.join("dir-050/New.md"), "note").unwrap();
		let scan = Instant::now();
		let update =
			with_dependencies.refresh(HashSet::from([root.join("dir-050")]));
		let incremental_ms = scan.elapsed().as_secs_f64() * 1000.;
		assert_eq!(with_dependencies.reads, [root.join("dir-050")]);
		eprintln!(
			"folder scan: tree={} entries/{tree_ms:.2} ms, snapshot without dependencies={:.2} ms, with 2000 excluded notes={with_dependencies_ms:.2} ms, local update={incremental_ms:.3} ms/{} directory",
			full.entries.len(),
			without_dependencies.as_secs_f64() * 1000.,
			update.directories.len()
		);
	}
}

#[cfg(test)]
mod schedule_tests {
	use super::*;

	#[test]
	fn batches_have_a_fixed_deadline_and_audits_find_changes_with_unchanged_stamps()
	 {
		let dir = tempfile::tempdir().unwrap();
		let root = dir.path().canonicalize().unwrap();
		let mut state = tests::state(root.clone());
		state.refresh(HashSet::from([root.clone()]));
		let mut schedule = Schedule::new(root.clone());
		schedule.due = None;
		schedule.dirty.clear();
		let overflow = AtomicBool::new(false);
		let start = Instant::now();
		for ms in [0, 20, 40, 60, 80] {
			schedule.dirty.insert(root.clone());
			schedule.tick(
				&mut state,
				&overflow,
				start + Duration::from_millis(ms),
			);
		}
		assert_eq!(schedule.due, Some(start + BATCH));
		assert!(!schedule.take_due(start + BATCH - Duration::from_millis(1)));
		assert!(schedule.take_due(start + BATCH));
		schedule.dirty.clear();
		std::fs::write(root.join("Lost.md"), "note").unwrap();
		state.stamps.insert(root.clone(), stamp(&root));
		state.watched.insert(root.clone());
		schedule.tick(&mut state, &overflow, start + POLL);
		assert!(schedule.dirty.is_empty());
		schedule.tick(&mut state, &overflow, start + AUDIT);
		let update = state.refresh(std::mem::take(&mut schedule.dirty));
		assert!(
			update.directories[0]
				.1
				.iter()
				.any(|child| child.path.ends_with("Lost.md"))
		);
		overflow.store(true, Ordering::Relaxed);
		schedule.tick(&mut state, &overflow, start + AUDIT + BATCH);
		assert!(schedule.dirty.contains(&root));
	}
}
