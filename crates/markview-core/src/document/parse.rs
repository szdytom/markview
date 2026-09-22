//! Semantic Markdown; raw HTML is limited to a small supported subset.
use crate::html;
use comrak::{
	Arena, Options,
	nodes::{AstNode, ListType, NodeValue, TableAlignment},
	parse_document,
};
use std::{collections::HashMap, ops::Range, sync::Arc};

use super::{
	Anchors, Block, BlockKind, CellAlign, Document, Inline, InlineKind,
	ListItem, RichText, TextStyle, content_identity, fingerprint, front_matter,
	incremental, plain_text, semantic_key,
};
struct Reader<'s> {
	source: &'s str,
	lines: Vec<usize>,
	footnotes: HashMap<String, u32>,
	/// Digit columns every note reserves for its number; see
	/// [`BlockKind::Footnote`].
	footnote_column: u32,
	/// Heading anchors already used by this document, in reading order.
	anchors: Anchors,
	/// How many `<details>` elements this document has already numbered.
	details_ordinal: u32,
	/// The document's reference and footnote definitions, which resolve inside
	/// a snippet the same way they do in a full parse.
	definitions: &'s str,
	limits: crate::limits::Limits,
}

impl Reader<'_> {
	fn range(&self, node: &AstNode<'_>) -> Range<usize> {
		let p = node.data.borrow().sourcepos;
		let start = self
			.lines
			.get(p.start.line.saturating_sub(1))
			.copied()
			.unwrap_or(0)
			+ p.start.column.saturating_sub(1);
		let end = self
			.lines
			.get(p.end.line.saturating_sub(1))
			.copied()
			.unwrap_or(0)
			+ p.end.column;
		// Comrak columns are byte offsets unless sourcepos_chars is enabled.
		let mut start = start.min(self.source.len());
		let mut end = end.min(self.source.len()).max(start);
		while !self.source.is_char_boundary(start) {
			start -= 1;
		}
		while !self.source.is_char_boundary(end) {
			end += 1;
		}
		start..end
	}

	fn inlines<'a>(
		&self,
		node: &'a AstNode<'a>,
		style: &TextStyle,
		out: &mut RichText,
		depth: usize,
	) {
		// Comrak builds the AST iteratively but produces recursion as deep as
		// the input demands; past the budget the remaining text is kept flat
		// instead of descending.
		if depth >= self.limits.inline_depth {
			let text = self.flattened(node);
			if !text.is_empty() {
				out.push(Inline {
					kind: InlineKind::Text(text),
					style: style.clone(),
					source: self.range(node),
				});
			}
			return;
		}
		// Raw HTML tags are siblings, so a supported tag opens a style scope
		// that the matching closing tag ends; unsupported markup stays source.
		let mut style = style.clone();
		let mut scopes: Vec<(String, TextStyle)> = Vec::new();
		for child in node.children() {
			let mut child_style = style.clone();
			let value = child.data.borrow();
			let kind = match &value.value {
				NodeValue::Text(t) => Some(InlineKind::Text(t.to_string())),
				NodeValue::SoftBreak => Some(InlineKind::Text(" ".into())),
				NodeValue::LineBreak => {
					Some(InlineKind::LineBreak { justify: false })
				}
				NodeValue::Code(c) => {
					child_style.code = true;
					Some(InlineKind::Text(c.literal.clone()))
				}
				NodeValue::Raw(t) => {
					child_style.code = true;
					Some(InlineKind::Text(t.clone()))
				}
				NodeValue::HtmlInline(t) => match html::inline(t) {
					html::Inline::Image(image) => {
						Some(InlineKind::Image(image))
					}
					html::Inline::Ignore => continue,
					html::Inline::Break => {
						Some(InlineKind::LineBreak { justify: true })
					}
					html::Inline::Open { name, patch } => {
						scopes.push((name, style.clone()));
						apply_patch(&patch, &mut style);
						continue;
					}
					html::Inline::Close { name } => {
						if let Some(i) =
							scopes.iter().rposition(|(open, _)| *open == name)
						{
							style = scopes[i].1.clone();
							scopes.truncate(i);
						}
						continue;
					}
					html::Inline::Literal => {
						child_style.code = true;
						Some(InlineKind::Text(t.clone()))
					}
				},
				NodeValue::Math(m) => Some(InlineKind::Math {
					latex: m.literal.clone(),
					display: m.display_math,
				}),
				NodeValue::FootnoteReference(f) => {
					let label = f.ix.to_string();
					child_style.superscript = true;
					child_style.footnote_ref = true;
					child_style.link = Some(super::footnote::url(&label));
					Some(InlineKind::FootnoteRef(f.ix))
				}
				NodeValue::Strong => {
					child_style.bold = true;
					None
				}
				NodeValue::Emph => {
					child_style.italic = true;
					None
				}
				NodeValue::Strikethrough => {
					child_style.strike = true;
					None
				}
				NodeValue::Link(l) => {
					child_style.link = Some(l.url.clone());
					None
				}
				NodeValue::Image(link) => {
					let mut alt = Vec::new();
					self.inlines(
						child,
						&TextStyle::default(),
						&mut alt,
						depth + 1,
					);
					let alt = plain_text(&alt);
					Some(InlineKind::Image(crate::image::ImageSpec {
						src: link.url.clone(),
						alt,
						title: link.title.clone(),
						width: None,
						height: None,
					}))
				}
				_ => None,
			};
			if let Some(kind) = kind {
				out.push(Inline {
					kind,
					style: child_style,
					source: self.range(child),
				});
			} else {
				self.inlines(child, &child_style, out, depth + 1);
			}
		}
	}

	/// The readable text of a subtree, collected without recursion.
	fn flattened<'a>(&self, node: &'a AstNode<'a>) -> String {
		let mut text = String::new();
		for descendant in node.descendants() {
			match &descendant.data.borrow().value {
				NodeValue::Text(t) => text.push_str(t),
				NodeValue::Raw(t) => text.push_str(t),
				NodeValue::Code(c) => text.push_str(&c.literal),
				NodeValue::Math(m) => text.push_str(&m.literal),
				NodeValue::SoftBreak => text.push(' '),
				NodeValue::LineBreak => text.push('\n'),
				_ => {}
			}
		}
		text
	}

	fn rich<'a>(&self, node: &'a AstNode<'a>) -> RichText {
		let mut text = Vec::new();
		self.inlines(node, &TextStyle::default(), &mut text, 0);
		merge_text(text)
	}

	fn blocks<'a>(
		&mut self,
		node: &'a AstNode<'a>,
		depth: usize,
	) -> Vec<Block> {
		let children: Vec<&'a AstNode<'a>> = node.children().collect();
		self.sequence(&children, depth)
	}

	/// One sibling list. A `<details>` element may span several siblings, so
	/// the scan is index-based rather than one child at a time.
	fn sequence<'a>(
		&mut self,
		children: &[&'a AstNode<'a>],
		depth: usize,
	) -> Vec<Block> {
		let mut blocks = Vec::new();
		let mut i = 0;
		while i < children.len() {
			if let Some(consumed) =
				self.details(children, i, depth, &mut blocks)
			{
				i += consumed;
				continue;
			}
			match self.child_block(children[i], depth) {
				Child::Block(block) => blocks.push(block),
				Child::Skip => {}
				Child::Flatten => {
					blocks.extend(self.blocks(children[i], depth + 1));
				}
			}
			i += 1;
		}
		blocks
	}

	fn child_block<'a>(
		&mut self,
		child: &'a AstNode<'a>,
		depth: usize,
	) -> Child {
		let source = self.range(child);
		let data = child.data.borrow();
		let kind = if depth >= self.limits.block_depth {
			BlockKind::Code {
				language: "nested Markdown".into(),
				text: self.source[source.clone()].to_string(),
			}
		} else {
			match &data.value {
				NodeValue::FrontMatter(text) => {
					match front_matter::parse(text, &source) {
						front_matter::Content::Empty => {
							return Child::Skip;
						}
						front_matter::Content::Table(table, yaml) => {
							BlockKind::FrontMatter {
								table: Some(table),
								text: yaml,
							}
						}
						front_matter::Content::Source(yaml) => {
							BlockKind::FrontMatter {
								table: None,
								text: yaml,
							}
						}
					}
				}
				NodeValue::Paragraph => BlockKind::Paragraph(self.rich(child)),
				NodeValue::Heading(h) => {
					let text = self.rich(child);
					let anchor = self.anchors.unique(&plain_text(&text));
					BlockKind::Heading {
						level: h.level,
						text,
						anchor,
					}
				}
				// The info string's first word is the language; trailing
				// words are metadata, so `mermaid title="x"` still renders.
				NodeValue::CodeBlock(c)
					if c.info.split_whitespace().next() == Some("mermaid") =>
				{
					BlockKind::Paragraph(vec![Inline {
						kind: InlineKind::Image(crate::image::ImageSpec {
							src: crate::image::mermaid_source(&c.literal),
							// An empty `alt` draws no caption and keeps the
							// fence source out of the reading text, so only
							// the placeholder message is selectable.
							alt: String::new(),
							title: String::new(),
							width: None,
							height: None,
						}),
						style: TextStyle::default(),
						source: source.clone(),
					}])
				}
				NodeValue::CodeBlock(c) if c.info.trim() == "math" => {
					BlockKind::Paragraph(vec![Inline {
						kind: InlineKind::Math {
							latex: c.literal.clone(),
							display: true,
						},
						style: TextStyle::default(),
						source: source.clone(),
					}])
				}
				NodeValue::CodeBlock(c) => BlockKind::Code {
					language: c.info.clone(),
					text: c.literal.clone(),
				},
				NodeValue::HtmlBlock(h) => match html::block(&h.literal) {
					html::Block::Unsupported => BlockKind::Code {
						language: "HTML source".into(),
						text: h.literal.clone(),
					},
					html::Block::Empty => return Child::Skip,
					html::Block::Rule => BlockKind::Rule,
					html::Block::Heading { level, text } => {
						let text = html_rich(text, &source);
						let anchor = self.anchors.unique(&plain_text(&text));
						BlockKind::Heading {
							level,
							text,
							anchor,
						}
					}
					html::Block::Paragraph(text) => {
						BlockKind::Paragraph(html_rich(text, &source))
					}
				},
				NodeValue::ThematicBreak => BlockKind::Rule,
				NodeValue::BlockQuote => BlockKind::Quote {
					label: None,
					blocks: self.blocks(child, depth + 1),
				},
				NodeValue::Alert(a) => BlockKind::Quote {
					label: Some(format!("{:?}", a.alert_type)),
					blocks: self.blocks(child, depth + 1),
				},
				NodeValue::List(l) => BlockKind::List {
					start: (l.list_type == ListType::Ordered)
						.then_some(l.start),
					tight: l.tight,
					items: child
						.children()
						.map(|item| {
							let checked = match &item.data.borrow().value {
								NodeValue::TaskItem(t) => {
									Some(t.symbol.is_some())
								}
								_ => None,
							};
							ListItem {
								checked,
								blocks: self.blocks(item, depth + 1),
							}
						})
						.collect(),
				},
				NodeValue::Table(t) => BlockKind::Table {
					align: t
						.alignments
						.iter()
						.map(|a| match a {
							TableAlignment::Center => CellAlign::Center,
							TableAlignment::Right => CellAlign::Right,
							_ => CellAlign::Left,
						})
						.collect(),
					rows: child
						.children()
						.map(|r| r.children().map(|c| self.rich(c)).collect())
						.collect(),
				},
				NodeValue::FootnoteDefinition(f) => BlockKind::Footnote {
					label: self
						.footnotes
						.get(&f.name)
						.map_or_else(|| f.name.clone(), u32::to_string),
					column: self.footnote_column,
					blocks: self.blocks(child, depth + 1),
				},
				_ => {
					return Child::Flatten;
				}
			}
		};
		// Content identity deliberately excludes source offsets, which shift on append/insert.
		let id = fingerprint(&(
			std::mem::discriminant(&kind),
			&self.source[source.clone()],
		));
		let content_key = semantic_key(&kind);
		Child::Block(Block {
			id,
			content_key,
			source,
			kind,
		})
	}

	/// Consumes a `<details>` element that starts at `children[start]`, when
	/// the whole element is present. Appends one block and returns how many
	/// siblings it owns; `None` leaves the opener as ordinary raw HTML.
	fn details<'a>(
		&mut self,
		children: &[&'a AstNode<'a>],
		start: usize,
		depth: usize,
		out: &mut Vec<Block>,
	) -> Option<usize> {
		if depth >= self.limits.block_depth {
			return None;
		}
		let literal = match &children[start].data.borrow().value {
			NodeValue::HtmlBlock(h) => h.literal.clone(),
			_ => return None,
		};
		match html::details(&literal) {
			// The whole element is in this block; its body is Markdown. Comrak
			// can keep adjacent elements in one block, so the remainder is read
			// iteratively at this depth: those elements are siblings, not
			// children, and must not spend the nesting budget.
			html::Details::Inline {
				mut open,
				mut summary,
				mut body,
				mut rest,
			} => {
				let source = self.range(children[start]);
				loop {
					let blocks = self.markdown_blocks(&body, depth + 1);
					let rich = self.summary_rich(
						summary.as_deref(),
						depth + 1,
						&source,
					);
					out.push(self.details_block(
						open,
						rich,
						blocks,
						source.clone(),
					));
					if rest.trim().is_empty() {
						break;
					}
					let html::Details::Inline {
						open: next_open,
						summary: next_summary,
						body: next_body,
						rest: next_rest,
					} = html::details(&rest)
					else {
						out.extend(self.markdown_blocks(&rest, depth));
						break;
					};
					(open, summary, body, rest) =
						(next_open, next_summary, next_body, next_rest);
				}
				Some(1)
			}
			// The opener ends at a blank line, so the element owns the source
			// up to its closing tag, whether that tag shares its block with
			// further tags or not.
			html::Details::Open {
				open,
				summary,
				lead,
				depth: open_depth,
			} => {
				let (close, tag) = details_close(children, start, open_depth)?;
				let start_source = self.range(children[start]);
				let close_source = self.range(children[close]);
				// The body is the source between the opener and the closing
				// tag: a nested element that shares that closing block is
				// parsed from the inside out, so none of its content is lost.
				let (between, prefix, rest) = {
					let data = children[close].data.borrow();
					let NodeValue::HtmlBlock(h) = &data.value else {
						return None;
					};
					let between = self
						.source
						.get(start_source.end..close_source.start)
						.unwrap_or_default();
					if tag.end > h.literal.len() {
						return None;
					}
					// The literals around the body have already lost the
					// enclosing quote markers, so the raw slice between them
					// must lose the same ones or the body gains a quote.
					let quotes =
						enclosing_quotes(self.source, start_source.start);
					(
						strip_blockquotes(between, quotes),
						h.literal[..tag.start].to_string(),
						h.literal[tag.end..].to_string(),
					)
				};
				let mut body = lead;
				body.push('\n');
				body.push_str(&between);
				body.push_str(&prefix);
				let source = start_source.start..close_source.start + tag.end;
				let blocks = self.markdown_blocks(&body, depth + 1);
				let summary =
					self.summary_rich(summary.as_deref(), depth + 1, &source);
				// Content after the closing tag is a sibling of the element,
				// so it keeps its place instead of being dropped with the
				// block that carries the tag.
				out.push(self.details_block(open, summary, blocks, source));
				if !rest.trim().is_empty() {
					out.extend(self.markdown_blocks(&rest, depth));
				}
				Some(close - start + 1)
			}
			html::Details::Close | html::Details::No => None,
		}
	}

	/// The block list `text` describes, parsed by the ordinary pipeline. The
	/// shared anchors keep headings inside the snippet unique in the document.
	fn markdown_blocks(&mut self, text: &str, depth: usize) -> Vec<Block> {
		if text.trim().is_empty() {
			return Vec::new();
		}
		// A snippet only holds part of the document, so a reference or note it
		// uses may be defined outside it; parsing it together with the
		// document's definitions resolves those, and only the blocks the
		// snippet itself covers are kept.
		if self.definitions.is_empty() || !text.contains('[') {
			return self.snippet(text, depth);
		}
		let mut joined =
			String::with_capacity(text.len() + self.definitions.len() + 2);
		joined.push_str(text);
		joined.push_str("\n\n");
		joined.push_str(self.definitions);
		let blocks = self.snippet(&joined, depth);
		// An unclosed fence or HTML block can swallow the appended
		// definitions; then the bare snippet parses to what the full document
		// puts there.
		if blocks
			.iter()
			.any(|b| b.source.start < text.len() && b.source.end > text.len())
		{
			return self.snippet(text, depth);
		}
		blocks
			.into_iter()
			.filter(|b| b.source.start < text.len())
			.collect()
	}

	/// `text` parsed on its own by the ordinary pipeline.
	fn snippet(&mut self, text: &str, depth: usize) -> Vec<Block> {
		let arena = Arena::new();
		let root = parse_document(&arena, text, &markdown_options());
		// A note keeps the number the document gave it, so a reference inside
		// a snippet and the note block outside it still agree.
		for node in root.descendants() {
			if let NodeValue::FootnoteReference(f) =
				&mut node.data.borrow_mut().value
				&& let Some(ix) = self.footnotes.get(&f.name)
			{
				f.ix = *ix;
			}
		}
		let mut lines = vec![0];
		lines.extend(text.match_indices('\n').map(|(i, _)| i + 1));
		let mut reader = Reader {
			source: text,
			lines,
			footnotes: std::mem::take(&mut self.footnotes),
			footnote_column: self.footnote_column,
			anchors: std::mem::take(&mut self.anchors),
			details_ordinal: std::mem::take(&mut self.details_ordinal),
			definitions: self.definitions,
			limits: self.limits,
		};
		let blocks = reader.blocks(root, depth);
		self.anchors = reader.anchors;
		self.footnotes = reader.footnotes;
		self.details_ordinal = reader.details_ordinal;
		blocks
	}

	/// The summary's rich text: its Markdown inline content, or its plain text
	/// when it is not phrasing content.
	fn summary_rich(
		&mut self,
		text: Option<&str>,
		depth: usize,
		source: &Range<usize>,
	) -> RichText {
		let Some(text) = text.filter(|text| !text.trim().is_empty()) else {
			return RichText::new();
		};
		let mut out = RichText::new();
		// The summary lives inside an HTML block, so the document never numbers
		// the notes it references; parsing it alone keeps the two in step.
		for block in self.snippet(text, depth) {
			if let BlockKind::Paragraph(rich) = block.kind {
				out.extend(rich);
			}
		}
		if out.is_empty() {
			out.push(Inline {
				kind: InlineKind::Text(text.trim().to_string()),
				style: TextStyle::default(),
				source: source.clone(),
			});
		}
		// The summary has no sub-range of its own in the document; like a raw
		// HTML block, every run is attributed to the whole element.
		for inline in &mut out {
			inline.source = source.clone();
		}
		merge_text(out)
	}

	fn details_block(
		&mut self,
		open: bool,
		summary: RichText,
		blocks: Vec<Block>,
		source: Range<usize>,
	) -> Block {
		let ordinal = self.details_ordinal;
		self.details_ordinal += 1;
		let kind = BlockKind::Details {
			open,
			ordinal,
			summary,
			blocks,
		};
		// The occurrence ordinal distinguishes two identical elements, whose
		// source text alone would fingerprint the same; `content_key` still
		// ignores it, so matching states share geometry.
		let id = fingerprint(&(
			std::mem::discriminant(&kind),
			&self.source[source.clone()],
			ordinal,
		));
		Block {
			id,
			content_key: semantic_key(&kind),
			source,
			kind,
		}
	}
}

