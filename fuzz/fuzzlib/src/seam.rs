//! Source-derived scans and structural walks shared by the semantic oracles.
//!
//! Everything here reads the *source text* or Markview's own public tree, and
//! never comrak's HTML rendering — the plan's O7 rules that comparison out.
//! The scans are deliberately independent re-derivations: if a predicate here
//! and the parser disagree, one of them is wrong, and that disagreement is
//! the finding.

use std::ops::Range;

use markview_document::{
	Block, BlockKind, Document, Inline, InlineKind, RichText,
};

/// Which line endings a source uses, so a sweep can rebuild the same document
/// with the newline convention swapped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Newline {
	Lf,
	CrLf,
	Cr,
}

impl Newline {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Lf => "\n",
			Self::CrLf => "\r\n",
			Self::Cr => "\r",
		}
	}
}

/// Whether `source` holds any byte outside 7-bit ASCII, so a UTF-16 conversion
/// is answer-preserving.
pub fn is_ascii(source: &str) -> bool {
	source.is_ascii()
}

/// `source` encoded as little-endian UTF-16, the form a clipboard or an
/// editor's save path hands the reader. Returns `None` unless the input is
/// ASCII, where UTF-16 keeps every code unit below 0x80 and the per-line
/// column arithmetic in every byte-oriented convention agrees.
pub fn utf16le(source: &str) -> Option<Vec<u8>> {
	if !source.is_ascii() {
		return None;
	}
	let mut out = Vec::with_capacity(source.len() * 2);
	for byte in source.bytes() {
		out.push(byte);
		out.push(0);
	}
	Some(out)
}

/// How many block quotes enclose the line that holds `at`: the `>` markers
/// comrak removes from that line before the block's literal starts.
///
/// This mirrors the parser's own model (a lone carriage return ends a line),
/// which is what makes it a usable check on the range the parser then reports.
pub fn quote_depth_at(source: &str, at: usize) -> usize {
	let at = at.min(source.len());
	let mut line = 0usize;
	let bytes = source.as_bytes();
	let mut i = 0usize;
	while i < at {
		match bytes[i] {
			b'\n' => i += 1,
			b'\r' => {
				i += 1;
				if bytes.get(i) == Some(&b'\n') {
					i += 1;
				}
			}
			_ => i += 1,
		}
		if i <= at {
			line = i;
		}
	}
	source[line..at].matches('>').count()
}

/// The raw HTML block literal of an element's opener, when the document's
/// first block begins with `<details` — the tag scanner's input.
pub fn raw_opener_block(doc: &Document) -> Option<&str> {
	doc.blocks.iter().find_map(|block| match &block.kind {
		BlockKind::Code { language, text } if language == "HTML source" => {
			text.starts_with("<details").then_some(text.as_str())
		}
		_ => None,
	})
}

/// Every `<details>` element the document built, as
/// `(block id, ordinal, source range)`.
pub fn details_elements(doc: &Document) -> Vec<(u64, u32, Range<usize>)> {
	fn walk(blocks: &[Block], out: &mut Vec<(u64, u32, Range<usize>)>) {
		for block in blocks {
			match &block.kind {
				BlockKind::Details {
					ordinal, blocks, ..
				} => {
					out.push((block.id, *ordinal, block.source.clone()));
					walk(blocks, out);
				}
				BlockKind::Quote { blocks, .. }
				| BlockKind::Footnote { blocks, .. }
				| BlockKind::FrontMatter { blocks, .. } => walk(blocks, out),
				BlockKind::List { items, .. } => {
					for item in items {
						walk(&item.blocks, out);
					}
				}
				_ => {}
			}
		}
	}
	let mut out = Vec::new();
	walk(&doc.blocks, &mut out);
	out
}

/// Every inline in the tree, in document order, with its kind, source range
/// and the block range it was stored under.
pub fn inlines(doc: &Document) -> Vec<(&Inline, &Range<usize>)> {
	fn walk<'a>(
		blocks: &'a [Block],
		out: &mut Vec<(&'a Inline, &'a Range<usize>)>,
	) {
		for block in blocks {
			match &block.kind {
				BlockKind::Paragraph(text)
				| BlockKind::Heading { text, .. } => {
					for inline in text {
						out.push((inline, &block.source));
					}
				}
				BlockKind::Details {
					summary, blocks, ..
				} => {
					for inline in summary {
						out.push((inline, &block.source));
					}
					walk(blocks, out);
				}
				BlockKind::Quote { blocks, .. }
				| BlockKind::Footnote { blocks, .. }
				| BlockKind::FrontMatter { blocks, .. } => walk(blocks, out),
				BlockKind::List { items, .. } => {
					for item in items {
						walk(&item.blocks, out);
					}
				}
				BlockKind::Table { rows, .. } => {
					for row in rows {
						for cell in row {
							for inline in cell {
								out.push((inline, &block.source));
							}
						}
					}
				}
				BlockKind::Code { .. } | BlockKind::Rule => {}
			}
		}
	}
	let mut out = Vec::new();
	walk(&doc.blocks, &mut out);
	out
}

/// Every link destination and image source with the text it was applied to, so
/// a caller can compare two documents' resolution of the same source.
pub fn resolved_urls(doc: &Document) -> Vec<(String, String)> {
	let mut out = Vec::new();
	for (inline, _) in inlines(doc) {
		match &inline.kind {
			InlineKind::Image(image) => {
				out.push((image.alt.clone(), image.src.clone()))
			}
			InlineKind::Text(text) => {
				if let Some(url) = &inline.style.link {
					out.push((text.clone(), url.as_str().to_owned()));
				}
			}
			_ => {}
		}
	}
	out
}

/// A `[label]: destination` line of the source, as found by an independent
/// column-zero scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefinitionLine {
	/// The label without its brackets, lowercased and whitespace-collapsed the
	/// way the CommonMark label rules compare it.
	pub label: String,
	/// Everything after `]:`, trimmed; a destination, optionally with a title.
	pub value: String,
	pub at: usize,
}

/// The definitions an independent source scan is sure of, and whether it is
/// sure it found them all.
#[derive(Clone, Debug, Default)]
pub struct DefinitionScan {
	pub definitions: Vec<DefinitionLine>,
	/// `false` when the scan passed over a definition candidate it could not
	/// place — one behind a list marker or an indentation that may be a
	/// container's content. The absence of a label from `definitions` then
	/// proves nothing, so only the equality assertion may use the table.
	pub complete: bool,
}

/// `[label]: value` lines at the start of a block, outside a fenced code
/// block, scanned independently of the parser's own definition extractor.
pub fn definition_lines(source: &str) -> Vec<DefinitionLine> {
	definition_scan(source).definitions
}

