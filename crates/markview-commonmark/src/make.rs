//! Construction helpers shared by the Markdown and raw-HTML front ends.
//!
//! The parser records source spans next to the semantic text it builds. At the
//! end of a parse the spans are converted into a [`SourceMap`] keyed by the
//! block identities the builder allocated.
use crate::document::{
	Block, BlockId, BlockSource, DocumentBuilder, DocumentRevision, FieldId,
	FieldSource, InlineKind, LinkOccurrenceId, SourceMap, SourceSpan,
};
use std::{collections::HashMap, ops::Range};

/// A finished block and the source metadata it was built from.
pub(crate) struct Made {
	pub(crate) kind: crate::BlockKind,
	pub(crate) anchors: Vec<String>,
	pub(crate) source: Range<usize>,
	/// Spans of this block's own fields, excluding nested blocks.
	pub(crate) fields: Vec<Vec<SourceSpan>>,
	/// Covering interval per inline for the same fields.
	pub(crate) covers: Vec<Vec<SourceSpan>>,
}

struct Record {
	source: Range<usize>,
	fields: Vec<Vec<SourceSpan>>,
	covers: Vec<Vec<SourceSpan>>,
}

impl Record {
	/// Maps every recorded range through `map`.
	fn remap(&mut self, map: &impl Fn(Range<usize>) -> Range<usize>) {
		self.source = map(self.source.clone());
		for field in &mut self.fields {
			for span in field {
				span.source = map(span.source.clone());
			}
		}
		for field in &mut self.covers {
			for cover in field {
				cover.source = map(cover.source.clone());
			}
		}
	}
}

/// Allocates identities and collects source spans for one parse.
pub(crate) struct Producer {
	builder: DocumentBuilder,
	records: HashMap<BlockId, Record>,
}

impl Producer {
	pub(crate) fn new(revision: DocumentRevision) -> Self {
		Self {
			builder: DocumentBuilder::new(revision),
			records: HashMap::new(),
		}
	}

	pub(crate) fn continuing(
		revision: DocumentRevision,
		after: Option<BlockId>,
	) -> Self {
		Self {
			builder: DocumentBuilder::continuing(revision, after),
			records: HashMap::new(),
		}
	}

	pub(crate) fn block(&mut self, made: Made) -> Block {
		let block = self.builder.block(made.anchors, made.kind);
		self.records.insert(
			block.id(),
			Record {
				source: made.source,
				fields: made.fields,
				covers: made.covers,
			},
		);
		block
	}

	/// Remaps the recorded source ranges of `blocks` and their subtree.
	pub(crate) fn remap(&mut self, blocks: &mut [Block], offsets: &[usize]) {
		let map = |range: Range<usize>| {
			let start = offsets[range.start.min(offsets.len() - 1)];
			let end = if range.end > range.start {
				offsets[(range.end - 1).min(offsets.len() - 1)] + 1
			} else {
				start
			};
			start..end.max(start)
		};
		for block in blocks {
			self.remap_block(block, &map);
			rebase_links(block, &map);
		}
	}

	fn remap_block(
		&mut self,
		block: &Block,
		map: &impl Fn(Range<usize>) -> Range<usize>,
	) {
		if let Some(record) = self.records.get_mut(&block.id()) {
			record.remap(map);
		}
		match block.kind() {
			crate::BlockKind::Quote { blocks, .. }
			| crate::BlockKind::Footnote { blocks, .. }
			| crate::BlockKind::Details { blocks, .. }
			| crate::BlockKind::FrontMatter { blocks, .. } => {
				for child in blocks {
					self.remap_block(child, map);
				}
			}
			crate::BlockKind::List { items, .. } => {
				for item in items {
					for child in &item.blocks {
						self.remap_block(child, map);
					}
				}
			}
			crate::BlockKind::Table(table) => {
				for child in table.cell_blocks() {
					self.remap_block(child, map);
				}
			}
			crate::BlockKind::DefinitionList(items) => {
				for item in items {
					for blocks in &item.definitions {
						for child in blocks {
							self.remap_block(child, map);
						}
					}
				}
			}
			crate::BlockKind::Figure { blocks, .. } => {
				for child in blocks {
					self.remap_block(child, map);
				}
			}
			_ => {}
		}
	}

