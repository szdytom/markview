//! Shared font shaping for document text, labels and renderer fallbacks.
use crate::style::{Condition, Font, Stylesheet, TextAppearance, Variant};
use crate::{
	document::TextStyle,
	scene::{Draw, Glyph, Paint},
};
use anyhow::Result;
use parley::{
	FontContext, FontStyle, FontWeight, LayoutContext, StyleProperty,
};
use std::{
	collections::{HashMap, HashSet},
	io::Write,
	ops::Range,
	sync::Arc,
};
use unicode_segmentation::UnicodeSegmentation;
#[derive(Clone)]
struct Face {
	family: String,
	font: parley::FontData,
	style: FontStyle,
	weight: u16,
}

#[derive(Default)]
struct FontSet {
	faces: Vec<Face>,
	choices: HashMap<String, Option<usize>>,
	diagnostic_key: u64,
	diagnostic_fonts: Vec<Font>,
	diagnostic_weight: u16,
}
type FaceChoice = Option<(usize, usize)>;
impl FontSet {
	fn choose(&mut self, text: &str) -> Option<usize> {
		if let Some(choice) = self.choices.get(text) {
			return *choice;
		}
		let choice = self.faces.iter().position(|face| {
			swash::FontRef::from_index(
				face.font.data.data(),
				face.font.index as usize,
			)
			.is_some_and(|font| {
				let charmap = font.charmap();
				text.chars().all(|c| {
					c.is_control()
						|| matches!(c as u32,0x200c..=0x200f|0xfe00..=0xfe0f|0xe0100..=0xe01ef)
						|| charmap.map(c) != 0
				})
			})
		});
		// Bound retained text even for documents containing unique joining words.
		// Cache misses (including unsupported clusters) preserve the same scan.
		if text.len() <= 128 && self.choices.len() < 4096 {
			self.choices.insert(text.to_owned(), choice);
		}
		choice
	}
}

