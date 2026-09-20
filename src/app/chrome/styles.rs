use super::super::Button;
use super::components::CONTROL;
use super::controls::{draw_button, panel_rect};
use super::icons;
use crate::{
	layout::{Draw, Paint, Rect, TextShaper},
	state::{Command, InteractionState},
};
use markview_core::style::{ColorField as C, Condition, TextAppearance};

/// Which list a stylesheet page edits: the reader's effective styles, or the
/// sequence one export layers on the print sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StylesTarget {
	Reader,
	Export,
}

impl StylesTarget {
	fn toggle(self, index: usize) -> Command {
		match self {
			Self::Reader => Command::StyleToggle(index),
			Self::Export => Command::ExportStyleToggle(index),
		}
	}
	fn up(self, index: usize) -> Command {
		match self {
			Self::Reader => Command::StyleUp(index),
			Self::Export => Command::ExportStyleUp(index),
		}
	}
	fn down(self, index: usize) -> Command {
		match self {
			Self::Reader => Command::StyleDown(index),
			Self::Export => Command::ExportStyleDown(index),
		}
	}
	/// The page's Back button returns to the panel that opened it.
	fn back(self) -> Command {
		match self {
			Self::Reader => Command::Styles,
			Self::Export => Command::ExportStyles,
		}
	}
	fn prev(self) -> Command {
		match self {
			Self::Reader => Command::StylePrev,
			Self::Export => Command::ExportStylePrev,
		}
	}
	fn next(self) -> Command {
		match self {
			Self::Reader => Command::StyleNext,
			Self::Export => Command::ExportStyleNext,
		}
	}
	/// Only the reader can follow the system theme.
	fn system(self) -> Option<Command> {
		(self == Self::Reader).then_some(Command::SystemTheme)
	}
	fn summary(self, selected: Option<&[String]>) -> &'static str {
		match self {
			Self::Reader if selected.is_none() => {
				"Following the system appearance"
			}
			Self::Reader => "Enabled styles appear first, in priority order",
			Self::Export => "Applied to the exported document",
		}
	}
}

pub(in crate::app) fn styles_rect(
	width: f32,
	height: f32,
	count: usize,
) -> Rect {
	let mut rect = panel_rect(width, height);
	rect.h = rect.h.min(152.0 + count.max(1) as f32 * 60.0);
	rect.y = (height - rect.h) / 2.0;
	rect
}

fn style_rows(rect: Rect) -> usize {
	((rect.h - 142.) / 60.).floor().max(1.) as usize
}

fn style_order(
	selected: Option<&[String]>,
	entries: &[crate::stylesheet::Entry],
) -> Vec<usize> {
	let mut indices: Vec<_> = (0..entries.len()).collect();
	indices.sort_by_key(|i| {
		selected
			.and_then(|ids| ids.iter().position(|id| id == &entries[*i].id))
			.unwrap_or(usize::MAX)
	});
	indices
}

