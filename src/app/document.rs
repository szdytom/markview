use super::tabs;
use crate::watch::{FileWatch, file_stamp};
use std::{
	path::{Path, PathBuf},
	time::Instant,
};

use super::modal::Message;
use super::viewer::Message as ViewerMessage;
use super::{App, Event};
use crate::state::Selection;
impl<P: super::SendEvent> App<P> {
	pub(super) fn request(&mut self, follow: bool) {
		self.update_media();
		if let Some(mut request) = self.readers.request(self.options(), follow)
		{
			if self.readers.session.accepted_revision != request.content_version
			{
				self.readers.session.source_stamp =
					Some(file_stamp(&request.path));
			}
			request.coverage = if follow
				&& self.readers.session.scrolling.offset
					>= (self.readers.session.snapshot.height
						- self.viewport() - 3.)
						.max(0.)
			{
				self.readers.session.scrolling.target = Some(f32::INFINITY);
				f32::INFINITY
			} else {
				self.readers.session.coverage(self.viewport())
			};
			self.status.set(
				self.preferences.values.lang().status_updating(),
				false,
				0,
			);
			self.worker.submit(request);
			self.redraw();
		}
	}
	pub(super) fn observe_document(&mut self) {
		self.watch = self
			.readers
			.session
			.path
			.clone()
			.filter(|_| !self.readers.session.web_loading)
			.map(|path| {
				let proxy = self.proxy.clone();
				let observed = path.clone();
				FileWatch::new(path, move || {
					proxy.send(Event::Changed(observed.clone()));
				})
			});
	}
	pub(super) fn reload_document(&mut self, path: &Path, follow: bool) {
		self.cancel_gestures();
		self.abandon_dm();
		let session = &mut self.readers.session;
		session.content_version += 1;
		session.security.revoke();
		session.blocked_images.clear();
		self.interaction.overlays.modal.send(Message::Dismiss);
		session.load_error = None;
		session.parse_complete = false;
		session.search.retained = session
			.search
			.current
			.and_then(|i| session.search.matches.get(i))
			.cloned();
		session.search.document = None;
		session.search.matches = Default::default();
		// New content resets remote-image choices and `<details>` overrides.
		session.load_all_images = false;
		session.remote_notice_dismissed = false;
		session.details_open = Default::default();
		session.cancel_scroll_animation();
		self.search_changed();
		self.request(follow);
		self.export.schedule_watch_export(path);
	}
	fn resume_document(&mut self) {
		self.observe_document();
		let session = &self.readers.session;
		if !session.web_loading
			&& let Some(path) = session.path.clone()
			&& session
				.source_stamp
				.is_some_and(|stamp| stamp != file_stamp(&path))
		{
			self.reload_document(&path, false);
			return;
		}
		// The worker releases inactive pixels, so image tabs must resume loading.
		if session.document.is_none()
			|| session.layout_pending
			|| !session.snapshot.images.entries.is_empty()
			|| session.requested_options.as_ref() != Some(&self.options())
		{
			self.request(false);
		}
	}
	pub(super) fn open(&mut self, path: PathBuf) {
		if path.is_dir() {
			self.restore_session();
			self.open_folder(path, true);
			return;
		}
		self.open_document(
			path,
			crate::security::Security::local(crate::security::Trust::Trusted),
		);
	}
	pub(super) fn open_document(
		&mut self,
		path: PathBuf,
		security: crate::security::Security,
	) {
		self.open_path(path, false, security);
	}
	pub(super) fn open_pending(
		&mut self,
		path: PathBuf,
		security: crate::security::Security,
	) {
		self.open_path(path, true, security);
	}
	fn open_path(
		&mut self,
		path: PathBuf,
		web_loading: bool,
		security: crate::security::Security,
	) {
		self.restore_session();
		self.folder.navigation_revision += 1;
		self.interaction.overlays.modal.send(Message::Dismiss);
		self.clear_input_focus();
		self.cancel_gestures();
		self.abandon_dm();
		self.tab_strip.cancel_drag();
		self.tab_strip.reveal_active = true;
		let path = if path.is_absolute() {
			path
		} else {
			std::env::current_dir().unwrap_or_default().join(path)
		};
		let path = std::fs::canonicalize(&path).unwrap_or(path);
		if let Some(index) = self.readers.find_origin(&path, &security.origin) {
			self.select_tab(index);
			if self.readers.session.load_error.take().is_some() {
				self.request(false);
			}
			return;
		}
		self.close_search();
		self.worker.cancel();
		self.readers.open(path, Instant::now());
		self.readers.session.security = security;
		self.readers.session.web_loading = web_loading;
		self.readers.session.layout_pending = true;
		self.status.clear_message();
		self.status.set_error(false);
		self.interaction.clear_selection();
		if web_loading {
			self.watch = None;
			self.redraw();
		} else {
			self.observe_document();
			self.request(false);
		}
	}
	pub(super) fn new_page(&mut self) {
		self.restore_session();
		self.folder.navigation_revision += 1;
		self.interaction.overlays.modal.send(Message::Dismiss);
		self.clear_input_focus();
		self.cancel_gestures();
		self.abandon_dm();
		self.tab_strip.cancel_drag();
		self.tab_strip.reveal_active = false;
		self.close_search();
		self.readers.new_page(Instant::now());
		self.interaction.overlays.viewer.send(ViewerMessage::Close);
		self.worker.cancel();
		self.search_worker.release();
		self.watch = None;
		self.interaction.clear_selection();
		self.interaction.overlays.outline.set_open(false);
		self.show_panel(crate::state::PanelPage::Closed);
		self.status.clear();
		if let Some(window) = &self.window {
			window.set_title("Markview");
		}
		self.refresh_hover();
		self.redraw();
	}
	pub(super) fn select_tab(&mut self, index: usize) {
		self.folder.navigation_revision += 1;
		if self.folder_inset() == 0. {
			self.set_folder_open(false);
		}
		self.interaction.overlays.modal.send(Message::Dismiss);
		self.clear_input_focus();
		self.cancel_gestures();
		self.abandon_dm();
		self.tab_strip.cancel_drag();
		self.tab_strip.reveal_active = true;
		if self.readers.select(index, Instant::now()) {
			// The viewer showed the page that was active, so a tab switch
			// closes it rather than leaving the new document under it.
			self.interaction.overlays.viewer.send(ViewerMessage::Close);
			// A card describes the tab it was opened for, so a new active tab
			// closes it; the resting pointer re-arms it on its next move.
			self.dismiss_preview();
			self.interaction.clear_selection();
			self.worker.cancel();
			self.close_search();
			self.search_changed();
			self.status.clear();
			self.resume_document();
		}
		// The tree follows the clicked tab, so a collapsed branch cannot
		// hide its entry.
		self.expand_folder_to_tab();
		self.apply_anchor();
		self.redraw();
	}
	pub(super) fn close_tab(&mut self, index: usize) {
		self.interaction.overlays.modal.send(Message::Dismiss);
		self.clear_input_focus();
		self.cancel_gestures();
		self.abandon_dm();
		self.tab_strip.cancel_drag();
		self.tab_strip.reveal_active = true;
		let Some(closed) = self
			.readers
			.entries()
			.get(index)
			.map(|tab| tab.path.clone())
		else {
			return;
		};
		let origin = self.readers.session_mut(index).security.origin.clone();
		self.dismiss_preview();
		let result = self.readers.close(index, Instant::now());
		// A watched export belongs to the document and origin that chose it.
		if self.export.watches_document(&closed, &origin) {
			self.export.disarm_watch();
		}
		match result {
			tabs::Closed::Missing => return,
			tabs::Closed::Inactive if !self.readers.entries().is_empty() => {
				self.redraw();
				return;
			}
			tabs::Closed::Active | tabs::Closed::Inactive => {}
		}
		self.folder.navigation_revision += 1;
		self.watch = None;
		if self.readers.entries().is_empty() {
			// No tab is left, so the worker can drop the document it kept for
			// the closed one instead of holding it until the next open. The
			// export panel has nothing to export either.
			self.worker.release();
			self.search_worker.release();
			self.show_panel(crate::state::PanelPage::Closed);
			if let Some(window) = &self.window {
				window.set_title("Markview");
			}
		} else {
			self.worker.cancel();
		}
		self.interaction.clear_selection();
		self.close_search();
		self.search_changed();
		self.status.clear();
		if !self.readers.entries().is_empty()
			&& self.readers.session.path.is_some()
		{
			self.resume_document();
		}
		self.redraw();
	}
}

#[cfg(test)]
mod tests;