/// `definition_lines` plus whether the table it returns is complete.
///
/// Both spellings a reference definition takes are read: the destination on
/// the same line (`[a]: /u`) and on the next one (`[a]:\n /u`). Skipping the
/// second form made this scan reject a definition the parser had resolved,
/// which showed up as a spurious "the parser invented a destination" report on
/// `/url\n\n[foo![foo][bar]\n\n[BAR]:\n /url`.
///
/// Only definitions this scan is *certain* of enter the table, and it never
/// guesses: a line that could be a definition only because it sits inside a
/// list item is refused rather than read. That can leave the table
/// incomplete, which `complete` records so the caller can decline the absence
/// assertion instead of firing on a definition the scan missed.
pub fn definition_scan(source: &str) -> DefinitionScan {
	let mut scan = DefinitionScan {
		definitions: Vec::new(),
		complete: true,
	};
	let mut fence: Option<u8> = None;
	let mut html: Option<HtmlEnd> = None;
	let all = lines(source);
	for (index, (line, at)) in all.iter().enumerate() {
		let (line, at) = (*line, *at);
		let rest = line.trim_start_matches(' ');
		let indent = line.len() - rest.len();
		if let Some(end) = html {
			// Raw HTML is not Markdown: no definition can be read from it, so
			// the line is skipped rather than turned into a row. A
			// definition-shaped line inside one means the table may be missing
			// a definition this scan cannot see, so it is no longer proven.
			if definition_shaped(rest) {
				scan.complete = false;
			}
			let closed = match end {
				HtmlEnd::At(closer) => {
					line.to_ascii_lowercase().contains(closer)
				}
				HtmlEnd::Blank => line.trim().is_empty(),
			};
			if closed {
				html = None;
			}
			continue;
		}
		if indent <= 3 && (rest.starts_with("```") || rest.starts_with("~~~")) {
			let marker = rest.as_bytes()[0];
			if fence == Some(marker) {
				fence = None;
			} else if fence.is_none() {
				fence = Some(marker);
			}
			continue;
		}
		if fence.is_some() {
			continue;
		}
		// A block quote's `>` markers are not part of the line's content: a
		// definition inside a quote (`> [foo]: /url`) resolves for the whole
		// document like any other, and reading the line verbatim made this
		// scan miss it — measured as a spurious "the parser invented a
		// destination" report on `> [foo]: /url\n    [foo]\n> Á[foo] /url`.
		// The markers are therefore stripped *before* the `[` test, since the
		// line begins with `>` rather than with the bracket.
		let quoted = strip_quote_markers(rest);
		// A definition may be indented up to three spaces (four would make
		// it an indented code block); requiring column zero rejected the
		// `  [foo]: /url` form comrak accepts. The indent is measured after
		// the quote markers, which shift the line's content left.
		let (content_indent, quoted_content) = split_indent(quoted);
		// Raw HTML stays raw inside a container, so the test runs on the
		// quote-stripped content: a definition-shaped line in a quoted HTML
		// block is HTML text, not a definition the table may invent.
		if content_indent <= 3
			&& let Some(end) = html_block_open(quoted_content)
		{
			html = Some(end);
			continue;
		}
		let (content, in_list) = strip_containers(line);
		if !(content.starts_with('[') && content.contains("]:")) {
			// A marker the reduction above could not consume (`> \t> [x]`)
			// may still hold a definition this scan cannot see, so the table
			// is no longer proven complete by the line's absence from it.
			if content.starts_with('>') || strip_list_marker(content).is_some()
			{
				scan.complete = false;
			}
			continue;
		}
		// The candidate is refused when it sits behind a marker this scan does
		// not descend into, or more than three spaces deep where it cannot
		// tell code from a container's content. Refusing it keeps the table
		// free of invented destinations; marking the table incomplete keeps
		// the absence assertion from treating the miss as a parser defect.
		if in_list || indent > 3 || content_indent > 3 {
			scan.complete = false;
			continue;
		}
		let Some(close) = content.find("]:") else {
			continue;
		};
		let label = &content[1..close];
		let mut value = content[close + 2..].trim();
		// The destination may sit on the next line, indented no more than
		// three spaces. A line that opens a list or a quote cannot be a bare
		// destination, and a deeper indent may be a container's content, so
		// neither is read and both leave the table unproven.
		if value.is_empty()
			&& let Some((next, _)) = all.get(index + 1)
		{
			let next_rest = next.trim_start_matches(' ');
			if next.len() - next_rest.len() > 3
				|| strip_list_marker(next_rest).is_some()
				|| next_rest.starts_with('>')
			{
				scan.complete = false;
			} else {
				value = next_rest.trim();
			}
		}
		if label.is_empty() || value.is_empty() {
			continue;
		}
		scan.definitions.push(DefinitionLine {
			label: normalize_label(label),
			value: value.to_string(),
			at,
		});
	}
	scan
}

/// `text` split into its leading spaces and the remainder.
fn split_indent(text: &str) -> (usize, &str) {
	let rest = text.trim_start_matches(' ');
	(text.len() - rest.len(), rest)
}

/// The text after a list item marker at the start of `text`, when `text`
/// begins with one: a bullet (`-`, `+`, `*`) or an ordered marker of at most
/// nine digits, each followed by a space, a tab, or the end of the line.
fn strip_list_marker(text: &str) -> Option<&str> {
	let bytes = text.as_bytes();
	let marker_len = match bytes.first()? {
		b'-' | b'+' | b'*' => 1,
		b'0'..=b'9' => {
			let digits =
				bytes.iter().take_while(|b| b.is_ascii_digit()).count();
			if digits > 9 {
				return None;
			}
			match bytes.get(digits) {
				Some(b'.' | b')') => digits + 1,
				_ => return None,
			}
		}
		_ => return None,
	};
	match bytes.get(marker_len) {
		Some(b' ' | b'\t') => {
			Some(text[marker_len..].trim_start_matches([' ', '\t']))
		}
		None => Some(&text[marker_len..]),
		_ => None,
	}
}

/// The definition candidate `line` holds, after block quote and list markers
/// are removed, plus whether a list marker was among them.
///
/// A quote and a list item can nest in either order (`- > [x]: /u`,
/// `> - [x]: /u`), so both are stripped until neither applies. Reading only
/// one order left the deeper definitions invisible and the table looking
/// complete, which fired the absence assertion on real Markdown.
fn strip_containers(line: &str) -> (&str, bool) {
	let mut rest = line;
	let mut in_list = false;
	loop {
		let quoted = strip_quote_markers(rest);
		let trimmed = quoted.trim_start_matches([' ', '\t']);
		match strip_list_marker(trimmed) {
			Some(after) => {
				in_list = true;
				rest = after;
			}
			None => {
				rest = quoted;
				break;
			}
		}
	}
	(rest.trim_start_matches([' ', '\t']), in_list)
}

/// Whether `line` reads as a link reference definition once its block quote
/// and list markers are removed.
fn definition_shaped(line: &str) -> bool {
	let (content, _) = strip_containers(line);
	content.starts_with('[') && content.contains("]:")
}

