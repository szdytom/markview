//! Minimal raw-HTML support for a reader, not a browser.
//!
//! Comments are dropped. Tags whose meaning Markdown already expresses map to
//! the same semantics: `b`, `strong`, `i`, `em`, `del`, `s`, `strike`, `code`,
//! `kbd`, `samp`, `tt`, `sup`, `a`, `br`, `h1`-`h6`, `p` and `hr`. Attributes
//! such as `class` or `style` are never interpreted. Any other markup keeps
//! the existing fallback: the raw source is shown as code.

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
	pub image: Option<crate::image::ImageSpec>,
	pub text: String,
	/// Style patches in opening order.
	pub styles: Vec<Patch>,
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
	Comment,
	Tag(String),
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
pub fn block(source: &str) -> Block {
	let mut spans: Vec<Span> = Vec::new();
	let mut styles: Vec<Patch> = Vec::new();
	let mut scopes: Vec<(String, usize)> = Vec::new();
	let mut first_open: Option<String> = None;
	let mut containers = 0;
	let mut rule = false;
	for token in tokenize(source) {
		let fragment = match token {
			Token::Text(t) => {
				push_text(&mut spans, &t, &styles);
				continue;
			}
			Token::Comment => continue,
			Token::Tag(t) => t,
		};
		match classify(&fragment) {
			Tag::Image(image) => spans.push(Span {
				image: Some(image),
				text: String::new(),
				styles: styles.clone(),
			}),
			Tag::Comment => {}
			Tag::Unknown => return Block::Unsupported,
			Tag::Void { name } => {
				if name == "hr" {
					rule = true;
				} else {
					push_span(&mut spans, "\n".into(), &styles);
				}
			}
			Tag::Open { name, patch } => {
				if first_open.is_none() {
					first_open = Some(name.clone());
				}
				if name == "p" || heading_level(&name).is_some() {
					containers += 1;
				}
				scopes.push((name, styles.len()));
				if patch != Patch::None {
					styles.push(patch);
				}
			}
			Tag::Close { name } => {
				if let Some(i) =
					scopes.iter().rposition(|(open, _)| *open == name)
				{
					styles.truncate(scopes[i].1);
					scopes.truncate(i);
				}
			}
		}
	}
	let text = normalize(spans);
	if text.is_empty() {
		return if rule { Block::Rule } else { Block::Empty };
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
			reading: None,
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

/// Read one attribute value; `class`, `style` and the rest are simply ignored.
fn attribute(attrs: &str, name: &str) -> Option<String> {
	let mut rest = attrs;
	while !rest.is_empty() {
		rest = rest.trim_start().trim_start_matches('/');
		let end = rest
			.find(|c: char| c.is_whitespace() || c == '=')
			.unwrap_or(rest.len());
		if end == 0 {
			rest = &rest[1..];
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
	None
}
fn tokenize(source: &str) -> Vec<Token> {
	let mut tokens = Vec::new();
	let mut i = 0;
	while i < source.len() {
		let rest = &source[i..];
		if let Some(comment) = rest.strip_prefix("<!--") {
			i += match comment.find("-->") {
				Some(end) => 4 + end + 3,
				None => rest.len(),
			};
			tokens.push(Token::Comment);
			continue;
		}
		let Some(open) = rest.find('<') else {
			tokens.push(Token::Text(rest.to_string()));
			break;
		};
		if open > 0 {
			tokens.push(Token::Text(rest[..open].to_string()));
		}
		let candidate = &rest[open..];
		match tag_len(candidate) {
			Some(len) => {
				tokens.push(Token::Tag(candidate[..len].to_string()));
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

/// Byte length of a `<...>` candidate, honoring quoted attribute values.
fn tag_len(source: &str) -> Option<usize> {
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

fn push_text(spans: &mut Vec<Span>, text: &str, styles: &[Patch]) {
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
	push_span(spans, collapsed, styles);
}

fn push_span(spans: &mut Vec<Span>, text: String, styles: &[Patch]) {
	if text.is_empty() {
		return;
	}
	if let Some(last) = spans.last_mut()
		&& last.image.is_none()
		&& last.styles == styles
	{
		last.text.push_str(&text);
		return;
	}
	spans.push(Span {
		image: None,
		text,
		styles: styles.to_vec(),
	});
}

/// Collapse runs of whitespace and merge runs that share a style.
fn normalize(spans: Vec<Span>) -> Vec<Span> {
	let mut out: Vec<Span> = Vec::new();
	for span in spans {
		if span.image.is_some() {
			out.push(span);
			continue;
		}
		let text = if out.is_empty() {
			span.text.trim_start()
		} else {
			span.text.as_str()
		};
		if text.is_empty() {
			continue;
		}
		let mut merged = false;
		if let Some(last) = out.last_mut()
			&& last.image.is_none()
			&& last.styles == span.styles
		{
			last.text.push_str(text);
			merged = true;
		}
		if !merged {
			out.push(Span {
				image: None,
				text: text.to_string(),
				styles: span.styles,
			});
		}
	}
	let keep = out
		.iter()
		.enumerate()
		.rev()
		.find(|(_, span)| span.image.is_some() || !span.text.trim().is_empty())
		.map_or(0, |(i, _)| i + 1);
	out.truncate(keep);
	if let Some(last) = out.last_mut() {
		let len = last.text.trim_end().len();
		last.text.truncate(len);
	}
	out
}

#[cfg(test)]
mod tests;
