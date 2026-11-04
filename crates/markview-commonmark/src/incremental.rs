//! Bounded parses: reusing the blocks a small edit did not touch, and parsing
//! only a prefix of a document that is opened for the first time.
//!
//! Source positions belong to the [`MarkdownParse`]'s map, so reusing a block
//! no longer shifts anything inside it: the map entries move instead.
use super::{Anchors, Block, BlockKind, Dialect, MarkdownParse, plain_text};
use markview_document::{
	BlockId, BlockSource, DocumentBuilder, DocumentRevision, SourceMap,
	visit_blocks,
};
use std::{collections::HashSet, ops::Range, sync::Arc};

/// The document that `source` describes, reusing `previous`'s blocks where the
/// edit did not touch them, or `None` when the change or the document is
/// outside the fast path and the caller must [`super::parse`] instead.
#[cfg_attr(
	any(),
	markview::allow_complexity(
		reason = "Keep the incremental window, identity reuse, source-map shifting and fallback conditions in one auditable place; each early return is an out-of-fast-path decision."
	)
)]
pub fn parse_incremental(
	previous: &MarkdownParse,
	source: Arc<str>,
	dialect: Dialect,
) -> Option<MarkdownParse> {
	if [&*previous.source, &*source]
		.into_iter()
		.any(|text| comments_need_full_parse(text, dialect))
	{
		return None;
	}
	if !leaf_only(&previous.source)
		|| !leaf_only(&source)
		|| previous.document.blocks().is_empty()
	{
		return None;
	}
	let ranges: Vec<Range<usize>> = previous
		.document
		.blocks()
		.iter()
		.map(|block| block_range(&previous.map, block.id()))
		.collect::<Option<Vec<_>>>()?;
	if ranges.iter().any(Range::is_empty) {
		return None;
	}
	let changed = changed_range(&previous.source, &source);
	let delta = source.len() as isize - previous.source.len() as isize;
	let origin = group_start(&previous.source, changed.start);
	let end_old =
		line_end(&previous.source, group_end(&previous.source, changed.end));
	let end = (end_old as isize + delta) as usize;
	let first = ranges.partition_point(|range| range.end <= origin);
	let last = ranges.partition_point(|range| range.start < end_old);
	if first > last
		|| origin > end
		|| end > source.len()
		|| !source.is_char_boundary(origin)
		|| !source.is_char_boundary(end)
	{
		return None;
	}
	let window = &source[origin..end];
	if window.starts_with('\u{feff}') {
		return None;
	}
	let after = previous
		.document
		.blocks()
		.iter()
		.flat_map(|block| {
			let mut ids = Vec::new();
			visit_blocks(block, &mut |block| ids.push(block.id()));
			ids
		})
		.max();
	let parsed =
		super::parse::parse_with_seed(window.to_owned(), dialect, None, after);
	let mut blocks = Vec::with_capacity(previous.document.blocks().len());
	let mut map = Vec::new();
	for block in &previous.document.blocks()[..first] {
		blocks.push(block.clone());
		if let Some(entry) = previous.map.block(block.id()) {
			map.push(entry.clone());
		}
	}
	for block in parsed.document.blocks() {
		let mut block = block.clone();
		// The window parsed as its own document, so its link occurrences are
		// window-local offsets. Move them onto the document's source.
		super::make::rebase_links(&mut block, &|range| {
			shift_range(&range, origin as isize)
		});
		blocks.push(block);
	}
	for entry in parsed.map.blocks() {
		map.push(shifted(entry, origin as isize));
	}
	for block in &previous.document.blocks()[last..] {
		let mut block = block.clone();
		// Suffix blocks keep their identity but move on the source, so their
		// occurrences move with the map.
		if delta != 0 {
			super::make::rebase_links(&mut block, &|range| {
				shift_range(&range, delta)
			});
		}
		let id = block.id();
		blocks.push(block);
		if let Some(entry) = previous.map.block(id) {
			map.push(shifted(entry, delta));
		}
	}
	relabel_headings(&mut blocks);
	let revision = DocumentRevision::fresh();
	let document = DocumentBuilder::new(revision).finish(blocks);
	Some(MarkdownParse {
		document,
		source,
		map: SourceMap::new(revision, map),
		dialect,
	})
}