/// What one AST child contributes to its parent's block list.
enum Child {
	/// A finished block.
	Block(Block),
	/// Nothing readable, such as an empty HTML block.
	Skip,
	/// A container that only groups its children, which take its place.
	Flatten,
}

/// The sibling where an opening element's `</details>` appears, as its index
/// and the closing tag's byte range within that sibling's literal. Tags are
/// counted individually, so a block that carries several closing tags closes
/// several elements. `depth` is how many elements the opening block already
/// left open, so an inner opener there does not match the outer close. `None`
/// leaves the opener as literal source.
fn details_close<'a>(
	children: &[&'a AstNode<'a>],
	start: usize,
	mut depth: usize,
) -> Option<(usize, Range<usize>)> {
	for (i, child) in children.iter().enumerate().skip(start + 1) {
		let data = child.data.borrow();
		let NodeValue::HtmlBlock(h) = &data.value else {
			continue;
		};
		let (next, close) = html::close_tag(&h.literal, depth);
		depth = next;
		if let Some(range) = close {
			return Some((i, range));
		}
	}
	None
}

/// How many block quotes enclose the block that starts at `at`: the `>` markers
/// Comrak removed from the block's first line.
fn enclosing_quotes(source: &str, at: usize) -> usize {
	let line = source[..at.min(source.len())]
		.rfind('\n')
		.map_or(0, |i| i + 1);
	source[line..at.min(source.len())].matches('>').count()
}

