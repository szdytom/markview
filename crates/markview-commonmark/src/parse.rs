//! Semantic Markdown; raw HTML is limited to a small supported subset.
use crate::{
	container::{ContainerHost, ContainerStack, clamp_range, opaque_len},
	html,
	make::{Made, Producer},
	rich::{
		RunStyle as TextStyle, SpannedText, source_link, source_occurrence,
	},
};
use comrak::{
	Arena, Options,
	nodes::{
		AstNode, CalloutFold, ListType, NodeCallout, NodeValue, NodeWikiLink,
		TableAlignment,
	},
	parse_document,
};
use std::{
	borrow::Cow,
	cell::{Cell, RefCell},
	collections::{BTreeMap, HashMap, HashSet},
	ops::Range,
	rc::Rc,
	sync::Arc,
};

use super::{
	Block, BlockKind, Callout, CalloutKind, CellAlign, CellKind, FootnoteId,
	FootnoteReference, Inline, InlineKind, ListItem, MarkdownParse, RichText,
	Table, TableCell, TableColumn, TableGroup, TableGroupKind, TableRow,
	front_matter, incremental, plain_text,
};
use markview_document::{DocumentRevision, SourceSpan};

/// The reading dialect a parse uses: which Markdown extensions are on and
/// how ambiguous source reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dialect {
	/// Single newlines inside a paragraph end their line as a hard break
	/// rather than a space.
	pub hard_breaks: bool,
	/// `==text==` parses as a highlight mark rather than literal text.
	pub highlight_marks: bool,
	/// Obsidian extensions: `> [!type]` callouts read as callouts rather than
	/// GitHub alerts, and raw HTML admits `<sub>`, `<mark>`, `<u>`, `<ins>`
	/// and `<small>`.
	pub ofm: bool,
}

enum InlineStep {
	Emit(InlineKind, TextStyle),
	Descend(TextStyle),
	Skip,
}

/// Style scopes and comment spacing belong to one sibling sequence.
struct InlineFrame {
	style: TextStyle,
	scopes: Vec<(String, TextStyle)>,
	after_comment: bool,
}

impl InlineFrame {
	fn new(style: &TextStyle) -> Self {
		Self {
			style: style.clone(),
			scopes: Vec::new(),
			after_comment: false,
		}
	}

	fn html(
		&mut self,
		text: &str,
		at: usize,
		ofm: bool,
		limit: usize,
		mut style: TextStyle,
	) -> InlineStep {
		let kind = match html::inline(text, ofm) {
			html::Inline::Image(image) => InlineKind::Image(Box::new(image)),
			html::Inline::Ignore => return InlineStep::Skip,
			html::Inline::Break => {
				InlineKind::LineBreak(super::LineBreak::Forced)
			}
			html::Inline::Open { name, patch } => {
				if self.scopes.len() < limit {
					self.scopes.push((name, self.style.clone()));
					apply_patch(&patch, &mut self.style, at);
					return InlineStep::Skip;
				}
				// Past the scope budget, tags stay visible source.
				style.code = true;
				InlineKind::Text(text.to_string())
			}
			html::Inline::Close { name } => {
				if let Some(i) =
					self.scopes.iter().rposition(|(open, _)| *open == name)
				{
					self.style = self.scopes[i].1.clone();
					self.scopes.truncate(i);
				}
				return InlineStep::Skip;
			}
			html::Inline::Literal => {
				style.code = true;
				InlineKind::Text(text.to_string())
			}
		};
		InlineStep::Emit(kind, style)
	}

	fn emit(
		&mut self,
		mut kind: InlineKind,
		style: TextStyle,
		source: Range<usize>,
		out: &mut SpannedText,
	) {
		// A hidden comment consumes one space from a doubled gap.
		if self.after_comment
			&& let InlineKind::Text(text) = &mut kind
			&& matches!(text.as_bytes().first(), Some(b' ' | b'\t'))
			&& Self::comment_gap_collapses(out)
		{
			text.remove(0);
		}
		self.after_comment = false;
		out.push(kind, style, source);
	}

	fn finish(&self, depth: usize, out: &mut SpannedText) {
		if self.after_comment
			&& depth == 0
			&& let Some((
				Inline {
					kind: InlineKind::Text(text),
					..
				},
				_,
			)) = out.last_mut()
			&& matches!(text.as_bytes().last(), Some(b' ' | b'\t'))
		{
			text.pop();
		}
	}

	/// Whether a leading space after a hidden comment collapses: the run
	/// stands at the paragraph's start or follows another space.
	fn comment_gap_collapses(out: &SpannedText) -> bool {
		out.inlines.last().is_none_or(|inline| {
			matches!(&inline.kind, InlineKind::Text(text)
				if matches!(text.as_bytes().last(), Some(b' ' | b'\t')))
		})
	}
}

struct Reader<'s> {
	/// Opaque HTML content intervals in this source, shared across blocks.
	opaque: RefCell<BTreeMap<usize, usize>>,
	source: &'s str,
	/// The dialect this parse reads: how a soft break reads, and whether
	/// `==` marks a highlight.
	dialect: Dialect,
	lines: Vec<usize>,
	footnotes: HashMap<String, u32>,
	/// Definitions the full parse already owns; snippets only reference them.
	document_footnotes: &'s HashSet<String>,
	/// Definitions kept until every disclosure's references are known.
	footnote_blocks: HashMap<String, Block>,
	/// Appended definitions resolve references but never emit note bodies.
	snippet_end: Option<usize>,
	/// Allocates identities and records source spans for this parse.
	producer: Rc<RefCell<Producer>>,
	/// The document's reference and footnote definitions, which resolve inside
	/// a snippet the same way they do in a full parse.
	definitions: &'s str,
	limits: crate::limits::Limits,
	/// Cumulative snippet bytes whose global definitions were injected. A
	/// document-wide ceiling keeps repeated snippet parses from multiplying
	/// the definitions.
	reparse_used: Rc<Cell<usize>>,
	reparse_limit: usize,
}

