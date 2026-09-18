//! Page rendering: layout draws in, PDF content streams out.
//!
//! The painter is a projection of the settled layout. It never re-measures
//! text: every glyph, rule, box and image arrives with its final geometry, in
//! layout pixels measured down from the block's top-left corner. A `Frame`
//! maps one page fragment of one block into page points.
use crate::{
	Export,
	image::Images,
	link,
	text::{self, Fonts, RunGlyph},
};
use anyhow::{Context, Result};
use krilla::{
	Document,
	color::rgb,
	geom::{Path, PathBuilder, Point, Rect, Size, Transform},
	metadata::Metadata,
	num::NormalizedF32,
	page::PageSettings,
	paint::{Fill, FillRule, Paint as KrillaPaint, Stroke},
	surface::Surface,
	text::{Font, GlyphId, KrillaGlyph},
};
use markview_core::{
	layout::{BlockLayout, Draw, LayoutSnapshot, Paint, PlacedBlock},
	math::MathBox,
	paginate::{
		FurnitureText, PT_PER_PX, PageGeometry, PageItem, Pagination,
		page_furniture,
	},
	scene::Rect as LocalRect,
	shaping::TextShaper,
	style::{ColorField, Condition, Stylesheet},
};
use parley::FontData;
use ratex_types::{Color as MathColor, DisplayItem, PathCommand};
use std::{collections::HashMap, ops::Range, sync::Arc};
use swash::FontRef;

/// Quarter-circle handle length for a rounded rectangle.
const KAPPA: f32 = 0.552_284_8;

/// Where one block fragment is drawn on the page.
#[derive(Clone, Copy)]
pub struct Frame {
	/// Page-point x of the block's content origin.
	origin_x: f32,
	/// Page-point y of the fragment's first band top.
	origin_y: f32,
	/// Block-local y of the fragment's first band top.
	top: f32,
	scale: f32,
}
impl Frame {
	fn x(&self, x: f32) -> f32 {
		self.origin_x + x * self.scale * PT_PER_PX
	}
	fn y(&self, y: f32) -> f32 {
		self.origin_y + (y - self.top) * self.scale * PT_PER_PX
	}
	fn size(&self, size: f32) -> f32 {
		self.scale * PT_PER_PX * size
	}
	/// A path vertex of a formula item, in page points: the item's own origin
	/// inside the formula box, then the vertex in em units. Dropping `origin`
	/// would draw radicals and large delimiters at the column's left edge.
	fn math_point(
		&self,
		origin: (f32, f32),
		size: f32,
		vertex: (f64, f64),
	) -> (f32, f32) {
		(
			self.x(origin.0 + vertex.0 as f32 * size),
			self.y(origin.1 + vertex.1 as f32 * size),
		)
	}

	/// A block-local rectangle in page points, or `None` when it is empty.
	fn rect(&self, rect: LocalRect) -> Option<Rect> {
		Rect::from_xywh(
			self.x(rect.x),
			self.y(rect.y),
			self.size(rect.w),
			self.size(rect.h),
		)
	}
}

/// A KaTeX math face: the same bytes swash maps character codes with, and the
/// krilla font they are embedded through.
#[derive(Clone)]
struct MathFont {
	data: FontData,
	font: Font,
}

struct Painter<'a> {
	stylesheet: &'a Stylesheet,
	snapshot: &'a LayoutSnapshot,
	images: Images<'a>,
	geometry: &'a PageGeometry,
	pagination: &'a Pagination,
	fonts: Fonts,
	math_fonts: HashMap<String, MathFont>,
	shaper: TextShaper,
	/// Body text size in points, which page furniture sizes against.
	body_pt: f32,
	links: bool,
	/// The document title, which `{title}` names in page furniture.
	title: Option<String>,
	path: String,
}