pub(super) fn style_controls(
	target: StylesTarget,
	selected: Option<&[String]>,
	entries: &[crate::stylesheet::Entry],
	fonts: &crate::fonts::Status,
	page: usize,
	width: f32,
	height: f32,
) -> Vec<Button> {
	let r = styles_rect(width, height, entries.len());
	let rows = style_rows(r);
	let order = style_order(selected, entries);
	let page = page.min(order.len().saturating_sub(1) / rows);
	let mut out = vec![];
	let mut headers = vec![
		("Back", Some(icons::BACK), target.back(), r.w - 96., CONTROL),
		(
			"Close",
			Some(icons::CLOSE),
			Command::Settings,
			r.w - 24. - CONTROL,
			CONTROL,
		),
		(
			"Open styles folder",
			None,
			Command::StylesFolder,
			108.,
			146.,
		),
	];
	if let Some(system) = target.system() {
		headers.push(("System", None, system, 24., 74.));
	}
	for (label, icon, action, x, w) in headers {
		out.push(Button {
			label,
			icon,
			active: action == Command::SystemTheme && selected.is_none(),
			kind: Default::default(),
			enabled: true,
			action,
			rect: Rect {
				x: r.x + x,
				y: if matches!(
					action,
					Command::StylesFolder | Command::SystemTheme
				) {
					r.y + r.h - 48.
				} else {
					r.y + 16.
				},
				w,
				h: 32.,
			},
		});
	}
	// The catalogued sheets declare the files, so the button appears whether
	// or not the sheet that names them is currently enabled. It is disabled
	// only while the one running job still holds it.
	if entries.iter().any(|entry| !entry.urls.is_empty()) {
		// The previous button shares this row on every page after the first,
		// so the download button must leave room for either one.
		let paginated = page > 0 || (page + 1) * rows < order.len();
		let limit = if paginated { r.w - 104. } else { r.w - 24. };
		let w = (limit - 262.0).clamp(0.0, 148.0);
		if w >= 96.0 {
			out.push(Button {
				label: "Download fonts",
				icon: None,
				active: false,
				kind: Default::default(),
				enabled: !fonts.running,
				action: Command::FontsDownload,
				rect: Rect {
					x: r.x + 262.,
					y: r.y + r.h - 48.,
					w,
					h: 32.,
				},
			});
		}
	}
	if page > 0 {
		out.push(Button {
			label: "←",
			icon: None,
			active: false,
			kind: Default::default(),
			enabled: true,
			action: target.prev(),
			rect: Rect {
				x: r.x + r.w - 96.,
				y: r.y + r.h - 48.,
				w: 32.,
				h: 32.,
			},
		});
	}
	if (page + 1) * rows < order.len() {
		out.push(Button {
			label: "→",
			icon: None,
			active: false,
			kind: Default::default(),
			enabled: true,
			action: target.next(),
			rect: Rect {
				x: r.x + r.w - 56.,
				y: r.y + r.h - 48.,
				w: 32.,
				h: 32.,
			},
		});
	}
	for (row, index) in
		order.into_iter().skip(page * rows).take(rows).enumerate()
	{
		let e = &entries[index];
		let pos =
			selected.and_then(|ids| ids.iter().position(|id| id == &e.id));
		let y = r.y + 84. + row as f32 * 60.;
		if e.error.is_none() || pos.is_some() {
			out.push(Button {
				label: if pos.is_some() { "Enabled" } else { "Enable" },
				icon: None,
				active: pos.is_some(),
				kind: Default::default(),
				enabled: true,
				action: target.toggle(index),
				rect: Rect {
					x: r.x + r.w - 180.,
					y,
					w: 76.,
					h: 32.,
				},
			});
		}
		if let Some(pos) = pos {
			if pos > 0 {
				out.push(Button {
					label: "↑",
					icon: None,
					active: false,
					kind: Default::default(),
					enabled: true,
					action: target.up(index),
					rect: Rect {
						x: r.x + r.w - 96.,
						y,
						w: 32.,
						h: 32.,
					},
				});
			}
			if selected.is_some_and(|ids| pos + 1 < ids.len()) {
				out.push(Button {
					label: "↓",
					icon: None,
					active: false,
					kind: Default::default(),
					enabled: true,
					action: target.down(index),
					rect: Rect {
						x: r.x + r.w - 58.,
						y,
						w: 32.,
						h: 32.,
					},
				});
			}
		}
	}
	out
}

/// The one line under the page title: the font download's state when it has
/// anything to say, otherwise the page's own guidance.
fn summary_text(
	fonts: &crate::fonts::Status,
	declared: usize,
	fallback: &str,
) -> (String, bool) {
	if let Some(note) = &fonts.note {
		return (note.clone(), note.starts_with("Offline"));
	}
	if fonts.running {
		let percent = (fonts.done * 100).checked_div(fonts.total).unwrap_or(0);
		let current = fonts.current.as_deref().unwrap_or("fonts");
		return (
			format!(
				"Downloading {current} — {}/{} files ({percent}%)",
				fonts.done, fonts.total
			),
			false,
		);
	}
	if let Some(failure) = fonts.failures.last() {
		let more = if fonts.failures.len() > 1 {
			format!("{} failed; ", fonts.failures.len())
		} else {
			String::new()
		};
		return (
			format!(
				"{}/{} font files downloaded; {more}{}: {}",
				fonts.done, fonts.total, failure.file, failure.reason
			),
			true,
		);
	}
	if fonts.done > 0 {
		return (
			format!("{}/{} font files downloaded", fonts.done, fonts.total),
			false,
		);
	}
	if declared > 0 {
		return (
			format!("{declared} font files are available to download"),
			false,
		);
	}
	(fallback.to_string(), false)
}