impl Reader<'_> {
	fn opaque_until(&self, at: usize) -> usize {
		self.opaque
			.borrow()
			.range(..=at)
			.next_back()
			.map_or(0, |(_, &end)| if at < end { end } else { 0 })
	}

	fn open_opaque(&self, start: usize, tag_len: usize) -> Option<usize> {
		let end = start + opaque_len(&self.source[start..])?;
		self.opaque.borrow_mut().insert(start + tag_len, end);
		Some(end)
	}

	/// Block HTML can open an opaque scope that outlives its Comrak node.
	fn block_opaque(&self, range: Range<usize>) {
		let mut at = range.start;
		while at < range.end {
			at = at.max(self.opaque_until(at));
			if at >= range.end {
				break;
			}
			let Some(open) = self.source[at..range.end].find('<') else {
				break;
			};
			let open = at + open;
			let rest = &self.source[open..];
			if let Some(len) = html::non_element_len(rest) {
				at = open + len;
				continue;
			}
			let Some(len) = html::tag_len(rest) else {
				at = open + 1;
				continue;
			};
			at = self.open_opaque(open, len).unwrap_or(open + len);
		}
	}

	/// Discover snippet scopes in original source coordinates.
	fn snippet_opaque<'a>(&self, node: &'a AstNode<'a>, range: Range<usize>) {
		if matches!(node.data.borrow().value, NodeValue::HtmlBlock(_)) {
			self.block_opaque(range);
			return;
		}
		for child in node.descendants() {
			let at = self.range(child).start;
			if range.contains(&at)
				&& self.opaque_until(at) == 0
				&& let NodeValue::HtmlInline(t) = &child.data.borrow().value
				&& let Some(len) = html::tag_len(t)
				&& !child.ancestors().any(|parent| {
					matches!(parent.data.borrow().value, NodeValue::Image(_))
				}) {
				self.open_opaque(at, len);
			}
		}
	}

	fn range(&self, node: &AstNode<'_>) -> Range<usize> {
		let data = node.data.borrow();
		let p = data.sourcepos;
		let line_start = self
			.lines
			.get(p.start.line.saturating_sub(1))
			.copied()
			.unwrap_or(0);
		let start = line_start + p.start.column.saturating_sub(1);
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
		out: &mut SpannedText,
		depth: usize,
		mut from: usize,
		opaque_until: &mut usize,
	) {
		// Comrak builds the AST iteratively but produces recursion as deep as
		// the input demands; past the budget the remaining text is kept flat
		// instead of descending.
		if depth >= self.limits.inline_depth {
			let text = self.flattened_from(node, from);
			if !text.is_empty() {
				out.push(
					InlineKind::Text(text),
					style.clone(),
					self.range(node).start.max(from)..self.range(node).end,
				);
			}
			return;
		}
		let mut frame = InlineFrame::new(style);
		let end = self.range(node).end;
		for child in node.children() {
			let source = self.range(child);
			if from > 0 && source.end <= from {
				continue;
			}
			if let Some(end) = self.inline_svg(
				child,
				&source,
				end,
				&frame.style,
				out,
				*opaque_until,
			) {
				from = end;
				continue;
			}
			self.inline_anchors(
				child,
				&source,
				&frame.style,
				out,
				opaque_until,
			);
			match self.inline_step(
				child,
				source.clone(),
				&mut frame,
				depth,
				from,
			) {
				InlineStep::Emit(kind, style) => frame.emit(
					kind,
					style,
					source.start.max(from)..source.end,
					out,
				),
				InlineStep::Descend(style) => self.inlines(
					child,
					&style,
					out,
					depth + 1,
					from,
					opaque_until,
				),
				InlineStep::Skip => {}
			}
		}
		frame.finish(depth, out);
	}

	/// An inline SVG consumes source that can overlap several AST siblings.
	fn inline_svg<'a>(
		&self,
		child: &'a AstNode<'a>,
		source: &Range<usize>,
		parent_end: usize,
		style: &TextStyle,
		out: &mut SpannedText,
		opaque_until: usize,
	) -> Option<usize> {
		if !matches!(child.data.borrow().value, NodeValue::HtmlInline(_)) {
			return None;
		}
		let len = html::svg_len(&self.source[source.start..])?;
		let end = source.start + len;
		if end > parent_end {
			return None;
		}
		let markers = container_markers(child);
		let raw = HtmlSource::new(&self.source[source.start..end], &markers);
		let (_, image) = html::svg(&raw.text).unwrap();
		for anchor in html::block_anchors(&raw.text)
			.into_iter()
			.filter(|_| source.start >= opaque_until)
		{
			out.push(InlineKind::Anchor(anchor), style.clone(), source.clone());
		}
		out.push(
			InlineKind::Image(Box::new(image)),
			style.clone(),
			source.start..end,
		);
		Some(end)
	}

	fn inline_anchors<'a>(
		&self,
		child: &'a AstNode<'a>,
		source: &Range<usize>,
		style: &TextStyle,
		out: &mut SpannedText,
		opaque_until: &mut usize,
	) {
		if source.start >= *opaque_until
			&& let NodeValue::HtmlInline(t) = &child.data.borrow().value
		{
			for anchor in html::anchors(t) {
				out.push(
					InlineKind::Anchor(anchor),
					style.clone(),
					source.clone(),
				);
			}
			if let Some(tag_len) = html::tag_len(t)
				&& let Some(end) = self.open_opaque(source.start, tag_len)
			{
				*opaque_until = end;
			}
		}
	}

	#[cfg_attr(
		any(),
		markview::allow_complexity(
			reason = "Keep the Comrak node-to-inline mapping in one dispatch table; source consumption and HTML scopes are handled separately."
		)
	)]
	fn inline_step<'a>(
		&self,
		child: &'a AstNode<'a>,
		source: Range<usize>,
		frame: &mut InlineFrame,
		depth: usize,
		from: usize,
	) -> InlineStep {
		let mut child_style = frame.style.clone();
		let value = child.data.borrow();
		let kind = match &value.value {
			NodeValue::Text(t) => {
				Some(InlineKind::Text(self.text_after(t, source.clone(), from)))
			}
			NodeValue::SoftBreak => Some(if self.dialect.hard_breaks {
				InlineKind::LineBreak(super::LineBreak::Hard)
			} else {
				InlineKind::Text(" ".into())
			}),
			NodeValue::LineBreak => {
				Some(InlineKind::LineBreak(super::LineBreak::Hard))
			}
			NodeValue::Code(c) => {
				child_style.code = true;
				Some(InlineKind::Text(self.text_after(
					&c.literal,
					source.clone(),
					from,
				)))
			}
			NodeValue::Raw(t) => {
				child_style.code = true;
				Some(InlineKind::Text(self.text_after(t, source.clone(), from)))
			}
			// Comrak reads an indented callout title as a code block.
			NodeValue::CodeBlock(c) => Some(InlineKind::Text(self.text_after(
				&c.literal,
				source.clone(),
				from,
			))),
			NodeValue::HtmlInline(t) => {
				return frame.html(
					t,
					source.start,
					self.dialect.ofm,
					self.limits.html_scopes,
					child_style,
				);
			}
			NodeValue::Math(m) => Some(InlineKind::Math {
				latex: self.text_after(&m.literal, source.clone(), from),
				display: m.display_math,
			}),
			NodeValue::FootnoteReference(f) => {
				child_style.superscript = true;
				Some(InlineKind::FootnoteRef(FootnoteReference {
					definition: FootnoteId::new(note_key(&f.name)),
					number: f.ix,
					occurrence: source_occurrence(source.start),
				}))
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
			NodeValue::WikiLink(l) => {
				if self.dialect.ofm && l.embed && wikilink_image(l) {
					let in_table = child.ancestors().any(|parent| {
						matches!(
							parent.data.borrow().value,
							NodeValue::TableCell
						)
					});
					let alt = if wikilink_has_label(
						&self.source[source.clone()],
						in_table,
					) {
						self.alt_text(child, depth + 1, from)
					} else {
						String::new()
					};
					Some(InlineKind::Image(Box::new(crate::image::ImageSpec {
						source: crate::image::ImageSource::Resource(
							crate::image::ResourceRef::vault(
								wikilink_asset_url(l),
							),
						),
						alt,
						title: String::new(),
						width: None,
						height: None,
					})))
				} else {
					let url = wikilink_url(l);
					child_style.link = Some(source_link(url, source.start));
					None
				}
			}
			NodeValue::Highlight => {
				child_style.mark = true;
				None
			}
			// A hidden `%%inline%%` comment contributes no run; inside a
			// code scope it stays visible source instead.
			NodeValue::CommentInline(comment) => {
				if !frame.style.code {
					frame.after_comment = true;
					return InlineStep::Skip;
				}
				Some(InlineKind::Text(format!("%%{}%%", comment.literal)))
			}
			NodeValue::Link(l) => {
				child_style.link =
					Some(source_link(l.url.clone(), source.start));
				None
			}
			NodeValue::Image(link) => {
				Some(InlineKind::Image(Box::new(crate::image::ImageSpec {
					source: crate::image::ImageSource::Resource(
						crate::image::ResourceRef::document(link.url.clone()),
					),
					alt: self.alt_text(child, depth + 1, from),
					title: link.title.clone(),
					width: None,
					height: None,
				})))
			}
			_ => None,
		};
		match kind {
			Some(kind) => InlineStep::Emit(kind, child_style),
			None => InlineStep::Descend(child_style),
		}
	}

	/// Alt text contains no document elements or opaque scopes.
	fn alt_text<'a>(
		&self,
		node: &'a AstNode<'a>,
		depth: usize,
		from: usize,
	) -> String {
		let mut alt = SpannedText::new();
		let mut opaque_until = usize::MAX;
		self.inlines(
			node,
			&TextStyle::default(),
			&mut alt,
			depth,
			from,
			&mut opaque_until,
		);
		plain_text(&alt.inlines)
	}

	/// The node's source with hidden `%%` comment spans cut out, for the
	/// depth-limit fallback that embeds raw Markdown.
	fn visible_source<'a>(&self, node: &'a AstNode<'a>) -> String {
		let whole = self.range(node);
		let mut hidden: Vec<Range<usize>> = node
			.descendants()
			.filter(|descendant| {
				matches!(
					descendant.data.borrow().value,
					NodeValue::CommentInline(_)
						| NodeValue::MultilineComment(_)
				)
			})
			.map(|descendant| self.range(descendant))
			.collect();
		if hidden.is_empty() {
			return self.source[whole.clone()].to_string();
		}
		hidden.sort_by_key(|range| range.start);
		let mut text = String::new();
		let mut cursor = whole.start;
		for range in hidden {
			text.push_str(&self.source[cursor..range.start]);
			cursor = range.end;
		}
		text.push_str(&self.source[cursor..whole.end]);
		text
	}

	fn text_after(
		&self,
		text: &str,
		source: Range<usize>,
		from: usize,
	) -> String {
		if source.start >= from {
			return text.to_string();
		}
		let mut cursor = source.start;
		for (i, ch) in text.char_indices() {
			// Code spans normalize line endings to spaces.
			if let Some(at) = self.source[cursor..source.end].find(|raw| {
				raw == ch || ch == ' ' && matches!(raw, '\r' | '\n')
			}) {
				let at = cursor + at;
				if at >= from {
					return text[i..].to_string();
				}
				cursor =
					at + self.source[at..].chars().next().unwrap().len_utf8();
				if ch == ' ' && self.source[at..].starts_with("\r\n") {
					cursor += 1;
				}
			}
		}
		String::new()
	}

	/// The readable text after `from`, collected without recursion.
	fn flattened_from<'a>(&self, node: &'a AstNode<'a>, from: usize) -> String {
		let mut text = String::new();
		for descendant in node.descendants() {
			let source = self.range(descendant);
			if from > 0 && source.end <= from {
				continue;
			}
			match &descendant.data.borrow().value {
				NodeValue::Text(t) => {
					text.push_str(&self.text_after(t, source, from))
				}
				NodeValue::Raw(t) => {
					text.push_str(&self.text_after(t, source, from))
				}
				NodeValue::Code(c) => {
					text.push_str(&self.text_after(&c.literal, source, from))
				}
				NodeValue::CodeBlock(c) => text.push_str(&c.literal),
				NodeValue::Math(m) => {
					text.push_str(&self.text_after(&m.literal, source, from))
				}
				NodeValue::SoftBreak => {
					text.push(if self.dialect.hard_breaks { '\n' } else { ' ' })
				}
				NodeValue::LineBreak => text.push('\n'),
				_ => {}
			}
		}
		text
	}

	fn rich<'a>(&self, node: &'a AstNode<'a>) -> SpannedText {
		let mut text = SpannedText::new();
		let mut opaque_until = self.opaque_until(self.range(node).start);
		self.inlines(
			node,
			&TextStyle::default(),
			&mut text,
			0,
			0,
			&mut opaque_until,
		);
		text.merged()
	}

	fn blocks<'a>(
		&mut self,
		node: &'a AstNode<'a>,
		depth: usize,
	) -> Vec<Block> {
		let children: Vec<&'a AstNode<'a>> = node.children().collect();
		self.sequence(&children, depth)
	}

	/// One sibling list. A raw HTML container may span several siblings, so
	/// the walk keeps a stack of open frames; the Markdown siblings Comrak
	/// already parsed become the body and are never re-parsed.
	fn sequence<'a>(
		&mut self,
		children: &[&'a AstNode<'a>],
		depth: usize,
	) -> Vec<Block> {
		let mut out = Vec::new();
		let mut stack = ContainerStack::default();
		let mut i = 0;
		while i < children.len() {
			if let Some(consumed) =
				self.svg(children, i, depth, stack.target(&mut out))
			{
				i += consumed;
				continue;
			}
			let child = children[i];
			if matches!(&child.data.borrow().value, NodeValue::HtmlBlock(_)) {
				self.html_events(child, depth, &mut stack, &mut out);
			} else {
				let content_depth = depth + stack.len();
				match self.child_block(child, content_depth) {
					Child::Block(block) => stack.push_block(&mut out, block),
					Child::Skip => {}
					Child::Flatten => {
						let blocks = self.blocks(child, content_depth + 1);
						stack.extend_blocks(&mut out, blocks);
					}
				}
			}
			i += 1;
		}
		// An element that never closed keeps the literal fallback.
		stack.pop_unclosed(&mut out, self);
		out
	}

	/// Feed one raw HTML block's container events into the open frames.
	fn html_events<'a>(
		&mut self,
		child: &'a AstNode<'a>,
		depth: usize,
		stack: &mut ContainerStack,
		out: &mut Vec<Block>,
	) {
		let source = self.source;
		let child_range = self.range(child);
		self.block_opaque(child_range.clone());
		let markers = container_markers(child);
		let original = HtmlSource::new(&source[child_range.clone()], &markers);
		let events = html::block_events(&original.text);
		let opaque_until = self.opaque_until(child_range.start);
		let block_depth = self.limits.block_depth;
		let map = |range| map_source(&original, &child_range, range);
		stack.consume(
			&events,
			&original.text,
			map,
			opaque_until,
			depth,
			block_depth,
			source,
			self,
			out,
		);
	}

	/// SVG may cross Comrak's blank-line HTML boundaries.
	fn svg<'a>(
		&mut self,
		children: &[&'a AstNode<'a>],
		start: usize,
		depth: usize,
		out: &mut Vec<Block>,
	) -> Option<usize> {
		if depth >= self.limits.block_depth {
			return None;
		}
		let node = children[start];
		let range = self.range(node);
		let begin = match &node.data.borrow().value {
			NodeValue::HtmlBlock(_) => range.start,
			NodeValue::Paragraph => node.children().find_map(|child| {
				if !matches!(
					child.data.borrow().value,
					NodeValue::HtmlInline(_)
				) {
					return None;
				}
				let at = self.range(child).start;
				let len = html::svg_len(&self.source[at..])?;
				(at + len > range.end).then_some(at)
			})?,
			_ => return None,
		};
		let mut end = begin + html::svg_len(&self.source[begin..])?;
		let mut close = (start..children.len())
			.find(|&i| self.range(children[i]).end >= end)?;
		let markers = container_markers(node);
		if begin > range.start {
			self.snippet_opaque(node, range.start..begin);
			let prefix =
				HtmlSource::new(&self.source[range.start..begin], &markers);
			out.extend(self.markdown_blocks_at(
				&prefix.text,
				depth,
				range.start..begin,
			));
		}
		let mut begin = begin;
		loop {
			let raw = HtmlSource::new(&self.source[begin..end], &markers);
			let (_, image) = html::svg(&raw.text)?;
			let source = begin..end;
			let mut text = SpannedText::new();
			text.push(
				InlineKind::Image(Box::new(image)),
				TextStyle::default(),
				source.clone(),
			);
			let (inlines, spans, covers) = text.into_parts(self.source);
			let anchors = html::block_anchors_from(
				&self.source[source.clone()],
				self.opaque_until(source.start).saturating_sub(source.start),
			);
			let block = self.make(
				anchors,
				BlockKind::Paragraph(inlines),
				source,
				vec![spans],
				vec![covers],
			);
			out.push(block);
			let tail_end = self.range(children[close]).end;
			let mut rest = &self.source[end..tail_end];
			loop {
				let next =
					without_markers(rest.trim_start(), &markers).trim_start();
				if next.len() == rest.len() {
					break;
				}
				rest = next;
			}
			let next_begin = tail_end - rest.len();
			if !rest.is_empty()
				&& let Some(len) = html::svg_len(&self.source[next_begin..])
				&& let Some(next_close) = (close..children.len())
					.find(|&i| self.range(children[i]).end >= next_begin + len)
			{
				begin = next_begin;
				end = begin + len;
				close = next_close;
				continue;
			}
			let tail = HtmlSource::new(&self.source[end..tail_end], &markers);
			if !tail.text.trim().is_empty() {
				self.snippet_opaque(children[close], end..tail_end);
				out.extend(self.markdown_blocks_at(
					&tail.text,
					depth,
					end..tail_end,
				));
			}
			break;
		}
		Some(close - start + 1)
	}

	fn unused_footnote(
		&self,
		name: &str,
		references: u32,
		start: usize,
	) -> bool {
		(self.snippet_end.is_none() && references == 0)
			|| self.snippet_end.is_some_and(|end| {
				start >= end
					|| self.document_footnotes.contains(&note_key(name))
			})
	}

	fn callout_fields(
		kind: &BlockKind,
		source: &Range<usize>,
	) -> Vec<SourceSpan> {
		match kind {
			BlockKind::Quote {
				callout: Some(callout),
				..
			} => callout.title.as_ref().map_or_else(Vec::new, |title| {
				uniform_spans(title, source.clone())
			}),
			_ => Vec::new(),
		}
	}

	/// Comrak's callout metadata, inline title and body.
	fn callout_kind<'a>(
		&mut self,
		node: &'a AstNode<'a>,
		metadata: &NodeCallout,
		source: &Range<usize>,
		depth: usize,
	) -> BlockKind {
		let title = node.children().find(|child| {
			matches!(child.data.borrow().value, NodeValue::CalloutTitle)
		});
		let title = title.map_or_else(
			|| plain_rich(metadata.default_title(), source.clone()).inlines,
			|title| self.rich(title).trimmed().inlines,
		);
		let fold = match metadata.fold {
			CalloutFold::None => None,
			CalloutFold::Expanded => Some(true),
			CalloutFold::Collapsed => Some(false),
		};
		let mut blocks = self.blocks(node, depth + 1);
		let title = if let Some(default_open) = fold {
			// A foldable callout's body is one disclosure, so it toggles like
			// a `<details>` element. The disclosure owns the title as its
			// summary; the callout keeps only its type and fold state.
			let fields = vec![uniform_spans(&title, source.clone())];
			let made = Made {
				kind: BlockKind::Details {
					default_open,
					summary: title,
					blocks,
				},
				anchors: Vec::new(),
				source: source.clone(),
				fields: fields.clone(),
				covers: fields,
			};
			blocks = vec![self.producer.borrow_mut().block(made)];
			None
		} else {
			Some(title)
		};
		BlockKind::Quote {
			callout: Some(Callout {
				kind: CalloutKind::parse(&metadata.callout_type),
				name: metadata.callout_type.clone(),
				title,
				fold,
			}),
			blocks,
		}
	}

	#[cfg_attr(
		any(),
		markview::allow_complexity(
			reason = "Keep the block-kind dispatch with the hidden-comment paragraph and block-comment skips in one ordered walk; the same source ranges feed fingerprints and anchors."
		)
	)]
	fn child_block<'a>(
		&mut self,
		child: &'a AstNode<'a>,
		depth: usize,
	) -> Child {
		let source = self.range(child);
		let data = child.data.borrow();
		if matches!(data.value, NodeValue::CalloutTitle) {
			return Child::Skip;
		}
		let (kind, fields, covers) = if depth >= self.limits.block_depth {
			let text = self.visible_source(child);
			(
				BlockKind::Code {
					language: "nested Markdown".into(),
					text: text.clone(),
				},
				vec![vec![SourceSpan {
					semantic: 0..text.len(),
					source: source.clone(),
					exact: false,
				}]],
				vec![vec![SourceSpan {
					semantic: 0..text.len(),
					source: source.clone(),
					exact: false,
				}]],
			)
		} else {
			match &data.value {
				NodeValue::FrontMatter(text) => {
					let Some(yaml) = front_matter::parse(text).into_yaml()
					else {
						return Child::Skip;
					};
					let code_source = source.clone();
					let inner = self.make(
						Vec::new(),
						BlockKind::Code {
							language: front_matter::LANGUAGE.into(),
							text: yaml.clone(),
						},
						code_source.clone(),
						vec![vec![SourceSpan {
							semantic: 0..yaml.len(),
							source: code_source.clone(),
							exact: false,
						}]],
						vec![vec![SourceSpan {
							semantic: 0..yaml.len(),
							source: code_source,
							exact: false,
						}]],
					);
					(
						BlockKind::FrontMatter {
							default_open: false,
							blocks: vec![inner],
						},
						Vec::new(),
						Vec::new(),
					)
				}
				// A line-start `%%` comment is a block of its own whose whole
				// span stays hidden.
				NodeValue::MultilineComment(_) => return Child::Skip,
				NodeValue::Paragraph => {
					let text = self.rich(child);
					// A paragraph can hold only a hidden `%%` comment; the
					// source line then reads as no paragraph at all.
					if text.is_empty() && self.dialect.ofm {
						return Child::Skip;
					}
					let (inlines, spans, covers) = text.into_parts(self.source);
					(BlockKind::Paragraph(inlines), vec![spans], vec![covers])
				}
				NodeValue::Heading(h) => {
					let text = self.rich(child);
					let (inlines, spans, covers) = text.into_parts(self.source);
					(
						BlockKind::Heading {
							level: h.level,
							text: inlines,
							anchor: String::new(),
						},
						vec![spans],
						vec![covers],
					)
				}
				// The info string's first word is the language; trailing
				// words are metadata, so `mermaid title="x"` still renders.
				NodeValue::CodeBlock(c) => {
					let kind = code_block_kind(c, &source);
					let fields = match &kind {
						BlockKind::Code { text, .. } => {
							vec![code_spans(self.source, &source, text)]
						}
						_ => own_spans(&kind, &source),
					};
					(kind, fields.clone(), fields)
				}
				NodeValue::HtmlBlock(h) => {
					let Some(kind) = html_block_kind(
						&h.literal,
						self.limits.html_scopes,
						&source,
						self.dialect.ofm,
					) else {
						return Child::Skip;
					};
					let fields = own_spans(&kind, &source);
					(kind, fields.clone(), fields)
				}
				NodeValue::ThematicBreak => {
					(BlockKind::Rule, Vec::new(), Vec::new())
				}
				NodeValue::BlockQuote => (
					BlockKind::Quote {
						callout: None,
						blocks: self.blocks(child, depth + 1),
					},
					Vec::new(),
					Vec::new(),
				),
				NodeValue::Callout(callout) => {
					let kind =
						self.callout_kind(child, callout, &source, depth);
					let fields = Self::callout_fields(&kind, &source);
					(kind, vec![fields.clone()], vec![fields])
				}
				NodeValue::Alert(a) => {
					let kind = BlockKind::Quote {
						callout: Some(Callout {
							name: format!("{:?}", a.alert_type)
								.to_ascii_lowercase(),
							kind: match a.alert_type {
								comrak::nodes::AlertType::Note => {
									CalloutKind::Note
								}
								comrak::nodes::AlertType::Tip => {
									CalloutKind::Tip
								}
								comrak::nodes::AlertType::Important => {
									CalloutKind::Important
								}
								comrak::nodes::AlertType::Warning => {
									CalloutKind::Warning
								}
								comrak::nodes::AlertType::Caution => {
									CalloutKind::Caution
								}
							},
							title: Some(
								plain_rich(
									format!("{:?}", a.alert_type),
									source.clone(),
								)
								.inlines,
							),
							fold: None,
						}),
						blocks: self.blocks(child, depth + 1),
					};
					let fields = Self::callout_fields(&kind, &source);
					(kind, vec![fields.clone()], vec![fields])
				}
				NodeValue::List(l) => (
					BlockKind::List {
						start: (l.list_type == ListType::Ordered)
							.then_some(l.start as isize),
						reversed: false,
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
									number: None,
									blocks: self.blocks(item, depth + 1),
								}
							})
							.collect(),
					},
					Vec::new(),
					Vec::new(),
				),
				NodeValue::Table(t) => {
					let columns: Vec<TableColumn> = t
						.alignments
						.iter()
						.map(|a| TableColumn {
							align: Some(match a {
								TableAlignment::Center => CellAlign::Center,
								TableAlignment::Right => CellAlign::Right,
								_ => CellAlign::Left,
							}),
						})
						.collect();
					let mut groups: Vec<TableGroup> = Vec::new();
					for (index, row) in child.children().enumerate() {
						let kind = if index == 0 {
							TableGroupKind::Head
						} else {
							TableGroupKind::Body
						};
						let mut cells = Vec::new();
						for cell_node in row.children() {
							let range = self.range(cell_node);
							// A synthetic empty cell can take the row's line ending,
							// which lies past the table's own range.
							let range = range.start.min(source.end)
								..range.end.min(source.end);
							let text = self.rich(cell_node);
							let (inlines, spans, covers) =
								text.into_parts(self.source);
							let block = self.make(
								Vec::new(),
								BlockKind::Paragraph(inlines),
								range,
								vec![spans],
								vec![covers],
							);
							cells.push(TableCell {
								kind: if index == 0 {
									CellKind::Header
								} else {
									CellKind::Data
								},
								rowspan: 1,
								colspan: 1,
								align: None,
								blocks: vec![block],
							});
						}
						if let Some(group) = groups.last_mut()
							&& group.kind == kind
						{
							group.rows.push(TableRow { cells });
						} else {
							groups.push(TableGroup {
								kind,
								rows: vec![TableRow { cells }],
							});
						}
					}
					(
						BlockKind::Table(Table {
							caption: None,
							columns,
							groups,
						}),
						Vec::new(),
						Vec::new(),
					)
				}
				NodeValue::FootnoteDefinition(f) => {
					if self.unused_footnote(
						&f.name,
						f.total_references,
						source.start,
					) {
						return Child::Skip;
					}
					let key = note_key(&f.name);
					(
						BlockKind::Footnote {
							id: FootnoteId::new(key.clone()),
							number: self
								.footnotes
								.get(&key)
								.copied()
								.unwrap_or(0),
							blocks: self.blocks(child, depth + 1),
						},
						Vec::new(),
						Vec::new(),
					)
				}
				_ => {
					return Child::Flatten;
				}
			}
		};
		let block = self.make(Vec::new(), kind, source, fields, covers);
		if let NodeValue::FootnoteDefinition(f) = &data.value {
			// Snippet-only definitions also belong at the document's end.
			self.footnote_blocks.insert(note_key(&f.name), block);
			Child::Skip
		} else {
			Child::Block(block)
		}
	}

	/// Allocates one built block and records its own field spans.
	fn make(
		&mut self,
		anchors: Vec<String>,
		kind: BlockKind,
		source: Range<usize>,
		fields: Vec<Vec<SourceSpan>>,
		covers: Vec<Vec<SourceSpan>>,
	) -> Block {
		self.producer.borrow_mut().block(Made {
			kind,
			anchors,
			source,
			fields,
			covers,
		})
	}

	/// Restore snippet coordinates after removing disclosure tags and quote prefixes.
	fn markdown_blocks_at(
		&mut self,
		text: &str,
		depth: usize,
		source: Range<usize>,
	) -> Vec<Block> {
		let offsets = source_offsets(text, self.source, source);
		let opaque_until = offsets
			.partition_point(|offset| *offset < self.opaque_until(offsets[0]));
		let mut blocks = self.markdown_blocks(text, depth, opaque_until);
		self.producer.borrow_mut().remap(&mut blocks, &offsets);
		blocks
	}

	/// The block list `text` describes, parsed by the ordinary pipeline. The
	/// shared anchors keep headings inside the snippet unique in the document.
	fn markdown_blocks(
		&mut self,
		text: &str,
		depth: usize,
		opaque_until: usize,
	) -> Vec<Block> {
		if text.trim().is_empty() {
			return Vec::new();
		}
		// A snippet only holds part of the document, so a reference or note it
		// uses may be defined outside it; parsing it together with the
		// document's definitions resolves those, and only the blocks the
		// snippet itself covers are kept. Terminate it the same way even without
		// definitions, so an unused definition cannot change its EOF ranges.
		if !text.contains('[') {
			return self.snippet(text, depth, None, opaque_until);
		}
		let cost = text.len().saturating_add(self.definitions.len());
		if self.reparse_used.get().saturating_add(cost) > self.reparse_limit {
			// Over the document-wide ceiling: keep the snippet readable without
			// the global definitions so repeated parses stay bounded.
			return self.snippet(text, depth, None, opaque_until);
		}
		self.reparse_used
			.set(self.reparse_used.get().saturating_add(cost));
		let definitions =
			incremental::missing_definitions(self.definitions, text);
		let mut joined =
			String::with_capacity(text.len() + definitions.len() + 2);
		joined.push_str(text);
		joined.push_str("\n\n");
		joined.push_str(&definitions);
		let mut blocks = self.snippet(&joined, depth, Some(text), opaque_until);
		// An unclosed fence or HTML block can swallow the appended
		// definitions; then the bare snippet parses to what the full document
		// puts there.
		if blocks.iter().any(|b| {
			self.producer
				.borrow()
				.block_range(b)
				.is_some_and(|r| r.start < text.len() && r.end > text.len())
		}) {
			return self.snippet(text, depth, None, opaque_until);
		}
		blocks.retain(|b| {
			self.producer
				.borrow()
				.block_range(b)
				.is_some_and(|r| r.start < text.len())
		});
		blocks
	}

	/// `text` parsed by the ordinary pipeline, falling back to `bare` when a
	/// literal block absorbs the appended bytes.
	fn snippet(
		&mut self,
		text: &str,
		depth: usize,
		bare: Option<&str>,
		opaque_until: usize,
	) -> Vec<Block> {
		let arena = Arena::new();
		let mut options = markdown_options(self.dialect);
		// Another disclosure may reference a definition this snippet owns.
		options.parse.leave_footnote_definitions = true;
		let root = parse_document(&arena, text, &options);
		let lines = line_starts(text);
		if let Some(bare) = bare
			&& root.descendants().any(|node| {
				let data = node.data.borrow();
				let literal = match &data.value {
					NodeValue::CodeBlock(_) => true,
					NodeValue::HtmlBlock(h) => {
						// Raw HTML can consume synthetic blank lines while its
						// source position excludes them.
						if h.block_type <= 5
							&& text.ends_with("\n\n")
							&& h.literal.ends_with("\n\n")
						{
							return true;
						}
						true
					}
					_ => false,
				};
				let position = |p: comrak::nodes::LineColumn| {
					lines[p.line.saturating_sub(1)] + p.column
				};
				literal
					&& position(data.sourcepos.start).saturating_sub(1)
						< bare.len() && position(data.sourcepos.end) > bare.len()
			}) {
			return self.snippet(bare, depth, None, opaque_until);
		}
		// A note keeps the number the document gave it, so a reference inside
		// a snippet and the note block outside it still agree.
		let mut next_footnote =
			self.footnotes.values().copied().max().unwrap_or(0) + 1;
		for node in root.descendants() {
			if let NodeValue::FootnoteReference(f) =
				&mut node.data.borrow_mut().value
			{
				f.ix = *self.footnotes.entry(note_key(&f.name)).or_insert_with(
					|| {
						let ix = next_footnote;
						next_footnote += 1;
						ix
					},
				);
			}
		}
		let mut reader = Reader {
			opaque: RefCell::new(if opaque_until == 0 {
				BTreeMap::new()
			} else {
				BTreeMap::from([(0, opaque_until)])
			}),
			source: text,
			dialect: self.dialect,
			lines,
			footnotes: std::mem::take(&mut self.footnotes),
			document_footnotes: self.document_footnotes,
			footnote_blocks: std::mem::take(&mut self.footnote_blocks),
			snippet_end: Some(bare.map_or(text.len(), str::len)),
			producer: Rc::clone(&self.producer),
			definitions: self.definitions,
			limits: self.limits,
			reparse_used: Rc::clone(&self.reparse_used),
			reparse_limit: self.reparse_limit,
		};
		let blocks = reader.blocks(root, depth);
		self.footnotes = reader.footnotes;
		self.footnote_blocks = reader.footnote_blocks;
		blocks
	}

	/// The summary's rich text. GFM keeps raw HTML inside `<summary>` and
	/// does not parse Markdown, so `**Bold**` stays literal.
	fn summary_rich(&self, text: &str, source: &Range<usize>) -> SpannedText {
		if text.trim().is_empty() {
			return SpannedText::new();
		}
		let opaque_until = self.opaque_until(source.start);
		let anchor_from = if opaque_until > source.start {
			source_offsets(text, self.source, source.clone())
				.partition_point(|&at| at < opaque_until)
		} else {
			0
		};
		let mut out = match html::block_from(
			text,
			self.limits.html_scopes,
			anchor_from,
			source.start,
			self.dialect.ofm,
		) {
			html::Block::Paragraph(spans) => html_rich(spans, source),
			_ => SpannedText::new(),
		};
		if out.is_empty() {
			for anchor in html::block_anchors_from(text, anchor_from) {
				out.push(
					InlineKind::Anchor(anchor),
					TextStyle::default(),
					source.clone(),
				);
			}
			out.push(
				InlineKind::Text(text.trim().to_string()),
				TextStyle::default(),
				source.clone(),
			);
		}
		out.merged()
	}
}

