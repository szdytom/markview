use super::tabs;
use crate::watch::FileWatch;
use std::{path::PathBuf, time::Instant};

use super::{App, Event};
impl App {
	pub(super) fn request(&mut self, follow: bool) {
		if let Some(mut request) = self.readers.request(self.options(), follow)
		{
			request.coverage = if follow
				&& self.readers.session.scroll
					>= (self.readers.session.snapshot.height
						- self.viewport() - 3.)
						.max(0.)
			{
				self.readers.session.pending_scroll = Some(f32::INFINITY);
				f32::INFINITY
			} else {
				self.readers.session.coverage(self.viewport())
			};
			self.error = false;
			self.status_until = None;
			self.status = "Updating…".into();
			self.worker.submit(request);
			self.redraw();
		}
	}
	pub(super) fn observe_document(&mut self) {
		self.watch = self.readers.session.path.clone().map(|path| {
			let proxy = self.proxy.clone();
			let observed = path.clone();
			FileWatch::new(path, move || {
				let _ = proxy.send_event(Event::Changed(observed.clone()));
			})
		});
	}
	pub(super) fn open(&mut self, path: PathBuf) {
		self.tab_strip.cancel_drag();
		self.tab_strip.reveal_active = true;
		let path = if path.is_absolute() {
			path
		} else {
			std::env::current_dir().unwrap_or_default().join(path)
		};
		let path = std::fs::canonicalize(&path).unwrap_or(path);
		if let Some(index) = self.readers.find(&path) {
			self.select_tab(index);
			return;
		}
		self.readers.open(path, Instant::now());
		self.interaction.clear_selection();
		self.observe_document();
		self.request(false);
	}
	pub(super) fn select_tab(&mut self, index: usize) {
		self.tab_strip.cancel_drag();
		self.tab_strip.reveal_active = true;
		if !self.readers.select(index, Instant::now()) {
			self.apply_anchor();
			self.redraw();
			return;
		}
		self.observe_document();
		self.interaction.clear_selection();
		self.worker.cancel();
		self.error = false;
		self.status.clear();
		self.status_until = None;
		if self.readers.session.document.is_none()
			|| self.readers.session.layout_pending
			|| self.readers.session.requested_options.as_ref()
				!= Some(&self.options())
		{
			self.request(false);
		}
		self.apply_anchor();
		self.redraw();
	}
	pub(super) fn close_tab(&mut self, index: usize) {
		self.tab_strip.cancel_drag();
		self.tab_strip.reveal_active = true;
		let closed = self.readers.session.path.clone();
		match self.readers.close(index, Instant::now()) {
			tabs::Closed::Missing => return,
			tabs::Closed::Inactive => {
				self.redraw();
				return;
			}
			tabs::Closed::Active => {}
		}
		self.watch = None;
		// A watched export belongs to the document that chose it.
		if self
			.watch_export
			.as_ref()
			.is_some_and(|watch| Some(&watch.source) == closed.as_ref())
		{
			self.watch_export = None;
			self.watch_at = None;
		}
		if self.readers.entries().is_empty() {
			// No tab is left, so the worker can drop the document it kept for
			// the closed one instead of holding it until the next open. The
			// export panel has nothing to export either.
			self.worker.release();
			self.interaction.panel_open = false;
			self.interaction.export_open = false;
			self.interaction.export_styles_open = false;
			self.interaction.styles_open = false;
			if let Some(window) = &self.window {
				window.set_title("Markview");
			}
		} else {
			self.worker.cancel();
		}
		self.interaction.clear_selection();
		self.error = false;
		self.status.clear();
		self.status_until = None;
		if !self.readers.entries().is_empty()
			&& self.readers.session.path.is_some()
		{
			self.observe_document();
			if self.readers.session.document.is_none()
				|| self.readers.session.layout_pending
				|| self.readers.session.requested_options.as_ref()
					!= Some(&self.options())
			{
				self.request(false);
			}
		}
		self.redraw();
	}
}
