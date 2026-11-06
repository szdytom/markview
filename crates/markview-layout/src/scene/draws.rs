//! Packed retained drawing geometry with stable expanded command positions.
use super::{Draw, GeometryArray, Glyph, GlyphStyle, GlyphStyles, Paint, Rect};
use std::{collections::HashMap, ops::Range, sync::Arc};

/// Index into the style table owned by one packed drawing buffer.
#[derive(Clone, Copy, Debug)]
pub struct GlyphStyleId(u32);

/// Glyph-local geometry; every other property belongs to the run's style.
#[derive(Clone, Copy, Debug)]
pub struct GlyphPosition {
	pub id: u16,
	pub x: f32,
	pub y: f32,
}
static EMPTY: Draw = Draw::Rect(
	Rect {
		x: 0.,
		y: 0.,
		w: 0.,
		h: 0.,
	},
	Paint::Text,
);

#[derive(Clone, Debug)]
enum Command {
	Empty,
	Glyphs {
		style: GlyphStyleId,
		glyphs: Range<u32>,
	},
	Other(u32),
}
#[derive(Clone, Debug)]
struct Run {
	start: u32,
	command: Command,
}
#[derive(Debug)]
pub struct Packed {
	runs: Box<[Run]>,
	glyphs: Box<[GlyphPosition]>,
	styles: Box<[Arc<GlyphStyle>]>,
	others: Box<[Draw]>,
	len: usize,
}
#[derive(Clone, Debug)]
pub struct DrawPiece {
	start: usize,
	buffer: DrawBuffer,
}
/// Drawing indices always address the expanded paint sequence, including
/// positions inside a glyph run. Packing never changes interaction bindings.
#[derive(Clone, Debug)]
pub enum DrawBuffer {
	#[doc(hidden)]
	Building(GeometryArray<Draw>),
	#[doc(hidden)]
	Packed(Arc<Packed>),
	#[doc(hidden)]
	Pieces(GeometryArray<DrawPiece>),
}
impl Default for DrawBuffer {
	fn default() -> Self {
		Self::Building(Default::default())
	}
}
impl From<Vec<Draw>> for DrawBuffer {
	fn from(draws: Vec<Draw>) -> Self {
		Self::Building(draws.into())
	}
}
impl FromIterator<Draw> for DrawBuffer {
	fn from_iter<I: IntoIterator<Item = Draw>>(iter: I) -> Self {
		Vec::from_iter(iter).into()
	}
}
impl DrawBuffer {
	pub fn len(&self) -> usize {
		match self {
			Self::Building(v) => v.len(),
			Self::Packed(p) => p.len,
			Self::Pieces(p) => p.last().map_or(0, |p| p.start + p.buffer.len()),
		}
	}
	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}
	/// Logical element capacity. Use `storage_bytes` for packed allocation sizes.
	pub fn capacity(&self) -> usize {
		match self {
			Self::Building(v) => v.capacity(),
			Self::Packed(p) => p.len,
			Self::Pieces(p) => p.last().map_or(0, |p| p.start + p.buffer.len()),
		}
	}
	/// Shared allocation identity, not an array of expanded `Draw` values.
	pub fn as_ptr(&self) -> *const () {
		match self {
			Self::Building(v) => v.storage_id(),
			Self::Packed(p) => Arc::as_ptr(p).cast(),
			Self::Pieces(p) => p.storage_id(),
		}
	}
	pub fn iter(&self) -> DrawIter<'_> {
		DrawIter::new(self, 0..self.len())
	}
	pub fn iter_range(&self, range: Range<usize>) -> Option<DrawIter<'_>> {
		(range.start <= range.end && range.end <= self.len())
			.then_some(DrawIter::new(self, range))
	}
	pub fn get(&self, index: usize) -> Option<Draw> {
		match self {
			Self::Building(v) => v.get(index).cloned(),
			Self::Pieces(p) => {
				let p = p.get(
					p.partition_point(|p| p.start <= index).checked_sub(1)?,
				)?;
				p.buffer.get(index - p.start)
			}
			Self::Packed(p) => {
				if index >= p.len {
					return None;
				}
				let run = &p.runs
					[p.runs.partition_point(|r| r.start as usize <= index) - 1];
				Some(expand(p, run, index))
			}
		}
	}
	pub fn at(&self, index: usize) -> Draw {
		self.get(index).expect("drawing index in bounds")
	}
	/// Borrow an ungrouped drawing without cloning its image or clipping payload.
	pub fn other(&self, index: usize) -> Option<&Draw> {
		match self {
			Self::Building(v) => v.get(index),
			Self::Pieces(p) => {
				let p = p.get(
					p.partition_point(|p| p.start <= index).checked_sub(1)?,
				)?;
				p.buffer.other(index - p.start)
			}
			Self::Packed(p) => {
				if index >= p.len {
					return None;
				}
				let run = &p.runs
					[p.runs.partition_point(|r| r.start as usize <= index) - 1];
				match &run.command {
					Command::Other(i) => Some(&p.others[*i as usize]),
					Command::Empty => Some(&EMPTY),
					Command::Glyphs { .. } => None,
				}
			}
		}
	}
	pub fn builder_mut(&mut self) -> &mut Vec<Draw> {
		if !matches!(self, Self::Building(_)) {
			*self = Self::from(self.iter().collect::<Vec<_>>());
		}
		let Self::Building(v) = self else {
			unreachable!()
		};
		v
	}
	pub fn get_mut(&mut self, index: usize) -> Option<&mut Draw> {
		if let Self::Pieces(p) = self {
			let i = p.partition_point(|p| p.start <= index).checked_sub(1)?;
			let piece = &mut p[i];
			piece.buffer.get_mut(index - piece.start)
		} else {
			self.builder_mut().get_mut(index)
		}
	}
	pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, Draw> {
		self.builder_mut().iter_mut()
	}
	pub fn reserve(&mut self, additional: usize) {
		self.tail_mut().reserve(additional);
	}
	pub fn push(&mut self, draw: Draw) {
		self.tail_mut().push(draw);
	}
	pub fn extend(&mut self, draws: impl IntoIterator<Item = Draw>) {
		self.tail_mut().extend(draws);
	}
	fn tail_mut(&mut self) -> &mut Vec<Draw> {
		if let Self::Pieces(p) = self {
			if p.last()
				.is_none_or(|p| !matches!(p.buffer, Self::Building(_)))
			{
				let start = p.last().map_or(0, |p| p.start + p.buffer.len());
				p.push(DrawPiece {
					start,
					buffer: Self::default(),
				});
			}
			p.last_mut().unwrap().buffer.builder_mut()
		} else {
			self.builder_mut()
		}
	}
	/// Append independently owned geometry without expanding its glyphs.
	pub(crate) fn append_shared(&mut self, buffer: &Self) {
		if buffer.is_empty() {
			return;
		}
		let start = self.len();
		if !matches!(self, Self::Pieces(_)) {
			let old = std::mem::take(self);
			*self = Self::Pieces(
				vec![DrawPiece {
					start: 0,
					buffer: old,
				}]
				.into(),
			);
		}
		let Self::Pieces(p) = self else {
			unreachable!()
		};
		if let Self::Pieces(children) = buffer {
			p.extend(children.iter().map(|c| DrawPiece {
				start: start + c.start,
				buffer: c.buffer.clone(),
			}));
		} else {
			p.push(DrawPiece {
				start,
				buffer: buffer.clone(),
			});
		}
	}
	/// Return a complete leaf allocation; partial ranges are copied to avoid
	/// retaining an unrelated parent container through a small slice.
	pub(crate) fn independent_range(&self, range: Range<usize>) -> Self {
		if range.start == 0 && range.end == self.len() {
			match self {
                Self::Packed(_)=>return self.clone(),
                Self::Building(v) if u32::try_from(v.len()).is_err() || (v.capacity()==v.len() && v.iter().all(|d| !matches!(d,Draw::Glyph(_)) && !matches!(d,Draw::Rect(r,Paint::Text) if [r.x,r.y,r.w,r.h].map(f32::to_bits)==[0;4])))=>return self.clone(),
                _=>{}
            }
		}
		if let Self::Pieces(p) = self
			&& let Some(piece) = p
				.partition_point(|p| p.start <= range.start)
				.checked_sub(1)
				.and_then(|i| p.get(i))
			&& piece.start == range.start
			&& piece.start + piece.buffer.len() == range.end
		{
			return piece.buffer.independent_range(0..piece.buffer.len());
		}
		if let Self::Building(v) = self {
			Self::from_paint_iter(v[range].iter())
		} else {
			Self::from_paint_iter(self.iter_range(range).unwrap())
		}
	}

	pub(crate) fn seal_spans(
		&mut self,
		ranges: impl Iterator<Item = Range<usize>>,
	) {
		let pieces: Vec<_> = ranges
			.filter(|r| !r.is_empty())
			.map(|r| {
				let start = r.start;
				let buffer = self.independent_range(r);
				DrawPiece { start, buffer }
			})
			.collect();
		if pieces.len() == 1
			&& pieces[0].start == 0
			&& pieces[0].buffer.len() == self.len()
		{
			*self = pieces.into_iter().next().unwrap().buffer;
		} else if !pieces.is_empty() {
			*self = Self::Pieces(pieces.into());
		} else {
			self.seal();
		}
	}
	pub fn shrink_to_fit(&mut self) {
		if let Self::Building(v) = self {
			v.shrink_to_fit();
		}
	}
	/// Keep the native-width builder when an unrestricted producer exceeds the
	/// compact representation's limits.
	pub fn seal(&mut self) {
		if let Self::Pieces(p) = self {
			for p in p {
				p.buffer.seal();
			}
			return;
		}
		if let Self::Building(draws) = self
			&& u32::try_from(draws.len()).is_ok()
		{
			*self = Self::from_paint_iter(draws.iter());
		}
	}
	pub(crate) fn from_paint_iter<T: std::borrow::Borrow<Draw>>(
		draws: impl ExactSizeIterator<Item = T>,
	) -> Self {
		let len = draws.len();
		if u32::try_from(len).is_err() {
			return draws.map(|d| d.borrow().clone()).collect();
		}
		let mut runs: Vec<Run> = Vec::new();
		let mut glyphs = Vec::new();
		let mut others = Vec::new();
		let mut styles = Vec::new();
		let mut ids = HashMap::new();
		let mut shared = GlyphStyles::default();
		let mut last: Option<(Arc<GlyphStyle>, GlyphStyleId)> = None;
		for (start, item) in draws.enumerate() {
			match item.borrow() {
				Draw::Rect(r, Paint::Text)
					if [r.x, r.y, r.w, r.h].map(f32::to_bits) == [0; 4] =>
				{
					if !matches!(
						runs.last().map(|r| &r.command),
						Some(Command::Empty)
					) {
						runs.push(Run {
							start: u32::try_from(start).unwrap(),
							command: Command::Empty,
						});
					}
				}
				Draw::Glyph(g) => {
					let id = if let Some((style, id)) = &last
						&& Arc::ptr_eq(style, &g.style)
					{
						*id
					} else {
						let style = shared.share(&g.style);
						let next = styles.len();
						let id = *ids
							.entry(Arc::as_ptr(&style))
							.or_insert_with(|| {
								styles.push(style.clone());
								GlyphStyleId(u32::try_from(next).expect(
									"styles cannot outnumber checked drawing count",
								))
							});
						last = Some((g.style.clone(), id));
						id
					};
					let index = u32::try_from(glyphs.len()).expect(
						"glyphs cannot outnumber checked drawing count",
					);
					glyphs.push(GlyphPosition {
						id: g.id,
						x: g.x,
						y: g.y,
					});
					if let Some(Run {
						command: Command::Glyphs { style, glyphs },
						..
					}) = runs.last_mut()
						&& style.0 == id.0
					{
						glyphs.end = index + 1;
					} else {
						runs.push(Run {
							start: u32::try_from(start).unwrap(),
							command: Command::Glyphs {
								style: id,
								glyphs: index..index + 1,
							},
						});
					}
				}
				d => {
					let index = u32::try_from(others.len()).expect(
						"other drawings cannot outnumber checked drawing count",
					);
					others.push(d.clone());
					runs.push(Run {
						start: u32::try_from(start).unwrap(),
						command: Command::Other(index),
					});
				}
			}
		}
		if glyphs.is_empty() && runs.len() == others.len() {
			others.shrink_to_fit();
			return Self::from(others);
		}
		Self::Packed(Arc::new(Packed {
			runs: runs.into_boxed_slice(),
			glyphs: glyphs.into_boxed_slice(),
			styles: styles.into_boxed_slice(),
			others: others.into_boxed_slice(),
			len,
		}))
	}
	pub(super) fn heap_bytes(&self) -> usize {
		self.storage_bytes()
			+ match self {
				Self::Building(v) => {
					v.iter().map(Draw::heap_bytes).sum::<usize>()
						+ super::memory::style_bytes(v, &mut Default::default())
				}
				Self::Pieces(p) => {
					p.iter().map(|p| p.buffer.heap_bytes()).sum::<usize>()
						+ p.capacity() * size_of::<DrawPiece>()
				}
				Self::Packed(p) => {
					p.styles
						.iter()
						.map(|s| std::mem::size_of_val(&*s.coords))
						.sum::<usize>() + p
						.others
						.iter()
						.map(Draw::heap_bytes)
						.sum::<usize>() + super::memory::style_bytes(
						&p.others,
						&mut Default::default(),
					)
				}
			}
	}

	pub fn storage_bytes(&self) -> usize {
		match self {
			Self::Building(v) => {
				v.capacity() * size_of::<Draw>() + size_of::<Vec<Draw>>() + 16
			}
			Self::Pieces(p) => {
				p.iter().map(|p| p.buffer.storage_bytes()).sum::<usize>()
					+ p.capacity() * size_of::<DrawPiece>()
					+ size_of::<Vec<DrawPiece>>()
					+ 16
			}
			Self::Packed(p) => {
				size_of::<Packed>()
					+ 16 + std::mem::size_of_val(&*p.runs)
					+ std::mem::size_of_val(&*p.glyphs)
					+ std::mem::size_of_val(&*p.styles)
					+ p.styles.len() * (size_of::<GlyphStyle>() + 16)
					+ std::mem::size_of_val(&*p.others)
			}
		}
	}
	pub fn packed_counts(&self) -> Option<(usize, usize, usize)> {
		match self {
			Self::Packed(p) => {
				Some((p.runs.len(), p.glyphs.len(), p.styles.len()))
			}
			Self::Pieces(p) => {
				Some(p.iter().fold((0, 0, 0), |(r, g, s), p| {
					let (pr, pg, ps) = p.buffer.packed_counts().unwrap_or((
						p.buffer.len(),
						0,
						0,
					));
					(r + pr, g + pg, s + ps)
				}))
			}
			_ => None,
		}
	}
}
pub struct DrawIter<'a> {
	buffer: &'a DrawBuffer,
	range: Range<usize>,
	front_run: usize,
	back_run: usize,
}
impl<'a> DrawIter<'a> {
	fn new(buffer: &'a DrawBuffer, range: Range<usize>) -> Self {
		let (front_run, back_run) = match buffer {
			DrawBuffer::Building(_) | DrawBuffer::Pieces(_) => (0, 0),
			DrawBuffer::Packed(p) => (
				p.runs
					.partition_point(|r| r.start as usize <= range.start)
					.saturating_sub(1),
				p.runs
					.partition_point(|r| (r.start as usize) < range.end)
					.saturating_sub(1),
			),
		};
		Self {
			buffer,
			range,
			front_run,
			back_run,
		}
	}
}
fn expand(p: &Packed, run: &Run, index: usize) -> Draw {
	match &run.command {
		Command::Other(i) => p.others[*i as usize].clone(),
		Command::Empty => EMPTY.clone(),
		Command::Glyphs { style, glyphs } => {
			let g =
				p.glyphs[glyphs.start as usize + index - run.start as usize];
			Draw::Glyph(Glyph {
				style: p.styles[style.0 as usize].clone(),
				id: g.id,
				x: g.x,
				y: g.y,
			})
		}
	}
}
impl Iterator for DrawIter<'_> {
	type Item = Draw;
	fn next(&mut self) -> Option<Draw> {
		let index = self.range.next()?;
		Some(match self.buffer {
			DrawBuffer::Building(v) => v[index].clone(),
			DrawBuffer::Pieces(_) => self.buffer.at(index),
			DrawBuffer::Packed(p) => {
				while self.front_run + 1 < p.runs.len()
					&& p.runs[self.front_run + 1].start as usize <= index
				{
					self.front_run += 1;
				}
				expand(p, &p.runs[self.front_run], index)
			}
		})
	}
	fn size_hint(&self) -> (usize, Option<usize>) {
		self.range.size_hint()
	}
}
impl DoubleEndedIterator for DrawIter<'_> {
	fn next_back(&mut self) -> Option<Draw> {
		let index = self.range.next_back()?;
		Some(match self.buffer {
			DrawBuffer::Building(v) => v[index].clone(),
			DrawBuffer::Pieces(_) => self.buffer.at(index),
			DrawBuffer::Packed(p) => {
				while p.runs[self.back_run].start as usize > index {
					self.back_run -= 1;
				}
				expand(p, &p.runs[self.back_run], index)
			}
		})
	}
}
impl ExactSizeIterator for DrawIter<'_> {}
impl<'a> IntoIterator for &'a DrawBuffer {
	type Item = Draw;
	type IntoIter = DrawIter<'a>;
	fn into_iter(self) -> Self::IntoIter {
		self.iter()
	}
}
impl<'a> IntoIterator for &'a mut DrawBuffer {
	type Item = &'a mut Draw;
	type IntoIter = std::slice::IterMut<'a, Draw>;
	fn into_iter(self) -> Self::IntoIter {
		self.iter_mut()
	}
}
pub struct OwnedDrawIter {
	buffer: DrawBuffer,
	range: Range<usize>,
}
impl Iterator for OwnedDrawIter {
	type Item = Draw;
	fn next(&mut self) -> Option<Draw> {
		self.range.next().map(|i| self.buffer.at(i))
	}
}
impl IntoIterator for DrawBuffer {
	type Item = Draw;
	type IntoIter = OwnedDrawIter;
	fn into_iter(self) -> Self::IntoIter {
		let range = 0..self.len();
		OwnedDrawIter {
			buffer: self,
			range,
		}
	}
}

