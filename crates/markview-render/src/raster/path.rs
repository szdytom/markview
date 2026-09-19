use super::{Entry, RasterCache, RasterKey};
use crate::{View, intersect};
use markview_core::{document::fingerprint, scene::Rect};
use ratex_types::PathCommand;
/// The number of device-grid phases an icon raster bakes in. Four steps bound
/// the cache to a handful of bitmaps per icon while leaving the placement
/// error inside 1/8 device pixel.
const ICON_PHASES: f32 = 4.0;
impl RasterCache {
	/// Encodes `commands` into the raster key and builds their tiny-skia path.
	///
	/// Every caller pushes its own leading marker first, so an entry can never
	/// be mistaken for another kind of shape.
	fn outline(
		commands: &[PathCommand],
		bits: &mut Vec<u64>,
	) -> Option<tiny_skia::Path> {
		let mut b = tiny_skia::PathBuilder::new();
		for command in commands {
			match *command {
				PathCommand::MoveTo { x, y } => {
					b.move_to(x as f32, y as f32);
					bits.extend([0, x.to_bits(), y.to_bits()]);
				}
				PathCommand::LineTo { x, y } => {
					b.line_to(x as f32, y as f32);
					bits.extend([1, x.to_bits(), y.to_bits()]);
				}
				PathCommand::QuadTo { x1, y1, x, y } => {
					b.quad_to(x1 as f32, y1 as f32, x as f32, y as f32);
					bits.extend([
						2,
						x1.to_bits(),
						y1.to_bits(),
						x.to_bits(),
						y.to_bits(),
					]);
				}
				PathCommand::CubicTo {
					x1,
					y1,
					x2,
					y2,
					x,
					y,
				} => {
					b.cubic_to(
						x1 as f32, y1 as f32, x2 as f32, y2 as f32, x as f32,
						y as f32,
					);
					bits.extend([
						3,
						x1.to_bits(),
						y1.to_bits(),
						x2.to_bits(),
						y2.to_bits(),
						x.to_bits(),
						y.to_bits(),
					]);
				}
				PathCommand::Close => {
					b.close();
					bits.push(4);
				}
			}
		}
		b.finish()
	}

	/// A formula path. Its device position follows the formula's baseline
	/// fractionally, so it is tiled and keyed by the tile it covers.
	#[expect(clippy::too_many_arguments, reason = "Vector path drawing state")]
	pub(crate) fn path(
		&mut self,
		queue: &wgpu::Queue,
		geometry: &mut crate::geometry::Geometry,
		commands: &[PathCommand],
		fill: bool,
		x: f32,
		y: f32,
		size: f32,
		color: [f32; 4],
		clip: Rect,
		view: &View<'_>,
	) {
		// Formula strokes have always been this wide, in path units.
		const STROKE_WIDTH: f32 = 0.04;
		let mut bits = vec![u64::from(fill)];
		let Some(path) = Self::outline(commands, &mut bits) else {
			return;
		};
		let bounds = path.bounds();
		let rect = Rect {
			x: x + bounds.x() * size - 1.0,
			y: y + bounds.y() * size - 1.0,
			w: bounds.width() * size + 2.0,
			h: bounds.height() * size + 2.0,
		};
		let Some(visible) = intersect(rect, clip) else {
			return;
		};
		let px = ((visible.x - x) * view.scale).floor() as i32;
		let py = ((visible.y - y) * view.scale).floor() as i32;
		let w = (visible.w * view.scale).ceil() as u32 + 1;
		let h = (visible.h * view.scale).ceil() as u32 + 1;
		let scale = size * view.scale;
		// Tile very large paths, limiting temporary raster memory to 1 MiB.
		for ty in (0..h).step_by(512) {
			for tx in (0..w).step_by(512) {
				let (w, h) = ((w - tx).min(512), (h - ty).min(512));
				let (ox, oy) = (px + tx as i32, py + ty as i32);
				let Some(e) = self.raster(
					queue,
					&bits,
					&path,
					fill,
					STROKE_WIDTH,
					false,
					scale,
					[-(ox as f32), -(oy as f32)],
					ox,
					oy,
					w,
					h,
				) else {
					continue;
				};
				geometry.quad(
					Rect {
						x: x + ox as f32 / view.scale,
						y: y + oy as f32 / view.scale,
						w: w as f32 / view.scale,
						h: h as f32 / view.scale,
					},
					Rect {
						x: e.x as f32,
						y: e.y as f32,
						w: w as f32,
						h: h as f32,
					},
					color,
					clip,
					view,
				);
			}
		}
	}