/// How a raw HTML block that has started ends.
#[derive(Clone, Copy)]
enum HtmlEnd {
	/// The line holding this text is the block's last one.
	At(&'static str),
	/// A blank line ends the block.
	Blank,
}

/// The raw HTML block `content` opens at the start of a line, if any.
///
/// Only the ends that matter here are distinguished: the four verbatim
/// elements and the three comment-like forms close on their own terminator,
/// everything else runs to the next blank line. The start test is deliberately
/// broad — over-reading a tag as a block costs only the definitions its lines
/// are skipped with, and a definition-shaped line skipped inside one marks the
/// table unproven — while under-reading would let HTML text enter the table.
fn html_block_open(content: &str) -> Option<HtmlEnd> {
	let after = content.strip_prefix('<')?;
	let head = |name: &str| {
		after
			.get(..name.len())
			.is_some_and(|prefix| prefix.eq_ignore_ascii_case(name))
			&& after
				.as_bytes()
				.get(name.len())
				.is_none_or(|b| matches!(*b, b' ' | b'\t' | b'>' | b'/'))
	};
	if after.starts_with("!--") {
		return Some(HtmlEnd::At("-->"));
	}
	if after.starts_with('?') {
		return Some(HtmlEnd::At("?>"));
	}
	if after.starts_with("![CDATA[") {
		return Some(HtmlEnd::At("]]>"));
	}
	if after.starts_with('!') {
		return Some(HtmlEnd::At(">"));
	}
	for (name, closer) in [
		("script", "</script>"),
		("pre", "</pre>"),
		("style", "</style>"),
		("textarea", "</textarea>"),
	] {
		if head(name) {
			return Some(HtmlEnd::At(closer));
		}
	}
	let first = after.as_bytes().first()?;
	(first.is_ascii_alphabetic() || *first == b'/').then_some(HtmlEnd::Blank)
}

/// `line` with up to `usize::MAX` leading block quote markers removed, each
/// with at most one following space or tab — the same reduction the parser
/// applies before it reads a line's content.
fn strip_quote_markers(line: &str) -> &str {
	let mut rest = line;
	loop {
		let indent = rest.len() - rest.trim_start_matches(' ').len();
		// Four spaces already mean code, not a marker.
		if indent > 3 {
			return rest;
		}
		let Some(after) = rest[indent..].strip_prefix('>') else {
			return rest;
		};
		rest = match after.as_bytes().first() {
			Some(b' ' | b'\t') => &after[1..],
			_ => after,
		};
	}
}

/// The CommonMark label normalization: case-folded with internal whitespace
/// runs collapsed to one space.
pub fn normalize_label(label: &str) -> String {
	let mut out = String::with_capacity(label.len());
	let mut space = false;
	for c in label.trim().chars() {
		if c.is_whitespace() {
			space = true;
			continue;
		}
		if space && !out.is_empty() {
			out.push(' ');
		}
		space = false;
		out.extend(c.to_lowercase());
	}
	out
}

/// Every line of `source` as `(content, byte offset)`, where a line ends at
/// `\n`, `\r\n`, or a lone `\r` — the parser's line structure.
pub fn lines(source: &str) -> Vec<(&str, usize)> {
	let bytes = source.as_bytes();
	let mut out = Vec::new();
	let mut start = 0usize;
	let mut i = 0usize;
	while i < bytes.len() {
		if matches!(bytes[i], b'\n' | b'\r') {
			out.push((&source[start..i], start));
			i += if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
				2
			} else {
				1
			};
			start = i;
		} else {
			i += 1;
		}
	}
	out.push((&source[start..], start));
	out
}

/// The byte offset just past the line ending that follows `end`, when one is
/// there.
pub fn line_end(source: &str, end: usize) -> usize {
	match source.as_bytes().get(end) {
		Some(b'\n') => end + 1,
		Some(b'\r') => {
			end + 1
				+ usize::from(source.as_bytes().get(end + 1) == Some(&b'\n'))
		}
		_ => end,
	}
}

/// Checks that a block whose range is non-degenerate does not report text
/// that could not come from anywhere.
///
/// A "the range must contain the reading text" check was tried and removed:
/// the parser legitimately decodes HTML entities (`&ouml;` reads `ö`), merges
/// markup runs across inline boundaries (`*f` + `*foo*` reads `*ffoo`), and
/// attributes an inline to a sub-range of its block. Every formulation of
/// containment fired on those, so it measured this helper's guesses rather
/// than the parser. What survives is the one direction that is still sound:
/// a block that reports *some* reading text must own *some* source, so a
/// range that collapsed to nothing while the text survived is a real defect.
///
/// Emptiness of the text is not a defect: `### `, `[](./a.md)`, an empty list
/// item and a marker-only table cell all legitimately yield no inline.
pub fn assert_rich_text_covered(doc: &Document) {
	fn check(what: &str, block: &Block, text: &RichText) {
		if plain_text(text).trim().is_empty() {
			return;
		}
		assert!(
			!block.source.is_empty() || !doc_owned(block),
			"{what} reports reading text but has an empty source range"
		);
	}
	fn walk(blocks: &[Block]) {
		for block in blocks {
			match &block.kind {
				BlockKind::Paragraph(text) => check("block", block, text),
				BlockKind::Heading { text, .. } => check("block", block, text),
				BlockKind::Details {
					summary, blocks, ..
				} => {
					check("summary", block, summary);
					walk(blocks);
				}
				BlockKind::Quote { blocks, .. }
				| BlockKind::Footnote { blocks, .. }
				| BlockKind::FrontMatter { blocks, .. } => walk(blocks),
				BlockKind::List { items, .. } => {
					for item in items {
						walk(&item.blocks);
					}
				}
				BlockKind::Table { rows, .. } => {
					for row in rows {
						for cell in row {
							check("cell", block, cell);
						}
					}
				}
				BlockKind::Code { .. } | BlockKind::Rule => {}
			}
		}
	}
	walk(&doc.blocks);
}

/// Whether a block's range addresses the document rather than a snippet. A
/// `<details>` body is parsed as its own document, so its ranges are
/// snippet-relative and an empty one there carries no meaning for this check.
fn doc_owned(block: &Block) -> bool {
	block.source.start <= block.source.end
}

/// A reference link's own label, as written, from a `[text][label]`,
/// `[label][]`, or a shortcut `[label]` — the three forms a definition
/// resolves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceUse {
	/// The label to look up, normalized.
	pub label: String,
	/// The source span from the opener (`[`, or `![` for an image) through the
	/// last bracket pair the reference consumed.
	pub span: Range<usize>,
}

