//! The Fonts page: every downloadable family, and what the reader has of it.
//!
//! The page is a catalogue, not a queue: it lists what the catalogued
//! stylesheets and the builtin recommendations declare, says what each family
//! is, what it is licensed under, and whether it is already there, and offers
//! one family or all of them at a time.
use super::Command as FontCommand;
use crate::app::Button;
use crate::app::chrome::components::{CONTROL, frame, line};
use crate::app::chrome::components::{draw_button, panel_rect};
use crate::app::chrome::list::List;
use crate::{
	layout::{Draw, Paint, Rect, TextShaper},
	state::{Command, InteractionState, PanelTab},
};
use markview_core::style::{ColorField as C, Condition, TextAppearance};
use std::collections::HashMap;

/// The height one family occupies, actions included.
const ROW: f32 = 72.0;

/// Where the list's first row starts, below the filter row's separator.
const LIST_TOP: f32 = 164.0;
/// How much of the panel's bottom the footer and its separator take.
const FOOTER: f32 = 64.0;

/// The panel rectangle the Fonts page uses.
fn fonts_rect(width: f32, height: f32) -> Rect {
	panel_rect(width, height)
}

/// The page's scrolling list of families.
pub(in crate::app) fn list(
	width: f32,
	height: f32,
	shown: usize,
	scroll: f32,
) -> List {
	let r = fonts_rect(width, height);
	List::new(
		r,
		Rect {
			x: r.x,
			y: r.y + LIST_TOP,
			w: r.w,
			h: (r.h - LIST_TOP - FOOTER).max(0.0),
		},
		ROW,
		shown,
		scroll,
	)
}

pub(in crate::app) fn buttons(
	view: &super::View<'_>,
	preview: bool,
	width: f32,
	height: f32,
) -> Vec<Button> {
	let list = list(width, height, view.shown.len(), view.scroll);
	let mut buttons = fonts_controls(
		view.catalog,
		&view.shown,
		view.jobs,
		view.source_filter,
		view.status_filter,
		preview,
		width,
		height,
	);
	buttons.extend(list.hit(font_rows(
		view.catalog,
		&view.shown,
		view.jobs,
		list,
	)));
	buttons
}

/// One family's action, given what state it is in and whether it is running.
fn action(
	family: &crate::fonts::Family,
	running: bool,
) -> (&'static str, Command) {
	if running {
		return ("Cancel", Command::Fonts(FontCommand::Cancel(0)));
	}
	if family.state == crate::fonts::State::Downloaded {
		return (
			"Download again",
			Command::Fonts(FontCommand::RedownloadOne(0)),
		);
	}
	("Download", Command::Fonts(FontCommand::DownloadOne(0)))
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

/// The page's fixed controls: the filter row, the footer and the settings
/// header. They sit outside the scrolling list.
#[expect(clippy::too_many_arguments, reason = "one page's explicit inputs")]
fn fonts_controls(
	catalog: &[crate::fonts::Family],
	shown: &[usize],
	jobs: &HashMap<String, crate::fonts::Progress>,
	source_filter: Option<&str>,
	status_filter: Option<crate::fonts::State>,
	preview: bool,
	width: f32,
	height: f32,
) -> Vec<Button> {
	let r = fonts_rect(width, height);
	let mut out = crate::app::chrome::components::settings_header_controls(
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
			action: Command::Fonts(FontCommand::OpenFolder),
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
			action: Command::Fonts(FontCommand::SourceFilter),
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
			action: Command::Fonts(FontCommand::StatusFilter),
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
		action: Command::Fonts(FontCommand::Download),
		rect: Rect {
			x: r.x + r.w - 24. - 148.,
			y: r.y + r.h - 48.,
			w: 148.,
			h: CONTROL,
		},
	};
	download.kind = crate::app::chrome::components::ButtonKind::Primary;
	out.push(download);
	out
}

/// One family's action, at the offset `list` puts its row.
///
/// Only the rows on screen have buttons, so the page never builds a control
/// nothing can draw or reach. The action names the family by its position in
/// the shown list.
fn font_rows(
	catalog: &[crate::fonts::Family],
	shown: &[usize],
	jobs: &HashMap<String, crate::fonts::Progress>,
	list: List,
) -> Vec<Button> {
	let r = list.panel;
	let mut out = vec![];
	for row in list.visible() {
		let family = &catalog[shown[row]];
		let running = jobs.contains_key(&family.family.id);
		let (label, action) = action(family, running);
		let action = match action {
			Command::Fonts(FontCommand::Cancel(_)) => {
				Command::Fonts(FontCommand::Cancel(row))
			}
			Command::Fonts(FontCommand::RedownloadOne(_)) => {
				Command::Fonts(FontCommand::RedownloadOne(row))
			}
			_ => Command::Fonts(FontCommand::DownloadOne(row)),
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
				y: list.row_rect(row).y + 10.,
				w: 136.,
				h: 28.,
			},
		});
	}
	out
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

