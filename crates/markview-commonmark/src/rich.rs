//! Semantic rich text plus the source interval each run came from.
//!
//! The finished [`crate::RichText`] carries no source positions; the parser
//! keeps them alongside until it can hand a [`crate::SourceMap`] to the
//! consumers that need source navigation.
use crate::document::{
	Inline, InlineKind, Link, RichText, SourceSpan, TextStyle,
};
use std::ops::Range;

/// Rich text under construction, one source range per inline.
#[derive(Clone, Debug, Default)]
pub(crate) struct SpannedText {
	pub(crate) inlines: RichText,
	pub(crate) sources: Vec<Range<usize>>,
}

impl SpannedText {
	pub(crate) fn new() -> Self {
		Self::default()
	}

	pub(crate) fn is_empty(&self) -> bool {
		self.inlines.is_empty()
	}

	pub(crate) fn push(
		&mut self,
		kind: InlineKind,
		style: RunStyle,
		source: Range<usize>,
	) {
		self.inlines.push(Inline {
			kind,
			style: style.visual,
			link: style.link,
		});
		self.sources.push(source);
	}

	pub(crate) fn last_mut(
		&mut self,
	) -> Option<(&mut Inline, &mut Range<usize>)> {
		let index = self.inlines.len().checked_sub(1)?;
		Some((&mut self.inlines[index], &mut self.sources[index]))
	}

	pub(crate) fn plain(text: impl Into<String>, source: Range<usize>) -> Self {
		let mut out = Self::new();
		out.push(InlineKind::Text(text.into()), RunStyle::default(), source);
		out
	}

	pub(crate) fn from_html_spans(
		spans: Vec<crate::html::Span>,
		source: &Range<usize>,
	) -> Self {
		let mut out = Self::new();
		for span in spans {
			let kind = if let Some(anchor) = span.anchor {
				InlineKind::Anchor(anchor)
			} else {
				span.image.map_or_else(
					|| InlineKind::Text(span.text),
					InlineKind::Image,
				)
			};
			out.push(
				kind,
				RunStyle {
					visual: TextStyle {
						bold: span.style.bold,
						italic: span.style.italic,
						strike: span.style.strike,
						code: span.style.code,
						superscript: span.style.superscript,
						subscript: span.style.subscript,
						mark: span.style.mark,
						..TextStyle::default()
					},
					link: span.style.link,
				},
				source.clone(),
			);
		}
		out
	}

	/// Merges neighboring runs that share a style, keeping their sources.
	pub(crate) fn merged(self) -> Self {
		let mut out = SpannedText::new();
		let mut previous_text: Option<usize> = None;
		for (mut span, source) in self.inlines.into_iter().zip(self.sources) {
			let InlineKind::Text(t) = &mut span.kind else {
				if !matches!(span.kind, InlineKind::Anchor(_)) {
					previous_text = None;
				}
				out.push_inline(span, source);
				continue;
			};
			if let Some(index) = previous_text
				&& index + 1 < out.inlines.len()
				&& mergeable_style(
					&out.inlines[index].style,
					&out.inlines[index].link,
					&span.style,
					&span.link,
				) && let InlineKind::Text(prev) = &out.inlines[index].kind
				&& prev.ends_with(char::is_whitespace)
				&& t.starts_with(char::is_whitespace)
			{
				let leading = t.len() - t.trim_start().len();
				t.drain(..leading);
				if t.is_empty() {
					continue;
				}
			}
			let mut merged = false;
			if let Some(last) = out.inlines.last_mut()
				&& mergeable_style(
					&last.style,
					&last.link,
					&span.style,
					&span.link,
				) && let InlineKind::Text(prev) = &mut last.kind
			{
				append_merged_text(prev, t);
				let last = out.sources.last_mut().expect("parallel source");
				last.end = last.end.max(source.end);
				merged = true;
			}
			if !merged {
				out.push_inline(span, source);
			}
			previous_text = Some(out.inlines.len() - 1);
		}
		out
	}

	pub(crate) fn push_inline(&mut self, inline: Inline, source: Range<usize>) {
		self.inlines.push(inline);
		self.sources.push(source);
	}

	pub(crate) fn trimmed(mut self) -> Self {
		fn trim_edge(text: &mut SpannedText, end: bool) {
			let mut leading = true;
			let mut keep = vec![true; text.inlines.len()];
			for (index, inline) in text.inlines.iter().enumerate() {
				if !leading {
					break;
				}
				match &inline.kind {
					InlineKind::Anchor(_) => {}
					InlineKind::Text(t) if t.trim().is_empty() => {
						keep[index] = false
					}
					_ => leading = false,
				}
			}
			let mut index = 0;
			text.inlines.retain(|_| {
				let hold = keep[index];
				index += 1;
				hold
			});
			let mut index = 0;
			text.sources.retain(|_| {
				let hold = keep[index];
				index += 1;
				hold
			});
			let target =
				text.inlines.iter().position(|inline| match &inline.kind {
					InlineKind::Anchor(_) => false,
					InlineKind::Text(t) => !t.trim().is_empty(),
					_ => true,
				});
			if let Some(index) = target
				&& let InlineKind::Text(t) = &mut text.inlines[index].kind
			{
				if end {
					t.truncate(t.trim_end().len());
				} else {
					t.drain(..t.len() - t.trim_start().len());
				}
			}
		}
		trim_edge(&mut self, false);
		self.inlines.reverse();
		self.sources.reverse();
		trim_edge(&mut self, true);
		self.inlines.reverse();
		self.sources.reverse();
		self
	}
}