	pub(crate) fn block_range(&self, block: &Block) -> Option<Range<usize>> {
		self.records
			.get(&block.id())
			.map(|record| record.source.clone())
	}

	/// Seals the document and returns it with a source map in the document's
	/// own field order.
	pub(crate) fn finish(
		self,
		blocks: Vec<Block>,
	) -> (crate::Document, SourceMap) {
		let source_map = build_map(&blocks, &self.records);
		let revision = self.builder.revision();
		let document = self.builder.finish(blocks);
		(document, SourceMap::new(revision, source_map))
	}
}

/// Records one of `block`'s own fields at the ordinal the walk reached.
fn own_field<'a>(
	owner: BlockId,
	record: Option<&'a Record>,
	own: &mut usize,
	ordinal: &mut u32,
	visit: &mut impl FnMut(BlockId, FieldId, &'a Record, usize),
) {
	let field = FieldId::from_index(*ordinal);
	*ordinal += 1;
	let index = *own;
	*own += 1;
	if let Some(record) = record {
		visit(owner, field, record, index);
	}
}

/// Visits the fields of `block`'s subtree in [`crate::visit_fields`] order,
/// with the ordinal each field has in that walk.
///
/// This is the producer-side exception to the single traversal: source spans
/// are recorded while blocks are being built, before [`crate::visit_fields`]
/// can read the finished blocks. The ordinal tests below keep the two walks
/// in step.
#[cfg_attr(
	any(),
	markview::allow_complexity(
		reason = "Keep the exhaustive block-kind dispatch in one walk, so it stays in step with visit_fields."
	)
)]
fn walk_fields<'a>(
	block: &'a Block,
	records: &'a HashMap<BlockId, Record>,
	ordinal: &mut u32,
	visit: &mut impl FnMut(BlockId, FieldId, &'a Record, usize),
) {
	let owner = block.id();
	let record = records.get(&owner);
	let mut own = 0usize;
	match block.kind() {
		crate::BlockKind::Paragraph(_)
		| crate::BlockKind::Heading { .. }
		| crate::BlockKind::Code { .. } => {
			own_field(owner, record, &mut own, ordinal, visit)
		}
		crate::BlockKind::Rule => {}
		// A table's caption and a figure's caption come after their children in
		// reading order, so the block's own fields follow the nested ones.
		crate::BlockKind::Table(table) => {
			for child in table.cell_blocks() {
				walk_fields(child, records, ordinal, visit);
			}
			if table.caption.is_some() {
				own_field(owner, record, &mut own, ordinal, visit);
			}
		}
		crate::BlockKind::Figure { blocks, caption } => {
			for child in blocks {
				walk_fields(child, records, ordinal, visit);
			}
			if caption.is_some() {
				own_field(owner, record, &mut own, ordinal, visit);
			}
		}
		crate::BlockKind::Details { blocks, .. } => {
			own_field(owner, record, &mut own, ordinal, visit);
			for child in blocks {
				walk_fields(child, records, ordinal, visit);
			}
		}
		// A term belongs to its own item, so terms and definition bodies
		// alternate instead of all terms coming first.
		crate::BlockKind::DefinitionList(items) => {
			for item in items {
				for _ in &item.terms {
					own_field(owner, record, &mut own, ordinal, visit);
				}
				for blocks in &item.definitions {
					for child in blocks {
						walk_fields(child, records, ordinal, visit);
					}
				}
			}
		}
		crate::BlockKind::Quote {
			callout: Some(callout),
			blocks,
		} if callout.fold.is_none() => {
			own_field(owner, record, &mut own, ordinal, visit);
			for child in blocks {
				walk_fields(child, records, ordinal, visit);
			}
		}
		crate::BlockKind::Quote { blocks, .. }
		| crate::BlockKind::Footnote { blocks, .. }
		| crate::BlockKind::FrontMatter { blocks, .. } => {
			for child in blocks {
				walk_fields(child, records, ordinal, visit);
			}
		}
		crate::BlockKind::List { items, .. } => {
			for item in items {
				for child in &item.blocks {
					walk_fields(child, records, ordinal, visit);
				}
			}
		}
	}
}

