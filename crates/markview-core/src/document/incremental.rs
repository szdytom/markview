//! Bounded parses: reusing the blocks a small edit did not touch, and parsing
//! only a prefix of a document that is opened for the first time.
//!
//! A change confined to one top-level block leaves every other block's bytes
//! and meaning identical, so only that block needs comrak. The rest is reused
//! by cloning it and shifting the source ranges of the blocks after the change,
//! which makes the cost of an edit follow the block it changed rather than the
//! document. This path only accepts documents whose top-level blocks are leaf
//! blocks separated by blank lines: lists, quotes, footnotes, fenced, indented
//! and HTML code, tables and reference definitions can span a blank line or
//! carry meaning outside the block that holds them, so a document with any of
//! them takes a full [`super::parse`].
//!
//! A prefix can tolerate more, because the complete parse follows and corrects
//! it: the only constructs a cut cannot leave half-resolved are reference
//! definitions and footnotes, whose meaning lives outside the block that names
//! them. A prefix that ends inside a code block or a list simply shows the part
//! of it that exists.
use super::{
	Anchors, Block, BlockKind, Document, RichText, content_identity,
	plain_text, semantic_key,
};
use std::{ops::Range, sync::Arc};

/// The document that `source` describes, reusing `previous`'s blocks where the
/// edit did not touch them, or `None` when the change or the document is
/// outside the fast path and the caller must [`super::parse`] instead.
pub fn parse_incremental(
	previous: &Document,
	source: Arc<str>,
) -> Option<Document> {
	if !leaf_only(&previous.source)
		|| !leaf_only(&source)
		|| previous.blocks.is_empty()
	{
		return None;
	}
	let (old, new) = (previous.source.as_bytes(), source.as_bytes());
	// A byte-wise prefix can end inside a UTF-8 character, so pull both ends
	// back to a character boundary.
	let mut prefix = common_prefix(old, new);
	while !previous.source.is_char_boundary(prefix) {
		prefix -= 1;
	}
	let mut suffix = common_suffix(&old[prefix..], &new[prefix..]);
	while !previous.source.is_char_boundary(old.len() - suffix)
		|| !source.is_char_boundary(new.len() - suffix)
	{
		suffix -= 1;
	}
	let changed = prefix..old.len() - suffix;
	let delta = new.len() as isize - old.len() as isize;
	// The window is the blank-line-delimited groups the change touches, so a
	// block that merges with its neighbour across the change is re-parsed with
	// it. Within those bounds `leaf_only` makes the old block boundaries hold.
	let origin = group_start(&previous.source, changed.start);
	let end_old = group_end(&previous.source, changed.end);
	let end = (end_old as isize + delta) as usize;
	let first = previous
		.blocks
		.partition_point(|block| block.source.end <= origin);
	let last = previous
		.blocks
		.partition_point(|block| block.source.start < end_old);
	if first > last
		|| origin > end
		|| end > new.len()
		|| !source.is_char_boundary(origin)
		|| !source.is_char_boundary(end)
	{
		return None;
	}
	// The changed window's bytes are replaced; everything before it is
	// unchanged and everything after it shifted by `delta`.
	let mut blocks = Vec::with_capacity(previous.blocks.len());
	blocks.extend(previous.blocks[..first].iter().cloned());
	let mut replaced = super::parse(source[origin..end].to_owned()).blocks;
	for block in &mut replaced {
		shift(block, origin as isize);
	}
	blocks.append(&mut replaced);
	for mut block in previous.blocks[last..].iter().cloned() {
		shift(&mut block, delta);
		blocks.push(block);
	}
	relabel_headings(&mut blocks);
	Some(Document {
		source,
		content_id: content_identity(&blocks),
		blocks,
	})
}

/// The document `source` describes, reusing `previous`'s untouched blocks when
/// it can and taking a full parse otherwise. A reader and a one-shot export
/// share one policy here, so neither can drift from the fast path.
pub fn reparse(previous: &Document, source: Arc<str>) -> Document {
	parse_incremental(previous, source.clone())
		.unwrap_or_else(|| super::parse(source))
}

