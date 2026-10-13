use crate::cli::Mode;
use anyhow::Result;
use log::{debug, info};
use std::{
	sync::Arc,
	time::{Duration, Instant},
};
use winit::{
	application::ApplicationHandler,
	dpi::LogicalSize,
	event::WindowEvent,
	event_loop::{ActiveEventLoop, ControlFlow},
	window::{Window, WindowId},
};

use super::window::Loop;
use super::{App, Event, TOP, dm};
use crate::state::Selection;
impl<P: super::SendEvent> ApplicationHandler<Event> for App<P> {
	fn resumed(&mut self, event_loop: &ActiveEventLoop) {
		if self.window.is_some() {
			return;
		}
		let result = (|| -> Result<()> {
			let window = self.create_window(event_loop)?;
			self.touch_frame =
				markview_platform::TouchFrame::new(window.clone())?;
			let proxy = self.proxy.clone();
			let style = self.frame.layout;
			let tab_end = self.native_tab_end.clone();
			self.native_frame = markview_platform::NativeFrame::new(
				&window,
				move |metrics, x, y| {
					super::frame::native_hit_test(
						style,
						tab_end.get(),
						metrics,
						x,
						y,
					)
				},
				move || {
					proxy.send(Event::FrameFeedback);
				},
			)?;
			self.frame.focused = window.has_focus();
			let size = window.inner_size();
			let scale = window.scale_factor();
			info!(
				"Display scale (DPR): {scale:.3}; framebuffer: {}×{} physical px; window: {:.1}×{:.1} logical px",
				size.width,
				size.height,
				size.width as f64 / scale,
				size.height as f64 / scale,
			);
			let dm = dm::DirectManipulation::new(&window);
			self.window = Some(window);
			self.dm = dm;
			if self.args.mode == Mode::Window
				&& self.args.theme.is_none()
				&& self.args.style.is_none()
				&& self.preferences.theme_preference().is_none()
				&& let Some(theme) = self.system_theme()
			{
				self.preferences.values.theme = theme;
			}
			self.reload_styles();
			self.gpu()?;
			if !self.persistence.initialized {
				self.restore_session();
				if let Some(path) = self.args.path.clone() {
					self.open(path);
				}
			}
			if self.readers.session.path.is_some()
				&& self.readers.session.requested_options.as_ref()
					!= Some(&self.options())
			{
				self.request(false);
			}
			if let Some(url) = self.args.web_url.take() {
				self.open_web_page(url);
			}
			self.redraw();
			Ok(())
		})();
		if let Err(e) = result {
			self.fatal = Some(format!("{e:#}"));
			event_loop.exit();
		}
	}
	fn suspended(&mut self, _: &ActiveEventLoop) {
		if !markview_platform::touch_layout() {
			return;
		}
		self.cancel_gestures();
		self.flush_settings();
		self.flush_session();
		self.renderer.take();
		self.window.take();
	}
	fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Event) {
		self.handle_user_event(event_loop, event);
	}

	fn window_event(
		&mut self,
		event_loop: &ActiveEventLoop,
		window: WindowId,
		event: WindowEvent,
	) {
		self.handle_window_event(event_loop, window, event);
	}
	fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
		let deadline = self.tick(event_loop, Instant::now());
		event_loop.set_control_flow(
			deadline.map_or(ControlFlow::Wait, ControlFlow::WaitUntil),
		);
	}
	fn exiting(&mut self, _: &ActiveEventLoop) {
		self.flush_session();
		self.instance_path = None;
		self.instance.take();
	}
}

impl<P: super::SendEvent> App<P> {
	fn create_window(
		&self,
		event_loop: &ActiveEventLoop,
	) -> Result<Arc<Window>> {
		let mut attributes = Window::default_attributes()
			.with_title("Markview")
			.with_inner_size(LogicalSize::new(
				self.args.width,
				self.args.height,
			))
			.with_min_inner_size(LogicalSize::new(536, 300));
		attributes = markview_platform::window_attributes(
			attributes,
			self.frame.layout == crate::settings::WindowLayout::Macos,
		);
		if let Some(icon) = markview_platform::window_icon() {
			attributes = attributes.with_window_icon(Some(icon));
		}
		Ok(Arc::new(event_loop.create_window(attributes)?))
	}

