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
pub use incremental::{
	parse_incremental, parse_prefix, parse_prefix_with, reparse, reparse_with,
};
use markview_document as document;
use markview_document::image;
use markview_document::{
	Block, BlockKind, Callout, CalloutKind, CellAlign, Document, Inline,
	InlineKind, Link, ListItem, RichText, TextStyle, content_identity,
	fingerprint, footnote, plain_text, semantic_key,
};
pub use parse::{Dialect, parse, parse_with};
#[cfg(test)]
mod tests;
