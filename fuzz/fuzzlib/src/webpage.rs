//! Oracles for `markview_webpage`, the HTML-to-document lowering.
//!
//! Every check here reads the produced document and demands a property the
//! lowering must have, without restating its code: identities are unique,
//! addresses are absolute, a table's spans tile its grid, a field's text is
//! whitespace-normalised, and the title/source preamble is exactly where the
//! reader expects it. `fingerprint` is the differential's reading, so two
//! conversions of one page either agree field by field or fail.
use std::collections::{HashMap, HashSet};

use markview_document::{
	Block, BlockId, BlockKind, CellAlign, CellKind, Document, FieldText,
	Inline, InlineKind, LinkOccurrenceId, TableGroupKind, image, plain_text,
	visit_blocks, visit_fields,
};
use markview_webpage::{
	ArticleMetadata, PROFILE_V1, Profile, WebArticle, extract_with,
};

/// Bases a target picks from; each one exercises a different `Url::join`.
pub const BASES: &[&str] = &[
	"https://example.org/blog/post",
	"https://example.org/",
	"https://example.org/a/b/c.html?q=1&r=2",
	"http://127.0.0.1:8443/x/y",
	"https://example.org/%E4%B8%AD/x",
	"https://sub.example.org/deep/path/page.html",
];

/// The base for one input: a byte selects it, so a mutator can vary it.
pub fn base_for(data: &[u8]) -> &'static str {
	BASES[usize::from(data.first().copied().unwrap_or(0)) % BASES.len()]
}

/// A structural digest of everything one conversion produced.
pub struct Digest(u64);

impl Digest {
	fn new() -> Self {
		Self(0xcbf2_9ce4_8422_2325)
	}

	fn byte(&mut self, byte: u8) {
		self.0 ^= u64::from(byte);
		self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
	}

	fn bytes(&mut self, bytes: &[u8]) {
		self.u64(bytes.len() as u64);
		for byte in bytes {
			self.byte(*byte);
		}
	}

	fn str(&mut self, text: &str) {
		self.bytes(text.as_bytes());
	}

	fn u64(&mut self, value: u64) {
		for byte in value.to_le_bytes() {
			self.byte(byte);
		}
	}

	fn option(&mut self, value: Option<impl ToString>) {
		match value {
			Some(value) => {
				self.byte(1);
				self.str(&value.to_string());
			}
			None => self.byte(0),
		}
	}

	fn finish(&self) -> u64 {
		self.0
	}
}

fn digest_rich(digest: &mut Digest, text: &[Inline]) {
	digest.u64(text.len() as u64);
	for inline in text {
		match &inline.kind {
			InlineKind::Text(value) => {
				digest.byte(0);
				digest.str(value);
			}
			InlineKind::Anchor(value) => {
				digest.byte(1);
				digest.str(value);
			}
			InlineKind::Image(image) => {
				digest.byte(2);
				digest.str(image.address());
				digest.str(&image.alt);
				digest.str(&image.title);
				digest.u64(u64::from(image.width.unwrap_or(u32::MAX)));
				digest.u64(u64::from(image.height.unwrap_or(u32::MAX)));
				match &image.source {
					image::ImageSource::Resource(resource) => {
						digest.byte(0);
						digest.str(&resource.address);
						digest.byte(u8::from(
							resource.base != image::ResourceBase::Document,
						));
					}
					image::ImageSource::Diagram(diagram) => {
						digest.byte(1);
						digest.str(&diagram.language);
						digest.str(&diagram.source);
					}
				}
			}
			InlineKind::Math { latex, display } => {
				digest.byte(3);
				digest.str(latex);
				digest.byte(u8::from(*display));
			}
			InlineKind::LineBreak(kind) => {
				digest.byte(4);
				digest.byte(u8::from(
					*kind == markview_document::LineBreak::Hard,
				));
			}
			InlineKind::FootnoteRef(reference) => {
				digest.byte(5);
				digest.str(reference.definition.as_str());
				digest.u64(u64::from(reference.number));
				digest.u64(u64::from(reference.occurrence.get()));
			}
		}
		let style = &inline.style;
		digest.byte(
			u8::from(style.bold)
				| u8::from(style.italic) << 1
				| u8::from(style.strike) << 2
				| u8::from(style.mark) << 3
				| u8::from(style.code) << 4
				| u8::from(style.superscript) << 5
				| u8::from(style.subscript) << 6,
		);
		match &inline.link {
			Some(link) => {
				digest.byte(1);
				digest.str(link.as_str());
				digest.u64(u64::from(link.occurrence().get()));
			}
			None => digest.byte(0),
		}
	}
}

