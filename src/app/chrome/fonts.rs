//! The Fonts page: every downloadable family, and what the reader has of it.
//!
//! The page is a catalogue, not a queue: it lists what the catalogued
//! stylesheets and the builtin recommendations declare, says what each family
//! is, what it is licensed under, and whether it is already there, and offers
//! one family or all of them at a time.
use super::super::Button;
use super::components::{CONTROL, frame, line};
use super::controls::{draw_button, panel_rect};
use crate::{
	layout::{Draw, Paint, Rect, TextShaper},
	state::{Command, InteractionState, PanelTab},
};
use markview_core::style::{ColorField as C, Condition, TextAppearance};

/// The height one family occupies, actions included.
const ROW: f32 = 72.0;

/// The panel rectangle the Fonts page uses.
pub(in crate::app) fn fonts_rect(width: f32, height: f32) -> Rect {
	panel_rect(width, height)
}

/// Where the list's first row starts, below the filter row's separator.
const LIST_TOP: f32 = 164.0;
/// How much of the panel's bottom the footer and its separator take.
const FOOTER: f32 = 64.0;

/// How many families fit between the filter row and the footer.
///
/// A window too short for even one whole row reports zero rather than drawing
/// a row under the footer, where the bulk action would answer the pointer.
fn fonts_rows(rect: Rect) -> usize {
	let room = rect.h - FOOTER - LIST_TOP;
	if room < ROW {
		return 0;
	}
	(room / ROW).floor() as usize
}

/// The pages the shown families need, never zero so paging arithmetic is safe.
fn fonts_pages(rows: usize, shown: usize) -> usize {
	shown.div_ceil(rows.max(1)).max(1)
}

/// One family's action, given what state it is in and whether it is running.
fn action(
	family: &crate::fonts::Family,
	running: bool,
) -> (&'static str, Command) {
	if running {
		return ("Cancel", Command::FontsCancel(0));
	}
	if family.state == crate::fonts::State::Downloaded {
		return ("Download again", Command::FontsRedownloadOne(0));
	}
	("Download", Command::FontsDownloadOne(0))
}

/// The state badge one family shows.
fn state_label(state: crate::fonts::State) -> &'static str {
	match state {
		crate::fonts::State::Downloaded => "On disk",
		crate::fonts::State::Provided => "Installed",
		crate::fonts::State::Missing => "Missing",
	}
}

fn bytes_label(bytes: u64) -> String {
	if bytes >= 1024 * 1024 {
		format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
	} else {
		format!("{} KiB", bytes / 1024)
	}
}

