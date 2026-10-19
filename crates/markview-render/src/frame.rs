use crate::{Renderer, View};
use anyhow::{Result, bail};
use markview_layout::scene::{Draw, LayoutSnapshot, Paint, Rect};
use std::sync::Arc;

/// A visible command staged until its layer is painted.
///
/// The block and command indices are resolved again when the stage is
/// replayed. That keeps the buffer free of borrows, so one renderer reuses it
/// across frames instead of allocating per frame.
struct Staged {
	block: usize,
	command: usize,
	dx: f32,
	dy: f32,
	clip: Rect,
	hovered: bool,
}

/// Per-frame paint order, kept across frames so painting allocates nothing.
#[derive(Default)]
pub(super) struct FrameScratch {
	/// Block backgrounds and boxes, painted below the selection.
	backgrounds: Vec<Staged>,
	/// Glyphs and other marks, painted above the selection.
	foreground: Vec<Staged>,
	/// Overflow scrollbar tracks and thumbs, painted last.
	tracks: Vec<(Rect, [f32; 4])>,
}

impl FrameScratch {
	fn clear(&mut self) {
		self.backgrounds.clear();
		self.foreground.clear();
		self.tracks.clear();
	}
}

/// The preview scene the glyph atlas could not hold, with the atlas generation
/// it failed at. Preparing an unchanged scene again would fill the atlas the
/// same way, so the renderer skips it until the scene or the atlas changes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct RejectedPreview {
	content: u64,
	presentation: u64,
	scroll: u32,
	rect: [u32; 4],
	dpi: u32,
	width: u32,
	blocks: usize,
	height: u32,
	atlas: u64,
}

impl RejectedPreview {
	fn new(preview: &crate::Preview<'_>, dpi: f32, atlas: u64) -> Self {
		let rect = preview.rect;
		Self {
			content: preview.snapshot.content_id,
			presentation: preview.snapshot.presentation_key,
			scroll: preview.scroll.to_bits(),
			rect: [
				rect.x.to_bits(),
				rect.y.to_bits(),
				rect.w.to_bits(),
				rect.h.to_bits(),
			],
			dpi: dpi.to_bits(),
			width: preview.snapshot.width.to_bits(),
			blocks: preview.snapshot.blocks.len(),
			height: preview.snapshot.height.to_bits(),
			atlas,
		}
	}
}

