use crate::{
	Renderer, Theme, View, intersect,
	raster::{ATLAS_SIZE, COLOR_ATLAS_SIZE, GlyphOrigin},
};
use markview_layout::{
	scene::{Draw, Glyph, Paint, Rect},
	shaping::TextShaper,
};
use ratex_types::DisplayItem;
use std::sync::Arc;
use swash::FontRef;

/// Puts an image's edges on the device pixel grid. A raster decodes at the
/// rounded display size, so a fractional position or width would make the
/// sampler interpolate every texel with its neighbour, which blurs text drawn
/// inside a diagram or SVG far more than pixel-snapped body glyphs.
fn device_aligned(rect: Rect, scale: f32) -> Rect {
	Rect {
		x: (rect.x * scale).round() / scale,
		y: (rect.y * scale).round() / scale,
		w: (rect.w * scale).round().max(1.0) / scale,
		h: (rect.h * scale).round().max(1.0) / scale,
	}
}

/// Snaps one logical coordinate to the nearest whole device pixel.
fn device_snap(value: f32, scale: f32) -> f32 {
	(value * scale).round() / scale
}

/// Snaps a one-pixel rule to whole device pixels. A rule must not shrink
/// below one device pixel, but rounding thin formula rules down would make
/// them thinner at a higher DPR than at 1x, so its thickness rounds up.
fn device_aligned_rule(rect: Rect, scale: f32) -> Rect {
	let left = (rect.x * scale).round();
	let top = (rect.y * scale).round();
	let right = ((rect.x + rect.w) * scale).round().max(left + 1.0);
	Rect {
		x: left / scale,
		y: top / scale,
		w: (right - left) / scale,
		h: (rect.h * scale).ceil().max(1.0) / scale,
	}
}

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
			// A prewarm pass emits no vertices for these glyphs and is not the
			// frame the run list describes, so it must not record either.
			if e.color && !self.prewarming {
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
	#[cfg_attr(
		any(),
		markview::allow_complexity(
			reason = "Keep exhaustive Draw dispatch and recursive clipping with prewarm guards; image demand, atlas preparation and vertex emission share one ordering."
		)
	)]
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
				// A prewarm pass leaves image textures and the texture cache
				// alone: the frame it prepares is not the one on screen.
				if self.prewarming {
					return;
				}
				let rect = device_aligned(
					Rect {
						x: rect.x + dx,
						y: rect.y + dy,
						..*rect
					},
					view.scale,
				);
				if rect.intersect(clip).is_none() {
					return;
				}
				let Some(key) = self
					.images
					.prepare(src, *version, rect, view.scale, &self.gpu)
				else {
					// A preview draws a tab whose loader may have released its
					// decoded pictures; a neutral block is clearer than a hole.
					if self.guest {
						let color = self.color(
							Paint::Styled(
								markview_layout::style::Condition::Button,
								markview_layout::style::ColorField::HoverBackground,
							),
							view.theme,
						);
						self.geometry.solid(rect, color, clip, view);
					}
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
				decoration,
			} => {
				let rect = Rect {
					x: rect.x + dx,
					y: rect.y + dy,
					..*rect
				};
				let background = if *condition
					== markview_layout::style::Condition::Preview
				{
					Paint::Background
				} else {
					Paint::Scoped(
						*chain,
						*condition,
						markview_layout::style::ColorField::Background,
					)
				};
				let background = self.color(background, view.theme);
				if let Some(decoration) = decoration {
					let border_color = self.color(
						Paint::Scoped(
							*chain,
							*condition,
							markview_layout::style::ColorField::BorderColor,
						),
						view.theme,
					);
					self.geometry.decorated(
						rect,
						*decoration,
						background,
						border_color,
						clip,
						view,
					);
					return;
				}
				self.geometry.rounded(rect, *radius, background, clip, view);
				if *border > 0. {
					let color = self.color(
						Paint::Scoped(
							*chain,
							*condition,
							markview_layout::style::ColorField::BorderColor,
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
			Draw::Icon {
				paths,
				paint,
				x,
				y,
				size,
			} => {
				let color = self.color(
					if hovered {
						Self::hover_paint(*paint)
					} else {
						*paint
					},
					view.theme,
				);
				for path in *paths {
					self.raster.icon(
						&self.gpu.queue,
						&mut self.geometry,
						path.commands,
						path.fill,
						path.stroke_width,
						x + dx,
						y + dy,
						*size,
						color,
						clip,
						view,
					);
				}
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
								if self.fallback.is_none() {
									let mut shaper = TextShaper::with_fonts(
										self.fonts.clone(),
									);
									if let Some(stylesheet) =
										self.stylesheet.clone()
									{
										shaper.set_stylesheet(stylesheet);
									}
									self.fallback = Some(shaper);
								}
								let fallback = self
									.fallback
									.as_mut()
									.expect("a shaper was just built");
								let (glyphs, _, _) = fallback.math_fallback(
									&ch,
									size * *scale as f32,
									x + *gx as f32 * size,
									y + *gy as f32 * size,
									&math.appearance,
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
							let rect = device_aligned_rule(
								Rect {
									x: x + *lx as f32 * size,
									y: y + *ly as f32 * size,
									w: *width as f32 * size,
									h: (*thickness as f32 * size).max(0.6),
								},
								view.scale,
							);
							let color =
								self.math_color(*color, view.theme, *paint);
							if *dashed {
								let mut left = 0.0;
								while left < rect.w {
									let dash = device_aligned_rule(
										Rect {
											x: rect.x + left,
											w: (rect.w - left).min(size * 0.3),
											..rect
										},
										view.scale,
									);
									self.geometry
										.solid(dash, color, clip, view);
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
							device_aligned(
								Rect {
									x: x + *rx as f32 * size,
									y: y + *ry as f32 * size,
									w: *width as f32 * size,
									h: *height as f32 * size,
								},
								view.scale,
							),
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
								device_snap(x + *px as f32 * size, view.scale),
								device_snap(y + *py as f32 * size, view.scale),
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

#[cfg(test)]
mod tests {
	use super::*;

	/// A picture at a fractional DPR covers exactly the whole texel grid its
	/// raster decodes to, so no sampling phase blurs it.
	#[test]
	fn images_land_on_whole_device_pixels_at_fractional_dpr() {
		let rect = Rect {
			x: 10.2,
			y: 4.6,
			w: 40.3,
			h: 20.1,
		};
		let aligned = device_aligned(rect, 1.5);
		let whole = |pixels: f32| (pixels - pixels.round()).abs() < 1e-4;
		assert!(whole(aligned.x * 1.5));
		assert!(whole(aligned.y * 1.5));
		assert!(whole(aligned.w * 1.5));
		assert!(whole(aligned.h * 1.5));
		assert!(whole((aligned.x + aligned.w) * 1.5));
		assert!(whole((aligned.y + aligned.h) * 1.5));
	}

	/// Whole-pixel layouts at 1x keep their exact geometry.
	#[test]
	fn images_keep_whole_pixel_geometry_at_1x() {
		let rect = Rect {
			x: 8.,
			y: 12.,
			w: 40.,
			h: 20.,
		};
		assert_eq!(device_aligned(rect, 1.), rect);
	}

	/// Formula rules cover whole pixels and do not get thinner when the
	/// display scale rises.
	#[test]
	fn formula_rules_round_outward_to_whole_device_pixels() {
		let rect = Rect {
			x: 10.2,
			y: 4.6,
			w: 40.3,
			h: 0.6,
		};
		let one = device_aligned_rule(rect, 1.);
		let two = device_aligned_rule(rect, 2.);
		let whole = |pixels: f32| (pixels - pixels.round()).abs() < 1e-4;
		for (scale, rule) in [(1., one), (2., two)] {
			assert!(whole(rule.x * scale));
			assert!(whole(rule.y * scale));
			assert!(whole((rule.x + rule.w) * scale));
			assert!(whole((rule.y + rule.h) * scale));
		}
		assert!(one.h <= two.h * 2.);
		assert!(two.h * 2. >= 1.2);
	}

	/// A formula path's origin lands on a whole device pixel, so its atlas
	/// entry is sampled one texel per pixel.
	#[test]
	fn formula_path_origins_land_on_whole_device_pixels() {
		assert_eq!(device_snap(10.2, 1.5) * 1.5, 15.);
		assert_eq!(device_snap(4.6, 2.5) * 2.5, 12.);
	}
}
