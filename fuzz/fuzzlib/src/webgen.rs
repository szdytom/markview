//! A generated article and the semantics the lowering must produce for it.
//!
//! The generator writes HTML from a byte stream and records what it emitted:
//! unique marker words in document order, each block's kind and text, a
//! table's spans, a link's address and the runs' styles. Comparing the
//! extracted document against that record is a differential with an
//! independent reading — the generator — rather than a restatement of the
//! lowering. Markers are unique, so the comparison cannot be satisfied by a
//! block that merely looks similar.
//!
//! The generator only writes shapes `dom_smoothie` preserves: it unwraps a
//! single-cell table (`fix_single_cell_tables`) and drops a layout-only
//! `<div>` wrapper, so those are not generated.
use std::collections::HashSet;

use markview_document::{
	Block, BlockKind, CellAlign, CellKind, Inline, InlineKind, TableGroupKind,
	TextStyle, plain_text,
};
use markview_webpage::WebArticle;

use crate::webpage;

/// One generated article together with its expected reading.
pub struct Generated {
	pub html: String,
	pub title: String,
	/// The base every relative address resolves against.
	pub base: String,
	/// Every marker, in the order the generator wrote it.
	pub markers: Vec<String>,
	pub blocks: Vec<Expected>,
}

/// What one generated block must become.
#[derive(Clone, Debug)]
pub enum Expected {
	Heading {
		level: u8,
		text: String,
		id: Option<String>,
	},
	Paragraph(Paragraph),
	List(List),
	Quote {
		blocks: Vec<Expected>,
	},
	Code {
		language: String,
		text: String,
	},
	Table(Table),
	Details {
		open: bool,
		summary: String,
		blocks: Vec<Expected>,
	},
	Figure {
		caption: String,
		alt: String,
	},
	DefinitionList(Vec<DefinitionItem>),
	/// A paragraph whose only visible run is one formula.
	Math {
		latex: String,
		display: bool,
	},
	/// A paragraph mixing prose and one image.
	ImageParagraph {
		text: String,
		alt: String,
		address: String,
	},
	Rule,
}

#[derive(Clone, Debug)]
pub struct List {
	pub ordered: bool,
	pub reversed: bool,
	pub start: Option<isize>,
	pub tight: bool,
	pub items: Vec<Item>,
}

#[derive(Clone, Debug)]
pub struct Item {
	pub checked: Option<bool>,
	pub blocks: Vec<Expected>,
}

#[derive(Clone, Debug)]
pub struct DefinitionItem {
	pub terms: Vec<String>,
	pub definitions: Vec<Vec<Expected>>,
}

#[derive(Clone, Debug)]
pub struct Paragraph {
	pub text: String,
	pub runs: Vec<Run>,
}

#[derive(Clone, Debug)]
pub struct Run {
	/// The run's exact text, already inside `Paragraph::text`.
	pub text: String,
	pub style: TextStyle,
	/// The raw `href` of the enclosing link, if the run is linked.
	pub href: Option<String>,
	/// The TeX behind an inline formula, when this run is math.
	pub math: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Table {
	pub caption: Option<String>,
	pub groups: Vec<Group>,
}

#[derive(Clone, Debug)]
pub struct Group {
	pub kind: TableGroupKind,
	pub rows: Vec<Row>,
}

#[derive(Clone, Debug)]
pub struct Row {
	pub cells: Vec<Cell>,
}

#[derive(Clone, Debug)]
pub struct Cell {
	pub text: String,
	pub kind: CellKind,
	pub rowspan: u32,
	pub colspan: u32,
	pub align: Option<CellAlign>,
}

const WORDS: &[&str] = &[
	"alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel",
	"india", "juliet",
];

const LINKS: &[&str] = &[
	"/relative/post",
	"#local-anchor",
	"https://example.net/elsewhere",
	"mailto:reader@example.org",
	"../up/one",
	"item?q=1#frag",
];

const CODE_LANGUAGES: &[&str] = &["rust", "python", "yaml", ""];

/// Padding keeps the article above Readability's length threshold.
const PADDING: &str = "The generated article keeps enough readable words in \
	its body for the extractor to accept the page as an article, so every \
	generated element below reaches the lowering instead of a rejection.";

struct Gen<'a> {
	data: &'a [u8],
	at: usize,
	/// The base every address resolves against, after `<base href>`.
	base: String,
	next_marker: usize,
	next_id: usize,
	html: String,
	markers: Vec<String>,
	blocks: Vec<Expected>,
}