impl ContainerHost for Reader<'_> {
	fn make(&mut self, made: Made) -> Block {
		Reader::make(
			self,
			made.anchors,
			made.kind,
			made.source,
			made.fields,
			made.covers,
		)
	}

	fn leaf(
		&mut self,
		text: &str,
		range: Range<usize>,
		opaque_until: usize,
	) -> Option<Block> {
		let made = html_leaf_block(
			self.source,
			text,
			range,
			self.limits.html_scopes,
			opaque_until,
			self.dialect.ofm,
		)?;
		Some(self.make(
			made.anchors,
			made.kind,
			made.source,
			made.fields,
			made.covers,
		))
	}

	fn summary(&mut self, text: &str, source: &Range<usize>) -> SpannedText {
		Reader::summary_rich(self, text, source)
	}

	fn opaque_until(&self, at: usize) -> usize {
		Reader::opaque_until(self, at)
	}
}

fn map_source(
	original: &HtmlSource,
	child: &Range<usize>,
	range: Range<usize>,
) -> Range<usize> {
	let mapped = original.range(range);
	child.start + mapped.start..child.start + mapped.end
}

fn html_leaf_block(
	source: &str,
	text: &str,
	range: Range<usize>,
	scope_limit: usize,
	opaque_until: usize,
	ofm: bool,
) -> Option<Made> {
	let range = clamp_range(source, range);
	let anchor_from = if opaque_until > range.start {
		source_offsets(text, source, range.clone())
			.partition_point(|&at| at < opaque_until)
	} else {
		0
	};
	let (kind, fields, covers) = match html::block_from(
		text,
		scope_limit,
		anchor_from,
		range.start,
		ofm,
	) {
		html::Block::Unsupported => {
			let code = text.to_string();
			(
				BlockKind::Code {
					language: "HTML source".into(),
					text: code.clone(),
				},
				vec![vec![SourceSpan {
					semantic: 0..code.len(),
					source: range.clone(),
					exact: false,
				}]],
				vec![vec![SourceSpan {
					semantic: 0..code.len(),
					source: range.clone(),
					exact: false,
				}]],
			)
		}
		html::Block::Empty => return None,
		html::Block::Rule => (BlockKind::Rule, Vec::new(), Vec::new()),
		html::Block::Heading { level, text } => {
			let (inlines, spans, covers) =
				html_rich(text, &range).into_parts(source);
			(
				BlockKind::Heading {
					level,
					text: inlines,
					anchor: String::new(),
				},
				vec![spans],
				vec![covers],
			)
		}
		html::Block::Paragraph(text) => {
			let (inlines, spans, covers) =
				html_rich(text, &range).into_parts(source);
			(BlockKind::Paragraph(inlines), vec![spans], vec![covers])
		}
	};
	let anchors = if matches!(kind, BlockKind::Code { .. } | BlockKind::Rule) {
		html::block_anchors_from(text, anchor_from)
	} else {
		Vec::new()
	};
	Some(Made {
		kind,
		anchors,
		source: range,
		fields,
		covers,
	})
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

enum ContainerMarker {
	Indent(usize),
	Quote,
}

/// Continuation prefixes in the order the AST's containers consume them.
fn container_markers<'a>(node: &'a AstNode<'a>) -> Vec<ContainerMarker> {
	let mut markers = Vec::new();
	for parent in node.ancestors().skip(1) {
		match &parent.data.borrow().value {
			NodeValue::Item(list) => markers.push(ContainerMarker::Indent(
				list.marker_offset + list.padding,
			)),
			NodeValue::TaskItem(_) => {
				if let Some(list) = parent.parent()
					&& let NodeValue::List(list) = &list.data.borrow().value
				{
					markers.push(ContainerMarker::Indent(
						list.marker_offset + list.padding,
					));
				}
			}
			NodeValue::FootnoteDefinition(_) => {
				markers.push(ContainerMarker::Indent(4))
			}
			NodeValue::BlockQuote
			| NodeValue::Alert(_)
			| NodeValue::Callout(_) => markers.push(ContainerMarker::Quote),
			_ => {}
		}
	}
	markers.reverse();
	markers
}