#[cfg(feature = "fuzz")]
impl DrawBuffer {
	pub(super) fn visit_storage(
		&self,
		visit: &mut super::memory::StorageVisitor<'_>,
	) {
		use super::memory::allocation;
		match self {
			Self::Building(v) => v.visit_storage("draw_commands", visit),
			Self::Pieces(p) => {
				p.visit_storage("draw_pieces", visit);
				for p in p {
					p.buffer.visit_storage(visit);
				}
			}
			Self::Packed(p) => {
				visit(
					Arc::as_ptr(p) as usize,
					"draw_runs",
					size_of::<Packed>() + 2 * size_of::<usize>(),
					0,
					0,
				);
				allocation(&p.runs, p.runs.len(), "draw_runs", visit);
				allocation(&p.glyphs, p.glyphs.len(), "glyph_positions", visit);
				allocation(
					&p.styles,
					p.styles.len(),
					"glyph_style_handles",
					visit,
				);
				allocation(&p.others, p.others.len(), "draw_commands", visit);
				for style in &p.styles {
					visit(
						Arc::as_ptr(style) as usize,
						"glyph_styles",
						size_of::<GlyphStyle>() + 2 * size_of::<usize>(),
						1,
						1,
					);
					visit(
						style.coords.as_ptr() as usize,
						"glyph_coordinates",
						size_of_val(&*style.coords) + 2 * size_of::<usize>(),
						style.coords.len(),
						style.coords.len(),
					);
				}
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	fn glyph(id: u16, x: f32) -> Draw {
		Draw::Glyph(Glyph {
			style: Arc::new(GlyphStyle {
				font: {
					static FONT: std::sync::OnceLock<parley::FontData> =
						std::sync::OnceLock::new();
					FONT.get_or_init(|| parley::FontData {
						data: parley::fontique::Blob::new(Arc::new(
							include_bytes!(
								"../../tests/fonts/NotoSerif-Regular-subset.otf"
							)
							.as_slice(),
						)),
						index: 0,
					})
					.clone()
				},
				coords: Arc::from([]),
				size: 18.,
				synthetic_italic: false,
				synthetic_bold: false,
				paint: super::super::Paint::Text,
			}),
			id,
			x,
			y: -0.,
		})
	}
	#[test]
	fn packing_preserves_order_and_partial_run_bindings() {
		let mut buffer: DrawBuffer = vec![
			glyph(10, 0.),
			glyph(11, 1.),
			glyph(12, 2.),
			Draw::Rect(Default::default(), super::super::Paint::Accent),
			glyph(13, 3.),
			glyph(14, 4.),
		]
		.into();
		buffer.seal();
		assert_eq!(buffer.packed_counts(), Some((3, 5, 1)));
		let mut middle = buffer.iter_range(1..5).unwrap();
		let Draw::Glyph(left) = middle.next().unwrap() else {
			panic!("glyph")
		};
		let Draw::Glyph(right) = middle.next_back().unwrap() else {
			panic!("glyph")
		};
		assert_eq!(
			(left.id, left.x.to_bits(), left.y.to_bits()),
			(11, 1f32.to_bits(), (-0f32).to_bits())
		);
		assert_eq!(right.id, 13);
		assert!(matches!(middle.next(),Some(Draw::Glyph(g)) if g.id==12));
		assert!(matches!(
			middle.next_back(),
			Some(Draw::Rect(_, super::super::Paint::Accent))
		));
		assert!(middle.next().is_none());
		assert!(buffer.iter_range(0..7).is_none());
		assert!(buffer.get(6).is_none());
	}
	#[test]
	fn packed_snapshots_share_and_release_geometry() {
		let mut buffer: DrawBuffer =
			(0..2000).map(|i| glyph(10, i as f32)).collect();
		buffer.seal();
		assert_eq!(buffer.packed_counts(), Some((1, 2000, 1)));
		assert!(buffer.storage_bytes() < 2000 * size_of::<Draw>() / 2);
		let shared = buffer.clone();
		let weak = match &shared {
			DrawBuffer::Packed(p) => Arc::downgrade(p),
			_ => unreachable!(),
		};
		assert_eq!(buffer.as_ptr(), shared.as_ptr());
		buffer.get_mut(0).unwrap().translate(7., 0.);
		assert!(
			matches!(shared.at(0),Draw::Glyph(g) if g.x.to_bits()==0f32.to_bits())
		);
		assert!(matches!(buffer.at(0),Draw::Glyph(g) if g.x==7.));
		drop(shared);
		assert!(weak.upgrade().is_none());
	}
}
