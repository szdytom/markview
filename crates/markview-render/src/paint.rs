use crate::{
	Renderer, Theme, View, intersect,
	raster::{ATLAS_SIZE, COLOR_ATLAS_SIZE, GlyphOrigin},
};
use markview_core::{
	scene::{Draw, Glyph, Paint, Rect},
	shaping::TextShaper,
};
use ratex_types::DisplayItem;
use std::sync::Arc;
use swash::FontRef;
impl Renderer {
	pub(super) fn glyph_quad(
		&mut self,
		g: &Glyph,
		x: f32,
		y: f32,
		color: [f32; 4],
		clip: Rect,
		view: &View<'_>,
	) {
		if g.y + y + g.size < clip.y || g.y + y - g.size * 2.0 > clip.y + clip.h
		{
			return;
		}
		// Wide code/table rows can contain thousands of offscreen glyphs.
		// Keep a conservative overhang margin, but do not rasterize the row
		// outside its local horizontal viewport just to discard its quads.
		if g.x + x + g.size * 4.0 < clip.x
			|| g.x + x - g.size * 4.0 > clip.x + clip.w
		{
			return;
		}
		let origin = GlyphOrigin::new(g.x + x, g.y + y, view.scale);
		if let Some(e) = self.raster.glyph(
			&self.gpu.device,
			&self.images.pipeline,
			&self.gpu.queue,
			g,
			view.scale,
			origin.phase,
		) && e.w > 0
			&& e.h > 0
		{
			let start = self.geometry.len();
			let uv_scale = if e.color {
				ATLAS_SIZE as f32 / COLOR_ATLAS_SIZE as f32
			} else {
				1.
			};
			self.geometry.quad(
				Rect {
					x: (origin.x + e.left) / view.scale,
					y: (origin.y + e.top) / view.scale,
					w: e.w as f32 / view.scale,
					h: e.h as f32 / view.scale,
				},
				Rect {
					x: e.x as f32 * uv_scale,
					y: e.y as f32 * uv_scale,
					w: e.w as f32 * uv_scale,
					h: e.h as f32 * uv_scale,
				},
				color,
				clip,
				view,
			);
			if e.color {
				self.images.record_color_glyph(start..self.geometry.len());
			}
		}
	}

