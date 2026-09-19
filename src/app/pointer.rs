use crate::{link, state};
use markview_core::document::footnote;
use std::{
	path::PathBuf,
	time::{Duration, Instant},
};
use winit::window::CursorIcon;

use super::{App, anchor, chrome};
impl App {
	pub(super) fn pointer_in_panel(&self) -> bool {
		let (width, height, _) = self.dimensions();
		let rect = if self.interaction.styles_open
			|| self.interaction.export_styles_open
		{
			chrome::styles_rect(
				width,
				height,
				self.preferences.style_entries.len(),
			)
		} else {
			chrome::panel_rect(width, height)
		};
		self.interaction.panel_open
			&& rect
				.contains(self.interaction.cursor.0, self.interaction.cursor.1)
	}
	pub(super) fn panel_has_focus(&self) -> bool {
		self.interaction.panel_open
	}
	pub(super) fn scroll_by(&mut self, dy: f32) {
		self.readers.session.scroll_by(dy, self.viewport());
		self.worker
			.prioritize(self.readers.session.coverage(self.viewport()));
		self.refresh_hover();
		self.redraw();
	}
	/// The link under a window point, using the same origin as the renderer.
	pub(super) fn link_at(&self, px: f32, py: f32) -> Option<String> {
		let geometry = self.view_geometry();
		if !geometry.clip().contains(px, py) {
			return None;
		}
		let (x, y) = geometry.document_point(px, py);
		self.readers
			.session
			.snapshot
			.link_at(x, y, &self.readers.session.horizontal)
			.map(str::to_string)
	}
	pub(super) fn button_at_cursor(&mut self) -> bool {
		self.buttons().into_iter().any(|button| {
			button
				.rect
				.contains(self.interaction.cursor.0, self.interaction.cursor.1)
		})
	}

	/// Hover state follows scrolling and reflow, not only pointer motion.
	pub(super) fn refresh_hover(&mut self) {
		let holding = self.interaction.pointer_down.is_some()
			|| self.interaction.scrollbar.is_some()
			|| self.tab_strip.drag.is_some();
		let idle = !self.interaction.panel_open
			&& self.interaction.modal.is_none()
			&& !holding;
		let hover = if idle {
			self.link_at(self.interaction.cursor.0, self.interaction.cursor.1)
		} else {
			None
		};
		// Wide-block scrollbars live in the middle of the window, so pointer
		// motion alone does not repaint them: their hover state is tracked
		// here and drives the redraw.
		let hover_overflow = if idle {
			self.overflow_scrollbar_at(
				self.interaction.cursor.0,
				self.interaction.cursor.1,
			)
			.map(|(block, overflow, _)| (block, overflow))
		} else {
			None
		};
		let cursor = if self.tab_strip.drag.is_some_and(|d| d.moving) {
			CursorIcon::Grabbing
		} else if self.interaction.scrollbar.is_some() {
			CursorIcon::Default
		} else if self.interaction.pointer_down.is_some() {
			if self.text_under_cursor() {
				CursorIcon::Text
			} else {
				CursorIcon::Default
			}
		} else if self.button_at_cursor()
			|| self.tab_at_cursor().is_some()
			|| hover.is_some()
		{
			CursorIcon::Pointer
		} else if !self.interaction.panel_open && self.text_under_cursor() {
			CursorIcon::Text
		} else {
			CursorIcon::Default
		};
		let hover_changed = hover != self.interaction.hover
			|| hover_overflow != self.interaction.hover_overflow;
		let geometry = self.view_geometry();
		let (x, y) = geometry.document_point(
			self.interaction.cursor.0,
			self.interaction.cursor.1,
		);
		let hover_image = if idle
			&& geometry
				.clip()
				.contains(self.interaction.cursor.0, self.interaction.cursor.1)
		{
			self.readers
				.session
				.snapshot
				.image_title_at(x, y, &self.readers.session.horizontal)
				.map(str::to_owned)
		} else {
			None
		};
		let hover_changed =
			hover_changed || hover_image != self.interaction.hover_image;
		self.interaction.hover_image = hover_image;
		self.interaction.hover = hover;
		self.interaction.hover_overflow = hover_overflow;
		if let Some(w) = &self.window {
			w.set_cursor(cursor);
		}
		if hover_changed {
			self.redraw();
		}
	}
	pub(super) fn open_link(&mut self, url: &str, background: bool) {
		let fragment = anchor::link_fragment(url);
		if anchor::link_target(url).is_empty() {
			// A bare fragment addresses the current document.
			if let Some(fragment) = fragment {
				if let Some(label) = footnote::back_label(&fragment) {
					self.return_from_footnote(label);
				} else {
					self.goto_anchor(fragment);
				}
			}
			return;
		}
		let directory = self
			.readers
			.session
			.path
			.as_deref()
			.and_then(std::path::Path::parent);
		match crate::link::resolve(url, directory) {
			Some(link::Target::Markdown(path)) => {
				if background {
					if let Some(index) = self.readers.find(&path) {
						self.readers.queue_anchor(index, fragment);
						if index == self.readers.active() {
							self.apply_anchor();
						}
						self.redraw();
					} else if self.readers.open_background(path, fragment) {
						self.redraw();
					}
				} else {
					self.open(path);
					if let Some(fragment) = fragment {
						self.goto_anchor(fragment);
					}
				}
			}
			Some(link::Target::Remote(link)) => self.launch(&link),
			Some(link::Target::OsDirect(path)) => {
				self.launch(&path.display().to_string())
			}
			Some(link::Target::Confirm(path)) => {
				let dir = path
					.parent()
					.filter(|p| !p.as_os_str().is_empty())
					.map_or_else(|| PathBuf::from("."), PathBuf::from);
				self.interaction.modal = Some(state::Modal::OpenLocal {
					path,
					dir,
					document_dir: directory.map(std::path::Path::to_path_buf),
				});
				// "Open folder" is the default, and Enter activates it.
				self.interaction.focus = Some(state::Command::ModalOpenFolder);
				self.refresh_hover();
				self.redraw();
			}
			None => {
				self.error = true;
				self.status = format!("Not opened: {url}");
				self.status_until =
					Some(Instant::now() + Duration::from_secs(4));
				self.redraw();
			}
		}
	}

	/// Hands an already-approved target to the operating system.
	pub(super) fn launch(&mut self, target: &str) {
		self.error = false;
		self.status = match open::that_detached(target) {
			Ok(()) => format!("Opened {target}"),
			Err(error) => {
				self.error = true;
				format!("Cannot open {target}: {error}")
			}
		};
		self.status_until = Some(Instant::now() + Duration::from_secs(4));
		self.redraw();
	}

	pub(super) fn horizontal_by(&mut self, dx: f32) {
		let (cx, cy) = self.view_geometry().document_point(
			self.interaction.cursor.0,
			self.interaction.cursor.1,
		);
		for (bi, b) in self.readers.session.snapshot.blocks.iter().enumerate() {
			for (oi, o) in b.layout.overflow.iter().enumerate() {
				if o.rect.contains(cx, cy - b.y) {
					let offset = self
						.readers
						.session
						.horizontal
						.entry((bi, oi))
						.or_default();
					*offset = (*offset + dx)
						.clamp(0.0, (o.content_width - o.rect.w).max(0.0));
					self.refresh_hover();
					self.redraw();
					return;
				}
			}
		}
	}
}
