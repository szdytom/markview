//! Parent-directory observation, bounded debouncing, and latest-request wins.
#[cfg(test)]
use crate::file::read_document;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::{
	fs,
	path::{Path, PathBuf},
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
		mpsc,
	},
	thread,
	time::{Duration, Instant, SystemTime},
};

/// A save is usually one event, so the quiet window only has to outlast the
/// event burst of a single write. The ceiling keeps a stream of writes from
/// postponing the reload forever.
pub const QUIET: Duration = Duration::from_millis(10);
pub const MAX_WAIT: Duration = Duration::from_millis(40);

#[derive(Default)]
pub struct Debounce {
	first: Option<Instant>,
	last: Option<Instant>,
}
impl Debounce {
	pub fn push(&mut self, now: Instant) {
		self.first.get_or_insert(now);
		self.last = Some(now);
	}
	pub fn deadline(&self) -> Option<Instant> {
		Some((self.first? + MAX_WAIT).min(self.last? + QUIET))
	}
	pub fn take_due(&mut self, now: Instant) -> bool {
		if self.deadline().is_some_and(|d| d <= now) {
			*self = Self::default();
			true
		} else {
			false
		}
	}
}

pub struct FileWatch {
	_watcher: Option<RecommendedWatcher>,
	stop: Arc<AtomicBool>,
	thread: Option<thread::JoinHandle<()>>,
}
impl FileWatch {
	pub fn new(path: PathBuf, changed: impl Fn() + Send + 'static) -> Self {
		Self::observe(path, false, changed)
	}
	pub fn directory(
		path: PathBuf,
		changed: impl Fn() + Send + 'static,
	) -> Self {
		Self::observe(path, true, changed)
	}
	fn observe(
		path: PathBuf,
		directory: bool,
		changed: impl Fn() + Send + 'static,
	) -> Self {
		let stop = Arc::new(AtomicBool::new(false));
		let (tx, rx) = mpsc::sync_channel(64);
		let target = path.clone();
		let mut watcher = notify::recommended_watcher(
			move |event: notify::Result<notify::Event>| {
				if let Ok(event) = event {
					if matches!(event.kind, notify::EventKind::Access(_)) {
						return;
					}
					if event.paths.is_empty()
						|| event.paths.iter().any(|p| {
							p == &target
								|| (directory && p.starts_with(&target))
								|| p.file_name() == target.file_name()
						}) {
						let _ = tx.try_send(());
					}
				} else {
					let _ = tx.try_send(());
				}
			},
		)
		.ok();
		if let Some(w) = &mut watcher
			&& w.watch(
				path.parent()
					.filter(|p| !p.as_os_str().is_empty())
					.unwrap_or(Path::new(".")),
				if directory {
					RecursiveMode::Recursive
				} else {
					RecursiveMode::NonRecursive
				},
			)
			.is_err()
		{
			watcher = None;
		}
		let polling = watcher.is_none();
		let flag = stop.clone();
		let handle = thread::spawn(move || {
			let mut pending = Debounce::default();
			let mut stamp = observed_stamp(&path, directory);
			let mut poll_at = Instant::now() + Duration::from_millis(500);
			while !flag.load(Ordering::Relaxed) {
				let now = Instant::now();
				let timeout = pending
					.deadline()
					.unwrap_or(poll_at)
					.min(poll_at)
					.saturating_duration_since(now)
					.min(Duration::from_millis(100));
				match rx.recv_timeout(timeout) {
					Ok(()) => pending.push(Instant::now()),
					Err(mpsc::RecvTimeoutError::Disconnected) if !polling => {
						break;
					}
					Err(_) => {
						if polling {
							thread::sleep(timeout);
						}
					}
				}
				let now = Instant::now();
				// A cheap metadata poll also recovers lost events on native watchers.
				if now >= poll_at {
					let next = observed_stamp(&path, directory);
					if next != stamp {
						pending.push(now);
						stamp = next;
					}
					poll_at = now + Duration::from_millis(500);
				}
				if pending.take_due(now) {
					stamp = observed_stamp(&path, directory);
					changed();
				}
			}
		});
		Self {
			_watcher: watcher,
			stop,
			thread: Some(handle),
		}
	}
}
impl Drop for FileWatch {
	fn drop(&mut self) {
		self.stop.store(true, Ordering::Relaxed);
		if let Some(t) = self.thread.take() {
			let _ = t.join();
		}
	}
}

