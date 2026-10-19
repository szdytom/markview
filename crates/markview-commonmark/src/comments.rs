//! OFM comments are removed before semantic parsing and mapped back once.
use super::{Block, BlockKind, Dialect, RichText};
use comrak::{Arena, nodes::NodeValue, parse_document};
use std::ops::Range;

pub(super) struct Masked {
	pub(super) text: String,
	offsets: Vec<(usize, usize)>,
}

pub(super) fn mask(source: &str, dialect: Dialect) -> Option<Masked> {
	if !dialect.ofm || !source.contains("%%") {
		return None;
	}
	let ranges = scan(source, dialect);
	(!ranges.is_empty()).then(|| build(source, &ranges))
}

/// Only original source determines opener positions, even after a deletion.
fn scan(source: &str, dialect: Dialect) -> Vec<Range<usize>> {
	let mut protected = protection(source, dialect);
	let mut ranges = Vec::new();
	let mut cursor = 0;
	let lines: Vec<_> = source_lines(source)
		.into_iter()
		.map(|range| {
			let content = source[range.clone()].trim_end_matches(['\r', '\n']);
			let end = range.start + content.len();
			let opener = range.start + content.len() - logical(content).len();
			(range, end, opener)
		})
		.collect();
	while let Some(start) = next_marker(source.as_bytes(), cursor) {
		let protected_at =
			protected.partition_point(|range| range.end <= start);
		if let Some(range) = protected.get(protected_at)
			&& range.start <= start
		{
			cursor = range.end;
			continue;
		}
		let line_at = lines.partition_point(|(range, _, _)| range.end <= start);
		let (line, line_end, opener) = &lines[line_at];
		let line_start = line.start;
		let line_end = *line_end;
		let block = start == *opener;
		let limit = if block { source.len() } else { line_end };
		let close = next_marker(&source.as_bytes()[..limit], start + 2);
		let Some(mut end) = close
			.map(|at| at + 2)
			.or_else(|| block.then_some(source.len()))
		else {
			cursor = start + 2;
			continue;
		};
		// Keep existing word separation without manufacturing whitespace.
		if start > line_start
			&& end < line_end
			&& matches!(source.as_bytes()[start - 1], b' ' | b'\t')
			&& matches!(source.as_bytes()[end], b' ' | b'\t')
		{
			end += 1;
		}
		let invalidated = protected
			.get(protected_at)
			.is_some_and(|range| range.start < end);
		let structural = source[start..end].contains(['<', '`', '~']);
		let exposed = block && exposes_code(&source[end..]);
		ranges.push(start..end);
		cursor = end;
		if invalidated || structural || exposed {
			// Only code discovery sees the visible copy. Delimiter decisions
			// always use original lines, so no midline opener is promoted.
			let visible = build(source, &ranges);
			protected = protection(&visible.text, dialect);
			for range in &mut protected {
				*range = original_range(range, &visible.offsets);
			}
		}
	}
	ranges
}

/// A removed line-start comment can expose a fence or an indented block.
fn exposes_code(tail: &str) -> bool {
	let Some(line) = tail
		.split(['\r', '\n'])
		.find(|line| !line.trim().is_empty())
	else {
		return false;
	};
	let rest = line.trim_start_matches([' ', '\t']);
	line.len() - rest.len() >= 4
		|| line.starts_with('\t')
		|| rest.starts_with(['`', '~', '$'])
}