	/// The loop's own events, handled without naming winit's loop type so a
	/// test can deliver an update to the same code the window runs. The only
	/// thing asked of the loop is whether to stop.
	pub(super) fn handle_user_event(
		&mut self,
		event_loop: &impl Loop,
		event: Event,
	) {
		match event {
			Event::NativeAssetsChanged => {
				self.register_fonts(1);
				self.settings_resources.invalidate();
				self.refresh_settings_resources(false);
				self.handle_user_event(event_loop, Event::StylesChanged);
			}
			Event::SystemThemeChanged(theme) => {
				self.desktop_theme = Some(theme);
				if self.preferences.theme_preference().is_none() {
					self.apply_saved_settings();
				}
			}
			Event::AndroidBack => self.android_back(),
			Event::AndroidConfiguration => {
				self.tab_strip.phone =
					markview_platform::android::phone_layout();
				if self.update_media() {
					self.request(false);
				}
				self.cancel_gestures();
				if self.interaction.panel == crate::state::PanelPage::Tabs {
					self.interaction
						.show_panel(crate::state::PanelPage::Closed);
				}
				self.tab_strip.reveal_active = true;
				self.redraw();
			}
			#[cfg(debug_assertions)]
			Event::AndroidInspect(send) => {
				let _ = send.send(self.android_snapshot());
			}
			Event::SettingsLoaded(completion) => {
				self.settings_loaded(*completion)
			}
			Event::SearchReady(result) => self.search_ready(result),
			Event::ImageSelected {
				document,
				revision,
				source,
				file,
			} => {
				self.dialog_open = false;
				if self.readers.session.path == document
					&& self.readers.session.content_version == revision
					&& self.readers.session.security.origin
						== crate::security::Origin::Clipboard
					&& let Some(path) = file
				{
					let path = std::fs::canonicalize(&path).unwrap_or(path);
					self.readers.session.security.grant(
						crate::security::Resource::SelectedImage {
							source,
							path,
						},
					);
					if let Some(document) = &self.readers.session.document {
						self.readers.session.security.bind(&document.source);
					}
					self.request(false);
				}
			}
			Event::WebLoaded { url, path, result } => {
				self.web_page_loaded(url, path, result)
			}
			Event::Parsed {
				path,
				content_version,
				document,
			} if self.readers.session.path.as_ref() == Some(&path)
				&& self.readers.session.content_version == content_version =>
			{
				if self
					.readers
					.session
					.saved_reading
					.as_ref()
					.is_some_and(|saved| saved.content != document.content_id)
					&& !self.readers.session.details_open.is_empty()
				{
					self.readers.session.details_open = Default::default();
					self.readers
						.session
						.saved_reading
						.as_mut()
						.unwrap()
						.details
						.clear();
					self.request(false);
				}
				self.readers.session.parse_complete = true;
				self.readers.session.search.document = Some(document);
				self.search_tick();
				self.redraw();
			}
			Event::FrameFeedback => self.redraw(),
			Event::StylesChanged => {
				self.reload_styles();
				self.redraw();
			}
			Event::SettingsChanged => {
				if self.preferences.reload() {
					self.apply_saved_settings();
				}
				self.redraw();
			}
			Event::Activate(path) => {
				if let Some(path) = path {
					self.open(path);
				}
				if let Some(window) = &self.window {
					window.set_minimized(false);
					window.focus_window();
				}
			}
			Event::Open(path) => {
				self.dialog_open = false;
				if let Some(path) = path {
					self.open(path);
				}
			}
			Event::AndroidOpenUrl(url) => self.open_web_page(url),
			Event::AndroidShared(path) => {
				self.open_document(path, crate::security::Security::default());
				self.readers.session.content_version += 1;
				self.readers.session.security.revoke();
				self.request(false);
			}
			Event::Changed(path)
				if self.readers.session.path.as_ref() == Some(&path) =>
			{
				self.reload_document(&path, true);
			}
			Event::Ready(mut update)
				if update.version == self.readers.session.version
					&& self.readers.session.path.as_ref()
						== Some(&update.path) =>
			{
				let counts = update.counts;
				match update.result.take() {
					Some(Ok(mut reader)) => {
						reader.layout.set_disclosures(
							&self.readers.session.details_open,
							false,
						);
						if !reader.complete
							&& self.interaction.selection.is_some_and(|s| {
								s.anchor.block.max(s.focus.block)
									>= reader.layout.blocks.len()
							}) {
							return;
						}
						if !self
							.readers
							.session
							.can_display(&reader, self.viewport())
						{
							return;
						}
						let first = self.readers.session.displayed_version
							!= update.version;
						let complete = reader.complete;
						let reading_changed =
							!self.readers.session.extends_prefix(&reader)
								&& !self
									.readers
									.session
									.snapshot
									.same_reading_text(&reader.layout);
						let rebased =
							self.interaction.selection.and_then(|s| {
								self.readers.session.snapshot.rebase_selection(
									&reader.layout,
									s,
									self.readers.session.accepted_revision,
									reader.content_version,
								)
							});
						if self.readers.session.accept(
							reader,
							self.viewport(),
							counts,
						) {
							self.interaction.clear_selection();
						} else {
							if reading_changed {
								self.interaction.clear_selection();
								self.interaction.selection_counts = None;
							}
							self.interaction.selection = rebased;
						}
						self.error = false;
						self.readers.session.load_error = None;
						self.readers.session.displayed_version = update.version;
						if complete && self.readers.session.select_all_pending {
							self.interaction.selection =
								self.readers.session.snapshot.select_all(
									self.readers.session.accepted_revision,
								);
							self.readers.session.select_all_pending = false;
						}
						self.refresh_hover();
						let lang = self.preferences.values.lang();
						self.status = if !complete {
							lang.footer_loading().to_owned()
						} else if self.readers.session.snapshot.math_errors > 0
						{
							lang.status_math_errors(
								self.readers.session.snapshot.math_errors,
							)
						} else {
							String::new()
						};
						self.apply_anchor();
						self.search_tick();
						self.apply_search_navigation();
						if let Some(w) = &self.window {
							w.set_title(&format!(
								"{} — Markview",
								update
									.path
									.file_name()
									.unwrap_or_default()
									.to_string_lossy()
							));
						}
						if complete {
							debug!(
								"full layout complete: {:.2} ms; {} blocks",
								update.requested.elapsed().as_secs_f64()
									* 1000.,
								self.readers.session.snapshot.blocks.len()
							);
						}
						if first {
							self.first_frame = Some(*update);
						}
					}
					Some(Err(error)) => {
						self.readers.session.layout_pending = false;
						self.readers.session.load_error = Some(error.clone());
						self.readers.session.scrolling.target = None;
						self.readers.session.pending_anchor = None;
						self.readers.session.select_all_pending = false;
						self.readers.session.cancel_scroll_animation();
						self.error = true;
						self.status = error;
						if self.args.mode == Mode::Smoke {
							self.fatal = Some(self.status.clone());
							event_loop.exit();
						}
					}
					None => {}
				}
				self.redraw();
			}
			Event::Exported(outcome) => {
				self.export_finished(*outcome);
			}
			Event::Fonts(message) => self.font_message(message),
			Event::DeviceLost => {
				if let Err(e) = self.gpu() {
					self.fatal = Some(
						self.preferences
							.values
							.lang()
							.status_gpu_failed(format!("{e:#}")),
					);
					event_loop.exit();
				} else {
					self.redraw();
				}
			}
			_ => {}
		}
	}
	fn pump_native_pan(&mut self) {
		let held = self.pointer_owns_input();
		// The window's hook offers its hit-tested touchpad pointers before
		// the pump: with `MANUALUPDATE` the OS buffers the gesture until the
		// pump's own `Update` consumes it, so the offer beats the input.
		// While a pointer drag, a confirmation, an option list or the image
		// viewer owns the input the offer is withheld, and the wheel paths
		// keep serving the pad — under the viewer they zoom it, as the wheel
		// always has.
		let offered = dm::DirectManipulation::take_offered_pointers();
		if !offered.is_empty()
			&& !held && self.interaction.modal.is_none()
			&& self.interaction.dropdown.is_none()
			&& self.interaction.viewer.is_none()
			&& let Some(viewport) = self.dm.as_mut()
		{
			for pointer in offered {
				viewport.contact(pointer);
			}
		}
		let pans = self.dm.as_mut().map_or_else(Vec::new, |dm| dm.pump(held));
		if !pans.is_empty() {
			let speed = self.scroll_speed();
			for (phase, dx, dy) in pans {
				self.trackpad_scroll(
					dx * speed,
					dy * speed,
					phase,
					super::gestures::Inertia::Native,
				);
			}
		}
	}