pub fn render(input: &Export<'_>) -> Result<Vec<u8>> {
	let mut document = Document::new();
	document.set_metadata(information(&input.metadata));
	let mut painter = Painter {
		stylesheet: input.stylesheet,
		snapshot: input.snapshot,
		images: Images::new(input.images),
		geometry: input.geometry,
		pagination: input.pagination,
		fonts: Fonts::default(),
		math_fonts: HashMap::new(),
		shaper: TextShaper::new(),
		body_pt: input.body_size_px * PT_PER_PX,
		links: input.links,
		title: input.metadata.title.clone(),
		path: input.path.clone(),
	};
	painter
		.shaper
		.set_stylesheet(Arc::new(input.stylesheet.clone()));
	for (index, items) in input.pagination.pages.iter().enumerate() {
		let size =
			Size::from_wh(input.geometry.width_pt, input.geometry.height_pt)
				.context("page size")?;
		let mut page = document.start_page_with(PageSettings::new(size));
		{
			let mut surface = page.surface();
			painter.page(&mut surface, index, items);
		}
		if painter.links {
			for item in items {
				for annotation in painter.annotations(item) {
					page.add_annotation(annotation);
				}
			}
		}
	}
	document
		.finish()
		.map_err(|error| anyhow::anyhow!("PDF: {error}"))
}