/// `text` with the markers of `depth` enclosing block quotes removed from every
/// line, so reparsing it alone does not nest the body in a quote again.
fn strip_blockquotes(text: &str, depth: usize) -> String {
	if depth == 0 {
		return text.to_string();
	}
	let mut out = String::with_capacity(text.len());
	for (i, line) in text.split('\n').enumerate() {
		if i > 0 {
			out.push('\n');
		}
		out.push_str(without_quotes(line, depth));
	}
	out
}

/// One line with up to `depth` block quote markers removed.
fn without_quotes(line: &str, mut depth: usize) -> &str {
	let mut rest = line;
	while depth > 0 {
		let indent = rest.len() - rest.trim_start_matches(' ').len();
		// Four spaces already mean code, not a marker.
		if indent > 3 {
			break;
		}
		let Some(after) = rest[indent..].strip_prefix('>') else {
			break;
		};
		rest = match after.as_bytes().first() {
			Some(b' ' | b'\t') => &after[1..],
			_ => after,
		};
		depth -= 1;
	}
	rest
}

pub fn parse(source: impl Into<Arc<str>>) -> Document {
	let source = source.into();
	let arena = Arena::new();
	let root = parse_document(&arena, &source, &markdown_options());
	let mut lines = vec![0];
	lines.extend(source.match_indices('\n').map(|(i, _)| i + 1));
	let footnotes: HashMap<String, u32> = root
		.descendants()
		.filter_map(|n| match &n.data.borrow().value {
			NodeValue::FootnoteReference(f) => Some((f.name.clone(), f.ix)),
			_ => None,
		})
		.collect();
	let footnote_column = footnotes
		.values()
		.copied()
		.max()
		.unwrap_or(1)
		.to_string()
		.len() as u32;
	let definitions = if incremental::definition_free(&source) {
		String::new()
	} else {
		incremental::definitions(&source)
	};
	let mut reader = Reader {
		source: &source,
		lines,
		footnotes,
		footnote_column,
		anchors: Anchors::default(),
		details_ordinal: 0,
		definitions: &definitions,
		limits: crate::limits::Limits::default(),
	};
	let blocks = reader.blocks(root, 0);
	Document {
		source,
		content_id: content_identity(&blocks),
		blocks,
	}
}

