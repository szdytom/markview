//! Page furniture: the header, footer and page number drawn in the margins.
//!
//! A slot is a template. `{page}` and `{pages}` are set in the `page_number`
//! style and everything else in `page_header` or `page_footer`, so a page
//! number can be quieter than the text beside it. Distances are PDF points;
//! page furniture is never part of the reading layout.
use super::PageGeometry;
use crate::{
	layout::{Draw, Paint},
	shaping::TextShaper,
	style::{ColorField, Condition, Stylesheet, TextAppearance},
};
use std::ops::Range;

/// The placeholders a slot may use.
const PLACEHOLDERS: &[&str] = &["page", "pages", "title", "path"];

/// One run of a slot: literal text, or a page number.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SlotSegment {
	Text(String),
	Number(String),
}

/// Whether every placeholder in `text` is one this module knows. A stylesheet
/// that misspells one is rejected rather than quietly printing the braces.
pub fn template_is_valid(text: &str) -> bool {
	let mut rest = text;
	while let Some(start) = rest.find('{') {
		let after = &rest[start + 1..];
		let Some(end) = after.find('}') else {
			return false;
		};
		if !PLACEHOLDERS.contains(&&after[..end]) {
			return false;
		}
		rest = &after[end + 1..];
	}
	true
}

/// Splits a slot template into literal and page-number runs.
pub fn expand_template(
	text: &str,
	page: usize,
	pages: usize,
	title: &str,
	path: &str,
) -> Vec<SlotSegment> {
	let mut out: Vec<SlotSegment> = Vec::new();
	let mut push = |segment: SlotSegment| match (out.last_mut(), &segment) {
		// Adjacent literals are one run, so they are measured once.
		(Some(SlotSegment::Text(previous)), SlotSegment::Text(next)) => {
			previous.push_str(next)
		}
		_ => out.push(segment),
	};
	let mut rest = text;
	loop {
		let Some(start) = rest.find('{') else {
			push(SlotSegment::Text(rest.into()));
			break;
		};
		let after = &rest[start + 1..];
		let Some(end) = after.find('}') else {
			push(SlotSegment::Text(rest.into()));
			break;
		};
		push(SlotSegment::Text(rest[..start].into()));
		let value = match &after[..end] {
			"page" => SlotSegment::Number(page.to_string()),
			"pages" => SlotSegment::Number(pages.to_string()),
			"title" => SlotSegment::Text(title.into()),
			"path" => SlotSegment::Text(path.into()),
			other => SlotSegment::Text(format!("{{{other}}}")),
		};
		push(value);
		rest = &after[end + 1..];
	}
	out.retain(
		|segment| !matches!(segment, SlotSegment::Text(text) if text.is_empty()),
	);
	out
}

fn condition(header: bool) -> Condition {
	if header {
		Condition::PageHeader
	} else {
		Condition::PageFooter
	}
}

fn segment_condition(header: bool, segment: &SlotSegment) -> Condition {
	match segment {
		SlotSegment::Number(_) => Condition::PageNumber,
		SlotSegment::Text(_) => condition(header),
	}
}

/// One laid-out piece of page furniture: the reading text it shows, the byte
/// range behind each glyph draw in order, and the draws themselves.
pub struct Furniture {
	pub text: String,
	pub ranges: Vec<Range<usize>>,
	pub draws: Vec<Draw>,
}

/// What page furniture may name about the document it belongs to.
pub struct FurnitureText<'a> {
	pub title: &'a str,
	pub path: &'a str,
}

/// The pieces of one page's furniture, in page points.
///
/// `base_pt` is the body text size in points, which the stylesheet's size
/// multipliers are relative to. The caller's shaper must already carry
/// `sheet`; only the appearance changes here.
pub fn page_furniture(
	sheet: &Stylesheet,
	shaper: &mut TextShaper,
	geometry: &PageGeometry,
	base_pt: f32,
	page: usize,
	pages: usize,
	text: &FurnitureText<'_>,
) -> Vec<Furniture> {
	let [left, top, width, _] = geometry.text_pt();
	let [_, _, bottom, _] = geometry.margin_pt;
	let mut out = Vec::new();
	for header in [true, false] {
		for (slot, template) in sheet.page().slots(header).iter().enumerate() {
			if template.trim().is_empty() {
				continue;
			}
			let segments =
				expand_template(template, page, pages, text.title, text.path);
			let mut runs = Vec::new();
			let mut total = 0.0_f32;
			let mut size = 0.0_f32;
			for segment in &segments {
				let condition = segment_condition(header, segment);
				let text = match segment {
					SlotSegment::Text(text) | SlotSegment::Number(text) => {
						text.as_str()
					}
				};
				let appearance =
					sheet.text(&TextAppearance::default(), condition);
				// Labels take the base size and let the appearance scale it.
				let visual = base_pt * appearance.size;
				shaper.appearance = appearance;
				let run_width = shaper.text_width(text, base_pt);
				runs.push((condition, text.to_owned(), run_width));
				total += run_width;
				size = size.max(visual);
			}
			let x = match slot {
				0 => left,
				1 => left + (width - total) * 0.5,
				_ => left + width - total,
			};
			// Centred in the margin band, so a reader's eye skips it.
			let baseline = if header {
				top * 0.5 + size * 0.35
			} else {
				geometry.height_pt - bottom * 0.5 + size * 0.35
			};
			let mut cursor = x;
			for (condition, text, run_width) in runs {
				let appearance =
					sheet.text(&TextAppearance::default(), condition);
				let paint = Paint::Styled(condition, ColorField::Color);
				let (draws, ranges, _) = shaper.label_runs(
					&text,
					base_pt,
					cursor,
					baseline,
					&appearance,
					paint,
					None,
				);
				out.push(Furniture {
					text,
					ranges,
					draws,
				});
				cursor += run_width;
			}
		}
	}
	out
}