/// HTML without container prefixes, mapped to its original byte offsets.
struct HtmlSource<'a> {
	text: Cow<'a, str>,
	lines: Vec<(usize, usize)>,
}

impl<'a> HtmlSource<'a> {
	fn new(source: &'a str, markers: &[ContainerMarker]) -> Self {
		if markers.is_empty() {
			return Self {
				text: Cow::Borrowed(source),
				lines: Vec::new(),
			};
		}
		let mut text = String::with_capacity(source.len());
		let mut lines = Vec::new();
		let mut at = 0;
		for line in source.split_inclusive(['\r', '\n']) {
			let body = without_markers(line, markers);
			lines.push((text.len(), at + line.len() - body.len()));
			text.push_str(body);
			at += line.len();
		}
		Self {
			text: Cow::Owned(text),
			lines,
		}
	}

	fn range(&self, range: Range<usize>) -> Range<usize> {
		let original = |at| {
			if self.lines.is_empty() {
				return at;
			}
			let i = self.lines.partition_point(|(start, _)| *start <= at) - 1;
			let (start, source_start) = self.lines[i];
			source_start + at - start
		};
		original(range.start)..original(range.end)
	}
}

/// One line with its enclosing list indentation and quote markers removed.
fn without_markers<'a>(line: &'a str, markers: &[ContainerMarker]) -> &'a str {
	let mut rest = line;
	for marker in markers {
		let indent = rest.len() - rest.trim_start_matches(' ').len();
		if let ContainerMarker::Indent(width) = marker {
			if indent < *width {
				break;
			}
			rest = &rest[*width..];
			continue;
		}
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
	}
	rest
}

