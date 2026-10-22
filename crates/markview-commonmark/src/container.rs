//! HTML container-stack semantics.
//!
//! `html.rs` is the tag lexer: it turns bytes into tags and into
//! [`html::BlockEvent`]s. This module owns what those events mean: which
//! elements open a container, how an end tag pairs with the nearest open one,
//! which element contents are opaque, and how a closed frame becomes blocks.
//! `parse.rs` keeps the source coordinates and the reader cursor; it maps an
//! event's range back to the original text and implements [`ContainerHost`]
//! for leaf rendering and summary text.

use std::ops::Range;

use crate::html;

use super::{Block, BlockKind, RichText, fingerprint, semantic_key};

/// Elements whose content is not markup for the reader: raw text, RCDATA,
/// foreign content, and `<pre>`, which the reader shows as literal source.
const OPAQUE_ELEMENTS: &[&str] =
	&["math", "pre", "script", "style", "svg", "textarea", "title"];

/// Opaque elements whose content ends at the first matching close tag.
const RAW_ELEMENTS: &[&str] = &["script", "style", "textarea", "title"];

/// The offset just past the opaque element `name` whose opening tag ends at
/// `start`, when `name` is opaque. An unclosed element is skipped to the end
/// of `source`.
pub(crate) fn opaque_end(
	source: &str,
	start: usize,
	name: &str,
) -> Option<usize> {
	if RAW_ELEMENTS.contains(&name) {
		return Some(raw_end(source, start, name));
	}
	if OPAQUE_ELEMENTS.contains(&name) {
		return Some(
			html::enclosed_end(source, start, name).unwrap_or(source.len()),
		);
	}
	None
}

/// The offset just past the first `</name>` at or after `start`, or the end
/// of `source` when the element never closes.
fn raw_end(source: &str, start: usize, name: &str) -> usize {
	let mut at = start;
	while let Some(found) = source[at..].find("</") {
		let found = at + found;
		let rest = &source[found..];
		if let Some(len) = html::tag_len(rest)
			&& let Some((tag, _, closing)) = html::tag_parts(&rest[..len])
			&& closing
			&& tag == name
		{
			return found + len;
		}
		at = found + 2;
	}
	source.len()
}

/// The end of an opaque element starting at this opening tag.
pub(crate) fn opaque_len(source: &str) -> Option<usize> {
	let len = html::tag_len(source)?;
	let (name, attrs, closing) = html::tag_parts(&source[..len])?;
	if closing || attrs.trim_end().ends_with('/') {
		return None;
	}
	opaque_end(source, len, &name)
}

/// One open raw-HTML container while a sibling list is walked.
struct Frame {
	anchors: Vec<String>,
	kind: FrameKind,
	blocks: Vec<Block>,
	/// The opener's HTML literal, used when the element never closes.
	raw: String,
	/// Source offset where the opener begins.
	start: usize,
}

impl Frame {
	/// Whether a closing tag `name` closes this frame.
	fn matches(&self, name: &str) -> bool {
		match &self.kind {
			FrameKind::Transparent { name: open } => name == open,
			FrameKind::Details { .. } => name == "details",
		}
	}
}

enum FrameKind {
	Transparent {
		name: String,
	},
	Details {
		open: bool,
		ordinal: u32,
		summary: Option<(String, Range<usize>)>,
	},
}

/// The parser operations the stack calls back into.
pub(crate) trait ContainerHost {
	/// Render one raw-HTML leaf at its original `range`.
	fn leaf(
		&mut self,
		text: &str,
		range: Range<usize>,
		opaque_until: usize,
	) -> Option<Block>;
	/// Render one `<summary>` body.
	fn summary(&mut self, text: &str, source: &Range<usize>) -> RichText;
	/// The end of the opaque interval containing `at`, or `0`.
	fn opaque_until(&self, at: usize) -> usize;
	/// Allocate the next document-wide `<details>` ordinal.
	fn next_details_ordinal(&mut self) -> u32;
}

/// The open raw-HTML containers of one sibling list.
#[derive(Default)]
pub(crate) struct ContainerStack {
	frames: Vec<Frame>,
}

