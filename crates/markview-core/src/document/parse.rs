//! Semantic Markdown; raw HTML is limited to a small supported subset.
use crate::html;
use comrak::{
	Arena, Options,
	nodes::{AstNode, ListType, NodeValue, TableAlignment},
	parse_document,
};
use std::{
	collections::{HashMap, hash_map::DefaultHasher},
	hash::{Hash, Hasher},
	ops::Range,
	sync::Arc,
};

use super::{
	Anchors, Block, BlockKind, CellAlign, Document, Inline, InlineKind,
	ListItem, RichText, TextStyle, fingerprint, plain_text, semantic_key,
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
		let mut blocks = Vec::new();
		for child in node.children() {
			let source = self.range(child);
			let data = child.data.borrow();
			let kind = if depth >= self.limits.block_depth {
				BlockKind::Code {
					language: "nested Markdown".into(),
					text: self.source[source.clone()].to_string(),
				}
			} else {
				match &data.value {
					NodeValue::Paragraph => {
						BlockKind::Paragraph(self.rich(child))
					}
					NodeValue::Heading(h) => {
						let text = self.rich(child);
						let anchor = self.anchors.unique(&plain_text(&text));
						BlockKind::Heading {
							level: h.level,
							text,
							anchor,
						}
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
						html::Block::Empty => continue,
						html::Block::Rule => BlockKind::Rule,
						html::Block::Heading { level, text } => {
							let text = html_rich(text, &source);
							let anchor =
								self.anchors.unique(&plain_text(&text));
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
							.map(|r| {
								r.children().map(|c| self.rich(c)).collect()
							})
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
						blocks.extend(self.blocks(child, depth + 1));
						continue;
					}
				}
			};
			// Content identity deliberately excludes source offsets, which shift on append/insert.
			let id = fingerprint(&(
				std::mem::discriminant(&kind),
				&self.source[source.clone()],
			));
			let content_key = semantic_key(&kind);
			blocks.push(Block {
				id,
				content_key,
				source,
				kind,
			});
		}
		blocks
	}
}

pub fn parse(source: impl Into<Arc<str>>) -> Document {
	let source = source.into();
	let mut options = Options::default();
	options.extension.table = true;
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
	let arena = Arena::new();
	let root = parse_document(&arena, &source, &options);
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
	let mut reader = Reader {
		source: &source,
		lines,
		footnotes,
		footnote_column,
		anchors: Anchors::default(),
		limits: crate::limits::Limits::default(),
	};
	let blocks = reader.blocks(root, 0);
	let mut hasher = DefaultHasher::new();
	for block in &blocks {
		block.content_key.hash(&mut hasher);
	}
	Document {
		source,
		blocks,
		content_id: hasher.finish(),
	}
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