/// The byte offset of the start of every line, in the sense comrak counts
/// them. Comrak ends a line at `\n`, `\r`, and `\r\n`; a table built from
/// `\n` alone would point the source ranges at the wrong line wherever a
/// lone carriage return occurs.
fn line_starts(source: &str) -> Vec<usize> {
	let mut lines = vec![0];
	let bytes = source.as_bytes();
	let mut i = 0;
	while i < bytes.len() {
		match bytes[i] {
			b'\n' => {
				i += 1;
				lines.push(i);
			}
			b'\r' => {
				i += 1;
				if i < bytes.len() && bytes[i] == b'\n' {
					i += 1;
				}
				lines.push(i);
			}
			_ => i += 1,
		}
	}
	lines
}

/// The identity of a footnote label: comrak matches labels case-insensitively
/// and collapses whitespace, so every label map is keyed by this.
fn note_key(name: &str) -> String {
	name.split_whitespace()
		.collect::<Vec<_>>()
		.join(" ")
		.to_lowercase()
}

/// The document `source` describes in the default dialect, reading single
/// newlines inside a paragraph as spaces and `==` as literal text.
pub fn parse(source: impl Into<Arc<str>>) -> MarkdownParse {
	parse_with(source, Dialect::default())
}

/// The document `source` describes when read in `dialect`.
pub fn parse_with(
	source: impl Into<Arc<str>>,
	dialect: Dialect,
) -> MarkdownParse {
	parse_with_seed(source, dialect, None, None)
}

