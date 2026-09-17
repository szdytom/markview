//! PDF export for a laid-out Markview document.
mod image;
mod link;
mod paint;
mod text;
use anyhow::Result;
use markview_core::{
	image::ImageSnapshot,
	layout::LayoutSnapshot,
	paginate::{PageGeometry, Pagination},
	style::Stylesheet,
};

/// The document metadata written into the PDF information dictionary. A field
/// left unset is not written at all, and a creation date never is, so the same
/// document always exports the same bytes.
#[derive(Clone, Debug, Default)]
pub struct Metadata {
	/// The document title; also what `{title}` names in page furniture.
	pub title: Option<String>,
	pub authors: Vec<String>,
	pub subject: Option<String>,
	pub keywords: Vec<String>,
	/// An RFC 3066 language tag, which a viewer or a screen reader uses.
	pub language: Option<String>,
	/// The application that produced the source document.
	pub creator: Option<String>,
}

/// Everything one export needs: the settled layout, the decoded images, the
/// resolved print stylesheet, and the pages the document breaks into.
pub struct Export<'a> {
	pub snapshot: &'a LayoutSnapshot,
	pub images: &'a ImageSnapshot,
	pub stylesheet: &'a Stylesheet,
	pub geometry: &'a PageGeometry,
	pub pagination: &'a Pagination,
	pub metadata: Metadata,
	/// Source path, for the `{path}` placeholder.
	pub path: String,
	/// Body text size in layout pixels, which page furniture sizes against.
	pub body_size_px: f32,
	pub links: bool,
}

pub fn export(input: Export<'_>) -> Result<Vec<u8>> {
	paint::render(&input)
}