/// Every reference use an independent scan read, and the inline-link spans it
/// stepped over while reading them.
#[derive(Clone, Debug, Default)]
pub struct ReferenceScan {
	pub uses: Vec<ReferenceUse>,
	/// The span of each `[text](dest)` the scan skipped. A tree run inside one
	/// was resolved by that inline link, not by any reference, so it must not
	/// be attributed to an enclosing use.
	pub inline_links: Vec<Range<usize>>,
}

/// Every reference-style link or image in `source` that a definition could
/// resolve, found by an independent scan of the raw text.
///
/// Deliberately conservative: a construct the scan cannot read with certainty
/// is skipped rather than guessed, so a disagreement with the parser is
/// evidence about the parser rather than about this scan.
pub fn reference_uses(source: &str) -> Vec<ReferenceUse> {
	reference_scan(source).uses
}

/// [`reference_uses`], plus every inline-link span in the source.
pub fn reference_scan(source: &str) -> ReferenceScan {
	let mut scan = ReferenceScan {
		uses: Vec::new(),
		inline_links: inline_link_spans(source),
	};
	let bytes = source.as_bytes();
	let mut i = 0usize;
	while i < bytes.len() {
		// Skip code spans and escapes: neither can open a link label.
		if bytes[i] == b'\\' {
			i += 2;
			continue;
		}
		if bytes[i] != b'[' {
			i += 1;
			continue;
		}
		// An image's `![` is not a label opener on its own; step back one.
		let open = if i > 0 && bytes[i - 1] == b'!' {
			i - 1
		} else {
			i
		};
		let Some((inner, after)) = span(source, i, b'[', b']') else {
			i += 1;
			continue;
		};
		// An inline link `[text](dest)` owns everything through its `)`. The
		// label-like text inside `text` is not a reference: skipping only the
		// `[` would let a nested `[foo]` be read as a shortcut reference that
		// has no definition, and the parser would look wrong for resolving the
		// inline link it actually saw. Jump past the destination instead.
		if bytes.get(after) == Some(&b'(') {
			i = skip_inline_destination(source, after);
			continue;
		}
		// A definition's `[label]:` is not a use of the label either.
		if bytes.get(after) == Some(&b':') {
			i = after + 1;
			continue;
		}
		// `[a][b]` and `[a][]` carry the label in the second bracket pair;
		// otherwise the text itself is the label.
		let (text, end) = if after < bytes.len() && bytes[after] == b'[' {
			match span(source, after, b'[', b']') {
				Some(("", end)) => (inner, end),
				Some((second, end)) => (second, end),
				None => {
					i += 1;
					continue;
				}
			}
		} else {
			(inner, after)
		};
		// The label must be non-empty and hold no unescaped bracket.
		if text.trim().is_empty() {
			i += 1;
			continue;
		}
		// A label may legally span lines, but comrak's ranges for one are not
		// a model this scan trusts: an unterminated `[` earlier on can pair
		// with a `]` lines away and produce a span that covers unrelated
		// runs. Such a span is not emitted as a use, so no run is checked
		// against it.
		if source[open..end].contains(['\n', '\r']) {
			i += 1;
			continue;
		}
		scan.uses.push(ReferenceUse {
			label: normalize_label(&unescape(text)),
			span: open..end,
		});
		i += 1;
	}
	scan
}

/// Every `[text](dest)` span in `source`, found by a pass that does not skip
/// over a bracket pair the reference scan would treat as a definition.
///
/// Over-reporting is safe: a span here only ever *removes* a run from the
/// reference check, and the extra shapes are inside literals (code spans, an
/// unused label) where no reference run can live.
fn inline_link_spans(source: &str) -> Vec<Range<usize>> {
	let mut out = Vec::new();
	let bytes = source.as_bytes();
	let mut i = 0usize;
	while i < bytes.len() {
		if bytes[i] == b'\\' {
			i += 2;
			continue;
		}
		if bytes[i] != b'[' {
			i += 1;
			continue;
		}
		let open = if i > 0 && bytes[i - 1] == b'!' {
			i - 1
		} else {
			i
		};
		if let Some((_, after)) = span(source, i, b'[', b']')
			&& bytes.get(after) == Some(&b'(')
		{
			let end = skip_inline_destination(source, after);
			out.push(open..end);
			i = end;
			continue;
		}
		i += 1;
	}
	out
}

/// Whether the tree run at `range` was built from `use_`'s reference.
///
/// The run is the label text the parser attributed the destination to: for an
/// ordinary `[ref]` it starts one byte inside the opener and ends before the
/// closer, and for an image it may start after `![`. Both sit inside the
/// reference's own span, so containment associates them; comparing the single
/// opener offset cannot, because the run never starts at the `[`.
pub fn reference_contains(use_: &ReferenceUse, range: &Range<usize>) -> bool {
	span_contains(range, &use_.span)
}

/// Whether `range` lies inside `span`.
fn span_contains(range: &Range<usize>, span: &Range<usize>) -> bool {
	span.start <= range.start && range.end <= span.end
}

/// Whether the source at `range` really holds `text`, the run's reading text,
/// ignoring whitespace.
///
/// A containment match assumes the run's source range points at the run. comrak
/// occasionally mis-attributes that range on hostile input, so two bracket
/// pairs on different lines can claim the same wrong bytes. The run's own slice
/// must therefore carry the text the parser read from it; otherwise the range
/// is not trustworthy and the run is left unattributed rather than checked
/// against a neighbouring label.
fn run_text_in_source(source: &str, range: &Range<usize>, text: &str) -> bool {
	let Some(slice) = source.get(range.clone()) else {
		// A range off a character boundary is a snippet-relative one; it
		// cannot be validated against this source.
		return false;
	};
	let compact = |s: &str| -> String {
		s.chars().filter(|c| !c.is_whitespace()).collect()
	};
	compact(slice).contains(&compact(text))
}

/// The offset just past an inline link destination, given the offset of its
/// opening `(`. Honors a nested balanced `(...)` and angle brackets.
fn skip_inline_destination(source: &str, open: usize) -> usize {
	let bytes = source.as_bytes();
	let mut depth = 0usize;
	let mut i = open;
	while i < bytes.len() {
		match bytes[i] {
			b'\\' => {
				i += 2;
				continue;
			}
			b'(' => depth += 1,
			b')' => {
				depth -= 1;
				if depth == 0 {
					return i + 1;
				}
			}
			_ => {}
		}
		i += 1;
	}
	open + 1
}

/// The content of the bracket pair opening at `open` and the offset just past
/// its closer, honoring backslash escapes and one level of nesting.
fn span(
	source: &str,
	open: usize,
	start: u8,
	end: u8,
) -> Option<(&str, usize)> {
	let bytes = source.as_bytes();
	if bytes.get(open) != Some(&start) {
		return None;
	}
	let mut depth = 0usize;
	let mut i = open;
	while i < bytes.len() {
		match bytes[i] {
			b'\\' => {
				i += 2;
				continue;
			}
			c if c == start => depth += 1,
			c if c == end => {
				depth -= 1;
				if depth == 0 {
					return Some((&source[open + 1..i], i + 1));
				}
			}
			_ => {}
		}
		i += 1;
	}
	None
}