/// The fields of every block in one top-level subtree, in
/// [`crate::visit_fields`] order.
fn collect(
	block: &Block,
	records: &HashMap<BlockId, Record>,
	out: &mut Vec<FieldSource>,
) {
	let mut ordinal = 0;
	walk_fields(
		block,
		records,
		&mut ordinal,
		&mut |_, field, record, index| {
			out.push(FieldSource {
				field,
				spans: record.fields.get(index).cloned().unwrap_or_default(),
				covers: record.covers.get(index).cloned().unwrap_or_default(),
			});
		},
	);
}

/// One nested block's own fields, numbered as [`crate::visit_fields`] numbers
/// them inside that block.
fn entry(block: &Block, records: &HashMap<BlockId, Record>) -> BlockSource {
	let id = block.id();
	let mut fields = Vec::new();
	let mut ordinal = 0;
	walk_fields(
		block,
		records,
		&mut ordinal,
		&mut |owner, field, record, index| {
			if owner != id {
				return;
			}
			fields.push(FieldSource {
				field,
				spans: record.fields.get(index).cloned().unwrap_or_default(),
				covers: record.covers.get(index).cloned().unwrap_or_default(),
			});
		},
	);
	BlockSource {
		block: id,
		source: records.get(&id).map(|record| record.source.clone()),
		fields,
	}
}

fn build_map(
	blocks: &[Block],
	records: &HashMap<BlockId, Record>,
) -> Vec<BlockSource> {
	fn nested(
		block: &Block,
		records: &HashMap<BlockId, Record>,
		out: &mut Vec<BlockSource>,
	) {
		match block.kind() {
			crate::BlockKind::Quote { blocks, .. }
			| crate::BlockKind::Footnote { blocks, .. }
			| crate::BlockKind::Details { blocks, .. }
			| crate::BlockKind::FrontMatter { blocks, .. } => {
				for child in blocks {
					out.push(entry(child, records));
					nested(child, records, out);
				}
			}
			crate::BlockKind::List { items, .. } => {
				for item in items {
					for child in &item.blocks {
						out.push(entry(child, records));
						nested(child, records, out);
					}
				}
			}
			crate::BlockKind::Table(table) => {
				for child in table.cell_blocks() {
					out.push(entry(child, records));
					nested(child, records, out);
				}
			}
			crate::BlockKind::DefinitionList(items) => {
				for item in items {
					for blocks in &item.definitions {
						for child in blocks {
							out.push(entry(child, records));
							nested(child, records, out);
						}
					}
				}
			}
			crate::BlockKind::Figure { blocks, .. } => {
				for child in blocks {
					out.push(entry(child, records));
					nested(child, records, out);
				}
			}
			_ => {}
		}
	}
	let mut out = Vec::new();
	for block in blocks {
		let mut fields = Vec::new();
		collect(block, records, &mut fields);
		out.push(BlockSource {
			block: block.id(),
			source: records
				.get(&block.id())
				.map(|record| record.source.clone()),
			fields,
		});
		nested(block, records, &mut out);
	}
	out
}

fn rebase_callout<M: Fn(Range<usize>) -> Range<usize>>(
	block: &mut Block,
	map: &M,
	rich: &impl Fn(&mut crate::RichText, &M),
) {
	if let crate::BlockKind::Quote {
		callout: Some(callout),
		..
	} = block.kind_mut()
		&& let Some(title) = &mut callout.title
	{
		rich(title, map);
	}
}

