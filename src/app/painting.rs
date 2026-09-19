use crate::cli::Mode;
use crate::state::ScrollbarAxis;
use crate::{
	benchmark,
	render::{Renderer, View},
};
use anyhow::Result;
use log::{debug, info};
use std::time::Instant;
use winit::event_loop::ActiveEventLoop;

use super::{App, BOTTOM, Event};
/// How long one glyph prewarm pass may spend before yielding to the next
/// frame. Scroll frames cost about 0.4 ms when their glyphs are cached, so
/// this hands the reader a much better frame than the 13 ms a screenful of
/// new CJK glyphs costs when it arrives cold.
const PREWARM_BUDGET: std::time::Duration = std::time::Duration::from_millis(2);
/// How long the reader must stay put before the next prewarm pass.
const PREWARM_INTERVAL: std::time::Duration =
	std::time::Duration::from_millis(16);
/// A frame slower than this means the reader is moving through content that
/// is not prepared yet, so prewarming waits instead of competing with it.
const PREWARM_QUIET_MS: f64 = 3.0;
impl App {
	pub(super) fn render(
		&mut self,
		event_loop: &ActiveEventLoop,
	) -> Result<()> {
		let Some(window) = self.window.clone() else {
			return Ok(());
		};
		let size = window.inner_size();
		if size.width == 0 || size.height == 0 {
			return Ok(());
		}
		// A pass only survives a frame that completes; an occluded or retried
		// frame must not leave a deadline that wakes the loop forever.
		self.prewarm_at = None;
		let overlay = self.overlay();
		let (width, _, scale) = self.dimensions();
		let view = View {
			selection: self.interaction.selection,
			revision: self.readers.session.accepted_revision,
			width: size.width,
			height: size.height,
			scale,
			scroll: self.readers.session.scroll,
			left: ((width - self.readers.session.snapshot.width) / 2.0)
				.max(20.0),
			top: self.content_top() + 10.0,
			bottom: BOTTOM + 10.0,
			theme: self.preferences.values.theme,
			horizontal: &self.readers.session.horizontal,
			hovered_link: self.interaction.hover.as_deref(),
			hovered_overflow: self.interaction.hover_overflow,
			held_overflow: self.interaction.scrollbar.and_then(
				|drag| match drag.target {
					ScrollbarAxis::Overflow { block, overflow } => {
						Some((block, overflow))
					}
					ScrollbarAxis::Document => None,
				},
			),
		};
		let Some(renderer) = &mut self.renderer else {
			return Ok(());
		};
		renderer.set_pointer(
			(!self.interaction.panel_open && self.interaction.modal.is_none())
				.then_some(self.interaction.cursor),
		);
		let (frame, suboptimal) = match renderer.acquire(window.clone())? {
			crate::render::FrameStatus::Ready(frame, suboptimal) => {
				(frame, suboptimal)
			}
			crate::render::FrameStatus::Retry(delay) => {
				self.retry_at = Some(Instant::now() + delay);
				return Ok(());
			}
			crate::render::FrameStatus::Occluded => return Ok(()),
		};
		let target = frame.texture.create_view(&Default::default());
		let rasterized_before = renderer.raster_stats().rasterized;
		let started = Instant::now();
		let submission = renderer.render(
			&self.readers.session.snapshot,
			&view,
			&overlay,
			&target,
		)?;
		let prepared_ms = started.elapsed().as_secs_f64() * 1000.0;
		let rasterized = renderer.raster_stats().rasterized - rasterized_before;
		window.pre_present_notify();
		frame.present();
		if suboptimal {
			renderer.resize(size.width, size.height);
		}
		if let Some(update) = self.first_frame.take() {
			renderer.wait(Some(submission.clone()))?;
			debug!(
				"open→GPU complete: {:.2} ms (read {:.2}, parse {:.2}, layout {:.2}); reused {} blocks; {}",
				update.requested.elapsed().as_secs_f64() * 1000.0,
				update.read_ms,
				update.parse_ms,
				update.layout_ms,
				self.readers.session.snapshot.reused,
				renderer.adapter_name
			);
			if self.args.mode == Mode::Smoke {
				info!(
					"process entry→readable GPU frame: {:.2} ms",
					crate::process_started().elapsed().as_secs_f64() * 1000.0,
				);
				info!(
					"process app entry→readable GPU frame: {:.2} ms; memory {}",
					self.started.elapsed().as_secs_f64() * 1000.0,
					serde_json::to_string(&benchmark::memory())?
				);
				if let Some(output) = &self.args.output {
					if let Some(parent) =
						output.parent().filter(|p| !p.as_os_str().is_empty())
					{
						std::fs::create_dir_all(parent)?;
					}
					let texture = renderer.offscreen(size.width, size.height);
					let s = renderer.render(
						&self.readers.session.snapshot,
						&view,
						&overlay,
						&texture.create_view(&Default::default()),
					)?;
					renderer.wait(Some(s))?;
					renderer.save_png(&texture, output)?;
				}
			}
		}
		// Exercise completion too, while the diagnostic above records only the
		// first readable frame (which may contain a prefix of the document).
		if self.args.mode == Mode::Smoke
			&& self.readers.session.snapshot_complete
		{
			renderer.wait(Some(submission))?;
			event_loop.exit();
		}
		// Prepare the next screenful while the reader is not moving through
		// unprepared content, so its glyphs are rasterized before the scroll
		// frame that needs them rather than inside it.
		let idle = prepared_ms < PREWARM_QUIET_MS;
		let more = if idle {
			renderer.prewarm(
				&self.readers.session.snapshot,
				&view,
				PREWARM_BUDGET,
			)
		} else {
			// A slow frame was slow because it rasterized what the reader had
			// just scrolled into; the same content is warm now, so come back
			// once it is done. A slow frame that rasterized nothing will not
			// get cheaper by waiting, and schedules nothing.
			rasterized > 0
		};
		if more {
			self.prewarm_at = Some(Instant::now() + PREWARM_INTERVAL);
		}
		Ok(())
	}
	pub(super) fn gpu(&mut self) -> Result<()> {
		let mut renderer =
			pollster::block_on(Renderer::new(self.window.clone()))?;
		renderer.set_stylesheet(self.preferences.values.stylesheet.clone());
		let proxy = self.proxy.clone();
		renderer.on_device_lost(move || {
			let _ = proxy.send_event(Event::DeviceLost);
		});
		self.renderer = Some(renderer);
		Ok(())
	}
}