impl<'a> Gen<'a> {
	fn new(data: &'a [u8], base: String) -> Self {
		Self {
			data,
			at: 0,
			base,
			next_marker: 0,
			next_id: 0,
			html: String::new(),
			markers: Vec::new(),
			blocks: Vec::new(),
		}
	}

	fn byte(&mut self) -> u8 {
		let byte = self
			.data
			.get(self.at % self.data.len().max(1))
			.copied()
			.unwrap_or(0);
		self.at += 1;
		byte
	}

	fn word(&mut self) -> &'static str {
		WORDS[usize::from(self.byte()) % WORDS.len()]
	}

	fn marker(&mut self) -> String {
		self.next_marker += 1;
		let marker = format!("mk{}", self.next_marker);
		self.markers.push(marker.clone());
		marker
	}

	/// A short unique sentence for a block or cell.
	fn sentence(&mut self) -> String {
		let marker = self.marker();
		let word = self.word();
		let other = self.word();
		format!("{marker} {word} {other}")
	}

	fn id(&mut self) -> String {
		self.next_id += 1;
		format!("anchor-{}", self.next_id)
	}

	/// The absolute address of one relative resource.
	fn address(&self, raw: &str) -> String {
		url::Url::parse(&self.base)
			.expect("a generated base")
			.join(raw)
			.expect("a generated resource path")
			.to_string()
	}

	/// Generates the blocks of one container, up to `depth` descendants.
	fn container(&mut self, depth: usize) -> Vec<Expected> {
		let count = 1 + usize::from(self.byte() % 3);
		let mut blocks = Vec::new();
		for _ in 0..count {
			let expected = self.block(depth);
			blocks.push(expected);
		}
		blocks
	}

	fn block(&mut self, depth: usize) -> Expected {
		let choice = if depth >= 2 {
			// A leaf block still exercises every inline shape, but never
			// opens a container: the generator's recursion must terminate.
			[0u8, 1, 4, 5, 7, 8, 9][usize::from(self.byte()) % 7]
		} else {
			self.byte() % 12
		};
		match choice {
			0 => self.heading(),
			1 => self.paragraph(),
			2 => self.list(depth),
			3 => self.quote(depth),
			4 => self.code(),
			5 => self.table(),
			6 => self.details(depth),
			7 => self.figure(),
			8 => self.math(),
			9 => self.definition_list(depth),
			10 => self.image_paragraph(),
			_ => self.rule(),
		}
	}

	fn runs(&mut self, count: usize) -> (Paragraph, String) {
		let mut runs = Vec::new();
		for _ in 0..count {
			let style = match self.byte() % 9 {
				0 => TextStyle {
					bold: true,
					..TextStyle::default()
				},
				1 => TextStyle {
					italic: true,
					..TextStyle::default()
				},
				2 => TextStyle {
					strike: true,
					..TextStyle::default()
				},
				3 => TextStyle {
					code: true,
					..TextStyle::default()
				},
				4 => TextStyle {
					superscript: true,
					..TextStyle::default()
				},
				5 => TextStyle {
					mark: true,
					..TextStyle::default()
				},
				_ => TextStyle::default(),
			};
			let href = (self.byte() % 5 == 0).then(|| {
				LINKS[usize::from(self.byte()) % LINKS.len()].to_owned()
			});
			// An inline formula is a run like any other: its TeX is the
			// paragraph's reading text at that position.
			// A formula inside `code` is literal text, so the generator only
			// asks for math on a non-code run.
			let math = (!style.code && self.byte() % 8 == 0)
				.then(|| format!("{} + {}", self.marker(), self.word()));
			let text = match &math {
				Some(latex) => latex.clone(),
				None => format!("{} {}", self.marker(), self.word()),
			};
			runs.push(Run {
				text,
				style,
				href,
				math,
			});
		}
		let mut html = String::new();
		let mut text = String::new();
		for (index, run) in runs.iter().enumerate() {
			if index > 0 {
				html.push(' ');
				text.push(' ');
			}
			let tag = match run.style {
				TextStyle { bold: true, .. } => Some("b"),
				TextStyle { italic: true, .. } => Some("i"),
				TextStyle { strike: true, .. } => Some("s"),
				TextStyle { code: true, .. } => Some("code"),
				TextStyle {
					superscript: true, ..
				} => Some("sup"),
				TextStyle { mark: true, .. } => Some("mark"),
				_ => None,
			};
			if let Some(href) = &run.href {
				html.push_str(&format!("<a href=\"{href}\">"));
			}
			if let Some(tag) = tag {
				html.push_str(&format!("<{tag}>"));
			}
			match &run.math {
				Some(latex) => html.push_str(&format!(
					"<span class=\"math inline\">\\({latex}\\)</span>"
				)),
				None => html.push_str(&run.text),
			}
			if let Some(tag) = tag {
				html.push_str(&format!("</{tag}>"));
			}
			if run.href.is_some() {
				html.push_str("</a>");
			}
			text.push_str(&run.text);
		}
		(Paragraph { text, runs }, html)
	}

	/// Writes a paragraph and returns it.
	fn paragraph_html(&mut self, count: usize) -> Paragraph {
		let (paragraph, html) = self.runs(count);
		self.html.push_str(&format!("<p>{html}</p>"));
		paragraph
	}

	fn heading(&mut self) -> Expected {
		let level = 2 + self.byte() % 3;
		let text = self.sentence();
		let id = (self.byte() % 3 == 0).then(|| self.id());
		let attr = match &id {
			Some(id) => format!(" id=\"{id}\""),
			None => String::new(),
		};
		self.html
			.push_str(&format!("<h{level}{attr}>{text}</h{level}>"));
		Expected::Heading { level, text, id }
	}

	fn paragraph(&mut self) -> Expected {
		let count = 1 + usize::from(self.byte()) % 3;
		Expected::Paragraph(self.paragraph_html(count))
	}

	fn list(&mut self, depth: usize) -> Expected {
		let ordered = self.byte() % 2 == 0;
		let reversed = ordered && self.byte() % 3 == 0;
		let start = (ordered && self.byte() % 3 == 0)
			.then(|| 1 + isize::from(self.byte() % 5));
		let loose = self.byte() % 3 == 0;
		let count = 1 + usize::from(self.byte()) % 3;
		let tag = if ordered { "ol" } else { "ul" };
		let reversed_attr = if reversed { " reversed" } else { "" };
		match start {
			Some(start) => self
				.html
				.push_str(&format!("<{tag} start=\"{start}\"{reversed_attr}>")),
			None => self.html.push_str(&format!("<{tag}{reversed_attr}>")),
		}
		let mut items = Vec::new();
		for _ in 0..count {
			self.html.push_str("<li>");
			let checked = (self.byte() % 4 == 0).then(|| self.byte() % 2 == 0);
			if let Some(checked) = checked {
				self.html.push_str(&format!(
					"<input type=\"checkbox\"{}>",
					if checked { " checked" } else { "" }
				));
			}
			let runs = 1 + usize::from(self.byte()) % 2;
			let (paragraph, html) = self.runs(runs);
			if loose {
				self.html.push_str(&format!("<p>{html}</p>"));
			} else {
				self.html.push_str(&html);
			}
			let mut blocks = vec![Expected::Paragraph(paragraph)];
			if depth < 2 && self.byte() % 3 == 0 {
				let nested = self.list(depth + 1);
				blocks.push(nested);
			}
			self.html.push_str("</li>");
			items.push(Item { checked, blocks });
		}
		self.html.push_str(&format!("</{tag}>"));
		// A reversed list without a start counts down from its item count.
		let start = if reversed {
			Some(start.unwrap_or(items.len() as isize))
		} else {
			start
		};
		Expected::List(List {
			ordered,
			reversed,
			start,
			tight: !loose,
			items,
		})
	}

	fn quote(&mut self, depth: usize) -> Expected {
		self.html.push_str("<blockquote>");
		let blocks = self.container(depth + 1);
		self.html.push_str("</blockquote>");
		Expected::Quote { blocks }
	}

	fn code(&mut self) -> Expected {
		let language = CODE_LANGUAGES
			[usize::from(self.byte()) % CODE_LANGUAGES.len()]
		.to_owned();
		let text = format!("{} = {};", self.marker(), self.word());
		let class = if language.is_empty() {
			String::new()
		} else {
			format!(" class=\"language-{language}\"")
		};
		self.html
			.push_str(&format!("<pre><code{class}>{text}</code></pre>"));
		Expected::Code { language, text }
	}

	fn math(&mut self) -> Expected {
		let display = self.byte() % 3 == 0;
		let latex = format!("{} + {}", self.marker(), self.word());
		if display {
			self.html.push_str(&format!(
				"<div class=\"math display\">\\[{latex}\\]</div>"
			));
		} else {
			self.html.push_str(&format!(
				"<p><span class=\"math inline\">\\({latex}\\)</span></p>"
			));
		}
		Expected::Math { latex, display }
	}

	fn details(&mut self, depth: usize) -> Expected {
		let open = self.byte() % 2 == 0;
		let summary = self.sentence();
		self.html.push_str(&format!(
			"<details{}><summary>{summary}</summary>",
			if open { " open" } else { "" }
		));
		let blocks = self.container(depth + 1);
		self.html.push_str("</details>");
		Expected::Details {
			open,
			summary,
			blocks,
		}
	}

	fn figure(&mut self) -> Expected {
		// The figure's content is read before its caption, and the image's
		// alt text is part of that content.
		let marker = self.marker();
		let alt = format!("alt {marker}");
		let caption = self.sentence();
		self.html.push_str(&format!(
			"<figure><img src=\"plot.png\" alt=\"{alt}\"><figcaption>{caption}</figcaption></figure>"
		));
		Expected::Figure { caption, alt }
	}

	fn definition_list(&mut self, _depth: usize) -> Expected {
		let count = 1 + usize::from(self.byte()) % 3;
		self.html.push_str("<dl>");
		let mut items = Vec::new();
		for _ in 0..count {
			let terms = 1 + usize::from(self.byte()) % 2;
			let mut texts = Vec::new();
			for _ in 0..terms {
				let text = self.sentence();
				self.html.push_str(&format!("<dt>{text}</dt>"));
				texts.push(text);
			}
			let definitions = 1 + usize::from(self.byte()) % 2;
			let mut bodies = Vec::new();
			for _ in 0..definitions {
				let runs = 1 + usize::from(self.byte()) % 2;
				let (paragraph, html) = self.runs(runs);
				self.html.push_str(&format!("<dd>{html}</dd>"));
				bodies.push(vec![Expected::Paragraph(paragraph)]);
			}
			items.push(DefinitionItem {
				terms: texts,
				definitions: bodies,
			});
		}
		self.html.push_str("</dl>");
		Expected::DefinitionList(items)
	}

	fn image_paragraph(&mut self) -> Expected {
		let before = format!("{} {}", self.marker(), self.word());
		let alt_marker = self.marker();
		let alt = format!("alt {alt_marker}");
		let after = format!("{} {}", self.marker(), self.word());
		let address = self.address("inline.png");
		let text = format!("{before} {alt} {after}");
		self.html.push_str(&format!(
			"<p>{before} <img src=\"inline.png\" alt=\"{alt}\"> {after}</p>"
		));
		Expected::ImageParagraph { text, alt, address }
	}

	fn rule(&mut self) -> Expected {
		self.html.push_str("<hr>");
		Expected::Rule
	}

	fn table(&mut self) -> Expected {
		let with_caption = self.byte() % 3 == 0;
		let head = usize::from(self.byte() % 2);
		// `dom_smoothie` unwraps a single-cell table, so the grid needs at
		// least two rows.
		let body = 2 + usize::from(self.byte() % 2);
		let foot = usize::from(self.byte() % 3 == 0);
		let columns = 2 + usize::from(self.byte() % 3);
		let mut groups_html = String::new();
		let mut groups = Vec::new();
		for (kind, rows) in [
			(TableGroupKind::Head, head),
			(TableGroupKind::Body, body),
			(TableGroupKind::Foot, foot),
		] {
			if rows == 0 {
				continue;
			}
			let tag = match kind {
				TableGroupKind::Head => "thead",
				TableGroupKind::Body => "tbody",
				TableGroupKind::Foot => "tfoot",
			};
			groups_html.push_str(&format!("<{tag}>"));
			let mut group = Group {
				kind,
				rows: Vec::new(),
			};
			// The generator places cells on its own grid, so a span it writes
			// is a claim the lowering must reproduce.
			let mut occupied = vec![vec![false; columns]; rows];
			for row in 0..rows {
				let mut cells = Vec::new();
				groups_html.push_str("<tr>");
				let mut column = 0;
				while column < columns {
					if occupied[row][column] {
						column += 1;
						continue;
					}
					let colspan = 1 + usize::from(self.byte() % 2);
					let rowspan = 1 + usize::from(self.byte() % 2);
					// Clip the span to the free box at this position.
					let mut colspan = colspan.min(columns - column);
					while colspan > 1
						&& (column..column + colspan).any(|c| occupied[row][c])
					{
						colspan -= 1;
					}
					let mut rowspan = rowspan.min(rows - row);
					while rowspan > 1
						&& (row..row + rowspan).any(|r| {
							(column..column + colspan).any(|c| occupied[r][c])
						}) {
						rowspan -= 1;
					}
					for slots in
						occupied.iter_mut().take(row + rowspan).skip(row)
					{
						for slot in
							slots.iter_mut().take(column + colspan).skip(column)
						{
							*slot = true;
						}
					}
					let header = kind == TableGroupKind::Head;
					let tag = if header { "th" } else { "td" };
					let text = self.sentence();
					let align = match self.byte() % 4 {
						0 => Some(CellAlign::Left),
						1 => Some(CellAlign::Center),
						2 => Some(CellAlign::Right),
						_ => None,
					};
					let align_attr = match align {
						Some(CellAlign::Left) => " align=\"left\"",
						Some(CellAlign::Center) => " align=\"center\"",
						Some(CellAlign::Right) => " align=\"right\"",
						None => "",
					};
					let mut span_attr = String::new();
					if colspan > 1 {
						span_attr.push_str(&format!(" colspan=\"{colspan}\""));
					}
					if rowspan > 1 {
						span_attr.push_str(&format!(" rowspan=\"{rowspan}\""));
					}
					groups_html.push_str(&format!(
						"<{tag}{align_attr}{span_attr}>{text}</{tag}>"
					));
					cells.push(Cell {
						text,
						kind: if header {
							CellKind::Header
						} else {
							CellKind::Data
						},
						rowspan: rowspan as u32,
						colspan: colspan as u32,
						align,
					});
					column += colspan;
				}
				groups_html.push_str("</tr>");
				group.rows.push(Row { cells });
			}
			groups_html.push_str(&format!("</{tag}>"));
			groups.push(group);
		}
		// The caption is generated after the grid because the lowering reads
		// it last, as a footer line; the markup itself keeps it first.
		let caption = with_caption.then(|| self.sentence());
		match &caption {
			Some(caption) => self.html.push_str(&format!(
				"<table><caption>{caption}</caption>{groups_html}</table>"
			)),
			None => {
				self.html.push_str(&format!("<table>{groups_html}</table>"))
			}
		}
		Expected::Table(Table { caption, groups })
	}

	fn push(&mut self, expected: Expected) {
		self.blocks.push(expected);
	}
}

