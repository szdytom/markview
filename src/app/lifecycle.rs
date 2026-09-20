use crate::cli::Mode;
use anyhow::Result;
use log::{debug, info};
use std::{
	sync::Arc,
	time::{Duration, Instant},
};
#[cfg(target_os = "linux")]
use winit::platform::wayland::WindowAttributesExtWayland;
use winit::{
	application::ApplicationHandler,
	dpi::LogicalSize,
	event::WindowEvent,
	event_loop::{ActiveEventLoop, ControlFlow},
	window::{Window, WindowId},
};

use super::{App, Event, TOP, system_theme};
impl ApplicationHandler<Event> for App {
	fn resumed(&mut self, event_loop: &ActiveEventLoop) {
		if self.window.is_some() {
			return;
		}
		let result = (|| -> Result<()> {
			let mut attributes = Window::default_attributes()
				.with_title("Markview")
				.with_inner_size(LogicalSize::new(
					self.args.width,
					self.args.height,
				))
				.with_min_inner_size(LogicalSize::new(500, 300));
			// The Wayland application ID. Desktops match it against the
			// installed `markview.desktop` to find the window icon, and the
			// X11 backend reads the same name for `WM_CLASS`.
			#[cfg(target_os = "linux")]
			{
				attributes = attributes.with_name("markview", "markview");
			}
			if let Some(icon) = super::icon::window_icon() {
				attributes = attributes.with_window_icon(Some(icon));
			}
			let window = Arc::new(event_loop.create_window(attributes)?);
			if self.args.mode == Mode::Window
				&& self.args.theme.is_none()
				&& self.args.style.is_none()
				&& self.preferences.theme_preference().is_none()
				&& let Some(theme) = system_theme(&window)
			{
				self.preferences.values.theme = theme;
			}
			let size = window.inner_size();
			let scale = window.scale_factor();
			info!(
				"Display scale (DPR): {scale:.3}; framebuffer: {}×{} physical px; window: {:.1}×{:.1} logical px",
				size.width,
				size.height,
				size.width as f64 / scale,
				size.height as f64 / scale,
			);
			self.window = Some(window);
			self.reload_styles();
			self.gpu()?;
			if let Some(path) = self.args.path.clone() {
				self.open(path);
			}
			self.redraw();
			Ok(())
		})();
		if let Err(e) = result {
			self.fatal = Some(format!("{e:#}"));
			event_loop.exit();
		}
	}
	fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Event) {
		match event {
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
			Event::Open(path) => {
				self.dialog_open = false;
				if let Some(path) = path {
					self.open(path);
				}
			}
			Event::Changed(path)
				if self.readers.session.path.as_ref() == Some(&path) =>
			{
				self.readers.session.content_version += 1;
				// New content asks again before fetching every remote image,
				// and `<details>` start from what the new source declares.
				self.readers.session.load_all_images = false;
				self.readers.session.remote_notice_dismissed = false;
				self.readers.session.details_open = Default::default();
				self.readers.session.cancel_scroll_animation();
				self.request(true);
				// A watched export rebuilds from the same save.
				self.schedule_watch_export(&path);
			}
			Event::Ready(mut update)
				if update.version == self.readers.session.version
					&& self.readers.session.path.as_ref()
						== Some(&update.path) =>
			{
				let counts = update.counts;
				match update.result.take() {
					Some(Ok(reader)) => {
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
						self.readers.session.displayed_version = update.version;
						if complete && self.readers.session.select_all_pending {
							self.interaction.selection =
								self.readers.session.snapshot.select_all(
									self.readers.session.accepted_revision,
								);
							self.readers.session.select_all_pending = false;
						}
						self.refresh_hover();
						self.status = if !complete {
							"Loading…".into()
						} else if self.readers.session.snapshot.math_errors > 0
						{
							format!(
								"{} formulas shown as source",
								self.readers.session.snapshot.math_errors
							)
						} else {
							String::new()
						};
						self.apply_anchor();
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
						self.readers.session.layout_pending =
							!self.readers.session.snapshot_complete;
						self.readers.session.pending_scroll = None;
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
			Event::Fonts(progress) => {
				self.font_jobs.insert(progress.id.clone(), *progress);
				self.redraw();
			}
			Event::FontsSettled(summary) => {
				// Only the families this run owned are retired: another run's
				// progress must survive this event.
				for id in &summary.requested {
					self.font_jobs.remove(id);
				}
				self.font_stored = summary.stored;
				self.register_fonts();
				// The catalogue's states follow the directory, which the job
				// may just have changed.
				self.refresh_font_catalog();
				self.font_note =
					match (summary.failed.first(), summary.cancelled.first()) {
						(Some((id, reason)), _) => {
							Some(format!("{id}: {reason}"))
						}
						(None, Some(id)) => Some(format!("{id}: cancelled")),
						(None, None) if summary.stored > 0 => Some(format!(
							"{} files stored, {} MiB",
							summary.stored,
							summary.bytes / (1024 * 1024)
						)),
						(None, None) => Some("Nothing to download".into()),
					};
				if let Some((_, reason)) = summary.failed.first() {
					self.notify(reason, true, 6);
				}
				self.redraw();
			}
			Event::DeviceLost => {
				if let Err(e) = self.gpu() {
					self.fatal = Some(format!("GPU recovery failed: {e:#}"));
					event_loop.exit();
				} else {
					self.redraw();
				}
			}
			_ => {}
		}
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
		let now = Instant::now();
		self.auto_scroll_tabs(now);
		self.advance_scroll(now);
		self.readers.release_inactive(now);
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
				self.scroll_by(if self.interaction.cursor.1 < TOP + 24.0 {
					-14.0
				} else {
					14.0
				});
				self.update_drag();
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
			return;
		}
		let deadline = self
			.reflow_at
			.into_iter()
			.chain(self.retry_at)
			.chain(self.prewarm_at)
			.chain(self.watch_at)
			.chain(self.status_until)
			.chain(self.preferences.save_deadline())
			.chain(self.interaction.drag_at)
			.chain(self.readers.session.scroll_animation_deadline(now))
			.chain(self.tab_strip.scroll_at)
			.chain(self.readers.release_deadline())
			.chain(
				(self.args.mode == Mode::Smoke)
					.then_some(self.started + Duration::from_secs(30)),
			)
			.min();
		event_loop.set_control_flow(
			deadline.map_or(ControlFlow::Wait, ControlFlow::WaitUntil),
		);
	}
}
