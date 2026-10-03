//! Independent layout and decoration edges, measured relative to a baseline.
use super::TextShaper;
use crate::style::{TextAppearance, TextEdge, TextMetric};
use parley::{FontData, RunMetrics};
use skrifa::{
	GlyphId, MetadataProvider,
	instance::{NormalizedCoord, Size},
};
use std::sync::Arc;

pub(super) type BoundsKey = (u64, u32, Arc<[i16]>, u16);

pub(super) fn blank(text: &str) -> bool {
	use icu_properties::{
		CodePointSetDataBorrowed, props::DefaultIgnorableCodePoint,
	};
	let ignorable =
		CodePointSetDataBorrowed::new::<DefaultIgnorableCodePoint>();
	text.chars()
		.all(|c| c.is_whitespace() || c.is_control() || ignorable.contains(c))
}

impl TextEdge {
	pub(super) fn position(
		self,
		metrics: &RunMetrics,
		size: f32,
		bounds: (f32, f32),
		top: bool,
	) -> f32 {
		match self {
			Self::Em(v) => v * size,
			Self::Metric(metric) => match metric {
				TextMetric::Ascender => metrics.ascent,
				TextMetric::Descender => -metrics.descent,
				TextMetric::CapHeight => metrics
					.cap_height
					.filter(|v| v.is_finite() && *v > 0.)
					.unwrap_or(metrics.ascent),
				TextMetric::XHeight => metrics
					.x_height
					.filter(|v| v.is_finite() && *v > 0.)
					.unwrap_or(metrics.ascent),
				TextMetric::Baseline => 0.,
				TextMetric::Bounds => {
					if top {
						bounds.1
					} else {
						bounds.0
					}
				}
			},
		}
	}
}

impl TextAppearance {
	pub(super) fn uses_bounds(&self) -> bool {
		[
			self.top_edge,
			self.bottom_edge,
			self.background_top_edge,
			self.background_bottom_edge,
		]
		.into_iter()
		.any(TextEdge::uses_bounds)
	}
}

impl TextShaper {
	/// Bounds are cached in em units, independent of the requested size.
	pub(super) fn glyph_bounds(
		&mut self,
		font: &FontData,
		coords: &Arc<[i16]>,
		id: u16,
	) -> Option<(f32, f32)> {
		let key = (font.data.id(), font.index, coords.clone(), id);
		if let Some(bounds) = self.bounds.get(&key) {
			return *bounds;
		}
		let bounds = skrifa::FontRef::from_index(font.data.data(), font.index)
			.ok()
			.and_then(|font| {
				let location: Vec<_> = coords
					.iter()
					.map(|&v| NormalizedCoord::from_bits(v))
					.collect();
				let upem = f32::from(
					font.metrics(Size::unscaled(), &location[..]).units_per_em,
				);
				let bounds = font
					.glyph_metrics(Size::unscaled(), &location[..])
					.bounds(GlyphId::new(u32::from(id)))?;
				(bounds.y_max > bounds.y_min)
					.then_some((bounds.y_min / upem, bounds.y_max / upem))
			});
		if self.bounds.len() < 4096 {
			self.bounds.insert(key, bounds);
		}
		bounds
	}

	/// A blank line keeps the configured font's edges without inventing metrics.
	pub(crate) fn empty_metrics(&mut self, size: f32) -> (f32, f32) {
		let clusters = self.shape(" ", &[], size, false);
		clusters.first().map_or((0., 0.), |c| (c.ascent, c.descent))
	}
}
