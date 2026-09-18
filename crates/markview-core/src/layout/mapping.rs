use crate::{math::MathBox, shaping::Span};
use std::{
	collections::{BTreeMap, BTreeSet},
	ops::Range,
	sync::Arc,
};
pub(super) struct Prepared {
	pub(super) images: BTreeMap<usize, crate::image::ImageSpec>,
	pub(super) reading: String,
	pub(super) mapping: Vec<(Range<usize>, Range<usize>, bool)>,
	pub(super) text: String,
	pub(super) spans: Vec<Span>,
	/// The inline code chip padding for each span, in logical pixels, in the
	/// canonical top, right, bottom, left order.
	pub(super) padding: Vec<[f32; 4]>,
	pub(super) math: BTreeMap<usize, Arc<MathBox>>,
	/// Footnote references by the offset of their first digit, each with the
	/// offset just past its last one, so a link covers every digit.
	pub(super) notes: BTreeMap<usize, (u32, usize)>,
	/// The text offsets of the forced breaks that asked to be justified.
	pub(super) breaks: BTreeSet<usize>,
}

impl Prepared {
	/// The reference whose digits cover `offset`, and whether `offset` is their
	/// first one. Only the first digit registers the return anchor; every digit
	/// stays part of the link.
	pub(super) fn note_at(&self, offset: usize) -> Option<(u32, bool)> {
		let (&start, &(number, end)) =
			self.notes.range(..=offset).next_back()?;
		(offset < end).then_some((number, offset == start))
	}

	pub(super) fn reading_range(&self, range: Range<usize>) -> Range<usize> {
		let Some((visual, logical, atomic)) = self
			.mapping
			.iter()
			.find(|(v, _, _)| v.contains(&range.start))
		else {
			return self.reading.len()..self.reading.len();
		};
		if *atomic {
			return logical.clone();
		}
		let start = logical.start + range.start - visual.start;
		let end = self
			.mapping
			.iter()
			.find(|(v, _, _)| v.start < range.end && v.end >= range.end)
			.map(|(v, l, atomic)| {
				if *atomic {
					l.end
				} else {
					l.start + range.end - v.start
				}
			})
			.unwrap_or(self.reading.len());
		start..end
	}
}
pub(super) fn expand_tabs_mapped(
	text: &str,
	size: usize,
) -> (String, Vec<usize>) {
	let mut out = String::new();
	let mut offsets = vec![0];
	let mut column = 0;
	for (i, ch) in text.char_indices() {
		if ch == '\t' {
			let count = size - column % size;
			for n in 0..count {
				out.push(' ');
				offsets.push(if n + 1 == count { i + 1 } else { i });
			}
			column += count;
		} else {
			out.push(ch);
			for n in 1..=ch.len_utf8() {
				offsets.push(i + n);
			}
			column += 1;
		}
	}
	(out, offsets)
}