#[expect(clippy::too_many_arguments, reason = "one page's explicit inputs")]
pub(super) fn draw_styles(
	shaper: &mut TextShaper,
	target: StylesTarget,
	selected: Option<&[String]>,
	interaction: &InteractionState,
	entries: &[crate::stylesheet::Entry],
	fonts: &crate::fonts::Status,
	page: usize,
	width: f32,
	height: f32,
) -> Vec<Draw> {
	shaper.appearance = shaper.stylesheet.text(
		&shaper
			.stylesheet
			.text(&TextAppearance::default(), Condition::Ui),
		Condition::Panel,
	);
	let r = styles_rect(width, height, entries.len());
	let rows = style_rows(r);
	let order = style_order(selected, entries);
	let page = page.min(order.len().saturating_sub(1) / rows);
	let mut out = super::components::frame(r, width, height);
	let weight = shaper.appearance.weight;
	shaper.appearance.weight = 700;
	out.extend(shaper.label(
		"Stylesheets",
		20.0,
		r.x + 24.0,
		r.y + 36.0,
		Paint::Styled(Condition::Panel, C::Color),
	));
	shaper.appearance.weight = weight;
	for y in [r.y + 76.0, r.y + r.h - 64.0] {
		out.push(super::components::line(
			Rect {
				x: r.x + 1.0,
				y,
				w: r.w - 2.0,
				h: 1.0,
			},
			Condition::Panel,
			C::BorderColor,
		));
	}

	let declared = entries
		.iter()
		.flat_map(|entry| entry.urls.iter())
		.collect::<std::collections::BTreeSet<_>>()
		.len();
	let (summary, error) =
		summary_text(fonts, declared, target.summary(selected));
	let summary = shaper.fit(&summary, 12., r.w - 40.);
	out.extend(shaper.label(
		&summary,
		12.,
		r.x + 20.,
		r.y + 62.,
		Paint::Styled(
			Condition::Panel,
			if error { C::Error } else { C::Color },
		),
	));
	for (row, index) in
		order.into_iter().skip(page * rows).take(rows).enumerate()
	{
		let e = &entries[index];
		let pos =
			selected.and_then(|ids| ids.iter().position(|id| id == &e.id));
		let y = r.y + 84. + row as f32 * 60.;
		out.push(super::components::line(
			Rect {
				x: r.x + 20.0,
				y: y + 53.0,
				w: r.w - 40.0,
				h: 1.0,
			},
			Condition::Panel,
			C::BorderColor,
		));
		if pos.is_some() {
			out.push(super::components::line(
				Rect {
					x: r.x + 8.0,
					y: y + 5.0,
					w: 2.0,
					h: 38.0,
				},
				Condition::Panel,
				C::Accent,
			));
		}

		let title = format!(
			"{}{} ({})",
			pos.map(|p| format!("{}. ", p + 1)).unwrap_or_default(),
			e.name,
			e.id
		);
		let title = shaper.fit(&title, 13., r.w - 212.);
		out.extend(shaper.label(
			&title,
			13.,
			r.x + 20.,
			y + 18.,
			Paint::Styled(Condition::Panel, C::Color),
		));
		if e.error.is_some() && pos.is_none() {
			let rect = Rect {
				x: r.x + r.w - 180.,
				y,
				w: 76.,
				h: 32.,
			};
			out.push(Draw::Rect(
				rect,
				Paint::Styled(Condition::Button, C::Background),
			));
			out.extend(shaper.label(
				"Invalid",
				12.,
				rect.x + 7.,
				rect.y + 18.,
				Paint::Styled(Condition::Button, C::DisabledColor),
			));
		}
		let detail = e.error.as_deref().unwrap_or(&e.source);
		let detail = shaper.fit(detail, 12., r.w - 48.);
		out.extend(shaper.label(
			&detail,
			12.,
			r.x + 20.,
			y + 40.,
			Paint::Styled(
				Condition::Panel,
				if e.error.is_some() {
					C::Error
				} else {
					C::Muted
				},
			),
		));
	}
	for b in
		style_controls(target, selected, entries, fonts, page, width, height)
	{
		out.extend(draw_button(shaper, interaction, &b, true));
	}
	out
}

#[cfg(test)]
mod stylesheet_tests {
	use super::*;

	fn entry(
		id: &str,
		error: Option<&str>,
		urls: &[&str],
	) -> crate::stylesheet::Entry {
		crate::stylesheet::Entry {
			id: id.into(),
			name: id.into(),
			source: "test".into(),
			error: error.map(str::to_owned),
			urls: urls.iter().map(|url| (*url).to_owned()).collect(),
		}
	}

	#[test]
	fn stylesheet_controls_fit_and_cannot_enable_invalid_entries() {
		let entries =
			vec![entry("a", None, &[]), entry("broken", Some("Invalid"), &[])];
		let selected = vec!["a".to_string()];
		let fonts = crate::fonts::Status::default();
		for target in [StylesTarget::Reader, StylesTarget::Export] {
			for (w, h) in [(500., 300.), (820., 600.)] {
				let panel = panel_rect(w, h);
				let buttons = style_controls(
					target,
					Some(&selected),
					&entries,
					&fonts,
					0,
					w,
					h,
				);
				assert!(buttons.iter().all(|b| {
					panel.contains(b.rect.x, b.rect.y)
						&& panel
							.contains(b.rect.x + b.rect.w, b.rect.y + b.rect.h)
				}));
				assert!(!buttons.iter().any(|b| matches!(
					b.action,
					Command::StyleToggle(1) | Command::ExportStyleToggle(1)
				)));
				let back =
					buttons.iter().find(|b| b.action == target.back()).unwrap();
				assert_eq!(back.label, "Back");
				assert!(back.icon.is_some());

				// Only the reader page offers the system theme.
				let system =
					buttons.iter().any(|b| b.action == Command::SystemTheme);
				assert_eq!(system, target == StylesTarget::Reader);
			}
		}
	}

