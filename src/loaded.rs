//! A document ready for layout, with whatever source information its front end
//! can supply.
use markview_commonmark::MarkdownParse;
use markview_document::Document;
use std::sync::Arc;

/// A layout-ready document from a file, an editor or a web page.
#[derive(Clone, Debug)]
pub enum LoadedDocument {
	/// Parsed Markdown: the document plus its original text and source map.
	Markdown(Arc<MarkdownParse>),
	/// A semantic document without original source, as produced by HTML, and
	/// the size of its semantic text plus its input identity.
	Semantic {
		document: Arc<Document>,
		size: usize,
		input: u64,
		frontend: u64,
	},
}

impl LoadedDocument {
	/// Reuses the document unless its Markdown dialect changed.
	pub fn with_dialect(
		self: &Arc<Self>,
		dialect: markview_commonmark::Dialect,
	) -> Arc<Self> {
		if let Self::Markdown(parse) = self.as_ref()
			&& parse.dialect != dialect
		{
			return Self::markdown(Arc::new(markview_commonmark::parse_with(
				parse.source.clone(),
				dialect,
			)));
		}
		self.clone()
	}

	pub fn markdown(parse: Arc<MarkdownParse>) -> Arc<Self> {
		Arc::new(Self::Markdown(parse))
	}

	#[cfg(test)]
	pub fn semantic(document: Arc<Document>) -> Arc<Self> {
		let input = markview_document::stable_fingerprint(
			document.content_key().get().to_le_bytes().as_slice(),
		);
		Self::semantic_with_input(document, input)
	}

	/// A semantic document with the digest of the bytes it came from.
	#[cfg(test)]
	pub fn semantic_with_input(
		document: Arc<Document>,
		input: u64,
	) -> Arc<Self> {
		Self::semantic_with_versions(document, input, 2)
	}

	/// A semantic document with an explicit input and front-end version.
	pub fn semantic_with_versions(
		document: Arc<Document>,
		input: u64,
		frontend: u64,
	) -> Arc<Self> {
		let size = semantic_size(&document);
		Arc::new(Self::Semantic {
			document,
			size,
			input,
			frontend,
		})
	}

	pub fn document(&self) -> &Document {
		match self {
			Self::Markdown(parse) => &parse.document,
			Self::Semantic { document, .. } => document,
		}
	}

	pub fn source(&self) -> Option<&str> {
		match self {
			Self::Markdown(parse) => Some(&parse.source),
			Self::Semantic { .. } => None,
		}
	}

	/// The identity of the input bytes this document was built from.
	pub fn input_fingerprint(&self) -> u64 {
		match self {
			Self::Markdown(parse) => {
				markview_document::stable_fingerprint(parse.source.as_bytes())
			}
			Self::Semantic { input, .. } => *input,
		}
	}

	/// The front-end and parse-option version that produced this document.
	pub fn frontend_version(&self) -> u64 {
		match self {
			Self::Markdown(parse) => {
				2 + u64::from(parse.dialect.hard_breaks)
					+ (u64::from(parse.dialect.highlight_marks) << 1)
					+ (u64::from(parse.dialect.ofm) << 2)
					+ (u64::from(parse.dialect.grid_tables) << 3)
			}
			Self::Semantic { frontend, .. } => *frontend,
		}
	}

	/// An estimate of the document's size in bytes, for the thresholds that
	/// decide whether it is large: the original text when the front end kept
	/// one, else the semantic text it was lowered into, measured once when
	/// this handle was built.
	pub fn size(&self) -> usize {
		match self {
			Self::Markdown(parse) => parse.source.len(),
			Self::Semantic { size, .. } => *size,
		}
	}
}

/// The bytes of semantic text a document holds, walked once per build.
fn semantic_size(document: &Document) -> usize {
	let mut total = 0;
	for block in document.blocks() {
		markview_document::visit_fields(
			block,
			&mut Vec::new(),
			&|| false,
			&mut |_, _, text, _| total += text.len(),
		);
	}
	total
}

impl std::ops::Deref for LoadedDocument {
	type Target = Document;

	fn deref(&self) -> &Document {
		self.document()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use markview_document::{
		BlockKind, DocumentBuilder, DocumentRevision, Inline, InlineKind,
	};

	fn text(text: &str) -> Vec<Inline> {
		vec![Inline {
			link: None,
			kind: InlineKind::Text(text.to_owned()),
			style: Default::default(),
		}]
	}

	/// A web page has no source text, so its size is the semantic text of every
	/// field, nested containers included, measured when the handle is built.
	#[test]
	fn a_semantic_document_measures_nested_text_once() {
		let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
		let nested =
			builder.block(Vec::new(), BlockKind::Paragraph(text("quoted")));
		let quote = builder.block(
			Vec::new(),
			BlockKind::Quote {
				callout: None,
				blocks: vec![nested],
			},
		);
		let code = builder.block(
			Vec::new(),
			BlockKind::Code {
				language: "rust".to_owned(),
				text: "fn f() {}".to_owned(),
			},
		);
		let loaded = LoadedDocument::semantic(Arc::new(
			builder.finish(vec![quote, code]),
		));
		assert_eq!(loaded.source(), None);
		assert_eq!(loaded.size(), "quoted".len() + "fn f() {}".len());
	}
}
