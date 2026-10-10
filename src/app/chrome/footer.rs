use super::super::BOTTOM;
use crate::lang::Lang;
use crate::layout::{Draw, Paint, Rect, TextShaper};
use markview_core::style::{ColorField as C, Condition, TextAppearance};

/// The text style the footer measures and draws its labels with.
fn appearance(shaper: &TextShaper) -> TextAppearance {
	shaper.stylesheet.text(
		&shaper
			.stylesheet
			.text(&TextAppearance::default(), Condition::Ui),
		Condition::Statusbar,
	)
}

/// Fits `text` to `max` and measures it with the footer's own style, so a label
/// and its hit target are shaped alike whatever the caller left the shaper with.
fn measure(shaper: &mut TextShaper, text: &str, max: f32) -> (String, f32) {
	let style = appearance(shaper);
	let old = std::mem::replace(&mut shaper.appearance, style);
	let text = shaper.fit(text, 11.0, max);
	let used = shaper.text_width(&text, 11.0);
	shaper.appearance = old;
	(text, used)
}

/// The label that leads the footer — the document's own counts, plus the
/// selection's — fitted to the bar, with the width it occupies.
pub(super) fn counts_label(
	shaper: &mut TextShaper,
	counts: Option<markview_core::text::TextCounts>,
	selected: Option<markview_core::text::TextCounts>,
	width: f32,
	lang: Lang,
) -> (String, f32) {
	let mut text = counts.map_or_else(
		|| lang.footer_loading().to_owned(),
		|counts| lang.footer_counts(counts.chars, counts.words),
	);
	if let Some(selected) = selected {
		text.push_str(&lang.footer_selected(selected.chars, selected.words));
	}
	measure(shaper, &text, width - 32.0)
}

/// The label that trails the counts, trimmed to the width left beside them,
/// with the rectangle it occupies — or `None` when the bar has no room for it.
///
/// Drawing and hit testing both read this, so a press lands on the label the
/// footer drew.
pub(super) fn trailing(
	shaper: &mut TextShaper,
	text: &str,
	used: f32,
	size: (f32, f32),
) -> Option<(String, Rect)> {
	let (width, height) = size;
	let available = width - used - 56.0;
	if text.is_empty() || available < 80.0 {
		return None;
	}
	let (text, drawn) = measure(shaper, text, available);
	Some((
		text,
		Rect {
			x: (width - 16.0 - drawn).max(0.0),
			y: height - BOTTOM,
			w: drawn,
			h: BOTTOM,
		},
	))
}

pub(super) fn draw_footer(
	shaper: &mut TextShaper,
	counts: Option<markview_core::text::TextCounts>,
	selected: Option<markview_core::text::TextCounts>,
	warning: Option<&str>,
	secondary: &str,
	size: (f32, f32),
	lang: Lang,
) -> Vec<Draw> {
	let (width, height) = size;
	shaper.appearance = appearance(shaper);
	let mut out = vec![
		Draw::Rect(
			Rect {
				x: 0.0,
				y: height - BOTTOM,
				w: width,
				h: BOTTOM,
			},
			Paint::Styled(Condition::Statusbar, C::Background),
		),
		Draw::Rect(
			Rect {
				x: 0.0,
				y: height - BOTTOM,
				w: width,
				h: 1.0,
			},
			Paint::Styled(Condition::Statusbar, C::BorderColor),
		),
	];
	let (text, used) = counts_label(shaper, counts, selected, width, lang);
	out.extend(shaper.label(
		&text,
		11.0,
		16.0,
		height - 9.0,
		Paint::Styled(Condition::Statusbar, C::Muted),
	));
	if let Some((secondary, rect)) = trailing(shaper, secondary, used, size) {
		out.extend(shaper.label(
			&secondary,
			11.0,
			rect.x,
			height - 9.0,
			Paint::Styled(Condition::Statusbar, C::Muted),
		));
	}
	if let Some(warning) = warning {
		out.push(Draw::Rect(
			Rect {
				x: 0.0,
				y: height - BOTTOM - 24.0,
				w: width,
				h: 24.0,
			},
			Paint::Styled(Condition::Statusbar, C::Background),
		));
		let warning = shaper.fit(warning, 11.0, width - 32.0);
		out.extend(shaper.label(
			&warning,
			11.0,
			16.0,
			height - BOTTOM - 8.0,
			Paint::Styled(Condition::Statusbar, C::Error),
		));
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	/// A path too long for the band keeps its start and its end, which is what
	/// leaves the file name readable once the middle is dropped.
	#[test]
	fn a_path_too_long_for_the_band_keeps_its_start_and_end() {
		let path = "/Users/reader/Documents/markview/assets/a-long-name.md";
		let mut ui = crate::test_support::shaper();
		let size = (320.0, 700.0);
		let (text, rect) = trailing(&mut ui, path, 60.0, size).unwrap();
		assert!(text.contains('…'), "{text}");
		assert!(text.starts_with("/Users"), "{text}");
		assert!(text.ends_with("name.md"), "{text}");
		assert!(text.len() < path.len());
		// The label stands against the right inset, in the footer's own band.
		assert_eq!(rect.x + rect.w, size.0 - 16.0);
		assert_eq!((rect.y, rect.h), (size.1 - BOTTOM, BOTTOM));
	}

	/// A band with nothing to name, or no room left beside the counts, draws
	/// nothing at all.
	#[test]
	fn an_empty_or_full_band_names_nothing() {
		let mut ui = crate::test_support::shaper();
		let size = (320.0, 700.0);
		assert!(trailing(&mut ui, "", 60.0, size).is_none());
		assert!(trailing(&mut ui, "/a.md", 200.0, size).is_none());
	}
}