impl Painter<'_> {
	fn fill(&self, paint: Paint) -> Fill {
		let (color, alpha) = self.color(paint);
		Fill {
			paint: KrillaPaint::from(color),
			opacity: NormalizedF32::new(alpha).unwrap_or(NormalizedF32::ONE),
			rule: FillRule::NonZero,
		}
	}

	fn color(&self, paint: Paint) -> (rgb::Color, f32) {
		let [r, g, b, a] = self.stylesheet.paint(paint);
		let channel =
			|value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
		(
			rgb::Color::new(channel(r), channel(g), channel(b)),
			a.clamp(0.0, 1.0),
		)
	}

	fn solid(&self, surface: &mut Surface<'_>, rect: Rect, paint: Paint) {
		if let Some(path) = rect_path(rect) {
			surface.set_fill(Some(self.fill(paint)));
			surface.draw_path(&path);
		}
	}

	fn solid_color(
		&self,
		surface: &mut Surface<'_>,
		rect: Rect,
		color: rgb::Color,
		alpha: f32,
	) {
		if let Some(path) = rect_path(rect) {
			surface.set_fill(Some(Fill {
				paint: KrillaPaint::from(color),
				opacity: NormalizedF32::new(alpha)
					.unwrap_or(NormalizedF32::ONE),
				rule: FillRule::NonZero,
			}));
			surface.draw_path(&path);
		}
	}

	fn page(
		&mut self,
		surface: &mut Surface<'_>,
		index: usize,
		items: &[PageItem],
	) {
		let [left, top, width, height] = self.geometry.text_pt();
		// The sheet first, then everything clipped to the text area, so a wide
		// table or an unbreakable token cannot leak into the margins.
		self.solid(
			surface,
			Rect::from_xywh(
				0.0,
				0.0,
				self.geometry.width_pt,
				self.geometry.height_pt,
			)
			.unwrap_or_else(fallback_rect),
			Paint::Styled(Condition::Page, ColorField::Background),
		);
		surface.push_clip_path(
			&rect_path(
				Rect::from_xywh(left, top, width, height)
					.unwrap_or_else(fallback_rect),
			)
			.expect("text area path"),
			&FillRule::NonZero,
		);
		for item in items {
			self.item(surface, item);
		}
		surface.pop();
		self.furniture(surface, index);
	}

	fn furniture(&mut self, surface: &mut Surface<'_>, index: usize) {
		let text = FurnitureText {
			title: self.title.as_deref().unwrap_or_default(),
			path: &self.path,
		};
		let pieces = page_furniture(
			self.stylesheet,
			&mut self.shaper,
			self.geometry,
			self.body_pt,
			index + 1,
			self.pagination.pages.len(),
			&text,
		);
		for piece in &pieces {
			// The ranges belong to the glyph draws in order; the other draws
			// are backgrounds and rules and carry none.
			let mut ranges = piece.ranges.iter();
			let mut index = 0;
			let draws = &piece.draws;
			while index < draws.len() {
				match &draws[index] {
					Draw::Glyph(first) => {
						let mut end = index + 1;
						while end < draws.len() {
							let Draw::Glyph(next) = &draws[end] else {
								break;
							};
							if !text::same_face(first, next) {
								break;
							}
							end += 1;
						}
						let run: Vec<RunGlyph> = draws[index..end]
							.iter()
							.filter_map(|draw| match draw {
								Draw::Glyph(glyph) => Some(RunGlyph {
									id: glyph.id,
									x: glyph.x,
									y: glyph.y,
									range: ranges
										.next()
										.cloned()
										.unwrap_or(0..0),
									synthetic_italic: glyph.synthetic_italic,
								}),
								_ => None,
							})
							.collect();
						surface.set_fill(Some(self.fill(first.paint)));
						text::emit(
							surface,
							&mut self.fonts,
							&first.font,
							&first.coords,
							first.size,
							&piece.text,
							&run,
						);
						index = end;
					}
					Draw::Rect(rect, paint) => {
						if let Some(rect) =
							Rect::from_xywh(rect.x, rect.y, rect.w, rect.h)
						{
							self.solid(surface, rect, *paint);
						}
						index += 1;
					}
					_ => index += 1,
				}
			}
		}
	}

	fn frame(&self, item: &PageItem) -> Frame {
		let [left, top, _, _] = self.geometry.text_pt();
		Frame {
			origin_x: left,
			origin_y: top + item.y * PT_PER_PX,
			top: item.top,
			scale: item.scale,
		}
	}

	fn item(&mut self, surface: &mut Surface<'_>, item: &PageItem) {
		let frame = self.frame(item);
		let layout = &self.snapshot.blocks[item.block].layout;
		let clusters = cluster_map(layout);
		let draws = &layout.draws;
		let mut index = 0;
		while index < draws.len() {
			match &draws[index] {
				Draw::Glyph(_) => {
					index = self.text_run(
						surface, layout, &clusters, index, frame, item,
					);
				}
				_ => {
					self.draw(
						surface,
						&draws[index],
						frame,
						item,
						Some(&clusters[index]),
					);
					index += 1;
				}
			}
		}
	}

	/// Shows one run of glyph draws as a single string. The run ends where the
	/// face or the text node changes.
	fn text_run(
		&mut self,
		surface: &mut Surface<'_>,
		layout: &BlockLayout,
		clusters: &[Cluster],
		start: usize,
		frame: Frame,
		item: &PageItem,
	) -> usize {
		let Draw::Glyph(first) = &layout.draws[start] else {
			return start + 1;
		};
		let node = clusters[start].node;
		let mut end = start + 1;
		while end < layout.draws.len() {
			let Draw::Glyph(next) = &layout.draws[end] else {
				break;
			};
			if clusters[end].node != node || !text::same_face(first, next) {
				break;
			}
			end += 1;
		}
		let text = layout
			.text
			.get(node)
			.map(|text| text.text.as_str())
			.unwrap_or("");
		let glyphs: Vec<RunGlyph> = (start..end)
			.filter(|at| {
				visible(Some(&clusters[*at]), &layout.draws[*at], item)
			})
			.filter_map(|at| {
				let Draw::Glyph(glyph) = &layout.draws[at] else {
					return None;
				};
				Some(RunGlyph {
					id: glyph.id,
					x: frame.x(glyph.x),
					y: frame.y(glyph.y),
					range: clusters[at].range.clone(),
					synthetic_italic: glyph.synthetic_italic,
				})
			})
			.collect();
		if glyphs.is_empty() {
			return end;
		}
		surface.set_fill(Some(self.fill(first.paint)));
		text::emit(
			surface,
			&mut self.fonts,
			&first.font,
			&first.coords,
			frame.size(first.size),
			text,
			&glyphs,
		);
		end
	}

	fn draw(
		&mut self,
		surface: &mut Surface<'_>,
		draw: &Draw,
		frame: Frame,
		item: &PageItem,
		cluster: Option<&Cluster>,
	) {
		if !visible(cluster, draw, item) {
			return;
		}
		match draw {
			Draw::Clipped { rect, draws } => {
				if let Some(path) = frame.rect(*rect).and_then(rect_path) {
					surface.push_clip_path(&path, &FillRule::NonZero);
					for draw in draws {
						self.draw(surface, draw, frame, item, None);
					}
					surface.pop();
				}
			}
			Draw::Glyph(_) => {}
			Draw::Rect(rect, paint) => {
				if let Some(rect) = frame.rect(*rect) {
					self.solid(surface, rect, *paint);
				}
			}
			Draw::Image {
				src, version, rect, ..
			} => {
				let Some(image) = self.images.get(src, *version) else {
					log::debug!("PDF: image {src} has no decoded pixels");
					return;
				};
				let Some(rect) = frame.rect(*rect) else {
					return;
				};
				let Some(size) = Size::from_wh(rect.width(), rect.height())
				else {
					return;
				};
				surface.push_transform(&Transform::from_translate(
					rect.left(),
					rect.top(),
				));
				surface.draw_image(image, size);
				surface.pop();
			}
			Draw::Box {
				rect,
				chain,
				condition,
				radius,
				border,
				left_only,
			} => {
				// A container is painted only where its own rectangle meets
				// this fragment: a nested list or quote can belong entirely to
				// another page, and must not be stretched across this one.
				let box_bottom = rect.y + rect.h;
				if box_bottom <= item.top || rect.y >= item.bottom {
					return;
				}
				// The fragment keeps the box's own edge at its first and last
				// page, so its padding survives a page break.
				let top = if item.first {
					rect.y
				} else {
					rect.y.max(item.top)
				};
				let bottom = if item.last {
					box_bottom
				} else {
					box_bottom.min(item.bottom)
				};
				let owns_top = top == rect.y;
				let owns_bottom = bottom == box_bottom;
				let clipped = LocalRect {
					y: top,
					h: (bottom - top).max(0.0),
					..*rect
				};
				let Some(box_rect) = frame.rect(clipped) else {
					return;
				};
				let radius = if owns_top && owns_bottom {
					frame.size(*radius)
				} else {
					0.0
				};
				surface.set_fill(Some(self.fill(Paint::Scoped(
					*chain,
					*condition,
					ColorField::Background,
				))));
				if let Some(path) =
					rounded_path(box_rect, radius, owns_top, owns_bottom)
				{
					surface.draw_path(&path);
				}
				if *border <= 0.0 {
					return;
				}
				let (color, alpha) = self.color(Paint::Scoped(
					*chain,
					*condition,
					ColorField::BorderColor,
				));
				let width = frame.size(*border);
				if *left_only {
					if let Some(rect) = Rect::from_ltrb(
						box_rect.left(),
						box_rect.top(),
						box_rect.left() + width,
						box_rect.bottom(),
					) {
						self.solid_color(surface, rect, color, alpha);
					}
				} else {
					let mut builder = PathBuilder::new();
					edge(&mut builder, box_rect, owns_top, owns_bottom);
					if let Some(path) = builder.finish() {
						surface.set_fill(None);
						surface.set_stroke(Some(Stroke {
							paint: KrillaPaint::from(color),
							width,
							..Stroke::default()
						}));
						surface.draw_path(&path);
						surface.set_stroke(None);
					}
				}
			}
			Draw::Math { math, paint, x, y } => {
				self.math(surface, math, *paint, frame, *x, *y);
			}
		}
	}

	/// A formula's own colors win where it sets them; otherwise it takes the
	/// document's, exactly as the GPU painter decides.
	fn math_color(&self, color: MathColor, paint: Paint) -> (rgb::Color, f32) {
		if color.r == 0.0 && color.g == 0.0 && color.b == 0.0 {
			return self.color(paint);
		}
		let channel =
			|value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
		(
			rgb::Color::new(
				channel(color.r),
				channel(color.g),
				channel(color.b),
			),
			color.a.clamp(0.0, 1.0),
		)
	}

	/// Math arrives as a flat display list in em units, measured from the
	/// formula's top-left corner.
	fn math(
		&mut self,
		surface: &mut Surface<'_>,
		math: &Arc<MathBox>,
		paint: Paint,
		frame: Frame,
		x: f32,
		y: f32,
	) {
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
					let (color, alpha) = self.math_color(*color, paint);
					let character = ratex_font::FontId::parse(font)
						.map_or_else(
							|| char::from_u32(*char_code).unwrap_or('\u{fffd}'),
							|id| {
								ratex_font::katex_ttf_glyph_char(id, *char_code)
							},
						);
					let glyph_size = frame.size(size * *scale as f32);
					let point = Point::from_xy(
						frame.x(x + *gx as f32 * size),
						frame.y(y + *gy as f32 * size),
					);
					// A CJK or emoji character inside a formula names a face
					// the KaTeX bundle does not hold; the document's own fonts
					// draw it, exactly as the GPU painter falls back.
					let Some(math_font) = self.math_font(font).cloned() else {
						self.math_fallback(
							surface, character, glyph_size, point, color, alpha,
						);
						continue;
					};
					let Some(id) =
						FontRef::from_index(math_font.data.data.data(), 0).map(
							|reference| reference.charmap().map(character),
						)
					else {
						continue;
					};
					if id == 0 {
						continue;
					}
					surface.set_fill(Some(Fill {
						paint: KrillaPaint::from(color),
						opacity: NormalizedF32::new(alpha)
							.unwrap_or(NormalizedF32::ONE),
						rule: FillRule::NonZero,
					}));
					let glyph = KrillaGlyph::new(
						GlyphId::new(u32::from(id)),
						0.0,
						0.0,
						0.0,
						0.0,
						0..0,
						None,
					);
					surface.draw_glyphs(
						point,
						&[glyph],
						math_font.font.clone(),
						"",
						glyph_size,
						false,
					);
				}
				DisplayItem::Line {
					x: lx,
					y: ly,
					width,
					thickness,
					color,
					dashed,
				} => {
					let rect = LocalRect {
						x: x + *lx as f32 * size,
						y: y + *ly as f32 * size,
						w: *width as f32 * size,
						h: (*thickness as f32 * size).max(0.6),
					};
					let (color, alpha) = self.math_color(*color, paint);
					if *dashed {
						let mut left = 0.0;
						while left < rect.w {
							let segment = LocalRect {
								x: rect.x + left,
								w: (rect.w - left).min(size * 0.3),
								..rect
							};
							if let Some(segment) = frame.rect(segment) {
								self.solid_color(
									surface, segment, color, alpha,
								);
							}
							left += size * 0.5;
						}
					} else if let Some(rect) = frame.rect(rect) {
						self.solid_color(surface, rect, color, alpha);
					}
				}
				DisplayItem::Rect {
					x: rx,
					y: ry,
					width,
					height,
					color,
				} => {
					let rect = LocalRect {
						x: x + *rx as f32 * size,
						y: y + *ry as f32 * size,
						w: *width as f32 * size,
						h: *height as f32 * size,
					};
					let (color, alpha) = self.math_color(*color, paint);
					if let Some(rect) = frame.rect(rect) {
						self.solid_color(surface, rect, color, alpha);
					}
				}
				DisplayItem::Path {
					x: px,
					y: py,
					commands,
					fill,
					color,
				} => {
					let mut builder = PathBuilder::new();
					// Path commands are em units from the item's own origin,
					// which is in turn offset inside the formula's box.
					let origin = (x + *px as f32 * size, y + *py as f32 * size);
					let at = |dx: f64, dy: f64| {
						frame.math_point(origin, size, (dx, dy))
					};
					for command in commands {
						match *command {
							PathCommand::MoveTo { x, y } => {
								let (x, y) = at(x, y);
								builder.move_to(x, y);
							}
							PathCommand::LineTo { x, y } => {
								let (x, y) = at(x, y);
								builder.line_to(x, y);
							}
							PathCommand::QuadTo { x1, y1, x, y } => {
								let (x, y) = at(x, y);
								let (x1, y1) = at(x1, y1);
								builder.quad_to(x1, y1, x, y);
							}
							PathCommand::CubicTo {
								x1,
								y1,
								x2,
								y2,
								x,
								y,
							} => {
								let (x, y) = at(x, y);
								let (x1, y1) = at(x1, y1);
								let (x2, y2) = at(x2, y2);
								builder.cubic_to(x1, y1, x2, y2, x, y);
							}
							PathCommand::Close => builder.close(),
						}
					}
					let (color, alpha) = self.math_color(*color, paint);
					let Some(path) = builder.finish() else {
						continue;
					};
					if *fill {
						surface.set_fill(Some(Fill {
							paint: KrillaPaint::from(color),
							opacity: NormalizedF32::new(alpha)
								.unwrap_or(NormalizedF32::ONE),
							rule: FillRule::NonZero,
						}));
						surface.draw_path(&path);
					} else {
						// The GPU rasterizer strokes formulas at 0.04 em.
						let width =
							(size * frame.scale * PT_PER_PX * 0.04).max(0.05);
						surface.set_stroke(Some(Stroke {
							paint: KrillaPaint::from(color),
							width,
							..Stroke::default()
						}));
						surface.draw_path(&path);
						surface.set_stroke(None);
					}
				}
			}
		}
	}

	/// Draws a formula character whose face is not in the KaTeX bundle, such as
	/// the CJK and emoji a `\text{…}` group can hold, with the document's own
	/// fonts. The shaped glyphs carry their byte ranges, so the characters stay
	/// searchable and copyable like the rest of the page's text.
	fn math_fallback(
		&mut self,
		surface: &mut Surface<'_>,
		text: char,
		size: f32,
		point: Point,
		color: rgb::Color,
		alpha: f32,
	) {
		let text = text.to_string();
		let (draws, ranges, _) = self.shaper.label_runs_measured(
			&text,
			size,
			point.x,
			point.y,
			Paint::Text,
		);
		surface.set_fill(Some(Fill {
			paint: KrillaPaint::from(color),
			opacity: NormalizedF32::new(alpha).unwrap_or(NormalizedF32::ONE),
			rule: FillRule::NonZero,
		}));
		let mut ranges = ranges.iter();
		let mut index = 0;
		while index < draws.len() {
			let Draw::Glyph(first) = &draws[index] else {
				index += 1;
				continue;
			};
			let mut end = index + 1;
			while end < draws.len() {
				let Draw::Glyph(next) = &draws[end] else {
					break;
				};
				if !text::same_face(first, next) {
					break;
				}
				end += 1;
			}
			let run: Vec<RunGlyph> = draws[index..end]
				.iter()
				.filter_map(|draw| match draw {
					Draw::Glyph(glyph) => Some(RunGlyph {
						id: glyph.id,
						x: glyph.x,
						y: glyph.y,
						range: ranges.next().cloned().unwrap_or(0..0),
						synthetic_italic: glyph.synthetic_italic,
					}),
					_ => None,
				})
				.collect();
			text::emit(
				surface,
				&mut self.fonts,
				&first.font,
				&first.coords,
				first.size,
				&text,
				&run,
			);
			index = end;
		}
	}

	fn math_font(&mut self, name: &str) -> Option<&MathFont> {
		if !self.math_fonts.contains_key(name) {
			let bytes =
				ratex_katex_fonts::ttf_bytes(&format!("KaTeX_{name}.ttf"))?;
			let data = FontData::new(bytes.into_owned().into(), 0);
			let font = Font::new(
				krilla::Data::from(data.data.clone().into_raw_parts().0),
				0,
			)?;
			self.math_fonts.insert(name.into(), MathFont { data, font });
		}
		self.math_fonts.get(name)
	}

	fn annotations(
		&self,
		item: &PageItem,
	) -> Vec<krilla::annotation::Annotation> {
		let placed: &PlacedBlock = &self.snapshot.blocks[item.block];
		let frame = self.frame(item);
		let mut out = Vec::new();
		for link_rect in &placed.layout.links {
			let rect = link_rect.rect;
			if rect.y + rect.h <= item.top || rect.y >= item.bottom {
				continue;
			}
			let clipped = LocalRect {
				y: rect.y.max(item.top),
				h: (rect.y + rect.h).min(item.bottom) - rect.y.max(item.top),
				..rect
			};
			let Some(page_rect) = frame.rect(clipped) else {
				continue;
			};
			// Annotations do not inherit the content stream's clip path.
			let [left, top, width, height] = self.geometry.text_pt();
			let Some(page_rect) = Rect::from_ltrb(
				page_rect.left().max(left),
				page_rect.top().max(top),
				page_rect.right().min(left + width),
				page_rect.bottom().min(top + height),
			) else {
				continue;
			};
			if let Some(annotation) = link::annotation(
				page_rect,
				&link_rect.url,
				self.pagination,
				self.geometry,
			) {
				out.push(annotation);
			}
		}
		out
	}
}