fn digest_block(digest: &mut Digest, block: &Block) {
	digest.u64(u64::from(block.id().get()));
	digest.u64(block.content_key().get());
	digest.u64(block.anchors().len() as u64);
	for anchor in block.anchors() {
		digest.str(anchor);
	}
	match block.kind() {
		BlockKind::Paragraph(text) => {
			digest.byte(0);
			digest_rich(digest, text);
		}
		BlockKind::Heading {
			level,
			text,
			anchor,
		} => {
			digest.byte(1);
			digest.byte(*level);
			digest.str(anchor);
			digest_rich(digest, text);
		}
		BlockKind::Code { language, text } => {
			digest.byte(2);
			digest.str(language);
			digest.str(text);
		}
		BlockKind::Rule => {
			digest.byte(3);
		}
		BlockKind::Quote { callout, blocks } => {
			digest.byte(4);
			digest.byte(u8::from(callout.is_some()));
			for child in blocks {
				digest_block(digest, child);
			}
		}
		BlockKind::Footnote { id, number, blocks } => {
			digest.byte(5);
			digest.str(id.as_str());
			digest.u64(u64::from(*number));
			for child in blocks {
				digest_block(digest, child);
			}
		}
		BlockKind::FrontMatter {
			default_open,
			blocks,
		} => {
			digest.byte(6);
			digest.byte(u8::from(*default_open));
			for child in blocks {
				digest_block(digest, child);
			}
		}
		BlockKind::Details {
			default_open,
			summary,
			blocks,
		} => {
			digest.byte(7);
			digest.byte(u8::from(*default_open));
			digest_rich(digest, summary);
			for child in blocks {
				digest_block(digest, child);
			}
		}
		BlockKind::Figure { blocks, caption } => {
			digest.byte(8);
			match caption {
				Some(caption) => {
					digest.byte(1);
					digest_rich(digest, caption);
				}
				None => digest.byte(0),
			}
			for child in blocks {
				digest_block(digest, child);
			}
		}
		BlockKind::List {
			start,
			reversed,
			tight,
			items,
		} => {
			digest.byte(9);
			digest.u64(start.map_or(i64::MIN, |v| v as i64) as u64);
			digest.byte(u8::from(*reversed));
			digest.byte(u8::from(*tight));
			for item in items {
				digest.byte(match item.checked {
					None => 0,
					Some(false) => 1,
					Some(true) => 2,
				});
				// The option is encoded separately, and the signed value keeps
				// its bits: offsetting the unsigned cast overflows on `-1`.
				match item.number {
					Some(number) => {
						digest.byte(1);
						digest.u64(number as i64 as u64);
					}
					None => digest.byte(0),
				}
				for child in &item.blocks {
					digest_block(digest, child);
				}
			}
		}
		BlockKind::Table(table) => {
			digest.byte(10);
			if let Some(caption) = &table.caption {
				digest.byte(1);
				digest_rich(digest, caption);
			} else {
				digest.byte(0);
			}
			for column in &table.columns {
				digest.byte(align_byte(column.align));
			}
			for group in &table.groups {
				digest.byte(match group.kind {
					TableGroupKind::Head => 0,
					TableGroupKind::Body => 1,
					TableGroupKind::Foot => 2,
				});
				for row in &group.rows {
					for cell in &row.cells {
						digest.byte(match cell.kind {
							CellKind::Header => 0,
							CellKind::Data => 1,
						});
						digest.u64(u64::from(cell.rowspan));
						digest.u64(u64::from(cell.colspan));
						digest.byte(align_byte(cell.align));
						for child in &cell.blocks {
							digest_block(digest, child);
						}
					}
				}
			}
		}
		BlockKind::DefinitionList(items) => {
			digest.byte(11);
			for item in items {
				for term in &item.terms {
					digest_rich(digest, term);
				}
				for definition in &item.definitions {
					for child in definition {
						digest_block(digest, child);
					}
				}
			}
		}
	}
}

fn align_byte(align: Option<CellAlign>) -> u8 {
	match align {
		None => 0,
		Some(CellAlign::Left) => 1,
		Some(CellAlign::Center) => 2,
		Some(CellAlign::Right) => 3,
	}
}

fn digest_metadata(digest: &mut Digest, metadata: &ArticleMetadata) {
	digest.str(&metadata.title);
	digest.option(metadata.byline.as_ref());
	digest.option(metadata.site_name.as_ref());
	digest.option(metadata.lang.as_ref());
	digest.option(metadata.dir.as_ref());
	digest.option(metadata.published_time.as_ref());
	digest.option(metadata.modified_time.as_ref());
	digest.option(metadata.excerpt.as_ref());
	digest.option(metadata.image.as_ref());
	digest.option(metadata.canonical_url.as_ref());
	digest.str(&metadata.final_url);
	digest.u64(metadata.diagnostics.len() as u64);
	for message in &metadata.diagnostics {
		digest.str(message);
	}
}