	/// A filled polygon, such as a list marker, through the same antialiased
	/// vector rasterizer the formula paths use. The shape is snapped to the
	/// device grid and its coverage is cached in its own frame, so scrolling
	/// reuses one raster instead of filling the atlas with position variants.
	#[expect(clippy::too_many_arguments, reason = "Vector path drawing state")]
	pub(crate) fn polygon(
		&mut self,
		queue: &wgpu::Queue,
		geometry: &mut crate::geometry::Geometry,
		center: [f32; 2],
		points: &[[f32; 2]],
		color: [f32; 4],
		clip: Rect,
		view: &View<'_>,
	) {
		let mut b = tiny_skia::PathBuilder::new();
		// A leading `2` keeps a polygon key apart from a `fill`-first formula
		// path, whose commands start with 0..=4.
		let mut bits = vec![2u64];
		for (i, point) in points.iter().enumerate() {
			if i == 0 {
				b.move_to(point[0], point[1]);
			} else {
				b.line_to(point[0], point[1]);
			}
			bits.extend([
				u64::from(point[0].to_bits()),
				u64::from(point[1].to_bits()),
			]);
		}
		b.close();
		let Some(path) = b.finish() else {
			return;
		};
		let scale = view.scale;
		let bounds = path.bounds();
		// One bitmap covering the whole shape, in the shape's own frame. The
		// quad below does any clipping, so the raster is position independent.
		let left = (bounds.x() * scale).floor() - 1.0;
		let top = (bounds.y() * scale).floor() - 1.0;
		let w = (bounds.width() * scale).ceil() as u32 + 2;
		let h = (bounds.height() * scale).ceil() as u32 + 2;
		let Some(e) = self.raster(
			queue,
			&bits,
			&path,
			true,
			0.0,
			false,
			scale,
			[-left, -top],
			left as i32,
			top as i32,
			w,
			h,
		) else {
			return;
		};
		let device = [
			(center[0] * scale).round() + left,
			(center[1] * scale).round() + top,
		];
		geometry.quad(
			Rect {
				x: device[0] / scale,
				y: device[1] / scale,
				w: w as f32 / scale,
				h: h as f32 / scale,
			},
			Rect {
				x: e.x as f32,
				y: e.y as f32,
				w: w as f32,
				h: h as f32,
			},
			color,
			clip,
			view,
		);
	}