/// Generates one article from `data`.
///
/// `data` is the fuzzer's unit; the same bytes always produce the same page,
/// so a failing input is reproducible.
pub fn generate(data: &[u8]) -> Generated {
	// A page-level `<base>` is left to the `webpage` target's mutator: the
	// extractor resolves fragment links against it, which would make this
	// differential test upstream policy instead of the lowering.
	let base = webpage::base_for(data).to_owned();
	let mut writer = Gen::new(data, base.clone());
	let title = format!("Generated article {}", writer.marker());
	let count = 1 + usize::from(writer.byte()) % 5;
	for _ in 0..count {
		let expected = writer.block(0);
		writer.push(expected);
	}
	let body = std::mem::take(&mut writer.html);
	// Readability builds the article from its best-scoring candidate, so a
	// short body can lose a sibling to candidate selection. Two prose
	// paragraphs keep the `<article>` itself the top candidate.
	let html = format!(
		"<!doctype html><html lang=\"en\"><head><title>{title}</title></head>\
		 <body><article><h1>{title}</h1><p>{PADDING}</p>{body}\
		 <p>{PADDING}</p></article></body></html>\n"
	);
	Generated {
		html,
		title,
		base,
		markers: writer.markers,
		blocks: writer.blocks,
	}
}

/// Checks the extracted article against what the generator wrote.
pub fn assert_generated(article: &WebArticle, generated: &Generated) {
	assert_eq!(
		article.metadata.title, generated.title,
		"the generated title was not the article's title"
	);
	assert_eq!(
		found_markers(&webpage::reading_text(&article.document)),
		generated.markers,
		"the reading text lost, repeated or reordered a marker"
	);
	let blocks = body_blocks(article);
	let mut cursor = 0;
	for expected in &generated.blocks {
		let found = blocks[cursor..]
			.iter()
			.position(|block| matches_loosely(block, expected))
			.unwrap_or_else(|| {
				panic!(
					"generated block {expected:?} is missing between {:?}",
					blocks[cursor..]
						.iter()
						.map(|block| format!("{:?}", block.kind()))
						.collect::<Vec<_>>()
				)
			});
		let block = blocks[cursor + found];
		match_block(block, expected, &generated.base);
		cursor += found + 1;
	}
}