/// A digest of one conversion: metadata, input identity and every block.
pub fn fingerprint(article: &WebArticle) -> u64 {
	let mut digest = Digest::new();
	digest.u64(article.input_fingerprint);
	digest_metadata(&mut digest, &article.metadata);
	digest.u64(article.document.content_key().get());
	// The revision is a process-wide allocation, not a property of the page.
	for block in article.document.blocks() {
		digest_block(&mut digest, block);
	}
	digest.finish()
}

/// Fails when the article violates an invariant the reader relies on.
pub fn assert_article(article: &WebArticle) {
	assert_metadata(&article.metadata);
	assert_block_variants(&article.document);
	assert_preamble(article);
	assert_anchors(&article.document);
	assert_links(&article.document);
	assert_images(&article.document);
	assert_fields(&article.document);
	assert_tables(&article.document);
	assert_outline(&article.document);
	assert_index(&article.document);
	assert_diagnostics(article);
	assert_content(&article.document);
}

fn assert_metadata(metadata: &ArticleMetadata) {
	assert!(
		!metadata.title.is_empty(),
		"an extracted web page has no title"
	);
	assert_collapsed("title", &metadata.title);
	for (name, value) in [
		("byline", metadata.byline.as_deref()),
		("site name", metadata.site_name.as_deref()),
		("excerpt", metadata.excerpt.as_deref()),
	] {
		if let Some(value) = value {
			assert!(
				!value.is_empty(),
				"{name} is present but empty: {metadata:?}"
			);
			assert_collapsed(name, value);
		}
	}
	let url = url::Url::parse(&metadata.final_url).unwrap_or_else(|error| {
		panic!("final URL {:?} does not parse: {error}", metadata.final_url)
	});
	assert!(
		matches!(url.scheme(), "http" | "https"),
		"final URL is not HTTP(S): {url}"
	);
	for (name, value) in [
		("canonical URL", metadata.canonical_url.as_deref()),
		("lead image", metadata.image.as_deref()),
	] {
		if let Some(value) = value {
			assert!(
				!value.is_empty(),
				"{name} is present but empty: {metadata:?}"
			);
			assert!(
				url::Url::parse(value).is_ok(),
				"{name} is not an absolute URL: {value:?} (base {})",
				metadata.final_url
			);
		}
	}
	for message in &metadata.diagnostics {
		assert!(!message.trim().is_empty(), "an empty conversion diagnostic");
	}
}

/// A field the extraction normalised: no ASCII whitespace runs, no edges.
fn assert_collapsed(name: &str, text: &str) {
	// Only ASCII whitespace is collapsed; a non-breaking space is content
	// the extractor deliberately keeps, including at an edge.
	let trimmed = text.trim_matches(|c: char| c.is_ascii_whitespace());
	assert!(
		trimmed == text,
		"{name} has surrounding whitespace: {text:?}"
	);
	assert!(
		!text.contains("  "),
		"{name} has a collapsed-space run: {text:?}"
	);
	assert!(
		!text.contains(['\n', '\r', '\t']),
		"{name} carries raw whitespace: {text:?}"
	);
}

fn assert_block_variants(document: &Document) {
	for block in document.blocks() {
		visit_blocks(block, &mut |block| {
			match block.kind() {
				BlockKind::Footnote { .. } => {
					panic!("a web page lowered a footnote block")
				}
				BlockKind::FrontMatter { .. } => {
					panic!("a web page lowered front matter")
				}
				BlockKind::Quote {
					callout: Some(_), ..
				} => panic!("a web page lowered a callout"),
				_ => {}
			}
			for text in rich_fields(block) {
				for inline in text {
					if let InlineKind::FootnoteRef(reference) = &inline.kind {
						panic!(
							"a web page lowered footnote reference {}",
							reference.number
						);
					}
				}
			}
		});
	}
}