/// Text runs merge only when they carry one style and one link occurrence.
fn mergeable_style(
	left: &TextStyle,
	a: &Option<Link>,
	right: &TextStyle,
	b: &Option<Link>,
) -> bool {
	left == right
		&& match (a, b) {
			(Some(a), Some(b)) => a.same_occurrence(b),
			(None, None) => true,
			_ => false,
		}
}

pub(crate) fn append_merged_text(previous: &mut String, text: &str) {
	if previous.ends_with(char::is_whitespace)
		&& text.starts_with(char::is_whitespace)
	{
		let len = previous.trim_end().len();
		previous.truncate(len);
		previous.push(' ');
		previous.push_str(text.trim_start());
	} else {
		previous.push_str(text);
	}
}

impl SpannedText {
	/// Splits into finished inlines and field spans.
	pub(crate) fn into_parts(
		self,
		source_text: &str,
	) -> (RichText, Vec<SourceSpan>, Vec<SourceSpan>) {
		let spans = self.spans(source_text);
		let covers = self.covers();
		(self.inlines, spans, covers)
	}

	/// One covering interval per inline, preserving delimiters and escapes.
	pub(crate) fn covers(&self) -> Vec<SourceSpan> {
		let mut semantic = 0;
		self.inlines
			.iter()
			.zip(&self.sources)
			.map(|(inline, source)| {
				let len = crate::plain_text(std::slice::from_ref(inline)).len();
				let span = SourceSpan {
					semantic: semantic..semantic + len,
					source: source.clone(),
					exact: false,
				};
				semantic += len;
				span
			})
			.collect()
	}

	/// The field spans for this text, aligning transformed runs character by
	/// character so an entity or escape does not cover the whole run.
	pub(crate) fn spans(&self, source_text: &str) -> Vec<SourceSpan> {
		let mut spans = Vec::with_capacity(self.inlines.len());
		let mut semantic = 0;
		for (inline, source) in self.inlines.iter().zip(&self.sources) {
			let projection = crate::plain_text(std::slice::from_ref(inline));
			let start = semantic;
			semantic += projection.len();
			// An image reads as its alt text, which is not character-aligned
			// with `![alt](src)`: the whole inline is its source.
			if matches!(inline.kind, InlineKind::Image(_)) {
				spans.push(SourceSpan {
					semantic: start..semantic,
					source: source.clone(),
					exact: false,
				});
				continue;
			}
			let Some(raw) = source_text.get(source.clone()) else {
				spans.push(SourceSpan {
					semantic: start..semantic,
					source: source.clone(),
					exact: false,
				});
				continue;
			};
			if raw == projection {
				spans.push(SourceSpan {
					semantic: start..semantic,
					source: source.clone(),
					exact: true,
				});
				continue;
			}
			let mut cursor = 0;
			let mut missing = std::collections::HashSet::new();
			for (offset, ch) in projection.char_indices() {
				let mapped = if missing.contains(&ch) {
					None
				} else {
					match raw[cursor..].find(ch) {
						Some(found) => {
							let at = cursor + found;
							cursor = at + ch.len_utf8();
							Some(source.start + at..source.start + cursor)
						}
						None => {
							missing.insert(ch);
							None
						}
					}
				};
				let reading = start + offset..start + offset + ch.len_utf8();
				match (mapped, spans.last_mut()) {
					(Some(mapped), Some(previous))
						if previous.semantic.end == reading.start
							&& previous.source.end == mapped.start
							&& previous.semantic.len()
								== previous.source.len()
							&& reading.len() == mapped.len() =>
					{
						previous.semantic.end = reading.end;
						previous.source.end = mapped.end;
					}
					(Some(mapped), _) => spans.push(SourceSpan {
						semantic: reading,
						source: mapped,
						exact: true,
					}),
					(None, _) => spans.push(SourceSpan {
						semantic: reading,
						source: source.clone(),
						exact: false,
					}),
				}
			}
			if projection.is_empty() {
				spans.push(SourceSpan {
					semantic: start..semantic,
					source: source.clone(),
					exact: false,
				});
			}
		}
		spans
	}
}

/// Inherited producer state; navigation is separated when emitting an inline.
#[derive(Clone, Debug, Default)]
pub(crate) struct RunStyle {
	pub(crate) visual: TextStyle,
	pub(crate) link: Option<Link>,
}
impl std::ops::Deref for RunStyle {
	type Target = TextStyle;
	fn deref(&self) -> &TextStyle {
		&self.visual
	}
}
impl std::ops::DerefMut for RunStyle {
	fn deref_mut(&mut self) -> &mut TextStyle {
		&mut self.visual
	}
}

/// Source coordinates belong to the CommonMark producer.
pub(crate) fn source_occurrence(
	offset: usize,
) -> markview_document::LinkOccurrenceId {
	markview_document::LinkOccurrenceId::new(
		u32::try_from(offset.checked_add(1).unwrap()).unwrap(),
	)
	.unwrap()
}
pub(crate) fn source_link(
	url: impl Into<std::sync::Arc<str>>,
	offset: usize,
) -> Link {
	Link::new(url, source_occurrence(offset))
}
