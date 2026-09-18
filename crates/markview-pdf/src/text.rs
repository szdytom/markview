//! Font embedding and glyph runs.
//!
//! Markview has already chosen every glyph and its position, so an exported
//! run is a list of glyph ids with advances taken from the layout itself.
//! krilla turns that into a font subset with a character map, which is what
//! makes the text in the PDF searchable and copyable.
use krilla::{
	Data,
	geom::{Point, Transform},
	surface::Surface,
	text::{Font, GlyphId, KrillaGlyph, Tag},
};
use markview_core::layout::Glyph;
use markview_core::style::SYNTHETIC_ITALIC_ANGLE_DEG;
use parley::FontData;
use skrifa::MetadataProvider;
use std::{collections::HashMap, ops::Range};
use swash::FontRef;

/// One glyph of a run, in page points.
pub struct RunGlyph {
	pub id: u16,
	pub x: f32,
	/// Baseline, in page points.
	pub y: f32,
	/// The byte range of the text this glyph came from; empty when the glyph
	/// has no reading text of its own, as a code block's language label does.
	pub range: Range<usize>,
	/// The face carries no italic, so the run is sheared about its baseline.
	pub synthetic_italic: bool,
}

/// Font instances, embedded once per export.
#[derive(Default)]
pub struct Fonts {
	instances: HashMap<Key, Option<Font>>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
	/// `Blob`'s process-unique id names the file without hashing it.
	file: u64,
	index: u32,
	coords: Vec<i16>,
}

impl Fonts {
	pub fn get(&mut self, font: &FontData, coords: &[i16]) -> Option<Font> {
		let key = Key {
			file: font.data.id(),
			index: font.index,
			coords: coords.to_vec(),
		};
		self.instances
			.entry(key)
			.or_insert_with(|| instance(font, coords))
			.clone()
	}
}

/// Builds the krilla font for one face, at the variation instance the layout
/// used. A face krilla cannot read is reported once and skipped.
fn instance(font: &FontData, coords: &[i16]) -> Option<Font> {
	// The blob keeps its own allocation, so handing the arc over embeds the
	// font data without copying it.
	let (data, _) = font.data.clone().into_raw_parts();
	let data = Data::from(data);
	let font = if coords.is_empty() {
		Font::new(data, font.index)
	} else {
		let settings = variation_settings(font.data.data(), font.index, coords);
		Font::new_variable(data, font.index, &settings)
	};
	if font.is_none() {
		log::warn!("PDF: unsupported font face, its text is dropped");
	}
	font
}

/// The variation settings a PDF font takes, in the design space of the axes.
///
/// The layout records normalized F2Dot14 coordinates, which is what shaping
/// used, while the font API takes settings such as `wght = 700` and normalizes
/// them itself: passing the normalized values would clamp every axis to its
/// minimum. Each coordinate is mapped back through its own axis range.
fn variation_settings(
	bytes: &[u8],
	index: u32,
	coords: &[i16],
) -> Vec<(Tag, f32)> {
	let Ok(font) = skrifa::FontRef::from_index(bytes, index) else {
		return Vec::new();
	};
	font.axes()
		.iter()
		.zip(coords)
		.map(|(axis, value)| {
			let normalized = *value as f32 / 16384.0;
			let (min, default, max) =
				(axis.min_value(), axis.default_value(), axis.max_value());
			(
				Tag::new(&axis.tag().to_be_bytes()),
				design_value(min, default, max, normalized),
			)
		})
		.collect()
}

/// One axis setting in design space, from a normalized coordinate: the inverse
/// of the fvar normalization, which maps `[-1, 1]` across the axis range on
/// either side of its default.
fn design_value(min: f32, default: f32, max: f32, normalized: f32) -> f32 {
	let span = if normalized >= 0.0 {
		max - default
	} else {
		default - min
	};
	(default + normalized * span).clamp(min, max)
}