fn shifted(entry: &BlockSource, delta: isize) -> BlockSource {
	let mut entry = entry.clone();
	entry.source = entry.source.as_ref().map(|range| shift_range(range, delta));
	for field in &mut entry.fields {
		for span in &mut field.spans {
			span.source = shift_range(&span.source, delta);
		}
		for cover in &mut field.covers {
			cover.source = shift_range(&cover.source, delta);
		}
	}
	entry
}

fn shift_range(range: &Range<usize>, delta: isize) -> Range<usize> {
	let at = |value: usize| (value as isize + delta).max(0) as usize;
	at(range.start)..at(range.end)
}

fn block_range(map: &SourceMap, block: BlockId) -> Option<Range<usize>> {
	map.block(block).and_then(|entry| entry.source.clone())
}

/// The document `source` describes, reusing `previous`'s untouched blocks when
/// it can and taking a full parse otherwise.
pub fn reparse(previous: &MarkdownParse, source: Arc<str>) -> MarkdownParse {
	reparse_with(previous, source, Dialect::default())
}

pub fn reparse_with(
	previous: &MarkdownParse,
	source: Arc<str>,
	dialect: Dialect,
) -> MarkdownParse {
	parse_incremental(previous, source.clone(), dialect)
		.unwrap_or_else(|| super::parse_with(source, dialect))
}

/// The blocks at the start of `source` that `bytes` reaches, as a document over
/// the whole source, or `None` when `bytes` already covers it.
pub fn parse_prefix(source: &Arc<str>, bytes: usize) -> Option<MarkdownParse> {
	parse_prefix_with(source, bytes, Dialect::default())
}

pub fn parse_prefix_with(
	source: &Arc<str>,
	bytes: usize,
	dialect: Dialect,
) -> Option<MarkdownParse> {
	if comments_need_full_parse(source, dialect) {
		return None;
	}
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
	let end = line_end(source, end);
	if unread_footnotes_affect_details(source, end) {
		return None;
	}
	if let Some(close) = front_matter_close(source)
		&& end < close
	{
		return None;
	}
	if ends_in_open_html(&source[..end]) {
		return None;
	}
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
	let parsed = prefix_blocks(source, end, dialect);
	let kept: Vec<&Block> = parsed
		.document
		.blocks()
		.iter()
		.filter(|block| {
			block_range(&parsed.map, block.id())
				.is_some_and(|range| range.start < end)
		})
		.filter(|block| {
			end >= source.len()
				|| !matches!(block.kind(), BlockKind::Footnote { .. })
		})
		.collect();
	// A kept container clones its children, so the map entries of those
	// children must stay too: keeping only the top-level ids dropped them.
	let mut keep: HashSet<BlockId> = HashSet::new();
	for block in &kept {
		visit_blocks(block, &mut |block| {
			keep.insert(block.id());
		});
	}
	let blocks: Vec<Block> = kept.into_iter().cloned().collect();
	let map: Vec<BlockSource> = parsed
		.map
		.blocks()
		.iter()
		.filter(|entry| keep.contains(&entry.block))
		.cloned()
		.collect();
	let revision = DocumentRevision::fresh();
	Some(MarkdownParse {
		document: DocumentBuilder::new(revision).finish(blocks),
		source: source.clone(),
		map: SourceMap::new(revision, map),
		dialect,
	})
}

fn changed_range(previous: &str, source: &str) -> Range<usize> {
	let (old, new) = (previous.as_bytes(), source.as_bytes());
	let mut prefix = common_prefix(old, new);
	while !previous.is_char_boundary(prefix) {
		prefix -= 1;
	}
	let mut suffix = common_suffix(&old[prefix..], &new[prefix..]);
	while !previous.is_char_boundary(old.len() - suffix)
		|| !source.is_char_boundary(new.len() - suffix)
	{
		suffix -= 1;
	}
	prefix..old.len() - suffix
}

fn comments_need_full_parse(source: &str, dialect: Dialect) -> bool {
	dialect.ofm && source.contains("%%")
}

fn unread_footnotes_affect_details(source: &str, end: usize) -> bool {
	end < source.len()
		&& source[end..].contains("[^")
		&& source.as_bytes()[..end]
			.windows(8)
			.any(|tag| tag.eq_ignore_ascii_case(b"<details"))
}