impl Renderer {
	pub(super) fn prepare(
		&mut self,
		snapshot: &LayoutSnapshot,
		view: &View<'_>,
		underlay: &[Draw],
		overlay: &[Draw],
	) {
		self.geometry.clear();
		self.geometry.set_emitting(!self.prewarming);
		if !self.prewarming {
			self.begin_images(snapshot);
		}
		let full = Rect {
			x: 0.0,
			y: 0.0,
			w: view.width as f32 / view.scale,
			h: view.height as f32 / view.scale,
		};
		let clip = view.viewport().clip();
		let metrics = self.stylesheet.as_ref().map_or_else(
			|| markview_layout::scene::ScrollbarMetrics::OVERFLOW,
			|s| s.overflow_scrollbar_metrics(),
		);
		if let Some(background) = &snapshot.document_box {
			self.draw(
				background,
				view.left,
				view.top - view.scroll,
				clip,
				view,
				false,
			);
		}
		for draw in underlay {
			self.draw(
				draw,
				self.ui_origin.0,
				self.ui_origin.1,
				full,
				view,
				false,
			);
		}
		let start = snapshot
			.blocks
			.partition_point(|b| b.y + b.height() < view.scroll);
		// Selection sits above block and code backgrounds but below glyphs, so a
		// highlight never tints the text it covers.
		let mut scratch = std::mem::take(&mut self.scratch);
		scratch.clear();
		for (index, block) in snapshot.blocks.iter().enumerate().skip(start) {
			let dy = view.top + block.y - view.scroll;
			if dy > clip.y + clip.h {
				break;
			}
			for (i, draw, position) in block.draws() {
				let hovered = view.hovered_link.is_some_and(|link| {
					link.block == block.source.start
						&& block.layout.link_hovered(i, link.url, link.id)
				});
				let (offset, local_clip) =
					block.command_view(i, index, view.horizontal);
				let clip = if let Some(rect) = local_clip {
					let rect = Rect {
						x: view.left + rect.x,
						y: dy + rect.y,
						..rect
					};
					let Some(clipped) = clip.intersect(rect) else {
						continue;
					};
					clipped
				} else {
					clip
				};
				let staged = Staged {
					block: index,
					command: i,
					dx: view.left + position[0] - offset,
					dy: dy + position[1],
					clip,
					hovered,
				};
				match draw {
					// Strike-through and similar marks stay above the glyphs.
					Draw::Rect(
						_,
						Paint::Text
						| Paint::Cascade(
							_,
							markview_layout::style::ColorField::Color,
						)
						| Paint::Styled(
							_,
							markview_layout::style::ColorField::Color,
						),
					)
					| Draw::Clipped { .. }
					| Draw::Glyph(_)
					| Draw::Image { .. }
					| Draw::Icon { .. }
					| Draw::Math { .. } => scratch.foreground.push(staged),
					Draw::Rect(..)
					| Draw::Box { .. }
					| Draw::Polygon { .. } => scratch.backgrounds.push(staged),
				}
			}
			self.stage_overflow_tracks(
				&mut scratch,
				block,
				index,
				dy,
				view,
				metrics,
			);
		}
		for staged in &scratch.backgrounds {
			self.draw(
				snapshot.blocks[staged.block].draw(staged.command),
				staged.dx,
				staged.dy,
				staged.clip,
				view,
				staged.hovered,
			);
		}
		self.draw_selection(snapshot, view, clip);
		for staged in &scratch.foreground {
			self.draw(
				snapshot.blocks[staged.block].draw(staged.command),
				staged.dx,
				staged.dy,
				staged.clip,
				view,
				staged.hovered,
			);
		}
		for (rect, color) in &scratch.tracks {
			self.geometry.solid(*rect, *color, clip, view);
		}
		for draw in overlay {
			self.draw(
				draw,
				self.ui_origin.0,
				self.ui_origin.1,
				full,
				view,
				false,
			);
		}
		// Publish atomically; the loader must never observe a half-painted frame.
		// A prewarm pass has no frame to publish and must not replace the demand
		// of the one the reader is looking at. A preview pass adds to a frame
		// but shows another tab, so its demand must stay unpublished too.
		if !self.prewarming && !self.guest {
			self.images.publish();
		}
		// Keep the stage buffers for the next frame.
		self.scratch = scratch;
	}
	/// Starts the image pass for the snapshot this layer draws.
	fn begin_images(&mut self, snapshot: &LayoutSnapshot) {
		if self.guest {
			self.images.begin_guest(&snapshot.images);
		} else {
			self.images.begin(&snapshot.images);
		}
	}
	fn stage_overflow_tracks(
		&self,
		scratch: &mut FrameScratch,
		block: &markview_layout::scene::PlacedBlock,
		index: usize,
		dy: f32,
		view: &View<'_>,
		metrics: markview_layout::scene::ScrollbarMetrics,
	) {
		for (oi, o) in block.layout.overflow.iter().enumerate() {
			let Some(rect) = block.overflow_rect(oi) else {
				continue;
			};
			let offset =
				view.horizontal.get(&(index, oi)).copied().unwrap_or(0.0);
			let band = Rect {
				x: view.left + rect.x,
				y: dy + rect.y + rect.h,
				w: o.rect.w,
				h: metrics.overflow_band(o.gutter),
			};
			let Some(bar) = markview_layout::scene::Scrollbar::horizontal(
				band,
				offset,
				o.content_width,
				o.rect.w,
				metrics,
			) else {
				continue;
			};
			let held = view.held_overflow == Some((index, oi));
			let hovered = held || view.hovered_overflow == Some((index, oi));
			let on_thumb =
				held || self.pointer.is_some_and(|(x, y)| bar.on_thumb(x, y));
			let (track, thumb) = bar.bars(hovered);
			scratch.tracks.push((
				track,
				self.color(
					Paint::Styled(
						markview_layout::style::Condition::Scrollbar,
						markview_layout::style::ColorField::Track,
					),
					view.theme,
				),
			));
			scratch.tracks.push((
				thumb,
				self.color(
					Paint::Styled(
						markview_layout::style::Condition::Scrollbar,
						if on_thumb {
							markview_layout::style::ColorField::ThumbHover
						} else {
							markview_layout::style::ColorField::Thumb
						},
					),
					view.theme,
				),
			));
		}
	}

