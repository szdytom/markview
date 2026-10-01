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
	// A block with an empty source range cannot be positioned against the
	// change window, so the ranges it would be shifted by are untrustworthy;
	// the full parse is the correct answer.
	if previous.blocks.iter().any(|block| block.source.is_empty()) {
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
	let end_old =
		line_end(&previous.source, group_end(&previous.source, changed.end));
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
	let window = &source[origin..end];
	// The window is parsed as its own document, and a parser drops a BOM only
	// at the very start of a document. A window opening on a mid-document BOM
	// would parse its first block shorter than the full parse does, so the
	// BOM is an out-of-fast-path marker like any other.
	let replaced = if window.starts_with('\u{feff}') {
		None
	} else {
		Some(super::parse(window.to_owned()).blocks)
	};
	let mut replaced = replaced?;
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
	// The slice must end the line it cuts: a bare list marker with no line
	// ending parses as a paragraph where the document's own line ends a list
	// item.
	let end = line_end(source, end);
	// Front matter is one block however many blank lines it holds, and comrak
	// only recognizes it once the closing delimiter arrives. A cut inside it
	// would parse the opening `---` as a thematic break, which the document
	// never puts there.
	if let Some(close) = front_matter_close(source)
		&& end < close
	{
		return None;
	}
	// A raw HTML block is read to its closing token, not to the cut: without
	// the token the slice parses to ordinary text where the document has one
	// block, or none at all.
	if ends_in_open_html(&source[..end]) {
		return None;
	}
	// An SVG image is atomic even when blank lines split its raw HTML nodes.
	let mut at = 0;
	while let Some(open) = source[at..end].find("<svg") {
		let open = at + open;
		if let Some(len) = crate::html::svg_len(&source[open..]) {
			if open + len > end {
				return None;
			}
			at = open + len;
		} else {
			at = open + 4;
		}
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
	let definitions = missing_definitions(source, bare);
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

/// The definitions of `source` that `bare` does not already hold.
///
/// A definition inside the prefix resolves natively, and appending a second
/// copy of it changes how the parser reads the prefix: comrak keeps a repeated
/// footnote definition where a lone unreferenced one disappears, so the copy
/// would add a block the document does not have.
fn missing_definitions(source: &str, bare: &str) -> String {
	let all = definitions(source);
	let present = definitions(bare);
	if present.is_empty() {
		return all;
	}
	let present: Vec<&str> =
		present.lines().filter_map(definition_label).collect();
	all.lines()
		.filter(|line| {
			!definition_label(line).is_some_and(|l| present.contains(&l))
		})
		.map(|line| format!("{line}\n"))
		.collect()
}

/// The `[label` of one line `definitions` emitted, for comparing two runs.
fn definition_label(line: &str) -> Option<&str> {
	line.split_once("]:").map(|(label, _)| label)
}

/// The reference and footnote definitions of `source`, rewritten so they can be
/// appended to a prefix.
///
/// Only column-zero lines outside a fence are taken, because a `[x]: ...` line
/// inside code is text the full parse would not resolve either. A definition
/// whose destination is on the next line, or a footnote body, is not needed:
/// the reference only needs its target and its number.
///
/// A link reference definition cannot interrupt a paragraph, so a `[x]: y` line
/// that continues one is ordinary text. Taking it as a definition would let the
/// appended copy resolve a reference the full parse leaves alone, changing a
/// prefix block or a `<details>` body that the document itself parses as plain
/// text.
pub(super) fn definitions(source: &str) -> String {
	let mut out = String::new();
	let mut fence = None;
	// Whether a paragraph is open going into this line, which is what decides
	// whether a `[x]: y` line can start a definition here.
	let mut paragraph = false;
	// The parser's lines: a lone carriage return ends one too, so `lines()`
	// would glue a definition onto the text before it and miss the marker.
	let ranges = line_ranges(source);
	for range in &ranges {
		let line = &source[range.clone()];
		let rest = line.trim_start_matches(' ');
		let indent = line.len() - rest.len();
		if indent <= 3 && (rest.starts_with("```") || rest.starts_with("~~~")) {
			let marker = rest.as_bytes()[0];
			if fence == Some(marker) {
				fence = None;
			} else if fence.is_none() {
				fence = Some(marker);
			}
			// A fence interrupts a paragraph, and its lines are code.
			paragraph = false;
			continue;
		}
		if fence.is_some() {
			continue;
		}
		if indent == 0
			&& !paragraph
			&& rest.starts_with('[')
			&& let Some(close) = rest.find("]:")
		{
			let label = &rest[1..close];
			let value = rest[close + 2..].trim();
			if !label.is_empty() && !value.is_empty() {
				out.push('[');
				out.push_str(label);
				out.push_str("]: ");
				// A note needs only its label to take the number a reference
				// expects.
				out.push_str(if label.starts_with('^') { "x" } else { value });
				out.push('\n');
				paragraph = false;
				continue;
			}
		}
		paragraph = continues_paragraph(line, rest, indent, paragraph);
	}
	out
}

/// Whether a line leaves a paragraph open for the line after it.
///
/// A heading or a thematic break ends one; ordinary text, a list marker, a
/// block quote, raw HTML and indented code all leave the next column-zero line
/// as a continuation too, because a definition cannot interrupt any of them
/// either. Indented text inside an open paragraph is a continuation; on its
/// own it opens an indented code block, which the next unindented line ends.
fn continues_paragraph(
	line: &str,
	rest: &str,
	indent: usize,
	open: bool,
) -> bool {
	if indent >= 4 {
		return open;
	}
	!blank(line) && !atx_heading(rest) && !thematic_break(rest)
}

/// An ATX heading: one to six `#` followed by a space, a tab, or the line end.
fn atx_heading(rest: &str) -> bool {
	let hashes = rest.bytes().take_while(|b| *b == b'#').count();
	(1..=6).contains(&hashes)
		&& matches!(rest.as_bytes().get(hashes), None | Some(b' ' | b'\t'))
}

/// A thematic break: at least three of one of `*`, `-`, `_`, spaces between.
fn thematic_break(rest: &str) -> bool {
	let mut marker = None;
	let mut count = 0;
	for c in rest.chars() {
		match c {
			' ' | '\t' => {}
			'*' | '-' | '_' => {
				if *marker.get_or_insert(c) != c {
					return false;
				}
				count += 1;
			}
			_ => return false,
		}
	}
	count >= 3
}

/// Whether no reference definition or footnote needs source the cut would
/// leave behind.
pub(super) fn definition_free(source: &str) -> bool {
	!source.contains("]:") && !source.contains("[^")
}

/// Whether every top-level block is a leaf delimited by blank lines: no
/// container, fence, indented or HTML code, table, or reference definition.
///
/// Lines are the parser's lines: a lone carriage return breaks a line too,
/// so a marker hidden after a mid-line carriage return still counts as a
/// list line and takes the document off the fast path.
fn leaf_only(source: &str) -> bool {
	if !definition_free(source) || source.contains('|') {
		return false;
	}
	line_ranges(source)
		.iter()
		.all(|range| is_leaf_line(&source[range.clone()]))
}

fn is_leaf_line(line: &str) -> bool {
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

/// Whether a line is blank to the Markdown parser: only spaces and tabs. A
/// carriage return is a line ending in this model and never line content.
/// Unicode spaces such as NBSP are content, so treating them as blank would
/// cut through a paragraph.
fn blank(line: &str) -> bool {
	line.bytes().all(|b| matches!(b, b' ' | b'\t'))
}

/// The content range of every line of `source`, where a line ends at `\n`,
/// `\r\n`, or a lone `\r`, matching the parser's line structure. The group
/// and window boundaries this path cuts must fall where the parser cuts, or
/// a reused block's range does not match the one a full parse assigns.
fn line_ranges(source: &str) -> Vec<Range<usize>> {
	let bytes = source.as_bytes();
	let mut lines = Vec::new();
	let mut start = 0usize;
	let mut i = 0usize;
	while i < bytes.len() {
		let terminated = matches!(bytes[i], b'\n' | b'\r');
		if terminated {
			lines.push(start..i);
			if bytes[i] == b'\r' && i + 1 < bytes.len() && bytes[i + 1] == b'\n'
			{
				i += 2;
			} else {
				i += 1;
			}
			start = i;
		} else {
			i += 1;
		}
	}
	lines.push(start..source.len());
	lines
}

/// Whether the line at `index` of `lines` is blank.
fn line_blank(lines: &[Range<usize>], source: &str, index: usize) -> bool {
	blank(&source[lines[index].clone()])
}

/// The line of `source` that owns the byte at `at`: a terminator position
/// belongs to the line it ends.
fn line_at(lines: &[Range<usize>], at: usize) -> usize {
	lines
		.iter()
		.position(|line| at <= line.end)
		.unwrap_or(lines.len() - 1)
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
	let lines = line_ranges(source);
	let mut index = line_at(&lines, at.min(source.len()));
	while index > 0 && !line_blank(&lines, source, index - 1) {
		index -= 1;
	}
	lines[index].start
}

/// The offset of the line end after the run of non-blank lines holding `at`,
/// or the end of the source.
fn group_end(source: &str, at: usize) -> usize {
	let lines = line_ranges(source);
	let mut index = line_at(&lines, at.min(source.len()));
	while index + 1 < lines.len() && !line_blank(&lines, source, index + 1) {
		index += 1;
	}
	lines[index].end
}

/// `end`, extended over the line ending that follows it. A slice that stops at
/// a line's content end has no terminator, and the parser can read the last
/// line differently without one.
fn line_end(source: &str, end: usize) -> usize {
	match source.as_bytes().get(end) {
		Some(b'\n') => end + 1,
		Some(b'\r') => {
			end + 1
				+ usize::from(source.as_bytes().get(end + 1) == Some(&b'\n'))
		}
		_ => end,
	}
}

/// The offset just past the line that closes the front matter `source` opens,
/// or `None` when the document opens none or never closes one. Comrak closes
/// on the next line that is exactly `---`; without one, the opening line is an
/// ordinary thematic break and a prefix may cut through it.
fn front_matter_close(source: &str) -> Option<usize> {
	let lines = line_ranges(source);
	if lines.first().map(|range| &source[range.clone()]) != Some("---") {
		return None;
	}
	lines
		.into_iter()
		.skip(1)
		.find(|range| &source[range.clone()] == "---")
		.map(|range| range.end)
}

/// Whether `source` ends inside a raw HTML construct that is still open.
///
/// Only the last line that can open an HTML block counts: an earlier closed
/// one does not matter, and a later ordinary line is inside the block the
/// opener began. A `<` anywhere else is ordinary text.
fn ends_in_open_html(source: &str) -> bool {
	for range in line_ranges(source).iter().rev() {
		let line = &source[range.clone()];
		let indent = line.len() - line.trim_start_matches(' ').len();
		if indent > 3 {
			continue;
		}
		let rest = &line[indent..];
		let Some(after) = rest.strip_prefix('<') else {
			continue;
		};
		let opens = matches!(
			after.as_bytes().first(),
			Some(b) if b.is_ascii_alphabetic() || matches!(b, b'/' | b'!' | b'?')
		);
		if !opens {
			continue;
		}
		return crate::html::tag_len(rest).is_none();
	}
	false
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
		BlockKind::FrontMatter { blocks, .. } => {
			for block in blocks {
				shift(block, delta);
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
