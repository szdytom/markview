//! The Fonts page: every downloadable family, and what the reader has of it.
//!
//! The page is a catalogue, not a queue: it lists what the catalogued
//! stylesheets and the builtin recommendations declare, says what each family
//! is, what it is licensed under, and whether it is already there, and offers
//! one family or all of them at a time.
use super::Command as FontCommand;
use crate::app::Button;
use crate::app::chrome::components::{ButtonKind, draw_button, panel_rect};
use crate::app::chrome::components::{CONTROL, frame, line};
use crate::app::chrome::list::List;
use crate::{
	layout::{Draw, Paint, Rect, TextShaper},
	state::{Command, InteractionState, PanelTab},
};
use markview_core::style::{ColorField as C, Condition, TextAppearance};
use std::collections::HashMap;

/// The height one family occupies, actions included.
const ROW: f32 = 88.0;

/// Where the list's first row starts, below the filter row's separator.
const LIST_TOP: f32 = 136.0;
/// How much of the panel's bottom the footer and its separator take.
fn footer(panel: Rect) -> f32 {
	if panel.h >= 360.0 { 88.0 } else { 56.0 }
}

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
			h: (r.h - LIST_TOP - footer(r)).max(0.0),
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
	let mut buttons =
		fonts_controls(&view.shown, view.status_filter, preview, width, height);
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
		return ("Redownload", Command::Fonts(FontCommand::RedownloadOne(0)));
	}
	if family.state == crate::fonts::State::Provided {
		return ("Download copy", Command::Fonts(FontCommand::DownloadOne(0)));
	}
	("Download", Command::Fonts(FontCommand::DownloadOne(0)))
}

