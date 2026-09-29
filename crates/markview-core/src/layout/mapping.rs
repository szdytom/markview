use crate::{math::MathBox, shaping::Span};
use std::{
	collections::{BTreeMap, BTreeSet},
	ops::Range,
	sync::Arc,
};
pub(super) struct Prepared {
	pub(super) images: BTreeMap<usize, crate::image::ImageSpec>,
	pub(super) reading: String,
	pub(super) search_ranges: Vec<(Range<usize>, Range<usize>)>,
	pub(super) mapping: Vec<(Range<usize>, Range<usize>, bool)>,
	/// Reading text to the document bytes it was set from, one segment per
	/// inline, in reading order.
	pub(super) source: Vec<(Range<usize>, Range<usize>)>,
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

	/// The document bytes a reading range was set from, where that is known.
	///
	/// Reading text is what the reader shows and copies, so it is not a slice
	/// of the source. Every run the range touches contributes: the first gives
	/// where it starts, the last where it ends. A run whose reading and source
	/// lengths agree maps offset for offset; a run that does not is a single
	/// decoded unit, such as an entity reference or an escape, and is taken
	/// whole because its interior boundaries are not recoverable.
	pub(super) fn source_range(
		&self,
		reading: &Range<usize>,
	) -> Option<Range<usize>> {
		runs_source_option(&self.source, reading)
	}
}

/// Where a reading offset enters a run.
fn map_start(run: &(Range<usize>, Range<usize>), at: usize) -> usize {
	let (r, s) = run;
	if r.len() == s.len() {
		s.start + at.saturating_sub(r.start)
	} else {
		s.start
	}
}

/// Where a reading offset leaves a run.
fn map_end(run: &(Range<usize>, Range<usize>), at: usize) -> usize {
	let (r, s) = run;
	if r.len() == s.len() {
		s.start + at.min(r.end).saturating_sub(r.start)
	} else {
		s.end
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

/// The source a reading range came from, given the runs it was set from.
///
/// Every run the range touches contributes: the first gives where it starts,
/// the last where it ends. A run whose reading and source lengths agree maps
/// offset for offset; a run that does not is a single decoded unit and is
/// taken whole. An empty run set means the range maps onto `fallback`.
pub(super) fn runs_source(
	runs: &[(Range<usize>, Range<usize>)],
	reading: &Range<usize>,
	fallback: &Range<usize>,
) -> Range<usize> {
	runs_source_option(runs, reading).unwrap_or_else(|| fallback.clone())
}

/// The shared mapping: every run the range touches contributes, the first
/// giving where it starts and the last where it ends. A run whose reading and
/// source lengths agree maps offset for offset; one that does not is a single
/// decoded unit, such as an entity reference, and is taken whole.
fn runs_source_option(
	runs: &[(Range<usize>, Range<usize>)],
	reading: &Range<usize>,
) -> Option<Range<usize>> {
	let mut first = None;
	let mut last = None;
	for (index, (r, _)) in runs.iter().enumerate() {
		if r.start < reading.end && reading.start < r.end {
			first.get_or_insert(index);
			last = Some(index);
		}
	}
	let (Some(first), Some(last)) = (first, last) else {
		// An empty range belongs to whichever run holds its start.
		let (_, s) = runs.iter().find(|(r, _)| r.contains(&reading.start))?;
		return Some(s.clone());
	};
	let start = map_start(&runs[first], reading.start);
	let end = map_end(&runs[last], reading.end);
	Some(start..end.max(start))
}
