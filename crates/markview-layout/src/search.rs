//! Full-document literal search over semantic reading fields.
use crate::document::{Document, FieldId, plain_text};
use std::{
	ops::Range,
	sync::{Arc, OnceLock},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchOptions {
	pub case_sensitive: bool,
	pub whole_word: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchMatch {
	pub revision: markview_document::DocumentRevision,
	pub field: FieldId,
	pub range: Range<usize>,
	pub block: markview_document::BlockId,
	pub enclosing: Arc<[markview_document::BlockId]>,
}
struct Field {
	boundaries: OnceLock<Vec<usize>>,
	id: FieldId,
	range: Range<usize>,
	block: markview_document::BlockId,
	enclosing: Arc<[markview_document::BlockId]>,
}
/// Literal matches in semantic field order, with field-relative UTF-8 ranges.
/// Matches never cross fields and do not overlap within a field. Empty queries
/// produce no matches. Cancellation returns `None` once observed.
pub struct SearchIndex {
	revision: markview_document::DocumentRevision,
	text: Box<str>,
	fields: Box<[Field]>,
}
impl SearchIndex {
	pub fn new(document: &Document) -> Self {
		Self::new_cancellable(document, || false).unwrap()
	}
	pub fn new_cancellable(
		document: &Document,
		cancelled: impl Fn() -> bool,
	) -> Option<Self> {
		let mut fields = Vec::new();
		let mut reading = String::new();
		let empty: Arc<[markview_document::BlockId]> = Arc::from([]);
		for block in document.blocks() {
			markview_document::visit_fields(
				block,
				&mut Vec::new(),
				&cancelled,
				&mut |_, id, text, enclosing| {
					let start = reading.len();
					reading.push_str(&field_reading(text));
					fields.push(Field {
						boundaries: OnceLock::new(),
						id,
						range: start..reading.len(),
						block: block.id(),
						enclosing: if enclosing.is_empty() {
							empty.clone()
						} else {
							Arc::from(enclosing)
						},
					});
				},
			);
			if cancelled() {
				return None;
			}
		}
		Some(Self {
			revision: document.revision(),
			text: reading.into_boxed_str(),
			fields: fields.into_boxed_slice(),
		})
	}

	pub fn find(
		&self,
		query: &str,
		options: SearchOptions,
	) -> Vec<SearchMatch> {
		self.find_cancellable(query, options, || false).unwrap()
	}
	pub fn find_cancellable(
		&self,
		query: &str,
		options: SearchOptions,
		cancelled: impl Fn() -> bool,
	) -> Option<Vec<SearchMatch>> {
		let mut found = Vec::new();
		if cancelled() {
			return None;
		}
		if query.is_empty() {
			return Some(found);
		}
		let regex = regex_automata::meta::Regex::builder()
			.configure(
				regex_automata::meta::Regex::config().nfa_size_limit(None),
			)
			.syntax(
				regex_automata::util::syntax::Config::new()
					.case_insensitive(!options.case_sensitive),
			)
			.build(&regex::escape(query))
			.expect("an escaped literal has no invalid syntax or size cap");
		let mut cache = regex.create_cache();
		let overlap = query.chars().count().saturating_mul(4);
		let mut offset = 0;
		let mut field_index = 0;
		while offset < self.text.len() {
			if cancelled() {
				return None;
			}
			let end = self.text.floor_char_boundary(
				(offset + 64 * 1024 + overlap).min(self.text.len()),
			);
			let input =
				regex_automata::Input::new(&self.text[..end]).span(offset..end);
			let Some(m) = regex.search_with(&mut cache, &input) else {
				if end == self.text.len() {
					break;
				}
				offset = self.text.ceil_char_boundary(end - overlap);
				continue;
			};
			while self.fields[field_index].range.end <= m.start() {
				if cancelled() {
					return None;
				}
				field_index += 1;
			}
			let field = &self.fields[field_index];
			if m.end() > field.range.end {
				// A literal has a fixed character count: later starts in this
				// field cannot fit either. Retry from the next field boundary.
				offset = field.range.end;
				continue;
			}
			offset = m.end();
			let range =
				m.start() - field.range.start..m.end() - field.range.start;
			if options.whole_word {
				if field.boundaries.get().is_none() {
					let mut boundaries = Vec::new();
					for end in crate::text::word_segmenter()
						.segment_str(&self.text[field.range.clone()])
					{
						if cancelled() {
							return None;
						}
						boundaries.push(end);
					}
					let _ = field.boundaries.set(boundaries);
				}
				let boundaries = field.boundaries.get().unwrap();
				if boundaries.binary_search(&range.start).is_err()
					|| boundaries.binary_search(&range.end).is_err()
				{
					continue;
				}
			}
			found.push(SearchMatch {
				revision: self.revision,
				field: field.id,
				range,
				block: field.block,
				enclosing: field.enclosing.clone(),
			});
		}
		Some(found)
	}
	pub fn text_bytes(&self) -> usize {
		self.text.len()
	}
	pub fn memory_bytes(&self) -> usize {
		self.text.len()
			+ self.fields.len() * std::mem::size_of::<Field>()
			+ self
				.fields
				.iter()
				.map(|f| {
					f.enclosing.len() * std::mem::size_of::<u64>()
						+ f.boundaries.get().map_or(0, |b| {
							b.capacity() * std::mem::size_of::<usize>()
						})
				})
				.sum::<usize>()
	}
}
fn field_reading(text: markview_document::FieldText<'_>) -> String {
	match text {
		markview_document::FieldText::Rich(rich) => plain_text(rich),
		markview_document::FieldText::Code(code) => code.to_owned(),
	}
}

impl crate::layout::LayoutSnapshot {
	/// Visits only visible clusters, projecting reading geometry into semantic ranges.
	pub fn visit_search_clusters(
		&self,
		horizontal: &std::collections::HashMap<(usize, usize), f32>,
		visible: Range<f32>,
		mut visit: impl FnMut(
			markview_document::BlockId,
			FieldId,
			Range<usize>,
			crate::layout::Rect,
		),
	) {
		let first = self
			.blocks
			.partition_point(|b| b.y + b.height() < visible.start);
		for (bi, block) in self.blocks.iter().enumerate().skip(first) {
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
				// Image placeholders can put later clusters above earlier ones.
				for cluster in &node.clusters {
					let Some(rect) = self.text_rect(bi, &cluster, horizontal)
					else {
						continue;
					};
					if rect.y + rect.h < visible.start || rect.y > visible.end {
						continue;
					}
					let first = node
						.search_ranges
						.partition_point(|(_, r)| r.end <= cluster.range.start);
					for (semantic, reading) in
						node.search_ranges.iter().skip(first)
					{
						if reading.start >= cluster.range.end {
							break;
						}
						let range = if semantic.len() != reading.len() {
							semantic.clone()
						} else {
							semantic.start
								+ cluster.range.start.max(reading.start)
								- reading.start
								..semantic.start
									+ cluster.range.end.min(reading.end)
									- reading.start
						};
						visit(block.id, field, range, rect);
					}
				}
			}
		}
	}

	pub(crate) fn geometry_search_selection(
		&self,
		hit: &SearchMatch,
		revision: u64,
	) -> Option<crate::text::GeometrySelection> {
		use crate::text::{Affinity, GeometryPosition, GeometrySelection};
		if hit.revision != self.revision {
			return None;
		}
		let (bi, block) = self
			.blocks
			.iter()
			.enumerate()
			.find(|(_, block)| block.id == hit.block)?;
		let mut start = None;
		let mut end = None;
		for (ni, node) in block.layout.text.iter().enumerate() {
			if !block.text_visible(ni) {
				continue;
			}
			if node.search_field != Some(hit.field) {
				continue;
			}
			for (semantic, reading) in &node.search_ranges {
				let a = hit.range.start.max(semantic.start);
				let b = hit.range.end.min(semantic.end);
				if a >= b {
					continue;
				}
				let atomic = semantic.len() != reading.len();
				start.get_or_insert(GeometryPosition {
					revision,
					block: bi,
					node: ni,
					offset: if atomic {
						reading.start
					} else {
						reading.start + a - semantic.start
					},
					affinity: Affinity::Before,
				});
				end = Some(GeometryPosition {
					revision,
					block: bi,
					node: ni,
					offset: if atomic {
						reading.end
					} else {
						reading.start + b - semantic.start
					},
					affinity: Affinity::After,
				});
			}
		}
		Some(GeometrySelection {
			anchor: start?,
			focus: end?,
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	fn index(source: &str) -> SearchIndex {
		SearchIndex::new(&markview_commonmark::parse(Arc::from(source)))
	}
	#[test]
	fn semantic_fields_and_repeated_occurrences() {
		let i = index(
			"a**bc**[de](https://hidden.test)\n\nabcde\n\n| ab | cd |\n|---|---|\n| ab | cd |\n\n```\nabcde\n```\n",
		);
		let hits = i.find("abcde", SearchOptions::default());
		assert_eq!(hits.len(), 3);
		assert_ne!(hits[0].block, hits[1].block);
		assert!(i.find("hidden.test", SearchOptions::default()).is_empty());
		assert!(i.find("ab\tcd", SearchOptions::default()).is_empty());
	}
	#[test]
	fn unicode_literal_and_dictionary_boundaries() {
		let i = index("中文文字 Σσς KKk Cafe\u{301} café .* aaa");
		assert_eq!(i.find("σ", SearchOptions::default()).len(), 3);
		assert_eq!(i.find("k", SearchOptions::default()).len(), 3);
		let whole = SearchOptions {
			whole_word: true,
			..SearchOptions::default()
		};
		assert_eq!(i.find("中文", whole).len(), 1);
		assert!(i.find("中", whole).is_empty());
		assert!(i.find("Cafe", whole).is_empty());
		assert_eq!(i.find("Cafe\u{301}", whole).len(), 1);
		assert!(i.find("cafe", SearchOptions::default()).len() == 1);
		assert_eq!(i.find(".*", whole).len(), 1);
		assert_eq!(i.find("aa", SearchOptions::default()).len(), 1);
		assert!(i.find("", whole).is_empty());
		assert!(i.find_cancellable("x", whole, || true).is_none());
	}
	#[test]
	fn hidden_content_is_indexed_without_layout() {
		let i = index(
			"---\ntitle: secret\n---\n\n<details>\n<summary>outer</summary>\n\n<details>\n<summary>inner</summary>\n\nsecret\n\n</details>\n</details>\n",
		);
		let hits = i.find("secret", SearchOptions::default());
		assert_eq!(hits.len(), 2);
		assert_eq!(hits[0].enclosing.len(), 1);
		assert_eq!(hits[1].enclosing.len(), 2);
	}
}

#[cfg(test)]
mod proptests;