/// Discover code only in AST contexts where Markdown or HTML is active.
fn protection(source: &str, dialect: Dialect) -> Vec<Range<usize>> {
	let arena = Arena::new();
	let root = parse_document(
		&arena,
		source,
		&super::parse::markdown_options(dialect),
	);
	let lines = source_lines(source);
	let position = |p: comrak::nodes::Sourcepos| {
		let start = lines[p.start.line.saturating_sub(1)].start
			+ p.start.column.saturating_sub(1);
		let end = (lines[p.end.line.saturating_sub(1)].start + p.end.column)
			.min(source.len());
		start..end
	};
	let mut ranges = Vec::new();
	let mut inline_scopes = std::collections::HashSet::new();
	for node in root.descendants() {
		let data = node.data.borrow();
		let range = position(data.sourcepos);
		match &data.value {
			NodeValue::Code(_)
			| NodeValue::CodeBlock(_)
			| NodeValue::Math(_)
			| NodeValue::FrontMatter(_) => ranges.push(range),
			NodeValue::HtmlInline(_) => {
				let scope = node
					.ancestors()
					.find(|parent| parent.data.borrow().value.block())
					.unwrap();
				let range = position(scope.data.borrow().sourcepos);
				if inline_scopes.insert((range.start, range.end)) {
					ranges.extend(inline_code_ranges(
						scope, source, dialect, &position, false, 0, &mut 0,
					));
				}
			}
			NodeValue::HtmlBlock(_) => {
				let html = &source[range.clone()];
				ranges.extend(
					super::html::comment_code_ranges(html).into_iter().map(
						|inner| {
							inner.start + range.start..inner.end + range.start
						},
					),
				);
				let events = super::html::block_events(html);
				if events.iter().any(|event| {
					matches!(event, super::html::BlockEvent::Open { .. })
				}) {
					for event in events {
						if let super::html::BlockEvent::Text {
							text,
							range: inner,
						} = event
						{
							ranges.extend(
								protection(&text, dialect).into_iter().map(
									|part| {
										part.start + range.start + inner.start
											..part.end
												+ range.start + inner.start
									},
								),
							);
						}
					}
				}
			}
			_ => {}
		}
	}
	ranges.sort_by_key(|range| range.start);
	let mut merged: Vec<Range<usize>> = Vec::new();
	for range in ranges {
		if let Some(last) = merged.last_mut()
			&& range.start <= last.end
		{
			last.end = last.end.max(range.end);
		} else {
			merged.push(range);
		}
	}
	merged
}

/// Each Markdown recursion inherits code style but owns its HTML scope stack.
#[cfg_attr(
	any(),
	markview::allow_complexity(
		reason = "Keep the source-ordered HTML scope stack, inherited code style and opaque cursor together to match the reader recursion boundaries."
	)
)]
fn inline_code_ranges<'a>(
	scope: &'a comrak::nodes::AstNode<'a>,
	source: &str,
	dialect: Dialect,
	position: &impl Fn(comrak::nodes::Sourcepos) -> Range<usize>,
	inherited_code: bool,
	depth: usize,
	opaque_until: &mut usize,
) -> Vec<Range<usize>> {
	use super::html::{self, Inline, Patch};
	let scope_range = position(scope.data.borrow().sourcepos);
	let end = scope_range.end;
	if depth >= crate::limits::Limits::default().inline_depth {
		return inherited_code.then_some(scope_range).into_iter().collect();
	}
	let html_end = position(
		scope
			.ancestors()
			.find(|parent| parent.data.borrow().value.block())
			.unwrap()
			.data
			.borrow()
			.sourcepos,
	)
	.end;
	let mut ranges = Vec::new();
	let mut scopes = Vec::new();
	let mut code = inherited_code;
	let mut start = inherited_code.then_some(scope_range.start);
	for node in scope.children() {
		let data = node.data.borrow();
		let NodeValue::HtmlInline(fragment) = &data.value else {
			if node.first_child().is_some() {
				ranges.extend(inline_code_ranges(
					node,
					source,
					dialect,
					position,
					code,
					depth + 1,
					opaque_until,
				));
			}
			continue;
		};
		let range = position(data.sourcepos);
		if range.start < *opaque_until {
			continue;
		}
		if let Some(len) = html::opaque_len(&source[range.start..html_end]) {
			*opaque_until = range.start + len;
			if html::comment_code_len(&source[range.start..html_end]).is_some()
			{
				ranges.push(range.start..*opaque_until);
			}
			continue;
		}
		match html::inline(fragment, dialect.ofm) {
			Inline::Open { name, patch }
				if scopes.len()
					< crate::limits::Limits::default().html_scopes =>
			{
				scopes.push((name, code));
				code |= matches!(patch, Patch::Code);
			}
			Inline::Close { name } => {
				if let Some(index) =
					scopes.iter().rposition(|(open, _)| *open == name)
				{
					code = scopes[index].1;
					scopes.truncate(index);
				}
			}
			_ => {}
		}
		if code && start.is_none() {
			start = Some(range.start);
		}
		if !code && let Some(start) = start.take() {
			ranges.push(start..range.end);
		}
	}
	if let Some(start) = start {
		ranges.push(start..end);
	}
	ranges
}

fn source_lines(source: &str) -> Vec<Range<usize>> {
	let mut start = 0;
	let mut lines = Vec::new();
	let bytes = source.as_bytes();
	for (at, byte) in bytes.iter().enumerate() {
		if *byte == b'\n'
			|| (*byte == b'\r' && bytes.get(at + 1) != Some(&b'\n'))
		{
			lines.push(start..at + 1);
			start = at + 1;
		}
	}
	lines.push(start..source.len());
	lines
}

