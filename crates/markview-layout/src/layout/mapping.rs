use crate::{math::MathBox, scene::LinkId, shaping::Span};
use std::{
	collections::{BTreeMap, BTreeSet},
	ops::Range,
	sync::Arc,
};
pub(super) struct Prepared {
	pub(super) anchors: Vec<(usize, String)>,
	pub(super) images: BTreeMap<usize, crate::image::ImageSpec>,
	pub(super) image_indices: BTreeMap<usize, usize>,
	pub(super) reading: String,
	pub(super) search_ranges: Vec<(Range<usize>, Range<usize>)>,
	pub(super) mapping: Vec<(Range<usize>, Range<usize>, bool)>,
	pub(super) text: String,
	pub(super) spans: Vec<Span>,
	/// The occurrence of each span's link, parallel to `spans`.
	pub(super) link_ids: Vec<Option<LinkId>>,
	/// Every inline link occurrence the paragraph prepares, in reading order,
	/// including runs that line breaking discards.
	pub(super) link_occurrences: Vec<usize>,
	/// The inline code chip padding for each span, in logical pixels, in the
	/// canonical top, right, bottom, left order.
	pub(super) padding: Vec<[f32; 4]>,
	pub(super) math: BTreeMap<usize, Arc<MathBox>>,
	/// Footnote references by the offset of their first digit, each with the
	/// offset just past its last one and its link occurrence, so a link covers
	/// every digit.
	pub(super) notes: BTreeMap<usize, (u32, usize, LinkId)>,
	/// The text offsets of the forced breaks that asked to be justified.
	pub(super) breaks: BTreeSet<usize>,
}

impl Prepared {
	/// The reference whose digits cover `offset`, and whether `offset` is their
	/// first one. Only the first digit registers the return anchor; every digit
	/// stays part of the link.
	pub(super) fn note_at(&self, offset: usize) -> Option<(u32, bool)> {
		let (&start, &(number, end, _)) =
			self.notes.range(..=offset).next_back()?;
		(offset < end).then_some((number, offset == start))
	}

	/// The link occurrence covering `offset`, if any. `span` is the index of
	/// the containing styled span, so a link that spans several styles still
	/// uses its occurrence's id.
	pub(super) fn link_id(
		&self,
		offset: usize,
		span: Option<usize>,
	) -> Option<LinkId> {
		if let Some(id) =
			span.and_then(|index| self.link_ids.get(index).copied().flatten())
		{
			return Some(id);
		}
		let (&_, &(_, end, id)) = self.notes.range(..=offset).next_back()?;
		(offset < end).then_some(id)
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