/// [`parse_with`] continuing an existing identity space.
pub(crate) fn parse_with_seed(
	source: impl Into<Arc<str>>,
	dialect: Dialect,
	revision: Option<DocumentRevision>,
	after: Option<markview_document::BlockId>,
) -> MarkdownParse {
	let source = source.into();
	let revision = revision.unwrap_or_else(DocumentRevision::fresh);
	let (blocks, producer) = {
		let arena = Arena::new();
		let root = parse_document(&arena, &source, &markdown_options(dialect));
		let lines = line_starts(&source);
		let footnotes: HashMap<String, u32> = root
			.descendants()
			.filter_map(|n| match &n.data.borrow().value {
				NodeValue::FootnoteReference(f) => {
					Some((note_key(&f.name), f.ix))
				}
				_ => None,
			})
			.collect();
		let document_footnotes: HashSet<String> = root
			.descendants()
			.filter_map(|node| {
				if let NodeValue::FootnoteDefinition(note) =
					&node.data.borrow().value
					&& note.total_references > 0
				{
					Some(note_key(&note.name))
				} else {
					None
				}
			})
			.collect();
		let definitions = if incremental::definition_free(&source) {
			String::new()
		} else {
			incremental::definitions(&source)
		};
		let mut reader = Reader {
			opaque: RefCell::new(BTreeMap::new()),
			source: &source,
			dialect,
			lines,
			footnotes,
			document_footnotes: &document_footnotes,
			footnote_blocks: HashMap::new(),
			snippet_end: None,
			producer: Rc::new(RefCell::new(match after {
				Some(after) => Producer::continuing(revision, Some(after)),
				None => Producer::new(revision),
			})),
			definitions: &definitions,
			limits: crate::limits::Limits::default(),
			reparse_used: Rc::new(Cell::new(0)),
			reparse_limit: source
				.len()
				.saturating_mul(8)
				.clamp(1 << 20, 64 << 20),
		};
		let mut blocks = reader.blocks(root, 0);
		let mut notes: Vec<_> = reader
			.footnote_blocks
			.drain()
			.filter(|(name, _)| {
				reader.footnotes.contains_key(name)
					|| document_footnotes.contains(name)
			})
			.collect();
		notes.sort_by(|(left, _), (right, _)| {
			(reader.footnotes.get(left), left)
				.cmp(&(reader.footnotes.get(right), right))
		});
		for (name, note) in notes {
			let BlockKind::Footnote {
				id,
				number,
				blocks: body,
			} = note.kind().clone()
			else {
				continue;
			};
			let number = reader.footnotes.get(&name).copied().unwrap_or(number);
			let note = note.with_kind(BlockKind::Footnote {
				id,
				number,
				blocks: body,
			});
			blocks.push(note);
		}
		incremental::relabel_headings(&mut blocks);
		let producer = Rc::try_unwrap(reader.producer)
			.ok()
			.expect("snippet readers are dropped")
			.into_inner();
		(blocks, producer)
	};
	let (document, map) = producer.finish(blocks);
	MarkdownParse {
		document,
		source,
		map,
		dialect,
	}
}