	/// Runs every deadline that came due and returns the soonest one still
	/// pending, so the loop can sleep until then. `None` means nothing is
	/// waiting, or this frame asked the loop to stop.
	///
	/// The loop type is named generically because a test drives the same
	/// timers without a window server, and because the only thing asked of
	/// the loop here is whether to stop.
	pub(super) fn tick(
		&mut self,
		event_loop: &impl Loop,
		now: Instant,
	) -> Option<Instant> {
		if let Some(frame) = &mut self.touch_frame {
			frame.dispatch();
		}
		let insets = self.insets();
		if self.surface_insets != insets {
			self.surface_insets = insets;
			self.cancel_gestures();
			self.request(false);
			self.redraw();
		}
		self.input_tick(now);
		self.search_tick();
		self.auto_scroll_tabs(now);
		// Direct Manipulation's updates arrive by pumping, before any
		// synthesis of the reader's own: a stream that starts here cancels
		// what is running before this tick can carry it a frame further, and
		// the deltas drive the seam exactly as a macOS pixel stream does.
		// While a pointer-driven interaction owns the input the pump is
		// `held`: the deltas drop, and the stream they belonged to cannot
		// resume when the owner lets go.
		self.pump_native_pan();
		self.advance_scroll(now);
		self.advance_gestures(now);
		if self.interaction.advance_drawers(now) {
			self.refresh_hover();
			self.redraw();
		}
		self.readers.release_inactive(now);
		self.session_tick(now);
		// One PNG strip per frame keeps the window responsive and the status
		// line counting; the draw requests the next frame while work remains.
		self.advance_png_export();
		if self.status_until.is_some_and(|until| until <= now) {
			self.status_until = None;
			self.status.clear();
			self.error = false;
			self.redraw();
		}
		if self.preferences.save_deadline().is_some_and(|d| d <= now) {
			self.flush_settings();
			self.redraw();
		}
		if self.interaction.drag_at.is_some_and(|d| d <= now) {
			self.interaction.drag_at = None;
			if self.interaction.pointer_down.is_some() {
				// `scroll_by` re-derives the selection and re-arms this
				// deadline through `after_scroll`, because the pointer stays
				// where it is while the text moves under it.
				self.scroll_by(markview_selection::selection_scroll(
					self.interaction.cursor.1,
					TOP,
					self.dimensions().1 - self.bottom(),
					self.readers.session.scrolling.offset,
					(self.readers.session.snapshot.height - self.viewport())
						.max(0.0),
				));
			}
		}
		if self.reflow_at.is_some_and(|d| d <= now) {
			self.reflow_at = None;
			if self.readers.session.requested_options.as_ref()
				!= Some(&self.options())
			{
				self.request(false);
			}
			self.scroll_by(0.0);
		}
		if self.retry_at.is_some_and(|d| d <= now) {
			self.retry_at = None;
			self.redraw();
		}
		if self.prewarm_at.is_some_and(|d| d <= now) {
			self.prewarm_at = None;
			self.redraw();
		}
		if self.watch_at.is_some_and(|d| d <= now) {
			self.watch_at = None;
			self.start_watch_export();
		}
		if self.args.mode == Mode::Smoke
			&& self.started.elapsed() > Duration::from_secs(30)
		{
			self.fatal = Some("Native window smoke test timed out".into());
			event_loop.exit();
			return None;
		}
		self.flush_ime_area();
		self.reflow_at
			.into_iter()
			.chain(self.text_input.deadline)
			.chain(self.retry_at)
			.chain(self.prewarm_at)
			.chain(self.watch_at)
			.chain(self.status_until)
			.chain(self.preferences.save_deadline())
			.chain(self.persistence.deadline)
			.chain(self.interaction.drag_at)
			.chain(self.readers.session.scroll_animation_deadline(now))
			.chain(self.tab_strip.scroll_at)
			.chain(self.gestures.deadline(now))
			.chain(self.interaction.drawer_deadline(now))
			.chain(self.dm.as_ref().and_then(|dm| dm.deadline(now)))
			.chain(self.readers.release_deadline())
			.chain(
				(self.args.mode == Mode::Smoke)
					.then_some(self.started + Duration::from_secs(30)),
			)
			.min()
	}
}
