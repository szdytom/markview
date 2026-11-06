use super::*;
use crate::{BlockKind, DocumentBuilder};
use proptest::prelude::*;

fn block_id(value: u32) -> BlockId {
	BlockId::new(value).unwrap()
}

fn entry(id: u32, source: Range<usize>, span: SourceSpan) -> BlockSource {
	BlockSource {
		block: block_id(id),
		source: Some(source),
		fields: vec![FieldSource {
			field: FieldId::from_index(0),
			spans: vec![span],
			covers: Vec::new(),
		}],
	}
}

proptest! {
	#[test]
	fn overlapping_spans_reverse_lookup_returns_a_declared_cover(
		ranges in prop::collection::vec(prop::collection::vec((0usize..25, 0usize..25, any::<bool>(), 0usize..25), 1..4), 1..5),
		reverse in any::<bool>(),
	) {
		let revision = DocumentRevision::new(1);
		let mut builder = DocumentBuilder::new(revision);
		let mut blocks = Vec::new();
		let mut entries = Vec::new();
		for ranges in ranges {
			let spans: Vec<_> = ranges.into_iter().map(|(a, b, exact, shift)| {
				let source = a.min(b)..a.max(b);
				let len = if exact { source.len() } else { 3 };
				let start = shift % (25 - len);
				SourceSpan { semantic: start..start + len, source, exact }
			}).collect();
			let block = builder.block(Vec::new(), BlockKind::Code {
				language: String::new(), text: "x".repeat(24),
			});
			let cover = spans.iter().map(|span| span.source.start).min().unwrap()..spans.iter().map(|span| span.source.end).max().unwrap();
			entries.push(BlockSource { block: block.id(), source: Some(cover), fields: vec![FieldSource {
				field: FieldId::from_index(0), spans, covers: Vec::new(),
			}] });
			blocks.push(block);
		}
		if reverse { entries.reverse(); }
		let map = SourceMap::new(revision, entries.clone());
		let source = "x".repeat(24);
		prop_assert_eq!(map.validate(&source, &builder.finish(blocks)), Ok(()));
		for offset in 0..=25 {
			let mut candidates = Vec::new();
			for entry in &entries {
				for span in &entry.fields[0].spans {
					// Endpoint positions belong to both adjacent spans; ties are unspecified.
					if (span.source.start..=span.source.end).contains(&offset) {
						let semantic = if span.exact {
							let byte = span.semantic.start + offset - span.source.start;
							byte..byte
						} else { span.semantic.clone() };
						candidates.push((entry.block, FieldId::from_index(0), semantic));
					}
				}
			}
			match map.source_to_semantic(offset) {
				Some(actual) => prop_assert!(candidates.contains(&actual), "offset={offset}, actual={actual:?}, candidates={candidates:?}"),
				None => prop_assert!(candidates.is_empty(), "missing cover at {offset}"),
			}
			let block_candidates: Vec<_> = entries.iter().filter(|entry| { let range = entry.source.as_ref().unwrap(); (range.start..=range.end).contains(&offset) }).map(|entry| entry.block).collect();
			match map.source_block(offset) {
				Some(block) => prop_assert!(block_candidates.contains(&block)),
				None => prop_assert!(block_candidates.is_empty()),
			}
		}
	}

	#[test]
	fn disjoint_fields_map_every_byte_and_preserve_lookup_identity(
		texts in prop::collection::vec(".{0,16}", 0..8),
		revision in 1u64..100,
	) {
		let revision = DocumentRevision::new(revision);
		let mut builder = DocumentBuilder::new(revision);
		let mut blocks = Vec::new();
		let mut entries = Vec::new();
		let mut source = String::new();
		let mut expected = Vec::new();
		for text in texts {
			source.push('|');
			let start = source.len();
			source.push_str(&text);
			let end = source.len();
			let block = builder.block(Vec::new(), BlockKind::Code {
				language: String::new(), text: text.clone(),
			});
			expected.push((block.id(), start, text.len()));
			entries.push(entry(block.id().get(), start..end, SourceSpan {
				semantic: 0..text.len(), source: start..end, exact: true,
			}));
			blocks.push(block);
		}
		// Storage order is independent of block identity and source order.
		entries.reverse();
		let map = SourceMap::new(revision, entries.clone());
		let document = builder.finish(blocks);
		prop_assert_eq!(map.revision(), Some(revision));
		prop_assert_eq!(map.blocks(), entries);
		prop_assert_eq!(map.validate(&source, &document), Ok(()));
		let field = FieldId::from_index(0);
		for (id, start, len) in expected {
			prop_assert_eq!(map.block(id).unwrap().block, id);
			prop_assert!(map.field(id, FieldId::from_index(1)).is_none());
			for offset in 0..=len {
				prop_assert_eq!(map.source_block(start + offset), Some(id));
				prop_assert_eq!(map.source_to_semantic(start + offset), Some((id, field, offset..offset)));
				prop_assert_eq!(map.semantic_to_source(id, field, offset..offset), Some(SourceTarget {
					block: id, field, source: start + offset..start + offset, exact: true,
				}));
			}
			for a in 0..len {
				for b in a + 1..=len {
					prop_assert_eq!(map.semantic_to_source(id, field, a..b).unwrap().source, start + a..start + b);
				}
			}
		}
		prop_assert!(map.block(block_id(100)).is_none());
		prop_assert!(map.source_to_semantic(source.len() + 1).is_none());
		prop_assert!(map.source_block(source.len() + 1).is_none());
	}

	#[test]
	fn transformed_spans_return_the_whole_cover(
		start in 0usize..32, semantic_len in 1usize..32, source_len in 0usize..32,
	) {
		let field = FieldId::from_index(0);
		let map = SourceMap::new(DocumentRevision::new(1), vec![entry(1, start..start + source_len, SourceSpan {
			semantic: 0..semantic_len, source: start..start + source_len, exact: false,
		})]);
		for a in 0..=semantic_len {
			for b in a..=semantic_len {
				prop_assert_eq!(map.semantic_to_source(block_id(1), field, a..b), Some(SourceTarget {
					block: block_id(1), field, source: start..start + source_len, exact: false,
				}));
			}
		}
		for offset in start..=start + source_len {
			prop_assert_eq!(map.source_to_semantic(offset), Some((block_id(1), field, 0..semantic_len)));
		}
	}

	#[test]
	fn validation_checks_block_span_and_inline_cover_boundaries(
		text in ".{0,24}", a in 0usize..128, b in 0usize..128,
	) {
		let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
		let block = builder.block(Vec::new(), BlockKind::Code { language: String::new(), text: text.clone() });
		let document = builder.finish(vec![block]);
		let boundaries: Vec<_> = text.char_indices().map(|(i, _)| i).chain([text.len()]).collect();
		let range = a..b;
		let expected = if a > b || b > text.len() {
			Err(SourceMapError::OutOfBounds(range.clone()))
		} else if !boundaries.contains(&a) {
			Err(SourceMapError::NotCharBoundary(a))
		} else if !boundaries.contains(&b) {
			Err(SourceMapError::NotCharBoundary(b))
		} else { Ok(()) };
		for location in 0..3 {
			let mut block = entry(1, 0..text.len(), SourceSpan {
				semantic: 0..text.len(), source: 0..text.len(), exact: true,
			});
			match location {
				0 => block.source = Some(range.clone()),
				1 => block.fields[0].spans[0].source = range.clone(),
				_ => block.fields[0].covers.push(SourceSpan { semantic: 0..0, source: range.clone(), exact: false }),
			}
			let map = SourceMap::new(document.revision(), vec![block]);
			prop_assert_eq!(map.validate(&text, &document), expected.clone());
		}
	}

	#[test]
	fn reverse_lookup_finds_an_outer_span_after_an_inner_span_ends(
		prefix in 1usize..16, inner_len in 1usize..16, tail in 1usize..16,
	) {
		let end = prefix + inner_len + tail + 1;
		let map = SourceMap::new(DocumentRevision::new(1), vec![
			entry(1, 0..end, SourceSpan { semantic: 0..end, source: 0..end, exact: true }),
			entry(2, prefix..prefix + inner_len, SourceSpan {
				semantic: 0..inner_len, source: prefix..prefix + inner_len, exact: true,
			}),
		]);
		let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
		let blocks = [end, inner_len].map(|len| builder.block(Vec::new(), BlockKind::Code {
			language: String::new(), text: "x".repeat(len),
		}));
		prop_assert_eq!(map.validate(&"x".repeat(end), &builder.finish(blocks.into())), Ok(()));
		let offset = end - 1;
		prop_assert_eq!(map.source_to_semantic(offset), Some((block_id(1), FieldId::from_index(0), offset..offset)));
	}
}