#[derive(Clone)]
pub(crate) struct Span {
	pub(crate) range: Range<usize>,
	pub(crate) style: TextStyle,
}
#[derive(Clone)]
pub(crate) struct Cluster {
	pub(crate) rtl: bool,
	pub(crate) range: Range<usize>,
	pub(crate) width: f32,
	pub(crate) ascent: f32,
	pub(crate) descent: f32,
	pub(crate) glyphs: Vec<Glyph>,
	pub(crate) continuation: bool,
}
/// Reusable shaping context for UI labels and document text.
pub struct TextShaper {
	fonts: FontContext,
	context: LayoutContext<usize>,
	pub stylesheet: Arc<Stylesheet>,
	pub appearance: TextAppearance,
	faces: HashMap<(Vec<Font>, u16), usize>,
	font_sets: Vec<FontSet>,
	// Retained across reflows and stylesheet resets; no document text is stored.
	warned_fallbacks: HashSet<u64>,
}
impl Default for TextShaper {
	fn default() -> Self {
		Self::new()
	}
}
impl TextShaper {
	pub fn new() -> Self {
		Self {
			fonts: FontContext::new(),
			context: LayoutContext::new(),
			stylesheet: Stylesheet::bundled(false),
			appearance: Stylesheet::bundled(false)
				.text(&TextAppearance::default(), Condition::Body),
			faces: HashMap::new(),
			font_sets: Vec::new(),
			warned_fallbacks: HashSet::new(),
		}
	}
	pub fn set_stylesheet(&mut self, stylesheet: Arc<Stylesheet>) {
		self.faces.clear();
		self.font_sets.clear();
		self.appearance =
			stylesheet.text(&TextAppearance::default(), Condition::Body);
		self.stylesheet = stylesheet;
	}
	pub fn validate_stylesheet(
		&mut self,
		_stylesheet: &Stylesheet,
	) -> Result<()> {
		// An unavailable lookfor list is an ignored fallback, not an invalid
		// stylesheet. choose_font skips it and continues with later candidates.
		Ok(())
	}
	#[cfg(test)]
	fn choose_font(
		&mut self,
		text: &str,
		appearance: &TextAppearance,
	) -> Option<Face> {
		let index = self.resolve_fonts(appearance);
		let set = &mut self.font_sets[index];
		set.choose(text).map(|i| set.faces[i].clone())
	}
	fn resolve_fonts(&mut self, appearance: &TextAppearance) -> usize {
		let key = (appearance.font.clone(), appearance.weight);
		if !self.faces.contains_key(&key) {
			let mut faces = Vec::new();
			for candidate in &appearance.font {
				let style = match candidate.variant {
					Variant::Normal => FontStyle::Normal,
					Variant::Italic => FontStyle::Italic,
					Variant::Oblique => FontStyle::Oblique(None),
				};
				let weight = candidate.weight.unwrap_or(appearance.weight);
				let families: Vec<_> = if let Some(def) =
					self.stylesheet.fontdefs.get(&candidate.family)
				{
					def.lookfor
						.iter()
						.find_map(|name| {
							let generic = match name.as_str() {
								"serif" => Some(parley::GenericFamily::Serif),
								"sans-serif" => {
									Some(parley::GenericFamily::SansSerif)
								}
								"monospace" => {
									Some(parley::GenericFamily::Monospace)
								}
								_ => None,
							};
							if let Some(generic) = generic {
								let ids: Vec<_> = self
									.fonts
									.collection
									.generic_families(generic)
									.collect();
								ids.into_iter().find_map(|id| {
									self.fonts.collection.family(id)
								})
							} else {
								self.fonts.collection.family_by_name(name)
							}
						})
						.into_iter()
						.collect()
				} else {
					// Partial stylesheets used by low-level callers may omit the
					// fontdef table entirely. A declared-but-unselected variant,
					// however, is intentionally unavailable.
					if self.stylesheet.has_fontdef_variant(&candidate.family) {
						Vec::new()
					} else {
						self.fonts
							.collection
							.family_by_name(&candidate.family)
							.into_iter()
							.collect()
					}
				};
				for family in families {
					let Some(info) = family.match_font(
						Default::default(),
						style,
						FontWeight::new(weight as f32),
						false,
					) else {
						continue;
					};
					let axis = |tag: &[u8; 4], value: f32| {
						info.axes().iter().any(|a| {
							a.tag.to_be_bytes() == *tag
								&& a.min <= value && value <= a.max
						})
					};
					let exact_style = info.style() == style
						|| (candidate.variant == Variant::Oblique
							&& matches!(info.style(), FontStyle::Oblique(_)))
						|| match candidate.variant {
							Variant::Italic => axis(b"ital", 1.),
							Variant::Oblique => axis(b"slnt", -14.),
							Variant::Normal => {
								axis(b"ital", 0.) || axis(b"slnt", 0.)
							}
						};
					let exact_weight = info.weight()
						== FontWeight::new(weight as f32)
						|| axis(b"wght", weight as f32);
					if !exact_style || !exact_weight {
						continue;
					}
					if let Some(data) =
						info.load(Some(&mut self.fonts.source_cache))
					{
						faces.push(Face {
							family: family.name().into(),
							font: parley::FontData::new(data, info.index()),
							style: if matches!(
								info.style(),
								FontStyle::Oblique(_)
							) && candidate.variant == Variant::Oblique
							{
								info.style()
							} else {
								style
							},
							weight,
						});
					}
				}
			}
			self.faces.insert(key.clone(), self.font_sets.len());
			self.font_sets.push(FontSet {
				diagnostic_key: crate::document::fingerprint(&key),
				diagnostic_fonts: appearance.font.clone(),
				diagnostic_weight: appearance.weight,
				faces,
				..Default::default()
			});
		}
		self.faces[&key]
	}
	fn fallback_warning(&mut self, fonts: usize, text: &str) -> Option<String> {
		const LIMIT: usize = 64;
		// Inline images/math use an object replacement character for layout,
		// not a visible glyph. Do not report it as a missing user font.
		if text.chars().all(|c| c == '\u{fffc}' || c.is_control()) {
			return None;
		}
		let set = &self.font_sets[fonts];
		if self.warned_fallbacks.len() >= LIMIT
			|| !self.warned_fallbacks.insert(set.diagnostic_key)
		{
			return None;
		}
		let codes = text
			.chars()
			.take(8)
			.map(|c| format!("U+{:04X}", c as u32))
			.collect::<Vec<_>>()
			.join(" ");
		let resolved = set
			.faces
			.iter()
			.map(|f| format!("{:?} (weight {})", f.family, f.weight))
			.collect::<Vec<_>>()
			.join(", ");
		let requested = set
			.diagnostic_fonts
			.iter()
			.map(|f| {
				format!(
					"{:?} ({:?}, weight {})",
					f.family,
					f.variant,
					f.weight.unwrap_or(set.diagnostic_weight)
				)
			})
			.collect::<Vec<_>>()
			.join(", ");
		Some(format!(
			"markview: warning: font fallback to Parley/system for [{codes}]: no configured face covers the entire cluster/word. Requested: [{}]. Available exact faces: [{}]. Check installed fonts and candidate weight/variant (Emoji fonts commonly require weight = 400).{}",
			requested,
			resolved,
			if self.warned_fallbacks.len() == LIMIT {
				" Further font fallback warnings suppressed for this text shaper."
			} else {
				" Repeated warnings for this candidate set are suppressed."
			}
		))
	}
	pub(crate) fn shape(
		&mut self,
		text: &str,
		spans: &[Span],
		size: f32,
		_sans: bool,
	) -> Vec<Cluster> {
		if text.is_empty() {
			return Vec::new();
		}
		let base = self.appearance.clone();
		let appearances: Vec<_> = spans
			.iter()
			.map(|span| self.stylesheet.inline(&base, &span.style))
			.collect();
		let (base_fonts, span_fonts) =
			crate::profile::span(crate::profile::Stage::FontResolve, || {
				let base_fonts = self.resolve_fonts(&base);
				let span_fonts: Vec<_> = appearances
					.iter()
					.map(|appearance| self.resolve_fonts(appearance))
					.collect();
				(base_fonts, span_fonts)
			});
		let mut choices: Vec<(Range<usize>, FaceChoice)> = Vec::new();
		crate::profile::span(crate::profile::Stage::FontChoose, || {
			// Resolve whole joining-script words together; elsewhere resolve grapheme clusters.
			// All ranges are subsequently shaped in one paragraph, preserving bidi and context.
			for (start, word) in text.split_word_bound_indices() {
				let joining = word.chars().any(
					|c| matches!(c as u32,0x600..=0x1cff|0xa800..=0xabff|0x11000..=0x11fff),
				);
				let mut parts: Vec<(usize, &str)> = Vec::new();
				for (offset, cluster) in word.grapheme_indices(true) {
					let span_at =
						|pos| spans.iter().position(|s| s.range.contains(&pos));
					if joining
						&& let Some((previous, part)) = parts.last_mut()
						&& span_at(start + *previous) == span_at(start + offset)
					{
						*part = &word[*previous..offset + cluster.len()];
					} else {
						parts.push((offset, cluster));
					}
				}
				for (offset, part) in parts {
					let pos = start + offset;
					let fonts = spans
						.iter()
						.position(|s| s.range.contains(&pos))
						.map(|i| span_fonts[i])
						.unwrap_or(base_fonts);
					let face =
						self.font_sets[fonts].choose(part).map(|i| (fonts, i));
					if face.is_none()
						&& let Some(warning) =
							self.fallback_warning(fonts, part)
					{
						let _ = writeln!(std::io::stderr().lock(), "{warning}");
					}
					let identity = |choice: FaceChoice| {
						choice.map(|(set, index)| {
							let f = &self.font_sets[set].faces[index];
							(&f.family, f.style, f.weight)
						})
					};
					if let Some((range, previous)) = choices.last_mut()
						&& range.end == pos
						&& identity(face) == identity(*previous)
					{
						range.end = pos + part.len();
						continue;
					}
					choices.push((pos..pos + part.len(), face));
				}
			}
		});
		let mut builder =
			self.context
				.ranged_builder(&mut self.fonts, text, 1.0, false);
		builder.push_default(StyleProperty::FontSize(size));
		builder.push_default(StyleProperty::FontFamily("sans-serif".into()));
		builder.push_default(StyleProperty::FontWeight(FontWeight::NORMAL));
		builder.push_default(StyleProperty::FontStyle(FontStyle::Normal));
		builder.push_default(StyleProperty::Brush(usize::MAX));
		for (range, face) in &choices {
			if let Some((set, index)) = face {
				let face = &self.font_sets[*set].faces[*index];
				builder.push(
					StyleProperty::FontFamily(
						parley::FontFamilyName::Named(
							face.family.as_str().into(),
						)
						.into(),
					),
					range.clone(),
				);
				builder
					.push(StyleProperty::FontStyle(face.style), range.clone());
				builder.push(
					StyleProperty::FontWeight(FontWeight::new(
						face.weight as f32,
					)),
					range.clone(),
				);
			}
		}
		for (i, span) in spans.iter().enumerate() {
			builder.push(StyleProperty::Brush(i), span.range.clone());
			builder.push(
				StyleProperty::FontSize(size * appearances[i].size),
				span.range.clone(),
			);
		}
		let mut layout =
			crate::profile::span(crate::profile::Stage::ShapeBuild, || {
				builder.build(text)
			});
		layout.break_all_lines(None);
		let mut clusters = Vec::new();
		for line in layout.lines() {
			for run in line.runs() {
				let coords: Arc<[i16]> = run.normalized_coords().into();
				for c in run.visual_clusters() {
					let mut x = 0.0;
					let mut glyphs = Vec::new();
					for g in c.glyphs() {
						let index = layout.styles()[g.style_index()].brush;
						let style = spans.get(index).map(|s| &s.style);
						let rise = if style.is_some_and(|s| s.superscript) {
							size * 0.35
						} else {
							0.0
						};
						glyphs.push(Glyph {
							font: run.font().clone(),
							coords: coords.clone(),
							id: g.id as u16,
							size: run.font_size(),
							x: x + g.x,
							y: g.y - rise,
							paint: appearances
								.get(index)
								.unwrap_or(&base)
								.paint,
						});
						x += g.advance;
					}
					clusters.push(Cluster {
						rtl: c.is_rtl(),
						range: c.text_range(),
						width: c.advance(),
						ascent: run.metrics().ascent,
						descent: run.metrics().descent,
						glyphs,
						continuation: c.is_ligature_continuation(),
					});
				}
			}
		}
		clusters
	}

