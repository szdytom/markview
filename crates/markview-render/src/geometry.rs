//! Clipped quads and reusable frame vertex storage.
use crate::{View, intersect};
use bytemuck::{Pod, Zeroable};
use markview_layout::scene::Rect;
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(super) struct Vertex {
	position: [f32; 2],
	uv: [f32; 2],
	color: [f32; 4],
}

pub(super) struct Geometry {
	vertices: Vec<Vertex>,
	vertex_buffer: wgpu::Buffer,
	vertex_capacity: usize,
	/// Whether the current pass builds a frame. A prewarm pass fills the
	/// atlases for a frame it throws away, so it emits no vertices.
	emitting: bool,
	/// The atlas dimension UVs are normalized against.
	uv_scale: f32,
}
impl Geometry {
	pub(super) fn decorated(
		&mut self,
		r: Rect,
		decoration: &markview_layout::scene::BoxDecoration,
		background: [f32; 4],
		border: [f32; 4],
		clip: Rect,
		view: &View<'_>,
	) {
		if !self.emitting {
			return;
		}
		self.decorated_base(r, decoration, background, border, clip, view);
		for edge in &decoration.border_rects {
			self.solid(
				Rect {
					x: r.x + edge.x,
					y: r.y + edge.y,
					..*edge
				},
				border,
				clip,
				view,
			);
		}
	}

	fn decorated_base(
		&mut self,
		r: Rect,
		decoration: &markview_layout::scene::BoxDecoration,
		background: [f32; 4],
		border: [f32; 4],
		clip: Rect,
		view: &View<'_>,
	) {
		let Some(visible) = r.intersect(clip) else {
			return;
		};
		let corners =
			markview_layout::scene::fit_corners(r, decoration.corners);
		let [t, right, b, l] =
			markview_layout::scene::fit_edges(r, decoration.edges);
		let inner = Rect {
			x: r.x + l,
			y: r.y + t,
			w: (r.w - l - right).max(0.0),
			h: (r.h - t - b).max(0.0),
		};
		// Square borders use their exact widths, without scanline rounding.
		if corners == [0.0; 4] {
			self.solid(r, background, clip, view);
			for edge in [
				Rect { h: t, ..r },
				Rect {
					y: r.y + r.h - b,
					h: b,
					..r
				},
				Rect {
					x: r.x,
					w: l,
					..inner
				},
				Rect {
					x: r.x + r.w - right,
					w: right,
					..inner
				},
			] {
				self.solid(edge, border, clip, view);
			}
			return;
		}
		let inner_corners = markview_layout::scene::fit_corners(
			inner,
			[
				(corners[0] - t.max(l)).max(0.0),
				(corners[1] - t.max(right)).max(0.0),
				(corners[2] - b.max(right)).max(0.0),
				(corners[3] - b.max(l)).max(0.0),
			],
		);
		let span = |rect: Rect, c: [f32; 4], y: f32| {
			let inset = |top: f32, bottom: f32| {
				let (radius, d) = if y - rect.y < top {
					(top, y - rect.y)
				} else {
					(bottom, rect.y + rect.h - y)
				};
				if d >= radius {
					0.0
				} else {
					radius
						- (radius * radius - (radius - d).powi(2))
							.max(0.0)
							.sqrt()
				}
			};
			(
				rect.x + inset(c[0], c[3]),
				rect.x + rect.w - inset(c[1], c[2]),
			)
		};
		let top_end = r.y + corners[0].max(corners[1]).max(t);
		let bottom_start = r.y + r.h - corners[2].max(corners[3]).max(b);
		let mut y = visible.y;
		while y < visible.y + visible.h {
			let h = if y >= top_end && y < bottom_start {
				bottom_start - y
			} else {
				1.0 / view.scale
			}
			.min(visible.y + visible.h - y);
			let mid = y + h * 0.5;
			let (a, z) = span(r, corners, mid);
			self.solid(
				Rect {
					x: a,
					y,
					w: (z - a).max(0.0),
					h,
				},
				background,
				clip,
				view,
			);
			if mid < inner.y || mid >= inner.y + inner.h || inner.w == 0.0 {
				self.solid(
					Rect {
						x: a,
						y,
						w: (z - a).max(0.0),
						h,
					},
					border,
					clip,
					view,
				);
			} else {
				let (ia, iz) = span(inner, inner_corners, mid);
				self.solid(
					Rect {
						x: a,
						y,
						w: (ia - a).max(0.0),
						h,
					},
					border,
					clip,
					view,
				);
				self.solid(
					Rect {
						x: iz,
						y,
						w: (z - iz).max(0.0),
						h,
					},
					border,
					clip,
					view,
				);
			}
			y += h;
		}
	}

