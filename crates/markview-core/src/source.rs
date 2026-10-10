//! Source navigation over semantic fields and cached rendering geometry.
use crate::{
	document::{BlockKind, Document, InlineKind, plain_text},
	scene::{LayoutSnapshot, Rect},
	search::{FieldText, visit_fields},
};
use std::{
	collections::{BTreeMap, HashMap},
	ops::Range,
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
	fields: Vec<Vec<FieldSpans>>,
	blocks: Vec<Range<usize>>,
	relocated_footnotes: Vec<bool>,
	lines: Vec<SourceLine>,
}
impl SourceIndex {
	pub fn new(document: &Document) -> Self {
		let fields = document
			.blocks
			.iter()
			.map(|block| {
				let mut fields = Vec::new();
				visit_fields(
					block,
					&mut Vec::new(),
					&|| false,
					&mut |text, _| {
						let mut spans = Vec::new();
						let mut images = Vec::new();
						match text {
							FieldText::Rich(rich) => {
								let mut semantic = 0;
								for (index, inline) in rich.iter().enumerate() {
									if matches!(
										inline.kind,
										InlineKind::Image(_)
									) {
										images.push((
											index,
											inline.source.clone(),
										));
									}
									let text = plain_text(
										std::slice::from_ref(inline),
									);
									if matches!(
										inline.kind,
										InlineKind::Text(_)
									) {
										align(
											&text,
											&document.source,
											inline.source.clone(),
											semantic,
											&mut spans,
										);
									} else if !matches!(
										inline.kind,
										InlineKind::Image(_)
									) {
										spans.push((
											semantic..semantic + text.len(),
											inline.source.clone(),
										));
									}
									semantic += text.len();
								}
							}
							FieldText::Code(text, block) => {
								let mut source = block.source.clone();
								let raw = &document.source[source.clone()];
								let first = raw
									.lines()
									.next()
									.unwrap_or("")
									.trim_start();
								if first.starts_with("```")
									|| first.starts_with("~~~")
								{
									source.start += raw
										.find('\n')
										.map_or(raw.len(), |i| i + 1);
								}
								align(
									text,
									&document.source,
									source,
									0,
									&mut spans,
								);
							}
						}
						fields.push(FieldSpans {
							text: spans,
							images,
						});
					},
				);
				fields
			})
			.collect();
		Self {
			fields,
			blocks: document.blocks.iter().map(|b| b.source.clone()).collect(),
			relocated_footnotes: document
				.blocks
				.iter()
				.scan(0, |end, block| {
					let relocated =
						matches!(block.kind, BlockKind::Footnote { .. })
							&& block.source.start < *end;
					*end = (*end).max(block.source.end);
					Some(relocated)
				})
				.collect(),
			lines: {
				let mut lines = Vec::new();
				let mut byte = 0;
				let mut utf16 = 0;
				while byte < document.source.len() {
					let rest = &document.source[byte..];
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
			},
		}
	}

	/// Batches anchors for newly published blocks, excluding relocated footnotes.
	pub fn scroll_anchors(
		&self,
		snapshot: &LayoutSnapshot,
		horizontal: &HashMap<(usize, usize), f32>,
		from_block: usize,
	) -> Vec<SourceScrollAnchor> {
		let Some(first_block) = snapshot.blocks.get(from_block) else {
			return Vec::new();
		};
		let mut lines = BTreeMap::<usize, SourceScrollAnchor>::new();
		let mut seen = vec![false; snapshot.blocks.len() - from_block];
		let mut collect = |position: SourcePosition| {
			if position.source.is_empty() {
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
			|block, position| {
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
				let line = self.lines.partition_point(|line| {
					line.bytes.start <= placed.source.start
				}) - 1;
				collect(SourcePosition {
					source: placed.source.start
						..placed.source.end.min(self.lines[line].bytes.end),
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
		let block = self
			.blocks
			.iter()
			.enumerate()
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
					.unwrap_or_else(|| distance(range, offset))
			})?
			.0;
		let placed = snapshot.blocks.get(block)?;
		let mut best: Option<(usize, SourcePosition)> = None;
		self.visit(
			snapshot,
			horizontal,
			placed.y..placed.y + placed.height(),
			|bi, position| {
				if bi != block {
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
				source: placed.source.clone(),
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
				source: placed.source.clone(),
				rect: Rect {
					x: 0.,
					y: placed.y,
					w: placed.layout.width,
					h: placed.height(),
				},
			})
		})
	}

	fn visit(
		&self,
		snapshot: &LayoutSnapshot,
		horizontal: &HashMap<(usize, usize), f32>,
		visible: Range<f32>,
		mut visit: impl FnMut(usize, SourcePosition),
	) {
		snapshot.visit_search_clusters(
			horizontal,
			visible.clone(),
			|block, field, semantic, rect| {
				let Some(spans) = self
					.fields
					.get(block)
					.and_then(|fields| fields.get(field.0))
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
					visit(block, SourcePosition { source, rect });
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
				let images = &self.fields[bi][field.0].images;
				for (index, cluster) in &node.source_images {
					let source = &images[images
						.binary_search_by_key(index, |(i, _)| *i)
						.unwrap()]
					.1;
					let Some(rect) =
						snapshot.text_rect(bi, cluster, horizontal)
					else {
						continue;
					};
					if rect.y + rect.h >= visible.start && rect.y <= visible.end
					{
						visit(
							bi,
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

/// Exact runs stay compact; parser transformations use ordered character matches.
fn align(
	text: &str,
	source: &str,
	range: Range<usize>,
	semantic: usize,
	spans: &mut Vec<(Range<usize>, Range<usize>)>,
) {
	let raw = &source[range.clone()];
	if raw == text {
		spans.push((semantic..semantic + text.len(), range));
		return;
	}
	let mut cursor = 0;
	for (i, ch) in text.char_indices() {
		let mapped = if let Some(found) = raw[cursor..].find(ch) {
			let start = cursor + found;
			cursor = start + ch.len_utf8();
			range.start + start..range.start + cursor
		} else {
			range.clone()
		};
		let reading = semantic + i..semantic + i + ch.len_utf8();
		if let Some((previous, original)) = spans.last_mut()
			&& previous.end == reading.start
			&& original.end == mapped.start
			&& previous.len() == original.len()
			&& reading.len() == mapped.len()
		{
			previous.end = reading.end;
			original.end = mapped.end;
		} else {
			spans.push((reading, mapped));
		}
	}
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