/// Which text node, and which bytes in it, every glyph draw of one block came
/// from. A cluster records the draw index it starts at, so a glyph belongs to
/// the last cluster that started at or before it.
#[derive(Clone, Default)]
struct Cluster {
	node: usize,
	range: Range<usize>,
	/// The cluster's line box, in block-local y.
	span: Option<(f32, f32)>,
}

/// Where one cluster starts in the draw list, and what it is.
struct ClusterSpan {
	command: usize,
	cluster: Cluster,
}

fn cluster_map(layout: &BlockLayout) -> Vec<Cluster> {
	let mut out = vec![Cluster::default(); layout.draws.len()];
	let mut spans: Vec<ClusterSpan> = Vec::new();
	for (node, text) in layout.text.iter().enumerate() {
		for cluster in &text.clusters {
			spans.push(ClusterSpan {
				command: cluster.command,
				cluster: Cluster {
					node,
					range: cluster.range.clone(),
					span: Some((
						cluster.rect.y,
						cluster.rect.y + cluster.rect.h.max(0.0),
					)),
				},
			});
		}
	}
	spans.sort_by_key(|span| span.command);
	for (index, span) in spans.iter().enumerate() {
		let end = spans
			.get(index + 1)
			.map(|next| next.command)
			.unwrap_or(layout.draws.len())
			.min(layout.draws.len());
		for entry in out.iter_mut().take(end).skip(span.command) {
			*entry = span.cluster.clone();
		}
	}
	out
}