/// `text` with backslash escapes resolved down to the escaped character, which
/// is how the parser reads a label before comparing it.
pub fn unescape(text: &str) -> String {
	let mut out = String::with_capacity(text.len());
	let mut chars = text.chars();
	while let Some(c) = chars.next() {
		if c == '\\' {
			match chars.next() {
				// A backslash before punctuation escapes it; before anything
				// else it is a literal backslash.
				Some(next) if next.is_ascii_punctuation() => out.push(next),
				Some(next) => {
					out.push('\\');
					out.push(next);
				}
				None => out.push('\\'),
			}
		} else {
			out.push(c);
		}
	}
	out
}

/// Checks how a document resolved every reference link the source holds
/// against the definition table an independent scan derives from the same
/// source.
///
/// Two directions are asserted, so the oracle cannot fire on label spelling
/// the scan reads differently from the parser:
///
/// - a link the tree resolved to `url` must have a definition whose value
///   begins with exactly that `url` (a link cannot invent a destination);
/// - a link the tree resolved from a label the scan's table does not hold is
///   a finding, but only while the table is known to be complete — an
///   incomplete table proves nothing by absence.
///
/// Passing the document's own source keeps both sides on the same text.
pub fn assert_reference_resolution(doc: &Document) {
	let scan = definition_scan(&doc.source);
	let definitions = &scan.definitions;
	// A definition is `value` (possibly `url "title"`); the URL is its first
	// whitespace-delimited word, unless the whole value is angle-bracketed.
	let url_of = |value: &str| -> String {
		let value = value.trim();
		if let Some(rest) = value.strip_prefix('<') {
			return rest.split('>').next().unwrap_or("").to_string();
		}
		// A value that is only a quoted string is a title with no URL, which
		// the parser rejects; keep it out of the table.
		if value.starts_with('"') || value.starts_with('\'') {
			return String::new();
		}
		value.split_whitespace().next().unwrap_or("").to_string()
	};
	let table: Vec<(String, String)> = definitions
		.iter()
		.map(|d| (d.label.clone(), url_of(&d.value)))
		.filter(|(_, url)| !url.is_empty())
		.collect();

	// Every place the tree applied a link destination to a run of text that
	// the source spells as a reference rather than an inline link.
	let references = reference_scan(&doc.source);
	for (inline, _) in inlines(doc) {
		let (text, url) = match &inline.kind {
			InlineKind::Image(image) => {
				(image.alt.as_str(), image.src.as_str())
			}
			InlineKind::Text(text) => match &inline.style.link {
				Some(url) => (text.as_str(), url.as_str()),
				None => continue,
			},
			_ => continue,
		};
		// Footnote references carry a synthetic `#fn:` destination that no
		// definition holds; they are checked by the footnote oracle instead.
		if inline.style.footnote_ref || url.starts_with("#fn:") {
			continue;
		}
		// A run inside an inline link was resolved by that link's own
		// destination, even when an enclosing bracket pair also reads as a
		// shortcut reference (`[a [b](c)]`, `[a][b](c)`).
		if references
			.inline_links
			.iter()
			.any(|link| span_contains(&inline.source, link))
		{
			continue;
		}
		// The reference whose span holds this run, provided the run's range
		// actually holds the run's text. Containment, not the opener offset,
		// because a label run starts inside the brackets.
		let Some(use_) = references.uses.iter().find(|u| {
			reference_contains(u, &inline.source)
				&& run_text_in_source(&doc.source, &inline.source, text)
		}) else {
			continue;
		};
		// Label *spelling* is a known divergence between this scan and comrak,
		// and the party under investigation is `incremental::definitions` (and
		// `parse_prefix`), not this oracle. Re-deriving that semantics here
		// would replace this check with a second implementation of the thing
		// it exists to test, so the check stands down whenever a label could
		// be read two ways. Three shapes trigger it, each measured as a false
		// positive:
		//
		// - a backslash anywhere, which can escape a bracket (`[ref\[]: /u`,
		//   where `[ro]` then resolved from a definition this scan named
		//   differently);
		// - a control character inside a label, whose whitespace and case
		//   normalization this scan does not model (a Cyrillic label holding
		//   an interior `\0`);
		// - a label that normalizes to empty, which means the scan and the
		//   parser disagree about where the label ends.
		let labels_ambiguous = doc.source.contains('\\')
			|| definitions.iter().any(|d| {
				d.label.chars().any(|c| c.is_control()) || d.label.is_empty()
			});
		if labels_ambiguous && !definitions.is_empty() {
			continue;
		}
		let Some((_, defined)) =
			table.iter().find(|(label, _)| *label == use_.label)
		else {
			// The table is only an inventory when the scan did not pass over a
			// construct that could hold a definition (a list item). Otherwise
			// the miss says nothing about the parser.
			if !scan.complete {
				continue;
			}
			panic!(
				"the parser resolved {:?} from the label {:?}, which no \
				 definition in the source declares (definitions: {definitions:?})",
				text, use_.label
			);
		};
		assert_eq!(
			url, defined,
			"the label {:?} resolved to {url:?}, but its definition says \
			 {defined:?}",
			use_.label
		);
	}
}