/// The blocks at the start of `source` that `bytes` reaches, as a document over
/// the whole source, or `None` when `bytes` already covers it.
///
/// Every block before the cut is exactly the one a full parse would place
/// there, so a reader can show the opening viewport of a large file without
/// waiting for the whole parse, and the complete parse reuses their geometry.
pub fn parse_prefix(source: &Arc<str>, bytes: usize) -> Option<Document> {
	if bytes == 0 || bytes >= source.len() {
		return None;
	}
	let mut at = bytes;
	while !source.is_char_boundary(at) {
		at -= 1;
	}
	let end = group_end(source, at);
	if end == 0 {
		return None;
	}
	let blocks = prefix_blocks(source, end);
	Some(Document {
		source: source.clone(),
		content_id: content_identity(&blocks),
		blocks,
	})
}

/// The blocks of `source[..end]`, with every reference or note resolved from
/// the definitions that follow the cut.
fn prefix_blocks(source: &str, end: usize) -> Vec<Block> {
	let bare = &source[..end];
	if !bare.contains('[') {
		return super::parse(bare.to_owned()).blocks;
	}
	let definitions = definitions(source);
	if definitions.is_empty() {
		return super::parse(bare.to_owned()).blocks;
	}
	let mut slice = bare.to_owned();
	slice.push_str("\n\n");
	slice.push_str(&definitions);
	let parsed = super::parse(slice).blocks;
	// An open fence or HTML block can swallow the appended text; then only the
	// bare prefix parses to what the full parse would put there.
	if parsed
		.iter()
		.any(|block| block.source.start < end && block.source.end > end)
	{
		return super::parse(bare.to_owned()).blocks;
	}
	parsed
		.into_iter()
		.filter(|block| block.source.start < end)
		.collect()
}

/// The reference and footnote definitions of `source`, rewritten so they can be
/// appended to a prefix.
///
/// Only column-zero lines outside a fence are taken, because a `[x]: ...` line
/// inside code is text the full parse would not resolve either. A definition
/// whose destination is on the next line, or a footnote body, is not needed:
/// the reference only needs its target and its number.
pub(super) fn definitions(source: &str) -> String {
	let mut out = String::new();
	let mut fence = None;
	for line in source.lines() {
		let rest = line.trim_start_matches(' ');
		let indent = line.len() - rest.len();
		if indent <= 3 && (rest.starts_with("```") || rest.starts_with("~~~")) {
			let marker = rest.as_bytes()[0];
			if fence == Some(marker) {
				fence = None;
			} else if fence.is_none() {
				fence = Some(marker);
			}
			continue;
		}
		if fence.is_some() || indent > 0 || !rest.starts_with('[') {
			continue;
		}
		let Some(close) = rest.find("]:") else {
			continue;
		};
		let label = &rest[1..close];
		let value = rest[close + 2..].trim();
		if label.is_empty() || value.is_empty() {
			continue;
		}
		out.push('[');
		out.push_str(label);
		out.push_str("]: ");
		// A note needs only its label to take the number a reference expects.
		out.push_str(if label.starts_with('^') { "x" } else { value });
		out.push('\n');
	}
	out
}

/// Whether no reference definition or footnote needs source the cut would
/// leave behind.
pub(super) fn definition_free(source: &str) -> bool {
	!source.contains("]:") && !source.contains("[^")
}

/// Whether every top-level block is a leaf delimited by blank lines: no
/// container, fence, indented or HTML code, table, or reference definition.
fn leaf_only(source: &str) -> bool {
	if !definition_free(source) || source.contains('|') {
		return false;
	}
	source.lines().all(|line| {
		let indent = line.len() - line.trim_start_matches(' ').len();
		let rest = &line[indent..];
		indent < 4
			&& !rest.starts_with('\t')
			&& !rest.starts_with('>')
			&& !rest.starts_with('<')
			&& !rest.starts_with("```")
			&& !rest.starts_with("~~~")
			&& !list_marker(rest)
			// Front matter is one block however many blank lines it holds, so
			// the window this path cuts cannot contain it.
			&& !delimiter_line(rest)
	})
}

/// Whether a line is a bare `---`, which can only be a front-matter delimiter
/// or a thematic break. Neither is a leaf block: front matter spans blank
/// lines, and a break becomes a setext underline for the text above it.
fn delimiter_line(line: &str) -> bool {
	line.trim_end().len() == 3 && line.starts_with("---")
}

/// A bullet or ordered list marker, which keeps its list open across blank
/// lines. A marker at the end of its line starts an empty item, which is still
/// a list.
fn list_marker(line: &str) -> bool {
	let bullet = |bytes: &[u8]| {
		matches!(
			bytes,
			[b'-' | b'+' | b'*'] | [b'-' | b'+' | b'*', b' ' | b'\t', ..]
		)
	};
	if bullet(line.as_bytes()) {
		return true;
	}
	let digits = line.bytes().take_while(u8::is_ascii_digit).count();
	digits > 0
		&& digits <= 9
		&& matches!(
			&line.as_bytes()[digits..],
			[b'.' | b')'] | [b'.' | b')', b' ' | b'\t', ..]
		)
}