/// The rich fields a block owns directly, in reading order.
fn rich_fields(block: &Block) -> Vec<&[Inline]> {
	let single = match block.kind() {
		BlockKind::Paragraph(text) | BlockKind::Heading { text, .. } => {
			Some(text.as_slice())
		}
		BlockKind::Details { summary, .. } => Some(summary.as_slice()),
		BlockKind::Quote { callout, .. } => callout
			.as_ref()
			.and_then(|callout| callout.title.as_deref()),
		BlockKind::Table(table) => table.caption.as_deref(),
		BlockKind::Figure { caption, .. } => caption.as_deref(),
		BlockKind::DefinitionList(items) => {
			return items
				.iter()
				.flat_map(|item| item.terms.iter().map(Vec::as_slice))
				.collect();
		}
		BlockKind::Code { .. }
		| BlockKind::Rule
		| BlockKind::Footnote { .. }
		| BlockKind::FrontMatter { .. }
		| BlockKind::List { .. } => None,
	};
	single.into_iter().collect()
}

/// The synthetic title and source line sit in a fixed place.
fn assert_preamble(article: &WebArticle) {
	let blocks = article.document.blocks();
	let source = format!("Source: {}", article.metadata.final_url);
	let is_source = |block: &Block| matches!(block.kind(), BlockKind::Paragraph(text) if plain_text(text) == source);
	let title = |block: &Block| {
		matches!(block.kind(), BlockKind::Heading { level: 1, .. })
	};
	match blocks.first() {
		Some(block) if is_source(block) => {
			let heading = blocks.get(1).unwrap_or_else(|| {
				panic!("title heading missing after the source line")
			});
			assert!(
				title(heading),
				"the first body block is not the title heading: {heading:?}"
			);
		}
		Some(block) => {
			assert!(title(block), "the document does not start with a title");
			let line = blocks
				.get(1)
				.unwrap_or_else(|| panic!("the source line is missing"));
			assert!(
				is_source(line),
				"the second block is not the source line: {}",
				plain_text_of(line)
			);
		}
		None => panic!("an extracted web page has no blocks"),
	}
	// The source line's own link carries the page URL, and only once.
	let source_paragraph = blocks
		.iter()
		.find(|block| is_source(block))
		.expect("the source paragraph");
	let BlockKind::Paragraph(text) = source_paragraph.kind() else {
		unreachable!();
	};
	let links: Vec<_> = text
		.iter()
		.filter_map(|inline| {
			inline
				.link
				.as_ref()
				.map(|link| (link.as_str().to_owned(), link.occurrence()))
		})
		.collect();
	assert_eq!(
		links.len(),
		1,
		"the source line carries {} links",
		links.len()
	);
	assert_eq!(links[0].0, article.metadata.final_url);
	assert_eq!(
		links[0].1.get(),
		highest_occurrence(&article.document),
		"the source line's occurrence is not the last allocated one"
	);
}

/// The highest link occurrence in the document; the source line takes it.
fn highest_occurrence(document: &Document) -> u32 {
	let mut highest = 0;
	for block in document.blocks() {
		visit_fields(
			block,
			&mut Vec::new(),
			&|| false,
			&mut |_, _, field, _| {
				let FieldText::Rich(text) = field else {
					return;
				};
				for inline in text {
					if let Some(link) = &inline.link {
						highest = highest.max(link.occurrence().get());
					}
				}
			},
		);
	}
	highest
}

fn plain_text_of(block: &Block) -> String {
	match block.kind() {
		BlockKind::Paragraph(text) | BlockKind::Heading { text, .. } => {
			plain_text(text)
		}
		BlockKind::Code { text, .. } => text.clone(),
		other => format!("{other:?}"),
	}
}

/// Visits each block with the disclosure chain that encloses it.
fn walk_with_enclosing(
	block: &Block,
	enclosing: &mut Vec<BlockId>,
	visit: &mut impl FnMut(&Block, &[BlockId]),
) {
	visit(block, enclosing);
	let disclosure = matches!(
		block.kind(),
		BlockKind::Details { .. } | BlockKind::FrontMatter { .. }
	);
	if disclosure {
		enclosing.push(block.id());
	}
	for child in child_blocks(block) {
		walk_with_enclosing(child, enclosing, visit);
	}
	if disclosure {
		enclosing.pop();
	}
}

/// Child blocks in the reading order the document index uses.
fn child_blocks(block: &Block) -> Vec<&Block> {
	match block.kind() {
		BlockKind::Quote { blocks, .. }
		| BlockKind::Footnote { blocks, .. }
		| BlockKind::Details { blocks, .. }
		| BlockKind::FrontMatter { blocks, .. }
		| BlockKind::Figure { blocks, .. } => blocks.iter().collect(),
		BlockKind::List { items, .. } => {
			items.iter().flat_map(|item| item.blocks.iter()).collect()
		}
		BlockKind::Table(table) => table.cell_blocks().collect(),
		BlockKind::DefinitionList(items) => items
			.iter()
			.flat_map(|item| item.definitions.iter())
			.flat_map(|blocks| blocks.iter())
			.collect(),
		BlockKind::Paragraph(_)
		| BlockKind::Heading { .. }
		| BlockKind::Code { .. }
		| BlockKind::Rule => Vec::new(),
	}
}

