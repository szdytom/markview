//! Logical reading text and its final layout geometry. No clipboard or input APIs.
use crate::layout::{LayoutSnapshot, Rect};
use icu_segmenter::{WordSegmenter, WordSegmenterBorrowed};
pub use markview_document::Affinity;
use std::{
	ops::Range,
	sync::{Arc, OnceLock},
};
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
pub(crate) struct GeometryPosition {
	pub revision: u64,
	pub block: usize,
	pub node: usize,
	pub offset: usize,
	pub affinity: Affinity,
}
impl GeometryPosition {
	fn key(self) -> (usize, usize, usize) {
		(self.block, self.node, self.offset)
	}

	/// The semantic field position this layout position names.
	pub fn semantic(
		self,
		snapshot: &LayoutSnapshot,
	) -> Option<SemanticPosition> {
		if self.revision != snapshot.revision.get() {
			return None;
		}
		let block = snapshot.blocks.get(self.block)?;
		let node = block.layout.text.get(self.node)?;
		if !node.text.is_char_boundary(self.offset) {
			return None;
		}
		let field = field_for_node(&block.layout.text, self.node)?;
		let offset =
			reading_to_semantic(node, self.offset).unwrap_or_else(|| {
				block
					.layout
					.text
					.iter()
					.filter(|n| n.search_field == Some(field))
					.flat_map(|n| &n.search_ranges)
					.map(|(s, _)| s.end)
					.max()
					.unwrap_or(0)
			});
		let mut position = SemanticPosition {
			revision: markview_document::DocumentRevision::new(self.revision),
			block: block.id,
			field,
			offset,
			affinity: self.affinity,
			projection: None,
		};
		if text_position(snapshot, position)
			.is_none_or(|p| p.node != self.node || p.offset != self.offset)
		{
			position.projection = Some(
				block
					.layout
					.text
					.iter()
					.enumerate()
					.take(self.node)
					.filter(|(i, _)| {
						field_for_node(&block.layout.text, *i) == Some(field)
					})
					.map(|(_, n)| n.text.len())
					.sum::<usize>() + self.offset,
			);
		}
		Some(position)
	}
	/// Reading order of two positions, ignoring revision and affinity. Repeated
	/// blocks and nodes order by their occurrence.
	pub fn cmp_reading(self, other: Self) -> std::cmp::Ordering {
		self.key().cmp(&other.key())
	}
}
pub use markview_document::SemanticPosition;

/// The layout position of a semantic field offset.
pub(crate) fn text_position(
	snapshot: &LayoutSnapshot,
	position: SemanticPosition,
) -> Option<GeometryPosition> {
	if position.revision != snapshot.revision {
		return None;
	}
	let (bi, block) = snapshot
		.blocks
		.iter()
		.enumerate()
		.find(|(_, block)| block.id == position.block)?;
	if let Some(mut offset) = position.projection {
		let mut boundary = None;
		for (ni, node) in block.layout.text.iter().enumerate() {
			if field_for_node(&block.layout.text, ni) != Some(position.field) {
				continue;
			}
			if offset <= node.text.len() {
				if !node.text.is_char_boundary(offset) {
					return None;
				}
				let resolved = GeometryPosition {
					revision: position.revision.get(),
					block: bi,
					node: ni,
					offset,
					affinity: position.affinity,
				};
				if offset < node.text.len()
					|| position.affinity == Affinity::After
				{
					return Some(resolved);
				}
				boundary = Some(resolved);
			}
			offset = offset.saturating_sub(node.text.len());
		}
		return boundary;
	}
	for (ni, node) in block.layout.text.iter().enumerate() {
		if node.search_field != Some(position.field) {
			continue;
		}
		let Some(offset) = semantic_to_reading(node, position.offset) else {
			continue;
		};
		return Some(GeometryPosition {
			revision: position.revision.get(),
			block: bi,
			node: ni,
			offset,
			affinity: position.affinity,
		});
	}
	None
}

/// Maps a field offset to the reading offset of one layout node.
pub(crate) fn semantic_to_reading(
	node: &TextNode,
	offset: usize,
) -> Option<usize> {
	let index = node
		.search_ranges
		.partition_point(|(semantic, _)| semantic.end < offset);
	for (semantic, reading) in node.search_ranges.iter().skip(index) {
		if offset < semantic.start {
			break;
		}
		if offset > semantic.end {
			continue;
		}
		if semantic.len() == reading.len() {
			return Some(reading.start + offset - semantic.start);
		}
		return Some(if offset == semantic.start {
			reading.start
		} else {
			reading.end
		});
	}
	None
}

