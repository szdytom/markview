//! Semantic document nodes and stable reading identities.
//!
//! A [`Document`] owns only semantic nodes and their derived indexes. Original
//! text, format-specific parse state and layout geometry live elsewhere: a
//! CommonMark front end keeps a [`crate::source::SourceMap`], while an HTML
//! front end may have no source text at all.
mod color;
pub mod image;
pub use color::Color;
pub use image::{Diagram, ImageSource, ImageSpec, ResourceBase, ResourceRef};
mod build;
mod ids;
mod index;
mod slug;
mod source;
mod visit;
pub use build::{DocumentBuilder, DocumentLimit, DocumentLimits};
pub use ids::{
	Affinity, BlockId, ContentKey, DocumentRevision, FieldId, LinkOccurrenceId,
};
pub use slug::{Anchors, heading_slug};
pub use source::{
	BlockSource, FieldSource, SemanticPosition, SourceMap, SourceMapError,
	SourceSpan, SourceTarget,
};
use std::{
	collections::HashMap,
	hash::{Hash, Hasher},
	sync::Arc,
};
pub use visit::{FieldText, visit_blocks, visit_fields};

/// One link occurrence, shared by every run and every open scope that carries
/// it. Address equality and occurrence equality are separate operations; the
/// type deliberately has no `Eq`, so a caller cannot compare by accident.
#[derive(Clone, Debug)]
pub struct Link {
	url: Arc<str>,
	/// The address's content hash for semantic cache keys.
	key: u64,
	occurrence: LinkOccurrenceId,
}

impl Link {
	/// One occurrence of `url`.
	pub fn new(url: impl Into<Arc<str>>, occurrence: LinkOccurrenceId) -> Self {
		let url = url.into();
		Self {
			key: fingerprint(&url),
			url,
			occurrence,
		}
	}

	/// The same address as another occurrence, without rehashing it.
	pub fn with_occurrence(&self, occurrence: LinkOccurrenceId) -> Self {
		Self {
			url: self.url.clone(),
			key: self.key,
			occurrence,
		}
	}

	/// The identity of this occurrence, shared by every run of the link.
	pub fn occurrence(&self) -> LinkOccurrenceId {
		self.occurrence
	}

	pub fn as_str(&self) -> &str {
		&self.url
	}

	fn hash_key(&self) -> u64 {
		self.key
	}

	/// The shared address, so a caller that stores one keeps the sharing.
	pub fn shared(&self) -> &Arc<str> {
		&self.url
	}

	/// Whether both links name the same address, regardless of occurrence.
	pub fn same_address(&self, other: &Self) -> bool {
		Arc::ptr_eq(&self.url, &other.url) || self.url == other.url
	}

	/// Whether both links name this very occurrence, not merely the
	/// same address: adjacent links keep separate ids even when they match.
	pub fn same_occurrence(&self, other: &Self) -> bool {
		self.occurrence == other.occurrence && self.same_address(other)
	}
}

/// Visual inline style. Navigation lives on [`Inline::link`].
#[derive(Clone, Debug, Default, Hash, PartialEq, Eq)]
pub struct TextStyle {
	pub bold: bool,
	pub italic: bool,
	pub strike: bool,
	pub mark: bool,
	pub code: bool,
	pub superscript: bool,
	pub subscript: bool,
	pub color: Option<Color>,
}

/// A footnote definition's identity, normalized from its source label.
#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct FootnoteId(String);

impl FootnoteId {
	pub fn new(label: impl Into<String>) -> Self {
		Self(label.into())
	}

	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl std::fmt::Display for FootnoteId {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		self.0.fmt(f)
	}
}

/// One reference to a footnote definition. The occurrence is this reference's
/// own identity, so several references to one definition hit independently.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct FootnoteReference {
	pub definition: FootnoteId,
	pub number: u32,
	pub occurrence: LinkOccurrenceId,
}

/// A typed target inside the current document.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum AnchorId {
	/// An HTML fragment, including a real element `id`.
	Fragment(String),
	FootnoteDefinition(FootnoteId),
	FootnoteReference {
		definition: FootnoteId,
		occurrence: LinkOccurrenceId,
	},
}

/// A value that names a document anchor.
pub trait ToAnchor {
	fn to_anchor(&self) -> AnchorId;
}

