//! Minimal raw-HTML support for a reader, not a browser.
//!
//! Comments are dropped. Tags whose meaning Markdown already expresses map to
//! the same semantics: `b`, `strong`, `i`, `em`, `del`, `s`, `strike`, `code`,
//! `kbd`, `samp`, `tt`, `sup`, `a`, `br`, `h1`-`h6`, `p` and `hr`. Attributes
//! such as `class` or `style` are never interpreted. Any other markup keeps
//! the existing fallback: the raw source is shown as code. Complete SVG
//! elements become atomic host-decoded images.

use std::collections::HashMap;

/// Block-level HTML tags whose multi-block form only groups Markdown
/// content. The group has no visual semantics of its own, so its children
/// render in place.
pub(crate) const TRANSPARENT_TAGS: &[&str] = &[
	"address",
	"article",
	"aside",
	"div",
	"figcaption",
	"figure",
	"footer",
	"header",
	"main",
	"nav",
	"p",
	"section",
];

/// One style delta carried by a supported tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Patch {
	Bold,
	Italic,
	Strike,
	Code,
	Superscript,
	Link(String),
	/// Recognized but visually neutral, such as `<a>` without an `href`.
	None,
}

/// Meaning of a single inline HTML fragment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inline {
	Image(crate::image::ImageSpec),
	/// Drop it: comment, declaration, stray closing tag.
	Ignore,
	/// Open a style scope; the matching close tag ends it.
	Open {
		name: String,
		patch: Patch,
	},
	/// Close the innermost scope with this name.
	Close {
		name: String,
	},
	/// A hard line break (`<br>`).
	Break,
	/// Unsupported markup: keep the literal source.
	Literal,
}

/// A styled run produced from a raw HTML block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
	pub anchor: Option<String>,
	pub image: Option<crate::image::ImageSpec>,
	pub text: String,
	/// The style in force for this run.
	pub style: Style,
}

/// The style one run inherited from the open tags around it.
///
/// The supported set is small and applying a patch twice adds nothing, so the
/// resolved state stays a few flags plus the active link however deep the tags
/// nest. Keeping the tags instead made every run carry a snapshot that grew
/// with the nesting depth. The link is shared so that copying the state — once
/// per run and once per open tag — never copies the URL.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Style {
	pub bold: bool,
	pub italic: bool,
	pub strike: bool,
	pub code: bool,
	pub superscript: bool,
	pub link: Option<crate::document::Link>,
}

impl Style {
	fn apply(&mut self, patch: &Patch) {
		match patch {
			Patch::Bold => self.bold = true,
			Patch::Italic => self.italic = true,
			Patch::Strike => self.strike = true,
			Patch::Code => self.code = true,
			Patch::Superscript => self.superscript = true,
			Patch::Link(url) => {
				self.link = Some(crate::document::Link::new(url.as_str()))
			}
			Patch::None => {}
		}
	}
}

/// Meaning of a raw HTML block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
	/// Nothing readable; no block is produced.
	Empty,
	/// `<hr>`.
	Rule,
	/// `<hN>...</hN>`.
	Heading { level: u8, text: Vec<Span> },
	/// Supported inline content; render as a paragraph.
	Paragraph(Vec<Span>),
	/// Contains unsupported markup; keep the literal source.
	Unsupported,
}

/// The name, remaining attributes and closing flag of one `<...>` tag.
fn tag_parts(tag: &str) -> Option<(String, &str, bool)> {
	let body = tag.strip_prefix('<')?.strip_suffix('>')?.trim();
	let closing = body.starts_with('/');
	let body = if closing {
		body[1..].trim_start()
	} else {
		body
	};
	let name = tag_name(body);
	if name.is_empty() {
		return None;
	}
	Some((name.to_ascii_lowercase(), &body[name.len()..], closing))
}

/// Elements whose content is not markup for the reader: raw text, RCDATA,
/// foreign content, and `<pre>`, which the reader shows as literal source.
const OPAQUE_ELEMENTS: &[&str] =
	&["math", "pre", "script", "style", "svg", "textarea", "title"];

/// Opaque elements whose content ends at the first matching close tag.
const RAW_ELEMENTS: &[&str] = &["script", "style", "textarea", "title"];