/// Maps a reading offset of one layout node to its field offset.
pub(crate) fn reading_to_semantic(
	node: &TextNode,
	offset: usize,
) -> Option<usize> {
	let index = node
		.search_ranges
		.partition_point(|(_, reading)| reading.end < offset);
	for (semantic, reading) in node.search_ranges.iter().skip(index) {
		if offset < reading.start {
			break;
		}
		if offset > reading.end {
			continue;
		}
		if semantic.len() == reading.len() {
			return Some(semantic.start + offset - reading.start);
		}
		return Some(if offset == reading.start {
			semantic.start
		} else {
			semantic.end
		});
	}
	None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GeometrySelection {
	pub anchor: GeometryPosition,
	pub focus: GeometryPosition,
}
impl GeometrySelection {
	pub fn ordered(self) -> (GeometryPosition, GeometryPosition) {
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
	pub search_field: Option<markview_document::FieldId>,
	/// Pairs of semantic and reading byte ranges; diagnostics have no pair.
	pub search_ranges:
		crate::scene::GeometryArray<(Range<usize>, Range<usize>)>,
	/// Image geometry bound by inline ordinal, independent of reading text.
	pub(crate) source_images: Vec<(usize, TextCluster)>,
	pub text: Arc<String>,
	pub separator: &'static str,
	pub clusters: ClusterBuffer,
	/// First node of the paragraph or cell; `None` denotes a standalone node.
	pub(crate) selection_group: Option<usize>,
	boundaries: GraphemeBoundaries,
}
impl TextNode {
	/// Reading metadata for representation-independent harness records.
	#[cfg(feature = "fuzz")]
	pub fn reading_metadata(
		&self,
	) -> (&[(usize, TextCluster)], Option<usize>, Vec<usize>) {
		(
			&self.source_images,
			self.selection_group,
			(0..self.boundaries.len())
				.map(|i| self.boundaries.at(i))
				.collect(),
		)
	}
	pub(crate) fn heap_bytes(&self) -> usize {
		self.text.capacity()
			+ size_of::<String>()
			+ 2 * size_of::<usize>()
			+ self.clusters.storage_bytes()
			+ self.search_ranges.heap_bytes()
			+ self.source_images.capacity()
				* std::mem::size_of::<(usize, TextCluster)>()
			+ self.boundaries.heap_bytes()
	}
	pub(crate) fn seal(&mut self) {
		self.source_images.shrink_to_fit();
		self.clusters.seal();
		if self.search_ranges.windows(2).any(|p| {
			let (a, b) = (&p[0], &p[1]);
			a.0.start < a.0.end
				&& b.0.start < b.0.end
				&& a.0.len() == a.1.len()
				&& b.0.len() == b.1.len()
				&& a.0.end == b.0.start
				&& a.1.end == b.1.start
		}) {
			self.search_ranges.dedup_by(
				|(next_s, next_r), (prev_s, prev_r)| {
					if prev_s.start < prev_s.end
						&& next_s.start < next_s.end
						&& prev_s.len() == prev_r.len()
						&& next_s.len() == next_r.len()
						&& prev_s.end == next_s.start
						&& prev_r.end == next_r.start
					{
						prev_s.end = next_s.end;
						prev_r.end = next_r.end;
						true
					} else {
						false
					}
				},
			);
		}
		if self.search_ranges.capacity() != self.search_ranges.len() {
			self.search_ranges.shrink_to_fit();
		}
	}
	pub fn new(mut text: String, separator: &'static str) -> Self {
		text.shrink_to_fit();
		let boundaries = GraphemeBoundaries::new(&text);
		Self {
			search_field: None,
			search_ranges: Default::default(),
			source_images: Vec::new(),
			text: Arc::new(text),
			separator,
			clusters: ClusterBuffer::default(),
			selection_group: None,
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
		self.boundaries.at(self
			.boundaries
			.partition_point(|i| i <= offset)
			.saturating_sub(1))
	}
	/// Least grapheme boundary at or after `offset`.
	fn grapheme_ceil(&self, offset: usize) -> usize {
		self.boundaries.at(self
			.boundaries
			.partition_point(|i| i < offset)
			.min(self.boundaries.len() - 1))
	}
	/// The offsets a cluster may be split at, its own start first and its end
	/// last.
	///
	/// A cluster is a unit of shaping, not of reading. A ligature sets two or
	/// three letters as a single glyph, and picking one letter out of it has
	/// to be possible; a base letter and its combining mark are one grapheme,
	/// and parting them would put a caret inside a character. Only the
	/// grapheme boundaries tell the two apart, and an atomic box — a formula
	/// or a drawn image — has no letters to pick apart at all.
	///
	/// The boundaries are sorted, so the cluster's own slice is found with
	/// `partition_point`; scanning the whole node here would make painting a
	/// long paragraph quadratic in its text length.
	fn splits<'a>(
		&'a self,
		cluster: &'a TextCluster,
	) -> impl Iterator<Item = usize> + 'a {
		let start =
			self.boundaries.partition_point(|b| b < cluster.range.start);
		let end = self.boundaries.partition_point(|b| b <= cluster.range.end);
		let inside = (start + 1..end - 1)
			.filter(move |_| !cluster.atomic)
			.map(move |i| self.boundaries.at(i));
		std::iter::once(cluster.range.start)
			.chain(inside)
			.chain(std::iter::once(cluster.range.end))
	}
	/// How many graphemes a cluster covers, and so how many parts it has.
	fn parts(&self, cluster: &TextCluster) -> usize {
		self.splits(cluster).count() - 1
	}
	/// The offset `part` parts into a cluster, where zero is its start and
	/// [`Self::parts`] is its end.
	fn split_at(&self, cluster: &TextCluster, part: usize) -> usize {
		self.splits(cluster).nth(part).unwrap_or(cluster.range.end)
	}
	/// How many parts of a cluster end at or before `offset`.
	fn part_at(&self, cluster: &TextCluster, offset: usize) -> usize {
		self.splits(cluster).take_while(|&b| b <= offset).count() - 1
	}
	/// The parts a selection covers of a cluster, empty when it takes none.
	///
	/// `at` names the cluster's node; `from` and `to` are the ordered ends of
	/// the selection as `(block, node, offset)`. A cluster the selection never
	/// reaches is taken by none of it, which is most of the document; one the
	/// selection only reaches into is taken up to where it stops, and an
	/// atomic formula or image is taken whole as soon as it is reached.
	fn covered_parts(
		&self,
		cluster: &TextCluster,
		at: (usize, usize),
		from: (usize, usize, usize),
		to: (usize, usize, usize),
	) -> Range<usize> {
		let here = |offset| (at.0, at.1, offset);
		if here(cluster.range.end) <= from || here(cluster.range.start) >= to {
			return 0..0;
		}
		// An atomic box is all or nothing: once the selection reaches it, it
		// is marked whole. Its source text is not what is displayed, so a
		// selection made against a placeholder must not lose its highlight
		// when a partial offset pair falls between the box's two parts.
		if cluster.atomic {
			return 0..self.parts(cluster);
		}
		let start = if at == (from.0, from.1) {
			from.2.max(cluster.range.start)
		} else {
			cluster.range.start
		};
		let end = if at == (to.0, to.1) {
			to.2.min(cluster.range.end)
		} else {
			cluster.range.end
		};
		if end <= start {
			return 0..0;
		}
		self.part_at(cluster, start)..self.part_at(cluster, end)
	}
	/// The part of a cluster's own rect `parts` covers, measured in graphemes.
	/// The letters of a ligature share its advance evenly: one glyph carries
	/// no per-letter advance to divide it by. The result stays in cluster
	/// coordinates; the caller scrolls and clips it.
	fn part_rect(&self, cluster: &TextCluster, parts: Range<usize>) -> Rect {
		let total = self.parts(cluster) as f32;
		let (mut left, mut right) =
			(parts.start as f32 / total, parts.end as f32 / total);
		if cluster.rtl {
			(left, right) = (1.0 - right, 1.0 - left);
		}
		Rect {
			x: cluster.rect.x + cluster.rect.w * left,
			w: cluster.rect.w * (right - left),
			..cluster.rect
		}
	}
}
#[derive(Clone, Debug)]
pub struct TextCluster {
	/// Automatic CJK/Latin spacing on the visual left and right edges.
	pub mixed_spacing: (f32, f32),
	pub range: Range<usize>,
	pub rect: Rect,
	pub rtl: bool,
	/// A formula or a drawn image is one box that reads as one unit, so it is
	/// never split between its graphemes the way a ligature is.
	pub atomic: bool,
	/// Draw index binds geometry to the same overflow viewport as painted text.
	pub command: usize,
}

mod clusters;
pub use clusters::ClusterBuffer;
mod geometry;
mod selection;
impl LayoutSnapshot {
	/// Complete retained reading text, including collapsed disclosure bodies.
	pub fn full_reading_text(&self) -> String {
		let mut result = String::new();
		for node in self.blocks.iter().flat_map(|b| &b.layout.text) {
			if node.text.is_empty() {
				continue;
			}
			if !result.is_empty() {
				result.push_str(node.separator);
			}
			result.push_str(&node.text);
		}
		result.replace('\u{ad}', "")
	}
	pub(crate) fn geometry_extract_text(
		&self,
		selection: GeometrySelection,
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
				if node.text.is_empty() || !block.text_visible(ni) {
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

#[cfg(all(test, feature = "font-directories"))]
mod tests;

mod semantic;
pub use semantic::{ResolvedPosition, TextSelection};

/// Generated labels belong to the following semantic field, or the preceding
/// field at the end of a block. Their reading offsets remain field-relative.
fn field_for_node(
	nodes: &[TextNode],
	index: usize,
) -> Option<markview_document::FieldId> {
	nodes[index..]
		.iter()
		.find_map(|n| n.search_field)
		.or_else(|| nodes[..index].iter().rev().find_map(|n| n.search_field))
}

/// Large producer-supplied fields retain native offsets without truncation.
#[derive(Clone, Debug)]
enum GraphemeBoundaries {
	Compact(Arc<[u32]>),
	Wide(Arc<[usize]>),
}
impl GraphemeBoundaries {
	fn new(text: &str) -> Self {
		let offsets = text
			.grapheme_indices(true)
			.map(|(i, _)| i)
			.chain(std::iter::once(text.len()));
		if u32::try_from(text.len()).is_ok() {
			Self::Compact(
				offsets
					.map(|i| {
						u32::try_from(i)
							.expect("boundary is within checked text length")
					})
					.collect(),
			)
		} else {
			Self::Wide(offsets.collect())
		}
	}
	fn len(&self) -> usize {
		match self {
			Self::Compact(v) => v.len(),
			Self::Wide(v) => v.len(),
		}
	}
	fn at(&self, i: usize) -> usize {
		match self {
			Self::Compact(v) => v[i] as usize,
			Self::Wide(v) => v[i],
		}
	}
	fn partition_point(&self, mut p: impl FnMut(usize) -> bool) -> usize {
		match self {
			Self::Compact(v) => v.partition_point(|&i| p(i as usize)),
			Self::Wide(v) => v.partition_point(|&i| p(i)),
		}
	}
	fn heap_bytes(&self) -> usize {
		match self {
			Self::Compact(v) => {
				std::mem::size_of_val(&**v) + 2 * size_of::<usize>()
			}
			Self::Wide(v) => {
				std::mem::size_of_val(&**v) + 2 * size_of::<usize>()
			}
		}
	}
}

#[cfg(feature = "fuzz")]
impl TextNode {
	pub fn visit_storage(&self, visit: &mut crate::scene::StorageVisitor<'_>) {
		use crate::scene::memory::allocation;
		visit(
			Arc::as_ptr(&self.text) as usize,
			"reading_text",
			size_of::<String>() + 2 * size_of::<usize>(),
			0,
			0,
		);
		allocation(
			self.text.as_bytes(),
			self.text.capacity(),
			"reading_text",
			visit,
		);
		self.clusters.visit_storage(visit);
		self.search_ranges.visit_storage("search_mappings", visit);
		allocation(
			&self.source_images,
			self.source_images.capacity(),
			"image_bindings",
			visit,
		);
		let (ptr, bytes, len) = match &self.boundaries {
			GraphemeBoundaries::Compact(v) => {
				(v.as_ptr() as usize, size_of_val(&**v), v.len())
			}
			GraphemeBoundaries::Wide(v) => {
				(v.as_ptr() as usize, size_of_val(&**v), v.len())
			}
		};
		visit(
			ptr,
			"grapheme_boundaries",
			bytes + 2 * size_of::<usize>(),
			len,
			len,
		);
	}
}