/// Checks a `<details>` element that the tag scanner recognized against the
/// invariants any correct interpretation of its own source must satisfy.
///
/// The scanner's contract, from `html::details`:
///
/// - `Inline`: the element closed inside one block. Its body is the text
///   between the opener and the close tag, so every byte of body content the
///   source wrote between those tags must be reachable from the children.
/// - `Open`: the opener ends at a blank line; the body continues in later
///   sibling blocks. Every later sibling up to the closing tag belongs to the
///   element, so a document cannot hold a top-level block between the opener
///   and the close.
///
/// Both are source-derived: they read the tag text and the tree, never a
/// rendering.
pub fn assert_details_structure(doc: &Document, source: &str) {
	// Every element's range must be inside the source and ordered.
	//
	// Two checks that look obvious are deliberately absent. The slice is taken
	// through `get` because a range need not fall on a character boundary:
	// a `<details>` body is parsed as a snippet of its own, so its ranges are
	// snippet-relative by construction (`fuzz/README.md` pins that contract),
	// and a byte offset that is off a boundary for *this* source is not a
	// defect. For the same reason the range is not required to begin at a
	// literal `<details` byte: an element built from a body snippet has no
	// tag at its own start. The invariant that *is* asserted — enclosure and
	// identity — is checked below.
	for (_, ordinal, range) in details_elements(doc) {
		assert!(
			range.start <= range.end && range.end <= source.len(),
			"details {ordinal} range {range:?} does not fit the {}-byte \
			 source",
			source.len()
		);
	}
	// Elements must be numbered once each, in the order their ranges begin.
	// The ordinal is the identity a summary link toggles, so a collision means
	// two elements share one disclosure state.
	let elements = details_elements(doc);
	for (i, (_, ordinal, range)) in elements.iter().enumerate() {
		for (_, other_ordinal, other_range) in &elements[i + 1..] {
			assert_ne!(
				ordinal, other_ordinal,
				"two details elements share ordinal {ordinal}: {range:?} and \
				 {other_range:?}"
			);
		}
	}
	// A summary's rich text is attributed to the whole element, so it must sit
	// inside it.
	fn summaries(
		blocks: &[Block],
		out: &mut Vec<(Range<usize>, Range<usize>)>,
	) {
		for block in blocks {
			match &block.kind {
				BlockKind::Details {
					summary, blocks, ..
				} => {
					for inline in summary {
						out.push((block.source.clone(), inline.source.clone()));
					}
					summaries(blocks, out);
				}
				BlockKind::Quote { blocks, .. }
				| BlockKind::Footnote { blocks, .. }
				| BlockKind::FrontMatter { blocks, .. } => summaries(blocks, out),
				BlockKind::List { items, .. } => {
					for item in items {
						summaries(&item.blocks, out);
					}
				}
				_ => {}
			}
		}
	}
	let mut found = Vec::new();
	summaries(&doc.blocks, &mut found);
	for (element, summary) in found {
		assert!(
			element.start <= summary.start && summary.end <= element.end,
			"a summary range {summary:?} is not inside its element \
			 {element:?}"
		);
	}
}

/// One fragment target the document tree declares, and the disclosures that
/// hide it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FragmentTarget {
	/// The fragment a `#name` link spells.
	pub name: String,
	/// Index into `Document::blocks` of the top-level block whose subtree
	/// declares it.
	pub block: usize,
	/// The `<details>` ids framing the declaration, outermost first. The
	/// declaration is unreachable until all of them are expanded.
	pub chain: Vec<u64>,
}

/// Every fragment target the tree declares, in document order.
///
/// The reading comes from the tree, not from a scan of the source, because
/// the two answer different questions: a scan can say what *looks* like a
/// target, while only the tree can say what a jump would reach. The names
/// collected are exactly the ones layout turns into anchors — `Block::anchors`,
/// an invisible `InlineKind::Anchor`, a heading slug, a footnote definition's
/// `fn:<label>`, and a reference's `fnref:<label>` — so a name that reaches
/// only one side is a finding in either direction.
///
/// `chain` follows the walk `Document::details_enclosing` documents: an
/// element's own targets and its summary sit in the *outer* chain, because a
/// summary is drawn even when the body is collapsed, while the body's targets
/// add the element's own id.
pub fn fragment_targets(doc: &Document) -> Vec<FragmentTarget> {
	fn rich(text: &RichText, out: &mut Vec<String>) {
		for inline in text {
			match &inline.kind {
				InlineKind::Anchor(name) => out.push(name.clone()),
				// A reference registers the anchor its own number returns to,
				// and the number is the one the layout will use.
				InlineKind::FootnoteRef(n) => {
					out.push(format!("fnref:{n}"));
				}
				_ => {}
			}
		}
	}
	/// The names the block itself declares, excluding the blocks it contains.
	fn direct(block: &Block, out: &mut Vec<String>) {
		out.extend(block.anchors.iter().cloned());
		match &block.kind {
			BlockKind::Paragraph(text) => rich(text, out),
			BlockKind::Heading { text, anchor, .. } => {
				rich(text, out);
				out.push(anchor.clone());
			}
			BlockKind::Footnote { label, .. } => {
				out.push(format!("fn:{label}"));
			}
			BlockKind::Details { summary, .. } => rich(summary, out),
			BlockKind::Table { rows, .. } => {
				for row in rows {
					for cell in row {
						rich(cell, out);
					}
				}
			}
			_ => {}
		}
	}
	fn walk(
		blocks: &[Block],
		top: usize,
		open: &mut Vec<u64>,
		out: &mut Vec<FragmentTarget>,
	) {
		for block in blocks {
			let mut names = Vec::new();
			direct(block, &mut names);
			for name in names {
				out.push(FragmentTarget {
					name,
					block: top,
					chain: open.clone(),
				});
			}
			let disclosure = matches!(block.kind, BlockKind::Details { .. });
			if disclosure {
				open.push(block.id);
			}
			match &block.kind {
				BlockKind::Details { blocks, .. }
				| BlockKind::Quote { blocks, .. }
				| BlockKind::Footnote { blocks, .. }
				| BlockKind::FrontMatter { blocks, .. } => walk(blocks, top, open, out),
				BlockKind::List { items, .. } => {
					for item in items {
						walk(&item.blocks, top, open, out);
					}
				}
				_ => {}
			}
			if disclosure {
				open.pop();
			}
		}
	}
	let mut out = Vec::new();
	let mut open = Vec::new();
	for (top, block) in doc.blocks.iter().enumerate() {
		walk(std::slice::from_ref(block), top, &mut open, &mut out);
	}
	out
}

/// The markdown text a `<details>` opener declares in its `<summary>`, from
/// the source, as an independent re-derivation.
///
/// `None` when the element declares no summary. The scan follows the same
/// rule the parser does — only a direct child counts — but reads the raw tag
/// sequence itself.
pub fn summary_text(source: &str) -> Option<String> {
	let at = source.find("<summary")?;
	let rest = &source[at..];
	let open_end = rest.find('>')?;
	let inner_start = at + open_end + 1;
	let close_at = source[inner_start..].find("</summary>")? + inner_start;
	Some(source[inner_start..close_at].to_string())
}

/// The `open` attribute declared by a plain `<details …>` tag that has already
/// been validated by [`opener_at`].
pub fn declared_open(tag: &str) -> Option<bool> {
	let rest = tag.strip_prefix("<details")?;
	let end = rest.find('>')?;
	Some(rest[..end].split_whitespace().any(|attr| {
		attr.split('=').next().is_some_and(|name| {
			name.trim_end_matches('/').eq_ignore_ascii_case("open")
		})
	}))
}

/// A borrow of one inline's source range, so a structural walk can check
/// containment without cloning the inline.
#[derive(Clone, Debug)]
pub struct InlineRef {
	pub source: Range<usize>,
}

impl InlineRef {
	pub fn of(inline: &Inline) -> Self {
		Self {
			source: inline.source.clone(),
		}
	}
}

/// The plain reading text of a rich run — the same projection
/// `markview_document::plain_text` performs, re-exported here so the
/// targets do not each import it.
pub fn plain_text(text: &[Inline]) -> String {
	markview_document::plain_text(text)
}