/// The blocks after the synthetic title and source line.
fn body_blocks(article: &WebArticle) -> Vec<&Block> {
	let mut blocks: Vec<&Block> = article.document.blocks().iter().collect();
	let source = blocks.first().is_some_and(|block| {
		matches!(block.kind(), BlockKind::Paragraph(text) if plain_text(text).starts_with("Source: "))
	});
	if source {
		blocks.remove(0);
	} else {
		blocks.remove(0);
		blocks.remove(0);
	}
	// In the title-heading case the page's own `<h1>` is the next block.
	if blocks.first().is_some_and(|block| {
		matches!(block.kind(), BlockKind::Heading { level: 1, .. })
	}) {
		blocks.remove(0);
	}
	blocks
}

fn found_markers(text: &str) -> Vec<String> {
	let mut markers = Vec::new();
	let bytes = text.as_bytes();
	let mut at = 0;
	while at + 2 <= bytes.len() {
		if &bytes[at..at + 2] == b"mk" {
			let mut end = at + 2;
			while end < bytes.len() && bytes[end].is_ascii_digit() {
				end += 1;
			}
			if end > at + 2
				&& (end == bytes.len() || !bytes[end].is_ascii_alphanumeric())
			{
				markers.push(text[at..end].to_owned());
				at = end;
				continue;
			}
		}
		at += 1;
	}
	markers
}