impl ContainerStack {
	pub(crate) fn len(&self) -> usize {
		self.frames.len()
	}
	/// The block list the next block belongs to: the innermost frame, or the
	/// sibling list when no container is open.
	pub(crate) fn target<'a>(
		&'a mut self,
		out: &'a mut Vec<Block>,
	) -> &'a mut Vec<Block> {
		self.frames
			.last_mut()
			.map_or(out, |frame| &mut frame.blocks)
	}

	pub(crate) fn push_block(&mut self, out: &mut Vec<Block>, block: Block) {
		self.target(out).push(block);
	}

	pub(crate) fn extend_blocks(
		&mut self,
		out: &mut Vec<Block>,
		blocks: Vec<Block>,
	) {
		self.target(out).extend(blocks);
	}
	fn open(
		&mut self,
		name: &str,
		open: bool,
		ordinal: u32,
		anchors: Vec<String>,
		raw: String,
		start: usize,
	) {
		let kind = if name == "details" {
			FrameKind::Details {
				open,
				ordinal,
				summary: None,
			}
		} else {
			FrameKind::Transparent {
				name: name.to_owned(),
			}
		};
		self.frames.push(Frame {
			anchors,
			kind,
			blocks: Vec::new(),
			raw,
			start,
		});
	}

	/// Pop the frame a closing tag names, or report that none matches so the
	/// caller can keep the literal fallback.
	fn close<H: ContainerHost>(
		&mut self,
		name: &str,
		end: usize,
		source: &str,
		out: &mut Vec<Block>,
		host: &mut H,
	) -> bool {
		// An end tag closes the nearest matching frame and implicitly closes
		// anything still open inside it.
		let Some(index) =
			self.frames.iter().rposition(|frame| frame.matches(name))
		else {
			return false;
		};
		while self.frames.len() > index {
			let frame = self.frames.pop().expect("index is in range");
			self.finish(frame, end, source, out, host);
		}
		true
	}

	/// Close one frame into its parent: a transparent frame flattens and a
	/// details frame becomes one block.
	fn finish<H: ContainerHost>(
		&mut self,
		mut frame: Frame,
		end: usize,
		source: &str,
		out: &mut Vec<Block>,
		host: &mut H,
	) {
		match frame.kind {
			FrameKind::Transparent { .. } => {
				let anchors = frame.anchors;
				if !anchors.is_empty() {
					let mut block = block_of(
						source,
						BlockKind::Paragraph(Vec::new()),
						frame.start..frame.start + frame.raw.len(),
					);
					block.anchors = anchors;
					if let Some(first) = frame.blocks.first_mut() {
						first.anchors.splice(0..0, block.anchors);
					} else {
						frame.blocks.push(block);
					}
				}
				self.extend_blocks(out, frame.blocks);
			}
			FrameKind::Details {
				open,
				ordinal,
				summary,
			} => {
				let span = frame.start..end.max(frame.start);
				let summary = summary
					.as_ref()
					.map_or_else(RichText::new, |(text, source)| {
						host.summary(text, source)
					});
				let mut block = details_block_of(
					source,
					open,
					ordinal,
					summary,
					frame.blocks,
					span,
				);
				block.anchors = frame.anchors;
				self.push_block(out, block);
			}
		}
	}

	/// Feed one raw HTML block's container events into the open frames.
	#[expect(clippy::too_many_arguments, reason = "one HTML block's inputs")]
	pub(crate) fn consume<H: ContainerHost>(
		&mut self,
		events: &[html::BlockEvent],
		text: &str,
		map: impl Fn(Range<usize>) -> Range<usize>,
		opaque_until: usize,
		depth: usize,
		block_depth: usize,
		source: &str,
		host: &mut H,
		out: &mut Vec<Block>,
	) {
		for event in events {
			match event {
				html::BlockEvent::Text { text: leaf, range } => {
					let at = map(range.clone());
					if let Some(block) = host.leaf(leaf, at, opaque_until) {
						self.push_block(out, block);
					}
				}
				html::BlockEvent::Summary { text: body, range } => {
					if !self.attach_summary(text, &map, body, range, host) {
						let html = &text[range.clone()];
						let at = map(range.clone());
						if let Some(block) = host.leaf(html, at, opaque_until) {
							self.push_block(out, block);
						}
					}
				}
				html::BlockEvent::Open { name, open, range } => {
					let at = map(range.clone());
					if depth + self.len() >= block_depth {
						// Too deep: keep the tag as source instead of nesting.
						if let Some(block) =
							host.leaf(&text[range.clone()], at, opaque_until)
						{
							self.push_block(out, block);
						}
						continue;
					}
					let anchors = if host.opaque_until(at.start) == 0 {
						html::anchors(&text[range.clone()])
					} else {
						Vec::new()
					};
					let ordinal = if name == "details" {
						host.next_details_ordinal()
					} else {
						0
					};
					self.open(
						name,
						*open,
						ordinal,
						anchors,
						text[range.clone()].to_owned(),
						at.start,
					);
				}
				html::BlockEvent::Close { name, range } => {
					let at = map(range.clone());
					if !self.close(name, at.end, source, out, host) {
						// A stray close keeps only its own fragment as source.
						if let Some(block) =
							host.leaf(&text[range.clone()], at, opaque_until)
						{
							self.push_block(out, block);
						}
					}
				}
			}
		}
	}

	/// Attach a `<summary>` to the innermost open disclosure.
	fn attach_summary<H: ContainerHost>(
		&mut self,
		source_text: &str,
		map: &impl Fn(Range<usize>) -> Range<usize>,
		body: &str,
		range: &Range<usize>,
		host: &mut H,
	) -> bool {
		let Some(frame) = self.frames.last_mut() else {
			return false;
		};
		let FrameKind::Details { summary, .. } = &mut frame.kind else {
			return false;
		};
		if summary.is_some() {
			return false;
		}
		let fragment = &source_text[range.clone()];
		let opener_len =
			html::tag_len(fragment).expect("summary event holds a tag");
		let body_start = range.start + opener_len;
		let body_source = map(body_start..body_start + body.len());
		*summary = Some((body.to_owned(), body_source));
		let at = map(range.clone()).start;
		if host.opaque_until(at) == 0 {
			frame.anchors.extend(html::anchors(&fragment[..opener_len]));
		}
		frame.raw.push_str(fragment);
		true
	}

	/// Unclosed containers keep the literal fallback: the opener is shown as
	/// HTML source and its collected content follows it.
	pub(crate) fn pop_unclosed<H: ContainerHost>(
		&mut self,
		out: &mut Vec<Block>,
		host: &mut H,
	) {
		while let Some(frame) = self.frames.pop() {
			let mut blocks = Vec::new();
			let opaque_until = host.opaque_until(frame.start);
			if let Some(block) = host.leaf(
				&frame.raw,
				frame.start..frame.start.saturating_add(frame.raw.len()),
				opaque_until,
			) {
				blocks.push(block);
			}
			blocks.extend(frame.blocks);
			self.extend_blocks(out, blocks);
		}
	}
}