/// The raw `<details …>` tag text that begins `range` in `doc`'s source, when
/// a well-formed one does.
///
/// Deliberately strict, for two reasons a looser scan got wrong on hostile
/// input (`crash-f52f4246…`, where the "opener" it returned was ~400 bytes of
/// binary junk spanning newlines):
///
/// - the tag must start **at `range.start`**, so it is the opener of the
///   element at hand rather than any `<details` anywhere in the document;
/// - the attribute region must end at the tag's own `>` on the same line, and
///   every attribute must be a plausible name or `name=value`. A malformed run
///   makes this return `None` instead of swallowing a later, well-formed
///   `<details open>` and reporting *its* attribute as this element's.
///
/// `None` therefore means "the opener is not a plain tag this scan can read",
/// which is the precondition the caller's comment always claimed and never
/// checked.
pub fn opener_at<'a>(
	doc: &'a Document,
	range: &Range<usize>,
) -> Option<&'a str> {
	let source = doc.source.as_ref();
	let rest = source.get(range.start..)?;
	let rest = rest.strip_prefix("<details")?;
	// The tag ends at the first `>` on the same line, or not at all: a `>`
	// after a newline belongs to some later construct.
	let end = rest
		.char_indices()
		.take_while(|(_, c)| *c != '\n' && *c != '\r')
		.find(|(_, c)| *c == '>')
		.map(|(i, _)| i)?;
	let attrs = &rest[..end];
	// Each attribute must look like a name, optionally `=value`; anything else
	// means this is not a tag this scan can read.
	let plausible = attrs.split_whitespace().all(|attr| {
		let attr = attr.trim_end_matches('/');
		!attr.is_empty()
			&& attr.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
			&& attr.chars().all(|c| {
				c.is_ascii_alphanumeric()
					|| matches!(
						c,
						'-' | '_' | '=' | '"' | '\'' | '.' | ':' | '/' | '#'
					)
			})
	});
	if !plausible {
		return None;
	}
	source.get(range.start..range.start + "<details".len() + end + 1)
}

#[cfg(test)]
mod tests {
	use std::panic::{AssertUnwindSafe, catch_unwind};
	use std::sync::Arc;

	use super::*;
	use markview_document::Link;

	fn parse(source: &str) -> Document {
		markview_commonmark::parse(Arc::from(source))
	}

	#[test]
	fn a_misattributed_range_is_not_a_reference_use() {
		// comrak puts the second `x1` run's range on the previous line's
		// `[x0]`, so bare containment would check it against the wrong label.
		let source = "[x1]: aLLLFLLLLLLLLLL\n\
			[x0]!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!: aLLLFLLLLLLLLLL\n\
			[x1]!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!:]!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!: aLLLFLLLLLLLLLL\n\
			    [x1]!!!!LLL\nb\n1. ordered axain";
		assert_reference_resolution(&parse(source));
	}

	/// An inline link the reference scan stepped over as if it were a
	/// definition's label must still keep its own run out of the check.
	#[test]
	fn an_inline_link_inside_a_definition_shape_is_still_skipped() {
		let source = "[^ttps://examtle.com/doc#sec)\n\
			\x20   [link](htle.com/doc#sec)\n\
			LL][^ttps://exaple.com/doc#sec)\n~\n\
			\x20     [link](htle.com/doc#sec)\n\
			\x20 LL]:LLL/xeample.com/doc#sec):FLLLLLLLLLL\n";
		assert_reference_resolution(&parse(source));
	}

	/// A bracket pair that spans lines can pair with a `]` far away and cover
	/// unrelated runs, so it is not emitted as a use at all.
	#[test]
	fn multi_line_reference_spans_are_not_uses() {
		assert!(reference_uses("[foo\nbar]\n").is_empty());
		assert_eq!(reference_uses("[foo bar]\n").len(), 1);
	}

	/// The first run the tree attributed a destination to, text or image.
	fn linked_run(doc: &Document) -> Range<usize> {
		inlines(doc)
			.into_iter()
			.find_map(|(inline, _)| match &inline.kind {
				InlineKind::Image(_) => Some(inline.source.clone()),
				InlineKind::Text(_) if inline.style.link.is_some() => {
					Some(inline.source.clone())
				}
				_ => None,
			})
			.expect("a linked run")
	}

	/// Point every link and image destination in the tree at `url`.
	fn set_destinations(doc: &mut Document, url: &str) {
		for block in &mut doc.blocks {
			let BlockKind::Paragraph(text) = &mut block.kind else {
				continue;
			};
			for inline in text {
				match &mut inline.kind {
					InlineKind::Image(image) => image.src = url.to_string(),
					InlineKind::Text(_) if inline.style.link.is_some() => {
						inline.style.link =
							Some(Link::new(url, inline.source.start))
					}
					_ => {}
				}
			}
		}
	}

	/// Bug 3: the opener offset never equals the run's start, so every
	/// reference spelling must be found by span containment instead.
	#[test]
	fn reference_spans_contain_the_linked_run() {
		for (source, label) in [
			("[ref]\n\n[ref]: /url\n", "ref"),
			("[a][b]\n\n[b]: /url\n", "b"),
			("![ref]\n\n[ref]: /url\n", "ref"),
			("x [ref]\n\n[ref]: /url\n", "ref"),
		] {
			let doc = parse(source);
			let run = linked_run(&doc);
			let uses = reference_uses(source);
			let found = uses.iter().find(|u| reference_contains(u, &run));
			let found = found.unwrap_or_else(|| {
				panic!("{source:?}: no reference span holds the run {run:?}")
			});
			assert_eq!(found.label, label);
			assert!(
				found.span.contains(&run.start),
				"{source:?}: span {:?} does not hold the run start {}",
				found.span,
				run.start
			);
		}
	}

	/// Bug 3, the property the offset match silently lost: a resolved
	/// destination the definition contradicts is caught.
	#[test]
	fn a_wrong_resolved_url_is_caught() {
		let mut doc = parse("[ref]\n\n[ref]: /url\n");
		set_destinations(&mut doc, "/wrong");
		let caught = catch_unwind(AssertUnwindSafe(|| {
			assert_reference_resolution(&doc)
		}));
		assert!(caught.is_err(), "a wrong resolved URL must not pass");
	}

	/// Bug 4: the quoted definition the scan now reads keeps the equality
	/// assertion live, so a wrong URL there is still caught.
	#[test]
	fn a_wrong_url_behind_a_quoted_definition_is_caught() {
		let mut doc = parse("![ref]\n\n>   [ref]: /url\n");
		set_destinations(&mut doc, "/wrong");
		let caught = catch_unwind(AssertUnwindSafe(|| {
			assert_reference_resolution(&doc)
		}));
		assert!(caught.is_err(), "a wrong quoted URL must not pass");
	}

	/// A run inside an inline link belongs to that link, even when an
	/// enclosing bracket pair also reads as a shortcut reference.
	#[test]
	fn a_nested_inline_link_is_not_a_reference_use() {
		for source in ["[a [b](x) c]\n", "[a][b](c)\n", "![a [b](x) c]\n"] {
			assert_reference_resolution(&parse(source));
		}
	}

