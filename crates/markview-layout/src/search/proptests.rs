use super::*;
use crate::document::{
	BlockId, BlockKind, CellKind, DefinitionItem, DocumentBuilder,
	DocumentRevision, Inline, InlineKind, ListItem, RichText, Table, TableCell,
	TableColumn, TableGroup, TableGroupKind, TableRow,
};
use proptest::prelude::*;

fn rich(s: &str) -> RichText {
	vec![Inline {
		link: None,
		kind: InlineKind::Text(s.into()),
		style: Default::default(),
	}]
}
type ModelField = (BlockId, u32, String, Vec<BlockId>);

fn fixture(
	values: &[(u8, String)],
	revision: DocumentRevision,
) -> (Document, Vec<ModelField>) {
	let mut builder = DocumentBuilder::new(revision);
	let mut blocks = Vec::new();
	let mut fields = Vec::new();
	for (kind, s) in values {
		let leaf = builder.block(vec![], BlockKind::Paragraph(rich(s)));
		let (node, descriptions) = match kind % 8 {
			0 => (leaf, vec![(s.clone(), vec![])]),
			1 => (
				builder.block(
					vec![],
					BlockKind::Heading {
						level: 1,
						text: rich(s),
						anchor: String::new(),
					},
				),
				vec![(s.clone(), vec![])],
			),
			2 => (
				builder.block(
					vec![],
					BlockKind::Code {
						language: String::new(),
						text: s.clone(),
					},
				),
				vec![(s.clone(), vec![])],
			),
			3 => {
				let second =
					builder.block(vec![], BlockKind::Paragraph(rich(s)));
				let cells = [leaf, second]
					.into_iter()
					.map(|block| TableCell {
						kind: CellKind::Data,
						rowspan: 1,
						colspan: 1,
						align: None,
						blocks: vec![block],
					})
					.collect();
				let table = Table {
					caption: Some(rich("caption")),
					columns: vec![TableColumn { align: None }; 2],
					groups: vec![TableGroup {
						kind: TableGroupKind::Body,
						rows: vec![TableRow { cells }],
					}],
				};
				(
					builder.block(vec![], BlockKind::Table(table)),
					vec![
						(s.clone(), vec![]),
						(s.clone(), vec![]),
						("caption".into(), vec![]),
					],
				)
			}
			4 => (
				builder.block(
					vec![],
					BlockKind::List {
						start: None,
						reversed: false,
						tight: false,
						items: vec![ListItem {
							checked: None,
							number: None,
							blocks: vec![leaf],
						}],
					},
				),
				vec![(s.clone(), vec![])],
			),
			5 => {
				let inner = builder.block(
					vec![],
					BlockKind::Details {
						default_open: true,
						summary: rich("inner"),
						blocks: vec![leaf],
					},
				);
				let inner_id = inner.id();
				let outer = builder.block(
					vec![],
					BlockKind::Details {
						default_open: false,
						summary: rich("summary"),
						blocks: vec![inner],
					},
				);
				let id = outer.id();
				(
					outer,
					vec![
						("summary".into(), vec![]),
						("inner".into(), vec![id]),
						(s.clone(), vec![id, inner_id]),
					],
				)
			}
			6 => (
				builder.block(
					vec![],
					BlockKind::DefinitionList(vec![DefinitionItem {
						terms: vec![rich("term"), rich(s)],
						definitions: vec![vec![leaf]],
					}]),
				),
				vec![
					("term".into(), vec![]),
					(s.clone(), vec![]),
					(s.clone(), vec![]),
				],
			),
			_ => (
				builder.block(
					vec![],
					BlockKind::Figure {
						blocks: vec![leaf],
						caption: Some(rich("caption")),
					},
				),
				vec![(s.clone(), vec![]), ("caption".into(), vec![])],
			),
		};
		for (ordinal, (text, path)) in descriptions.into_iter().enumerate() {
			fields.push((node.id(), ordinal as u32, text, path));
		}
		blocks.push(node);
	}
	(builder.finish(blocks), fields)
}
fn expected(
	fields: &[ModelField],
	query: &str,
	revision: DocumentRevision,
) -> Vec<SearchMatch> {
	let mut result = Vec::new();
	if query.is_empty() {
		return result;
	}
	for (block, field, text, path) in fields {
		for (start, _) in text.match_indices(query) {
			result.push(SearchMatch {
				revision,
				block: *block,
				field: FieldId::from_index(*field),
				range: start..start + query.len(),
				enclosing: Arc::from(path.as_slice()),
			});
		}
	}
	result
}