pub(in crate::app) fn draw_fonts(
	shaper: &mut TextShaper,
	interaction: &InteractionState,
	view: &super::View<'_>,
	width: f32,
	height: f32,
) -> Vec<Draw> {
	let super::View {
		catalog,
		shown,
		jobs,
		scroll,
		note,
		source_filter,
		status_filter,
	} = view;
	let (scroll, note, source_filter, status_filter) =
		(*scroll, *note, *source_filter, *status_filter);
	let preview = interaction.settings_preview;
	shaper.appearance = shaper.stylesheet.text(
		&shaper
			.stylesheet
			.text(&TextAppearance::default(), Condition::Ui),
		Condition::Panel,
	);
	let r = fonts_rect(width, height);
	let list = list(width, height, shown.len(), scroll);
	// Previewing the document leaves only the panel surface, which then
	// recedes with everything else.
	let mut out = if preview {
		vec![line(r, Condition::Panel, C::Background)]
	} else {
		frame(r, width, height)
	};
	// A clip too short for one whole row would only show a sliver of it.
	let fits = list.fits();
	if !fits {
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
	for y in [r.y + LIST_TOP - 8.0, r.y + r.h - FOOTER] {
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
	// A pointer below the fold must not light up the row hidden under the
	// footer, so the body only sees the cursor while it is inside the clip.
	let body_interaction = InteractionState {
		cursor: if list
			.viewport
			.contains(interaction.cursor.0, interaction.cursor.1)
		{
			interaction.cursor
		} else {
			(f32::NEG_INFINITY, f32::NEG_INFINITY)
		},
		focus: interaction.focus,
		focus_visible: interaction.focus_visible,
		pressed: interaction.pressed,
		..Default::default()
	};
	let mut body = Vec::new();
	for row in if fits { list.visible() } else { 0..0 } {
		let family = &catalog[shown[row]];
		let y = list.row_rect(row).y;
		if row > 0 {
			body.push(line(
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
		body.extend(shaper.label(
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
		body.extend(shaper.label(
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
		body.extend(shaper.label(
			&meta,
			11.,
			r.x + 20.,
			y + 58.,
			Paint::Styled(Condition::Panel, C::Muted),
		));
	}
	for b in font_rows(catalog, shown, jobs, list) {
		if b.rect.intersect(list.viewport).is_some() {
			body.extend(draw_button(shaper, &body_interaction, &b, true));
		}
	}
	out.push(list.clip(body));
	// A list the page refused to draw has no bar to offer either.
	if fits {
		list.draw_bar(&mut out, shaper, interaction);
	}
	for b in fonts_controls(
		catalog,
		shown,
		jobs,
		source_filter,
		status_filter,
		preview,
		width,
		height,
	) {
		// The header of a settings tab is drawn once, by the header itself.
		if crate::app::chrome::components::is_settings_header(b.action) {
			continue;
		}
		out.extend(draw_button(shaper, interaction, &b, true));
	}
	if preview {
		crate::app::chrome::components::fade(
			&mut out,
			shaper,
			crate::app::chrome::components::PREVIEW_OPACITY,
		);
	}
	// The header goes on top of the fade, so its own controls stay legible.
	out.extend(crate::app::chrome::components::draw_settings_header(
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
		let jobs = HashMap::new();
		for (w, h) in [(500., 300.), (820., 600.)] {
			let panel = panel_rect(w, h);
			let rows = list(w, h, shown.len(), 0.0);
			let mut buttons = fonts_controls(
				&catalog, &shown, &jobs, None, None, false, w, h,
			);
			buttons.extend(rows.hit(font_rows(&catalog, &shown, &jobs, rows)));
			assert!(buttons.iter().all(|b| {
				panel.contains(b.rect.x, b.rect.y)
					&& panel.contains(b.rect.x + b.rect.w, b.rect.y + b.rect.h)
			}));
		}
		let rows = list(820., 600., shown.len(), 0.0);
		let buttons = rows.hit(font_rows(&catalog, &shown, &jobs, rows));
		// The first row downloads, the second offers to download again.
		assert!(buttons.iter().any(|b| {
			b.action == Command::Fonts(FontCommand::DownloadOne(0))
				&& b.label == "Download"
		}));
		assert!(buttons.iter().any(|b| {
			b.action == Command::Fonts(FontCommand::RedownloadOne(1))
				&& b.label == "Download again"
		}));
		// Only one family is missing, so the top action is offered for it.
		let top = fonts_controls(
			&catalog, &shown, &jobs, None, None, false, 820., 600.,
		)
		.into_iter()
		.find(|b| b.action == Command::Fonts(FontCommand::Download))
		.unwrap();
		assert!(top.enabled);
	}

	/// Every settings tab has to be reachable from this page too, or a reader
	/// who lands here cannot leave it.
	#[test]
	fn the_fonts_page_carries_the_settings_tabs() {
		let catalog = vec![entry("a", State::Missing)];
		let shown = vec![0];
		let jobs = HashMap::new();
		let buttons = fonts_controls(
			&catalog, &shown, &jobs, None, None, false, 820., 600.,
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

	/// At the shortest supported window no whole row fits, so none may be
	/// drawn over the footer where the bulk action would answer the pointer.
	#[test]
	fn a_window_too_short_for_a_row_offers_none() {
		let catalog: Vec<_> = (0..3)
			.map(|i| entry(&format!("f{i}"), State::Missing))
			.collect();
		let shown: Vec<usize> = (0..catalog.len()).collect();
		let jobs = HashMap::new();
		// The panel is 236 px tall here, eight pixels short of a whole row.
		let (w, h) = (500., 300.);
		let rows = list(w, h, shown.len(), 0.0);
		assert!(!rows.fits());
		let buttons = rows.hit(font_rows(&catalog, &shown, &jobs, rows));
		assert!(
			!buttons.iter().any(|b| matches!(
				b.action,
				Command::Fonts(FontCommand::DownloadOne(_))
					| Command::Fonts(FontCommand::Cancel(_))
					| Command::Fonts(FontCommand::RedownloadOne(_))
			)),
			"a row is drawn with no room for it"
		);
		// The page still offers a way out and the bulk action.
		let fixed =
			fonts_controls(&catalog, &shown, &jobs, None, None, false, w, h);
		assert!(
			fixed
				.iter()
				.any(|b| b.action == Command::Fonts(FontCommand::OpenFolder))
		);
		assert!(
			fixed
				.iter()
				.any(|b| b.action == Command::Fonts(FontCommand::Download))
		);
		// A whole row fits as soon as the panel is tall enough for one.
		assert!(list(w, 400., shown.len(), 0.0).fits());
	}

	/// A catalogue past the fold scrolls to its last family instead of paging.
	#[test]
	fn a_long_catalogue_scrolls_instead_of_paging() {
		let catalog: Vec<_> = (0..9)
			.map(|i| entry(&format!("f{i}"), State::Missing))
			.collect();
		let shown: Vec<usize> = (0..catalog.len()).collect();
		let jobs = HashMap::new();
		let (w, h) = (820., 600.);
		let top = list(w, h, shown.len(), 0.0);
		assert!(top.max_scroll() > 0.0);
		let rows = top.hit(font_rows(&catalog, &shown, &jobs, top));
		assert!(
			rows.iter().any(
				|b| b.action == Command::Fonts(FontCommand::DownloadOne(0))
			)
		);
		assert!(
			!rows.iter().any(
				|b| b.action == Command::Fonts(FontCommand::DownloadOne(8))
			)
		);
		let bottom = list(w, h, shown.len(), f32::MAX);
		assert_eq!(bottom.scroll, top.max_scroll());
		let rows = bottom.hit(font_rows(&catalog, &shown, &jobs, bottom));
		assert!(
			rows.iter().any(
				|b| b.action == Command::Fonts(FontCommand::DownloadOne(8))
			)
		);
	}

	#[test]
	fn a_running_family_offers_cancelling_instead_of_downloading() {
		let catalog = vec![entry("a", State::Missing)];
		let shown = vec![0];
		let mut jobs = HashMap::new();
		jobs.insert(
			"a".to_string(),
			crate::fonts::Progress {
				note: None,
				bytes_total: None,
				..crate::fonts::Progress::queued("a")
			},
		);
		let rows = list(820., 600., shown.len(), 0.0);
		let buttons = rows.hit(font_rows(&catalog, &shown, &jobs, rows));
		assert!(
			buttons
				.iter()
				.any(|b| b.action == Command::Fonts(FontCommand::Cancel(0)))
		);
		// Nothing is left to start, so the top action is disabled.
		let top = fonts_controls(
			&catalog, &shown, &jobs, None, None, false, 820., 600.,
		)
		.into_iter()
		.find(|b| b.action == Command::Fonts(FontCommand::Download))
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