	pub(super) fn math_color(
		&self,
		color: ratex_types::Color,
		theme: Theme,
		paint: Paint,
	) -> [f32; 4] {
		if color.r == 0.0 && color.g == 0.0 && color.b == 0.0 {
			self.color(paint, theme)
		} else {
			[color.r, color.g, color.b, color.a]
		}
	}
	pub(super) fn draw(
		&mut self,
		draw: &Draw,
		dx: f32,
		dy: f32,
		clip: Rect,
		view: &View<'_>,
		hovered: bool,
	) {
		match draw {
			Draw::Clipped { rect, draws } => {
				let rect = Rect {
					x: rect.x + dx,
					y: rect.y + dy,
					..*rect
				};
				if let Some(clip) = clip.intersect(rect) {
					for draw in draws {
						self.draw(draw, dx, dy, clip, view, hovered);
					}
				}
			}
			Draw::Image {
				src, version, rect, ..
			} => {
				let rect = Rect {
					x: rect.x + dx,
					y: rect.y + dy,
					..*rect
				};
				if rect.intersect(clip).is_none() {
					return;
				}
				let Some(key) = self
					.images
					.prepare(src, *version, rect, view.scale, &self.gpu)
				else {
					return;
				};
				let start = self.geometry.len();
				self.geometry.quad(
					rect,
					Rect {
						x: 0.,
						y: 0.,
						w: ATLAS_SIZE as f32,
						h: ATLAS_SIZE as f32,
					},
					[1.; 4],
					clip,
					view,
				);
				self.images.record(start..self.geometry.len(), key);
			}
			Draw::Glyph(g) => self.glyph_quad(
				g,
				dx,
				dy,
				self.color(
					if hovered {
						Self::hover_paint(g.paint)
					} else {
						g.paint
					},
					view.theme,
				),
				clip,
				view,
			),
			Draw::Rect(r, paint) => self.geometry.solid(
				Rect {
					x: r.x + dx,
					y: r.y + dy,
					..*r
				},
				self.color(
					if hovered {
						Self::hover_paint(*paint)
					} else {
						*paint
					},
					view.theme,
				),
				clip,
				view,
			),
			Draw::Box {
				rect,
				chain,
				condition,
				radius,
				border,
				left_only,
			} => {
				let rect = Rect {
					x: rect.x + dx,
					y: rect.y + dy,
					..*rect
				};
				let background = self.color(
					Paint::Scoped(
						*chain,
						*condition,
						markview_core::style::ColorField::Background,
					),
					view.theme,
				);
				self.geometry.rounded(rect, *radius, background, clip, view);
				if *border > 0. {
					let color = self.color(
						Paint::Scoped(
							*chain,
							*condition,
							markview_core::style::ColorField::BorderColor,
						),
						view.theme,
					);
					if *left_only {
						self.geometry.solid(
							Rect { w: *border, ..rect },
							color,
							clip,
							view,
						);
					} else {
						self.geometry.rounded_border(
							rect, *radius, *border, color, clip, view,
						);
					}
				}
			}
			Draw::Polygon {
				center,
				points,
				paint,
			} => {
				let color = self.color(
					if hovered {
						Self::hover_paint(*paint)
					} else {
						*paint
					},
					view.theme,
				);
				// The rasterizer snaps the shape to the device grid; it owns
				// that so its coverage atlas stays cacheable across scrolls.
				self.raster.polygon(
					&self.gpu.queue,
					&mut self.geometry,
					[center[0] + dx, center[1] + dy],
					points,
					color,
					clip,
					view,
				);
			}
			Draw::Math { math, x, y, paint } => {
				let (x, y) = (x + dx, y + dy);
				if intersect(
					Rect {
						x,
						y,
						w: math.width.max(1.0),
						h: math.ascent + math.descent,
					},
					clip,
				)
				.is_none()
				{
					return;
				}
				let size = math.size;
				for item in &math.display.items {
					match item {
						DisplayItem::GlyphPath {
							x: gx,
							y: gy,
							scale,
							font,
							char_code,
							color,
						} => {
							let color =
								self.math_color(*color, view.theme, *paint);
							if let Some(data) = self.raster.math_font(font) {
								let ch = ratex_font::FontId::parse(font)
									.map_or_else(
										|| {
											char::from_u32(*char_code)
												.unwrap_or('\u{fffd}')
										},
										|id| {
											ratex_font::katex_ttf_glyph_char(
												id, *char_code,
											)
										},
									);
								if let Some(font) =
									FontRef::from_index(data.data.data(), 0)
								{
									let id = font.charmap().map(ch);
									let g = Glyph {
										font: data,
										coords: Arc::from([]),
										id,
										size: size * *scale as f32,
										x: x + *gx as f32 * size,
										y: y + *gy as f32 * size,
										synthetic_italic: false,
										paint: Paint::Text,
									};
									self.glyph_quad(
										&g, 0.0, 0.0, color, clip, view,
									);
								}
							} else {
								let ch = char::from_u32(*char_code)
									.unwrap_or('\u{fffd}')
									.to_string();
								let fallback = self
									.fallback
									.get_or_insert_with(TextShaper::new);
								let glyphs = fallback.label(
									&ch,
									size * *scale as f32,
									x + *gx as f32 * size,
									y + *gy as f32 * size,
									Paint::Text,
								);
								for g in glyphs {
									if let Draw::Glyph(g) = g {
										self.glyph_quad(
											&g, 0.0, 0.0, color, clip, view,
										);
									}
								}
							}
						}
						DisplayItem::Line {
							x: lx,
							y: ly,
							width,
							thickness,
							color,
							dashed,
						} => {
							let rect = Rect {
								x: x + *lx as f32 * size,
								y: y + *ly as f32 * size,
								w: *width as f32 * size,
								h: (*thickness as f32 * size).max(0.6),
							};
							let color =
								self.math_color(*color, view.theme, *paint);
							if *dashed {
								let mut left = 0.0;
								while left < rect.w {
									self.geometry.solid(
										Rect {
											x: rect.x + left,
											w: (rect.w - left).min(size * 0.3),
											..rect
										},
										color,
										clip,
										view,
									);
									left += size * 0.5;
								}
							} else {
								self.geometry.solid(rect, color, clip, view);
							}
						}
						DisplayItem::Rect {
							x: rx,
							y: ry,
							width,
							height,
							color,
						} => self.geometry.solid(
							Rect {
								x: x + *rx as f32 * size,
								y: y + *ry as f32 * size,
								w: *width as f32 * size,
								h: *height as f32 * size,
							},
							self.math_color(*color, view.theme, *paint),
							clip,
							view,
						),
						DisplayItem::Path {
							x: px,
							y: py,
							commands,
							fill,
							color,
						} => {
							let color =
								self.math_color(*color, view.theme, *paint);
							self.raster.path(
								&self.gpu.queue,
								&mut self.geometry,
								commands,
								*fill,
								x + *px as f32 * size,
								y + *py as f32 * size,
								size,
								color,
								clip,
								view,
							);
						}
					}
				}
			}
		}
	}
}