	fn draw_selection(
		&mut self,
		snapshot: &LayoutSnapshot,
		view: &View<'_>,
		clip: Rect,
	) {
		if let Some(selection) = view.selection {
			let color = self.color(
				Paint::Styled(
					markview_layout::style::Condition::Selection,
					markview_layout::style::ColorField::Background,
				),
				view.theme,
			);
			for rect in snapshot.selection_rects_in(
				selection,
				view.horizontal,
				view.revision,
				view.scroll..view.scroll + clip.h,
			) {
				self.geometry.solid(
					view.viewport().window_rect(rect),
					color,
					clip,
					view,
				);
			}
		}
	}

	/// Renders one frame with a stylesheet of its own, leaving the renderer's
	/// colors exactly as they were. The PNG export uses this so its strips can
	/// never tint the reading view.
	pub fn render_with_stylesheet(
		&mut self,
		snapshot: &LayoutSnapshot,
		view: &View<'_>,
		underlay: &[Draw],
		overlay: &[Draw],
		target: &wgpu::TextureView,
		stylesheet: Arc<markview_layout::style::Stylesheet>,
	) -> Result<wgpu::SubmissionIndex> {
		let previous = self.stylesheet.replace(stylesheet);
		let result =
			self.render_layers(snapshot, view, underlay, overlay, target);
		self.stylesheet = previous;
		result
	}
	pub fn render(
		&mut self,
		snapshot: &LayoutSnapshot,
		view: &View<'_>,
		overlay: &[Draw],
		target: &wgpu::TextureView,
	) -> Result<wgpu::SubmissionIndex> {
		self.render_layers(snapshot, view, &[], overlay, target)
	}

	fn render_layers(
		&mut self,
		snapshot: &LayoutSnapshot,
		view: &View<'_>,
		underlay: &[Draw],
		overlay: &[Draw],
		target: &wgpu::TextureView,
	) -> Result<wgpu::SubmissionIndex> {
		// A preview or prewarm pass may have left the atlas full for content
		// this frame does not show; only this pass's own content may force a
		// rebuild of the frame the reader is looking at.
		self.raster.begin_frame();
		self.prepare(snapshot, view, underlay, overlay);
		if self.raster.full() {
			// Evict previous frames, then rebuild the entire current frame; never reuse stale UVs.
			self.raster.reset_atlas(&self.gpu.queue);
			self.prepare(snapshot, view, underlay, overlay);
			if self.raster.full() {
				bail!(
					"Visible content exceeds the glyph atlases (4 MiB masks / 1 MiB color); reduce zoom"
				);
			}
		}
		self.geometry.upload(&self.gpu.device, &self.gpu.queue);
		let mut encoder =
			self.gpu.device.create_command_encoder(&Default::default());
		self.encode(&mut encoder, target, None, Clear::Frame, view.theme);
		Ok(self.gpu.queue.submit([encoder.finish()]))
	}

