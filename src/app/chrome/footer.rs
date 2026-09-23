use super::super::BOTTOM;
use crate::lang::Lang;
use crate::layout::{Draw, Paint, Rect, TextShaper};
use markview_core::style::{ColorField as C, Condition, TextAppearance};
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
	shaper.appearance = shaper.stylesheet.text(
		&shaper
			.stylesheet
			.text(&TextAppearance::default(), Condition::Ui),
		Condition::Statusbar,
	);
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
	let mut text = counts.map_or_else(
		|| lang.footer_loading().to_owned(),
		|counts| lang.footer_counts(counts.chars, counts.words),
	);
	if let Some(selected) = selected {
		text.push_str(&lang.footer_selected(selected.chars, selected.words));
	}
	let text = shaper.fit(&text, 11.0, width - 32.0);
	let used = shaper.text_width(&text, 11.0);
	out.extend(shaper.label(
		&text,
		11.0,
		16.0,
		height - 9.0,
		Paint::Styled(Condition::Statusbar, C::Muted),
	));
	let available = width - used - 56.0;
	if !secondary.is_empty() && available >= 80.0 {
		out.extend(shaper.right_label(
			secondary,
			11.0,
			available,
			width - 16.0,
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