/// The comrak configuration every Markdown parse shares, including the body
/// of a `<details>` element. The dialect's extensions are on; the defaults
/// every dialect shares are declared here once.
fn markdown_options(dialect: Dialect) -> Options<'static> {
	let mut options = Options::default();
	options.extension.table = true;
	options.extension.wikilinks_title_after_pipe = true;
	// A document may open with `---` fenced YAML metadata. Comrak only splits
	// it off: it hands the block back verbatim, delimiters included, and the
	// YAML itself is read in `front_matter`.
	options.extension.front_matter_delimiter = Some("---".into());
	options.extension.strikethrough = true;
	// Only the highlight dialect reads `==` as a mark; otherwise the source
	// stays literal text.
	options.extension.highlight = dialect.highlight_marks;
	options.extension.tasklist = true;
	options.extension.autolink = true;
	options.extension.footnotes = true;
	// OFM callouts and GFM alerts share syntax but have different semantics.
	options.extension.alerts = !dialect.ofm;
	options.extension.ofm_callouts = dialect.ofm;
	// `%%` comments parse as skipped nodes with original source positions.
	options.extension.ofm_comments = dialect.ofm;
	options.extension.math_dollars = true;
	options.extension.math_latex = true;
	options.extension.math_code = true;
	// CommonMark's flanking rules miss emphasis that ends next to CJK text,
	// as in `**重要です。**但`, where the closing run follows punctuation.
	options.extension.cjk_friendly_emphasis = true;
	options
}

/// Image formats an OFM `![[...]]` embed reads as a picture.
const WIKILINK_IMAGE_EXTENSIONS: &[&str] =
	&["bmp", "gif", "ico", "jpeg", "jpg", "png", "svg", "webp"];

/// The Markview URL of an Obsidian wikilink or embed; literal `%` is escaped
/// so the link resolver decodes only the encoding Markview adds itself.
fn wikilink_url(link: &NodeWikiLink) -> String {
	link.url.split_once('#').map_or_else(
		|| link.url.replace('%', "%25"),
		|(path, heading)| {
			let path = path.replace('%', "%25");
			format!(
				"{path}#{}",
				crate::heading_slug(
					heading.rsplit('#').next().unwrap_or(heading)
				)
			)
		},
	)
}

/// Whether an embed names a picture, so note transclusion stays a link.
fn wikilink_image(link: &NodeWikiLink) -> bool {
	let path = link
		.url
		.split_once('#')
		.map_or(link.url.as_str(), |(p, _)| p);
	path.rsplit_once('.').is_some_and(|(_, extension)| {
		WIKILINK_IMAGE_EXTENSIONS
			.iter()
			.any(|known| extension.trim().eq_ignore_ascii_case(known))
	})
}