/// The state badge one family shows.
fn state_label(state: crate::fonts::State) -> &'static str {
	match state {
		crate::fonts::State::Downloaded => "Downloaded",
		crate::fonts::State::Provided => "In System",
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
fn fonts_controls(
	shown: &[usize],
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
	out.extend(vec![Button {
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
	}]);
	for (index, (label, state)) in [
		("All", None),
		("Missing", Some(crate::fonts::State::Missing)),
		("Downloaded", Some(crate::fonts::State::Downloaded)),
		("In System", Some(crate::fonts::State::Provided)),
	]
	.into_iter()
	.enumerate()
	{
		let w = (r.w - 48.) / 4.;
		out.push(Button {
			label,
			icon: None,
			active: status_filter == state,
			kind: ButtonKind::Quiet,
			enabled: true,
			action: Command::Fonts(FontCommand::StatusFilter(state)),
			rect: Rect {
				x: r.x + 24. + index as f32 * w,
				y: r.y + 96.,
				w,
				h: 28.,
			},
		});
	}
	for (label, action, missing_only, x, w) in [
		(
			"Download Missing",
			FontCommand::DownloadMissing,
			true,
			r.w - 288.,
			146.,
		),
		(
			"Download All",
			FontCommand::DownloadAll,
			false,
			r.w - 134.,
			110.,
		),
	] {
		out.push(Button {
			label,
			icon: None,
			active: false,
			kind: if missing_only {
				ButtonKind::Primary
			} else {
				ButtonKind::Standard
			},
			enabled: true,
			action: Command::Fonts(action),
			rect: Rect {
				x: r.x + x,
				y: r.y + r.h - 48.,
				w,
				h: CONTROL,
			},
		});
	}
	if !list(width, height, shown.len(), 0.).fits() {
		out.retain(|b| {
			!matches!(b.action, Command::Fonts(FontCommand::StatusFilter(_)))
		});
	}
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
	if !list.fits() {
		return out;
	}
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
			icon: if running {
				Some(crate::app::chrome::icons::CLOSE)
			} else if family.state == crate::fonts::State::Downloaded {
				Some(crate::app::chrome::icons::REDOWNLOAD)
			} else {
				Some(crate::app::chrome::icons::DOWNLOAD)
			},
			active: false,
			kind: if running || family.state != crate::fonts::State::Missing {
				ButtonKind::Quiet
			} else {
				ButtonKind::Standard
			},
			enabled: true,
			action,
			rect: Rect {
				x: r.x + r.w - 24. - CONTROL,
				y: list.row_rect(row).y + 34.,
				w: CONTROL,
				h: 32.,
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
	// The count and the byte total follow the same filter, so a filtered
	// page never claims bytes it is not showing.
	let bytes: u64 =
		shown.iter().map(|position| catalog[*position].bytes).sum();
	let missing = shown
		.iter()
		.filter(|position| {
			catalog[**position].state == crate::fonts::State::Missing
		})
		.count();
	let mut out = format!(
		"{} families, {missing} missing, {} on disk",
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
		status_filter,
	} = view;
	let (scroll, note, status_filter) = (*scroll, *note, *status_filter);
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
			"Enlarge the window to browse fonts",
			12.,
			r.x + 24.,
			r.y + 120.,
			Paint::Styled(Condition::Panel, C::Muted),
		));
	}

	let summary = summary_text(catalog, shown, note);
	let summary = shaper.fit(&summary, 12., r.w - 48.);
	if footer(r) > 56.0 {
		out.extend(shaper.label(
			&summary,
			12.,
			r.x + 24.,
			r.y + r.h - 66.,
			Paint::Styled(Condition::Panel, C::Muted),
		));
	}
	for y in [
		r.y + if fits { LIST_TOP - 1.0 } else { 92.0 },
		r.y + r.h - footer(r),
	] {
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
	if fits && shown.is_empty() {
		for (text, offset, color) in [
			("No fonts match these filters", 28., C::Color),
			("Select All to see all available families.", 50., C::Muted),
		] {
			let text = shaper.fit(text, 13., r.w - 48.);
			body.extend(shaper.label(
				&text,
				13.,
				r.x + 24.,
				list.viewport.y + offset,
				Paint::Styled(Condition::Panel, color),
			));
		}
	}
	for row in if fits { list.visible() } else { 0..0 } {
		let family = &catalog[shown[row]];
		let y = list.row_rect(row).y;
		if row > 0 {
			body.push(line(
				Rect {
					x: r.x + 24.0,
					y,
					w: r.w - 48.0,
					h: 1.0,
				},
				Condition::Panel,
				C::BorderColor,
			));
		}
		let weight = shaper.appearance.weight;
		shaper.appearance.weight = 600;
		let title = shaper.fit(family.family.display_name(), 14., r.w - 188.);
		body.extend(shaper.label(
			&title,
			14.,
			r.x + 24.,
			y + 26.,
			Paint::Styled(Condition::Panel, C::Color),
		));
		shaper.appearance.weight = weight;
		let status = if jobs.contains_key(&family.family.id) {
			"In progress"
		} else {
			state_label(family.state)
		};
		let status_x = r.x + r.w - 24. - shaper.text_width(status, 11.);
		body.extend(shaper.label(
			status,
			11.,
			status_x,
			y + 25.,
			Paint::Styled(Condition::Panel, C::Muted),
		));
		// A running family reports what it is doing instead of what it is.
		let detail = match jobs.get(&family.family.id) {
			Some(progress) => describe_job(progress),
			None => family
				.family
				.description
				.clone()
				.unwrap_or_else(|| family.family.id.clone()),
		};
		let detail = shaper.fit(&detail, 12., r.w - 96.);
		body.extend(shaper.label(
			&detail,
			12.,
			r.x + 24.,
			y + 47.,
			Paint::Styled(
				Condition::Panel,
				if jobs.contains_key(&family.family.id) {
					C::Accent
				} else {
					C::Muted
				},
			),
		));
		let meta = if let Some(progress) = jobs.get(&family.family.id) {
			progress
				.current
				.as_deref()
				.or(progress.note.as_deref())
				.unwrap_or("Preparing download")
				.to_owned()
		} else {
			meta_text(family)
		};
		let meta = shaper.fit(&meta, 11., r.w - 96.);
		body.extend(shaper.label(
			&meta,
			11.,
			r.x + 24.,
			y + 67.,
			Paint::Styled(Condition::Panel, C::Muted),
		));
		if let Some(progress) = jobs.get(&family.family.id) {
			let track = Rect {
				x: r.x + 24.,
				y: y + 78.,
				w: r.w - 48.,
				h: 3.,
			};
			body.push(line(track, Condition::Panel, C::BorderColor));
			body.push(line(
				Rect {
					w: track.w * job_fraction(progress),
					..track
				},
				Condition::Panel,
				C::Accent,
			));
		}
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
	for b in fonts_controls(shown, status_filter, preview, width, height) {
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
	let mut out = phase.to_owned();
	if progress.files_total > 0 {
		let percent = (job_fraction(progress) * 100.0) as u32;
		out.push_str(&format!(", {percent}%"));
	}
	if progress.bytes_done > 0 {
		out.push_str(&format!(" · {}", bytes_label(progress.bytes_done)));
	}
	if progress.files_total > 0 {
		out.push_str(&format!(
			" · {}/{} files",
			progress.files_done, progress.files_total
		));
	}

	out
}

/// Active transfers contribute their byte fraction to the file count.
fn job_fraction(progress: &crate::fonts::Progress) -> f32 {
	if progress.files_total > 0 {
		(progress.files_progress.max(progress.files_done as f64)
			/ progress.files_total as f64)
			.clamp(0.0, 1.0) as f32
	} else {
		0.0
	}
}

/// The third line: license, size, and who declares the family.
fn meta_text(family: &crate::fonts::Family) -> String {
	let mut parts = vec![];
	if let Some(license) = &family.family.license {
		parts.push(license.clone());
	}
	if family.bytes > 0 {
		parts.push(bytes_label(family.bytes));
	}
	let owners: Vec<&str> = family
		.owners
		.iter()
		.map(|owner| {
			if owner == "builtin" {
				"Built-in"
			} else {
				owner.as_str()
			}
		})
		.collect();
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
			let mut buttons = fonts_controls(&shown, None, false, w, h);
			buttons.extend(rows.hit(font_rows(&catalog, &shown, &jobs, rows)));
			assert!(buttons.iter().all(|b| {
				panel.contains(b.rect.x, b.rect.y)
					&& panel.contains(b.rect.x + b.rect.w, b.rect.y + b.rect.h)
			}));
			for (index, button) in buttons.iter().enumerate() {
				for other in &buttons[index + 1..] {
					assert!(
						button.rect.intersect(other.rect).is_none(),
						"{:?} overlaps {:?}",
						button.action,
						other.action
					);
				}
			}
		}
		let rows = list(820., 600., shown.len(), 0.0);
		let buttons = rows.hit(font_rows(&catalog, &shown, &jobs, rows));
		// The first row downloads, the second offers to download again.
		assert!(buttons.iter().any(|b| {
			b.action == Command::Fonts(FontCommand::DownloadOne(0))
				&& b.label == "Download"
				&& b.icon.is_some()
		}));
		assert!(buttons.iter().any(|b| {
			b.action == Command::Fonts(FontCommand::RedownloadOne(1))
				&& b.label == "Redownload"
				&& b.icon.is_some()
		}));
		// Only one family is missing, so the top action is offered for it.
		let top = fonts_controls(&shown, None, false, 820., 600.)
			.into_iter()
			.find(|b| b.action == Command::Fonts(FontCommand::DownloadMissing))
			.unwrap();
		assert!(top.enabled);
	}

	/// Every settings tab has to be reachable from this page too, or a reader
	/// who lands here cannot leave it.
	#[test]
	fn the_fonts_page_carries_the_settings_tabs() {
		let shown = vec![0];
		let buttons = fonts_controls(&shown, None, false, 820., 600.);
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
		// The remaining viewport is shorter than one whole row.
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
		let fixed = fonts_controls(&shown, None, false, w, h);
		assert!(
			fixed
				.iter()
				.any(|b| b.action == Command::Fonts(FontCommand::OpenFolder))
		);
		assert!(
			fixed
				.iter()
				.any(|b| b.action
					== Command::Fonts(FontCommand::DownloadMissing))
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
				files_progress: 0.0,
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
		// Bulk actions remain available and report when nothing can start.
		let top = fonts_controls(&shown, None, false, 820., 600.)
			.into_iter()
			.find(|b| b.action == Command::Fonts(FontCommand::DownloadMissing))
			.unwrap();
		assert!(top.enabled);
	}

	#[test]
	fn bulk_buttons_respond_to_hover_even_without_pending_downloads() {
		let mut ui = crate::test_support::shaper();
		for dark in [false, true] {
			ui.set_stylesheet(markview_core::style::Stylesheet::bundled(dark));
			for shown in [vec![0], vec![]] {
				for button in fonts_controls(&shown, None, false, 820., 600.)
					.into_iter()
					.filter(|b| {
						matches!(
							b.action,
							Command::Fonts(
								FontCommand::DownloadMissing
									| FontCommand::DownloadAll
							)
						)
					}) {
					assert!(button.enabled);
					let fills: Vec<_> = [
						(f32::NEG_INFINITY, f32::NEG_INFINITY),
						(
							button.rect.x + button.rect.w / 2.,
							button.rect.y + button.rect.h / 2.,
						),
					]
					.into_iter()
					.map(|cursor| {
						let draws = draw_button(
							&mut ui,
							&InteractionState {
								cursor,
								..Default::default()
							},
							&button,
							true,
						);
						let Draw::Rect(_, paint) = draws[0] else {
							panic!("button background")
						};
						ui.stylesheet.paint(paint)
					})
					.collect();
					assert_ne!(fills[0], fills[1]);
				}
			}
		}
	}

	#[test]
	fn the_summary_counts_what_is_missing() {
		let catalog =
			vec![entry("a", State::Missing), entry("b", State::Downloaded)];
		let shown = vec![0, 1];
		let text = summary_text(&catalog, &shown, None);
		assert!(text.contains("2 families"), "{text}");
		assert!(text.contains("1 missing"), "{text}");
		assert!(text.contains("2.0 MiB"), "{text}");
		// A filtered page reports only the families it shows.
		let empty = summary_text(&catalog, &[], None);
		assert!(empty.contains("0 families"), "{empty}");
		assert!(!empty.contains("MiB"), "{empty}");
		// A note stands in for the whole line.
		assert_eq!(summary_text(&catalog, &shown, Some("Offline")), "Offline");
	}

	#[test]
	fn download_progress_precedes_long_filenames() {
		let mut progress = crate::fonts::Progress::queued("a");
		progress.phase = crate::fonts::Phase::Downloading;
		progress.bytes_done = 512;
		progress.files_total = 1;
		progress.files_progress = 0.5;
		progress.current = Some("a-very-long-font-family-filename.otf".into());
		assert!(describe_job(&progress).starts_with("Downloading, 50%"));
	}

	#[test]
	fn downloads_without_byte_totals_have_a_track_and_svg_cancel() {
		let catalog = vec![entry("a", State::Missing)];
		let mut progress = crate::fonts::Progress::queued("a");
		progress.phase = crate::fonts::Phase::Downloading;
		progress.files_done = 6;
		progress.files_progress = 6.5;
		progress.files_total = 18;
		progress.bytes_done = 2 * 1024 * 1024;
		assert!(describe_job(&progress).contains("2.0 MiB · 6/18 files"));
		let jobs = HashMap::from([("a".into(), progress)]);
		let view = super::super::View {
			catalog: &catalog,
			shown: vec![0],
			jobs: &jobs,
			scroll: 0.,
			note: None,
			status_filter: None,
		};
		let rows = list(820., 600., 1, 0.);
		let button = font_rows(&catalog, &[0], &jobs, rows).remove(0);
		assert_eq!(button.action, Command::Fonts(FontCommand::Cancel(0)));
		assert!(button.icon.is_some());
		assert_eq!(button.rect.w, CONTROL);
		let mut shaper = crate::test_support::shaper();
		let draws = draw_fonts(
			&mut shaper,
			&InteractionState::default(),
			&view,
			820.,
			600.,
		);
		let Draw::Clipped { draws, .. } = draws
			.iter()
			.find(|d| matches!(d, Draw::Clipped { .. }))
			.unwrap()
		else {
			unreachable!()
		};
		let expected = (rows.panel.w - 48.) * (6.5 / 18.);
		assert!(draws.iter().any(|d| matches!(d,
			Draw::Rect(rect, Paint::Styled(Condition::Panel, C::Accent))
			if rect.h == 3. && (rect.w - expected).abs() < 0.01
		)));
	}

	#[test]
	fn a_family_reports_its_license_and_owner() {
		let family = entry("a", State::Provided);
		let meta = meta_text(&family);
		assert!(meta.contains("OFL-1.1"), "{meta}");
		assert!(meta.contains("Built-in"), "{meta}");
	}
}
