//! The floating hover preview card: a wrapped title, a detail line and an
//! optional body the renderer fills with a page scene.
use crate::layout::{Draw, Paint, Rect, TextShaper};
use markview_layout::style::{ColorField as C, Condition, TextAppearance};
use unicode_segmentation::UnicodeSegmentation;

/// Card width, padding included. The window's minimum width always keeps it
/// clear of an edge, so the title wraps against one measure.
const WIDTH: f32 = 288.0;
const TITLE_SIZE: f32 = 13.0;
const DETAIL_SIZE: f32 = 11.0;
const PADDING: f32 = 10.0;
const TITLE_LINE: f32 = 17.0;
const TITLE_GAP: f32 = 2.0;
const DETAIL_LINE: f32 = 14.0;
const BODY_GAP: f32 = 8.0;
/// Body height over width.
pub(in crate::app) const BODY_RATIO: f32 = 0.625;
/// Space between the card and its anchor, and the window edges.
const TOP_GAP: f32 = 4.0;
const MARGIN: f32 = 8.0;
const CONTENT_WIDTH: f32 = WIDTH - 2.0 * PADDING;

/// What one card shows. A target only decides the text, whether it has a body,
/// and what the renderer composes into that body.
pub(in crate::app) struct Card {
	pub(in crate::app) title: Vec<String>,
	pub(in crate::app) detail: String,
	/// Whether the card reserves a body rectangle below the text.
	pub(in crate::app) body: bool,
}

/// A card's rectangles. Geometry and painting share one value, so a body is
/// never reserved somewhere the card does not paint it.
pub(super) struct Layout {
	pub rect: Rect,
	title: Rect,
	detail: Rect,
	/// The rule between the path line and the page body.
	separator: Option<Rect>,
	pub(super) body: Option<Rect>,
}

/// The title, wrapped to at most two lines and ellipsized past them.
pub(in crate::app) fn title_lines(
	ui: &mut TextShaper,
	name: &str,
) -> Vec<String> {
	let old = ui.appearance.clone();
	ui.appearance = title_appearance(ui);
	let mut lines = super::controls::wrap(ui, name, TITLE_SIZE, CONTENT_WIDTH);
	if lines.len() > 2 {
		let rest = lines.split_off(1);
		let separator = if name.contains(char::is_whitespace) {
			" "
		} else {
			""
		};
		let rest = rest.join(separator);
		lines.push(clamp_line(ui, &rest, TITLE_SIZE, CONTENT_WIDTH));
	}
	if lines.is_empty() {
		lines.push(String::new());
	}
	ui.appearance = old;
	lines
}

/// The panel's text style, which the whole card paints on.
fn panel_appearance(ui: &TextShaper) -> TextAppearance {
	ui.stylesheet.text(
		&ui.stylesheet
			.text(&TextAppearance::default(), Condition::Ui),
		Condition::Panel,
	)
}

/// The card's bold title, resolved from the same panel style.
fn title_appearance(ui: &TextShaper) -> TextAppearance {
	let mut appearance = panel_appearance(ui);
	appearance.weight = 650;
	appearance
}

/// Shorten `text` to `width`, marking the cut with an ellipsis at its end.
fn clamp_line(
	ui: &mut TextShaper,
	text: &str,
	size: f32,
	width: f32,
) -> String {
	if ui.text_width(text, size) <= width {
		return text.to_owned();
	}
	let chars: Vec<&str> = text.graphemes(true).collect();
	let (mut lo, mut hi) = (0, chars.len());
	while lo < hi {
		let mid = (lo + hi).div_ceil(2);
		let candidate = format!("{}…", chars[..mid].concat());
		if ui.text_width(&candidate, size) <= width {
			lo = mid;
		} else {
			hi = mid - 1;
		}
	}
	format!("{}…", chars[..lo].concat())
}

/// The detail on its one line, keeping its start and end.
pub(in crate::app) fn detail_line(ui: &mut TextShaper, detail: &str) -> String {
	let old = ui.appearance.clone();
	ui.appearance = panel_appearance(ui);
	let line = ui.fit(detail, DETAIL_SIZE, CONTENT_WIDTH);
	ui.appearance = old;
	line
}