impl AnchorId {
	/// The HTML fragment this anchor names, if it is one.
	pub fn fragment(&self) -> Option<&str> {
		match self {
			Self::Fragment(fragment) => Some(fragment),
			_ => None,
		}
	}
}

impl ToAnchor for AnchorId {
	fn to_anchor(&self) -> AnchorId {
		self.clone()
	}
}

impl ToAnchor for &AnchorId {
	fn to_anchor(&self) -> AnchorId {
		(*self).clone()
	}
}

impl ToAnchor for str {
	fn to_anchor(&self) -> AnchorId {
		AnchorId::Fragment(self.to_owned())
	}
}

impl ToAnchor for &str {
	fn to_anchor(&self) -> AnchorId {
		AnchorId::Fragment((*self).to_owned())
	}
}

impl ToAnchor for String {
	fn to_anchor(&self) -> AnchorId {
		AnchorId::Fragment(self.clone())
	}
}

impl ToAnchor for &String {
	fn to_anchor(&self) -> AnchorId {
		AnchorId::Fragment((*self).clone())
	}
}

impl From<&str> for AnchorId {
	fn from(fragment: &str) -> Self {
		Self::Fragment(fragment.to_owned())
	}
}

impl From<String> for AnchorId {
	fn from(fragment: String) -> Self {
		Self::Fragment(fragment)
	}
}

impl PartialEq<str> for AnchorId {
	fn eq(&self, other: &str) -> bool {
		matches!(self, Self::Fragment(fragment) if fragment == other)
	}
}

impl PartialEq<&str> for AnchorId {
	fn eq(&self, other: &&str) -> bool {
		self == *other
	}
}

/// What kind of line ending a break is, independent of justification policy.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum LineBreak {
	/// A Markdown hard break: it ends the line without asking for flush text.
	Hard,
	/// An explicit HTML `<br>`, which asks for the line it ends to be set
	/// flush like any other.
	Forced,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum InlineKind {
	/// An invisible HTML fragment target at this reading position.
	Anchor(String),
	Text(String),
	Image(crate::image::ImageSpec),
	Math {
		latex: String,
		display: bool,
	},
	/// A footnote reference, drawn `[n]` and jumping to its definition.
	FootnoteRef(FootnoteReference),
	LineBreak(LineBreak),
}

#[derive(Clone, Debug)]
pub struct Inline {
	pub link: Option<Link>,
	pub kind: InlineKind,
	pub style: TextStyle,
}

impl Inline {
	pub fn link_url(&self) -> Option<&str> {
		self.link.as_ref().map(Link::as_str)
	}

	pub fn shares_link_occurrence(&self, other: &Self) -> bool {
		match (&self.link, &other.link) {
			(Some(a), Some(b)) => a.same_occurrence(b),
			(None, None) => true,
			_ => false,
		}
	}
}

impl PartialEq for Inline {
	fn eq(&self, other: &Self) -> bool {
		self.kind == other.kind
			&& self.style == other.style
			&& match (&self.link, &other.link) {
				(Some(a), Some(b)) => a.same_address(b),
				(None, None) => true,
				_ => false,
			}
	}
}
impl Eq for Inline {}
impl Hash for Inline {
	fn hash<H: Hasher>(&self, state: &mut H) {
		self.kind.hash(state);
		self.style.hash(state);
		self.link.as_ref().map(Link::hash_key).hash(state);
	}
}

pub type RichText = Vec<Inline>;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum CellAlign {
	Left,
	Center,
	Right,
}

/// Which part of a table a row group belongs to.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum TableGroupKind {
	Head,
	Body,
	Foot,
}

/// What a table cell is: a header cell or a data cell.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum CellKind {
	Header,
	Data,
}

/// One logical table column's presentation.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct TableColumn {
	pub align: Option<CellAlign>,
}

/// One cell of a table, occupying a rectangular span of the grid.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct TableCell {
	pub kind: CellKind,
	/// Positive row span, already clipped to the row group.
	pub rowspan: u32,
	/// Positive column span, clipped to the table's column count.
	pub colspan: u32,
	/// Per-cell override of the column's alignment.
	pub align: Option<CellAlign>,
	pub blocks: Vec<Block>,
}