/// Whether a block could be the one `expected` describes: the right kind
/// with the right markers.
fn matches_loosely(block: &Block, expected: &Expected) -> bool {
	let kind_matches = matches!(
		(block.kind(), expected),
		(BlockKind::Heading { .. }, Expected::Heading { .. })
			| (BlockKind::Paragraph(_), Expected::Paragraph(_))
			| (BlockKind::Paragraph(_), Expected::Math { .. })
			| (BlockKind::Paragraph(_), Expected::ImageParagraph { .. })
			| (BlockKind::List { .. }, Expected::List(_))
			| (BlockKind::Quote { .. }, Expected::Quote { .. })
			| (BlockKind::Code { .. }, Expected::Code { .. })
			| (BlockKind::Table(_), Expected::Table(_))
			| (BlockKind::Details { .. }, Expected::Details { .. })
			| (BlockKind::Figure { .. }, Expected::Figure { .. })
			| (BlockKind::DefinitionList(_), Expected::DefinitionList(_))
			| (BlockKind::Rule, Expected::Rule)
	);
	if !kind_matches {
		return false;
	}
	let mut wanted = Vec::new();
	expected_markers(expected, &mut wanted);
	let mut text = String::new();
	webpage::collect_text(block, &mut text);
	wanted.iter().all(|marker| text.contains(marker.as_str()))
}