fn assert_anchors(document: &Document) {
	let mut seen: HashSet<String> = HashSet::new();
	for root in document.blocks() {
		walk_with_enclosing(root, &mut Vec::new(), &mut |block, open| {
			// A heading with an explicit `id` may declare the same name as
			// its block anchor and as its own heading anchor, so one block's
			// declarations are collapsed before the global uniqueness check.
			let mut own: HashSet<String> =
				block.anchors().iter().cloned().collect();
			let heading = match block.kind() {
				BlockKind::Heading { anchor, .. } => {
					own.insert(anchor.clone());
					Some(anchor.clone())
				}
				_ => None,
			};
			for anchor in own {
				// An empty heading has an empty slug; Markdown produces the
				// same anchor for a heading with no letters or digits, so it
				// is the shared document model's edge, not a web defect.
				if heading.as_deref() != Some(anchor.as_str()) {
					assert!(
						!anchor.is_empty(),
						"a block declares an empty anchor: {block:?}"
					);
				}
				assert!(
					seen.insert(anchor.clone()),
					"anchor {anchor:?} is declared twice"
				);
				assert_eq!(
					document.fragment_enclosing(&anchor),
					open,
					"anchor {anchor:?} is filed under the wrong disclosures"
				);
			}
		});
		// One traversal of the subtree: `visit_fields` already descends into
		// cell, item and container fields.
		visit_fields(
			root,
			&mut Vec::new(),
			&|| false,
			&mut |_, _, field, open| {
				let FieldText::Rich(text) = field else {
					return;
				};
				for inline in text {
					let InlineKind::Anchor(anchor) = &inline.kind else {
						continue;
					};
					assert!(
						!anchor.is_empty(),
						"an inline declares an empty anchor"
					);
					assert!(
						seen.insert(anchor.clone()),
						"anchor {anchor:?} is declared twice"
					);
					assert_eq!(
						document.fragment_enclosing(anchor),
						open,
						"inline anchor {anchor:?} is filed under the wrong disclosures"
					);
				}
			},
		);
	}
}

fn assert_links(document: &Document) {
	let mut by_occurrence: HashMap<LinkOccurrenceId, String> = HashMap::new();
	for root in document.blocks() {
		visit_fields(
			root,
			&mut Vec::new(),
			&|| false,
			&mut |_, _, field, _| {
				let FieldText::Rich(text) = field else {
					return;
				};
				for inline in text {
					let Some(link) = &inline.link else {
						continue;
					};
					let url = link.as_str();
					assert!(
						!url.is_empty(),
						"an inline carries an empty link address"
					);
					// A fragment stays a fragment; anything else is an address
					// that either parses or kept the page's own scheme after a
					// failed `Url::join` (the documented fallback).
					if !url.starts_with('#') {
						let scheme =
							url.find(':').is_some_and(|at| at > 0 && at < 32);
						assert!(
							url::Url::parse(url).is_ok() || scheme,
							"link {url:?} is neither a fragment nor an absolute URL"
						);
					}
					let occurrence = link.occurrence();
					match by_occurrence.get(&occurrence) {
						Some(known) => assert_eq!(
							known, url,
							"occurrence {occurrence} is shared by {known:?} and {url:?}"
						),
						None => {
							by_occurrence.insert(occurrence, url.to_owned());
						}
					}
				}
			},
		);
	}
	// An occurrence may be allocated for an element that carries no text
	// (`<a href=x></a>`) and a block-level link may be split by a nested one,
	// so the ids need not be contiguous or once per field; what matters is
	// that no id is shared by two addresses and that the source line takes
	// the highest one (checked in `assert_preamble`).
}

/// Every resource an image draws must be an address the loader accepts.
fn assert_images(document: &Document) {
	for root in document.blocks() {
		visit_fields(
			root,
			&mut Vec::new(),
			&|| false,
			&mut |_, _, field, _| {
				let FieldText::Rich(text) = field else {
					return;
				};
				for inline in text {
					let InlineKind::Image(image) = &inline.kind else {
						continue;
					};
					match &image.source {
						image::ImageSource::Resource(resource) => {
							let address = &resource.address;
							assert!(
								!address.is_empty(),
								"an image carries an empty address"
							);
							assert!(
								url::Url::parse(address).is_ok(),
								"image {address:?} is not an absolute URL or a data URI"
							);
						}
						image::ImageSource::Diagram(diagram) => {
							assert!(
								!diagram.language.is_empty(),
								"a diagram carries no language"
							);
						}
					}
				}
			},
		);
	}
}

