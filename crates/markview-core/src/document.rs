//! Semantic Markdown nodes and stable reading identities.
pub mod footnote;
pub(crate) mod front_matter;
mod heading;
mod incremental;
mod parse;
pub(crate) use heading::Anchors;
pub use heading::heading_slug;
pub use incremental::{parse_incremental, parse_prefix, reparse};
pub use parse::parse;
use std::{
	collections::hash_map::DefaultHasher,
	hash::{Hash, Hasher},
	ops::Range,
	sync::Arc,
};

#[derive(Clone, Debug, Default, Hash, PartialEq, Eq)]
pub struct TextStyle {
	pub bold: bool,
	pub italic: bool,
	pub strike: bool,
	pub code: bool,
	pub math_error: bool,
	pub superscript: bool,
	/// A footnote reference: clickable, but styled by `footnote_ref` rather
	/// than by the link color.
	pub footnote_ref: bool,
	pub link: Option<String>,
	pub color: Option<crate::style::Color>,
}
impl TextStyle {
	/// The inline conditions this style activates, in application order.
	pub fn conditions(&self) -> impl Iterator<Item = crate::style::Condition> {
		use crate::style::Condition as C;
		[
			self.italic.then_some(C::Em),
			self.bold.then_some(C::Strong),
			(self.link.is_some() && !self.footnote_ref).then_some(C::Link),
			self.strike.then_some(C::Del),
			self.superscript.then_some(C::Sup),
			self.footnote_ref.then_some(C::FootnoteRef),
			self.code.then_some(C::Code),
			self.math_error.then_some(C::Math),
			self.math_error.then_some(C::Error),
		]
		.into_iter()
		.flatten()
	}
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum InlineKind {
	Text(String),
	Image(crate::image::ImageSpec),
	Math {
		latex: String,
		display: bool,
	},
	/// A footnote reference, drawn `[n]` and jumping to footnote `n`.
	FootnoteRef(u32),
	/// A forced line break. `justify` is set for an explicit HTML `<br>`, which
	/// asks for the line it ends to be set flush like any other.
	LineBreak {
		justify: bool,
	},
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Inline {
	pub kind: InlineKind,
	pub style: TextStyle,
	pub source: Range<usize>,
}

pub type RichText = Vec<Inline>;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum CellAlign {
	Left,
	Center,
	Right,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct ListItem {
	pub checked: Option<bool>,
	pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum BlockKind {
	Paragraph(RichText),
	Heading {
		level: u8,
		text: RichText,
		/// The GitHub-style fragment that addresses this heading.
		anchor: String,
	},
	Code {
		language: String,
		text: String,
	},
	Quote {
		label: Option<String>,
		blocks: Vec<Block>,
	},
	List {
		start: Option<usize>,
		tight: bool,
		items: Vec<ListItem>,
	},
	Table {
		align: Vec<CellAlign>,
		rows: Vec<Vec<RichText>>,
	},
	Footnote {
		label: String,
		/// Digits the widest number in the document occupies. Every note
		/// reserves this column, so their bodies start at one x.
		column: u32,
		blocks: Vec<Block>,
	},
	/// A raw-HTML `<details>` element: a collapsible container whose summary
	/// line toggles its content. `open` is the state the source declared; the
	/// reader's own choice lives in the layout options. `ordinal` counts the
	/// element among the document's `<details>`, so two identical elements
	/// still toggle independently.
	Details {
		open: bool,
		ordinal: u32,
		summary: RichText,
		blocks: Vec<Block>,
	},
	Rule,
	/// YAML front matter: a flat mapping rendered as a two-column table, and
	/// the verbatim source as a `yaml` code block when a value nests.
	FrontMatter {
		/// A key cell and a value cell per entry, set when every value fits
		/// a cell.
		table: Option<Vec<Vec<RichText>>>,
		/// The YAML between the delimiters, verbatim.
		text: String,
	},
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Block {
	pub id: u64,
	/// Semantic cache identity includes resolved references, excludes positions.
	pub content_key: u64,
	pub source: Range<usize>,
	pub kind: BlockKind,
}

/// One heading of a document's outline, in reading order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutlineEntry {
	pub level: u8,
	/// The heading's plain reading text.
	pub text: String,
	/// The GitHub-style fragment a link to this heading resolves.
	pub anchor: String,
}

#[derive(Clone, Debug)]
pub struct Document {
	pub source: Arc<str>,
	pub blocks: Vec<Block>,
	/// Semantic identity of the reading text; equal ids mean equal positions.
	pub content_id: u64,
}

impl Document {
	/// The state a `<details>` element with `id` declared in its source.
	pub fn details_declared(&self, id: u64) -> Option<bool> {
		self.blocks.iter().find_map(|b| b.details_declared(id))
	}

	/// The `<details>` elements enclosing the block that registers `anchor`,
	/// outermost first.
	///
	/// A heading and a footnote definition both register a layout anchor, and
	/// either can sit inside a collapsed body that is never laid out. A jump
	/// to such an anchor must expand the disclosures framing it, outermost
	/// first, before its target can be found.
	pub fn details_enclosing(&self, anchor: &str) -> Vec<u64> {
		fn registers(block: &Block, anchor: &str) -> bool {
			match &block.kind {
				BlockKind::Heading { anchor: a, .. } => a == anchor,
				BlockKind::Footnote { label, .. } => {
					footnote::anchor(label) == anchor
				}
				_ => false,
			}
		}
		fn walk(
			blocks: &[Block],
			anchor: &str,
			open: &mut Vec<u64>,
			out: &mut Vec<u64>,
		) -> bool {
			for block in blocks {
				if registers(block, anchor) {
					out.extend_from_slice(open);
					return true;
				}
				let disclosure =
					matches!(block.kind, BlockKind::Details { .. });
				if disclosure {
					open.push(block.id);
				}
				let found = match &block.kind {
					BlockKind::Details { blocks, .. }
					| BlockKind::Quote { blocks, .. }
					| BlockKind::Footnote { blocks, .. } => walk(blocks, anchor, open, out),
					BlockKind::List { items, .. } => items
						.iter()
						.any(|item| walk(&item.blocks, anchor, open, out)),
					_ => false,
				};
				if disclosure {
					open.pop();
				}
				if found {
					return true;
				}
			}
			false
		}
		let mut out = Vec::new();
		walk(&self.blocks, anchor, &mut Vec::new(), &mut out);
		out
	}

	/// The document's headings in reading order, with the anchors links use.
	///
	/// The walk follows the order anchors are assigned in: containers are
	/// entered where they appear, so a heading nested in a quote, list,
	/// footnote or `<details>` sits where its text is read.
	pub fn outline(&self) -> Vec<OutlineEntry> {
		fn walk(blocks: &[Block], out: &mut Vec<OutlineEntry>) {
			for block in blocks {
				match &block.kind {
					BlockKind::Heading {
						level,
						text,
						anchor,
					} => out.push(OutlineEntry {
						level: *level,
						text: plain_text(text),
						anchor: anchor.clone(),
					}),
					BlockKind::Quote { blocks, .. }
					| BlockKind::Footnote { blocks, .. }
					| BlockKind::Details { blocks, .. } => walk(blocks, out),
					BlockKind::List { items, .. } => {
						for item in items {
							walk(&item.blocks, out);
						}
					}
					_ => {}
				}
			}
		}
		let mut out = Vec::new();
		walk(&self.blocks, &mut out);
		out
	}
}

pub fn fingerprint(value: &impl Hash) -> u64 {
	let mut h = DefaultHasher::new();
	value.hash(&mut h);
	h.finish()
}

/// Semantic identity of a block list: equal ids mean equal reading text.
pub(super) fn content_identity(blocks: &[Block]) -> u64 {
	let mut hasher = DefaultHasher::new();
	for block in blocks {
		block.content_key.hash(&mut hasher);
	}
	hasher.finish()
}

pub fn plain_text(text: &RichText) -> String {
	let mut out = String::new();
	for span in text {
		match &span.kind {
			InlineKind::Text(t) => out.push_str(t),
			InlineKind::Image(image) => out.push_str(&image.alt),
			InlineKind::Math { latex, .. } => out.push_str(latex),
			InlineKind::FootnoteRef(n) => {
				out.push_str(&format!("[{n}]"));
			}
			InlineKind::LineBreak { .. } => out.push('\n'),
		}
	}
	out
}

/// Scheme of the pseudo URL a `<details>` summary's hit region carries.
pub const DETAILS_SCHEME: &str = "details:";

/// The pseudo URL that makes a summary line behave like a link, so the
/// existing non-drag release and hover state can drive the toggle.
pub fn details_url(id: u64) -> String {
	format!("{DETAILS_SCHEME}{id}")
}

/// The block id a summary pseudo URL addresses.
pub fn details_id(url: &str) -> Option<u64> {
	url.strip_prefix(DETAILS_SCHEME)?.parse().ok()
}

impl Block {
	pub fn images<'a>(&'a self, out: &mut Vec<&'a crate::image::ImageSpec>) {
		fn rich<'a>(
			text: &'a RichText,
			out: &mut Vec<&'a crate::image::ImageSpec>,
		) {
			for inline in text {
				if let InlineKind::Image(image) = &inline.kind {
					out.push(image);
				}
			}
		}
		match &self.kind {
			BlockKind::Paragraph(t) | BlockKind::Heading { text: t, .. } => {
				rich(t, out)
			}
			BlockKind::Quote { blocks, .. }
			| BlockKind::Footnote { blocks, .. } => {
				for b in blocks {
					b.images(out);
				}
			}
			BlockKind::Details {
				summary, blocks, ..
			} => {
				rich(summary, out);
				for b in blocks {
					b.images(out);
				}
			}
			BlockKind::List { items, .. } => {
				for item in items {
					for b in &item.blocks {
						b.images(out);
					}
				}
			}
			BlockKind::Table { rows, .. } => {
				for row in rows {
					for cell in row {
						rich(cell, out);
					}
				}
			}
			_ => {}
		}
	}

	/// The `(language, text)` of every code block in this block's subtree, in
	/// document order, so a caller can tell which syntax colors the geometry
	/// depends on without laying the block out again.
	pub fn code_blocks<'a>(&'a self, out: &mut Vec<(&'a str, &'a str)>) {
		self.for_each_code_block(&mut |language, text| {
			out.push((language, text));
		});
	}

	/// The same walk as [`Block::code_blocks`], without the intermediate
	/// vector. One walk serves both the geometry's external inputs and the
	/// highlighter's job list, so the two can never disagree about which code
	/// a block draws — a block that one of them saw and the other missed would
	/// keep its uncolored geometry forever.
	pub(crate) fn for_each_code_block<'a>(
		&'a self,
		visit: &mut impl FnMut(&'a str, &'a str),
	) {
		match &self.kind {
			BlockKind::Code { language, text } => visit(language, text),
			// A front matter drawn as source is a `yaml` code block; the table
			// shape holds cells with no code in them.
			BlockKind::FrontMatter { table: None, text } => {
				visit(front_matter::LANGUAGE, text);
			}
			BlockKind::Quote { blocks, .. }
			| BlockKind::Footnote { blocks, .. }
			| BlockKind::Details { blocks, .. } => {
				for b in blocks {
					b.for_each_code_block(visit);
				}
			}
			BlockKind::List { items, .. } => {
				for item in items {
					for b in &item.blocks {
						b.for_each_code_block(visit);
					}
				}
			}
			_ => {}
		}
	}