/// One definition-list item: one or more terms and their definitions.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct DefinitionItem {
	pub terms: Vec<RichText>,
	pub definitions: Vec<Vec<Block>>,
}

/// One table row, listing only the cells that start in it.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct TableRow {
	pub cells: Vec<TableCell>,
}

/// A head, body or foot group of table rows.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct TableGroup {
	pub kind: TableGroupKind,
	pub rows: Vec<TableRow>,
}

/// A table's caption, column model and row groups.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Table {
	pub caption: Option<RichText>,
	pub columns: Vec<TableColumn>,
	pub groups: Vec<TableGroup>,
}

impl Table {
	/// Every cell's blocks in row-major order.
	pub fn cell_blocks(&self) -> impl Iterator<Item = &Block> {
		self.groups
			.iter()
			.flat_map(|group| group.rows.iter())
			.flat_map(|row| row.cells.iter())
			.flat_map(|cell| cell.blocks.iter())
	}

	/// The same walk with mutable blocks.
	pub fn cell_blocks_mut(&mut self) -> impl Iterator<Item = &mut Block> {
		self.groups
			.iter_mut()
			.flat_map(|group| group.rows.iter_mut())
			.flat_map(|row| row.cells.iter_mut())
			.flat_map(|cell| cell.blocks.iter_mut())
	}
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct ListItem {
	pub checked: Option<bool>,
	/// An explicit number for an ordered item, overriding the sequence.
	pub number: Option<isize>,
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
		/// A GFM alert or Obsidian callout rather than a plain quote.
		callout: Option<Callout>,
		blocks: Vec<Block>,
	},
	List {
		start: Option<isize>,
		/// Whether an ordered list counts down from `start`.
		reversed: bool,
		tight: bool,
		items: Vec<ListItem>,
	},
	Table(Table),
	Footnote {
		id: FootnoteId,
		number: u32,
		blocks: Vec<Block>,
	},
	/// A raw-HTML `<details>` element: a collapsible container whose summary
	/// line toggles its content.
	Details {
		/// The state the source declared. A reader's override lives in the
		/// session layer, never here.
		default_open: bool,
		summary: RichText,
		blocks: Vec<Block>,
	},
	Rule,
	/// YAML front matter: a disclosure whose body is the verbatim source as a
	/// `yaml` code block. Metadata is not prose, so its default is collapsed.
	FrontMatter {
		/// The default collapse state; front matter has no source state, so it
		/// is always `false`. Reader overrides stay in the session layer.
		default_open: bool,
		/// The `yaml` code block holding the source between the delimiters.
		blocks: Vec<Block>,
	},
	/// A definition list (`dl`) with its `dt` terms and `dd` definitions.
	DefinitionList(Vec<DefinitionItem>),
	/// A figure: its content blocks followed by an optional caption.
	Figure {
		blocks: Vec<Block>,
		caption: Option<RichText>,
	},
}

/// GFM alert and Obsidian callout types. Unknown Obsidian types use `Note`.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum CalloutKind {
	Note,
	Abstract,
	Info,
	Todo,
	Tip,
	/// GFM important; Obsidian treats `important` as a `Tip` alias.
	Important,
	Success,
	Question,
	Warning,
	/// GFM caution; Obsidian treats `caution` as a `Warning` alias.
	Caution,
	Failure,
	Danger,
	Bug,
	Example,
	Quote,
}

impl CalloutKind {
	/// The type a `[!name]` marker names, case-insensitively.
	pub fn parse(name: &str) -> Self {
		match name.to_ascii_lowercase().as_str() {
			"abstract" | "summary" | "tldr" => Self::Abstract,
			"info" => Self::Info,
			"todo" => Self::Todo,
			"tip" | "hint" | "important" => Self::Tip,
			"success" | "check" | "done" => Self::Success,
			"question" | "help" | "faq" => Self::Question,
			"warning" | "caution" | "attention" => Self::Warning,
			"failure" | "fail" | "missing" => Self::Failure,
			"danger" | "error" => Self::Danger,
			"bug" => Self::Bug,
			"example" => Self::Example,
			"quote" | "cite" => Self::Quote,
			_ => Self::Note,
		}
	}
}