pub(crate) fn clamp_range(source: &str, range: Range<usize>) -> Range<usize> {
	let mut start = range.start.min(source.len());
	let mut end = range.end.min(source.len());
	while !source.is_char_boundary(start) {
		start -= 1;
	}
	while !source.is_char_boundary(end) {
		end -= 1;
	}
	start..end.max(start)
}

pub(crate) fn block_of(
	source: &str,
	kind: BlockKind,
	range: Range<usize>,
) -> Block {
	let range = clamp_range(source, range);
	let id =
		fingerprint(&(std::mem::discriminant(&kind), &source[range.clone()]));
	Block {
		anchors: Vec::new(),
		id,
		content_key: semantic_key(&kind),
		source: range,
		kind,
	}
}

pub(crate) fn details_block_of(
	source: &str,
	open: bool,
	ordinal: u32,
	summary: RichText,
	blocks: Vec<Block>,
	range: Range<usize>,
) -> Block {
	let range = clamp_range(source, range);
	let kind = BlockKind::Details {
		open,
		ordinal,
		summary,
		blocks,
	};
	let id = fingerprint(&(
		std::mem::discriminant(&kind),
		&source[range.clone()],
		ordinal,
	));
	Block {
		anchors: Vec::new(),
		id,
		content_key: semantic_key(&kind),
		source: range,
		kind,
	}
}

#[cfg(test)]
mod tests;