/// The blocks of `source[..end]`, with every reference or note resolved from
/// the definitions that follow the cut.
fn prefix_blocks(source: &str, end: usize, dialect: Dialect) -> MarkdownParse {
	let bare = &source[..end];
	if !bare.contains('[') {
		return super::parse_with(bare.to_owned(), dialect);
	}
	let definitions = missing_definitions(source, bare);
	if definitions.is_empty() {
		return super::parse_with(bare.to_owned(), dialect);
	}
	let mut slice = bare.to_owned();
	slice.push_str("\n\n");
	slice.push_str(&definitions);
	let parsed = super::parse_with(slice, dialect);
	// An open fence or HTML block can swallow the appended text; then only the
	// bare prefix parses to what the full parse would put there.
	if parsed.document.blocks().iter().any(|block| {
		block_range(&parsed.map, block.id())
			.is_some_and(|range| range.start < end && range.end > end)
	}) {
		return super::parse_with(bare.to_owned(), dialect);
	}
	parsed
}

/// The definitions of `source` that `bare` does not already hold.
///
/// A note inside the prefix resolves natively, and appending a second copy of
/// it changes how the parser reads the prefix: comrak keeps a repeated footnote
/// definition where a lone unreferenced one disappears, so the copy would add a
/// block the document does not have.
///
/// A *reference* definition is never dropped this way, because the prefix's copy
/// need not be one: `[x]: /a "unterminated` at the cut reads as a paragraph,
/// while the real `[x]: ...` after it still resolves the whole document.
/// Dropping the later line left the prefix with a literal `[x]` where the full
/// parse links it. The duplicate is harmless by comparison — a definition
/// renders no block, and the parser keeps the first match.
pub(super) fn missing_definitions(source: &str, bare: &str) -> String {
	let all = definitions(source);
	let present = definitions(bare);
	if present.is_empty() {
		return all;
	}
	let present: Vec<&str> =
		present.lines().filter_map(definition_label).collect();
	all.lines()
		.filter(|line| {
			!definition_label(line).is_some_and(|label| {
				label.starts_with("[^") && present.contains(&label)
			})
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
/// A definition may be indented up to three spaces (four make an indented code
/// block), so an indented `[x]: ...` line is taken like a flush one: comrak
/// reads it as a definition, and a prefix that skipped it kept a literal `[x]`
/// where the full parse links it. Lines inside a fence are text either way. A
/// definition whose destination is on the next line, or a footnote body, is not
/// needed: the reference only needs its target and its number.
///
/// A link reference definition cannot interrupt a paragraph, so a `[x]: y` line
/// that continues one is ordinary text. Taking it as a definition would let the
/// appended copy resolve a reference the full parse leaves alone, changing a
/// prefix block or a `<details>` body that the document itself parses as plain
/// text.
///
/// Two definitions the source separated by anything else are emitted apart: a
/// `[x]: ...` line comrak rejects is a paragraph, and a paragraph swallows the
/// definition line after it, so gluing the two together dropped a definition
/// the full parse resolves. Adjacent lines stay adjacent, which is what the
/// source did.
pub(super) fn definitions(source: &str) -> String {
	let mut out = String::new();
	let mut fence = None;
	// Whether a paragraph is open going into this line, which is what decides
	// whether a `[x]: y` line can start a definition here.
	let mut paragraph = false;
	// The line the last emitted definition came from, so a gap in the source
	// becomes a blank line in the rewrite.
	let mut last = None;
	// The parser's lines: a lone carriage return ends one too, so `lines()`
	// would glue a definition onto the text before it and miss the marker.
	let ranges = line_ranges(source);
	for (index, range) in ranges.iter().enumerate() {
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
		if indent <= 3
			&& !paragraph
			&& rest.starts_with('[')
			&& let Some(close) = rest.find("]:")
		{
			let label = &rest[1..close];
			let value = rest[close + 2..].trim();
			if ![label, value].contains(&"") {
				if last.is_some_and(|last| last + 1 != index) {
					out.push('\n');
				}
				out.push('[');
				out.push_str(label);
				out.push_str("]: ");
				// A note needs only its label to take the number a reference
				// expects.
				out.push_str(if label.starts_with('^') { "x" } else { value });
				out.push('\n');
				last = Some(index);
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
	// Opaque inline HTML can carry anchor-discovery state across blocks.
	if !definition_free(source)
		|| source.contains('|')
		|| source.match_indices('<').any(|(at, _)| {
			crate::container::opaque_len(&source[at..]).is_some()
		}) {
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
/// on the next line that is exactly `---` **after a body line**: it scans for
/// the delimiter from the end of the opening line, so a `---` on the line right
/// below the opener is body text, and a document whose only later `---` is that
/// line holds no front matter at all. Without a closer, the opening line is an
/// ordinary thematic break and a prefix may cut through it.
fn front_matter_close(source: &str) -> Option<usize> {
	let lines = line_ranges(source);
	if lines.first().map(|range| &source[range.clone()]) != Some("---") {
		return None;
	}
	lines
		.into_iter()
		.skip(2)
		.find(|range| &source[range.clone()] == "---")
		.map(|range| range.end)
}

/// Whether `source` ends inside a raw HTML construct that is still open.
///
/// Only the last line that can open an HTML block counts: an earlier closed
/// one does not matter, and a later ordinary line is inside the block the
/// opener began. A `<` anywhere else is ordinary text.
fn ends_in_open_html(source: &str) -> bool {
	// A `<details>` inside a code span or fence is literal text, not HTML.
	let masked = mask_code(source);
	if crate::html::has_open_container(&masked) {
		return true;
	}
	for range in line_ranges(&masked).iter().rev() {
		let line = &masked[range.clone()];
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

/// `source` with the regions Markdown reads as literal code replaced by
/// spaces, so a `<details>` inside a code span or fence is not counted as HTML.
fn mask_code(source: &str) -> String {
	let mut masked = String::with_capacity(source.len());
	let mut fence: Option<(u8, usize)> = None;
	for line in source.split_inclusive(['\r', '\n']) {
		let content = line.trim_end_matches(['\r', '\n']);
		let ending = &line[content.len()..];
		let code = match fence {
			Some((marker, length)) => {
				let rest = content.trim_start_matches(' ');
				let indent = content.len() - rest.len();
				let run = rest.bytes().take_while(|b| *b == marker).count();
				if indent <= 3 && run >= length && rest[run..].trim().is_empty()
				{
					fence = None;
				}
				true
			}
			None => match fence_open(content) {
				Some(open) => {
					fence = Some(open);
					true
				}
				None => false,
			},
		};
		if code {
			masked.extend(std::iter::repeat_n(' ', content.len()));
		} else {
			mask_inline_code(content, &mut masked);
		}
		masked.push_str(ending);
	}
	masked
}

/// The marker and length of the code fence a line opens, if it opens one.
fn fence_open(line: &str) -> Option<(u8, usize)> {
	let rest = line.trim_start_matches(' ');
	if line.len() - rest.len() > 3 {
		return None;
	}
	let marker = *rest.as_bytes().first()?;
	if !matches!(marker, b'`' | b'~') {
		return None;
	}
	let length = rest.bytes().take_while(|b| *b == marker).count();
	// A backtick fence's info string may not contain a backtick.
	(length >= 3 && (marker == b'~' || !rest[length..].contains('`')))
		.then_some((marker, length))
}

/// Replaces one line's code spans with spaces. Unclosed backticks stay as the
/// literal text Markdown reads.
fn mask_inline_code(line: &str, masked: &mut String) {
	let bytes = line.as_bytes();
	let mut at = 0;
	while at < bytes.len() {
		if bytes[at] != b'`' {
			let c = line[at..].chars().next().unwrap();
			masked.push(c);
			at += c.len_utf8();
			continue;
		}
		let open = bytes[at..].iter().take_while(|b| **b == b'`').count();
		let mut close = None;
		let mut scan = at + open;
		while scan < bytes.len() {
			let run = bytes[scan..].iter().take_while(|b| **b == b'`').count();
			if run == open {
				close = Some(scan + run);
				break;
			}
			scan += run.max(1);
		}
		match close {
			Some(end) => {
				masked.extend(std::iter::repeat_n(' ', end - at));
				at = end;
			}
			None => {
				masked.push_str(&line[at..at + open]);
				at += open;
			}
		}
	}
}

/// Assigns heading anchors in final reading order.
pub(super) fn relabel_headings(blocks: &mut [Block]) {
	fn walk(blocks: &mut [Block], anchors: &mut Anchors) {
		for block in blocks {
			match block.kind_mut() {
				BlockKind::Heading { text, anchor, .. } => {
					*anchor = anchors.unique(&plain_text(text))
				}
				BlockKind::Quote { blocks, .. }
				| BlockKind::Footnote { blocks, .. }
				| BlockKind::Details { blocks, .. }
				| BlockKind::FrontMatter { blocks, .. } => walk(blocks, anchors),
				BlockKind::List { items, .. } => {
					for item in items {
						walk(&mut item.blocks, anchors);
					}
				}
				_ => {}
			}
		}
	}
	walk(blocks, &mut Anchors::default());
}