fn assert_fields(document: &Document) {
	for root in document.blocks() {
		visit_fields(
			root,
			&mut Vec::new(),
			&|| false,
			&mut |_, _, field, _| {
				let text = match field {
					FieldText::Rich(text) => text,
					FieldText::Code(text) => {
						assert!(
							!text.contains('\0'),
							"a code block carries a NUL byte"
						);
						return;
					}
				};
				let mut leading = true;
				for inline in text {
					if let InlineKind::Text(value) = &inline.kind {
						assert_no_raw_whitespace(value);
						if leading {
							assert!(
								!value.starts_with(' '),
								"a field starts with a space: {value:?}"
							);
							leading = false;
						}
					} else if !matches!(inline.kind, InlineKind::Anchor(_)) {
						leading = false;
					}
				}
				// The last non-anchor run is what the reader sees at the edge;
				// a blank one there means the trailing trim missed it.
				if let Some(Inline {
					kind: InlineKind::Text(value),
					..
				}) = text.iter().rev().find(|inline| {
					!matches!(inline.kind, InlineKind::Anchor(_))
				}) {
					assert!(
						!value.ends_with(' '),
						"a field ends with a space: {value:?}"
					);
				}
			},
		);
	}
}

fn assert_no_raw_whitespace(text: &str) {
	// Other control characters come from the page's own text. The vertical
	// tab is not ASCII whitespace in Rust or in HTML, so it survives too.
	for c in text.chars() {
		assert!(
			!matches!(c, '\n' | '\r' | '\t' | '\u{0c}' | '\0'),
			"a text run carries raw whitespace: {text:?}"
		);
	}
	assert!(
		!text.contains("  "),
		"a text run has a double space: {text:?}"
	);
}

/// One table cell's placement on the group's grid.
struct Placed {
	start: usize,
	colspan: usize,
	align: Option<CellAlign>,
}

fn assert_tables(document: &Document) {
	for root in document.blocks() {
		visit_blocks(root, &mut |block| {
			let BlockKind::Table(table) = block.kind() else {
				return;
			};
			let mut columns = 0usize;
			let mut group_kind = -1i8;
			// Alignment is resolved across every group, so a conflict in a
			// later group clears a column the earlier ones agreed on.
			let mut alignments: Vec<Option<Option<CellAlign>>> = Vec::new();
			for group in &table.groups {
				let kind = match group.kind {
					TableGroupKind::Head => 0,
					TableGroupKind::Body => 1,
					TableGroupKind::Foot => 2,
				};
				assert!(
					kind >= group_kind,
					"table groups are out of HTML order: {table:?}"
				);
				group_kind = kind;
				let rows = group.rows.len();
				let mut grid: Vec<Vec<bool>> = vec![Vec::new(); rows];
				let mut placed: Vec<Placed> = Vec::new();
				for (row_index, row) in group.rows.iter().enumerate() {
					let mut col = 0;
					for cell in &row.cells {
						let rowspan = cell.rowspan as usize;
						let colspan = cell.colspan as usize;
						assert!(rowspan >= 1, "a cell has rowspan 0");
						assert!(colspan >= 1, "a cell has colspan 0");
						assert!(
							row_index + rowspan <= rows,
							"a cell spans past its row group"
						);
						let start = first_free(
							&grid, rows, row_index, rowspan, colspan, col,
						)
						.unwrap_or_else(|| {
							panic!(
								"cell {row_index}/{col} in a {}x{rows} group has no free slot",
								table.columns.len()
							)
						});
						let width = start + colspan;
						for covering in grid
							.iter_mut()
							.take(row_index + rowspan)
							.skip(row_index)
						{
							if covering.len() < width {
								covering.resize(width, false);
							}
							for slot in
								covering.iter_mut().take(width).skip(start)
							{
								assert!(
									!*slot,
									"two cells overlap at {row_index}/{width}"
								);
								*slot = true;
							}
						}
						columns = columns.max(width);
						placed.push(Placed {
							start,
							colspan,
							align: cell.align,
						});
						col = width;
					}
				}
				// A row with no cells is legal: earlier rowspans may cover
				// all of its slots.
				if alignments.len() < columns {
					alignments.resize(columns, None);
				}
				for cell in &placed {
					let Some(align) = cell.align else {
						continue;
					};
					if alignments.len() < cell.start + cell.colspan {
						alignments.resize(cell.start + cell.colspan, None);
					}
					for resolved in alignments
						.iter_mut()
						.take(cell.start + cell.colspan)
						.skip(cell.start)
					{
						match resolved {
							Some(Some(known)) if *known != align => {
								*resolved = Some(None);
							}
							None => *resolved = Some(Some(align)),
							_ => {}
						}
					}
				}
			}
			for (column, resolved) in alignments.iter().enumerate() {
				// A column no cell aligned is still a column, unaligned.
				let expected = resolved.unwrap_or(None);
				assert_eq!(
					table.columns.get(column).map(|c| c.align),
					Some(expected),
					"column {column} alignment disagrees with its cells"
				);
			}
			assert_eq!(
				table.columns.len(),
				columns,
				"the column model does not cover the widest row"
			);
		});
	}
}