fn logical(line: &str) -> &str {
	let mut rest = line;
	loop {
		// Indented code is already excluded by the parser's ranges.
		let trimmed = rest.trim_start_matches([' ', '\t']);
		if let Some(follow) = trimmed.strip_prefix('>') {
			rest = match follow.as_bytes().first() {
				Some(b' ' | b'\t') => &follow[1..],
				_ => follow,
			};
			continue;
		}
		if let Some(len) = list_marker_len(trimmed) {
			rest = &trimmed[len..];
			continue;
		}
		return trimmed;
	}
}

/// The length of the list marker `line` starts with, if any.
fn list_marker_len(line: &str) -> Option<usize> {
	let bytes = line.as_bytes();
	let bullet = matches!(bytes.first(), Some(b'-' | b'+' | b'*'));
	let digits = bytes
		.iter()
		.take_while(|byte| byte.is_ascii_digit())
		.count();
	let (marker, after) = if bullet {
		(1, &bytes[1..])
	} else if digits > 0
		&& digits <= 9
		&& matches!(bytes.get(digits), Some(b'.' | b')'))
	{
		(digits + 1, &bytes[digits + 1..])
	} else {
		return None;
	};
	match after.first() {
		None => Some(marker),
		Some(b' ' | b'\t') => Some(marker + 1),
		_ => None,
	}
}

fn escaped(bytes: &[u8], at: usize) -> bool {
	bytes[..at]
		.iter()
		.rev()
		.take_while(|byte| **byte == b'\\')
		.count()
		% 2 == 1
}

fn next_marker(bytes: &[u8], from: usize) -> Option<usize> {
	(from..bytes.len().saturating_sub(1))
		.find(|at| bytes[*at..].starts_with(b"%%") && !escaped(bytes, *at))
}

fn build(source: &str, ranges: &[Range<usize>]) -> Masked {
	let mut text = String::with_capacity(source.len());
	let mut offsets = Vec::with_capacity(ranges.len());
	let mut cursor = 0;
	for range in ranges {
		text.push_str(&source[cursor..range.start]);
		cursor = range.end;
		offsets.push((text.len(), cursor - text.len()));
	}
	text.push_str(&source[cursor..]);
	Masked { text, offsets }
}

/// Starts use the visible byte to their right; ends use the byte to their left.
fn original_range(
	source: &Range<usize>,
	offsets: &[(usize, usize)],
) -> Range<usize> {
	let shift = |at, end| {
		let count = offsets.partition_point(|(position, _)| {
			if end { *position < at } else { *position <= at }
		});
		at + count.checked_sub(1).map_or(0, |index| offsets[index].1)
	};
	let start = shift(source.start, false);
	let end = if source.is_empty() {
		start
	} else {
		shift(source.end, true)
	};
	start..end
}

pub(super) fn restore(blocks: &mut [Block], masked: &Masked) {
	let offsets = &masked.offsets;
	fn range(offsets: &[(usize, usize)], source: &mut Range<usize>) {
		*source = original_range(source, offsets);
	}
	fn rich(offsets: &[(usize, usize)], text: &mut RichText) {
		for inline in text {
			range(offsets, &mut inline.source);
		}
	}
	fn walk(offsets: &[(usize, usize)], blocks: &mut [Block]) {
		for block in blocks {
			range(offsets, &mut block.source);
			match &mut block.kind {
				BlockKind::Paragraph(text)
				| BlockKind::Heading { text, .. } => {
					rich(offsets, text);
				}
				BlockKind::Quote { blocks, .. }
				| BlockKind::Footnote { blocks, .. }
				| BlockKind::FrontMatter { blocks, .. } => {
					walk(offsets, blocks);
				}
				BlockKind::Details {
					summary, blocks, ..
				} => {
					rich(offsets, summary);
					walk(offsets, blocks);
				}
				BlockKind::List { items, .. } => {
					for item in items {
						walk(offsets, &mut item.blocks);
					}
				}
				BlockKind::Table { rows, .. } => {
					for row in rows {
						for cell in row {
							rich(offsets, cell);
						}
					}
				}
				BlockKind::Code { .. } | BlockKind::Rule => {}
			}
		}
	}
	walk(offsets, blocks);
}