/// A GFM alert or Obsidian callout's header.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Callout {
	pub kind: CalloutKind,
	/// The type name the source declared, including custom Obsidian types.
	pub name: String,
	/// The custom title, or the marker's type name when none was given.
	///
	/// A foldable callout has no title here: its title is the summary of the
	/// nested disclosure, so the content has one owner.
	pub title: Option<RichText>,
	/// `Some(default_open)` for a foldable `[!x]+` / `[!x]-` callout. Its body
	/// is then one `<details>`-style disclosure whose summary is the title.
	pub fold: Option<bool>,
}

/// Two blocks compare by what they read as, not by occurrence identity: an
/// incremental parse keeps the same meaning under new ids.
impl PartialEq for Block {
	fn eq(&self, other: &Self) -> bool {
		self.anchors == other.anchors && self.kind == other.kind
	}
}

impl Eq for Block {}

impl Hash for Block {
	fn hash<H: Hasher>(&self, state: &mut H) {
		self.anchors.hash(state);
		self.kind.hash(state);
	}
}

/// One semantic block.
///
/// The fields are private: producers submit semantic input to a
/// [`DocumentBuilder`], which allocates identities and derives cache keys.
/// A finished document cannot be edited into an inconsistent state.
#[derive(Clone, Debug)]
pub struct Block {
	pub(crate) anchors: Vec<String>,
	pub(crate) id: BlockId,
	/// Semantic cache identity includes resolved references, excludes positions.
	pub(crate) content_key: ContentKey,
	pub(crate) kind: BlockKind,
}

impl Block {
	/// HTML targets attached to the top of this block.
	pub fn anchors(&self) -> &[String] {
		&self.anchors
	}

	pub fn id(&self) -> BlockId {
		self.id
	}

	/// Semantic cache identity, excluding identities and source positions.
	pub fn content_key(&self) -> ContentKey {
		self.content_key
	}

	pub fn kind(&self) -> &BlockKind {
		&self.kind
	}

	/// Mutable kind access for a block that is being rebuilt before sealing.
	pub fn kind_mut(&mut self) -> &mut BlockKind {
		&mut self.kind
	}

	/// Replaces the anchors of a block that is being rebuilt.
	pub fn with_anchors(mut self, anchors: Vec<String>) -> Self {
		self.anchors = anchors;
		self
	}

	/// Replaces the anchors in place during a builder-side rebuild.
	pub fn relabel(&mut self, anchors: Vec<String>) {
		self.anchors = anchors;
	}

	/// Adds one anchor during a builder-side rebuild.
	pub fn add_anchor(&mut self, anchor: String) {
		if !self.anchors.contains(&anchor) {
			self.anchors.push(anchor);
		}
	}

	/// Replaces the kind of a block that is being rebuilt.
	pub fn with_kind(mut self, kind: BlockKind) -> Self {
		self.kind = kind;
		self
	}
}

/// One heading of a document's outline, in reading order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutlineEntry {
	pub block: BlockId,
	pub level: u8,
	/// The heading's plain reading text.
	pub text: String,
	/// The GitHub-style fragment a link to this heading resolves.
	pub anchor: String,
}

#[derive(Clone, Debug)]
pub struct Document {
	revision: DocumentRevision,
	/// Semantic identity of the reading text; equal keys mean equal content.
	content: ContentKey,
	blocks: Vec<Block>,
	index: index::DocumentIndex,
}

impl Document {
	pub fn revision(&self) -> DocumentRevision {
		self.revision
	}

	/// The semantic content identity used to reuse geometry.
	pub fn content_key(&self) -> ContentKey {
		self.content
	}

	pub fn blocks(&self) -> &[Block] {
		&self.blocks
	}

	/// Consumes the document, returning its sealed blocks for a rebuild.
	pub fn into_blocks(self) -> Vec<Block> {
		self.blocks
	}

	/// The block with `id`, using the index built when the document was sealed.
	pub fn block(&self, id: BlockId) -> Option<&Block> {
		let path = self.index.paths.get(&id)?;
		index::resolve(&self.blocks, path)
	}

	/// The structural path from the document root to `id`.
	pub fn path_of(&self, id: BlockId) -> Option<&[usize]> {
		self.index.paths.get(&id).map(Vec::as_slice)
	}