type FileStamp = Option<(u64, Option<SystemTime>)>;
fn observed_stamp(path: &Path, directory: bool) -> Vec<(PathBuf, FileStamp)> {
	if !directory {
		return vec![(path.into(), file_stamp(path))];
	}
	let mut values: Vec<_> = fs::read_dir(path)
		.into_iter()
		.flatten()
		.flatten()
		.filter(|e| e.path().extension().is_some_and(|e| e == "toml"))
		.map(|e| {
			let p = e.path();
			let stamp = file_stamp(&p);
			(p, stamp)
		})
		.collect();
	values.sort_by(|a, b| a.0.cmp(&b.0));
	values
}

fn file_stamp(path: &Path) -> Option<(u64, Option<SystemTime>)> {
	fs::metadata(path)
		.ok()
		.map(|m| (m.len(), m.modified().ok()))
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn continuous_events_have_a_deadline() {
		let start = Instant::now();
		let mut d = Debounce::default();
		for ms in [0, 20, 40, 60, 80, 95] {
			d.push(start + Duration::from_millis(ms));
		}
		assert_eq!(d.deadline(), Some(start + MAX_WAIT));
		assert!(d.take_due(start + MAX_WAIT));
		assert_eq!(d.deadline(), None);
	}
	#[test]
	fn atomic_replace_and_recreate_are_observed() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("read.md");
		fs::write(&path, "first").unwrap();
		let (tx, rx) = mpsc::channel();
		let _watch = FileWatch::new(path.clone(), move || {
			let _ = tx.send(());
		});
		let tmp = dir.path().join("save.tmp");
		fs::write(&tmp, "second").unwrap();
		fs::rename(&tmp, &path).unwrap();
		rx.recv_timeout(Duration::from_secs(3)).unwrap();
		assert_eq!(read_document(&path).unwrap(), "second");
		while rx.try_recv().is_ok() {}
		fs::remove_file(&path).unwrap();
		fs::write(&path, "third").unwrap();
		rx.recv_timeout(Duration::from_secs(3)).unwrap();
		assert_eq!(read_document(&path).unwrap(), "third");
	}
	#[test]
	fn empty_and_partial_utf8_reads_have_explicit_results() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("read.md");
		fs::write(&path, [0xe4, 0xb8]).unwrap();
		assert!(read_document(&path).is_err());
		fs::write(&path, "").unwrap();
		assert_eq!(read_document(&path).unwrap(), "");
		fs::write(&path, "\u{feff}中文").unwrap();
		assert_eq!(read_document(&path).unwrap(), "中文");
	}
}

#[cfg(test)]
mod stylesheet_tests {
	use super::*;
	#[test]
	fn directory_created_after_watch_and_atomic_edits_are_seen() {
		let tmp = tempfile::tempdir().unwrap();
		let dir = tmp.path().join("styles");
		let (tx, rx) = mpsc::channel();
		let _watch = FileWatch::directory(dir.clone(), move || {
			let _ = tx.send(());
		});
		fs::create_dir(&dir).unwrap();
		let path = dir.join("a.mvss.toml");
		fs::write(&path, "format_version=2\nversion=1").unwrap();
		rx.recv_timeout(Duration::from_secs(3)).unwrap();
		while rx.try_recv().is_ok() {}
		let replacement = dir.join("a.tmp");
		fs::write(
			&replacement,
			"format_version=2\nversion=1\n[[rule]]\nwhen=['em']\ncolor='#ffffff'",
		)
		.unwrap();
		fs::rename(replacement, &path).unwrap();
		rx.recv_timeout(Duration::from_secs(3)).unwrap();
		while rx.try_recv().is_ok() {}
		fs::remove_file(path).unwrap();
		rx.recv_timeout(Duration::from_secs(3)).unwrap();
	}
}