/// The first column at or after `col` where the span fits the grid.
fn first_free(
	grid: &[Vec<bool>],
	rows: usize,
	row: usize,
	rowspan: usize,
	colspan: usize,
	mut col: usize,
) -> Option<usize> {
	loop {
		if row + rowspan > rows {
			return None;
		}
		let fits = (row..row + rowspan).all(|r| {
			(col..col + colspan)
				.all(|c| !grid[r].get(c).copied().unwrap_or(false))
		});
		if fits {
			return Some(col);
		}
		col += 1;
	}
}

fn assert_outline(document: &Document) {
	let mut headings = Vec::new();
	for root in document.blocks() {
		visit_blocks(root, &mut |block| {
			if let BlockKind::Heading {
				level,
				text,
				anchor,
			} = block.kind()
			{
				headings.push((
					block.id(),
					*level,
					plain_text(text),
					anchor.clone(),
				));
			}
		});
	}
	let outline: Vec<_> = document
		.outline_entries()
		.iter()
		.map(|entry| {
			(
				entry.block,
				entry.level,
				entry.text.clone(),
				entry.anchor.clone(),
			)
		})
		.collect();
	assert_eq!(
		outline, headings,
		"the outline disagrees with the heading blocks"
	);
}

fn assert_content(document: &Document) {
	let has_visible = |block: &Block| -> bool {
		if let BlockKind::Code { text, .. } = block.kind() {
			return !text.trim().is_empty();
		}
		if let BlockKind::Table(table) = block.kind() {
			return table
				.caption
				.as_ref()
				.is_some_and(|caption| caption.iter().any(visible_inline))
				|| table.cell_blocks().any(code_or_field_visible);
		}
		if let BlockKind::Rule = block.kind() {
			return false;
		}
		code_or_field_visible(block)
	};
	// The preamble is the title (synthetic or the page's own) plus the source
	// line; content starts after the source line.
	let source = document.blocks().iter().position(|block| {
		matches!(block.kind(), BlockKind::Paragraph(text) if plain_text(text).starts_with("Source: "))
	});
	let body = document.blocks().iter().skip(source.map_or(0, |at| at + 1));
	let mut any = false;
	for block in body {
		walk_with_enclosing(block, &mut Vec::new(), &mut |block, _| {
			if has_visible(block) {
				any = true;
			}
		});
	}
	assert!(any, "an extracted article carries no readable content");
}

fn code_or_field_visible(block: &Block) -> bool {
	if matches!(block.kind(), BlockKind::Code { .. } | BlockKind::Rule) {
		return false;
	}
	rich_fields(block)
		.iter()
		.any(|text| text.iter().any(visible_inline))
}

fn visible_inline(inline: &Inline) -> bool {
	match &inline.kind {
		InlineKind::Text(text) => !text.trim().is_empty(),
		InlineKind::Image(_)
		| InlineKind::Math { .. }
		| InlineKind::FootnoteRef(_) => true,
		InlineKind::LineBreak(_) | InlineKind::Anchor(_) => false,
	}
}

/// Every block the sealed index names must resolve back to itself.
fn assert_index(document: &Document) {
	for root in document.blocks() {
		visit_blocks(root, &mut |block| {
			let id = block.id();
			let found = document
				.block(id)
				.unwrap_or_else(|| panic!("block {id} is not indexed"));
			assert_eq!(
				found.id(),
				id,
				"the index resolves {id} to another block"
			);
			let path = document
				.path_of(id)
				.unwrap_or_else(|| panic!("block {id} has no path"));
			let at_path = document
				.block_at_path(path)
				.unwrap_or_else(|| panic!("path {path:?} does not resolve"));
			assert_eq!(at_path.id(), id, "path {path:?} resolves elsewhere");
		});
	}
	for entry in document.outline_entries() {
		assert_eq!(
			document.block(entry.block).map(Block::id),
			Some(entry.block),
			"an outline entry names an unknown block"
		);
	}
}

