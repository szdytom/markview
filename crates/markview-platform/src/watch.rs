//! Native filesystem watches with backend-neutral events.
//!
//! Windows cannot rename a directory while an ancestor has an open change
//! notification handle. A single recursive watch on the tree root avoids
//! locking every directory below it; other platforms keep one watch per
//! directory so excluded subtrees stay unwatched.
use notify::{RecursiveMode, Watcher as _};
use std::{
	fmt,
	path::{Path, PathBuf},
};

/// What happened to the watched path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
	Access,
	Create,
	Remove,
	Rename,
	Modify,
	Other,
}

/// One native filesystem notification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
	pub kind: EventKind,
	pub paths: Vec<PathBuf>,
	rescan: bool,
}

impl Event {
	pub fn new(kind: EventKind) -> Self {
		Self {
			kind,
			paths: Vec::new(),
			rescan: false,
		}
	}

	pub fn add_path(mut self, path: impl Into<PathBuf>) -> Self {
		self.paths.push(path.into());
		self
	}

	/// Whether the backend lost events and the caller must rescan.
	pub fn need_rescan(&self) -> bool {
		self.rescan
	}
}

impl From<notify::Event> for Event {
	fn from(event: notify::Event) -> Self {
		let kind = match event.kind {
			notify::EventKind::Access(_) => EventKind::Access,
			notify::EventKind::Create(_) => EventKind::Create,
			notify::EventKind::Remove(_) => EventKind::Remove,
			notify::EventKind::Modify(notify::event::ModifyKind::Name(_)) => {
				EventKind::Rename
			}
			notify::EventKind::Modify(_) => EventKind::Modify,
			_ => EventKind::Other,
		};
		let rescan = event.need_rescan();
		Self {
			kind,
			paths: event.paths,
			rescan,
		}
	}
}

/// A backend failure, such as a lost event queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
	message: String,
}

impl Error {
	pub fn new(message: impl Into<String>) -> Self {
		Self {
			message: message.into(),
		}
	}
}

impl fmt::Display for Error {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str(&self.message)
	}
}

impl std::error::Error for Error {}

pub type Result<T = Event> = std::result::Result<T, Error>;

/// A native watcher for explicit directory or recursive registrations.
pub struct Watcher {
	inner: notify::RecommendedWatcher,
}

impl Watcher {
	pub fn new(handler: impl Fn(Result) + Send + 'static) -> Option<Self> {
		notify::recommended_watcher(
			move |event: notify::Result<notify::Event>| {
				handler(
					event
						.map(Event::from)
						.map_err(|error| Error::new(error.to_string())),
				);
			},
		)
		.ok()
		.map(|inner| Self { inner })
	}

	pub fn watch_recursive(&mut self, path: &Path) -> bool {
		self.inner.watch(path, RecursiveMode::Recursive).is_ok()
	}

	pub fn watch_directory(&mut self, path: &Path) -> bool {
		self.inner.watch(path, RecursiveMode::NonRecursive).is_ok()
	}

	pub fn unwatch(&mut self, path: &Path) -> bool {
		self.inner.unwatch(path).is_ok()
	}
}

/// A watch over one folder tree.
pub struct TreeWatcher {
	watcher: Watcher,
	root: PathBuf,
	active: bool,
}

impl TreeWatcher {
	pub fn new(
		root: PathBuf,
		handler: impl Fn(Result) + Send + 'static,
	) -> Option<Self> {
		Some(Self {
			watcher: Watcher::new(handler)?,
			root,
			active: false,
		})
	}

	/// Watch the root's parent so removing or replacing the root is noticed.
	pub fn watch_parent(&mut self) -> bool {
		self.root
			.parent()
			.is_some_and(|parent| self.watcher.watch_directory(parent))
	}

	/// Watch the tree root, recursively where the backend supports it.
	pub fn watch_root(&mut self) -> bool {
		if self.active {
			return true;
		}
		let watched = if cfg!(windows) {
			self.watcher.watch_recursive(&self.root)
		} else {
			self.watcher.watch_directory(&self.root)
		};
		self.active = watched;
		watched
	}

	/// Watch one directory, or report that the root watch covers it.
	pub fn watch_dir(&mut self, path: &Path) -> bool {
		if cfg!(windows) {
			self.active && path.starts_with(&self.root)
		} else {
			self.watcher.watch_directory(path)
		}
	}

	/// Stop watching a directory; only the root owns a native tree watch.
	pub fn unwatch(&mut self, path: &Path) -> bool {
		if path == self.root {
			self.active = false;
		} else if cfg!(windows) {
			return true;
		}
		self.watcher.unwatch(path)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::{
		fs,
		sync::mpsc,
		time::{Duration, Instant},
	};

	#[test]
	fn a_tree_watcher_reinstalls_its_root() {
		let dir = tempfile::tempdir().unwrap();
		let root = dir.path().canonicalize().unwrap();
		let (tx, rx) = mpsc::channel();
		let mut watcher = TreeWatcher::new(root.clone(), move |event| {
			let _ = tx.send(event);
		})
		.unwrap();
		assert!(watcher.watch_root());
		assert!(watcher.unwatch(&root));
		assert!(watcher.watch_root());
		let note = root.join("Note.md");
		fs::write(&note, "note").unwrap();
		let deadline = Instant::now() + Duration::from_secs(3);
		loop {
			let event = rx
				.recv_timeout(
					deadline.saturating_duration_since(Instant::now()),
				)
				.unwrap()
				.unwrap();
			if event.paths.contains(&note) {
				break;
			}
		}
	}
}