/// The comrak configuration every Markdown parse shares, including the body
/// of a `<details>` element.
fn markdown_options() -> Options<'static> {
	let mut options = Options::default();
	options.extension.table = true;
	// A document may open with `---` fenced YAML metadata. Comrak only splits
	// it off: it hands the block back verbatim, delimiters included, and the
	// YAML itself is read in `front_matter`.
	options.extension.front_matter_delimiter = Some("---".into());
	options.extension.strikethrough = true;
	options.extension.tasklist = true;
	options.extension.autolink = true;
	options.extension.footnotes = true;
	options.extension.alerts = true;
	options.extension.math_dollars = true;
	options.extension.math_latex = true;
	options.extension.math_code = true;
	// CommonMark's flanking rules miss emphasis that ends next to CJK text,
	// as in `**重要です。**但`, where the closing run follows punctuation.
	options.extension.cjk_friendly_emphasis = true;
	options
}

fn apply_patch(patch: &html::Patch, style: &mut TextStyle) {
	match patch {
		html::Patch::Bold => style.bold = true,
		html::Patch::Italic => style.italic = true,
		html::Patch::Strike => style.strike = true,
		html::Patch::Code => style.code = true,
		html::Patch::Superscript => style.superscript = true,
		html::Patch::Link(url) => style.link = Some(url.clone()),
		html::Patch::None => {}
	}
}

