//! Source navigation over semantic fields and cached rendering geometry.
//!
//! The source text and its mapping belong to the front end; this adapter
//! combines them with layout geometry only when a source-navigation query is
//! actually made.
use crate::{
	document::{
		BlockId, BlockKind, Document, FieldId, InlineKind, SourceMap,
		visit_fields,
	},
	scene::{LayoutSnapshot, Rect},
};
use std::{
	collections::{BTreeMap, HashMap},
	ops::Range,
	sync::Arc,
};

/// A rendered source unit, in UTF-8 bytes and document logical pixels.
#[derive(Clone, Debug)]
pub struct SourcePosition {
	pub source: Range<usize>,
	pub rect: Rect,
}

/// A visible source-line extent in UTF-16 units and document logical pixels.
#[derive(Clone, Debug)]
pub struct SourceScrollAnchor {
	pub source: Range<usize>,
	pub top: f32,
	pub bottom: f32,
}

struct SourceLine {
	bytes: Range<usize>,
	utf16: Range<usize>,
}

struct FieldSpans {
	text: Vec<(Range<usize>, Range<usize>)>,
	images: Vec<(usize, Range<usize>)>,
}

/// Source bindings belong to a parsed document, independent of cached geometry.
pub struct SourceIndex {
	source: Arc<str>,
	revision: markview_document::DocumentRevision,
	block_ids: HashMap<BlockId, usize>,
	blocks: Vec<Option<Range<usize>>>,
	fields: Vec<Vec<FieldSpans>>,
	relocated_footnotes: Vec<bool>,
	lines: Vec<SourceLine>,
}

impl SourceIndex {
	pub fn new(source: Arc<str>, document: &Document, map: &SourceMap) -> Self {
		let ids: Vec<BlockId> =
			document.blocks().iter().map(|block| block.id()).collect();
		let block_ids = ids
			.iter()
			.enumerate()
			.map(|(index, id)| (*id, index))
			.collect();
		let blocks = ids
			.iter()
			.map(|id| map.block(*id).and_then(|b| b.source.clone()))
			.collect::<Vec<_>>();
		let fields = document
			.blocks()
			.iter()
			.map(|block| {
				let mut fields = Vec::new();
				visit_fields(
					block,
					&mut Vec::new(),
					&|| false,
					&mut |_, field: FieldId, text, _| {
						let entry = map.block(block.id()).and_then(|entry| {
							entry
								.fields
								.iter()
								.find(|entry| entry.field == field)
						});
						let spans = entry
							.map(|entry| entry.spans.as_slice())
							.unwrap_or(&[]);
						let mapped: Vec<_> = spans
							.iter()
							.map(|span| {
								(span.semantic.clone(), span.source.clone())
							})
							.collect();
						let covers = entry
							.map(|entry| entry.covers.as_slice())
							.unwrap_or(&[]);
						let images = match text {
							crate::document::FieldText::Rich(rich) => rich
								.iter()
								.enumerate()
								.filter(|(_, inline)| {
									matches!(inline.kind, InlineKind::Image(_))
								})
								.filter_map(|(index, _)| {
									covers.get(index).map(|cover| {
										(index, cover.source.clone())
									})
								})
								.collect(),
							crate::document::FieldText::Code(_) => Vec::new(),
						};
						fields.push(FieldSpans {
							text: mapped,
							images,
						});
					},
				);
				fields
			})
			.collect();
		let relocated_footnotes = document
			.blocks()
			.iter()
			.enumerate()
			.scan(0, |end, (index, block)| {
				let Some(range) = &blocks[index] else {
					return Some(false);
				};
				let relocated =
					matches!(block.kind(), BlockKind::Footnote { .. })
						&& range.start < *end;
				*end = (*end).max(range.end);
				Some(relocated)
			})
			.collect();
		let lines = {
			let mut lines = Vec::new();
			let mut byte = 0;
			let mut utf16 = 0;
			while byte < source.len() {
				let rest = &source[byte..];
				let end = rest.find(['\r', '\n']).unwrap_or(rest.len());
				let units = rest[..end].encode_utf16().count();
				lines.push(SourceLine {
					bytes: byte..byte + end,
					utf16: utf16..utf16 + units,
				});
				let newline = if rest[end..].starts_with("\r\n") {
					2
				} else {
					usize::from(end < rest.len())
				};
				byte += end + newline;
				utf16 += units + newline;
			}
			lines
		};
		Self {
			source,
			revision: document.revision(),
			block_ids,
			blocks,
			fields,
			relocated_footnotes,
			lines,
		}
	}

