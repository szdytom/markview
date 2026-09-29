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
				let source = self.range(node);
				let text_map = text_map(&self.source[source.clone()], &text);
				out.push(Inline {
					kind: InlineKind::Text(text),
					style: style.clone(),
					source,
					text_map,
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
				let source = self.range(child);
				let text_map = match &kind {
					InlineKind::Text(text) => {
						text_map(&self.source[source.clone()], text)
					}
					_ => Vec::new(),
				};
				out.push(Inline {
					kind,
					style: child_style,
					source,
					text_map,
				});
			} else {
				self.inlines(child, &child_style, out, depth + 1);
			}
		}
	}

	/// Where a fenced block's contents sit inside the block's own source.
	///
	/// The body is searched for rather than counted from the opening fence, so
	/// an info string, an indentation, or a longer fence run cannot put it in
	/// the wrong place. An indented block, whose text is not a slice of its
	/// source at all, keeps the whole block.
	fn body(
		&self,
		source: &Range<usize>,
		literal: &str,
	) -> Option<Range<usize>> {
		let raw = &self.source[source.clone()];
		let after_fence = raw.find('\n').map_or(0, |at| at + 1);
		raw[after_fence..].find(literal).map(|at| {
			let start = source.start + after_fence + at;
			start..start + literal.len()
		})
	}

	/// The runs of a code block's literal, as (reading, source) ranges within
	/// the block.
	///
	/// A fence's contents are indented to whatever contains it, and an indented
	/// block is its own source with the indentation removed, so the literal is
	/// not a slice of the block. The runs are the pieces that are byte for byte
	/// the same; each is linear, and together they map a cluster back to its
	/// bytes even when lines do not sit where their text does.
	fn code_runs(
		&self,
		source: &Range<usize>,
		literal: &str,
		from: usize,
	) -> Vec<(Range<usize>, Range<usize>)> {
		let raw = self.source[source.clone()].as_bytes();
		let mut runs = Vec::new();
		let mut at = from;
		let mut reading = 0;
		// Each literal line is matched inside one source line, in order. A
		// whole-block search would happily land on a fence's language or a
		// container's marker, which read the same as the body they precede.
		for line in literal.split_inclusive('\n') {
			let content = line.trim_end_matches('\n');
			if at > raw.len() {
				return Vec::new();
			}
			let rest = &raw[at..];
			let end = rest.iter().position(|byte| *byte == b'\n');
			let line_end = end.map_or(rest.len(), |at| at + 1);
			if content.is_empty() {
				// A blank literal line still occupies its own source line.
				reading += line.len();
				at += line_end;
				continue;
			}
			// The literal line is the tail of the source line: whatever a
			// container prefix or the code's own indentation stripped sits in
			// front of it, so matching from the right cannot land on it.
			let body = rest[..line_end]
				.strip_suffix(b"\n")
				.unwrap_or(&rest[..line_end]);
			if !body.ends_with(content.as_bytes()) {
				return Vec::new();
			}
			let found = body.len() - content.len();
			runs.push((
				reading..reading + content.len(),
				at + found..at + found + content.len(),
			));
			at += line_end;
			reading += line.len();
		}
		runs
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
				body: source.clone(),
				runs: Vec::new(),
			}
		} else {
			match &data.value {
				NodeValue::FrontMatter(text) => {
					match front_matter::parse(text) {
						front_matter::Content::Empty => {
							return Child::Skip;
						}
						front_matter::Content::Source(yaml) => {
							let start = source.start
								+ self.source[source.clone()]
									.find('\n')
									.map_or(0, |at| at + 1);
							let body = start..start + yaml.len();
							let code = BlockKind::Code {
								body,
								runs: Vec::new(),
								language: front_matter::LANGUAGE.into(),
								text: yaml,
							};
							let id = fingerprint(&(
								std::mem::discriminant(&code),
								&self.source[source.clone()],
							));
							BlockKind::FrontMatter {
								open: false,
								blocks: vec![Block {
									id,
									content_key: semantic_key(
										&code,
										source.start,
									),
									source: source.clone(),
									kind: code,
								}],
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
						text_map: Vec::new(),
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
						text_map: Vec::new(),
					}])
				}
				NodeValue::CodeBlock(c) => {
					let body = self.body(&source, &c.literal);
					// Without a whole-literal match the search still has to
					// skip the opening fence's own line. Whether there is one
					// is the parser's answer: an indented block whose first
					// line happens to read like a fence is still indented.
					let from = body.as_ref().map_or_else(
						|| {
							if c.fenced {
								self.source[source.clone()]
									.find('\n')
									.map_or(0, |at| at + 1)
							} else {
								0
							}
						},
						|body| body.start - source.start,
					);
					BlockKind::Code {
						language: c.info.clone(),
						runs: self.code_runs(&source, &c.literal, from),
						body: body.unwrap_or_else(|| source.clone()),
						text: c.literal.clone(),
					}
				}
				NodeValue::HtmlBlock(h) => match html::block(&h.literal) {
					html::Block::Unsupported => BlockKind::Code {
						language: "HTML source".into(),
						text: h.literal.clone(),
						body: source.clone(),
						runs: Vec::new(),
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
		let content_key = semantic_key(&kind, source.start);
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
			let trimmed = text.trim();
			out.push(Inline {
				kind: InlineKind::Text(trimmed.to_string()),
				style: TextStyle::default(),
				source: source.clone(),
				text_map: text_map(&self.source[source.clone()], trimmed),
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
			content_key: semantic_key(&kind, source.start),
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
				// A span is a piece of a larger fragment, so no interior
				// boundary can be claimed.
				text_map: Vec::new(),
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
			&& last.source.end <= span.source.start
			&& let InlineKind::Text(prev) = &mut last.kind
		{
			let collapse = prev.ends_with(char::is_whitespace)
				&& t.starts_with(char::is_whitespace);
			let keep = if collapse {
				prev.trim_end().len()
			} else {
				prev.len()
			};
			let skip = if collapse {
				t.len() - t.trim_start().len()
			} else {
				0
			};
			let left_end = map_boundary(
				&last.text_map,
				prev.len(),
				last.source.len(),
				keep,
			);
			if last.text_map.is_empty() {
				last.text_map.push((0, 0));
			}
			last.text_map.retain(|(reading, _)| *reading < keep);
			last.text_map.push((keep, left_end));
			prev.truncate(keep);
			if collapse {
				prev.push(' ');
			}
			// A zero-length reading run preserves the gap left by a dropped tag.
			last.text_map.push((prev.len(), last.source.len()));
			let base = span.source.start - last.source.start;
			let right_start =
				map_boundary(&span.text_map, t.len(), span.source.len(), skip);
			last.text_map.push((prev.len(), base + right_start));
			last.text_map.extend(
				span.text_map
					.iter()
					.filter(|(reading, _)| *reading > skip)
					.map(|(reading, source)| {
						(prev.len() + reading - skip, base + source)
					}),
			);
			prev.push_str(&t[skip..]);
			last.source.end = span.source.end;
			merged = true;
		}
		if !merged {
			out.push(span);
		}
	}
	out
}

/// Maps a boundary inside a run, retaining a covering range for decoded units.
fn map_boundary(
	map: &[(usize, usize)],
	reading_len: usize,
	source_len: usize,
	at: usize,
) -> usize {
	if at == reading_len {
		return source_len;
	}
	let index = map.partition_point(|(reading, _)| *reading <= at);
	let (reading, source) =
		index.checked_sub(1).map_or((0, 0), |index| map[index]);
	let (next_reading, next_source) =
		map.get(index).copied().unwrap_or((reading_len, source_len));
	if next_reading - reading == next_source - source {
		source + at - reading
	} else {
		source
	}
}

/// Where each reading run of a decoded text node begins in the source.
///
/// Comrak decodes entity references and backslash escapes, so a text node's
/// reading text is not a slice of the document. Runs whose reading and source
/// lengths agree map offset for offset, and only the boundaries between them
/// need recording. An empty list means the whole node is one run, which is
/// also the answer when the text does not align at all: one covering run is
/// then the only mapping the source supports.
fn text_map(source: &str, text: &str) -> Vec<(usize, usize)> {
	let mut runs: Vec<(usize, usize)> = Vec::new();
	let mut linear: Option<(usize, usize)> = None;
	let mut offset = 0;
	let mut reading = 0;
	while offset < source.len() && reading < text.len() {
		let (source_span, reading_span) =
			source_unit(source, offset, &text[reading..]);
		if source_span == reading_span {
			linear.get_or_insert((reading, offset));
		} else {
			if let Some(start) = linear.take() {
				runs.push(start);
			}
			runs.push((reading, offset));
		}
		offset += source_span;
		reading += reading_span;
	}
	if let Some(start) = linear {
		runs.push(start);
	}
	// Anything left over means this text is not this source decoded, so no
	// interior boundary can be claimed.
	if offset != source.len() || reading != text.len() || runs.len() < 2 {
		return Vec::new();
	}
	runs
}

/// How many source bytes, and how many reading bytes, the text at `reading`
/// was decoded from.
///
/// An entity reference may decode to more than one character, so the whole
/// decoded sequence is matched rather than its first character. A `&` that
/// does not decode to what is being read is a literal ampersand, and a
/// backslash only escapes the punctuation after it.
fn source_unit(source: &str, offset: usize, reading: &str) -> (usize, usize) {
	let rest = &source[offset..];
	// The parser's own rule is mirrored rather than approximated, so the two
	// always agree on what an entity spells.
	if rest.starts_with('&')
		&& let Some((decoded, span)) = entity(rest)
		&& reading.starts_with(&decoded)
	{
		return (span, decoded.len());
	}
	if let Some(escaped) = rest
		.strip_prefix('\\')
		.and_then(|after| after.chars().next())
		&& escaped.is_ascii_punctuation()
		&& reading.starts_with(escaped)
	{
		return (1 + escaped.len_utf8(), escaped.len_utf8());
	}
	// Neither side is assumed to advance alike: when a source unit and what it
	// decodes to differ, this step is simply not a 1:1 run, and the caller
	// stops claiming interior boundaries from here on.
	let source_span = rest.chars().next().map_or(1, char::len_utf8);
	let reading_span = reading.chars().next().map_or(1, char::len_utf8);
	(source_span, reading_span)
}

/// The entity `text` begins with, and how many bytes of it the entity took.
///
/// This mirrors the parser's decoder: a numeric reference needs the right
/// number of digits and a terminating semicolon, and a named one ends at the
/// first semicolon, which must come before any space. Names come from the same
/// table the parser builds its own from, so the two cannot disagree.
fn entity(text: &str) -> Option<(String, usize)> {
	let bytes = text.as_bytes();
	if text.len() >= 4 && bytes[1] == b'#' {
		let (radix, start) = if bytes[2] == b'x' || bytes[2] == b'X' {
			(16u32, 3)
		} else {
			(10u32, 2)
		};
		let limit = if radix == 16 { 6 } else { 7 };
		let mut digits = 0;
		let mut codepoint = 0u32;
		let mut at = start;
		while at < bytes.len() {
			let Some(digit) = (bytes[at] as char).to_digit(radix) else {
				break;
			};
			codepoint = (codepoint * radix + digit).min(0x11_0000);
			digits += 1;
			at += 1;
		}
		if at < bytes.len()
			&& bytes[at] == b';'
			&& (1..=limit).contains(&digits)
		{
			// The parser substitutes the replacement character for anything
			// that is not a scalar value.
			let codepoint = if codepoint == 0
				|| (0xD800..=0xE000).contains(&codepoint)
				|| codepoint >= 0x110000
			{
				0xFFFD
			} else {
				codepoint
			};
			let character = char::from_u32(codepoint).unwrap_or('\u{FFFD}');
			return Some((character.to_string(), at + 1));
		}
	}
	for at in 2..text.len().min(32) {
		if bytes[at] == b' ' {
			return None;
		}
		if bytes[at] == b';' {
			let name = &text[..=at];
			return entities::ENTITIES
				.iter()
				.find(|entry| entry.entity == name)
				.map(|entry| (entry.characters.to_owned(), at + 1));
		}
	}
	None
}
