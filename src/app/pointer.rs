use crate::state::Selection;
use crate::{link, state};
use markview_core::document::footnote;
use std::{
	path::PathBuf,
	time::{Duration, Instant},
};
use winit::{event::MouseScrollDelta, window::CursorIcon};

use super::{App, LINE_STEP, anchor, chrome};
use crate::platform::{WheelAmount, WheelNotch};

/// The logical pixels one notch of `amount` travels along an axis `extent` long.
///
/// A character borrows the line step: Windows names lines and characters in
/// separate settings, but the two only differ in the count they carry.
fn notch_pixels(amount: WheelAmount, extent: f32) -> f32 {
	match amount {
		WheelAmount::Lines(lines) => LINE_STEP * lines,
		WheelAmount::Page => extent * 0.9,
	}
}

/// The logical pixels one wheel event asks the reader to move.
///
/// A line delta is a notch on X11 and Wayland, where the desktop's own speed
/// setting never reaches the event, and an already-scaled line on macOS, where
/// it does, so the notch only multiplies where the platform left it raw. Each
/// axis carries the desktop's own value, because Windows configures the two
/// separately. A pixel delta is physical, so the display scale turns it back
/// into logical pixels. The reader's speed multiplier scales both.
pub(super) fn wheel_pixels(
	delta: MouseScrollDelta,
	notch: WheelNotch,
	speed: f32,
	scale: f32,
	viewport: (f32, f32),
) -> (f32, f32) {
	match delta {
		MouseScrollDelta::LineDelta(x, y) => (
			x * notch_pixels(notch.horizontal, viewport.0) * speed,
			y * notch_pixels(notch.vertical, viewport.1) * speed,
		),
		MouseScrollDelta::PixelDelta(p) => {
			let step = speed / scale;
			(p.x as f32 * step, p.y as f32 * step)
		}
	}
}

/// Whether a wheel event comes from a high-resolution device.
///
/// A mouse wheel names whole detents. A touchpad emulating one through
/// Windows' high-resolution wheel support names the fractional counts it
/// accumulated instead, and the same device hands its inertia over in a few
/// large packets rather than as the stream a desktop's own smooth scrolling
/// would give.
pub(super) fn high_resolution(delta: MouseScrollDelta) -> bool {
	match delta {
		MouseScrollDelta::LineDelta(x, y) => {
			x.fract() != 0.0 || y.fract() != 0.0
		}
		MouseScrollDelta::PixelDelta(_) => false,
	}
}

/// Whether a `dy` of input has a finite distance to ease over.
fn eases(dy: f32) -> bool {
	dy.is_finite() && dy != 0.0
}

