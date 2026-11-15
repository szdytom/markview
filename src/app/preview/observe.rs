//! Observes the previewed file independently of the active reader's watcher.
use super::*;
use std::{
	path::PathBuf,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
};

pub(super) struct Observed {
	pub path: PathBuf,
	pub stamp: crate::watch::FileStamp,
	dirty: Arc<AtomicBool>,
	_watch: crate::watch::FileWatch,
}

impl Observed {
	pub fn new(path: PathBuf, wake: impl Fn() + Send + 'static) -> Self {
		let dirty = Arc::new(AtomicBool::new(false));
		let changed = dirty.clone();
		let watch = crate::watch::FileWatch::new(path.clone(), move || {
			changed.store(true, Ordering::Release);
			wake();
		});
		Self {
			stamp: crate::watch::file_stamp(&path),
			path,
			dirty,
			_watch: watch,
		}
	}
	pub fn changed(&self) -> bool {
		self.dirty.load(Ordering::Acquire)
	}
	pub fn invalidate_if_outdated(&self) -> bool {
		if crate::watch::file_stamp(&self.path) != self.stamp {
			self.dirty.store(true, Ordering::Release);
		}
		self.changed()
	}
	pub fn acknowledge(&mut self) {
		self.dirty.store(false, Ordering::Release);
		self.stamp = crate::watch::file_stamp(&self.path);
	}
}

impl<P: super::super::SendEvent> App<P> {
	pub(super) fn refresh_preview_file(&mut self) -> bool {
		if !self
			.preview
			.observed
			.as_ref()
			.is_some_and(Observed::changed)
		{
			return false;
		}
		self.preview.observed.as_mut().unwrap().acknowledge();
		let target = self.preview.hover.as_ref().unwrap().target;
		self.preview.cancel_work();
		self.start_link_preview(target);
		true
	}
}