	pub(super) fn clear(&mut self) {
		self.vertices.clear();
	}
	pub(super) fn set_uv_scale(&mut self, scale: f32) {
		self.uv_scale = scale;
	}
	/// A pass that only prepares atlases builds no frame; its vertices would
	/// be thrown away, so it must not pay to append them.
	pub(super) fn set_emitting(&mut self, emitting: bool) {
		self.emitting = emitting;
	}
	pub(super) fn len(&self) -> u32 {
		self.vertices.len() as u32
	}
	pub(super) fn capacity_bytes(&self) -> u64 {
		self.vertex_capacity as u64
	}
	pub(super) fn buffer(&self) -> &wgpu::Buffer {
		&self.vertex_buffer
	}
	pub(super) fn upload(
		&mut self,
		device: &wgpu::Device,
		queue: &wgpu::Queue,
	) {
		let bytes = bytemuck::cast_slice(&self.vertices);
		if bytes.len() > self.vertex_capacity {
			self.vertex_capacity = bytes.len().next_power_of_two();
			self.vertex_buffer =
				device.create_buffer(&wgpu::BufferDescriptor {
					label: Some("visible quads"),
					size: self.vertex_capacity as u64,
					usage: wgpu::BufferUsages::VERTEX
						| wgpu::BufferUsages::COPY_DST,
					mapped_at_creation: false,
				});
		}
		if !bytes.is_empty() {
			queue.write_buffer(&self.vertex_buffer, 0, bytes);
		}
	}
	pub(super) fn new(device: &wgpu::Device) -> Self {
		let vertex_capacity = 65_536;
		let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("visible quads"),
			size: vertex_capacity as u64,
			usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		Self {
			vertices: Vec::new(),
			vertex_buffer,
			vertex_capacity,
			emitting: true,
			uv_scale: crate::raster::ATLAS_SIZE as f32,
		}
	}
	pub(super) fn rounded(
		&mut self,
		r: Rect,
		radius: f32,
		color: [f32; 4],
		clip: Rect,
		view: &View<'_>,
	) {
		if !self.emitting {
			return;
		}
		if color[3] == 0.
			|| r.w <= 0.
			|| r.h <= 0.
			|| r.intersect(clip).is_none()
		{
			return;
		}
		let radius = radius.min(r.w / 2.).min(r.h / 2.).max(0.);
		if radius < 0.5 {
			self.solid(r, color, clip, view);
			return;
		}
		self.solid(
			Rect {
				y: r.y + radius,
				h: (r.h - 2. * radius).max(0.),
				..r
			},
			color,
			clip,
			view,
		);
		let steps = (radius * view.scale).ceil().max(1.) as usize;
		for i in 0..steps {
			let y = i as f32 * radius / steps as f32;
			let h = radius / steps as f32;
			let d = radius - y - h / 2.;
			let inset = radius - (radius * radius - d * d).max(0.).sqrt();
			for top in [r.y + y, r.y + r.h - y - h] {
				self.solid(
					Rect {
						x: r.x + inset,
						y: top,
						w: (r.w - 2. * inset).max(0.),
						h,
					},
					color,
					clip,
					view,
				);
			}
		}
	}
	pub(super) fn rounded_border(
		&mut self,
		r: Rect,
		radius: f32,
		border: f32,
		color: [f32; 4],
		clip: Rect,
		view: &View<'_>,
	) {
		if !self.emitting {
			return;
		}
		let b = border.min(r.w / 2.).min(r.h / 2.).max(0.);
		let radius = radius.min(r.w / 2.).min(r.h / 2.).max(0.);
		let step = 1. / view.scale;
		let start = ((clip.y - r.y).max(0.) / step).floor() as usize;
		let rows =
			((clip.y + clip.h - r.y).min(r.h).max(0.) / step).ceil() as usize;
		for i in start..rows {
			let y = i as f32 * step;
			let h = step.min(r.h - y);
			let cy = y + h / 2.;
			let edge = |rad: f32, dy: f32| {
				if dy >= rad {
					0.
				} else {
					rad - (rad * rad - (rad - dy).powi(2)).max(0.).sqrt()
				}
			};
			let inset = edge(radius, cy.min(r.h - cy));
			if cy < b || cy >= r.h - b {
				self.solid(
					Rect {
						x: r.x + inset,
						y: r.y + y,
						w: (r.w - 2. * inset).max(0.),
						h,
					},
					color,
					clip,
					view,
				);
			} else {
				let inner =
					b + edge((radius - b).max(0.), (cy - b).min(r.h - b - cy));
				for x in [r.x + inset, r.x + r.w - inner] {
					self.solid(
						Rect {
							x,
							y: r.y + y,
							w: (inner - inset).max(0.),
							h,
						},
						color,
						clip,
						view,
					);
				}
			}
		}
	}
	pub(super) fn quad(
		&mut self,
		rect: Rect,
		uv: Rect,
		color: [f32; 4],
		clip: Rect,
		view: &View<'_>,
	) {
		if !self.emitting {
			return;
		}
		let Some(r) = intersect(rect, clip) else {
			return;
		};
		let uv = Rect {
			x: uv.x + (r.x - rect.x) / rect.w * uv.w,
			y: uv.y + (r.y - rect.y) / rect.h * uv.h,
			w: uv.w * r.w / rect.w,
			h: uv.h * r.h / rect.h,
		};
		let x0 = r.x * view.scale / view.width as f32 * 2.0 - 1.0;
		let x1 = (r.x + r.w) * view.scale / view.width as f32 * 2.0 - 1.0;
		let y0 = 1.0 - r.y * view.scale / view.height as f32 * 2.0;
		let y1 = 1.0 - (r.y + r.h) * view.scale / view.height as f32 * 2.0;
		let v = |x, y, u, v| Vertex {
			position: [x, y],
			uv: [u / self.uv_scale, v / self.uv_scale],
			color,
		};
		let a = v(x0, y0, uv.x, uv.y);
		let b = v(x1, y0, uv.x + uv.w, uv.y);
		let c = v(x1, y1, uv.x + uv.w, uv.y + uv.h);
		let d = v(x0, y1, uv.x, uv.y + uv.h);
		self.vertices.extend_from_slice(&[a, b, c, a, c, d]);
	}
	pub(super) fn solid(
		&mut self,
		rect: Rect,
		color: [f32; 4],
		clip: Rect,
		view: &View<'_>,
	) {
		self.quad(
			rect,
			Rect {
				x: 0.5,
				y: 0.5,
				w: 0.0,
				h: 0.0,
			},
			color,
			clip,
			view,
		);
	}
}