/// Every marker the expected subtree declares.
fn expected_markers(expected: &Expected, out: &mut Vec<String>) {
	let text = |text: &str, out: &mut Vec<String>| {
		out.extend(found_markers(text));
	};
	match expected {
		Expected::Heading { text: value, .. }
		| Expected::Code { text: value, .. } => {
			text(value, out);
		}
		Expected::Paragraph(paragraph) => text(&paragraph.text, out),
		Expected::Math { latex, .. } => text(latex, out),
		Expected::ImageParagraph {
			text: value, alt, ..
		} => {
			text(value, out);
			text(alt, out);
		}
		Expected::List(list) => {
			for item in &list.items {
				for block in &item.blocks {
					expected_markers(block, out);
				}
			}
		}
		Expected::Quote { blocks } | Expected::Details { blocks, .. } => {
			for block in blocks {
				expected_markers(block, out);
			}
		}
		Expected::Table(table) => {
			if let Some(caption) = &table.caption {
				text(caption, out);
			}
			for group in &table.groups {
				for row in &group.rows {
					for cell in &row.cells {
						text(&cell.text, out);
					}
				}
			}
		}
		Expected::Figure { caption, alt } => {
			text(caption, out);
			text(alt, out);
		}
		Expected::DefinitionList(items) => {
			for item in items {
				for term in &item.terms {
					text(term, out);
				}
				for definition in &item.definitions {
					for block in definition {
						expected_markers(block, out);
					}
				}
			}
		}
		Expected::Rule => {}
	}
}