	pub fn render_with_preview(
		&mut self,
		snapshot: &LayoutSnapshot,
		view: &View<'_>,
		overlay: &[Draw],
		preview: Option<&crate::Preview<'_>>,
		target: &wgpu::TextureView,
	) -> Result<wgpu::SubmissionIndex> {
		let Some(preview) = preview else {
			self.preview_rejected = None;
			self.images.release_guest();
			return self.render_layers(snapshot, view, &[], overlay, target);
		};
		let submission =
			self.render_layers(snapshot, view, &[], overlay, target)?;
		Ok(self
			.render_preview(preview, view.scale, target)?
			.unwrap_or(submission))
	}

	/// Composes a second scene into `rect`, on top of the frame just submitted.
	///
	/// The scene is laid out at the same device scale as the reader, so its
	/// glyphs come from full-resolution atlas entries; the pass viewport is
	/// what scales that rendering down into the card. Returns the submission
	/// that completes the pass, or `None` when the atlas could not hold it.
	fn render_preview(
		&mut self,
		preview: &crate::Preview<'_>,
		dpi: f32,
		target: &wgpu::TextureView,
	) -> Result<Option<wgpu::SubmissionIndex>> {
		let rejected =
			RejectedPreview::new(preview, dpi, self.raster.stats().resets);
		if self.preview_rejected == Some(rejected) {
			return Ok(None);
		}
		let (view, viewport) = preview_view(preview, dpi, self.ui_origin);
		self.guest = true;
		self.prepare(preview.snapshot, &view, &[], &[]);
		self.guest = false;
		if self.raster.full() {
			// The preview cannot displace the frame the reader is looking at.
			// An unchanged scene would fill the atlas again next frame, so
			// remember it and keep the card's placeholder until the scene or
			// the atlas changes.
			self.preview_rejected = Some(rejected);
			return Ok(None);
		}
		self.preview_rejected = None;
		self.geometry.upload(&self.gpu.device, &self.gpu.queue);
		let mut encoder =
			self.gpu.device.create_command_encoder(&Default::default());
		self.encode(
			&mut encoder,
			target,
			Some(viewport),
			Clear::Keep,
			preview.theme,
		);
		Ok(Some(self.gpu.queue.submit([encoder.finish()])))
	}

	/// Records one render pass for the geometry the last prepare produced.
	fn encode(
		&mut self,
		encoder: &mut wgpu::CommandEncoder,
		target: &wgpu::TextureView,
		viewport: Option<[f32; 4]>,
		clear: Clear,
		theme: crate::Theme,
	) {
		let c = self.color(Paint::Background, theme);
		let linear = |v: f32| {
			if v <= 0.04045 {
				v as f64 / 12.92
			} else {
				((v as f64 + 0.055) / 1.055).powf(2.4)
			}
		};
		let load = match clear {
			Clear::Frame => wgpu::LoadOp::Clear(wgpu::Color {
				r: linear(c[0]),
				g: linear(c[1]),
				b: linear(c[2]),
				a: 1.0,
			}),
			Clear::Keep => wgpu::LoadOp::Load,
		};
		let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
			label: Some("readable frame"),
			color_attachments: &[Some(wgpu::RenderPassColorAttachment {
				view: target,
				depth_slice: None,
				resolve_target: None,
				ops: wgpu::Operations {
					load,
					store: wgpu::StoreOp::Store,
				},
			})],
			..Default::default()
		});
		if let Some([x, y, w, h]) = viewport {
			pass.set_viewport(x, y, w, h, 0.0, 1.0);
		}
		pass.set_pipeline(&self.pipeline);
		pass.set_bind_group(0, self.raster.bind_group(), &[]);
		pass.set_vertex_buffer(0, self.geometry.buffer().slice(..));
		let mut cursor = 0;
		for (range, key) in self.images.runs() {
			pass.set_pipeline(&self.pipeline);
			pass.set_bind_group(0, self.raster.bind_group(), &[]);
			pass.draw(cursor..range.start, 0..1);
			pass.set_pipeline(&self.images.pipeline);
			pass.set_bind_group(
				0,
				match key {
					Some(key) => self.images.bind_group(key),
					None => self.raster.color_bind_group(),
				},
				&[],
			);
			pass.draw(range.clone(), 0..1);
			cursor = range.end;
		}
		pass.set_pipeline(&self.pipeline);
		pass.set_bind_group(0, self.raster.bind_group(), &[]);
		pass.draw(cursor..self.geometry.len(), 0..1);
	}
}

