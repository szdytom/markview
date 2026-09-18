//! Semantic Markdown nodes and stable reading identities.
pub mod footnote;
mod heading;
mod incremental;
mod parse;
pub(crate) use heading::Anchors;
pub use heading::heading_slug;
pub use incremental::{parse_incremental, parse_prefix};
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
	Rule,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Block {
	pub id: u64,
	/// Semantic cache identity includes resolved references, excludes positions.
	pub content_key: u64,
	pub source: Range<usize>,
	pub kind: BlockKind,
}

#[derive(Clone, Debug)]
pub struct Document {
	pub source: Arc<str>,
	pub blocks: Vec<Block>,
	/// Semantic identity of the reading text; equal ids mean equal positions.
	pub content_id: u64,
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
		match &self.kind {
			BlockKind::Code { language, text } => {
				out.push((language, text));
			}
			BlockKind::Quote { blocks, .. }
			| BlockKind::Footnote { blocks, .. } => {
				for b in blocks {
					b.code_blocks(out);
				}
			}
			BlockKind::List { items, .. } => {
				for item in items {
					for b in &item.blocks {
						b.code_blocks(out);
					}
				}
			}
			_ => {}
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
