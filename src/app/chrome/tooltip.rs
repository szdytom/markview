//! Compact hints inherit the reader's panel palette and UI typography.
use crate::layout::{Draw, Paint, Rect, TextShaper};
use markview_layout::style::{ColorField as C, Condition, TextAppearance};

const SIZE: f32 = 12.;
const LINE: f32 = 17.;
const PAD: f32 = 8.;
const MARGIN: f32 = 8.;
const GAP: f32 = 6.;

/// Align the text start with the control's horizontal center, preferring below it.
fn place(anchor: Rect, size: (f32, f32), window: (f32, f32)) -> Rect {
	let margin = MARGIN.min(window.0 / 2.).min(window.1 / 2.);
	let area = (window.0 - 2. * margin, window.1 - 2. * margin);
	let w = size.0.min(area.0);
	let h = size.1.min(area.1);
	let below = anchor.y + anchor.h + GAP;
	let y = if below + h <= window.1 - margin {
		below
	} else {
		anchor.y - GAP - h
	};
	Rect {
		x: margin
			+ (anchor.x + anchor.w / 2. - PAD - margin).clamp(0., area.0 - w),
		y: margin + (y - margin).clamp(0., area.1 - h),
		w,
		h,
	}
}

pub(in crate::app) fn draw(
	ui: &mut TextShaper,
	text: &str,
	anchor: Rect,
	window: (f32, f32),
) -> Vec<Draw> {
	let old = ui.appearance.clone();
	ui.appearance = ui.stylesheet.text(
		&ui.stylesheet
			.text(&TextAppearance::default(), Condition::Ui),
		Condition::Panel,
	);
	let width = 280_f32.min((window.0 - 2. * (MARGIN + PAD)).max(0.));
	let lines: Vec<_> = text
		.lines()
		.flat_map(|line| super::controls::wrap(ui, line, SIZE, width))
		.collect();
	let w = lines
		.iter()
		.map(|line| ui.text_width(line, SIZE))
		.fold(0_f32, f32::max);
	let rect = place(
		anchor,
		(w + PAD * 2., lines.len() as f32 * LINE + PAD * 2.),
		window,
	);
	let mut draws = vec![Draw::Box {
		rect,
		chain: Condition::Panel.chain(),
		condition: Condition::Panel,
		radius: 0.,
		border: 1.,
		left_only: false,
		decoration: None,
	}];
	for (index, line) in lines.iter().enumerate() {
		draws.extend(ui.centered_label(
			line,
			SIZE,
			rect.x + PAD,
			rect.y + PAD + LINE * (index as f32 + 0.5),
			Paint::Styled(Condition::Panel, C::Color),
		));
	}
	ui.appearance = old;
	vec![Draw::Clipped { rect, draws }]
}

#[cfg(test)]
mod tests;
