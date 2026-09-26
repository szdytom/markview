//! Logical reading text and its final layout geometry. No clipboard or input APIs.
use crate::layout::{LayoutSnapshot, Rect};
use icu_segmenter::{WordSegmenter, WordSegmenterBorrowed};
use std::{ops::Range, sync::OnceLock};
use unicode_segmentation::UnicodeSegmentation;

/// Word boundaries for pointer gestures and text counts. ICU supplies the
/// UAX #29 rules plus the Chinese and Japanese dictionaries, so 中文文字 breaks
/// into 中文 / 文字 instead of one segment per character. The segmenter is
/// immutable and cheap to copy, so one process-wide instance serves every thread.
pub(crate) fn word_segmenter() -> WordSegmenterBorrowed<'static> {
	static SEGMENTER: OnceLock<WordSegmenterBorrowed<'static>> =
		OnceLock::new();
	*SEGMENTER.get_or_init(|| WordSegmenter::new_auto(Default::default()))
}

/// Whether a segment reads as a word: it carries letters or digits. ICU's own
/// `is_word_like` reports false for a segment that ends in a combining mark
/// after a base letter, such as `Cafe\u{301}`, so classify by content instead.
fn is_word(segment: &str) -> bool {
	segment.chars().any(char::is_alphanumeric)
}

/// The word-like range a click lands on. A click on punctuation or an emoji
/// selects that cluster; a click on whitespace selects the nearest word, with
/// the word before the gap winning a tie.
fn word_range(text: &str, clicked: Range<usize>) -> Option<Range<usize>> {
	let mut start = 0;
	let mut containing = None;
	let mut containing_word = None;
	let mut preceding = None;
	let mut following = None;
	for end in word_segmenter().segment_str(text) {
		let range = start..end;
		let word = is_word(&text[range.clone()]);
		if range.start <= clicked.start && clicked.end <= range.end {
			if word {
				containing_word = Some(range);
				break;
			}
			containing = Some(range);
		} else if word {
			if range.end <= clicked.start {
				preceding = Some(range);
			} else if range.start >= clicked.end {
				following = Some(range);
				break;
			}
		}
		start = end;
	}
	if let Some(range) = containing_word {
		return Some(range);
	}
	if let Some(range) = &containing
		&& !text[range.clone()].contains(char::is_whitespace)
	{
		return containing;
	}
	match (preceding, following) {
		(Some(before), Some(after)) => {
			Some(if clicked.start - before.end <= after.start - clicked.end {
				before
			} else {
				after
			})
		}
		(Some(before), None) => Some(before),
		(None, Some(after)) => Some(after),
		(None, None) => containing,
	}
}