fn html_rich(spans: Vec<html::Span>, source: &Range<usize>) -> RichText {
	spans
		.into_iter()
		.map(|span| {
			let mut style = TextStyle::default();
			for patch in &span.styles {
				apply_patch(patch, &mut style);
			}
			Inline {
				kind: span.image.map_or_else(
					|| InlineKind::Text(span.text),
					InlineKind::Image,
				),
				style,
				source: source.clone(),
			}
		})
		.collect()
}

/// Merge neighboring runs that share a style so a dropped comment or tag does
/// not leave a double space behind.
fn merge_text(text: RichText) -> RichText {
	let mut out: RichText = Vec::with_capacity(text.len());
	for span in text {
		let InlineKind::Text(t) = &span.kind else {
			out.push(span);
			continue;
		};
		let mut merged = false;
		if let Some(last) = out.last_mut()
			&& last.style == span.style
			&& let InlineKind::Text(prev) = &mut last.kind
		{
			if prev.ends_with(char::is_whitespace)
				&& t.starts_with(char::is_whitespace)
			{
				let len = prev.trim_end().len();
				prev.truncate(len);
				prev.push(' ');
				prev.push_str(t.trim_start());
			} else {
				prev.push_str(t);
			}
			last.source.end = span.source.end;
			merged = true;
		}
		if !merged {
			out.push(span);
		}
	}
	out
}
