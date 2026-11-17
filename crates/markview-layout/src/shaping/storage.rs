//! A shaping result owns one glyph array and shares styles over adjacent glyphs.
use super::Cluster;
use crate::scene::{Glyph, GlyphPosition, GlyphStyle};
use std::{
	ops::{Deref, DerefMut, Range},
	sync::Arc,
};

#[derive(Default)]
pub(crate) struct GlyphStorage {
	positions: Vec<GlyphPosition>,
	runs: Vec<(Range<usize>, Arc<GlyphStyle>)>,
}
impl GlyphStorage {
	pub(super) fn len(&self) -> usize {
		self.positions.len()
	}
	pub(super) fn extend(
		&mut self,
		style: Arc<GlyphStyle>,
		glyphs: impl Iterator<Item = GlyphPosition>,
	) {
		let start = self.positions.len();
		self.positions.extend(glyphs);
		let end = self.positions.len();
		if start == end {
			return;
		}
		if let Some((range, previous)) = self.runs.last_mut()
			&& Arc::ptr_eq(previous, &style)
		{
			range.end = end;
		} else {
			self.runs.push((start..end, style));
		}
	}
	pub(crate) fn shift(&mut self, range: Range<usize>, amount: f32) {
		for glyph in &mut self.positions[range] {
			glyph.x += amount;
		}
	}
	pub(crate) fn iter(
		&self,
		range: Range<usize>,
	) -> impl Iterator<Item = Glyph> + '_ {
		let mut run = self.runs.partition_point(|(r, _)| r.end <= range.start);
		let start = range.start;
		self.positions[range].iter().enumerate().map(move |(i, g)| {
			while self.runs[run].0.end <= start + i {
				run += 1;
			}
			Glyph {
				style: self.runs[run].1.clone(),
				id: g.id,
				x: g.x,
				y: g.y,
			}
		})
	}
}

#[derive(Default)]
pub(crate) struct ShapedText {
	pub(crate) clusters: Vec<Cluster>,
	pub(crate) glyphs: GlyphStorage,
}
impl Deref for ShapedText {
	type Target = [Cluster];
	fn deref(&self) -> &[Cluster] {
		&self.clusters
	}
}
impl DerefMut for ShapedText {
	fn deref_mut(&mut self) -> &mut [Cluster] {
		&mut self.clusters
	}
}
impl<'a> IntoIterator for &'a ShapedText {
	type Item = &'a Cluster;
	type IntoIter = std::slice::Iter<'a, Cluster>;
	fn into_iter(self) -> Self::IntoIter {
		self.clusters.iter()
	}
}
impl<'a> IntoIterator for &'a mut ShapedText {
	type Item = &'a mut Cluster;
	type IntoIter = std::slice::IterMut<'a, Cluster>;
	fn into_iter(self) -> Self::IntoIter {
		self.clusters.iter_mut()
	}
}

#[cfg(test)]
mod tests;