pub(crate) fn rebase_links(
	block: &mut Block,
	map: &impl Fn(Range<usize>) -> Range<usize>,
) {
	fn rich(
		text: &mut crate::RichText,
		map: &impl Fn(Range<usize>) -> Range<usize>,
	) {
		for inline in text {
			let occurrence = |old: LinkOccurrenceId| {
				let local = old.get() as usize - 1;
				let source = map(local..local + 1).start;
				crate::rich::source_occurrence(source)
			};
			if let Some(link) = &inline.link {
				inline.link =
					Some(link.with_occurrence(occurrence(link.occurrence())));
			}
			if let InlineKind::FootnoteRef(reference) = &mut inline.kind {
				reference.occurrence = occurrence(reference.occurrence);
			}
		}
	}
	// A plain callout's title is a field of its own; a folded callout's title
	// is the summary of the child disclosure walked below.
	rebase_callout(block, map, &rich);
	match block.kind_mut() {
		crate::BlockKind::Paragraph(text)
		| crate::BlockKind::Heading { text, .. } => rich(text, map),
		crate::BlockKind::Table(table) => {
			if let Some(caption) = &mut table.caption {
				rich(caption, map);
			}
			for child in table.cell_blocks_mut() {
				rebase_links(child, map);
			}
		}
		crate::BlockKind::DefinitionList(items) => {
			for item in items {
				for term in &mut item.terms {
					rich(term, map);
				}
				for blocks in &mut item.definitions {
					rebase_all(blocks, map);
				}
			}
		}
		crate::BlockKind::Figure { blocks, caption } => {
			rebase_all(blocks, map);
			if let Some(caption) = caption {
				rich(caption, map);
			}
		}
		crate::BlockKind::Details {
			summary, blocks, ..
		} => {
			rich(summary, map);
			rebase_all(blocks, map);
		}
		crate::BlockKind::Quote { blocks, .. }
		| crate::BlockKind::Footnote { blocks, .. }
		| crate::BlockKind::FrontMatter { blocks, .. } => {
			rebase_all(blocks, map);
		}
		crate::BlockKind::List { items, .. } => {
			for item in items {
				rebase_all(&mut item.blocks, map);
			}
		}
		crate::BlockKind::Code { .. } | crate::BlockKind::Rule => {}
	}
}

