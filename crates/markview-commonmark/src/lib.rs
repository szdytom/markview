//! CommonMark and Markview extensions converted into semantic documents.
mod container;
mod front_matter;
mod html;
mod incremental;
mod limits;
mod make;
mod parse;
mod rich;
pub use incremental::{
	parse_incremental, parse_prefix, parse_prefix_with, reparse, reparse_with,
};
use markview_document as document;
use markview_document::Anchors;
use markview_document::LineBreak;
pub use markview_document::heading_slug;
use markview_document::image;
use markview_document::{
	Block, BlockKind, Callout, CalloutKind, CellAlign, CellKind, Document,
	FieldText, FootnoteId, FootnoteReference, Inline, InlineKind, ListItem,
	RichText, Table, TableCell, TableColumn, TableGroup, TableGroupKind,
	TableRow, plain_text, visit_fields,
};
pub use parse::{Dialect, parse, parse_with};

/// A parsed Markdown input: the semantic document plus its original text.
///
/// The original text and [`markview_document::SourceMap`] belong to the front
/// end. A [`Document`] built from HTML has neither.
#[derive(Clone, Debug)]
pub struct MarkdownParse {
	pub document: Document,
	pub source: std::sync::Arc<str>,
	pub map: markview_document::SourceMap,
	pub dialect: Dialect,
}

impl MarkdownParse {
	pub fn blocks(&self) -> &[Block] {
		self.document.blocks()
	}

	/// The recorded source interval of one block, if it has one.
	pub fn block_source(
		&self,
		block: &Block,
	) -> Option<std::ops::Range<usize>> {
		self.map
			.block(block.id())
			.and_then(|entry| entry.source.clone())
	}

	/// The source interval one rich-text run came from.
	pub fn inline_source(
		&self,
		block: &Block,
		inline: &Inline,
	) -> Option<std::ops::Range<usize>> {
		let mut found = None;
		visit_fields(
			block,
			&mut Vec::new(),
			&|| false,
			&mut |_, field, text, _| {
				let FieldText::Rich(rich) = text else {
					return;
				};
				let mut semantic = 0;
				for candidate in rich {
					let len = plain_text(std::slice::from_ref(candidate)).len();
					if std::ptr::eq(candidate, inline) {
						found = Some((field, semantic..semantic + len));
						return;
					}
					semantic += len;
				}
			},
		);
		let (field, semantic) = found?;
		self.map
			.field(block.id(), field)
			.and_then(|entry| {
				entry
					.covers
					.iter()
					.filter(|cover| {
						cover.semantic.start <= semantic.start
							&& semantic.end <= cover.semantic.end
					})
					.min_by_key(|cover| cover.semantic.len())
					.map(|cover| cover.source.clone())
			})
			.or_else(|| {
				self.map
					.semantic_to_source(block.id(), field, semantic)
					.map(|target| target.source)
			})
	}
}

impl std::ops::Deref for MarkdownParse {
	type Target = Document;

	fn deref(&self) -> &Document {
		&self.document
	}
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod proptests;