	/// A reader-chrome label. The appearance comes from `paint`'s condition,
	/// which is always a UI condition for chrome.
	pub fn label(
		&mut self,
		text: &str,
		size: f32,
		x: f32,
		baseline: f32,
		paint: Paint,
	) -> Vec<Draw> {
		self.label_measured(text, size, x, baseline, paint).0
	}

	/// A label together with the advance width it occupies, so a caller that
	/// needs both does not shape the text twice.
	pub fn label_measured(
		&mut self,
		text: &str,
		size: f32,
		x: f32,
		baseline: f32,
		paint: Paint,
	) -> (Vec<Draw>, f32) {
		let old = self.appearance.clone();
		let condition = match paint {
			Paint::Styled(c, _) => c,
			Paint::Scoped(_, c, _) => c,
			_ => Condition::Ui,
		};
		let parent = if condition.ui() {
			self.stylesheet
				.text(&TextAppearance::default(), Condition::Ui)
		} else {
			old.clone()
		};
		let appearance = self.stylesheet.text(&parent, condition);
		let paint = if matches!(
			paint,
			Paint::Styled(_, crate::style::ColorField::Color)
		) {
			appearance.paint
		} else {
			paint
		};
		let background = (!condition.ui()).then_some(Paint::Styled(
			condition,
			crate::style::ColorField::Background,
		));
		self.label_with(text, size, x, baseline, &appearance, paint, background)
	}