/// Rebases the links of every block in one subtree, in reading order.
fn rebase_all(
	blocks: &mut [Block],
	map: &impl Fn(Range<usize>) -> Range<usize>,
) {
	for block in blocks {
		rebase_links(block, map);
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{
		BlockKind, CellKind, FieldText, Inline, InlineKind, RichText, Table,
		TableCell, TableGroup, TableGroupKind, TableRow, plain_text,
		visit_fields,
	};

	fn text(value: &str) -> RichText {
		vec![Inline {
			link: None,
			kind: InlineKind::Text(value.to_owned()),
			style: markview_document::TextStyle::default(),
		}]
	}

	fn paragraph(value: &str, source: usize) -> Made {
		Made {
			kind: BlockKind::Paragraph(text(value)),
			anchors: Vec::new(),
			source: source..source + 1,
			fields: vec![span(source)],
			covers: Vec::new(),
		}
	}

	fn span(source: usize) -> Vec<SourceSpan> {
		vec![SourceSpan {
			semantic: 0..1,
			source: source..source + 1,
			exact: false,
		}]
	}

	/// The field markers each block contributes, in reading order.
	fn markers() -> Vec<(&'static str, usize)> {
		vec![
			("term", 10),
			("definition", 20),
			("figure body", 30),
			("figure caption", 40),
			("cell", 50),
			("table caption", 60),
		]
	}

	fn assert_field_order(
		map: &SourceMap,
		block: &Block,
		markers: &[(&str, usize)],
	) {
		let mut texts = Vec::new();
		visit_fields(
			block,
			&mut Vec::new(),
			&|| false,
			&mut |_, _, text, _| {
				texts.push(match text {
					FieldText::Rich(rich) => plain_text(rich),
					FieldText::Code(code) => code.to_owned(),
				});
			},
		);
		let expected: Vec<usize> = texts
			.iter()
			.map(|text| {
				markers
					.iter()
					.find_map(|(marker, source)| {
						(marker == text).then_some(*source)
					})
					.unwrap_or_else(|| panic!("unexpected field {text:?}"))
			})
			.collect();
		let entry = map.block(block.id()).expect("a source entry");
		let actual: Vec<usize> = entry
			.fields
			.iter()
			.map(|field| {
				field.spans.first().map(|span| span.source.start).unwrap()
			})
			.collect();
		assert_eq!(actual, expected, "{texts:?}");
		assert_eq!(entry.fields.len(), texts.len(), "{texts:?}");
	}

	/// Checks every entry's field ordinals against [`visit_fields`], including
	/// the entries of nested blocks.
	fn assert_entry_ordinals(
		map: &SourceMap,
		document: &markview_document::Document,
		markers: &[(&str, usize)],
	) {
		for root in document.blocks() {
			markview_document::visit_blocks(root, &mut |block| {
				let Some(entry) = map.block(block.id()) else {
					return;
				};
				let mut texts = Vec::new();
				visit_fields(
					block,
					&mut Vec::new(),
					&|| false,
					&mut |_, _, text, _| {
						texts.push(match text {
							FieldText::Rich(rich) => plain_text(rich),
							FieldText::Code(code) => code.to_owned(),
						});
					},
				);
				for field in &entry.fields {
					let index = field.field.get() as usize;
					let Some(text) = texts.get(index) else {
						panic!("field {index} is outside {texts:?}");
					};
					let source = markers
						.iter()
						.find_map(|(marker, source)| {
							(marker == text).then_some(*source)
						})
						.unwrap_or_else(|| panic!("unexpected field {text:?}"));
					assert_eq!(
						field.spans.first().map(|span| span.source.start),
						Some(source),
						"{text:?}"
					);
				}
			});
		}
	}

	#[test]
	fn nested_field_ordinals_follow_the_reading_order_of_every_container() {
		let mut producer = Producer::new(DocumentRevision::new(1));
		let definition = producer.block(paragraph("definition", 20));
		let definitions = producer.block(Made {
			kind: BlockKind::DefinitionList(vec![
				markview_document::DefinitionItem {
					terms: vec![text("term")],
					definitions: vec![vec![definition]],
				},
			]),
			anchors: Vec::new(),
			source: 10..11,
			// A term belongs to the item, so it sorts first as the block's own
			// field, ahead of the definition blocks.
			fields: vec![span(10)],
			covers: Vec::new(),
		});
		let body = producer.block(paragraph("figure body", 30));
		let figure = producer.block(Made {
			kind: BlockKind::Figure {
				blocks: vec![body],
				caption: Some(text("figure caption")),
			},
			anchors: Vec::new(),
			source: 30..31,
			// A figure's caption follows its body in reading order.
			fields: vec![span(40)],
			covers: Vec::new(),
		});
		let cell = producer.block(paragraph("cell", 50));
		let table = producer.block(Made {
			kind: BlockKind::Table(Table {
				caption: Some(text("table caption")),
				columns: Vec::new(),
				groups: vec![TableGroup {
					kind: TableGroupKind::Body,
					rows: vec![TableRow {
						cells: vec![TableCell {
							kind: CellKind::Data,
							rowspan: 1,
							colspan: 1,
							align: None,
							blocks: vec![cell],
						}],
					}],
				}],
			}),
			anchors: Vec::new(),
			source: 50..51,
			// A table's caption follows its cells in reading order.
			fields: vec![span(60)],
			covers: Vec::new(),
		});
		let (document, map) = producer.finish(vec![definitions, figure, table]);
		let markers = markers();
		for block in document.blocks() {
			assert_field_order(&map, block, &markers);
		}
		assert_entry_ordinals(&map, &document, &markers);
		assert!(
			map.validate(
				"[term][definition][figure body][figure caption][cell][table caption]",
				&document
			)
			.is_ok()
		);
	}

	#[test]
	fn definition_terms_and_bodies_alternate_within_each_item() {
		let mut producer = Producer::new(DocumentRevision::new(1));
		let first = producer.block(paragraph("definition a", 20));
		let second = producer.block(paragraph("definition b", 40));
		let definitions = producer.block(Made {
			kind: BlockKind::DefinitionList(vec![
				markview_document::DefinitionItem {
					terms: vec![text("term a")],
					definitions: vec![vec![first]],
				},
				markview_document::DefinitionItem {
					terms: vec![text("term b")],
					definitions: vec![vec![second]],
				},
			]),
			anchors: Vec::new(),
			source: 10..11,
			// Terms are the block's own fields, recorded in document order.
			fields: vec![span(10), span(30)],
			covers: Vec::new(),
		});
		let (document, map) = producer.finish(vec![definitions]);
		let markers = [
			("term a", 10),
			("definition a", 20),
			("term b", 30),
			("definition b", 40),
		];
		assert_field_order(&map, &document.blocks()[0], &markers);
		assert_entry_ordinals(&map, &document, &markers);
		assert!(map.validate(&"x".repeat(41), &document).is_ok());
	}

	#[test]
	fn nested_caption_ordinals_match_the_visit_fields_walk() {
		let mut producer = Producer::new(DocumentRevision::new(1));
		let body = producer.block(paragraph("figure body", 10));
		let figure = producer.block(Made {
			kind: BlockKind::Figure {
				blocks: vec![body],
				caption: Some(text("figure caption")),
			},
			anchors: Vec::new(),
			source: 10..11,
			fields: vec![span(20)],
			covers: Vec::new(),
		});
		let inner_cell = producer.block(paragraph("inner cell", 30));
		let inner = producer.block(Made {
			kind: BlockKind::Table(Table {
				caption: Some(text("inner caption")),
				columns: Vec::new(),
				groups: vec![TableGroup {
					kind: TableGroupKind::Body,
					rows: vec![TableRow {
						cells: vec![TableCell {
							kind: CellKind::Data,
							rowspan: 1,
							colspan: 1,
							align: None,
							blocks: vec![inner_cell],
						}],
					}],
				}],
			}),
			anchors: Vec::new(),
			source: 30..31,
			fields: vec![span(40)],
			covers: Vec::new(),
		});
		let outer = producer.block(Made {
			kind: BlockKind::Table(Table {
				caption: Some(text("outer caption")),
				columns: Vec::new(),
				groups: vec![TableGroup {
					kind: TableGroupKind::Body,
					rows: vec![TableRow {
						cells: vec![TableCell {
							kind: CellKind::Data,
							rowspan: 1,
							colspan: 1,
							align: None,
							blocks: vec![figure, inner],
						}],
					}],
				}],
			}),
			anchors: Vec::new(),
			source: 10..11,
			fields: vec![span(50)],
			covers: Vec::new(),
		});
		let (document, map) = producer.finish(vec![outer]);
		let markers = [
			("figure body", 10),
			("figure caption", 20),
			("inner cell", 30),
			("inner caption", 40),
			("outer caption", 50),
		];
		assert_field_order(&map, &document.blocks()[0], &markers);
		assert_entry_ordinals(&map, &document, &markers);
		assert!(map.validate(&"x".repeat(51), &document).is_ok());
	}

	#[test]
	fn validation_matches_a_container_field_by_id_not_by_position() {
		// A table's caption is its second field because its cells come first. A
		// map that leaves the cell's field out must not validate the caption's
		// span against the cell's text, which a positional match did.
		let mut producer = Producer::new(DocumentRevision::new(1));
		let cell = producer.block(paragraph("cell", 50));
		let table = producer.block(Made {
			kind: BlockKind::Table(Table {
				caption: Some(text("a longer caption")),
				columns: Vec::new(),
				groups: vec![TableGroup {
					kind: TableGroupKind::Body,
					rows: vec![TableRow {
						cells: vec![TableCell {
							kind: CellKind::Data,
							rowspan: 1,
							colspan: 1,
							align: None,
							blocks: vec![cell],
						}],
					}],
				}],
			}),
			anchors: Vec::new(),
			source: 50..51,
			fields: vec![vec![SourceSpan {
				semantic: 0..16,
				source: 60..76,
				exact: false,
			}]],
			covers: Vec::new(),
		});
		let (document, map) = producer.finish(vec![table]);
		let root = document.blocks()[0].id();
		let mut blocks = map.blocks().to_vec();
		let entry = blocks
			.iter_mut()
			.find(|entry| entry.block == root)
			.expect("the table's entry");
		assert_eq!(entry.fields.len(), 2);
		entry.fields.remove(0);
		let map = SourceMap::new(document.revision(), blocks.clone());
		assert!(map.validate(&"x".repeat(76), &document).is_ok());
		// Removal stays valid, but an id no field of the block reaches does not.
		let unknown = markview_document::FieldId::from_index(2);
		blocks
			.iter_mut()
			.find(|entry| entry.block == root)
			.expect("the table's entry")
			.fields[0]
			.field = unknown;
		let map = SourceMap::new(document.revision(), blocks);
		assert_eq!(
			map.validate(&"x".repeat(76), &document),
			Err(markview_document::SourceMapError::UnknownField(unknown))
		);
	}
}
