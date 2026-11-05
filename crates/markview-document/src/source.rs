//! Format-independent correspondence between semantic fields and source text.
//!
//! A [`SourceMap`] is produced by a front end (for example CommonMark) and
//! passed explicitly to the consumers that need it. It never lives inside a
//! [`crate::Document`]: a document can be built from HTML, where no original
//! text exists.
use crate::{BlockId, Document, DocumentRevision, FieldId, visit_fields};
use std::ops::Range;

/// One span of a semantic field and the source interval it came from.
///
/// `exact` distinguishes a character-accurate correspondence from an
/// approximate covering interval, such as a transformed text run.
///
/// Absence is represented by omitting the field or the block, never by an
/// empty range: a zero-width `source` range is a legitimate position at that
/// byte offset. Generated content therefore has no span at all, and a front
/// end without source for a field leaves the field out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceSpan {
	/// The byte range inside the field's [`crate::plain_text`] projection.
	pub semantic: Range<usize>,
	/// The half-open UTF-8 byte range in the original source.
	pub source: Range<usize>,
	pub exact: bool,
}

/// The source spans of every text field in one top-level block's subtree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockSource {
	pub block: BlockId,
	/// The block's own covering interval, or `None` when the front end has no
	/// source mapping for it.
	pub source: Option<Range<usize>>,
	pub fields: Vec<FieldSource>,
}

/// The source spans of one semantic field, in field order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldSource {
	pub field: FieldId,
	pub spans: Vec<SourceSpan>,
	/// One covering interval per inline, independent of character alignment.
	pub covers: Vec<SourceSpan>,
}

/// A semantic position in a specific document revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SemanticPosition {
	pub revision: DocumentRevision,
	pub block: BlockId,
	pub field: FieldId,
	pub offset: usize,
	pub affinity: crate::ids::Affinity,
	/// Offset in the field's reading projection when generated labels,
	/// placeholders or diagnostics have no exact semantic byte boundary.
	/// This counts reading text, never layout nodes or glyphs.
	pub projection: Option<usize>,
}

/// What a source query resolved to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceTarget {
	pub block: BlockId,
	pub field: FieldId,
	pub source: Range<usize>,
	pub exact: bool,
}

#[derive(Clone, Debug)]
struct Reverse {
	source: Range<usize>,
	block: usize,
	field: usize,
	span: usize,
}

/// Semantic-to-source correspondence for one parsed input.
#[derive(Clone, Debug, Default)]
pub struct SourceMap {
	revision: Option<DocumentRevision>,
	blocks: Vec<BlockSource>,
	by_block: Vec<(BlockId, usize)>,
	/// Spans ordered by their source start, for reverse lookups.
	reverse: Vec<Reverse>,
}

impl SourceMap {
	pub fn new(revision: DocumentRevision, blocks: Vec<BlockSource>) -> Self {
		let mut by_block: Vec<(BlockId, usize)> = blocks
			.iter()
			.enumerate()
			.map(|(index, block)| (block.block, index))
			.collect();
		by_block.sort_unstable_by_key(|(id, _)| *id);
		let mut reverse: Vec<Reverse> = Vec::new();
		for (block, entry) in blocks.iter().enumerate() {
			for (field, entry) in entry.fields.iter().enumerate() {
				for (span, entry) in entry.spans.iter().enumerate() {
					reverse.push(Reverse {
						source: entry.source.clone(),
						block,
						field,
						span,
					});
				}
			}
		}
		reverse.sort_unstable_by(|a, b| {
			a.source
				.start
				.cmp(&b.source.start)
				.then(a.source.end.cmp(&b.source.end))
		});
		Self {
			revision: Some(revision),
			blocks,
			by_block,
			reverse,
		}
	}

	pub fn revision(&self) -> Option<DocumentRevision> {
		self.revision
	}

	pub fn blocks(&self) -> &[BlockSource] {
		&self.blocks
	}

	pub fn block(&self, id: BlockId) -> Option<&BlockSource> {
		let index = self
			.by_block
			.binary_search_by_key(&id, |(id, _)| *id)
			.ok()?;
		self.blocks.get(self.by_block[index].1)
	}

	pub fn field(
		&self,
		block: BlockId,
		field: FieldId,
	) -> Option<&FieldSource> {
		self.block(block)?
			.fields
			.iter()
			.find(|entry| entry.field == field)
	}

	/// The block whose recorded coverage contains `offset`.
	///
	/// A missing block source is skipped; a zero-width coverage at `offset`
	/// still belongs to that block.
	pub fn source_block(&self, offset: usize) -> Option<BlockId> {
		self.blocks
			.iter()
			.filter_map(|block| {
				let range = block.source.as_ref()?;
				(range.start <= offset && offset <= range.end)
					.then_some((block.block, distance(range, offset)))
			})
			.min_by_key(|(_, distance)| *distance)
			.map(|(block, _)| block)
	}

