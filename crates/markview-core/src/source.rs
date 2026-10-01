//! Source navigation over semantic fields and cached rendering geometry.
use crate::{
	document::{Document, InlineKind, plain_text},
	scene::{LayoutSnapshot, Rect},
	search::{FieldText, visit_fields},
};
use std::{collections::HashMap, ops::Range};

/// A rendered source unit, in UTF-8 bytes and document logical pixels.
#[derive(Clone, Debug)]
pub struct SourcePosition {
	pub source: Range<usize>,
	pub rect: Rect,
}

struct FieldSpans {
	text: Vec<(Range<usize>, Range<usize>)>,
	images: Vec<(usize, Range<usize>)>,
}

/// Source bindings belong to a parsed document, independent of cached geometry.
pub struct SourceIndex {
	fields: Vec<Vec<FieldSpans>>,
	blocks: Vec<Range<usize>>,
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
		}
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
			placed.y..placed.y + placed.layout.height,
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
					h: placed.layout.height,
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
		let placed = snapshot.blocks.iter().min_by(|a, b| {
			vertical_distance(a.y, a.layout.height, y)
				.total_cmp(&vertical_distance(b.y, b.layout.height, y))
		})?;
		let mut best: Option<SourcePosition> = None;
		self.visit(
			snapshot,
			horizontal,
			placed.y..placed.y + placed.layout.height,
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
					h: placed.layout.height,
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
			.partition_point(|b| b.y + b.layout.height < visible.start);
		for (bi, block) in snapshot.blocks.iter().enumerate().skip(first) {
			if block.y > visible.end {
				break;
			}
			for node in &block.layout.text {
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