	/// Batches anchors for newly published blocks, excluding relocated footnotes.
	pub fn scroll_anchors(
		&self,
		snapshot: &LayoutSnapshot,
		horizontal: &HashMap<(usize, usize), f32>,
		from_block: usize,
	) -> Vec<SourceScrollAnchor> {
		let Some(first_block) = self.snapshot_block(snapshot, from_block)
		else {
			return Vec::new();
		};
		let mut lines = BTreeMap::<usize, SourceScrollAnchor>::new();
		let mut seen = vec![false; snapshot.blocks.len() - from_block];
		let mut collect = |position: SourcePosition| {
			if position.source.is_empty() || self.lines.is_empty() {
				return;
			}
			let first = self.lines.partition_point(|line| {
				line.bytes.start <= position.source.start
			}) - 1;
			let last = if position.source.end <= self.lines[first].bytes.end {
				first
			} else {
				self.lines.partition_point(|line| {
					line.bytes.start < position.source.end
				}) - 1
			};
			let bottom = position.rect.y + position.rect.h;
			let anchor =
				lines.entry(first).or_insert_with(|| SourceScrollAnchor {
					source: self.lines[first].utf16.start
						..self.lines[last].utf16.end,
					top: position.rect.y,
					bottom,
				});
			anchor.source.end =
				anchor.source.end.max(self.lines[last].utf16.end);
			anchor.top = anchor.top.min(position.rect.y);
			anchor.bottom = anchor.bottom.max(bottom);
		};
		self.visit(
			snapshot,
			horizontal,
			first_block.y..snapshot.height,
			|id, position| {
				let Some(block) = self.block_ids.get(&id).copied() else {
					return;
				};
				if block >= from_block && !self.relocated_footnotes[block] {
					seen[block - from_block] = true;
					collect(position);
				}
			},
		);
		// Text-free blocks anchor their opening line, excluding collapsed bodies.
		for (block, placed) in
			snapshot.blocks.iter().enumerate().skip(from_block)
		{
			if !seen[block - from_block] && !self.relocated_footnotes[block] {
				let Some(source) = self.blocks.get(block).cloned().flatten()
				else {
					continue;
				};
				if self.lines.is_empty() {
					continue;
				}
				let line = self
					.lines
					.partition_point(|line| line.bytes.start <= source.start)
					- 1;
				collect(SourcePosition {
					source: source.start
						..source.end.min(self.lines[line].bytes.end),
					rect: Rect {
						x: 0.,
						y: placed.y,
						w: placed.layout.width,
						h: placed.height(),
					},
				});
			}
		}
		let mut anchors: Vec<SourceScrollAnchor> = Vec::new();
		for anchor in lines.into_values() {
			if let Some(last) = anchors.last_mut()
				&& anchor.source.start <= last.source.end
			{
				last.source.end = last.source.end.max(anchor.source.end);
				last.top = last.top.min(anchor.top);
				last.bottom = last.bottom.max(anchor.bottom);
			} else {
				anchors.push(anchor);
			}
		}
		anchors
	}