/// Every button the page draws, in drawing order.
#[expect(clippy::too_many_arguments, reason = "one page's explicit inputs")]
pub(super) fn fonts_controls(
	catalog: &[crate::fonts::Family],
	shown: &[usize],
	jobs: &std::collections::HashMap<String, crate::fonts::Progress>,
	page: usize,
	source_filter: Option<&str>,
	status_filter: Option<crate::fonts::State>,
	preview: bool,
	width: f32,
	height: f32,
) -> Vec<Button> {
	let r = fonts_rect(width, height);
	let rows = fonts_rows(r);
	let page = page.min(fonts_pages(rows, shown.len()).saturating_sub(1));
	let mut out = super::components::settings_header_controls(
		r,
		PanelTab::Fonts,
		preview,
	);
	out.extend(vec![
		Button {
			label: "Open fonts folder",
			icon: None,
			active: false,
			kind: Default::default(),
			enabled: true,
			action: Command::FontsOpenFolder,
			rect: Rect {
				x: r.x + 24.,
				y: r.y + r.h - 48.,
				w: 146.,
				h: CONTROL,
			},
		},
		Button {
			label: if source_filter.is_some() {
				"Source"
			} else {
				"Source: all"
			},
			icon: None,
			active: source_filter.is_some(),
			kind: Default::default(),
			enabled: true,
			action: Command::FontsSourceFilter,
			rect: Rect {
				x: r.x + 24.,
				y: r.y + 116.,
				w: 120.,
				h: 26.,
			},
		},
		Button {
			label: match status_filter {
				None => "Status: all",
				Some(crate::fonts::State::Missing) => "Status: missing",
				Some(crate::fonts::State::Provided) => "Status: installed",
				Some(crate::fonts::State::Downloaded) => "Status: on disk",
			},
			icon: None,
			active: status_filter.is_some(),
			kind: Default::default(),
			enabled: true,
			action: Command::FontsStatusFilter,
			rect: Rect {
				x: r.x + 152.,
				y: r.y + 116.,
				w: 130.,
				h: 26.,
			},
		},
	]);
	// The top action downloads exactly what the filters show and this machine
	// does not have yet.
	let missing = shown.iter().any(|position| {
		let family = &catalog[*position];
		family.state == crate::fonts::State::Missing
			&& !jobs.contains_key(&family.family.id)
	});
	let mut download = Button {
		label: "Download missing",
		icon: None,
		active: false,
		kind: Default::default(),
		enabled: missing,
		action: Command::FontsDownload,
		rect: Rect {
			x: r.x + r.w - 24. - 148.,
			y: r.y + r.h - 48.,
			w: 148.,
			h: CONTROL,
		},
	};
	download.kind = super::components::ButtonKind::Primary;
	out.push(download);
	// With no room for a row there is nothing to page through.
	if rows > 0 && page > 0 {
		out.push(nav(r, "←", Command::FontsPrev, 0.0));
	}
	if rows > 0 && (page + 1) * rows < shown.len() {
		out.push(nav(r, "→", Command::FontsNext, 40.0));
	}
	for (row, position) in shown.iter().skip(page * rows).take(rows).enumerate()
	{
		let family = &catalog[*position];
		let running = jobs.contains_key(&family.family.id);
		let (label, action) = action(family, running);
		let index = page * rows + row;
		let action = match action {
			Command::FontsCancel(_) => Command::FontsCancel(index),
			Command::FontsRedownloadOne(_) => {
				Command::FontsRedownloadOne(index)
			}
			_ => Command::FontsDownloadOne(index),
		};
		out.push(Button {
			label,
			icon: None,
			active: false,
			kind: Default::default(),
			enabled: true,
			action,
			rect: Rect {
				x: r.x + r.w - 160.,
				y: r.y + LIST_TOP + row as f32 * ROW + 10.,
				w: 136.,
				h: 28.,
			},
		});
	}
	out
}

/// One pagination arrow, in the footer beside the folder button so it never
/// overlaps the bulk action on the right.
fn nav(
	rect: Rect,
	label: &'static str,
	action: Command,
	offset: f32,
) -> Button {
	Button {
		label,
		icon: None,
		active: false,
		kind: Default::default(),
		enabled: true,
		action,
		rect: Rect {
			x: rect.x + 180. + offset,
			y: rect.y + rect.h - 48.,
			w: 32.,
			h: CONTROL,
		},
	}
}

/// The one line under the tab row: what the catalogue adds up to.
fn summary_text(
	catalog: &[crate::fonts::Family],
	shown: &[usize],
	note: Option<&str>,
) -> String {
	if let Some(note) = note {
		return note.to_owned();
	}
	let bytes: u64 = catalog.iter().map(|family| family.bytes).sum();
	let missing = shown
		.iter()
		.filter(|position| {
			catalog[**position].state == crate::fonts::State::Missing
		})
		.count();
	let mut out = format!(
		"{} families, {missing} to download, {} on disk",
		shown.len(),
		bytes_label(bytes)
	);
	// Past the soft cap the page says so; it never refuses a download.
	if bytes > crate::fonts::SOFT_TOTAL_BYTES {
		out.push_str(" — the font directory is large");
	}
	out
}