/// Whether a line is blank to the Markdown parser: only spaces, tabs and the
/// carriage returns of a CRLF ending. Unicode spaces such as NBSP are content,
/// so treating them as blank would cut through a paragraph.
fn blank(line: &str) -> bool {
	line.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\r'))
}

fn common_prefix(a: &[u8], b: &[u8]) -> usize {
	a.iter().zip(b).take_while(|(a, b)| a == b).count()
}

fn common_suffix(a: &[u8], b: &[u8]) -> usize {
	a.iter()
		.rev()
		.zip(b.iter().rev())
		.take_while(|(a, b)| a == b)
		.count()
}

/// The offset of the first line of the run of non-blank lines holding `at`.
fn group_start(source: &str, at: usize) -> usize {
	let at = at.min(source.len());
	let mut start = source[..at].rfind('\n').map_or(0, |i| i + 1);
	while start > 0 {
		let end = start - 1;
		let prev = source[..end].rfind('\n').map_or(0, |i| i + 1);
		if blank(&source[prev..end]) {
			break;
		}
		start = prev;
	}
	start
}

/// The offset of the newline ending the run of non-blank lines holding `at`,
/// or the end of the source.
fn group_end(source: &str, at: usize) -> usize {
	let at = at.min(source.len());
	let mut end = source[at..].find('\n').map_or(source.len(), |i| at + i);
	loop {
		let next = end + 1;
		if next >= source.len() {
			return end;
		}
		let line = source[next..].find('\n').map_or(source.len(), |i| next + i);
		if blank(&source[next..line]) {
			return end;
		}
		end = line;
	}
}

fn shift_range(range: &mut Range<usize>, delta: isize) {
	range.start = (range.start as isize + delta) as usize;
	range.end = (range.end as isize + delta) as usize;
}

fn shift_rich(ranges: &mut RichText, delta: isize) {
	for inline in ranges {
		shift_range(&mut inline.source, delta);
	}
}

fn shift(block: &mut Block, delta: isize) {
	shift_range(&mut block.source, delta);
	match &mut block.kind {
		BlockKind::Paragraph(text) | BlockKind::Heading { text, .. } => {
			shift_rich(text, delta)
		}
		BlockKind::Quote { blocks, .. }
		| BlockKind::Footnote { blocks, .. } => {
			for block in blocks {
				shift(block, delta);
			}
		}
		BlockKind::Details {
			summary, blocks, ..
		} => {
			shift_rich(summary, delta);
			for block in blocks {
				shift(block, delta);
			}
		}
		BlockKind::FrontMatter { table, .. } => {
			for cell in table.iter_mut().flatten().flatten() {
				shift_rich(cell, delta);
			}
		}
		BlockKind::List { items, .. } => {
			for item in items {
				for block in &mut item.blocks {
					shift(block, delta);
				}
			}
		}
		BlockKind::Table { rows, .. } => {
			for row in rows {
				for cell in row {
					shift_rich(cell, delta);
				}
			}
		}
		BlockKind::Code { .. } | BlockKind::Rule => {}
	}
}

/// Re-assigns heading anchors across the spliced block list. An edit can add,
/// remove or rename a heading, which changes the suffix every later heading
/// with the same slug takes.
fn relabel_headings(blocks: &mut [Block]) {
	fn walk(blocks: &mut [Block], anchors: &mut Anchors) {
		for block in blocks {
			let mut relabeled = false;
			match &mut block.kind {
				BlockKind::Heading { text, anchor, .. } => {
					let wanted = anchors.unique(&plain_text(text));
					relabeled = *anchor != wanted;
					*anchor = wanted;
				}
				BlockKind::Quote { blocks, .. }
				| BlockKind::Footnote { blocks, .. } => walk(blocks, anchors),
				BlockKind::Details { blocks, .. } => walk(blocks, anchors),
				BlockKind::List { items, .. } => {
					for item in items {
						walk(&mut item.blocks, anchors);
					}
				}
				_ => {}
			}
			if relabeled {
				block.content_key = semantic_key(&block.kind);
			}
		}
	}
	walk(blocks, &mut Anchors::default());
}
