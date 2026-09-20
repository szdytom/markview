use crate::{Renderer, View};
use anyhow::{Result, bail};
use markview_core::scene::{Draw, LayoutSnapshot, Paint, Rect};
use std::sync::Arc;
impl Renderer {
	pub(super) fn prepare(
		&mut self,
		snapshot: &LayoutSnapshot,
		view: &View<'_>,
		underlay: &[Draw],
		overlay: &[Draw],
	) {
		self.geometry.clear();
		if !self.prewarming {
			self.images.begin(&snapshot.images);
		}
		let full = Rect {
			x: 0.0,
			y: 0.0,
			w: view.width as f32 / view.scale,
			h: view.height as f32 / view.scale,
		};
		let clip = view.viewport().clip();
		let metrics = self.stylesheet.as_ref().map_or_else(
			|| markview_core::scene::ScrollbarMetrics::OVERFLOW,
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
			self.draw(draw, 0.0, 0.0, full, view, false);
		}
		let start = snapshot
			.blocks
			.partition_point(|b| b.y + b.layout.height < view.scroll);
		// Selection sits above block and code backgrounds but below glyphs, so a
		// highlight never tints the text it covers.
		let mut backgrounds: Vec<(&Draw, f32, f32, Rect, bool)> = Vec::new();
		let mut foreground: Vec<(&Draw, f32, f32, Rect, bool)> = Vec::new();
		let mut tracks: Vec<(Rect, [f32; 4])> = Vec::new();
		for (index, block) in snapshot.blocks.iter().enumerate().skip(start) {
			let dy = view.top + block.y - view.scroll;
			if dy > clip.y + clip.h {
				break;
			}
			for (i, draw) in block.layout.draws.iter().enumerate() {
				let hovered = view.hovered_link.is_some_and(|url| {
					block.layout.links.iter().enumerate().any(|(n, link)| {
						link.url == url
							&& link.command <= i && block
							.layout
							.links
							.get(n + 1)
							.map_or(i < block.layout.draws.len(), |next| {
								i < next.command
							})
					})
				});
				let (offset, local_clip) =
					block.layout.command_view(i, index, view.horizontal);
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
				let dx = view.left - offset;
				match draw {
					// Strike-through and similar marks stay above the glyphs.
					Draw::Rect(
						_,
						Paint::Text
						| Paint::Cascade(
							_,
							markview_core::style::ColorField::Color,
						)
						| Paint::Styled(
							_,
							markview_core::style::ColorField::Color,
						),
					)
					| Draw::Clipped { .. }
					| Draw::Glyph(_)
					| Draw::Image { .. }
					| Draw::Icon { .. }
					| Draw::Math { .. } => foreground.push((draw, dx, dy, clip, hovered)),
					Draw::Rect(..)
					| Draw::Box { .. }
					| Draw::Polygon { .. } => backgrounds.push((draw, dx, dy, clip, hovered)),
				}
			}
			for (oi, o) in block.layout.overflow.iter().enumerate() {
				let offset =
					view.horizontal.get(&(index, oi)).copied().unwrap_or(0.0);
				let band = Rect {
					x: view.left + o.rect.x,
					y: dy + o.rect.y + o.rect.h,
					w: o.rect.w,
					h: metrics.overflow_band(o.gutter),
				};
				let Some(bar) = markview_core::scene::Scrollbar::horizontal(
					band,
					offset,
					o.content_width,
					o.rect.w,
					metrics,
				) else {
					continue;
				};
				let held = view.held_overflow == Some((index, oi));
				let hovered =
					held || view.hovered_overflow == Some((index, oi));
				let on_thumb = held
					|| self.pointer.is_some_and(|(x, y)| bar.on_thumb(x, y));
				let (track, thumb) = bar.bars(hovered);
				tracks.push((
					track,
					self.color(
						Paint::Styled(
							markview_core::style::Condition::Scrollbar,
							markview_core::style::ColorField::Track,
						),
						view.theme,
					),
				));
				tracks.push((
					thumb,
					self.color(
						Paint::Styled(
							markview_core::style::Condition::Scrollbar,
							if on_thumb {
								markview_core::style::ColorField::ThumbHover
							} else {
								markview_core::style::ColorField::Thumb
							},
						),
						view.theme,
					),
				));
			}
		}
		for (draw, dx, dy, clip, hovered) in backgrounds {
			self.draw(draw, dx, dy, clip, view, hovered);
		}
		if let Some(selection) = view.selection {
			let color = self.color(
				Paint::Styled(
					markview_core::style::Condition::Selection,
					markview_core::style::ColorField::Background,
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
		for (draw, dx, dy, clip, hovered) in foreground {
			self.draw(draw, dx, dy, clip, view, hovered);
		}
		for (rect, color) in tracks {
			self.geometry.solid(rect, color, clip, view);
		}
		for draw in overlay {
			self.draw(draw, 0.0, 0.0, full, view, false);
		}
		// Publish atomically; the loader must never observe a half-painted frame.
		// A prewarm pass has no frame to publish and must not replace the demand
		// of the one the reader is looking at.
		if !self.prewarming {
			self.images.publish();
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
		stylesheet: Arc<markview_core::style::Stylesheet>,
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
		let c = self.color(Paint::Background, view.theme);
		let linear = |v: f32| {
			if v <= 0.04045 {
				v as f64 / 12.92
			} else {
				((v as f64 + 0.055) / 1.055).powf(2.4)
			}
		};
		{
			let mut pass =
				encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
					label: Some("readable frame"),
					color_attachments: &[Some(
						wgpu::RenderPassColorAttachment {
							view: target,
							depth_slice: None,
							resolve_target: None,
							ops: wgpu::Operations {
								load: wgpu::LoadOp::Clear(wgpu::Color {
									r: linear(c[0]),
									g: linear(c[1]),
									b: linear(c[2]),
									a: 1.0,
								}),
								store: wgpu::StoreOp::Store,
							},
						},
					)],
					..Default::default()
				});
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
		Ok(self.gpu.queue.submit([encoder.finish()]))
	}
}