/// Draws one run, advancing from the layout's own glyph positions. The glyph
/// positions are absolute, so the run's advances are exactly the distances the
/// layout chose, justification included.
pub fn emit(
	surface: &mut Surface<'_>,
	fonts: &mut Fonts,
	font: &FontData,
	coords: &[i16],
	size: f32,
	text: &str,
	glyphs: &[RunGlyph],
) {
	let Some(first) = glyphs.first() else {
		return;
	};
	let Some(pdf_font) = fonts.get(font, coords) else {
		return;
	};
	let mut out = Vec::with_capacity(glyphs.len());
	for (index, glyph) in glyphs.iter().enumerate() {
		// The last glyph of a run has no successor to measure against, so it
		// falls back to the face's own advance. Only its width entry in the
		// font subset depends on it.
		let advance = glyphs
			.get(index + 1)
			.map(|next| (next.x - glyph.x) / size)
			.unwrap_or_else(|| {
				natural_advance(font, coords, glyph.id, size) / size
			});
		// A super- or subscript shares the run but not the baseline.
		out.push(KrillaGlyph::new(
			GlyphId::new(u32::from(glyph.id)),
			advance,
			0.0,
			(first.y - glyph.y) / size,
			0.0,
			glyph.range.clone(),
			None,
		));
	}
	let shear = first.synthetic_italic.then(|| synthetic_shear(first.y));
	if let Some(transform) = &shear {
		surface.push_transform(transform);
	}
	surface.draw_glyphs(
		Point::from_xy(first.x, first.y),
		&out,
		pdf_font,
		text,
		size,
		false,
	);
	if shear.is_some() {
		surface.pop();
	}
}

/// The shear a synthetic italic run needs, about its baseline. Page coordinates
/// grow downward, so the top of a glyph leans right when `kx` is negative, and
/// carrying the baseline into `tx` keeps the pen advancing horizontally.
fn synthetic_shear(baseline: f32) -> Transform {
	let slant = SYNTHETIC_ITALIC_ANGLE_DEG.to_radians().tan();
	Transform::from_row(1.0, 0.0, -slant, 1.0, slant * baseline, 0.0)
}

/// The face's own advance for one glyph, at the given size.
fn natural_advance(font: &FontData, coords: &[i16], id: u16, size: f32) -> f32 {
	let Some(reference) =
		FontRef::from_index(font.data.data(), font.index as usize)
	else {
		return 0.0;
	};
	reference
		.glyph_metrics(coords)
		.scale(size)
		.advance_width(id)
}

/// Whether two glyphs can share one run. The paint belongs to the run, not to
/// the glyph, so a syntax-highlighted line ends its run wherever the color
/// changes.
pub fn same_face(a: &Glyph, b: &Glyph) -> bool {
	a.font.data == b.font.data
		&& a.font.index == b.font.index
		&& a.size == b.size
		&& a.coords == b.coords
		&& a.paint == b.paint
		&& a.synthetic_italic == b.synthetic_italic
}

#[cfg(test)]
mod tests {
	use super::{SYNTHETIC_ITALIC_ANGLE_DEG, design_value, synthetic_shear};

	#[test]
	fn normalized_coordinates_become_design_settings() {
		// A weight axis: passing the normalized 0.5 through as `wght = 0.5`
		// would clamp to the axis minimum, so it is mapped over the range.
		assert_eq!(design_value(100.0, 400.0, 900.0, 0.0), 400.0);
		assert_eq!(design_value(100.0, 400.0, 900.0, 1.0), 900.0);
		assert_eq!(design_value(100.0, 400.0, 900.0, -1.0), 100.0);
		assert_eq!(design_value(100.0, 400.0, 900.0, 0.5), 650.0);
		assert_eq!(design_value(100.0, 400.0, 900.0, -0.5), 250.0);
		// A one-sided axis still maps both directions, and an out-of-range
		// coordinate cannot escape the axis.
		assert_eq!(design_value(200.0, 400.0, 400.0, -1.0), 200.0);
		assert_eq!(design_value(100.0, 400.0, 900.0, 4.0), 900.0);
	}

	#[test]
	fn a_synthetic_shear_leans_the_top_of_a_glyph_to_the_right() {
		// Page y grows downward, so above the baseline is a smaller y and a
		// negative `kx` moves it right. The baseline itself must not move.
		let transform = synthetic_shear(100.0);
		let slant = SYNTHETIC_ITALIC_ANGLE_DEG.to_radians().tan();
		assert!((transform.kx() + slant).abs() < 1e-6);
		assert!(transform.kx() < 0.0);
		assert!((transform.tx() + transform.kx() * 100.0).abs() < 1e-6);
	}
}