	/// Unpublished targets return `None`; hidden content uses its visible container.
	pub fn source_to_preview(
		&self,
		snapshot: &LayoutSnapshot,
		horizontal: &HashMap<(usize, usize), f32>,
		offset: usize,
	) -> Option<SourcePosition> {
		self.current(snapshot)?;
		let block = self
			.blocks
			.iter()
			.enumerate()
			.filter(|(i, range)| {
				range.is_some()
					|| self.fields[*i].iter().any(|field| {
						!field.text.is_empty() || !field.images.is_empty()
					})
			})
			.min_by_key(|(i, range)| {
				self.fields[*i]
					.iter()
					.flat_map(|field| {
						field.text.iter().map(|(_, source)| source).chain(
							field.images.iter().map(|(_, source)| source),
						)
					})
					.map(|source| distance(source, offset))
					.min()
					.unwrap_or_else(|| {
						range
							.as_ref()
							.map_or(usize::MAX, |range| distance(range, offset))
					})
			})?
			.0;
		let placed = snapshot.blocks.get(block)?;
		let mut best: Option<(usize, SourcePosition)> = None;
		self.visit(
			snapshot,
			horizontal,
			placed.y..placed.y + placed.height(),
			|id, position| {
				if self.block_ids.get(&id).copied() != Some(block) {
					return;
				}
				let d = distance(&position.source, offset);
				if best.as_ref().is_none_or(|(old, p)| {
					d < *old
						|| (d == *old && position.source.start > p.source.start)
				}) {
					best = Some((d, position));
				}
			},
		);
		best.map(|(_, p)| p).or_else(|| {
			Some(SourcePosition {
				source: self.blocks.get(block)?.clone()?,
				rect: Rect {
					x: 0.,
					y: placed.y,
					w: placed.layout.width,
					h: placed.height(),
				},
			})
		})
	}

	/// Chooses the nearest visible line, then its leftmost source unit.
	pub fn preview_to_source(
		&self,
		snapshot: &LayoutSnapshot,
		horizontal: &HashMap<(usize, usize), f32>,
		y: f32,
	) -> Option<SourcePosition> {
		self.current(snapshot)?;
		let placed =
			snapshot.blocks.iter().min_by(|a, b| {
				vertical_distance(a.y, a.height(), y)
					.total_cmp(&vertical_distance(b.y, b.height(), y))
			})?;
		let mut best: Option<SourcePosition> = None;
		self.visit(
			snapshot,
			horizontal,
			placed.y..placed.y + placed.height(),
			|_, position| {
				if best.as_ref().is_none_or(|old| {
					vertical_distance(position.rect.y, position.rect.h, y)
						.total_cmp(&vertical_distance(
							old.rect.y, old.rect.h, y,
						))
						.then_with(|| {
							position.rect.y.total_cmp(&old.rect.y).reverse()
						})
						.then_with(|| position.rect.x.total_cmp(&old.rect.x))
						.is_lt()
				}) {
					best = Some(position);
				}
			},
		);
		best.or_else(|| {
			Some(SourcePosition {
				source: self.block_source(placed.id)?,
				rect: Rect {
					x: 0.,
					y: placed.y,
					w: placed.layout.width,
					h: placed.height(),
				},
			})
		})
	}

	fn current(&self, snapshot: &LayoutSnapshot) -> Option<()> {
		(snapshot.revision == self.revision).then_some(())
	}