	#[test]
	fn the_download_button_appears_only_for_declared_font_urls() {
		let selected = vec!["a".to_string()];
		let find = |entries: &[crate::stylesheet::Entry],
		            fonts: &crate::fonts::Status| {
			let (w, h) = (500., 300.);
			let panel = panel_rect(w, h);
			let buttons = style_controls(
				StylesTarget::Reader,
				Some(&selected),
				entries,
				fonts,
				0,
				w,
				h,
			);
			// Every button, the download one included, stays on the panel.
			assert!(buttons.iter().all(|b| {
				panel.contains(b.rect.x, b.rect.y)
					&& panel.contains(b.rect.x + b.rect.w, b.rect.y + b.rect.h)
			}));
			buttons
				.into_iter()
				.find(|b| b.action == Command::FontsDownload)
		};
		assert!(find(&[entry("a", None, &[])], &Default::default()).is_none());
		let url = "https://example.invalid/NotoSerif-Regular.ttf";
		let button = find(&[entry("a", None, &[url])], &Default::default())
			.expect("a declared URL shows the button");
		assert!(button.enabled);
		// The catalogued sheet declares it even when it is not enabled.
		assert!(
			find(&[entry("other", None, &[url])], &Default::default())
				.is_some()
		);
		// A running job holds the one button rather than stacking another.
		let running = crate::fonts::Status {
			running: true,
			total: 1,
			..Default::default()
		};
		assert!(!find(&[entry("a", None, &[url])], &running).unwrap().enabled);
	}

	/// The last page still draws the previous button, so the download button
	/// must not grow under it: hit testing takes the first matching button and
	/// would start a download from the visible back arrow.
	#[test]
	fn the_last_page_leaves_room_for_the_previous_button() {
		let url = "https://example.invalid/NotoSerif-Regular.ttf";
		let entries: Vec<_> = (0..6)
			.map(|i| entry(&format!("style-{i}"), None, &[url]))
			.collect();
		let (w, h) = (500., 300.);
		let rows = style_rows(styles_rect(w, h, entries.len()));
		let last = entries.len().div_ceil(rows).saturating_sub(1);
		assert!(last > 0, "the catalog spans more than one page");
		let buttons = style_controls(
			StylesTarget::Reader,
			None,
			&entries,
			&crate::fonts::Status::default(),
			last,
			w,
			h,
		);
		let download = buttons
			.iter()
			.find(|b| b.action == Command::FontsDownload)
			.expect("a declared URL shows the button");
		let prev = buttons
			.iter()
			.find(|b| b.action == Command::StylePrev)
			.expect("the last page has a previous button");
		assert!(
			download.rect.intersect(prev.rect).is_none(),
			"download {:?} overlaps previous {:?}",
			download.rect,
			prev.rect
		);
		// The window selects the first button under the pointer.
		let (x, y) = (prev.rect.x + 8.0, prev.rect.y + 8.0);
		let hit = buttons
			.iter()
			.find(|b| b.rect.contains(x, y))
			.expect("a button under the arrow");
		assert_eq!(hit.action, Command::StylePrev);
	}

	#[test]
	fn the_font_summary_reports_progress_and_failures() {
		let idle = crate::fonts::Status::default();
		assert_eq!(
			summary_text(&idle, 3, "guidance").0,
			"3 font files are available to download"
		);
		assert_eq!(summary_text(&idle, 0, "guidance").0, "guidance");
		let running = crate::fonts::Status {
			total: 4,
			done: 1,
			current: Some("Bold.ttf".into()),
			running: true,
			..Default::default()
		};
		let (text, error) = summary_text(&running, 3, "guidance");
		assert_eq!(text, "Downloading Bold.ttf — 1/4 files (25%)");
		assert!(!error);
		let failed = crate::fonts::Status {
			total: 4,
			done: 2,
			failures: vec![crate::fonts::Failure {
				file: "Missing.ttf".into(),
				reason: "HTTP 404".into(),
			}],
			..Default::default()
		};
		let (text, error) = summary_text(&failed, 3, "guidance");
		assert!(
			text.contains("Missing.ttf") && text.contains("404"),
			"{text}"
		);
		assert!(error);
	}
}