	/// Shape a label with an appearance that the caller already resolved, so a
	/// text element keeps its own typography instead of the UI default.
	#[expect(
		clippy::too_many_arguments,
		reason = "Label text, geometry, appearance and paints are independent inputs"
	)]
	pub fn label_with(
		&mut self,
		text: &str,
		size: f32,
		x: f32,
		baseline: f32,
		appearance: &TextAppearance,
		paint: Paint,
		background: Option<Paint>,
	) -> (Vec<Draw>, f32) {
		let old = self.appearance.clone();
		self.appearance = appearance.clone();
		let decoration = appearance.decoration.clone();
		let clusters = self.shape(text, &[], size * appearance.size, true);
		self.appearance = old;
		let mut draws = Vec::new();
		let mut cursor = x;
		if let Some(background) = background {
			let width = clusters.iter().map(|c| c.width).sum();
			let ascent = clusters.iter().map(|c| c.ascent).fold(0., f32::max);
			let descent = clusters.iter().map(|c| c.descent).fold(0., f32::max);
			draws.push(Draw::Rect(
				crate::scene::Rect {
					x,
					y: baseline - ascent,
					w: width,
					h: ascent + descent,
				},
				background,
			));
		}
		for c in clusters {
			for mut g in c.glyphs {
				g.x += cursor;
				g.y += baseline;
				g.paint = paint;
				draws.push(Draw::Glyph(g));
			}
			cursor += c.width;
		}
		for d in decoration {
			draws.push(Draw::Rect(
				crate::scene::Rect {
					x,
					y: if d == crate::style::Decoration::Strike {
						baseline - size * 0.3
					} else {
						baseline + size * 0.12
					},
					w: cursor - x,
					h: 1.,
				},
				paint,
			));
		}
		(draws, cursor - x)
	}

	/// Advance width of a UI label at `size`.
	pub fn text_width(&mut self, text: &str, size: f32) -> f32 {
		self.shape(text, &[], size * self.appearance.size, true)
			.iter()
			.map(|c| c.width)
			.sum()
	}

	/// Shorten `text` to `max` width, keeping its start and end like a browser.
	pub fn fit(&mut self, text: &str, size: f32, max: f32) -> String {
		if self.text_width(text, size) <= max {
			return text.to_string();
		}
		let chars: Vec<&str> = text.graphemes(true).collect();
		let mut tail = 16.min(chars.len() / 3);
		while tail > 0
			&& self.text_width(
				&format!("…{}", chars[chars.len() - tail..].concat()),
				size,
			) > max
		{
			tail -= 1;
		}
		if self.text_width("…", size) > max {
			return String::new();
		}
		let build = |head: usize| {
			let mut out = chars[..head].concat();
			out.push('…');
			out.push_str(&chars[chars.len() - tail..].concat());
			out
		};
		let (mut lo, mut hi) = (0, chars.len() - tail);
		while lo < hi {
			let mid = (lo + hi).div_ceil(2);
			if self.text_width(&build(mid), size) <= max {
				lo = mid;
			} else {
				hi = mid - 1;
			}
		}
		build(lo)
	}

	/// A right-aligned label, trimmed to `max` width.
	pub fn right_label(
		&mut self,
		text: &str,
		size: f32,
		max: f32,
		right: f32,
		baseline: f32,
		paint: Paint,
	) -> Vec<Draw> {
		let text = self.fit(text, size, max);
		let width = self.text_width(&text, size);
		self.label(&text, size, (right - width).max(0.0), baseline, paint)
	}
}

#[cfg(test)]
mod stylesheet_tests;