/// A dropped duplicate id must still be declared once, and every message
/// must be about something the document actually contains.
fn assert_diagnostics(article: &WebArticle) {
	let mut anchors = HashSet::new();
	for root in article.document.blocks() {
		walk_with_enclosing(root, &mut Vec::new(), &mut |block, _| {
			anchors.extend(block.anchors().iter().cloned());
			if let BlockKind::Heading { anchor, .. } = block.kind() {
				anchors.insert(anchor.clone());
			}
			visit_fields(
				block,
				&mut Vec::new(),
				&|| false,
				&mut |_, _, field, _| {
					if let FieldText::Rich(text) = field {
						for inline in text {
							if let InlineKind::Anchor(anchor) = &inline.kind {
								anchors.insert(anchor.clone());
							}
						}
					}
				},
			);
		});
	}
	for message in &article.metadata.diagnostics {
		if let Some(id) = message.strip_prefix("duplicate HTML id: ") {
			assert!(
				anchors.contains(id),
				"a duplicate id {id:?} was reported but no element kept it"
			);
		}
	}
}

/// Extracts with a tighter budget, then with the shipped profile, and demands
/// that a tighter budget never turns a success into a failure.
///
/// The document itself may legally differ between profiles (a table clipped
/// to fewer columns is a different grid), so only the outcome is compared.
pub fn check_profiles(html: &str, url: &str, profile: &Profile) {
	if let Ok(tight) = extract_with(html, url, profile) {
		assert_article(&tight);
		let shipped =
			extract_with(html, url, &PROFILE_V1).unwrap_or_else(|error| {
				panic!(
					"a page extracted under tighter limits failed at the shipped ones: {error}"
				)
			});
		assert_article(&shipped);
	}
}

/// The article must fingerprint the exact bytes it was given.
pub fn assert_input_fingerprint(article: &WebArticle, html: &str) {
	assert_eq!(
		article.input_fingerprint,
		markview_document::stable_fingerprint(html.as_bytes()),
		"the input fingerprint is not a digest of the page"
	);
}

/// A budget profile scaled down from the shipped one by `shift` bits.
pub fn tight_profile(shift: u32) -> Profile {
	let scale = |value: usize| (value >> shift).max(1);
	Profile {
		max_input_bytes: scale(PROFILE_V1.max_input_bytes),
		max_elements: scale(PROFILE_V1.max_elements),
		max_dom_nodes: scale(PROFILE_V1.max_dom_nodes),
		max_semantic_depth: scale(PROFILE_V1.max_semantic_depth),
		max_text_bytes: scale(PROFILE_V1.max_text_bytes),
		max_table_rows: scale(PROFILE_V1.max_table_rows),
		max_table_columns: scale(PROFILE_V1.max_table_columns),
		max_table_cells: scale(PROFILE_V1.max_table_cells),
		max_blocks: scale(PROFILE_V1.max_blocks),
		preserved_classes: PROFILE_V1.preserved_classes,
	}
}

/// The conversion the differential targets compare against itself.
pub fn assert_deterministic(html: &str, url: &str, article: &WebArticle) {
	let again = extract_with(html, url, &PROFILE_V1)
		.expect("the same page failed on its second conversion");
	assert_eq!(
		fingerprint(article),
		fingerprint(&again),
		"two conversions of one page disagree"
	);
	assert_eq!(
		article.metadata.final_url, again.metadata.final_url,
		"two conversions of one page disagree on the final URL"
	);
}

/// The page's text in reading order, joined by newlines.
pub fn reading_text(document: &Document) -> String {
	let mut out = String::new();
	for root in document.blocks() {
		visit_fields(
			root,
			&mut Vec::new(),
			&|| false,
			&mut |_, _, field, _| match field {
				FieldText::Rich(text) => {
					if !out.is_empty() {
						out.push('\n');
					}
					out.push_str(&plain_text(text));
				}
				FieldText::Code(text) => {
					if !out.is_empty() {
						out.push('\n');
					}
					out.push_str(text);
				}
			},
		);
	}
	out
}

/// The reading length of one inline, as `plain_text` projects it.
pub fn inline_text_len(inline: &Inline) -> usize {
	markview_document::plain_text_len(std::slice::from_ref(inline))
}

/// Appends every field's reading text of `block` and its nested blocks.
pub fn collect_text(block: &Block, out: &mut String) {
	visit_fields(block, &mut Vec::new(), &|| false, &mut |_, _, field, _| {
		match field {
			FieldText::Rich(text) => out.push_str(&plain_text(text)),
			FieldText::Code(text) => out.push_str(text),
		}
	});
}
