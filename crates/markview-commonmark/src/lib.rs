//! CommonMark and Markview extensions converted into semantic documents.
mod container;
mod front_matter;
mod heading;
mod html;
mod incremental;
mod limits;
mod parse;
use heading::Anchors;
pub use heading::heading_slug;
pub use incremental::{parse_incremental, parse_prefix, reparse};
use markview_document as document;
use markview_document::image;
use markview_document::{
	Block, BlockKind, CellAlign, Document, Inline, InlineKind, Link, ListItem,
	RichText, TextStyle, content_identity, fingerprint, footnote, plain_text,
	semantic_key,
};
pub use parse::parse;
#[cfg(test)]
mod tests;