	/// The state a `<details>` element with `id` declared in its source,
	/// searching the containers it may be nested in.
	pub fn details_declared(&self, id: u64) -> Option<bool> {
		match &self.kind {
			BlockKind::Details { open, blocks, .. } => {
				if self.id == id {
					return Some(*open);
				}
				blocks.iter().find_map(|b| b.details_declared(id))
			}
			BlockKind::Quote { blocks, .. }
			| BlockKind::Footnote { blocks, .. } => {
				blocks.iter().find_map(|b| b.details_declared(id))
			}
			BlockKind::List { items, .. } => items.iter().find_map(|item| {
				item.blocks.iter().find_map(|b| b.details_declared(id))
			}),
			_ => None,
		}
	}
}

fn semantic_key(kind: &BlockKind) -> u64 {
	let mut hash = DefaultHasher::new();
	std::mem::discriminant(kind).hash(&mut hash);
	let rich = |t: &RichText| {
		fingerprint(&t.iter().map(|i| (&i.kind, &i.style)).collect::<Vec<_>>())
	};
	let children =
		|b: &[Block]| b.iter().map(|b| b.content_key).collect::<Vec<_>>();
	match kind {
		BlockKind::Paragraph(t) => rich(t).hash(&mut hash),
		BlockKind::Heading {
			level,
			text,
			anchor,
		} => (level, rich(text), anchor).hash(&mut hash),
		BlockKind::Code { language, text } => (language, text).hash(&mut hash),
		// The YAML determines both shapes, so it is the whole identity.
		BlockKind::FrontMatter { table: _, text } => text.hash(&mut hash),
		BlockKind::Quote { label: _, blocks }
		| BlockKind::Footnote {
			label: _, blocks, ..
		} => {
			// The variants' labels have different types, so hash them separately.
			if let BlockKind::Quote { label, .. } = kind {
				label.hash(&mut hash);
			}
			if let BlockKind::Footnote { label, column, .. } = kind {
				(label, column).hash(&mut hash);
			}
			children(blocks).hash(&mut hash);
		}
		BlockKind::List {
			start,
			tight,
			items,
		} => {
			(start, tight).hash(&mut hash);
			for item in items {
				(item.checked, children(&item.blocks)).hash(&mut hash);
			}
		}
		BlockKind::Details {
			open,
			ordinal: _,
			summary,
			blocks,
		} => {
			(open, rich(summary), children(blocks)).hash(&mut hash);
		}
		BlockKind::Table { align, rows } => {
			align.hash(&mut hash);
			for row in rows {
				for cell in row {
					rich(cell).hash(&mut hash);
				}
			}
		}
		BlockKind::Rule => {}
	}
	hash.finish()
}

#[cfg(test)]
mod tests;