/// Whether a draw belongs to the part of its block that this page carries.
///
/// A block is painted fragment by fragment. Skipping the draws of the other
/// bands rather than painting them and relying on the page clip matters at a
/// break: the room the page builder leaves there is exactly where the next
/// fragment's first lines would otherwise show through, cut in half.
fn visible(cluster: Option<&Cluster>, draw: &Draw, item: &PageItem) -> bool {
	let span = match draw {
		Draw::Glyph(glyph) => {
			cluster.and_then(|cluster| cluster.span).unwrap_or((
				glyph.y - glyph.size * 0.9,
				glyph.y + glyph.size * 0.25,
			))
		}
		Draw::Rect(rect, _) => (rect.y, rect.y + rect.h),
		Draw::Image { rect, .. } => (rect.y, rect.y + rect.h),
		Draw::Math { math, y, .. } => (*y, *y + math.ascent + math.descent),
		// A container spans its whole block; each fragment clips its own part.
		Draw::Box { .. } | Draw::Clipped { .. } => return true,
	};
	span.1 > item.top && span.0 < item.bottom
}

/// The PDF information dictionary. Only what the user asked for is written:
/// an absent title, author or language leaves the entry out. The producer
/// always names this build, and a creation date is never invented, which is
/// what keeps two exports byte-identical.
fn information(metadata: &crate::Metadata) -> Metadata {
	let mut out = Metadata::new()
		.producer(format!("Markview {}", env!("CARGO_PKG_VERSION")));
	if let Some(title) = &metadata.title {
		out = out.title(title.clone());
	}
	if !metadata.authors.is_empty() {
		out = out.authors(metadata.authors.clone());
	}
	if let Some(subject) = &metadata.subject {
		out = out.description(subject.clone());
	}
	if !metadata.keywords.is_empty() {
		out = out.keywords(metadata.keywords.clone());
	}
	if let Some(language) = &metadata.language {
		out = out.language(language.clone());
	}
	if let Some(creator) = &metadata.creator {
		out = out.creator(creator.clone());
	}
	out
}