pub(super) fn layout(card: &Card, anchor: Rect, window: (f32, f32)) -> Layout {
	let title = card.title.len() as f32 * TITLE_LINE;
	let body = card.body.then_some(CONTENT_WIDTH * BODY_RATIO);
	let gap = if body.is_some() { BODY_GAP } else { 0.0 };
	let height = PADDING * 2.0
		+ title
		+ TITLE_GAP
		+ DETAIL_LINE
		+ gap
		+ body.unwrap_or(0.0);
	let max_x = (window.0 - WIDTH - MARGIN).max(MARGIN);
	let x = (anchor.x + anchor.w / 2.0 - WIDTH / 2.0).clamp(MARGIN, max_x);
	let top = anchor.y + anchor.h + TOP_GAP;
	let y = top.min((window.1 - height - MARGIN).max(0.0));
	let rect = Rect {
		x,
		y,
		w: WIDTH,
		h: height,
	};
	let title = Rect {
		x: x + PADDING,
		y: y + PADDING,
		w: CONTENT_WIDTH,
		h: title,
	};
	let detail = Rect {
		x: title.x,
		y: title.y + title.h + TITLE_GAP,
		w: CONTENT_WIDTH,
		h: DETAIL_LINE,
	};
	let separator = body.map(|_| Rect {
		x: title.x,
		y: detail.y + DETAIL_LINE + (BODY_GAP - 1.0) / 2.0,
		w: CONTENT_WIDTH,
		h: 1.0,
	});
	let body = body.map(|height| Rect {
		x: title.x,
		y: detail.y + DETAIL_LINE + BODY_GAP,
		w: CONTENT_WIDTH,
		h: height,
	});
	Layout {
		rect,
		title,
		detail,
		separator,
		body,
	}
}

/// Keeps the reading start near a stable point on the reference.
/// Lost reading height costs space; shorter vertical candidates remain available
/// when no side can fit the full scene.
pub(in crate::app) fn place(
	anchor: Rect,
	size: (f32, f32),
	bounds: Rect,
	padding: [f32; 4],
) -> Rect {
	let margin = 8.0_f32.min(bounds.w / 2.0).min(bounds.h / 2.0);
	let area = Rect {
		x: bounds.x + margin,
		y: bounds.y + margin,
		w: (bounds.w - 2.0 * margin).max(1.0),
		h: (bounds.h - 2.0 * margin).max(1.0),
	};
	let (w, h) = (size.0.min(area.w), size.1.min(area.h));
	let origin = (anchor.x, anchor.y + anchor.h / 2.0);
	let cx = origin.0 - padding[3];
	let cy = origin.1 - padding[0];
	let below = anchor.y + anchor.h + 6.0;
	let below_h = h.min((area.y + area.h - below).max(1.0));
	let above_h = h.min((anchor.y - 6.0 - area.y).max(1.0));
	let candidates = [
		(cx, below, below_h),
		(cx, anchor.y - above_h - 6.0, above_h),
		(anchor.x + anchor.w + 6.0, cy, h),
		(anchor.x - w - 6.0, cy, h),
	];
	candidates
		.into_iter()
		.map(|(x, y, height)| {
			let rect = Rect {
				x: x.clamp(area.x, area.x + (area.w - w)),
				y: y.clamp(area.y, area.y + (area.h - height)),
				w,
				h: height,
			};
			let overlap = rect.intersect(anchor).map_or(0.0, |r| r.w * r.h);
			let score = overlap * 1000.0
				+ (rect.x + padding[3] - origin.0)
					.hypot(rect.y + padding[0] - origin.1)
				+ (h - height) * 2.0;
			(rect, score)
		})
		.min_by(|a, b| a.1.total_cmp(&b.1))
		.unwrap()
		.0
}

/// The shared themed surface behind any floating scene.
pub(in crate::app) fn surface(ui: &TextShaper, rect: Rect) -> Vec<Draw> {
	surface_for(ui, rect, Condition::Preview)
}

fn surface_for(ui: &TextShaper, rect: Rect, condition: Condition) -> Vec<Draw> {
	let chain = condition.chain();
	let style = ui.stylesheet.element_rule(chain, condition);
	vec![Draw::Box {
		rect,
		chain,
		condition,
		radius: style.radius.unwrap_or(0.0),
		border: style.border_width.unwrap_or(1.0),
		left_only: false,
		decoration: markview_layout::scene::BoxDecoration::from_rule(
			&style, false,
		),
	}]
}

pub(super) fn draw(
	ui: &mut TextShaper,
	card: &Card,
	layout: &Layout,
) -> Vec<Draw> {
	let mut out = surface_for(ui, layout.rect, Condition::TabPreview);
	let appearance = title_appearance(ui);
	for (index, line) in card.title.iter().enumerate() {
		let baseline = layout.title.y + TITLE_SIZE + index as f32 * TITLE_LINE;
		out.extend(
			ui.label_with(
				line,
				TITLE_SIZE,
				layout.title.x,
				baseline,
				&appearance,
				Paint::Styled(Condition::Panel, C::Color),
				None,
			)
			.0,
		);
	}
	out.extend(ui.label(
		&card.detail,
		DETAIL_SIZE,
		layout.detail.x,
		layout.detail.y + DETAIL_SIZE,
		Paint::Styled(Condition::Panel, C::Muted),
	));
	if let Some(separator) = layout.separator {
		out.push(Draw::Rect(
			separator,
			Paint::Styled(Condition::Panel, C::BorderColor),
		));
	}
	out
}

#[cfg(test)]
mod tests;