#[test]
fn declared_unicode_equivalence_and_word_boundaries_keep_field_relative_bytes()
{
	// Simple case folding does not expand `ß` or normalize combining sequences.
	for (text, query, ranges) in [
		("Σσς", "σ", vec![0..2, 2..4, 4..6]),
		("KkK", "k", vec![0..1, 1..2, 2..5]),
		("Sſs", "s", vec![0..1, 1..3, 3..4]),
		("ßẞss", "ß", vec![0..2, 2..5]),
		("é e\u{301}", "é", std::iter::once(0..2).collect()),
		("Iİıi", "i", vec![0..1, 5..6]),
		(".*? a.*?", ".*?", vec![0..3, 5..8]),
	] {
		let (document, fields) = fixture(
			&[(0, text.to_owned()), (2, text.to_owned())],
			DocumentRevision::new(71),
		);
		let index = SearchIndex::new(&document);
		let revision = document.revision();
		let expected: Vec<_> = fields
			.iter()
			.flat_map(|(block, field, _, _)| {
				ranges.iter().cloned().map(move |range| SearchMatch {
					revision,
					block: *block,
					field: FieldId::from_index(*field),
					range,
					enclosing: Arc::from([]),
				})
			})
			.collect();
		assert_eq!(
			index.find(query, SearchOptions::default()),
			expected,
			"text={text:?}, query={query:?}"
		);
	}
	for (text, query, ranges) in [
		("café cafétéria café", "café", vec![0..5, 18..23]),
		("e\u{301} e\u{301}x e\u{301}", "e\u{301}", vec![0..3, 9..12]),
		("naïve (naïve)", "naïve", vec![0..6, 8..14]),
	] {
		let (document, _) =
			fixture(&[(0, text.to_owned())], DocumentRevision::new(72));
		let index = SearchIndex::new(&document);
		let options = SearchOptions {
			case_sensitive: true,
			whole_word: true,
		};
		let actual: Vec<_> = index
			.find(query, options)
			.into_iter()
			.map(|m| m.range)
			.collect();
		assert_eq!(actual, ranges, "text={text:?}");
		let cancelled = std::cell::Cell::new(false);
		assert!(
			index
				.find_cancellable(query, options, || {
					cancelled.set(true);
					true
				})
				.is_none()
		);
		assert!(cancelled.get());
		assert_eq!(
			index.find(query, options),
			SearchIndex::new(&document).find(query, options)
		);
	}
}

proptest! {
	#[test]
	fn literal_search_matches_independent_fields(values in prop::collection::vec((0u8..8,"[abAB中😀.*? ]{0,40}"),0..10), query in prop_oneof!["[abAB中😀.*? ]{0,4}",Just("summary".to_owned()),Just("inner".to_owned()),Just("caption".to_owned()),Just("term".to_owned())], revision in 1u64..1000) {
		let (doc, fields) = fixture(&values, DocumentRevision::new(revision));
		let index = SearchIndex::new(&doc);
		let options = SearchOptions {
			case_sensitive: true,
			whole_word: false,
		};
		let actual = index.find(&query, options);
		prop_assert_eq!(
			&actual,
			&expected(&fields, &query, DocumentRevision::new(revision))
		);
		prop_assert_eq!(
			index.find_cancellable(&query, options, || false),
			Some(actual.clone())
		);
		prop_assert_eq!(index.find_cancellable(&query, options, || true), None);
		let whole = index.find(
			&query,
			SearchOptions {
				whole_word: true,
				..options
			},
		);
		prop_assert!(whole.iter().all(|m| actual.contains(m)));
		prop_assert_eq!(
			&whole,
			&index.find(
				&query,
				SearchOptions {
					whole_word: true,
					..options
				}
			)
		);
		if !doc.blocks().is_empty() {
			prop_assert!(SearchIndex::new_cancellable(&doc, || true).is_none());
		}
	}
	#[test]
	fn ascii_case_search_has_independent_model(text in "[aAbB .?]{0,96}",query in "[aAbB.?]{1,4}") {
		let (doc, mut fields) = fixture(&[(0, text)], DocumentRevision::new(17));
		let index = SearchIndex::new(&doc);
		fields[0].2.make_ascii_lowercase();
		prop_assert_eq!(
			index.find(&query, SearchOptions::default()),
			expected(
				&fields,
				&query.to_ascii_lowercase(),
				DocumentRevision::new(17)
			)
		);
	}
	#[test]
	fn literal_spanning_scan_boundary(pad in 65520usize..65545, token in prop::sample::select(vec!["needle","中😀.*?"])) {
		let text = format!("{}{token}{}", "x".repeat(pad), "x".repeat(128));
		let (doc, fields) = fixture(&[(2, text)], DocumentRevision::new(23));
		let index = SearchIndex::new(&doc);
		let options = SearchOptions {
			case_sensitive: true,
			whole_word: false,
		};
		prop_assert_eq!(
			index.find(token, options),
			expected(&fields, token, DocumentRevision::new(23))
		);
		let calls = std::cell::Cell::new(0);
		let result = index.find_cancellable("absent", options, || {
			calls.set(calls.get() + 1);
			calls.get() >= 3
		});
		prop_assert_eq!(result, None);
	}
}