#[expect(clippy::too_many_arguments, reason = "one page's explicit inputs")]
pub(super) fn draw_fonts(
	shaper: &mut TextShaper,
	interaction: &InteractionState,
	catalog: &[crate::fonts::Family],
	shown: &[usize],
	jobs: &std::collections::HashMap<String, crate::fonts::Progress>,
	page: usize,
	note: Option<&str>,
	source_filter: Option<&str>,
	status_filter: Option<crate::fonts::State>,
	preview: bool,
	width: f32,
	height: f32,
) -> Vec<Draw> {
	shaper.appearance = shaper.stylesheet.text(
		&shaper
			.stylesheet
			.text(&TextAppearance::default(), Condition::Ui),
		Condition::Panel,
	);
	let r = fonts_rect(width, height);
	let rows = fonts_rows(r);
	let page = page.min(fonts_pages(rows, shown.len()).saturating_sub(1));
	// Previewing the document leaves only the panel surface, which then
	// recedes with everything else.
	let mut out = if preview {
		vec![line(r, Condition::Panel, C::Background)]
	} else {
		frame(r, width, height)
	};
	if rows == 0 {
		// Too short for one whole row: say so rather than draw a row the
		// footer would sit on top of.
		out.extend(shaper.label(
			"Not enough room to list fonts; enlarge the window",
			12.,
			r.x + 20.,
			r.y + LIST_TOP + 20.,
			Paint::Styled(Condition::Panel, C::Muted),
		));
	}

	let summary = summary_text(catalog, shown, note);
	let summary = shaper.fit(&summary, 12., r.w - 40.);
	out.extend(shaper.label(
		&summary,
		12.,
		r.x + 20.,
		r.y + 104.,
		Paint::Styled(Condition::Panel, C::Muted),
	));
	for y in [r.y + LIST_TOP - 8.0, r.y + r.h - 64.0] {
		out.push(line(
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
	for (row, position) in shown.iter().skip(page * rows).take(rows).enumerate()
	{
		let family = &catalog[*position];
		let y = r.y + LIST_TOP + row as f32 * ROW;
		if row > 0 {
			out.push(line(
				Rect {
					x: r.x + 20.0,
					y: y - 6.0,
					w: r.w - 40.0,
					h: 1.0,
				},
				Condition::Panel,
				C::BorderColor,
			));
		}
		let weight = shaper.appearance.weight;
		shaper.appearance.weight = 700;
		let title = shaper.fit(family.family.display_name(), 14., r.w - 220.);
		out.extend(shaper.label(
			&title,
			14.,
			r.x + 20.,
			y + 20.,
			Paint::Styled(Condition::Panel, C::Color),
		));
		shaper.appearance.weight = weight;
		// A running family reports what it is doing instead of what it is.
		let detail = match jobs.get(&family.family.id) {
			Some(progress) => describe_job(progress),
			None => family
				.family
				.description
				.clone()
				.unwrap_or_else(|| family.family.id.clone()),
		};
		let detail = shaper.fit(&detail, 12., r.w - 200.);
		out.extend(shaper.label(
			&detail,
			12.,
			r.x + 20.,
			y + 40.,
			Paint::Styled(
				Condition::Panel,
				if jobs.contains_key(&family.family.id) {
					C::Accent
				} else {
					C::Muted
				},
			),
		));
		let meta = meta_text(family);
		let meta = shaper.fit(&meta, 11., r.w - 200.);
		out.extend(shaper.label(
			&meta,
			11.,
			r.x + 20.,
			y + 58.,
			Paint::Styled(Condition::Panel, C::Muted),
		));
	}
	for b in fonts_controls(
		catalog,
		shown,
		jobs,
		page,
		source_filter,
		status_filter,
		preview,
		width,
		height,
	) {
		// The header of a settings tab is drawn once, by the header itself.
		if super::components::is_settings_header(b.action) {
			continue;
		}
		out.extend(draw_button(shaper, interaction, &b, true));
	}
	if preview {
		super::components::fade(
			&mut out,
			shaper,
			super::components::PREVIEW_OPACITY,
		);
	}
	// The header goes on top of the fade, so its own controls stay legible.
	out.extend(super::components::draw_settings_header(
		shaper,
		interaction,
		r,
		PanelTab::Fonts,
		preview,
	));
	out
}

/// One family's second line while it is downloading.
fn describe_job(progress: &crate::fonts::Progress) -> String {
	let phase = match progress.phase {
		crate::fonts::Phase::Queued => "Waiting",
		crate::fonts::Phase::Downloading => "Downloading",
		crate::fonts::Phase::Extracting => "Extracting",
		crate::fonts::Phase::Done => "Done",
		crate::fonts::Phase::Failed => "Failed",
		crate::fonts::Phase::Cancelled => "Cancelled",
	};
	let mut out = match (&progress.current, progress.files_total) {
		(Some(current), total) if total > 0 => {
			format!("{phase} {current} — {}/{total} files", progress.files_done)
		}
		(Some(current), _) => format!("{phase} {current}"),
		(None, _) => phase.to_owned(),
	};
	// A known total is more useful as a percentage; without one, the bytes
	// transferred so far are all there is to show.
	match progress.bytes_total {
		Some(total) if total > 0 => {
			let percent = (progress.bytes_done * 100 / total).min(100);
			out.push_str(&format!(", {percent}%"));
		}
		_ if progress.bytes_done > 0 => {
			out.push_str(&format!(", {}", bytes_label(progress.bytes_done)));
		}
		_ => {}
	}
	if let Some(note) = &progress.note {
		out.push_str(&format!(" — {note}"));
	}
	out
}

/// The third line: license, size, and who declares the family.
fn meta_text(family: &crate::fonts::Family) -> String {
	let mut parts = vec![state_label(family.state).to_owned()];
	if let Some(license) = &family.family.license {
		parts.push(license.clone());
	}
	if family.bytes > 0 {
		parts.push(bytes_label(family.bytes));
	}
	let owners: Vec<&str> = family.owners.iter().map(String::as_str).collect();
	if !owners.is_empty() {
		parts.push(owners.join(", "));
	}
	parts.join(" · ")
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::fonts::{Family, State};
	use markview_core::style::{FontFamily, FontSource};

	fn entry(id: &str, state: State) -> Family {
		Family {
			family: FontFamily {
				id: id.into(),
				lookfor: vec![format!("{id} family")],
				description: Some("described".into()),
				license: Some("OFL-1.1".into()),
				license_url: None,
				homepage: None,
				source: vec![FontSource {
					name: None,
					files: Vec::new(),
					archives: Vec::new(),
				}],
			},
			owners: vec!["builtin".into()],
			state,
			files: Vec::new(),
			bytes: 1024 * 1024,
		}
	}

	#[test]
	fn the_controls_stay_on_the_panel_and_follow_the_row() {
		let catalog =
			vec![entry("a", State::Missing), entry("b", State::Downloaded)];
		let shown = vec![0, 1];
		let jobs = std::collections::HashMap::new();
		for (w, h) in [(500., 300.), (820., 600.)] {
			let panel = panel_rect(w, h);
			let buttons = fonts_controls(
				&catalog, &shown, &jobs, 0, None, None, false, w, h,
			);
			assert!(buttons.iter().all(|b| {
				panel.contains(b.rect.x, b.rect.y)
					&& panel.contains(b.rect.x + b.rect.w, b.rect.y + b.rect.h)
			}));
		}
		let buttons = fonts_controls(
			&catalog, &shown, &jobs, 0, None, None, false, 820., 600.,
		);
		// The first row downloads, the second offers to download again.
		assert!(buttons.iter().any(|b| {
			b.action == Command::FontsDownloadOne(0) && b.label == "Download"
		}));
		assert!(buttons.iter().any(|b| {
			b.action == Command::FontsRedownloadOne(1)
				&& b.label == "Download again"
		}));
		// Only one family is missing, so the top action is offered for it.
		let top = buttons
			.iter()
			.find(|b| b.action == Command::FontsDownload)
			.unwrap();
		assert!(top.enabled);
	}

	/// Every settings tab has to be reachable from this page too, or a reader
	/// who lands here cannot leave it.
	#[test]
	fn the_fonts_page_carries_the_settings_tabs() {
		let catalog = vec![entry("a", State::Missing)];
		let shown = vec![0];
		let jobs = std::collections::HashMap::new();
		let buttons = fonts_controls(
			&catalog, &shown, &jobs, 0, None, None, false, 820., 600.,
		);
		for tab in [PanelTab::Generic, PanelTab::Styles, PanelTab::Fonts] {
			assert!(
				buttons
					.iter()
					.any(|b| b.action == Command::SettingsTab(tab)),
				"{tab:?} is missing"
			);
		}
	}

	/// At the shortest supported window no row fits, so none may be drawn over
	/// the footer where the bulk action would answer the pointer.
	#[test]
	fn a_window_too_short_for_a_row_offers_none() {
		let catalog: Vec<_> = (0..3)
			.map(|i| entry(&format!("f{i}"), State::Missing))
			.collect();
		let shown: Vec<usize> = (0..catalog.len()).collect();
		let jobs = std::collections::HashMap::new();
		// The panel is 236 px tall here, eight pixels short of a whole row.
		let (w, h) = (500., 300.);
		let buttons =
			fonts_controls(&catalog, &shown, &jobs, 0, None, None, false, w, h);
		assert!(fonts_rows(fonts_rect(w, h)) == 0);
		assert!(
			!buttons.iter().any(|b| matches!(
				b.action,
				Command::FontsDownloadOne(_)
					| Command::FontsCancel(_)
					| Command::FontsRedownloadOne(_)
					| Command::FontsPrev
					| Command::FontsNext
			)),
			"a row or an arrow is drawn with no room for it"
		);
		// The page still offers a way out and the bulk action.
		assert!(buttons.iter().any(|b| b.action == Command::FontsOpenFolder));
		assert!(buttons.iter().any(|b| b.action == Command::FontsDownload));
		// One row fits as soon as the panel is tall enough for it.
		assert!(fonts_rows(fonts_rect(w, 400.)) >= 1);
	}

	/// Pagination shares the footer with the bulk action, and hit testing takes
	/// the first button under the pointer, so they must never overlap.
	#[test]
	fn the_pagination_arrows_leave_room_for_the_bulk_action() {
		let catalog: Vec<_> = (0..9)
			.map(|i| entry(&format!("f{i}"), State::Missing))
			.collect();
		let shown: Vec<usize> = (0..catalog.len()).collect();
		let jobs = std::collections::HashMap::new();
		let buttons = fonts_controls(
			&catalog, &shown, &jobs, 1, None, None, false, 820., 600.,
		);
		let download = buttons
			.iter()
			.find(|b| b.action == Command::FontsDownload)
			.expect("a bulk action");
		for action in [Command::FontsPrev, Command::FontsNext] {
			let arrow = buttons
				.iter()
				.find(|b| b.action == action)
				.unwrap_or_else(|| panic!("{action:?} is missing"));
			assert!(
				download.rect.intersect(arrow.rect).is_none(),
				"{action:?} {:?} overlaps the bulk action {:?}",
				arrow.rect,
				download.rect
			);
		}
	}

	#[test]
	fn a_running_family_offers_cancelling_instead_of_downloading() {
		let catalog = vec![entry("a", State::Missing)];
		let shown = vec![0];
		let mut jobs = std::collections::HashMap::new();
		jobs.insert(
			"a".to_string(),
			crate::fonts::Progress {
				note: None,
				bytes_total: None,
				..crate::fonts::Progress::queued("a")
			},
		);
		let buttons = fonts_controls(
			&catalog, &shown, &jobs, 0, None, None, false, 820., 600.,
		);
		assert!(buttons.iter().any(|b| b.action == Command::FontsCancel(0)));
		// Nothing is left to start, so the top action is disabled.
		let top = buttons
			.iter()
			.find(|b| b.action == Command::FontsDownload)
			.unwrap();
		assert!(!top.enabled);
	}

	#[test]
	fn the_summary_counts_what_is_missing() {
		let catalog =
			vec![entry("a", State::Missing), entry("b", State::Downloaded)];
		let shown = vec![0, 1];
		let text = summary_text(&catalog, &shown, None);
		assert!(text.contains("2 families"), "{text}");
		assert!(text.contains("1 to download"), "{text}");
		assert!(text.contains("2.0 MiB"), "{text}");
		// A note stands in for the whole line.
		assert_eq!(summary_text(&catalog, &shown, Some("Offline")), "Offline");
	}

	#[test]
	fn a_family_reports_its_license_and_owner() {
		let family = entry("a", State::Provided);
		let meta = meta_text(&family);
		assert!(meta.contains("Installed"), "{meta}");
		assert!(meta.contains("OFL-1.1"), "{meta}");
		assert!(meta.contains("builtin"), "{meta}");
	}
}
