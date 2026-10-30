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
	/// Recently used tab ids, most recent first.
	mru: Vec<u64>,
	/// The fixed MRU snapshot a held Ctrl+Tab is walking through.
	cycle: Option<TabCycle>,
}

#[derive(Default)]
struct TabCycle {
	order: Vec<u64>,
	index: usize,
}

pub(super) enum Closed {
	Missing,
	Inactive,
	Active,
}
impl Tabs {
	pub(super) fn restore_tab(
		&mut self,
		path: PathBuf,
		reading: super::session::Reading,
	) -> usize {
		let mut tab = ReaderTab::new(path.clone());
		tab.session.security =
			crate::security::Security::local(crate::security::Trust::Trusted);
		tab.session.path = Some(path);
		tab.session.content_version = 1;
		tab.session.details_open = std::sync::Arc::new(reading.details.clone());
		tab.session.saved_reading = Some(reading);
		self.entries.push(tab);
		if self.entries.len() == 1 {
			self.session = std::mem::take(&mut self.entries[0].session);
		}
		self.entries.len() - 1
	}

	pub(super) fn entries(&self) -> &[ReaderTab] {
		&self.entries
	}
	fn entry_index(&self, id: u64) -> Option<usize> {
		self.entries.iter().position(|tab| tab.id == id)
	}
	fn active_id(&self) -> Option<u64> {
		self.entries.get(self.active()).map(|tab| tab.id)
	}
	/// Moves `id` to the front of the recent order.
	fn remember(&mut self, id: u64) {
		self.mru.retain(|recent| *recent != id);
		self.mru.insert(0, id);
	}
	/// Every tab in MRU order, with the active tab first and tabs never used
	/// appended in strip order for the Ctrl+Tab cycle.
	pub(super) fn recent_order(&self) -> Vec<u64> {
		let mut order = Vec::with_capacity(self.entries.len());
		if let Some(id) = self.active_id() {
			order.push(id);
		}
		for id in &self.mru {
			if self.entry_index(*id).is_some() && !order.contains(id) {
				order.push(*id);
			}
		}
		for tab in &self.entries {
			if !order.contains(&tab.id) {
				order.push(tab.id);
			}
		}
		order
	}
	/// Restores a saved recent order and appends tabs it did not mention.
	pub(super) fn set_recent(&mut self, ids: Vec<u64>) {
		let mut order = Vec::with_capacity(self.entries.len());
		for id in ids {
			if self.entry_index(id).is_some() && !order.contains(&id) {
				order.push(id);
			}
		}
		for tab in &self.entries {
			if !order.contains(&tab.id) {
				order.push(tab.id);
			}
		}
		self.mru = order;
		self.cycle = None;
	}
	pub(super) fn remember_active(&mut self) {
		if let Some(id) = self.active_id() {
			self.remember(id);
		}
	}
	/// Advances the held Ctrl+Tab cycle and returns its next tab.
	pub(super) fn cycle(&mut self, forward: bool) -> Option<usize> {
		if self.entries.len() < 2 || self.active_id().is_none() {
			return None;
		}
		let order = self
			.cycle
			.as_ref()
			.map_or_else(|| self.recent_order(), |cycle| cycle.order.clone());
		if order.len() < 2 {
			return None;
		}
		let current = self.cycle.as_ref().map_or(0, |cycle| cycle.index);
		let index = if forward {
			(current + 1) % order.len()
		} else {
			(current + order.len() - 1) % order.len()
		};
		let target = self.entry_index(order[index])?;
		self.cycle = Some(TabCycle { order, index });
		Some(target)
	}
	pub(super) fn cycling(&self) -> bool {
		self.cycle.is_some()
	}
	/// Ends a held cycle and records only the tab it settled on.
	pub(super) fn commit_cycle(&mut self) {
		if self.cycle.take().is_some() {
			self.remember_active();
		}
	}
	pub(super) fn session_mut(&mut self, index: usize) -> &mut ReaderSession {
		if index == self.active() {
			&mut self.session
		} else {
			&mut self.entries[index].session
		}
	}
	pub(super) fn rename(&mut self, index: usize, path: PathBuf) {
		self.entries[index].path = path.clone();
		self.session_mut(index).path = Some(path);
	}
	/// `usize::MAX` leaves every tab unselected on the empty page.
	pub(super) fn active(&self) -> usize {
		if self.session.path.is_some() {
			self.active
		} else {
			usize::MAX
		}
	}
	pub(super) fn new_page(&mut self, now: Instant) {
		self.cycle = None;
		self.session.cancel_scroll_animation();
		if self.session.path.is_some() {
			self.entries[self.active].preview =
				super::preview::Page::capture(&self.session);
			self.entries[self.active].session =
				std::mem::take(&mut self.session);
			self.entries[self.active].last_active = now;
		}
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
	#[cfg(test)]
	pub(super) fn find(&self, path: &std::path::Path) -> Option<usize> {
		self.entries.iter().position(|tab| tab.path == path)
	}
	pub(super) fn find_web(&self, path: &std::path::Path) -> Option<usize> {
		self.entries.iter().enumerate().find_map(|(index, tab)| {
			let session = if index == self.active() {
				&self.session
			} else {
				&tab.session
			};
			(tab.path == path
				&& matches!(
					session.security.origin,
					crate::security::Origin::Web(_)
				))
			.then_some(index)
		})
	}
	pub(super) fn find_origin(
		&self,
		path: &std::path::Path,
		origin: &crate::security::Origin,
	) -> Option<usize> {
		self.entries.iter().enumerate().find_map(|(index, tab)| {
			let session = if index == self.active() {
				&self.session
			} else {
				&tab.session
			};
			(tab.path == path && &session.security.origin == origin)
				.then_some(index)
		})
	}
	pub(super) fn open(&mut self, path: PathBuf, now: Instant) {
		// A switch ends whatever scroll was in flight in the old document.
		self.session.cancel_scroll_animation();
		let index = if let Some(current) = self.session.path.clone() {
			if self.entries.is_empty() {
				self.entries.push(ReaderTab::new(current));
			} else {
				self.entries[self.active].preview =
					super::preview::Page::capture(&self.session);
				self.entries[self.active].session =
					std::mem::take(&mut self.session);
				self.entries[self.active].last_active = now;
			}
			self.active + 1
		} else {
			self.entries.len()
		};
		self.entries.insert(index, ReaderTab::new(path.clone()));
		self.active = index;
		self.remember(self.entries[index].id);
		self.cycle = None;
		self.session.path = Some(path);
		self.session.security =
			crate::security::Security::local(crate::security::Trust::Trusted);
		self.session.content_version += 1;
		// A different document starts capped again.
		self.session.load_all_images = false;
		self.session.remote_notice_dismissed = false;
		self.session.scrolling.offset = 0.0;
		self.session.pending_anchor = None;
		self.session.jump_origin = None;
		self.session.horizontal.clear();
	}
	/// Queue a tab for first use without disturbing the active reader or worker.
	#[cfg(test)]
	pub(super) fn open_background(
		&mut self,
		path: PathBuf,
		anchor: Option<String>,
	) -> bool {
		self.open_background_secure(
			path,
			anchor,
			crate::security::Security::local(crate::security::Trust::Trusted),
		)
	}
	pub(super) fn open_background_secure(
		&mut self,
		path: PathBuf,
		anchor: Option<String>,
		security: crate::security::Security,
	) -> bool {
		if self.session.path.is_none()
			|| self.find_origin(&path, &security.origin).is_some()
		{
			return false;
		}
		let mut tab = ReaderTab::new(path.clone());
		tab.session.path = Some(path);
		tab.session.content_version = 1;
		tab.session.load_all_images = false;
		tab.session.remote_notice_dismissed = false;
		tab.session.pending_anchor = anchor;
		tab.session.security = security;
		let index = self.active + 1;
		self.entries.insert(index, tab);
		true
	}
	/// Replace the heading anchor a tab will apply when it is next displayed.
	pub(super) fn queue_anchor(
		&mut self,
		index: usize,
		anchor: Option<String>,
	) {
		let session = if index == self.active() {
			&mut self.session
		} else if let Some(tab) = self.entries.get_mut(index) {
			&mut tab.session
		} else {
			return;
		};
		if anchor.is_some() {
			session.saved_reading = None;
		}
		session.pending_anchor = anchor;
	}
	pub(super) fn select(&mut self, index: usize, now: Instant) -> bool {
		self.activate(index, now, true)
	}
	pub(super) fn select_untracked(
		&mut self,
		index: usize,
		now: Instant,
	) -> bool {
		self.activate(index, now, false)
	}
	fn activate(&mut self, index: usize, now: Instant, track: bool) -> bool {
		if index >= self.entries.len() || index == self.active() {
			return false;
		}
		self.session.cancel_scroll_animation();
		if self.session.path.is_some() {
			self.entries[self.active].preview =
				super::preview::Page::capture(&self.session);
			self.entries[self.active].session =
				std::mem::take(&mut self.session);
			self.entries[self.active].last_active = now;
		}
		self.active = index;
		self.entries[index].last_active = now;
		self.entries[index].preview = None;
		self.session = std::mem::take(&mut self.entries[index].session);
		self.session.cancel_scroll_animation();
		if self.session.path.is_none() {
			self.session.path = Some(self.entries[index].path.clone());
		}
		if track {
			self.cycle = None;
			self.remember(self.entries[index].id);
		}
		true
	}
	pub(super) fn close(&mut self, index: usize, now: Instant) -> Closed {
		if index >= self.entries.len() {
			return Closed::Missing;
		}
		let closed = self.entries[index].id;
		self.mru.retain(|id| *id != closed);
		self.cycle = None;
		if index != self.active() {
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
			self.mru.clear();
		} else {
			self.mru
				.retain(|id| self.entries.iter().any(|tab| tab.id == *id));
			self.active = self
				.mru
				.first()
				.and_then(|id| self.entry_index(*id))
				.unwrap_or_else(|| index.min(self.entries.len() - 1));
			self.entries[self.active].preview = None;
			self.session =
				std::mem::take(&mut self.entries[self.active].session);
			self.session.cancel_scroll_animation();
			self.entries[self.active].last_active = now;
			self.remember(self.entries[self.active].id);
		}
		Closed::Active
	}
	pub(super) fn request(
		&mut self,
		options: LayoutOptions,
		follow: bool,
	) -> Option<Request> {
		if self.session.web_loading || self.session.load_error.is_some() {
			return None;
		}
		let path = self.session.path.clone()?;
		self.request_serial += 1;
		self.session.version = self.request_serial;
		self.session.follow_update |= follow;
		self.session.requested_options = Some(options.clone());
		self.session.layout_pending = true;
		Some(Request {
			security: self.session.security.clone(),
			version: self.session.version,
			content_version: self.session.content_version,
			path,
			workspace: None,
			options,
			requested: Instant::now(),
			coverage: f32::INFINITY,
			load_all_images: self.session.load_all_images,
		})
	}
	pub(super) fn release_inactive(&mut self, now: Instant) {
		let active = self.active();
		for (index, tab) in self.entries.iter_mut().enumerate() {
			if index != active
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