	/// A UI icon figure, in its unit box.
	///
	/// The figure's subpixel device position is baked into the bitmap rather
	/// than snapped away, like a glyph's horizontal phase: the quad then
	/// starts on a whole device pixel, so the atlas's linear filter samples
	/// each texel exactly, while the icon keeps the button's optical centre.
	#[expect(clippy::too_many_arguments, reason = "Vector path drawing state")]
	pub(crate) fn icon(
		&mut self,
		queue: &wgpu::Queue,
		geometry: &mut crate::geometry::Geometry,
		commands: &[PathCommand],
		fill: bool,
		stroke_width: f32,
		x: f32,
		y: f32,
		size: f32,
		color: [f32; 4],
		clip: Rect,
		view: &View<'_>,
	) {
		// A leading `3` keeps an icon key apart from a formula path (`fill`
		// first) and a polygon (`2` first); the stroke state follows.
		let mut bits =
			vec![3u64, u64::from(stroke_width.to_bits()), u64::from(!fill)];
		let Some(path) = Self::outline(commands, &mut bits) else {
			return;
		};
		// The commands are in the icon's unit box, so the bitmap rasterizes
		// at the icon's device size, while the quad below is placed in
		// logical pixels, which the geometry expands by the DPI scale.
		let dpi = view.scale;
		let scale = size * dpi;
		let bounds = path.bounds();
		// A stroke paints half its width outside the outline, and round caps
		// and joins stay inside that ring.
		let margin = if fill { 0.0 } else { stroke_width * 0.5 };
		let origin = [
			(x * dpi * ICON_PHASES).round() / ICON_PHASES,
			(y * dpi * ICON_PHASES).round() / ICON_PHASES,
		];
		bits.push(u64::from(origin[0].to_bits()));
		bits.push(u64::from(origin[1].to_bits()));
		let left = (origin[0] + (bounds.x() - margin) * scale).floor() - 1.0;
		let top = (origin[1] + (bounds.y() - margin) * scale).floor() - 1.0;
		let right = origin[0] + (bounds.x() + bounds.width() + margin) * scale;
		let bottom =
			origin[1] + (bounds.y() + bounds.height() + margin) * scale;
		let w = (right.ceil() - left) as u32 + 1;
		let h = (bottom.ceil() - top) as u32 + 1;
		let Some(e) = self.raster(
			queue,
			&bits,
			&path,
			fill,
			stroke_width,
			true,
			scale,
			[origin[0] - left, origin[1] - top],
			left as i32,
			top as i32,
			w,
			h,
		) else {
			return;
		};
		geometry.quad(
			Rect {
				x: left / dpi,
				y: top / dpi,
				w: w as f32 / dpi,
				h: h as f32 / dpi,
			},
			Rect {
				x: e.x as f32,
				y: e.y as f32,
				w: w as f32,
				h: h as f32,
			},
			color,
			clip,
			view,
		);
	}

	/// Rasterizes one vector path into the coverage atlas, keyed by the path,
	/// its scale and the device-grid tile it covers. `tx`/`ty` identify the
	/// bitmap's origin; `offset` is the translation from path units to that
	/// bitmap, so a shape that bakes in a subpixel phase passes it here.
	#[expect(clippy::too_many_arguments, reason = "Vector path drawing state")]
	fn raster(
		&mut self,
		queue: &wgpu::Queue,
		bits: &[u64],
		path: &tiny_skia::Path,
		fill: bool,
		stroke_width: f32,
		round: bool,
		scale: f32,
		offset: [f32; 2],
		tx: i32,
		ty: i32,
		w: u32,
		h: u32,
	) -> Option<Entry> {
		let key = RasterKey::Path {
			path: fingerprint(&bits),
			size: scale.to_bits(),
			tx,
			ty,
			w,
			h,
		};
		if let Some(entry) = self.cache.get(&key) {
			return Some(*entry);
		}
		if self.prewarm_declines() {
			return None;
		}
		let mut pixmap = tiny_skia::Pixmap::new(w, h)?;
		let transform = tiny_skia::Transform::from_row(
			scale, 0.0, 0.0, scale, offset[0], offset[1],
		);
		let mut paint = tiny_skia::Paint::default();
		paint.set_color_rgba8(255, 255, 255, 255);
		if fill {
			pixmap.fill_path(
				path,
				&paint,
				tiny_skia::FillRule::Winding,
				transform,
				None,
			);
		} else {
			pixmap.stroke_path(
				path,
				&paint,
				&tiny_skia::Stroke {
					width: stroke_width,
					line_cap: if round {
						tiny_skia::LineCap::Round
					} else {
						tiny_skia::LineCap::Butt
					},
					line_join: if round {
						tiny_skia::LineJoin::Round
					} else {
						tiny_skia::LineJoin::Miter
					},
					..Default::default()
				},
				transform,
				None,
			);
		}
		let mask: Vec<u8> = pixmap
			.data()
			.as_chunks::<4>()
			.0
			.iter()
			.map(|p| p[3])
			.collect();
		self.insert(
			queue,
			key,
			Entry {
				w,
				h,
				..Default::default()
			},
			&mask,
		)
	}
}