/// Whether a pass replaces the target or adds to it.
enum Clear {
	Frame,
	Keep,
}

/// The scene view and destination viewport a preview is composed with.
///
/// The scene covers `snapshot.width` across and as much of the page as the
/// card's aspect shows, both in whole device pixels; the viewport maps that
/// rendering onto the card's rectangle. Host UI layers are drawn at `origin`,
/// so the destination moves with them while the scene does not.
fn preview_view<'a>(
	preview: &crate::Preview<'a>,
	dpi: f32,
	origin: (f32, f32),
) -> (View<'a>, [f32; 4]) {
	let page = preview.snapshot.width.max(1.0);
	let rect = preview.rect;
	let slice = (rect.h.max(1.0) * page / rect.w.max(1.0)).max(1.0);
	// The host supplies the tab's saved reading position, which may reach into
	// the trailing space the reader reserves past the last line. Clamping it
	// so the whole slice stays inside the document would pull such a tab back
	// to earlier content.
	let scroll = preview.scroll.max(0.0);
	let width = (page * dpi).round().max(1.0) as u32;
	let height = (slice * dpi).round().max(1.0) as u32;
	let view = View {
		selection: None,
		revision: 0,
		width,
		height,
		scale: dpi,
		scroll,
		left: 0.0,
		top: 0.0,
		bottom: 0.0,
		theme: preview.theme,
		horizontal: preview.horizontal,
		hovered_link: None,
		hovered_overflow: None,
		held_overflow: None,
	};
	let viewport = [
		(rect.x + origin.0) * dpi,
		(rect.y + origin.1) * dpi,
		rect.w * dpi,
		rect.h * dpi,
	];
	(view, viewport)
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::collections::HashMap;

	/// A card shorter than the reader's viewport still starts at the saved
	/// reading position, even when that position lies in the trailing space
	/// the reader reserves below the last line.
	#[test]
	fn a_preview_keeps_a_reading_position_inside_the_trailing_space() {
		let horizontal = HashMap::new();
		let snapshot = markview_layout::scene::LayoutSnapshot {
			width: 1_200.0,
			height: 3_000.0,
			..Default::default()
		};
		let preview = crate::Preview {
			snapshot: &snapshot,
			scroll: 2_500.0,
			rect: Rect {
				x: 0.0,
				y: 0.0,
				w: 300.0,
				h: 200.0,
			},
			theme: crate::Theme::Light,
			horizontal: &horizontal,
		};
		let (view, _) = preview_view(&preview, 1.0, (0.0, 0.0));
		// The slice is 800 px, so the old clamp moved the top to 2,200 px.
		assert_eq!(view.scroll, 2_500.0);
	}

	/// Host UI is drawn at the safe-area origin, so the composed scene's
	/// destination moves with it; the scene itself keeps document coordinates.
	#[test]
	fn a_preview_viewport_follows_the_ui_origin() {
		let horizontal = HashMap::new();
		let snapshot = markview_layout::scene::LayoutSnapshot {
			width: 1_200.0,
			height: 3_000.0,
			..Default::default()
		};
		let preview = crate::Preview {
			snapshot: &snapshot,
			scroll: 0.0,
			rect: Rect {
				x: 40.0,
				y: 20.0,
				w: 300.0,
				h: 200.0,
			},
			theme: crate::Theme::Light,
			horizontal: &horizontal,
		};
		let (view, viewport) = preview_view(&preview, 2.0, (10.0, 48.0));
		assert_eq!(viewport, [100.0, 136.0, 600.0, 400.0]);
		assert_eq!((view.left, view.top), (0.0, 0.0));
	}
}