/// The offset just past the opaque element `name` whose opening tag ends
/// at `start`, when `name` is opaque. An unclosed element is skipped to
/// the end of `source`.
fn opaque_end(source: &str, start: usize, name: &str) -> Option<usize> {
	if RAW_ELEMENTS.contains(&name) {
		return Some(raw_end(source, start, name));
	}
	if OPAQUE_ELEMENTS.contains(&name) {
		return Some(enclosed_end(source, start, name).unwrap_or(source.len()));
	}
	None
}

/// The offset just past the first `</name>` at or after `start`, or the
/// end of `source` when the element never closes.
fn raw_end(source: &str, start: usize, name: &str) -> usize {
	let mut at = start;
	while let Some(found) = source[at..].find("</") {
		let found = at + found;
		let rest = &source[found..];
		if let Some(len) = tag_len(rest)
			&& let Some((tag, _, closing)) = tag_parts(&rest[..len])
			&& closing
			&& tag == name
		{
			return found + len;
		}
		at = found + 2;
	}
	source.len()
}

/// Length of a non-element HTML region, including its closing delimiter.
pub(crate) fn non_element_len(source: &str) -> Option<usize> {
	for (open, close) in [("<!--", "-->"), ("<![CDATA[", "]]>"), ("<?", "?>")] {
		if let Some(body) = source.strip_prefix(open) {
			return Some(
				body.find(close)
					.map_or(source.len(), |at| open.len() + at + close.len()),
			);
		}
	}
	source
		.starts_with("<!")
		.then(|| tag_len(source).unwrap_or(source.len()))
}

/// Every `<...>` tag in `source`, as `(start, length)`. A tag written
/// inside an HTML comment is not markup and is skipped.
fn tags(source: &str) -> impl Iterator<Item = (usize, usize)> + '_ {
	let mut at = 0;
	std::iter::from_fn(move || {
		while at < source.len() {
			let open = source[at..].find('<')? + at;
			let rest = &source[open..];
			if let Some(len) = non_element_len(rest) {
				at = open + len;
				continue;
			}
			match tag_len(rest) {
				Some(len) => {
					if let Some((name, attrs, closing)) =
						tag_parts(&rest[..len])
						&& !closing && !attrs.trim_end().ends_with('/')
						&& let Some(end) = opaque_end(source, open + len, &name)
					{
						at = end;
						continue;
					}
					at = open + len;
					return Some((open, len));
				}
				None => at = open + 1,
			}
		}
		None
	})
}

/// One container-level event in a raw HTML block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BlockEvent {
	/// A supported container opens.
	Open {
		name: String,
		open: bool,
		range: std::ops::Range<usize>,
	},
	/// A supported container closes.
	Close {
		name: String,
		range: std::ops::Range<usize>,
	},
	/// The raw inner text of a `<summary>` element.
	Summary {
		text: String,
		range: std::ops::Range<usize>,
	},
	/// Everything else, which the HTML subset renders.
	Text {
		text: String,
		range: std::ops::Range<usize>,
	},
}

/// Whether `name` opens a supported block container.
fn is_container(name: &str) -> bool {
	name == "details" || TRANSPARENT_TAGS.contains(&name)
}

/// Scan one raw HTML block for container events. Comments and the contents
/// of opaque elements are left out, and tags that are not containers stay in
/// the surrounding `Text`.
pub(crate) fn block_events(source: &str) -> Vec<BlockEvent> {
	let mut events = Vec::new();
	let mut text_start = 0usize;
	let mut it = tags(source);
	while let Some((start, len)) = it.next() {
		let fragment = &source[start..start + len];
		let Some((name, attrs, closing)) = tag_parts(fragment) else {
			continue;
		};
		let container = is_container(&name);
		let summary = name == "summary" && !closing;
		if (!container && !summary)
			|| (!closing && attrs.trim_end().ends_with('/'))
		{
			continue;
		}
		if start > text_start {
			events.push(BlockEvent::Text {
				text: source[text_start..start].to_string(),
				range: text_start..start,
			});
		}
		if summary {
			// Consume the summary's own tags in the same pass, so an unclosed
			// opener cannot rescan the suffix once per opener.
			let content_start = start + len;
			let close = consume_summary_close(source, &mut it);
			match close {
				Some((next, next_len)) => {
					events.push(BlockEvent::Summary {
						text: source[content_start..next].to_string(),
						range: start..next + next_len,
					});
					text_start = next + next_len;
				}
				None => {
					// Keep the opener and the rest as one text run.
					text_start = start;
				}
			}
			continue;
		}
		let range = start..start + len;
		if closing {
			events.push(BlockEvent::Close { name, range });
		} else {
			events.push(BlockEvent::Open {
				open: has_attribute(attrs, "open"),
				name,
				range,
			});
		}
		text_start = start + len;
	}
	if text_start < source.len() {
		events.push(BlockEvent::Text {
			text: source[text_start..].to_string(),
			range: text_start..source.len(),
		});
	}
	events
}