fn fallback_rect() -> Rect {
	Rect::from_ltrb(0.0, 0.0, 1.0, 1.0).expect("unit rect")
}

fn rect_path(rect: Rect) -> Option<Path> {
	let mut builder = PathBuilder::new();
	builder.push_rect(rect);
	builder.finish()
}

/// A rectangle whose corners are rounded only where the fragment is the
/// block's own edge, so a split container keeps square corners at the break.
fn rounded_path(
	rect: Rect,
	radius: f32,
	first: bool,
	last: bool,
) -> Option<Path> {
	let radius = radius.clamp(0.0, rect.width().min(rect.height()) * 0.5);
	let (left, top, right, bottom) =
		(rect.left(), rect.top(), rect.right(), rect.bottom());
	let top_corner = if first { radius } else { 0.0 };
	let bottom_corner = if last { radius } else { 0.0 };
	let mut builder = PathBuilder::new();
	builder.move_to(left + top_corner, top);
	builder.line_to(right - top_corner, top);
	if top_corner > 0.0 {
		let r = top_corner;
		builder.cubic_to(
			right - r + r * KAPPA,
			top,
			right,
			top + r - r * KAPPA,
			right,
			top + r,
		);
	}
	builder.line_to(right, bottom - bottom_corner);
	if bottom_corner > 0.0 {
		let r = bottom_corner;
		builder.cubic_to(
			right,
			bottom - r + r * KAPPA,
			right - r + r * KAPPA,
			bottom,
			right - r,
			bottom,
		);
	}
	builder.line_to(left + bottom_corner, bottom);
	if bottom_corner > 0.0 {
		let r = bottom_corner;
		builder.cubic_to(
			left + r - r * KAPPA,
			bottom,
			left,
			bottom - r + r * KAPPA,
			left,
			bottom - r,
		);
	}
	builder.line_to(left, top + top_corner);
	if top_corner > 0.0 {
		let r = top_corner;
		builder.cubic_to(
			left,
			top + r - r * KAPPA,
			left + r - r * KAPPA,
			top,
			left + r,
			top,
		);
	}
	builder.close();
	builder.finish()
}