fn match_block(block: &Block, expected: &Expected, base: &str) {
	match expected {
		Expected::Heading { level, text, id } => {
			let BlockKind::Heading {
				level: found_level,
				text: found,
				anchor,
			} = block.kind()
			else {
				panic!("expected a heading, got {:?}", block.kind());
			};
			assert_eq!(*found_level, *level, "wrong heading level");
			assert_eq!(plain_text(found), *text, "wrong heading text");
			if let Some(id) = id {
				assert_eq!(anchor, id, "an explicit heading id was lost");
			}
		}
		Expected::Paragraph(paragraph) => {
			let BlockKind::Paragraph(found) = block.kind() else {
				panic!("expected a paragraph, got {:?}", block.kind());
			};
			assert_eq!(
				plain_text(found),
				paragraph.text,
				"the paragraph text changed"
			);
			assert_runs(found, paragraph, base);
		}
		Expected::Math { latex, display } => {
			let BlockKind::Paragraph(found) = block.kind() else {
				panic!("expected a formula paragraph, got {:?}", block.kind());
			};
			let math: Vec<_> = found
				.iter()
				.filter_map(|inline| match &inline.kind {
					InlineKind::Math { latex, display } => {
						Some((latex.as_str(), *display))
					}
					_ => None,
				})
				.collect();
			assert_eq!(
				math,
				[(latex.as_str(), *display)],
				"a formula changed shape"
			);
		}
		Expected::ImageParagraph { text, alt, address } => {
			let BlockKind::Paragraph(found) = block.kind() else {
				panic!("expected an image paragraph, got {:?}", block.kind());
			};
			assert_eq!(plain_text(found), *text, "an image paragraph changed");
			let images: Vec<_> = found
				.iter()
				.filter_map(|inline| match &inline.kind {
					InlineKind::Image(image) => Some(image),
					_ => None,
				})
				.collect();
			assert_eq!(images.len(), 1, "an inline image was lost or repeated");
			assert_eq!(&images[0].alt, alt, "an inline image's alt changed");
			assert_eq!(
				images[0].address(),
				address,
				"an inline image's address changed"
			);
		}
		Expected::List(list) => match_list(block, list, base),
		Expected::Quote { blocks } => {
			let BlockKind::Quote { blocks: found, .. } = block.kind() else {
				panic!("expected a quote, got {:?}", block.kind());
			};
			match_blocks(found, blocks, base);
		}
		Expected::Code { language, text } => {
			let BlockKind::Code {
				language: found_language,
				text: found_text,
			} = block.kind()
			else {
				panic!("expected a code block, got {:?}", block.kind());
			};
			assert_eq!(found_language, language, "a code language changed");
			assert_eq!(found_text, text, "a code block's text changed");
		}
		Expected::Table(table) => match_table(block, table),
		Expected::Details {
			open,
			summary,
			blocks,
		} => {
			let BlockKind::Details {
				default_open,
				summary: found_summary,
				blocks: found,
			} = block.kind()
			else {
				panic!("expected a details block, got {:?}", block.kind());
			};
			assert_eq!(default_open, open, "a disclosure's state changed");
			assert_eq!(plain_text(found_summary), *summary);
			match_blocks(found, blocks, base);
		}
		Expected::Figure {
			caption: expected_caption,
			alt,
		} => {
			let BlockKind::Figure { blocks, caption } = block.kind() else {
				panic!("expected a figure, got {:?}", block.kind());
			};
			let found_caption = caption
				.as_ref()
				.map(|caption| plain_text(caption))
				.unwrap_or_default();
			assert_eq!(
				&found_caption, expected_caption,
				"a figure caption changed"
			);
			let mut found_alt = None;
			for child in blocks {
				let mut images = Vec::new();
				child.images(&mut images);
				if let Some(image) = images.first() {
					found_alt = Some(image.alt.clone());
				}
			}
			assert_eq!(
				found_alt.as_deref(),
				Some(alt.as_str()),
				"a figure's image changed"
			);
		}
		Expected::DefinitionList(items) => {
			let BlockKind::DefinitionList(found) = block.kind() else {
				panic!("expected a definition list, got {:?}", block.kind());
			};
			assert_eq!(found.len(), items.len(), "a definition item was lost");
			for (found, expected) in found.iter().zip(items) {
				let terms: Vec<_> =
					found.terms.iter().map(|term| plain_text(term)).collect();
				assert_eq!(terms, expected.terms, "a term changed");
				assert_eq!(
					found.definitions.len(),
					expected.definitions.len(),
					"a definition was lost"
				);
				for (found, expected) in
					found.definitions.iter().zip(&expected.definitions)
				{
					match_blocks(found, expected, base);
				}
			}
		}
		Expected::Rule => assert!(
			matches!(block.kind(), BlockKind::Rule),
			"a rule changed into {:?}",
			block.kind()
		),
	}
}

fn match_blocks(found: &[Block], expected: &[Expected], base: &str) {
	assert_eq!(
		found.len(),
		expected.len(),
		"a container gained or lost blocks: found {:?}",
		found.iter().map(|block| block.kind()).collect::<Vec<_>>()
	);
	for (block, expected) in found.iter().zip(expected) {
		match_block(block, expected, base);
	}
}