fn consume_summary_close(
	source: &str,
	tags: &mut impl Iterator<Item = (usize, usize)>,
) -> Option<(usize, usize)> {
	for (start, len) in tags.by_ref() {
		if let Some((name, _, closing)) = tag_parts(&source[start..start + len])
			&& closing
			&& name == "summary"
		{
			return Some((start, len));
		}
	}
	None
}

/// Whether `source` leaves a supported block container open. `<details>`
/// and the transparent grouping tags both span ordinary Markdown blocks,
/// so a prefix that cuts one open cannot be parsed on its own.
pub(crate) fn has_open_container(source: &str) -> bool {
	let mut open: HashMap<String, u32> = HashMap::new();
	for (start, len) in tags(source) {
		let Some((name, attrs, closing)) =
			tag_parts(&source[start..start + len])
		else {
			continue;
		};
		if attrs.trim_end().ends_with('/')
			|| name != "details" && !TRANSPARENT_TAGS.contains(&name.as_str())
		{
			continue;
		}
		let depth = open.entry(name).or_insert(0);
		*depth = if closing {
			depth.saturating_sub(1)
		} else {
			depth.saturating_add(1)
		};
	}
	open.values().any(|depth| *depth > 0)
}

/// Whether an attribute is present, with or without a value. `open` is the one
/// attribute `<details>` interprets, and it may be bare.
fn has_attribute(attrs: &str, name: &str) -> bool {
	let mut rest = attrs;
	loop {
		// A trailing `/` of a self-closing tag is a separator, so trimming it
		// can leave nothing to read.
		rest = rest.trim_start().trim_start_matches('/');
		if rest.is_empty() {
			return false;
		}
		let end = rest
			.find(|c: char| c.is_whitespace() || c == '=')
			.unwrap_or(rest.len());
		if end == 0 {
			// Stripping a leading `/` can expose whitespace (or an `=`), and
			// that character may be multi-byte — a byte-wise skip splits it.
			rest = &rest[rest.chars().next().map_or(0, char::len_utf8)..];
			continue;
		}
		if rest[..end].eq_ignore_ascii_case(name) {
			return true;
		}
		rest = rest[end..].trim_start();
		let Some(value) = rest.strip_prefix('=') else {
			continue;
		};
		rest = value.trim_start();
		rest = match rest.chars().next() {
			Some(quote @ ('"' | '\'')) => {
				let body = &rest[1..];
				match body.find(quote) {
					Some(end) => &body[end + 1..],
					None => "",
				}
			}
			_ => {
				let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
				&rest[end..]
			}
		};
	}
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Tag {
	Image(crate::image::ImageSpec),
	/// Comment, declaration or processing instruction.
	Comment,
	Open {
		name: String,
		patch: Patch,
	},
	Close {
		name: String,
	},
	/// Void element; only `br` and `hr` are recognized.
	Void {
		name: String,
	},
	Unknown,
}

enum Token {
	Text(String),
	Image(crate::image::ImageSpec, Vec<String>),
	Comment,
	Tag(String, bool),
}

/// Interpret one inline HTML fragment as produced by the Markdown parser.
pub fn inline(fragment: &str) -> Inline {
	match classify(fragment) {
		Tag::Image(image) => Inline::Image(image),
		Tag::Comment => Inline::Ignore,
		Tag::Open { name, patch } => Inline::Open { name, patch },
		Tag::Close { name } => Inline::Close { name },
		Tag::Void { name } if name == "br" => Inline::Break,
		Tag::Void { .. } | Tag::Unknown => Inline::Literal,
	}
}

/// Interpret a raw HTML block; block-level tags must be self-contained.
///
/// `scope_limit` bounds the tags that may stay open at once, so a run of
/// unclosed tags cannot grow the scope stack without bound.
pub fn block(source: &str, scope_limit: usize) -> Block {
	block_from(source, scope_limit, 0)
}

pub(crate) fn block_from(
	source: &str,
	scope_limit: usize,
	anchor_from: usize,
) -> Block {
	let mut spans: Vec<Span> = Vec::new();
	let mut style = Style::default();
	// Each open tag remembers the style its contents inherited, so closing it
	// restores that state and drops whatever the tags inside it added.
	let mut scopes: Vec<(String, Style)> = Vec::new();
	let mut first_open: Option<String> = None;
	let mut containers = 0;
	let mut rule = false;
	for token in tokenize(source, anchor_from) {
		let Some((fragment, discover)) =
			append_html_token(token, &mut spans, &style)
		else {
			continue;
		};
		for anchor in anchors(&fragment).into_iter().filter(|_| discover) {
			spans.push(Span {
				anchor: Some(anchor),
				image: None,
				text: String::new(),
				style: style.clone(),
			});
		}
		match classify(&fragment) {
			Tag::Image(image) => spans.push(Span {
				anchor: None,
				image: Some(image),
				text: String::new(),
				style: style.clone(),
			}),
			Tag::Comment => {}
			Tag::Unknown => return Block::Unsupported,
			Tag::Void { name } => {
				if name == "hr" {
					rule = true;
				} else {
					push_span(&mut spans, "\n".into(), &style);
				}
			}
			Tag::Open { name, patch } => {
				if scopes.len() >= scope_limit {
					// Past the budget the block is kept as source, like any
					// unsupported markup.
					return Block::Unsupported;
				}
				if first_open.is_none() {
					first_open = Some(name.clone());
				}
				if name == "p" || heading_level(&name).is_some() {
					containers += 1;
				}
				scopes.push((name, style.clone()));
				style.apply(&patch);
			}
			Tag::Close { name } => {
				if let Some(i) =
					scopes.iter().rposition(|(open, _)| *open == name)
				{
					style = std::mem::take(&mut scopes[i].1);
					scopes.truncate(i);
				}
			}
		}
	}
	finish_html_block(spans, first_open, containers, rule)
}

fn append_html_token(
	token: Token,
	spans: &mut Vec<Span>,
	style: &Style,
) -> Option<(String, bool)> {
	match token {
		Token::Image(image, anchors) => {
			for anchor in anchors {
				spans.push(Span {
					anchor: Some(anchor),
					image: None,
					text: String::new(),
					style: style.clone(),
				});
			}
			spans.push(Span {
				anchor: None,
				image: Some(image),
				text: String::new(),
				style: style.clone(),
			});
			None
		}
		Token::Text(t) => {
			push_text(spans, &t, style);
			None
		}
		Token::Comment => None,
		Token::Tag(t, discover) => Some((t, discover)),
	}
}

fn finish_html_block(
	spans: Vec<Span>,
	first_open: Option<String>,
	containers: usize,
	rule: bool,
) -> Block {
	let text = normalize(spans);
	if text.iter().all(|s| s.anchor.is_some()) {
		return if rule {
			Block::Rule
		} else if text.is_empty() {
			Block::Empty
		} else {
			Block::Paragraph(text)
		};
	}
	// Only a block whose single element is the heading becomes a heading; a
	// run of elements is read as one paragraph instead.
	match first_open.as_deref().and_then(heading_level) {
		Some(level) if containers == 1 && !rule => {
			Block::Heading { level, text }
		}
		_ => Block::Paragraph(text),
	}
}

fn classify(fragment: &str) -> Tag {
	let text = fragment.trim();
	if text.starts_with("<!") || text.starts_with("<?") {
		return Tag::Comment;
	}
	let Some(body) = text.strip_prefix('<').and_then(|t| t.strip_suffix('>'))
	else {
		return Tag::Unknown;
	};
	let body = body.trim();
	let closing = body.starts_with('/');
	let body = if closing {
		body[1..].trim_start()
	} else {
		body
	};
	let name = tag_name(body);
	if name.is_empty() {
		return Tag::Unknown;
	}
	let name = name.to_ascii_lowercase();
	if closing {
		return if known(&name) {
			Tag::Close { name }
		} else {
			Tag::Unknown
		};
	}
	if name == "br" || name == "hr" {
		return Tag::Void { name };
	}
	let attrs = &body[name.len()..];
	if name == "img" {
		let dimension = |name| {
			attribute(attrs, name)
				.and_then(|v| v.parse::<u32>().ok())
				.filter(|v| *v > 0)
		};
		return Tag::Image(crate::image::ImageSpec {
			src: attribute(attrs, "src").unwrap_or_default(),
			alt: attribute(attrs, "alt").unwrap_or_default(),
			title: attribute(attrs, "title").unwrap_or_default(),
			width: dimension("width"),
			height: dimension("height"),
		});
	}
	if let Some(patch) = patch(&name, attrs) {
		return Tag::Open { name, patch };
	}
	if name == "p" || heading_level(&name).is_some() {
		return Tag::Open {
			name,
			patch: Patch::None,
		};
	}
	Tag::Unknown
}

fn known(name: &str) -> bool {
	name == "br"
		|| name == "hr"
		|| name == "p"
		|| heading_level(name).is_some()
		|| patch(name, "").is_some()
}

fn patch(name: &str, attrs: &str) -> Option<Patch> {
	Some(match name {
		"b" | "strong" => Patch::Bold,
		"i" | "em" => Patch::Italic,
		"del" | "s" | "strike" => Patch::Strike,
		"code" | "kbd" | "samp" | "tt" => Patch::Code,
		"sup" => Patch::Superscript,
		"a" => match attribute(attrs, "href") {
			Some(url) => Patch::Link(url),
			None => Patch::None,
		},
		_ => return None,
	})
}

fn heading_level(name: &str) -> Option<u8> {
	let level = name.strip_prefix('h')?.parse::<u8>().ok()?;
	(1..=6).contains(&level).then_some(level)
}

fn tag_name(body: &str) -> &str {
	let end = body
		.find(|c: char| {
			!(c.is_ascii_alphanumeric() || c == '-' || c == ':' || c == '_')
		})
		.unwrap_or(body.len());
	&body[..end]
}

/// Fragment targets declared by one opening tag.
pub(crate) fn anchors(fragment: &str) -> Vec<String> {
	let Some((name, attrs, false)) = tag_parts(fragment) else {
		return Vec::new();
	};
	attribute(attrs, "id")
		.into_iter()
		.chain((name == "a").then(|| attribute(attrs, "name")).flatten())
		.filter(|a| !a.is_empty())
		.collect()
}

/// The end of an opaque element starting at this opening tag.
pub(crate) fn opaque_len(source: &str) -> Option<usize> {
	let len = tag_len(source)?;
	let (name, attrs, closing) = tag_parts(&source[..len])?;
	if closing || attrs.trim_end().ends_with('/') {
		return None;
	}
	opaque_end(source, len, &name)
}

/// Targets in raw HTML, excluding comments and opaque element contents.
pub(crate) fn block_anchors(source: &str) -> Vec<String> {
	block_anchors_from(source, 0)
}

pub(crate) fn block_anchors_from(source: &str, from: usize) -> Vec<String> {
	let mut out = Vec::new();
	let mut at = from.min(source.len());
	while let Some(open) = source[at..].find('<').map(|i| at + i) {
		let rest = &source[open..];
		if let Some(len) = non_element_len(rest) {
			at = open + len;
			continue;
		}
		let Some(len) = tag_len(rest) else {
			at = open + 1;
			continue;
		};
		if open >= from {
			out.extend(anchors(&rest[..len]));
		}
		at = open + len;
		if let Some((name, attrs, false)) = tag_parts(&rest[..len])
			&& !attrs.trim_end().ends_with('/')
			&& let Some(end) = opaque_end(source, at, &name)
		{
			at = end;
		}
	}
	out
}

/// Read one attribute value; `class`, `style` and the rest are simply ignored.
fn attribute(attrs: &str, name: &str) -> Option<String> {
	let mut rest = attrs;
	loop {
		// A trailing `/` of a self-closing tag is a separator, so trimming it
		// can leave nothing to read.
		rest = rest.trim_start().trim_start_matches('/');
		if rest.is_empty() {
			return None;
		}
		let end = rest
			.find(|c: char| c.is_whitespace() || c == '=')
			.unwrap_or(rest.len());
		if end == 0 {
			// Stripping a leading `/` can expose whitespace (or an `=`), and
			// that character may be multi-byte — a byte-wise skip splits it.
			rest = &rest[rest.chars().next().map_or(0, char::len_utf8)..];
			continue;
		}
		let key = &rest[..end];
		rest = rest[end..].trim_start();
		let Some(value) = rest.strip_prefix('=') else {
			continue;
		};
		rest = value.trim_start();
		let value = match rest.chars().next() {
			Some(quote @ ('"' | '\'')) => {
				let body = &rest[1..];
				let end = body.find(quote)?;
				rest = &body[end + 1..];
				&body[..end]
			}
			_ => {
				let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
				let value = &rest[..end];
				rest = &rest[end..];
				value
			}
		};
		if key.eq_ignore_ascii_case(name) {
			return Some(html_escape::decode_html_entities(value).into_owned());
		}
	}
}
fn tokenize(source: &str, anchor_from: usize) -> Vec<Token> {
	let mut tokens = Vec::new();
	let mut i = 0;
	while i < source.len() {
		let rest = &source[i..];
		let Some(open) = rest.find('<') else {
			tokens.push(Token::Text(rest.to_string()));
			break;
		};
		if open > 0 {
			tokens.push(Token::Text(rest[..open].to_string()));
		}
		let candidate = &rest[open..];
		if let Some(len) = non_element_len(candidate) {
			i += open + len;
			tokens.push(Token::Comment);
			continue;
		}
		if let Some((len, image)) = svg(candidate) {
			tokens.push(Token::Image(
				image,
				block_anchors_from(
					&candidate[..len],
					anchor_from.saturating_sub(i + open),
				),
			));
			i += open + len;
			continue;
		}
		match tag_len(candidate) {
			Some(len) => {
				tokens.push(Token::Tag(
					candidate[..len].to_string(),
					i + open >= anchor_from,
				));
				i += open + len;
			}
			// A bare `<` is ordinary text, so keep scanning after it.
			None => {
				tokens.push(Token::Text("<".into()));
				i += open + 1;
			}
		}
	}
	tokens
}

/// One complete SVG element as an atomic, host-decoded image.
/// The offset just past the matching close tag of one `name` element whose
/// opening tag ends at `start`. Nested same-name elements are counted, and
/// comments and CDATA are skipped. `None` when the element never closes.
fn enclosed_end(source: &str, start: usize, name: &str) -> Option<usize> {
	let mut depth = 1usize;
	let mut at = start;
	while depth > 0 {
		let open = source[at..].find('<')? + at;
		let rest = &source[open..];
		if let Some(len) = non_element_len(rest) {
			at = open + len;
			continue;
		}
		let Some(len) = tag_len(rest) else {
			at = open + 1;
			continue;
		};
		let end = open + len;
		if let Some((tag, attrs, closing)) = tag_parts(&source[open..end])
			&& tag == name
			&& !attrs.trim_end().ends_with('/')
		{
			if closing {
				depth -= 1;
			} else {
				depth += 1;
			}
		}
		at = end;
	}
	Some(at)
}

pub(crate) fn svg_len(source: &str) -> Option<usize> {
	if !source.starts_with('<') {
		return None;
	}
	let first = tag_len(source)?;
	let (name, attrs, closing) = tag_parts(&source[..first])?;
	if name != "svg" || closing {
		return None;
	}
	if attrs.trim_end().ends_with('/') {
		return Some(first);
	}
	enclosed_end(source, first, "svg")
}

pub(crate) fn svg(source: &str) -> Option<(usize, crate::image::ImageSpec)> {
	let end = svg_len(source)?;
	let first = tag_len(source)?;
	let (_, attrs, _) = tag_parts(&source[..first])?;
	let mut xml = source[..end].to_string();
	if attribute(attrs, "xmlns").is_none() {
		xml.insert_str(4, " xmlns=\"http://www.w3.org/2000/svg\"");
	}
	Some((
		end,
		crate::image::ImageSpec {
			src: format!(
				"data:image/svg+xml,{}",
				percent_encoding::utf8_percent_encode(
					&xml,
					percent_encoding::NON_ALPHANUMERIC
				)
			),
			alt: String::new(),
			title: String::new(),
			width: attribute(attrs, "width").and_then(|v| v.parse().ok()),
			height: attribute(attrs, "height").and_then(|v| v.parse().ok()),
		},
	))
}

/// Byte length of a `<...>` candidate, honoring quoted attribute values.
pub(crate) fn tag_len(source: &str) -> Option<usize> {
	let mut chars = source.char_indices();
	chars.next()?;
	let (_, second) = chars.next()?;
	if !(second.is_ascii_alphabetic()
		|| second == '/'
		|| second == '!'
		|| second == '?')
	{
		return None;
	}
	let mut quote = None;
	for (i, c) in source.char_indices().skip(1) {
		match (quote, c) {
			(Some(q), c) if c == q => quote = None,
			(Some(_), _) => {}
			(None, '"' | '\'') => quote = Some(c),
			(None, '>') => return Some(i + 1),
			(None, '<') => return None,
			_ => {}
		}
	}
	None
}

fn push_text(spans: &mut Vec<Span>, text: &str, style: &Style) {
	let mut collapsed = String::with_capacity(text.len());
	let mut space = false;
	for c in text.chars() {
		if c.is_whitespace() {
			space = true;
			continue;
		}
		if space {
			collapsed.push(' ');
			space = false;
		}
		collapsed.push(c);
	}
	if space {
		collapsed.push(' ');
	}
	push_span(spans, collapsed, style);
}

fn push_span(spans: &mut Vec<Span>, text: String, style: &Style) {
	if text.is_empty() {
		return;
	}
	if let Some(last) = spans.last_mut()
		&& last.image.is_none()
		&& last.anchor.is_none()
		&& &last.style == style
	{
		let text = if last.text.ends_with(' ') {
			text.trim_start_matches(' ')
		} else {
			&text
		};
		last.text.push_str(text);
		return;
	}
	spans.push(Span {
		anchor: None,
		image: None,
		text,
		style: style.clone(),
	});
}

/// Collapse runs of whitespace and merge runs that share a style.
fn normalize(spans: Vec<Span>) -> Vec<Span> {
	let mut out: Vec<Span> = Vec::new();
	let mut content = false;
	let mut space = false;
	for span in spans {
		if span.image.is_some() || span.anchor.is_some() {
			content |= span.image.is_some();
			if span.image.is_some() {
				space = false;
			}
			out.push(span);
			continue;
		}
		let text = if !content {
			span.text.trim_start()
		} else if space {
			span.text.trim_start_matches(' ')
		} else {
			span.text.as_str()
		};
		if text.is_empty() {
			continue;
		}
		content = true;
		space = text.ends_with(' ');
		let mut merged = false;
		if let Some(last) = out.last_mut()
			&& last.image.is_none()
			&& last.anchor.is_none()
			&& last.style == span.style
		{
			last.text.push_str(text);
			merged = true;
		}
		if !merged {
			out.push(Span {
				anchor: None,
				image: None,
				text: text.to_string(),
				style: span.style,
			});
		}
	}
	trim_trailing_whitespace(&mut out);
	out
}

#[cfg(test)]
mod tests;

fn trim_trailing_whitespace(out: &mut Vec<Span>) {
	for span in out.iter_mut().rev() {
		if span.anchor.is_some() {
			continue;
		}
		if span.image.is_some() {
			break;
		}
		let len = span.text.trim_end().len();
		span.text.truncate(len);
		if len > 0 {
			break;
		}
	}
	out.retain(|span| {
		span.anchor.is_some() || span.image.is_some() || !span.text.is_empty()
	});
}