/// The container edges a fragment paints: its sides always, its top only on
/// the first fragment and its bottom only on the last.
fn edge(builder: &mut PathBuilder, rect: Rect, first: bool, last: bool) {
	let (left, top, right, bottom) =
		(rect.left(), rect.top(), rect.right(), rect.bottom());
	builder.move_to(left, top);
	builder.line_to(left, bottom);
	builder.move_to(right, top);
	builder.line_to(right, bottom);
	if first {
		builder.move_to(left, top);
		builder.line_to(right, top);
	}
	if last {
		builder.move_to(left, bottom);
		builder.line_to(right, bottom);
	}
}

#[cfg(test)]
mod tests {
	use super::Frame;

	#[test]
	fn a_formula_path_is_placed_from_the_formulas_own_origin() {
		// One device pixel per layout pixel, so the point is readable.
		let frame = Frame {
			origin_x: 100.0,
			origin_y: 200.0,
			top: 0.0,
			scale: 1.0,
		};
		// A formula 50px into the column, an item 1em into the formula, and a
		// vertex 2em into the item: everything is counted.
		let point =
			frame.math_point((50.0 + 18.0, 8.0 + 0.5 * 18.0), 18.0, (2.0, 0.0));
		assert_eq!(point.0, 100.0 + (50.0 + 18.0 + 36.0) * 0.75);
		assert_eq!(point.1, 200.0 + (8.0 + 9.0) * 0.75);
	}
}