impl<P: super::SendEvent> App<P> {
	pub(super) fn pointer_in_panel(&self) -> bool {
		let (width, height, _) = self.dimensions();
		let rect = chrome::panel_rect(width, height);
		self.interaction.panel_open()
			&& rect
				.contains(self.interaction.cursor.0, self.interaction.cursor.1)
	}
	pub(super) fn panel_has_focus(&self) -> bool {
		self.interaction.panel_open()
	}
	/// The reader's scroll-speed multiplier over the desktop's own speed.
	pub(super) fn scroll_speed(&self) -> f32 {
		self.preferences.values.scroll_speed
	}
	/// Logical pixels one line of a discrete scroll travels.
	pub(super) fn line_step(&self) -> f32 {
		LINE_STEP * self.scroll_speed()
	}
	pub(super) fn scroll_by(&mut self, dy: f32) {
		self.readers.session.scroll_by(dy, self.viewport());
		self.after_scroll();
	}
	/// A discrete scroll step, eased to its destination.
	pub(super) fn scroll_step(&mut self, dy: f32) {
		if eases(dy) {
			self.readers.session.animate_scroll_by(dy, Instant::now());
		} else {
			self.readers.session.scroll_by(dy, self.viewport());
		}
		self.after_scroll();
	}
	/// A wheel travel, eased like a discrete step.
	pub(super) fn scroll_wheel(&mut self, dy: f32) {
		if eases(dy) {
			self.readers.session.animate_wheel_by(dy, Instant::now());
		} else {
			self.readers.session.scroll_by(dy, self.viewport());
		}
		self.after_scroll();
	}
	/// A wheel travel from a high-resolution device, which arrives in packets.
	pub(super) fn scroll_wheel_packet(&mut self, dy: f32) {
		if eases(dy) {
			self.readers.session.coast_wheel_by(dy, Instant::now());
		} else {
			self.readers.session.scroll_by(dy, self.viewport());
		}
		self.after_scroll();
	}
	/// Home and End. The top is known before the geometry is, so it eases to
	/// zero; the end is only a number once the snapshot is complete, and until
	/// then the existing infinite target waits for the final height.
	pub(super) fn scroll_bound(&mut self, to_end: bool) {
		let destination = if to_end {
			(self.readers.session.snapshot_complete
				&& !self.readers.session.layout_pending)
				.then(|| {
					crate::state::scroll_limit(
						self.readers.session.snapshot.height,
						self.viewport(),
					)
				})
		} else {
			Some(0.0)
		};
		if let Some(destination) = destination
			&& (destination - self.readers.session.scrolling.offset).abs() > 0.5
		{
			self.readers
				.session
				.animate_scroll_to(destination, Instant::now());
			self.after_scroll();
			return;
		}
		self.scroll_by(if to_end {
			f32::INFINITY
		} else {
			f32::NEG_INFINITY
		});
	}
	/// Steps a running scroll animation and asks for the next frame.
	pub(super) fn advance_scroll(&mut self, now: Instant) {
		if !self.readers.session.scroll_animating() {
			return;
		}
		if self.readers.session.advance_scroll(now, self.viewport()) {
			self.worker
				.prioritize(self.readers.session.coverage(self.viewport()));
		}
		self.follow_scroll();
		self.refresh_hover();
		self.redraw();
	}
	/// Re-prioritizes the worker, follows the pointer and repaints after the
	/// offset moved.
	pub(super) fn after_scroll(&mut self) {
		self.follow_scroll();
		self.worker
			.prioritize(self.readers.session.coverage(self.viewport()));
		self.refresh_hover();
		self.redraw();
	}
	/// The text moves under a pointer that has not, so a press in flight now
	/// covers different reading text. No pointer event arrives to say so: the
	/// pointer never moved, which is why every path that moves the offset has
	/// to say it instead.
	fn follow_scroll(&mut self) {
		if self.interaction.pointer_down.is_some() {
			self.update_drag();
		}
	}
	/// The link under a window point, using the same origin as the renderer.
	pub(super) fn link_at(&self, px: f32, py: f32) -> Option<String> {
		self.document_interaction()
			.link(markview_selection::Point::new(px, py))
			.map(str::to_owned)
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
		if let Some(viewer) = &self.interaction.viewer {
			self.interaction.hover = None;
			self.interaction.hover_image = None;
			self.interaction.hover_overflow = None;
			if let Some(w) = &self.window {
				w.set_cursor(if viewer.grab.is_some() {
					CursorIcon::Grabbing
				} else {
					CursorIcon::Default
				});
			}
			return;
		}
		let holding = self.interaction.pointer_down.is_some()
			|| self.interaction.scrollbar.is_some()
			|| self.tab_strip.drag.is_some();
		// The drawer covers document content, so it must not inherit the link
		// or image hover underneath it.
		let over_outline = self.pointer_in_outline();
		let idle = !self.interaction.panel_open()
			&& self.interaction.modal.is_none()
			&& !holding
			&& !over_outline;
		let document_hover = if idle {
			self.document_interaction().hover(
				markview_selection::Point::new(
					self.interaction.cursor.0,
					self.interaction.cursor.1,
				),
				false,
				true,
				self.preferences
					.values
					.stylesheet
					.overflow_scrollbar_metrics(),
			)
		} else {
			markview_selection::Hover::default()
		};
		let hover = document_hover.link;
		let hover_overflow = document_hover.overflow;
		let cursor = if self.input_at_cursor().is_some() {
			CursorIcon::Text
		} else if self.tab_strip.drag.is_some_and(|d| d.moving) {
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
		} else if !self.interaction.panel_open() && !over_outline {
			let cursor = if idle {
				document_hover.cursor
			} else {
				self.document_interaction().cursor(
					markview_selection::Point::new(
						self.interaction.cursor.0,
						self.interaction.cursor.1,
					),
					false,
					true,
				)
			};
			match cursor {
				markview_selection::Cursor::Default => CursorIcon::Default,
				markview_selection::Cursor::Text => CursorIcon::Text,
				markview_selection::Cursor::Pointer => CursorIcon::Pointer,
			}
		} else {
			CursorIcon::Default
		};

		let hover_changed = hover != self.interaction.hover
			|| hover_overflow != self.interaction.hover_overflow;
		let hover_image = document_hover.image_title;
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
	/// The image under the cursor, named the way the viewer and the renderer
	/// name textures.
	pub(super) fn image_at_cursor(
		&self,
	) -> Option<(String, markview_core::scene::Rect)> {
		self.document_interaction()
			.image(markview_selection::Point::new(
				self.interaction.cursor.0,
				self.interaction.cursor.1,
			))
			.map(|(src, rect, _)| (src.to_owned(), rect))
	}

	/// Opens the full-size viewer over the image under the cursor, if any.
	pub(super) fn open_viewer_at_cursor(&mut self) {
		if self.interaction.viewer.is_some() {
			return;
		}
		let Some((src, rect)) = self.image_at_cursor() else {
			return;
		};
		// The pixels the renderer already holds cap the fitting, so the
		// viewer never promises a sharper picture than exists. A picture
		// still decoding fits its laid-out rect instead.
		let images = &self.readers.session.snapshot.images;
		let pixels = images
			.entries
			.get(&src)
			.and_then(|info| images.pixels.get(&src, info.version))
			.map(|p| (p.width as f32, p.height as f32))
			.unwrap_or((rect.w.max(1.), rect.h.max(1.)));
		let scale = self.dimensions().2;
		self.interaction.viewer = Some(crate::state::Viewer {
			src,
			pixels,
			scale,
			zoom: 1.,
			pan: (0., 0.),
			grab: None,
			pressed_at: None,
			dragged: false,
		});
		self.cancel_gestures();
		self.interaction.reset_clicks();
		self.sync_input();
		self.refresh_hover();
		self.redraw();
	}

	/// Decoded pixels can arrive while the viewer is already open.
	pub(super) fn refresh_viewer(&mut self) {
		let (width, height, scale) = self.dimensions();
		let Some(viewer) = self.interaction.viewer.as_mut() else {
			return;
		};
		let images = &self.readers.session.snapshot.images;
		if let Some(pixels) = images
			.entries
			.get(&viewer.src)
			.and_then(|info| images.pixels.get(&viewer.src, info.version))
		{
			viewer.pixels = (pixels.width as f32, pixels.height as f32);
		}
		viewer.scale = scale;
		viewer.pan_by((0., 0.), (width, height));
	}

	pub(super) fn open_link(&mut self, url: &str, background: bool) {
		// Activating a link is direct input, so nothing keeps easing behind it.
		self.readers.session.cancel_scroll_animation();
		// A `<details>` summary is hit like a link but toggles its element.
		if let Some(id) = markview_core::document::details_id(url) {
			self.toggle_details(id);
			return;
		}
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
				self.status =
					self.preferences.values.lang().status_not_opened(url);
				self.status_until =
					Some(Instant::now() + Duration::from_secs(4));
				self.redraw();
			}
		}
	}

	/// Toggles one `<details>` and reflows. The open set is layout input, so
	/// the worker re-lays out the toggled block and reuses every other one.
	pub(super) fn toggle_details(&mut self, id: u64) {
		let declared = self
			.readers
			.session
			.document
			.as_ref()
			.and_then(|document| document.details_declared(id))
			.unwrap_or(false);
		let expanded = self
			.readers
			.session
			.details_open
			.get(&id)
			.copied()
			.unwrap_or(declared);
		std::sync::Arc::make_mut(&mut self.readers.session.details_open)
			.insert(id, !expanded);
		self.request(false);
		self.refresh_hover();
		self.redraw();
	}

	/// Hands an already-approved target to the operating system.
	pub(super) fn launch(&mut self, target: &str) {
		self.error = false;
		let lang = self.preferences.values.lang();
		self.status = match open::that_detached(target) {
			Ok(()) => lang.status_opened(target),
			Err(error) => {
				self.error = true;
				lang.status_cannot_open(target, error)
			}
		};
		self.status_until = Some(Instant::now() + Duration::from_secs(4));
		self.redraw();
	}

	/// Pans the wide block under the pointer. `false` means no block was
	/// there, so the caller can scroll the page instead of dropping the event.
	pub(super) fn horizontal_by(&mut self, dx: f32) -> bool {
		let consumed = markview_selection::horizontal_by(
			&self.readers.session.snapshot,
			self.view_geometry(),
			&mut self.readers.session.horizontal,
			markview_selection::Point::new(
				self.interaction.cursor.0,
				self.interaction.cursor.1,
			),
			dx,
		);
		if consumed {
			self.refresh_hover();
			self.redraw();
		}
		consumed
	}
}

#[cfg(test)]
mod tests;
