//! Tab ownership and request identity, independent of windows and file watchers.
use crate::{
	layout::LayoutOptions,
	state::{ReaderSession, ReaderTab},
	worker::Request,
};
use std::{
	path::PathBuf,
	time::{Duration, Instant},
};
const RELEASE_AFTER: Duration = Duration::from_secs(20);

#[derive(Default)]
pub(super) struct Tabs {
	pub(super) session: ReaderSession,
	entries: Vec<ReaderTab>,
	active: usize,
	request_serial: u64,
}

pub(super) enum Closed {
	Missing,
	Inactive,
	Active,
}
impl Tabs {
	pub(super) fn entries(&self) -> &[ReaderTab] {
		&self.entries
	}
	pub(super) fn active(&self) -> usize {
		self.active
	}
	/// Reorder entries without replacing the active session or issuing requests.
	pub(super) fn move_tab(&mut self, from: usize, to: usize) -> bool {
		if from >= self.entries.len() || to >= self.entries.len() || from == to
		{
			return false;
		}
		let tab = self.entries.remove(from);
		self.entries.insert(to, tab);
		if self.active == from {
			self.active = to;
		} else if from < self.active && to >= self.active {
			self.active -= 1;
		} else if from > self.active && to <= self.active {
			self.active += 1;
		}
		true
	}
	pub(super) fn find(&self, path: &std::path::Path) -> Option<usize> {
		self.entries.iter().position(|tab| tab.path == path)
	}
	pub(super) fn open(&mut self, path: PathBuf, now: Instant) {
		// A switch ends whatever scroll was in flight in the old document.
		self.session.cancel_scroll_animation();
		if let Some(current) = self.session.path.clone() {
			if self.entries.is_empty() {
				self.entries.push(ReaderTab::new(current));
			} else {
				self.entries[self.active].session =
					std::mem::take(&mut self.session);
				self.entries[self.active].last_active = now;
			}
			self.entries.push(ReaderTab::new(path.clone()));
			self.active = self.entries.len() - 1;
		} else if self.entries.is_empty() {
			self.entries.push(ReaderTab::new(path.clone()));
			self.active = 0;
		}
		self.session.path = Some(path);
		self.session.content_version += 1;
		// A different document starts capped again.
		self.session.load_all_images = false;
		self.session.remote_notice_dismissed = false;
		self.session.scroll = 0.0;
		self.session.pending_anchor = None;
		self.session.jump_origin = None;
		self.session.horizontal.clear();
	}
	/// Queue a tab for first use without disturbing the active reader or worker.
	pub(super) fn open_background(
		&mut self,
		path: PathBuf,
		anchor: Option<String>,
	) -> bool {
		if self.session.path.is_none() || self.find(&path).is_some() {
			return false;
		}
		let mut tab = ReaderTab::new(path.clone());
		tab.session.path = Some(path);
		tab.session.content_version = 1;
		tab.session.load_all_images = false;
		tab.session.remote_notice_dismissed = false;
		tab.session.pending_anchor = anchor;
		self.entries.push(tab);
		true
	}
	/// Replace the heading anchor a tab will apply when it is next displayed.
	pub(super) fn queue_anchor(
		&mut self,
		index: usize,
		anchor: Option<String>,
	) {
		if index == self.active {
			self.session.pending_anchor = anchor;
		} else if let Some(tab) = self.entries.get_mut(index) {
			tab.session.pending_anchor = anchor;
		}
	}
	pub(super) fn select(&mut self, index: usize, now: Instant) -> bool {
		if index >= self.entries.len() || index == self.active {
			return false;
		}
		self.session.cancel_scroll_animation();
		if self.session.path.is_some() {
			self.entries[self.active].session =
				std::mem::take(&mut self.session);
			self.entries[self.active].last_active = now;
		}
		self.active = index;
		self.entries[index].last_active = now;
		self.session = std::mem::take(&mut self.entries[index].session);
		self.session.cancel_scroll_animation();
		if self.session.path.is_none() {
			self.session.path = Some(self.entries[index].path.clone());
		}
		true
	}
	pub(super) fn close(&mut self, index: usize, now: Instant) -> Closed {
		if index >= self.entries.len() {
			return Closed::Missing;
		}
		if index != self.active {
			self.entries.remove(index);
			if index < self.active {
				self.active -= 1;
			}
			return Closed::Inactive;
		}
		self.entries[index].session = std::mem::take(&mut self.session);
		self.entries.remove(index);
		if self.entries.is_empty() {
			self.active = 0;
			self.session = ReaderSession::default();
		} else {
			self.active = index.min(self.entries.len() - 1);
			self.session =
				std::mem::take(&mut self.entries[self.active].session);
			self.session.cancel_scroll_animation();
			self.entries[self.active].last_active = now;
		}
		Closed::Active
	}
	pub(super) fn request(
		&mut self,
		options: LayoutOptions,
		follow: bool,
	) -> Option<Request> {
		let path = self.session.path.clone()?;
		self.request_serial += 1;
		self.session.version = self.request_serial;
		self.session.follow_update |= follow;
		self.session.requested_options = Some(options.clone());
		self.session.layout_pending = true;
		Some(Request {
			version: self.session.version,
			content_version: self.session.content_version,
			path,
			options,
			requested: Instant::now(),
			coverage: f32::INFINITY,
			load_all_images: self.session.load_all_images,
		})
	}
	pub(super) fn release_inactive(&mut self, now: Instant) {
		for (index, tab) in self.entries.iter_mut().enumerate() {
			if index != self.active
				&& now.duration_since(tab.last_active) >= RELEASE_AFTER
				&& tab.session.document.is_some()
			{
				tab.session.release_heavy();
			}
		}
	}
	pub(super) fn release_deadline(&self) -> Option<Instant> {
		self.entries
			.iter()
			.filter(|tab| tab.session.document.is_some())
			.map(|tab| tab.last_active + RELEASE_AFTER)
			.min()
	}
}

#[cfg(test)]
mod tests;