fn match_list(block: &Block, list: &List, base: &str) {
	let BlockKind::List {
		start,
		reversed,
		tight,
		items,
	} = block.kind()
	else {
		panic!("expected a list, got {:?}", block.kind());
	};
	assert_eq!(*reversed, list.reversed, "a list's direction changed");
	if !list.ordered {
		assert_eq!(start, &None, "an unordered list gained a start");
	} else if let Some(expected) = list.start {
		assert_eq!(*start, Some(expected), "wrong list start");
	} else {
		assert_eq!(*start, Some(1), "an ordered list lost its start");
	}
	assert_eq!(*tight, list.tight, "a list's tightness changed");
	assert_eq!(items.len(), list.items.len(), "wrong list item count");
	for (item, expected) in items.iter().zip(&list.items) {
		assert_eq!(item.checked, expected.checked, "a checklist state changed");
		match_blocks(&item.blocks, &expected.blocks, base);
	}
}

fn assert_runs(found: &[Inline], paragraph: &Paragraph, base: &str) {
	let text = plain_text(found);
	for run in &paragraph.runs {
		let start = text
			.find(&run.text)
			.unwrap_or_else(|| panic!("run {:?} is missing", run.text));
		let mut at = 0;
		let inline = found.iter().find(|inline| {
			let len = webpage::inline_text_len(inline);
			let inside = at <= start && start < at + len;
			at += len;
			inside
		});
		let Some(inline) = inline else {
			panic!("run {:?} is not inside an inline", run.text);
		};
		assert_eq!(
			inline.style, run.style,
			"run {:?} lost its style ({:?})",
			run.text, inline.style
		);
		if let Some(latex) = &run.math {
			assert_eq!(
				inline.kind,
				InlineKind::Math {
					latex: latex.clone(),
					display: false,
				},
				"run {:?} lost its formula",
				run.text
			);
		}
		match &run.href {
			Some(href) => {
				let expected = if href.starts_with('#') {
					href.clone()
				} else {
					url::Url::parse(base)
						.expect("a generated base")
						.join(href)
						.expect("a generated href")
						.to_string()
				};
				assert_eq!(
					inline.link_url(),
					Some(expected.as_str()),
					"run {:?} lost its link",
					run.text
				);
			}
			None => assert_eq!(
				inline.link_url(),
				None,
				"run {:?} gained a link",
				run.text
			),
		}
	}
}

fn match_table(block: &Block, expected: &Table) {
	let BlockKind::Table(found) = block.kind() else {
		panic!("expected a table, got {:?}", block.kind());
	};
	assert_eq!(
		found.caption.as_ref().map(|caption| plain_text(caption)),
		expected.caption,
		"a table caption changed"
	);
	assert_eq!(
		found.groups.len(),
		expected.groups.len(),
		"a table group was dropped"
	);
	for (found, expected) in found.groups.iter().zip(&expected.groups) {
		assert_eq!(found.kind, expected.kind, "a table group moved");
		assert_eq!(
			found.rows.len(),
			expected.rows.len(),
			"a table row was dropped"
		);
		for (row, expected) in found.rows.iter().zip(&expected.rows) {
			assert_eq!(
				row.cells.len(),
				expected.cells.len(),
				"a table cell was dropped"
			);
			for (cell, expected) in row.cells.iter().zip(&expected.cells) {
				assert_eq!(cell.kind, expected.kind, "a header became data");
				// A colliding span may be shrunk instead of retried, so the
				// lowering may legitimately answer with a smaller span.
				assert!(
					cell.rowspan <= expected.rowspan,
					"a rowspan grew from {} to {}",
					expected.rowspan,
					cell.rowspan
				);
				assert!(
					cell.colspan <= expected.colspan,
					"a colspan grew from {} to {}",
					expected.colspan,
					cell.colspan
				);
				assert_eq!(
					cell.align, expected.align,
					"a cell alignment changed"
				);
				let found_text = cell
					.blocks
					.iter()
					.map(|block| match block.kind() {
						BlockKind::Paragraph(text) => plain_text(text),
						other => format!("{other:?}"),
					})
					.collect::<Vec<_>>()
					.join("\n");
				assert_eq!(found_text, expected.text, "a cell's text changed");
			}
		}
	}
}

/// Every marker exactly once, in the order the generator wrote them.
pub fn assert_markers(generated: &Generated) {
	let mut seen = HashSet::new();
	for marker in &generated.markers {
		assert!(
			seen.insert(marker.clone()),
			"the generator repeated {marker}"
		);
	}
	assert!(!generated.html.is_empty(), "the generator wrote no page");
}