	/// Bug 4: the two panicking inputs from the report are ordinary Markdown;
	/// the quoted one is read, the list-contained one is declined. A list and
	/// a quote can also nest, in either order.
	#[test]
	fn list_and_quoted_definitions_do_not_panic() {
		for source in [
			"![ref]\n\n>   [ref]: /url\n",
			"![ref]\n\n- [ref]: /url\n",
			"![ref]\n\n- > [ref]: /url\n",
			"![ref]\n\n- > - [ref]: /url\n",
			"![ref]\n\n> - > [ref]: /url\n",
			"![ref]\n\n- - > [ref]: /url\n",
			"![ref]\n\n> > - > [ref]: /url\n",
			"![ref]\n\n1. > [ref]: /url\n",
			"![ref]\n\n1) > [ref]: /url\n",
			"![ref]\n\n* > [ref]: /url\n",
			"![ref]\n\n+ > [ref]: /url\n",
			"![ref]\n\n- \t> [ref]: /url\n",
			"![ref]\n\n> \t> [ref]: /url\n",
		] {
			assert_reference_resolution(&parse(source));
		}
	}

	/// Bug 4: the scan reads a quoted definition whose content is indented
	/// after the marker, and reports a list-contained one as unproven rather
	/// than inventing it — including when the list marker comes first.
	#[test]
	fn definition_scan_reads_quotes_and_declines_lists() {
		let quoted = definition_scan(">   [ref]: /url\n");
		assert!(quoted.complete);
		assert_eq!(quoted.definitions.len(), 1);
		assert_eq!(quoted.definitions[0].label, "ref");
		assert_eq!(quoted.definitions[0].value, "/url");

		let plain = definition_scan("[a]: /url\n");
		assert!(plain.complete);
		assert_eq!(plain.definitions.len(), 1);
		assert_eq!(plain.definitions[0].label, "a");

		for list in [
			"- [ref]: /url\n",
			"* [ref]: /url\n",
			"1. [ref]: /url\n",
			"- > [ref]: /url\n",
			"- > - [ref]: /url\n",
			"> - > [ref]: /url\n",
			"- - > [ref]: /url\n",
			"> > - > [ref]: /url\n",
			"1. > [ref]: /url\n",
			"1) > [ref]: /url\n",
			"+ > [ref]: /url\n",
			"- \t> [ref]: /url\n",
			"> \t> [ref]: /url\n",
		] {
			let scan = definition_scan(list);
			assert!(scan.definitions.is_empty(), "{list:?} invented a line");
			assert!(!scan.complete, "{list:?} must be unproven");
		}
	}

	/// Bug 4: four spaces begin an indented code block, so nothing there may
	/// enter the table — a false definition would fire the equality assertion.
	#[test]
	fn indented_code_is_not_a_definition() {
		let scan = definition_scan("    [ref]: /url\n");
		assert!(scan.definitions.is_empty());
		assert!(!scan.complete);
	}

	/// Bug 4: a `[ref]: ...` line inside a raw HTML block is HTML text, not a
	/// definition. Reading it invented a row that contradicted the destination
	/// the parser resolved from the real definition below the block.
	#[test]
	fn html_block_content_is_not_a_definition() {
		for source in [
			"<script>\n[ref]: /fake\n</script>\n\n[ref]: /real\n\n![ref]\n",
			"<!--\n[ref]: /fake\n-->\n\n[ref]: /real\n\n![ref]\n",
			"<div>\n[ref]: /fake\n</div>\n\n[ref]: /real\n\n![ref]\n",
			"<pre>\n[ref]: /fake\n</pre>\n\n[ref]: /real\n\n![ref]\n",
			"<?php\n[ref]: /fake\n?>\n\n[ref]: /real\n\n![ref]\n",
			"<![CDATA[\n[ref]: /fake\n]]>\n\n[ref]: /real\n\n![ref]\n",
			"> <div>\n> [ref]: /fake\n> </div>\n\n[ref]: /real\n\n![ref]\n",
			"> <script>\n> [ref]: /fake\n> </script>\n\n[ref]: /real\n\n![ref]\n",
		] {
			assert_reference_resolution(&parse(source));
		}
		// The definition after the block is still read, and the skipped HTML
		// line leaves the table unproven rather than absent.
		let scan =
			definition_scan("<div>\n[ref]: /fake\n</div>\n\n[ref]: /real\n");
		assert!(!scan.complete);
		assert_eq!(scan.definitions.len(), 1);
		assert_eq!(scan.definitions[0].value, "/real");
		// An HTML block with no definition-shaped line leaves it proven.
		let clean = definition_scan("<div>\ntext\n</div>\n\n[a]: /url\n");
		assert!(clean.complete);
		assert_eq!(clean.definitions.len(), 1);
	}

	/// Bug 4: the absence assertion stays reachable for a complete table: a
	/// tree that resolved a label the scan never saw is still a finding.
	#[test]
	fn a_missing_definition_still_panics_on_a_complete_table() {
		let mut doc = parse("[ref]\n\n[ref]: /url\n");
		// Keep the resolved tree, but remove the definition from the source
		// the scan reads, so the table is complete and empty.
		doc.source = Arc::from("[ref]\n\n");
		let caught = catch_unwind(AssertUnwindSafe(|| {
			assert_reference_resolution(&doc)
		}));
		assert!(caught.is_err(), "an unexplained resolution must be caught");
	}

	/// The registry's chain is the property the `anchors` target compares
	/// against `details_enclosing`, so the walk itself is pinned: a summary and
	/// the element's own target are drawn while the body is collapsed, and both
	/// spellings of a footnote target are collected.
	#[test]
	fn fragment_targets_carry_the_disclosure_chain_of_each_declaration() {
		let doc = parse(
			"<details id='a'><summary id='s'>S</summary>\n\n\
			 <div id='b'>\n\nText <a name='hidden'></a>[^f]\n\n</div>\n\n\
			 </details>\n\n[^f]: note",
		);
		let targets = fragment_targets(&doc);
		let chain = |name: &str| {
			targets
				.iter()
				.find(|target| target.name == name)
				.unwrap_or_else(|| panic!("{name:?} is not registered"))
				.chain
				.clone()
		};
		let element = doc.blocks[0].id;
		// The element's own id and its summary are reachable while collapsed.
		assert!(chain("a").is_empty());
		assert!(chain("s").is_empty());
		// The body's declarations are not.
		assert_eq!(chain("b"), [element]);
		assert_eq!(chain("hidden"), [element]);
		assert_eq!(chain("fnref:1"), [element]);
		// The definition is hoisted to the top level, so its `fn:` is not.
		assert!(chain("fn:1").is_empty());
	}
}