	/// The block at a structural path from the document root.
	pub fn block_at_path(&self, path: &[usize]) -> Option<&Block> {
		index::resolve(&self.blocks, path)
	}

	/// The state a `<details>` element with `id` declared in its source.
	pub fn details_declared(&self, id: BlockId) -> Option<bool> {
		self.index.details.get(&id).copied()
	}

	/// The `<details>` elements enclosing the block that registers `anchor`,
	/// outermost first.
	pub fn details_enclosing(&self, anchor: impl ToAnchor) -> Vec<BlockId> {
		self.index
			.enclosing
			.get(&anchor.to_anchor())
			.cloned()
			.unwrap_or_default()
	}

	/// The `<details>` elements enclosing an HTML fragment.
	pub fn fragment_enclosing(&self, anchor: &str) -> Vec<BlockId> {
		self.details_enclosing(AnchorId::Fragment(anchor.to_owned()))
	}

	/// The document's headings in reading order, with the anchors links use.
	pub fn outline(&self) -> Vec<OutlineEntry> {
		self.index.outline.clone()
	}

	/// The cached outline, without cloning it.
	pub fn outline_entries(&self) -> &[OutlineEntry] {
		&self.index.outline
	}
}

pub fn fingerprint(value: &impl Hash) -> u64 {
	let mut h = std::collections::hash_map::DefaultHasher::new();
	value.hash(&mut h);
	h.finish()
}

/// A deterministic 64-bit digest of raw input bytes, stable across builds.
///
/// This is an input identity for saved sessions, not a permission token.
pub fn stable_fingerprint(bytes: &[u8]) -> u64 {
	let mut hash = 0xcbf2_9ce4_8422_2325_u64;
	for byte in bytes {
		hash ^= u64::from(*byte);
		hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
	}
	hash
}

/// Semantic identity of a block list: equal ids mean equal reading text.
pub fn content_identity(blocks: &[Block]) -> ContentKey {
	let mut hasher = std::collections::hash_map::DefaultHasher::new();
	for block in blocks {
		block.content_key.hash(&mut hasher);
	}
	ContentKey(hasher.finish())
}

pub fn plain_text(text: &[Inline]) -> String {
	let mut out = String::new();
	write_plain(text, &mut out);
	out
}

/// The length of the [`plain_text`] projection without materializing it.
pub fn plain_text_len(text: &[Inline]) -> usize {
	text.iter()
		.map(|span| match &span.kind {
			InlineKind::Text(t) => t.len(),
			InlineKind::Image(image) => image.alt.len(),
			InlineKind::Math { latex, .. } => latex.len(),
			InlineKind::FootnoteRef(reference) => {
				reference.number.to_string().len() + 2
			}
			InlineKind::LineBreak(_) => 1,
			InlineKind::Anchor(_) => 0,
		})
		.sum()
}

fn write_plain(text: &[Inline], out: &mut String) {
	for span in text {
		match &span.kind {
			InlineKind::Text(t) => out.push_str(t),
			InlineKind::Image(image) => out.push_str(&image.alt),
			InlineKind::Math { latex, .. } => out.push_str(latex),
			InlineKind::FootnoteRef(reference) => {
				out.push('[');
				out.push_str(&reference.number.to_string());
				out.push(']');
			}
			InlineKind::LineBreak(_) => out.push('\n'),
			InlineKind::Anchor(_) => {}
		}
	}
}

/// The occurrence of every inline link or footnote reference in `text`, in
/// reading order. Runs whose content is later discarded still count here: the
/// list has to mirror what layout prepares, not what line breaking paints.
pub fn inline_link_origins(text: &[Inline]) -> Vec<LinkOccurrenceId> {
	let mut out = Vec::new();
	for inline in text {
		let occurrence = match (&inline.link, &inline.kind) {
			(_, InlineKind::Anchor(_)) => continue,
			(Some(link), _) => link.occurrence(),
			(None, InlineKind::FootnoteRef(reference)) => reference.occurrence,
			_ => continue,
		};
		if out.last() != Some(&occurrence) {
			out.push(occurrence);
		}
	}
	out
}

