//! Full-document literal search over semantic reading fields.
use crate::document::{Block, BlockKind, Document, RichText, plain_text};
use std::{
	ops::Range,
	sync::{Arc, OnceLock},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
/// A field occurrence within one top-level semantic block, including hidden fields.
pub struct SearchField(pub usize);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchOptions {
	pub case_sensitive: bool,
	pub whole_word: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchMatch {
	pub field: SearchField,
	pub range: Range<usize>,
	pub block: usize,
	pub enclosing: Arc<[u64]>,
}
struct Field {
	boundaries: OnceLock<Vec<usize>>,
	id: SearchField,
	range: Range<usize>,
	block: usize,
	enclosing: Arc<[u64]>,
}
pub struct SearchIndex {
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
		let empty: Arc<[u64]> = Arc::from([]);
		for (i, block) in document.blocks.iter().enumerate() {
			let mut ordinal = 0;
			visit_fields(
				block,
				&mut Vec::new(),
				&cancelled,
				&mut |text, enclosing| {
					let start = reading.len();
					reading.push_str(&text.reading());
					fields.push(Field {
						boundaries: OnceLock::new(),
						id: SearchField(ordinal),
						range: start..reading.len(),
						block: i,
						enclosing: if enclosing.is_empty() {
							empty.clone()
						} else {
							Arc::from(enclosing)
						},
					});
					ordinal += 1;
				},
			);
			if cancelled() {
				return None;
			}
		}
		Some(Self {
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
enum FieldText<'a> {
	Rich(&'a RichText),
	Code(&'a str),
}
impl FieldText<'_> {
	fn locator(&self) -> usize {
		match self {
			Self::Rich(t) => t.as_ptr() as usize,
			Self::Code(t) => t.as_ptr() as usize,
		}
	}
	fn reading(&self) -> String {
		match self {
			Self::Rich(t) => plain_text(t),
			Self::Code(t) => (*t).to_owned(),
		}
	}
}
/// The same complete semantic traversal binds indexing and cached layout fields.
fn visit_fields(
	block: &Block,
	enclosing: &mut Vec<u64>,
	cancelled: &impl Fn() -> bool,
	visit: &mut impl FnMut(FieldText<'_>, &[u64]),
) {
	if cancelled() {
		return;
	}
	match &block.kind {
		BlockKind::Paragraph(t) | BlockKind::Heading { text: t, .. } => {
			visit(FieldText::Rich(t), enclosing)
		}
		BlockKind::Code { text, .. } => visit(FieldText::Code(text), enclosing),
		BlockKind::Table { rows, .. } => {
			for row in rows {
				for cell in row {
					if cancelled() {
						return;
					}
					visit(FieldText::Rich(cell), enclosing);
				}
			}
		}
		BlockKind::Details {
			summary, blocks, ..
		} => {
			visit(FieldText::Rich(summary), enclosing);
			enclosing.push(block.id);
			for child in blocks {
				visit_fields(child, enclosing, cancelled, visit);
			}
			enclosing.pop();
		}
		BlockKind::FrontMatter { blocks, .. } => {
			enclosing.push(block.id);
			for child in blocks {
				visit_fields(child, enclosing, cancelled, visit);
			}
			enclosing.pop();
		}
		BlockKind::Quote { blocks, .. }
		| BlockKind::Footnote { blocks, .. } => {
			for child in blocks {
				visit_fields(child, enclosing, cancelled, visit);
			}
		}
		BlockKind::List { items, .. } => {
			for item in items {
				for child in &item.blocks {
					visit_fields(child, enclosing, cancelled, visit);
				}
			}
		}
		BlockKind::Rule => {}
	}
}
pub(crate) fn layout_fields(
	block: &Block,
) -> std::collections::HashMap<usize, SearchField> {
	let mut fields = std::collections::HashMap::new();
	let mut ordinal = 0;
	visit_fields(block, &mut Vec::new(), &|| false, &mut |text, _| {
		fields.insert(text.locator(), SearchField(ordinal));
		ordinal += 1;
	});
	fields
}

impl crate::layout::LayoutSnapshot {
	/// Visits only visible clusters, projecting reading geometry into semantic ranges.
	pub fn visit_search_clusters(
		&self,
		horizontal: &std::collections::HashMap<(usize, usize), f32>,
		visible: Range<f32>,
		mut visit: impl FnMut(usize, SearchField, Range<usize>, crate::layout::Rect),
	) {
		let first = self
			.blocks
			.partition_point(|b| b.y + b.layout.height < visible.start);
		for (bi, block) in self.blocks.iter().enumerate().skip(first) {
			if block.y > visible.end {
				break;
			}
			for node in &block.layout.text {
				let Some(field) = node.search_field else {
					continue;
				};
				// Image placeholders can put later clusters above earlier ones.
				for cluster in &node.clusters {
					if cluster.rect.y + cluster.rect.h + block.y < visible.start
						|| cluster.rect.y + block.y > visible.end
					{
						continue;
					}
					let Some(rect) = self.text_rect(bi, cluster, horizontal)
					else {
						continue;
					};
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
						visit(bi, field, range, rect);
					}
				}
			}
		}
	}

	pub fn search_selection(
		&self,
		hit: &SearchMatch,
		revision: u64,
	) -> Option<crate::text::TextSelection> {
		use crate::text::{Affinity, TextPosition, TextSelection};
		let block = self.blocks.get(hit.block)?;
		let mut start = None;
		let mut end = None;
		for (ni, node) in block.layout.text.iter().enumerate() {
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
				start.get_or_insert(TextPosition {
					revision,
					block: hit.block,
					node: ni,
					offset: if atomic {
						reading.start
					} else {
						reading.start + a - semantic.start
					},
					affinity: Affinity::Before,
				});
				end = Some(TextPosition {
					revision,
					block: hit.block,
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
		Some(TextSelection {
			anchor: start?,
			focus: end?,
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	fn index(source: &str) -> SearchIndex {
		SearchIndex::new(&crate::document::parse(Arc::from(source)))
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