/// Counts the same reading text that is copied, including whitespace. Words use
/// the same dictionary segmentation as double-click, so Chinese and Japanese
/// count by word instead of by character.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextCounts {
	pub chars: usize,
	pub words: usize,
}
impl TextCounts {
	pub fn of(text: &str) -> Self {
		let mut words = 0;
		let mut start = 0;
		for end in word_segmenter().segment_str(text) {
			words += usize::from(is_word(&text[start..end]));
			start = end;
		}
		Self {
			chars: text.graphemes(true).count(),
			words,
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Affinity {
	Before,
	After,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextPosition {
	pub revision: u64,
	pub block: usize,
	pub node: usize,
	pub offset: usize,
	pub affinity: Affinity,
}
impl TextPosition {
	fn key(self) -> (usize, usize, usize) {
		(self.block, self.node, self.offset)
	}
	/// Reading order of two positions, ignoring revision and affinity. Repeated
	/// blocks and nodes order by their occurrence.
	pub fn cmp_reading(self, other: Self) -> std::cmp::Ordering {
		self.key().cmp(&other.key())
	}
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextSelection {
	pub anchor: TextPosition,
	pub focus: TextPosition,
}
impl TextSelection {
	pub fn ordered(self) -> (TextPosition, TextPosition) {
		if self.anchor.key() <= self.focus.key() {
			(self.anchor, self.focus)
		} else {
			(self.focus, self.anchor)
		}
	}
	pub fn is_empty(self) -> bool {
		self.anchor.key() == self.focus.key()
	}
}
#[derive(Clone, Debug)]
pub struct TextNode {
	pub search_field: Option<crate::search::SearchField>,
	/// Pairs of semantic and reading byte ranges; diagnostics have no pair.
	pub search_ranges: Vec<(Range<usize>, Range<usize>)>,
	pub text: String,
	pub separator: &'static str,
	pub clusters: Vec<TextCluster>,
	boundaries: Vec<usize>,
}
impl TextNode {
	pub fn new(text: String, separator: &'static str) -> Self {
		let mut boundaries: Vec<_> =
			text.grapheme_indices(true).map(|(i, _)| i).collect();
		boundaries.push(text.len());
		Self {
			search_field: None,
			search_ranges: Vec::new(),
			text,
			separator,
			clusters: Vec::new(),
			boundaries,
		}
	}
	pub fn push(&mut self, mut cluster: TextCluster) {
		// A shaping cluster may begin inside a grapheme; never expose that boundary.
		cluster.range.start = self.grapheme_floor(cluster.range.start);
		cluster.range.end = self.grapheme_ceil(cluster.range.end);
		self.clusters.push(cluster);
	}
	/// Greatest grapheme boundary at or before `offset`.
	fn grapheme_floor(&self, offset: usize) -> usize {
		self.boundaries[self
			.boundaries
			.partition_point(|&i| i <= offset)
			.saturating_sub(1)]
	}
	/// Least grapheme boundary at or after `offset`.
	fn grapheme_ceil(&self, offset: usize) -> usize {
		self.boundaries[self
			.boundaries
			.partition_point(|&i| i < offset)
			.min(self.boundaries.len() - 1)]
	}
}
#[derive(Clone, Debug)]
pub struct TextCluster {
	pub range: Range<usize>,
	pub rect: Rect,
	pub rtl: bool,
	/// Draw index binds geometry to the same overflow viewport as painted text.
	pub command: usize,
}

mod geometry;
mod selection;
impl LayoutSnapshot {
	pub fn extract_text(
		&self,
		selection: TextSelection,
		revision: u64,
	) -> String {
		if selection.anchor.revision != revision
			|| selection.focus.revision != revision
		{
			return String::new();
		}
		let (a, b) = selection.ordered();
		let mut result = String::new();
		for (bi, block) in self.blocks.iter().enumerate() {
			for (ni, node) in block.layout.text.iter().enumerate() {
				if node.text.is_empty() {
					continue;
				}
				if (bi, ni) < (a.block, a.node) || (bi, ni) > (b.block, b.node)
				{
					continue;
				}
				let start = if (bi, ni) == (a.block, a.node) {
					a.offset
				} else {
					0
				};
				let end = if (bi, ni) == (b.block, b.node) {
					b.offset
				} else {
					node.text.len()
				};
				if let Some(part) = node.text.get(start..end) {
					if (bi, ni) != (a.block, a.node) {
						result.push_str(node.separator);
					}
					result.push_str(part);
				}
			}
		}
		result.replace('\u{ad}', "")
	}
}

/// The differing interval, on grapheme boundaries, after trimming a shared
/// prefix and suffix. Also maps elided placeholder text back to its full value.
pub(crate) fn changed_span(old: &str, new: &str) -> (usize, usize, usize) {
	let prefix = old
		.graphemes(true)
		.zip(new.graphemes(true))
		.take_while(|(a, b)| a == b)
		.map(|(a, _)| a.len())
		.sum::<usize>();
	let suffix = old[prefix..]
		.graphemes(true)
		.rev()
		.zip(new[prefix..].graphemes(true).rev())
		.take_while(|(a, b)| a == b)
		.map(|(a, _)| a.len())
		.sum::<usize>();
	(prefix, old.len() - suffix, new.len() - suffix)
}

#[cfg(test)]
mod tests;