impl Block {
	#[cfg_attr(
		any(),
		markview::allow_complexity(
			reason = "Keep the exhaustive block-kind image walk beside the block model so a new container cannot silently lose its images."
		)
	)]
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
		fn callout_images<'a>(
			callout: &'a Option<Callout>,
			out: &mut Vec<&'a crate::image::ImageSpec>,
		) {
			if let Some(title) =
				callout.as_ref().and_then(|callout| callout.title.as_ref())
			{
				rich(title, out);
			}
		}
		match &self.kind {
			BlockKind::Paragraph(t) | BlockKind::Heading { text: t, .. } => {
				rich(t, out)
			}
			BlockKind::Quote { callout, blocks } => {
				callout_images(callout, out);
				for b in blocks {
					b.images(out);
				}
			}
			BlockKind::Footnote { blocks, .. } => {
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
			BlockKind::FrontMatter { .. } => {}
			BlockKind::List { items, .. } => {
				for item in items {
					for b in &item.blocks {
						b.images(out);
					}
				}
			}
			BlockKind::Table(table) => {
				if let Some(caption) = &table.caption {
					rich(caption, out);
				}
				for child in table.cell_blocks() {
					child.images(out);
				}
			}
			BlockKind::DefinitionList(items) => {
				for item in items {
					for terms in &item.terms {
						rich(terms, out);
					}
					for blocks in &item.definitions {
						for b in blocks {
							b.images(out);
						}
					}
				}
			}
			BlockKind::Figure { blocks, caption } => {
				if let Some(caption) = caption {
					rich(caption, out);
				}
				for b in blocks {
					b.images(out);
				}
			}
			BlockKind::Code { .. } | BlockKind::Rule => {}
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
	pub fn for_each_code_block<'a>(
		&'a self,
		visit: &mut impl FnMut(&'a str, &'a str),
	) {
		match &self.kind {
			BlockKind::Code { language, text } => visit(language, text),
			BlockKind::Quote { blocks, .. }
			| BlockKind::Footnote { blocks, .. }
			| BlockKind::Details { blocks, .. }
			| BlockKind::FrontMatter { blocks, .. } => {
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
			BlockKind::Table(table) => {
				for b in table.cell_blocks() {
					b.for_each_code_block(visit);
				}
			}
			BlockKind::DefinitionList(items) => {
				for item in items {
					for blocks in &item.definitions {
						for b in blocks {
							b.for_each_code_block(visit);
						}
					}
				}
			}
			BlockKind::Figure { blocks, .. } => {
				for b in blocks {
					b.for_each_code_block(visit);
				}
			}
			BlockKind::Paragraph(_)
			| BlockKind::Heading { .. }
			| BlockKind::Rule => {}
		}
	}

	/// The state a `<details>` element with `id` declared in its source,
	/// searching the containers it may be nested in.
	pub fn details_declared(&self, id: BlockId) -> Option<bool> {
		match &self.kind {
			BlockKind::Details {
				default_open,
				blocks,
				..
			}
			| BlockKind::FrontMatter {
				default_open,
				blocks,
				..
			} => {
				if self.id == id {
					return Some(*default_open);
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
			BlockKind::Table(table) => {
				table.cell_blocks().find_map(|b| b.details_declared(id))
			}
			BlockKind::DefinitionList(items) => items.iter().find_map(|item| {
				item.definitions.iter().find_map(|blocks| {
					blocks.iter().find_map(|b| b.details_declared(id))
				})
			}),
			BlockKind::Figure { blocks, .. } => {
				blocks.iter().find_map(|b| b.details_declared(id))
			}
			BlockKind::Paragraph(_)
			| BlockKind::Heading { .. }
			| BlockKind::Code { .. }
			| BlockKind::Rule => None,
		}
	}
}

/// Semantic cache identity of a block kind, excluding source positions.
#[cfg_attr(
	any(),
	markview::allow_complexity(
		reason = "Keep the exhaustive block-kind cache identity together; every structural field must be assessed in one place."
	)
)]
pub fn semantic_key(kind: &BlockKind) -> ContentKey {
	let mut hash = std::collections::hash_map::DefaultHasher::new();
	std::mem::discriminant(kind).hash(&mut hash);
	// Links with one address are still separate occurrences unless they share
	// an occurrence id, so the normalized grouping belongs to the identity:
	// three neighboring links and one link over three runs must not share
	// geometry.
	let rich = |text: &RichText| {
		let mut hash = std::collections::hash_map::DefaultHasher::new();
		let mut links = HashMap::new();
		let mut notes = HashMap::new();
		text.len().hash(&mut hash);
		for inline in text {
			inline.style.hash(&mut hash);
			inline.link.as_ref().map(Link::hash_key).hash(&mut hash);
			inline
				.link
				.as_ref()
				.map(|link| {
					let next = links.len();
					*links.entry(link.occurrence()).or_insert(next)
				})
				.hash(&mut hash);
			if let InlineKind::FootnoteRef(reference) = &inline.kind {
				let next = notes.len();
				let occurrence =
					*notes.entry(reference.occurrence).or_insert(next);
				std::mem::discriminant(&inline.kind).hash(&mut hash);
				(&reference.definition, reference.number, occurrence)
					.hash(&mut hash);
			} else {
				inline.kind.hash(&mut hash);
			}
		}
		hash.finish()
	};
	let children = |blocks: &[Block]| {
		let mut hash = std::collections::hash_map::DefaultHasher::new();
		blocks.len().hash(&mut hash);
		for block in blocks {
			block.content_key.hash(&mut hash);
		}
		hash.finish()
	};
	match kind {
		BlockKind::Paragraph(t) => rich(t).hash(&mut hash),
		BlockKind::Heading {
			level,
			text,
			anchor,
		} => (level, rich(text), anchor).hash(&mut hash),
		BlockKind::Code { language, text } => (language, text).hash(&mut hash),
		// The YAML and the source default are the identity; the reader's
		// disclosure override is session state, not content.
		BlockKind::FrontMatter {
			default_open,
			blocks,
		} => {
			(default_open, children(blocks)).hash(&mut hash);
		}
		BlockKind::Quote { blocks, .. }
		| BlockKind::Footnote { blocks, .. } => {
			// Hash each container's metadata separately.
			if let BlockKind::Quote {
				callout: Some(callout),
				..
			} = kind
			{
				(
					callout.kind,
					&callout.name,
					callout.title.as_ref().map(rich),
					callout.fold,
				)
					.hash(&mut hash);
			}
			if let BlockKind::Footnote { id, number, .. } = kind {
				(id, number).hash(&mut hash);
			}
			children(blocks).hash(&mut hash);
		}
		BlockKind::List {
			start,
			reversed,
			tight,
			items,
		} => {
			(start, reversed, tight).hash(&mut hash);
			for item in items {
				(item.checked, item.number, children(&item.blocks))
					.hash(&mut hash);
			}
		}
		BlockKind::Details {
			default_open,
			summary,
			blocks,
		} => {
			(default_open, rich(summary), children(blocks)).hash(&mut hash);
		}
		BlockKind::Table(table) => {
			table.caption.as_ref().map(rich).hash(&mut hash);
			table.columns.hash(&mut hash);
			// Group, row and cell boundaries are part of the identity: the
			// same cells split differently are a different table.
			table.groups.len().hash(&mut hash);
			for group in &table.groups {
				(group.kind, group.rows.len()).hash(&mut hash);
				for row in &group.rows {
					row.cells.len().hash(&mut hash);
					for cell in &row.cells {
						(cell.kind, cell.rowspan, cell.colspan, cell.align)
							.hash(&mut hash);
						children(&cell.blocks).hash(&mut hash);
					}
				}
			}
		}
		BlockKind::DefinitionList(items) => {
			// Item, term and definition boundaries are part of the identity:
			// the same text grouped differently is a different list.
			items.len().hash(&mut hash);
			for item in items {
				item.terms.len().hash(&mut hash);
				for terms in &item.terms {
					rich(terms).hash(&mut hash);
				}
				item.definitions.len().hash(&mut hash);
				for blocks in &item.definitions {
					blocks.len().hash(&mut hash);
					children(blocks).hash(&mut hash);
				}
			}
		}
		BlockKind::Figure { blocks, caption } => {
			caption.as_ref().map(rich).hash(&mut hash);
			children(blocks).hash(&mut hash);
		}
		BlockKind::Rule => {}
	}
	ContentKey(hash.finish())
}

#[cfg(test)]
mod proptests;