	fn snapshot_block<'a>(
		&self,
		snapshot: &'a LayoutSnapshot,
		index: usize,
	) -> Option<&'a crate::scene::PlacedBlock> {
		self.current(snapshot)?;
		snapshot.blocks.get(index)
	}

	fn visit(
		&self,
		snapshot: &LayoutSnapshot,
		horizontal: &HashMap<(usize, usize), f32>,
		visible: Range<f32>,
		mut visit: impl FnMut(BlockId, SourcePosition),
	) {
		snapshot.visit_search_clusters(
			horizontal,
			visible.clone(),
			|id, field, semantic, rect| {
				let Some(block) = self.block_ids.get(&id).copied() else {
					return;
				};
				let Some(spans) = self
					.fields
					.get(block)
					.and_then(|fields| fields.get(field.get() as usize))
				else {
					return;
				};
				let spans = &spans.text;
				let first =
					spans.partition_point(|(r, _)| r.end <= semantic.start);
				for (reading, source) in spans
					.iter()
					.skip(first)
					.take_while(|(r, _)| r.start < semantic.end)
				{
					let source = if reading.len() == source.len() {
						source.start + semantic.start.max(reading.start)
							- reading.start
							..source.start + semantic.end.min(reading.end)
								- reading.start
					} else {
						source.clone()
					};
					visit(id, SourcePosition { source, rect });
				}
			},
		);
		let first = snapshot
			.blocks
			.partition_point(|b| b.y + b.height() < visible.start);
		for (bi, block) in snapshot.blocks.iter().enumerate().skip(first) {
			if block.y > visible.end {
				break;
			}
			for (ni, node) in block.layout.text.iter().enumerate() {
				if !block.text_visible(ni) {
					continue;
				}
				let Some(field) = node.search_field else {
					continue;
				};
				let Some(block_index) = self.block_ids.get(&block.id).copied()
				else {
					continue;
				};
				let images =
					&self.fields[block_index][field.get() as usize].images;
				for (index, cluster) in &node.source_images {
					let Ok(found) =
						images.binary_search_by_key(index, |(i, _)| *i)
					else {
						continue;
					};
					let source = &images[found].1;
					let Some(rect) =
						snapshot.text_rect(bi, cluster, horizontal)
					else {
						continue;
					};
					if rect.y + rect.h >= visible.start && rect.y <= visible.end
					{
						visit(
							block.id,
							SourcePosition {
								source: source.clone(),
								rect,
							},
						);
					}
				}
			}
		}
	}

	/// The source at the start of `id`, for callers that need a block range.
	pub fn block_source(&self, id: BlockId) -> Option<Range<usize>> {
		let index = *self.block_ids.get(&id)?;
		self.blocks.get(index).cloned().flatten()
	}

	pub fn source_text(&self) -> &str {
		&self.source
	}
}

fn distance(range: &Range<usize>, offset: usize) -> usize {
	if offset < range.start {
		range.start - offset
	} else if offset >= range.end {
		offset - range.end + 1
	} else {
		0
	}
}
fn vertical_distance(top: f32, height: f32, y: f32) -> f32 {
	(top - y).max(y - top - height).max(0.)
}

/// Converts original-source UTF-8 bytes to JavaScript UTF-16 code units.
pub fn byte_to_utf16(source: &str, byte: usize) -> usize {
	let mut byte = byte.min(source.len());
	while !source.is_char_boundary(byte) {
		byte -= 1;
	}
	source[..byte].encode_utf16().count()
}
/// A position inside a surrogate pair snaps to the character's start.
pub fn utf16_to_byte(source: &str, offset: usize) -> usize {
	let mut units = 0;
	for (byte, ch) in source.char_indices() {
		units += ch.len_utf16();
		if units > offset {
			return byte;
		}
	}
	source.len()
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::document::{
		BlockKind, BlockSource, DefinitionItem, DocumentBuilder,
		DocumentRevision, FieldId, FieldSource, Inline, InlineKind, SourceSpan,
		TextStyle,
	};

	fn term(text: &str) -> Vec<Inline> {
		vec![Inline {
			link: None,
			kind: InlineKind::Text(text.to_owned()),
			style: TextStyle::default(),
		}]
	}

	#[test]
	fn omitted_field_entries_do_not_shift_later_fields() {
		let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
		let block = builder.block(
			Vec::new(),
			BlockKind::DefinitionList(vec![DefinitionItem {
				terms: vec![term("first"), term("second")],
				definitions: Vec::new(),
			}]),
		);
		let document = builder.finish(vec![block]);
		let id = document.blocks()[0].id();
		let map = SourceMap::new(
			document.revision(),
			vec![BlockSource {
				block: id,
				source: Some(0..11),
				fields: vec![FieldSource {
					field: FieldId::from_index(1),
					spans: vec![SourceSpan {
						semantic: 0..6,
						source: 5..11,
						exact: true,
					}],
					covers: Vec::new(),
				}],
			}],
		);
		let index = SourceIndex::new("firstsecond".into(), &document, &map);
		assert!(index.fields[0][0].text.is_empty());
		assert_eq!(index.fields[0][1].text, [(0..6, 5..11)]);
	}
}