	/// The source interval a semantic range maps onto, or `None` when the
	/// field has no mapping for any part of it.
	pub fn semantic_to_source(
		&self,
		block: BlockId,
		field: FieldId,
		semantic: Range<usize>,
	) -> Option<SourceTarget> {
		let entry = self.field(block, field)?;
		let mut best: Option<(bool, SourceTarget)> = None;
		for span in &entry.spans {
			let overlaps = if semantic.is_empty() {
				span.semantic.start <= semantic.start
					&& semantic.start <= span.semantic.end
			} else {
				span.semantic.end > semantic.start
					&& span.semantic.start < semantic.end
			};
			if !overlaps {
				continue;
			}
			let source =
				if span.exact && span.semantic.len() == span.source.len() {
					let start = semantic.start.max(span.semantic.start)
						- span.semantic.start;
					let end = semantic.end.min(span.semantic.end)
						- span.semantic.start;
					span.source.start + start..span.source.start + end
				} else {
					span.source.clone()
				};
			let target = SourceTarget {
				block,
				field,
				source,
				exact: span.exact,
			};
			let starts = span.semantic.start == semantic.start;
			if best.as_ref().is_none_or(|(old_starts, old)| {
				(starts && !old_starts)
					|| (starts == *old_starts
						&& distance(&target.source, semantic.start)
							< distance(&old.source, semantic.start))
			}) {
				best = Some((starts, target));
			}
		}
		best.map(|(_, target)| target)
	}

	/// The semantic range a source offset belongs to, or `None` when no span
	/// covers it.
	pub fn source_to_semantic(
		&self,
		offset: usize,
	) -> Option<(BlockId, FieldId, Range<usize>)> {
		let mut best: Option<&Reverse> = None;
		let start = self
			.reverse
			.partition_point(|entry| entry.source.start < offset)
			.saturating_sub(1);
		for entry in self.reverse.iter().skip(start) {
			if entry.source.start > offset {
				break;
			}
			if offset > entry.source.end {
				continue;
			}
			if best.is_none_or(|old| {
				distance(&entry.source, offset) < distance(&old.source, offset)
			}) {
				best = Some(entry);
			}
		}
		let entry = best?;
		let span =
			&self.blocks[entry.block].fields[entry.field].spans[entry.span];
		let semantic = if span.exact && span.semantic.len() == span.source.len()
		{
			let delta = offset
				.saturating_sub(span.source.start)
				.min(span.semantic.len());
			span.semantic.start + delta..span.semantic.start + delta
		} else {
			span.semantic.clone()
		};
		let field = self.blocks[entry.block].fields[entry.field].field;
		Some((self.blocks[entry.block].block, field, semantic))
	}

	/// Checks that every source range is inside `source_len` and on character
	/// boundaries, and that semantic offsets fit the document's own fields.
	///
	/// `covers` are checked like `spans`: `inline_source` reads them first, so
	/// an interval that is out of bounds or off a boundary would be handed to a
	/// caller that trusts it.
	pub fn validate(
		&self,
		source: &str,
		document: &Document,
	) -> Result<(), SourceMapError> {
		for block in &self.blocks {
			if let Some(range) = &block.source {
				check(range, source)?;
			}
			let Some(target) = document.block(block.block) else {
				return Err(SourceMapError::UnknownBlock(block.block));
			};
			let mut failure = None;
			let mut fields_seen = 0u32;
			visit_fields(
				target,
				&mut Vec::new(),
				&|| false,
				&mut |_, field, text, _| {
					fields_seen += 1;
					if failure.is_some() {
						return;
					}
					// A container's own fields follow its children, and a map need
					// not record every field, so match on the identity rather than
					// on position.
					let Some(entry) =
						block.fields.iter().find(|entry| entry.field == field)
					else {
						return;
					};
					let len = text.len();
					for span in entry.spans.iter().chain(&entry.covers) {
						if span.semantic.end > len
							|| span.semantic.start > span.semantic.end
						{
							failure = Some(SourceMapError::OutOfBounds(
								span.semantic.clone(),
							));
							return;
						}
						if let Err(error) = check(&span.source, source) {
							failure = Some(error);
							return;
						}
					}
				},
			);
			if let Some(error) = failure {
				return Err(error);
			}
			// A removed entry is a field the map chose not to record; an entry
			// whose id no field of this block ever reaches is stale or forged.
			if let Some(entry) = block
				.fields
				.iter()
				.find(|entry| entry.field.get() >= fields_seen)
			{
				return Err(SourceMapError::UnknownField(entry.field));
			}
		}
		Ok(())
	}
}

/// Why a source map cannot describe a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceMapError {
	UnknownBlock(BlockId),
	UnknownField(FieldId),
	OutOfBounds(Range<usize>),
	NotCharBoundary(usize),
}

fn check(range: &Range<usize>, source: &str) -> Result<(), SourceMapError> {
	if range.end > source.len() || range.start > range.end {
		return Err(SourceMapError::OutOfBounds(range.clone()));
	}
	if !source.is_char_boundary(range.start) {
		return Err(SourceMapError::NotCharBoundary(range.start));
	}
	if !source.is_char_boundary(range.end) {
		return Err(SourceMapError::NotCharBoundary(range.end));
	}
	Ok(())
}

fn distance(range: &Range<usize>, offset: usize) -> usize {
	if offset < range.start {
		range.start - offset
	} else if offset > range.end {
		offset.abs_diff(range.end)
	} else {
		0
	}
}

#[cfg(test)]
mod proptests;
