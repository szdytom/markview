//! Exact glyph coordinates with shared vertical positions.
use super::GlyphPosition;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug)]
pub(super) struct CompactGlyph {
	x: f32,
	id: u16,
	y: u16,
}

#[derive(Debug)]
pub(super) enum Positions {
	Native(Box<[GlyphPosition]>),
	Shared {
		glyphs: Box<[CompactGlyph]>,
		y: Box<[f32]>,
	},
}
impl Positions {
	pub(super) fn len(&self) -> usize {
		match self {
			Self::Native(glyphs) => glyphs.len(),
			Self::Shared { glyphs, .. } => glyphs.len(),
		}
	}
	pub(super) fn get(&self, index: usize) -> GlyphPosition {
		match self {
			Self::Native(glyphs) => glyphs[index],
			Self::Shared { glyphs, y } => {
				let g = glyphs[index];
				GlyphPosition {
					id: g.id,
					x: g.x,
					y: y[usize::from(g.y)],
				}
			}
		}
	}
	pub(super) fn storage_bytes(&self) -> usize {
		match self {
			Self::Native(glyphs) => size_of_val(&**glyphs),
			Self::Shared { glyphs, y } => {
				size_of_val(&**glyphs) + size_of_val(&**y)
			}
		}
	}
	#[cfg(feature = "fuzz")]
	pub(super) fn visit_storage(
		&self,
		visit: &mut crate::scene::StorageVisitor<'_>,
	) {
		use crate::scene::memory::allocation;
		match self {
			Self::Native(glyphs) => {
				allocation(glyphs, glyphs.len(), "glyph_positions", visit);
			}
			Self::Shared { glyphs, y } => {
				allocation(glyphs, glyphs.len(), "glyph_positions", visit);
				allocation(y, y.len(), "glyph_baselines", visit);
			}
		}
	}
}

#[derive(Default)]
pub(super) struct Builder {
	glyphs: Vec<CompactGlyph>,
	y: Vec<f32>,
	ids: HashMap<u32, u16>,
	last: Option<(u32, u16)>,
	native: Option<Vec<GlyphPosition>>,
}
impl Builder {
	pub(super) fn len(&self) -> usize {
		self.native.as_ref().map_or(self.glyphs.len(), Vec::len)
	}
	pub(super) fn is_empty(&self) -> bool {
		self.len() == 0
	}
	pub(super) fn push(&mut self, glyph: GlyphPosition) {
		if let Some(native) = &mut self.native {
			native.push(glyph);
			return;
		}
		let bits = glyph.y.to_bits();
		let index = if let Some((previous, index)) = self.last
			&& previous == bits
		{
			index
		} else if let Some(index) = self.ids.get(&bits) {
			*index
		} else if let Ok(index) = u16::try_from(self.y.len()) {
			self.y.push(glyph.y);
			self.ids.insert(bits, index);
			index
		} else {
			let mut native = self.expand();
			native.push(glyph);
			*self = Self {
				native: Some(native),
				..Default::default()
			};
			return;
		};
		self.last = Some((bits, index));
		self.glyphs.push(CompactGlyph {
			x: glyph.x,
			id: glyph.id,
			y: index,
		});
	}
	fn expand(&self) -> Vec<GlyphPosition> {
		self.glyphs
			.iter()
			.map(|g| GlyphPosition {
				id: g.id,
				x: g.x,
				y: self.y[usize::from(g.y)],
			})
			.collect()
	}
	pub(super) fn finish(self) -> Positions {
		if let Some(native) = self.native {
			return Positions::Native(native.into_boxed_slice());
		}
		// A tiny buffer or mostly distinct coordinates costs less without a table.
		if self.glyphs.len() * size_of::<CompactGlyph>()
			+ self.y.len() * size_of::<f32>()
			+ size_of::<Box<[f32]>>()
			>= self.glyphs.len() * size_of::<GlyphPosition>()
		{
			return Positions::Native(self.expand().into_boxed_slice());
		}
		Positions::Shared {
			glyphs: self.glyphs.into_boxed_slice(),
			y: self.y.into_boxed_slice(),
		}
	}
}

#[cfg(test)]
mod tests;
