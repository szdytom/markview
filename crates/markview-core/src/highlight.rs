use crate::style::Color;
use std::{ops::Range, sync::OnceLock};
use syntect::{
	easy::HighlightLines, highlighting::ThemeSet, parsing::SyntaxSet,
};

fn syntax_set() -> &'static SyntaxSet {
	static SET: OnceLock<SyntaxSet> = OnceLock::new();
	SET.get_or_init(two_face::syntax::extra_newlines)
}

fn theme_set() -> &'static ThemeSet {
	static SET: OnceLock<ThemeSet> = OnceLock::new();
	SET.get_or_init(|| ThemeSet::from(&two_face::theme::extra()))
}

pub(crate) struct Highlighter {
	inner: Option<HighlightLines<'static>>,
}

pub(crate) type HighlightedLines = Vec<Vec<(Range<usize>, Option<Color>)>>;

impl Highlighter {
	pub(crate) fn new(language: &str, theme: Option<&str>) -> Self {
		if theme == Some("none") {
			return Self { inner: None };
		}
		let language = language.split(',').next().unwrap_or(language).trim();
		let syntax = syntax_set()
			.find_syntax_by_token(language)
			.or_else(|| syntax_set().find_syntax_by_name(language));
		let theme = theme
			.and_then(|name| theme_set().themes.get(name))
			.or_else(|| theme_set().themes.get("InspiredGitHub"));
		Self {
			inner: syntax
				.zip(theme)
				.map(|(syntax, theme)| HighlightLines::new(syntax, theme)),
		}
	}

	pub(crate) fn highlight(
		&mut self,
		line: &str,
		max_bytes: usize,
	) -> Vec<(Range<usize>, Option<Color>)> {
		// Regex backtracking is the risk here, and it grows with the input, so
		// an over-long line keeps its text and loses only its colors.
		if line.len() > max_bytes {
			return vec![(0..line.len(), None)];
		}
		let Some(highlighter) = &mut self.inner else {
			return vec![(0..line.len(), None)];
		};
		// A line comment's scope pops at the line terminator, so syntect must
		// see one: without it, the comment swallows every following line.
		let terminated = line.ends_with('\n');
		let buffer;
		let line = if terminated {
			line
		} else {
			buffer = format!("{line}\n");
			&buffer
		};
		let Ok(regions) = highlighter.highlight_line(line, syntax_set()) else {
			return vec![(0..line.len(), None)];
		};
		let end = line.len() - usize::from(!terminated);
		regions
			.into_iter()
			.scan(0, |offset, (style, text)| {
				let start = *offset;
				*offset += text.len();
				Some((
					start..*offset,
					Some(Color(
						(u32::from(style.foreground.r) << 24)
							| (u32::from(style.foreground.g) << 16)
							| (u32::from(style.foreground.b) << 8)
							| u32::from(style.foreground.a),
					)),
				))
			})
			.take_while(|(range, _)| range.start < end)
			.map(|(range, color)| (range.start..range.end.min(end), color))
			.collect()
	}
}

pub(crate) fn highlight_block(
	language: &str,
	theme: Option<&str>,
	lines: impl Iterator<Item = String>,
	max_line_bytes: usize,
) -> HighlightedLines {
	let mut highlighter = Highlighter::new(language, theme);
	lines
		.map(|line| highlighter.highlight(&line, max_line_bytes))
		.collect()
}

#[cfg(test)]
mod tests {
	#[test]
	fn disabling_syntax_colors_preserves_code_text() {
		let line = "let ink = 42; // black and white";
		let mut highlighter = super::Highlighter::new("rust", Some("none"));
		assert_eq!(
			highlighter.highlight(line, 1024),
			vec![(0..line.len(), None)]
		);
		assert!(
			super::Highlighter::new("rust", None)
				.highlight(line, 1024)
				.iter()
				.any(|(_, color)| color.is_some())
		);
	}
}