/// The vault-relative source of an image embed; a heading is not part of the
/// file name, and literal `%` stays literal after the path decoder runs.
fn wikilink_asset_url(link: &NodeWikiLink) -> String {
	let path = link
		.url
		.split_once('#')
		.map_or(link.url.as_str(), |(p, _)| p);
	path.replace('%', "%25")
}

/// Whether the embed spells an explicit label. A table normalizes `\|` into a
/// plain pipe before the wikilink parser sees it.
fn wikilink_has_label(source: &str, in_table: bool) -> bool {
	let mut escaped = false;
	for ch in source.chars() {
		match ch {
			'\\' if !escaped => escaped = true,
			'|' if !escaped || in_table => return true,
			_ => escaped = false,
		}
	}
	false
}

fn apply_patch(patch: &html::Patch, style: &mut TextStyle, origin: usize) {
	match patch {
		html::Patch::Bold => style.bold = true,
		html::Patch::Italic => style.italic = true,
		html::Patch::Strike => style.strike = true,
		html::Patch::Code => style.code = true,
		html::Patch::Superscript => style.superscript = true,
		html::Patch::Subscript => style.subscript = true,
		html::Patch::Mark => style.mark = true,
		html::Patch::Link(url) => {
			style.link = Some(source_link(url.as_str(), origin));
		}
		html::Patch::None => {}
	}
}

/// Map snippet bytes to original lines, allowing removed quote prefixes and synthetic breaks.
fn source_offsets(
	text: &str,
	original: &str,
	source: Range<usize>,
) -> Vec<usize> {
	let raw = &original[source.clone()];
	let mut offsets = vec![source.start; text.len() + 1];
	let mut cursor = 0;
	let lines = line_starts(text);
	for (index, &at) in lines.iter().enumerate() {
		let line =
			&text[at..lines.get(index + 1).copied().unwrap_or(text.len())];
		let content = line.trim_end_matches(['\r', '\n']);
		if !content.is_empty()
			&& let Some(found) = raw[cursor..].find(content)
		{
			cursor += found;
			for i in 0..content.len() {
				offsets[at + i] = source.start + cursor + i;
			}
			cursor += content.len();
		}
		for i in content.len()..line.len() {
			offsets[at + i] = source.start + cursor;
			if raw.as_bytes().get(cursor) == Some(&line.as_bytes()[i]) {
				cursor += 1;
			}
		}
	}
	offsets[text.len()] = source.start + cursor;
	offsets
}

fn html_rich(spans: Vec<html::Span>, source: &Range<usize>) -> SpannedText {
	SpannedText::from_html_spans(spans, source)
}

fn plain_rich(text: String, source: Range<usize>) -> SpannedText {
	SpannedText::plain(text, source)
}

/// The field spans a block's own rich text contributes, with the whole block
/// source as its approximate coverage.
/// Maps a code block's literal body onto its source range.
fn code_spans(
	source_text: &str,
	source: &Range<usize>,
	text: &str,
) -> Vec<SourceSpan> {
	let raw = source_text.get(source.clone());
	let body = raw.map_or(0, |raw| {
		let trimmed = raw.trim_start();
		if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
			raw.find('\n').map_or(raw.len(), |at| at + 1)
		} else {
			0
		}
	});
	if let Some(raw) = raw
		&& let Some(at) = raw[body..].find(text)
	{
		let at = body + at;
		return vec![SourceSpan {
			semantic: 0..text.len(),
			source: source.start + at..source.start + at + text.len(),
			exact: true,
		}];
	}
	// Indented and normalized code is not a contiguous slice. Align each
	// character with the next matching character in the source, preserving
	// line and column navigation instead of collapsing to one covering span.
	let Some(raw) = source_text.get(source.clone()) else {
		return vec![SourceSpan {
			semantic: 0..text.len(),
			source: source.clone(),
			exact: false,
		}];
	};
	let mut spans: Vec<SourceSpan> = Vec::new();
	let mut cursor = body;
	for (offset, ch) in text.char_indices() {
		let mapped = raw[cursor..].find(ch).map(|found| {
			let start = cursor + found;
			cursor = start + ch.len_utf8();
			source.start + start..source.start + cursor
		});
		let semantic = offset..offset + ch.len_utf8();
		match (mapped, spans.last_mut()) {
			(Some(source), Some(previous))
				if previous.semantic.end == semantic.start
					&& previous.source.end == source.start =>
			{
				previous.semantic.end = semantic.end;
				previous.source.end = source.end;
			}
			(Some(source), _) => spans.push(SourceSpan {
				semantic,
				source,
				exact: true,
			}),
			(None, _) => {
				if let Some(previous) = spans.last_mut() {
					previous.semantic.end = semantic.end;
				} else {
					spans.push(SourceSpan {
						semantic: semantic.clone(),
						source: source.clone(),
						exact: false,
					});
				}
			}
		}
	}
	if spans.is_empty() {
		spans.push(SourceSpan {
			semantic: 0..text.len(),
			source: source.clone(),
			exact: false,
		});
	}
	spans
}

fn uniform_spans(text: &RichText, source: Range<usize>) -> Vec<SourceSpan> {
	let mut spans = Vec::with_capacity(text.len());
	let mut semantic = 0;
	for inline in text {
		let projection = plain_text(std::slice::from_ref(inline));
		let end = semantic + projection.len();
		spans.push(SourceSpan {
			semantic: semantic..end,
			source: source.clone(),
			exact: false,
		});
		semantic = end;
	}
	spans
}

/// The field spans of a block whose fields are all plain rich text or code.
fn own_spans(kind: &BlockKind, source: &Range<usize>) -> Vec<Vec<SourceSpan>> {
	match kind {
		BlockKind::Paragraph(text) | BlockKind::Heading { text, .. } => {
			vec![uniform_spans(text, source.clone())]
		}
		BlockKind::Code { text, .. } => vec![vec![SourceSpan {
			semantic: 0..text.len(),
			source: source.clone(),
			exact: false,
		}]],
		_ => Vec::new(),
	}
}

fn code_block_kind(
	c: &comrak::nodes::NodeCodeBlock,
	_source: &Range<usize>,
) -> BlockKind {
	if c.info.split_whitespace().next() == Some("mermaid") {
		BlockKind::Paragraph(vec![Inline {
			link: None,
			kind: InlineKind::Image(Box::new(crate::image::ImageSpec {
				source: crate::image::ImageSource::Diagram(
					crate::image::Diagram {
						language: "mermaid".into(),
						source: c.literal.clone(),
					},
				),
				// An empty `alt` draws no caption and keeps the
				// fence source out of the reading text, so only
				// the placeholder message is selectable.
				alt: String::new(),
				title: String::new(),
				width: None,
				height: None,
			})),
			style: markview_document::TextStyle::default(),
		}])
	} else if c.info.trim() == "math" {
		BlockKind::Paragraph(vec![Inline {
			link: None,
			kind: InlineKind::Math {
				latex: c.literal.clone(),
				display: true,
			},
			style: markview_document::TextStyle::default(),
		}])
	} else {
		BlockKind::Code {
			language: c.info.clone(),
			text: c.literal.clone(),
		}
	}
}

fn html_block_kind(
	literal: &str,
	scopes: usize,
	source: &Range<usize>,
	ofm: bool,
) -> Option<BlockKind> {
	Some(match html::block(literal, scopes, source.start, ofm) {
		html::Block::Unsupported => BlockKind::Code {
			language: "HTML source".into(),
			text: literal.to_owned(),
		},
		html::Block::Empty => return None,
		html::Block::Rule => BlockKind::Rule,
		html::Block::Heading { level, text } => BlockKind::Heading {
			level,
			text: html_rich(text, source).inlines,
			anchor: String::new(),
		},
		html::Block::Paragraph(text) => {
			BlockKind::Paragraph(html_rich(text, source).inlines)
		}
	})
}
