//! The Fonts page: every downloadable family, and what the reader has of it.
//!
//! The page is a catalogue, not a queue: it lists what the catalogued
//! stylesheets and the builtin recommendations declare, says what each family
//! is, what it is licensed under, and whether it is already there, and offers
//! one family or all of them at a time. The filter row's last step, past the
//! states of the catalogue, switches to the family choosers: one row per font
//! role, picking which family the reader shapes with — a different job from
//! putting a family on disk, which is what the catalogue views do.
use super::Command as FontCommand;
use crate::app::Button;
use crate::app::chrome::components::{
	ButtonKind, CONTROL, ROW as CHOOSER_ROW, action as entry, draw_button,
	draw_segmented_button, frame, line, menu as option_list, panel_rect,
};
use crate::app::chrome::list::List;
use crate::{
	lang::Lang,
	layout::{Draw, Paint, Rect, TextShaper},
	settings::{FontRole, ReaderSettings},
	state::{Command, Dropdown, DropdownId, InteractionState, PanelTab},
};
use markview_core::fonts::FontConfig;
use markview_core::style::{
	CjkType, ColorField as C, Condition, TextAppearance,
};
use std::collections::HashMap;
use std::sync::Arc;

/// The height one family occupies, actions included.
const ROW: f32 = 88.0;

/// Where the scrolling body starts, below the filter row's separator.
const LIST_TOP: f32 = 136.0;
/// How much of the panel's bottom the footer and its separator take.
fn footer(panel: Rect) -> f32 {
	let actions = if !cfg!(target_os = "android")
		&& panel.w < 488.0
		&& panel.h >= 360.0
	{
		96.0
	} else {
		56.0
	};
	actions + if panel.h >= 360.0 { 32.0 } else { 0.0 }
}

/// The panel rectangle the Fonts page uses.
fn fonts_rect(width: f32, height: f32) -> Rect {
	panel_rect(width, height)
}

/// The roles the page offers a family chooser for.
///
/// A Han role is offered only when the stylesheet resolves its definition
/// for the selected `cjk-type` variant.
pub(in crate::app) fn roles(settings: &ReaderSettings) -> Vec<FontRole> {
	let mut roles =
		vec![FontRole::Serif, FontRole::SansSerif, FontRole::Monospace];
	if settings.cjk_type != CjkType::None {
		let sheet = settings.styled();
		roles.extend(
			[
				FontRole::SerifHan,
				FontRole::SansSerifHan,
				FontRole::MonospaceHan,
			]
			.into_iter()
			.filter(|role| sheet.fontdefs.contains_key(role.id())),
		);
	}
	roles
}

/// The page's scrolling body: the chooser rows the Set step holds, or the
/// families every other step filters.
pub(in crate::app) fn list(
	width: f32,
	height: f32,
	shown: usize,
	scroll: f32,
	choosers: bool,
	roles: usize,
) -> List {
	let r = fonts_rect(width, height);
	let (lead, row, rows) = if choosers {
		(8.0, CHOOSER_ROW, roles)
	} else {
		(0.0, ROW, shown)
	};
	List::new(
		r,
		Rect {
			x: r.x,
			y: r.y + LIST_TOP,
			w: r.w,
			h: (r.h - LIST_TOP - footer(r)).max(0.0),
		},
		lead,
		row,
		rows,
		scroll,
	)
}

/// The baseline of `size`-point text centred in a band of `height`.
fn centered(top: f32, height: f32, size: f32) -> f32 {
	top + height / 2.0 + size * 0.35
}

/// The families a font role's chooser offers, with the stylesheet's own
/// candidate chain first.
///
/// Every family the machine can shape with is offered, because a family the
/// reader has installed is one the document may be set in.
fn font_options(
	role: FontRole,
	families: &[Arc<str>],
	generation: u64,
	settings: &ReaderSettings,
	t: Lang,
) -> Vec<crate::app::chrome::components::Action> {
	let mut entries = vec![entry(
		t.settings_font_default(),
		settings.font_family(role).is_none(),
		Command::FontFamily(role, None),
	)];
	entries.extend(families.iter().enumerate().map(|(index, family)| {
		entry(
			family.clone(),
			settings.font_family(role) == Some(family.as_ref()),
			Command::FontFamily(
				role,
				Some(super::Selection {
					catalog_generation: generation,
					index,
				}),
			),
		)
	}));
	entries
}

/// The name of a font role's row, in the language in force.
fn role_label(role: FontRole, t: Lang) -> &'static str {
	match role {
		FontRole::Serif => t.settings_font_serif(),
		FontRole::SansSerif => t.settings_font_sans_serif(),
		FontRole::Monospace => t.settings_font_monospace(),
		FontRole::SerifHan => t.settings_font_serif_han(),
		FontRole::SansSerifHan => t.settings_font_sans_serif_han(),
		FontRole::MonospaceHan => t.settings_font_monospace_han(),
	}
}

/// One role's chooser control: the family in force, opening its option list.
fn chooser_button(
	list: &List,
	index: usize,
	role: FontRole,
	choices: &super::Choices,
	settings: &ReaderSettings,
	lang: Lang,
) -> Button {
	let band = list.row_rect(index);
	let w = 232.0_f32.min(band.w * 0.56);
	let entries = font_options(
		role,
		choices.families(role),
		choices.generation,
		settings,
		lang,
	);
	let chosen = entries
		.iter()
		.find(|entry| entry.active)
		.or_else(|| entries.first());
	Button {
		label: chosen.map_or_else(|| "".into(), |entry| entry.label.clone()),
		icon: None,
		marker: Some(crate::app::chrome::icons::CHEVRON),
		active: false,
		kind: Default::default(),
		enabled: true,
		action: Command::ToggleDropdown(
			DropdownId::Font(role),
			entries.iter().position(|entry| entry.active).unwrap_or(0),
		),
		rect: Rect {
			x: band.x + band.w - 24.0 - w,
			y: band.y,
			w,
			h: CONTROL,
		},
	}
}

/// The open option list over this page, when one of its chooser rows anchors
/// it. A row the scroll has moved out of the clip holds no list, exactly as a
/// form row the page scrolled away does, and a page showing the catalogue
/// holds no chooser row at all.
pub(in crate::app) fn menu(
	view: &super::View<'_>,
	settings: &ReaderSettings,
	_fonts: &FontConfig,
	open: &mut Dropdown,
	size: (f32, f32),
) -> Option<crate::app::chrome::components::Menu> {
	let DropdownId::Font(role) = open.id else {
		return None;
	};
	if !view.choosers {
		return None;
	}
	let index = roles(settings).iter().position(|shown| *shown == role)?;
	let (width, height) = size;
	let list = list(
		width,
		height,
		view.shown.len(),
		view.scroll,
		true,
		roles(settings).len(),
	);
	let anchor = chooser_button(
		&list,
		index,
		role,
		&view.choices,
		settings,
		settings.lang(),
	)
	.rect;
	anchor.intersect(list.viewport)?;
	let entries = font_options(
		role,
		view.choices.families(role),
		view.choices.generation,
		settings,
		settings.lang(),
	);
	Some(option_list(anchor, &entries, open, size))
}

pub(in crate::app) fn buttons(
	view: &super::View<'_>,
	settings: &ReaderSettings,
	_fonts: &FontConfig,
	preview: bool,
	width: f32,
	height: f32,
	lang: Lang,
) -> Vec<Button> {
	let roles = roles(settings);
	let list = list(
		width,
		height,
		view.shown.len(),
		view.scroll,
		view.choosers,
		roles.len(),
	);
	let mut buttons =
		fonts_controls(list, view.status_filter, view.choosers, preview, lang);
	if view.choosers {
		buttons.extend(
			list.hit(
				roles
					.iter()
					.enumerate()
					.map(|(index, role)| {
						chooser_button(
							&list,
							index,
							*role,
							&view.choices,
							settings,
							lang,
						)
					})
					.collect(),
			),
		);
	} else {
		buttons.extend(list.hit(font_rows(
			view.catalog,
			&view.shown,
			view.jobs,
			list,
			lang,
		)));
	}
	buttons
}

/// One family's action, given what state it is in and whether it is running.
fn action(
	family: &crate::fonts::Family,
	running: bool,
	lang: Lang,
) -> (&'static str, Command) {
	if running {
		return (lang.fonts_cancel(), Command::Fonts(FontCommand::Cancel(0)));
	}
	if family.state == crate::fonts::State::Downloaded {
		return (
			lang.fonts_redownload(),
			Command::Fonts(FontCommand::RedownloadOne(0)),
		);
	}
	if family.state == crate::fonts::State::Provided {
		return (
			lang.fonts_download_copy(),
			Command::Fonts(FontCommand::DownloadOne(0)),
		);
	}
	(
		lang.fonts_download(),
		Command::Fonts(FontCommand::DownloadOne(0)),
	)
}

/// The state badge one family shows.
fn state_label(state: crate::fonts::State, lang: Lang) -> &'static str {
	match state {
		crate::fonts::State::Downloaded => lang.fonts_state_downloaded(),
		crate::fonts::State::Provided => lang.fonts_state_provided(),
		crate::fonts::State::Missing => lang.fonts_state_missing(),
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
	list: List,
	status_filter: Option<crate::fonts::State>,
	choosers: bool,
	preview: bool,
	lang: Lang,
) -> Vec<Button> {
	let r = list.panel;
	let compact = r.w < 488.0;
	let stacked = !cfg!(target_os = "android") && compact && r.h >= 360.0;
	let third = (r.w - 64.0) / 3.0;
	let mut out = crate::app::chrome::components::settings_header_controls(
		r,
		PanelTab::Fonts,
		preview,
		lang,
	);
	if !cfg!(target_os = "android") {
		out.push(Button {
			label: (lang.fonts_open_folder()).into(),
			icon: None,
			marker: None,
			active: false,
			kind: Default::default(),
			enabled: true,
			action: Command::Fonts(FontCommand::OpenFolder),
			rect: Rect {
				x: r.x + 24.,
				y: r.y + r.h - if stacked { 88.0 } else { 48.0 },
				w: if stacked {
					r.w - 48.0
				} else if compact {
					third
				} else {
					146.0
				},
				h: CONTROL,
			},
		});
	}
	// The row's last step leaves the catalogue for the chooser rows, so it is
	// held open by its own flag rather than by a state no family has.
	// Each step's left edge is the last one's right, accumulated in order, so
	// the shared borders meet exactly rather than to within an ulp.
	{
		let w = (r.w - 48.) / 5.;
		let mut x = r.x + 24.;
		for (label, state, set) in [
			(lang.fonts_filter_all(), None, false),
			(
				lang.fonts_state_missing(),
				Some(crate::fonts::State::Missing),
				false,
			),
			(
				lang.fonts_state_downloaded(),
				Some(crate::fonts::State::Downloaded),
				false,
			),
			(
				lang.fonts_state_provided(),
				Some(crate::fonts::State::Provided),
				false,
			),
			(lang.fonts_filter_set(), None, true),
		] {
			out.push(Button {
				label: label.into(),
				icon: None,
				marker: None,
				active: if set {
					choosers
				} else {
					!choosers && status_filter == state
				},
				kind: ButtonKind::Standard,
				enabled: true,
				action: Command::Fonts(if set {
					FontCommand::Choosers
				} else {
					FontCommand::StatusFilter(state)
				}),
				rect: Rect {
					x,
					y: r.y + 96.,
					w,
					h: CONTROL,
				},
			});
			x += w;
		}
	}
	for (label, action, missing_only, x, w) in [
		(
			lang.fonts_download_missing(),
			FontCommand::DownloadMissing,
			true,
			r.w - 288.,
			146.,
		),
		(
			lang.fonts_download_all(),
			FontCommand::DownloadAll,
			false,
			r.w - 134.,
			110.,
		),
	] {
		out.push(Button {
			label: label.into(),
			icon: None,
			marker: None,
			active: false,
			kind: if missing_only {
				ButtonKind::Primary
			} else {
				ButtonKind::Standard
			},
			enabled: true,
			action: Command::Fonts(action),
			rect: Rect {
				x: r.x
					+ if stacked || (cfg!(target_os = "android") && compact) {
						24.0 + if missing_only {
							0.0
						} else {
							(r.w - 40.0) / 2.0
						}
					} else if compact {
						24.0 + (third + 8.0)
							* if missing_only { 1.0 } else { 2.0 }
					} else {
						x
					},
				y: r.y + r.h - 48.,
				w: if stacked || (cfg!(target_os = "android") && compact) {
					(r.w - 56.0) / 2.0
				} else if compact {
					third
				} else {
					w
				},
				h: CONTROL,
			},
		});
	}
	if !list.fits() {
		out.retain(|b| {
			!matches!(
				b.action,
				Command::Fonts(
					FontCommand::StatusFilter(_) | FontCommand::Choosers
				)
			)
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
	lang: Lang,
) -> Vec<Button> {
	let r = list.panel;
	let mut out = vec![];
	if !list.fits() {
		return out;
	}
	for row in list.visible() {
		let family = &catalog[shown[row]];
		let running = jobs.contains_key(&family.family.id);
		let (label, action) = action(family, running, lang);
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
			label: label.into(),
			icon: if running {
				Some(crate::app::chrome::icons::CLOSE)
			} else if family.state == crate::fonts::State::Downloaded {
				Some(crate::app::chrome::icons::REDOWNLOAD)
			} else {
				Some(crate::app::chrome::icons::DOWNLOAD)
			},
			marker: None,
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
	lang: Lang,
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
	let mut out = lang.fonts_summary(shown.len(), missing, bytes_label(bytes));
	// Past the soft cap the page says so; it never refuses a download.
	if bytes > crate::fonts::SOFT_TOTAL_BYTES {
		out.push_str(lang.fonts_directory_large());
	}
	out
}

pub(in crate::app) fn draw_fonts(
	shaper: &mut TextShaper,
	interaction: &InteractionState,
	view: &super::View<'_>,
	settings: &ReaderSettings,
	width: f32,
	height: f32,
	load: Option<&crate::app::settings_load::Load>,
) -> Vec<Draw> {
	let super::View {
		catalog,
		shown,
		jobs,
		scroll,
		note,
		status_filter,
		choosers,
		..
	} = view;
	let (scroll, note, status_filter, choosers) =
		(*scroll, *note, *status_filter, *choosers);
	let lang = settings.lang();
	let preview = interaction.settings_preview;
	shaper.appearance = shaper.stylesheet.text(
		&shaper
			.stylesheet
			.text(&TextAppearance::default(), Condition::Ui),
		Condition::Panel,
	);
	let r = fonts_rect(width, height);
	let roles = roles(settings);
	let list = list(width, height, shown.len(), scroll, choosers, roles.len());
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
			lang.fonts_enlarge(),
			12.,
			r.x + 24.,
			r.y + 120.,
			Paint::Styled(Condition::Panel, C::Muted),
		));
	}

	let summary = summary_text(catalog, shown, note, lang);
	let summary = shaper.fit(&summary, 12., r.w - 48.);
	if r.h >= 360.0 {
		out.extend(shaper.label(
			&summary,
			12.,
			r.x + 24.,
			r.y + r.h - footer(r) + 22.0,
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
	if choosers && load.is_none_or(|load| load.displayed && load.cached) {
		// One chooser row per role: the role's name at the inset the catalogue
		// rows use, the family in force at the control beside it.
		let control_width = 232.0_f32.min(list.viewport.w * 0.56);
		let choosers: Vec<_> = roles
			.iter()
			.enumerate()
			.map(|(index, role)| {
				chooser_button(
					&list,
					index,
					*role,
					&view.choices,
					settings,
					lang,
				)
			})
			.collect();
		for (index, role) in roles.iter().enumerate() {
			let band = list.row_rect(index);
			let label = shaper.fit(
				role_label(*role, lang),
				13.0,
				list.viewport.w - control_width - 48.0,
			);
			body.extend(shaper.label(
				&label,
				13.0,
				list.viewport.x + 24.0,
				centered(band.y, CONTROL, 13.0),
				Paint::Styled(Condition::Panel, C::Color),
			));
		}
		for mut button in choosers {
			crate::app::settings_load::prepare_button(&mut button, load);
			if button.rect.intersect(list.viewport).is_some() {
				body.extend(draw_button(
					shaper,
					&body_interaction,
					&button,
					true,
				));
			}
		}
	}
	if fits && shown.is_empty() && !choosers {
		let empty = list.row_rect(0).y;
		for (text, offset, color) in [
			(lang.fonts_no_match(), 28., C::Color),
			(lang.fonts_select_all(), 50., C::Muted),
		] {
			let text = shaper.fit(text, 13., r.w - 48.);
			body.extend(shaper.label(
				&text,
				13.,
				r.x + 24.,
				empty + offset,
				Paint::Styled(Condition::Panel, color),
			));
		}
	}
	for row in if fits && !choosers {
		list.visible()
	} else {
		0..0
	} {
		let family = &catalog[shown[row]];
		let y = list.row_rect(row).y;
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
			lang.fonts_in_progress()
		} else {
			state_label(family.state, lang)
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
			Some(progress) => describe_job(progress, lang),
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
				.unwrap_or_else(|| lang.fonts_preparing())
				.to_owned()
		} else {
			meta_text(family, lang)
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
	if !choosers {
		for mut b in font_rows(catalog, shown, jobs, list, lang) {
			crate::app::settings_load::prepare_button(&mut b, load);
			if b.rect.intersect(list.viewport).is_some() {
				body.extend(draw_button(shaper, &body_interaction, &b, true));
			}
		}
	}
	out.push(list.clip(body));
	// A list the page refused to draw has no bar to offer either.
	if fits {
		list.draw_bar(&mut out, shaper, interaction);
	}
	let mut controls =
		fonts_controls(list, status_filter, choosers, preview, lang);
	for button in &mut controls {
		crate::app::settings_load::prepare_button(button, load);
	}
	for (i, b) in controls.iter().enumerate() {
		// The header of a settings tab is drawn once, by the header itself.
		if crate::app::chrome::components::is_settings_header(b.action) {
			continue;
		}
		out.extend(draw_segmented_button(shaper, interaction, &controls, i));
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
		lang,
	));
	out
}

/// One family's second line while it is downloading.
fn describe_job(progress: &crate::fonts::Progress, lang: Lang) -> String {
	let phase = match progress.phase {
		crate::fonts::Phase::Queued => lang.fonts_phase_waiting(),
		crate::fonts::Phase::Downloading => lang.fonts_phase_downloading(),
		crate::fonts::Phase::Extracting => lang.fonts_phase_extracting(),
		crate::fonts::Phase::Done => lang.fonts_phase_done(),
		crate::fonts::Phase::Failed => lang.fonts_phase_failed(),
		crate::fonts::Phase::Cancelled => lang.fonts_phase_cancelled(),
	};
	let mut out = phase.to_owned();
	if progress.files_total > 0 {
		let percent = (job_fraction(progress) * 100.0) as u32;
		out.push_str(&lang.fonts_job_percent(percent));
	}
	if progress.bytes_done > 0 {
		out.push_str(&lang.fonts_job_bytes(bytes_label(progress.bytes_done)));
	}
	if progress.files_total > 0 {
		out.push_str(
			&lang.fonts_job_files(progress.files_done, progress.files_total),
		);
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
fn meta_text(family: &crate::fonts::Family, lang: Lang) -> String {
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
				lang.fonts_meta_builtin()
			} else {
				owner.as_str()
			}
		})
		.collect();
	if !owners.is_empty() {
		parts.push(owners.join(lang.fonts_owner_separator()));
	}
	parts.join(" · ")
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::fonts::{Family, State};
	use crate::settings::ReaderSettings;
	use markview_core::style::CjkType;
	use markview_core::style::{FontFamily, FontSource};

	/// The chooser rows the default settings show: a CJK variant is in force,
	/// so all six roles are drawn.
	const CHOOSERS: usize = 6;

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
	fn status_filters_share_borders_and_highlight_the_selection() {
		let mut ui = crate::test_support::shaper();
		let jobs = HashMap::new();
		let settings = ReaderSettings::default();
		for status_filter in [
			None,
			Some(State::Missing),
			Some(State::Downloaded),
			Some(State::Provided),
		] {
			let view = super::super::View {
				choices: {
					let mut c = super::super::Choices::default();
					c.refresh(&crate::test_support::fonts());
					c
				},
				catalog: &[],
				shown: vec![],
				jobs: &jobs,
				scroll: 0.,
				note: None,
				status_filter,
				choosers: false,
			};
			let controls = fonts_controls(
				list(820., 600., 0, 0.0, false, CHOOSERS),
				status_filter,
				false,
				false,
				Lang::En,
			);
			let filters: Vec<_> = controls
				.iter()
				.filter(|b| {
					matches!(
						b.action,
						Command::Fonts(
							FontCommand::StatusFilter(_)
								| FontCommand::Choosers
						)
					)
				})
				.collect();
			assert_eq!(filters.len(), 5);
			assert_eq!(filters.iter().filter(|b| b.active).count(), 1);
			assert!(filters.iter().all(|b| b.rect.h == CONTROL));
			let draws = draw_fonts(
				&mut ui,
				&InteractionState::default(),
				&view,
				&settings,
				820.,
				600.,
				None,
			);
			let edges: Vec<_> = draws
				.iter()
				.filter_map(|d| match d {
					Draw::Rect(r, Paint::Styled(Condition::Button, color))
						if r.y == filters[0].rect.y
							&& r.h == CONTROL && r.w == 1. =>
					{
						Some(color)
					}
					_ => None,
				})
				.collect();
			assert_eq!(edges.len(), 6);
			assert_eq!(edges.iter().filter(|c| ***c == C::Accent).count(), 2);
		}
	}

	/// The Set step holds the chooser rows alone: the catalogue never shows
	/// under them, and the status steps show it alone in turn.
	#[test]
	fn the_set_step_and_the_catalogue_are_separate_views() {
		let catalog =
			vec![entry("a", State::Missing), entry("b", State::Downloaded)];
		let shown = vec![0, 1];
		let jobs = HashMap::new();
		let settings = ReaderSettings::default();
		let fonts = crate::test_support::fonts();
		for (choosers, open) in [(true, false), (false, true)] {
			let view = super::super::View {
				choices: {
					let mut c = super::super::Choices::default();
					c.refresh(&crate::test_support::fonts());
					c
				},
				catalog: &catalog,
				shown: shown.clone(),
				jobs: &jobs,
				scroll: 0.,
				note: None,
				status_filter: None,
				choosers,
			};
			let buttons =
				buttons(&view, &settings, &fonts, false, 820., 600., Lang::En);
			assert_eq!(
				buttons.iter().any(|b| matches!(
					b.action,
					Command::ToggleDropdown(DropdownId::Font(_), _)
				)),
				choosers,
				"the chooser rows belong to the Set step alone"
			);
			assert_eq!(
				buttons.iter().any(|b| matches!(
					b.action,
					Command::Fonts(FontCommand::DownloadOne(_))
				)),
				open,
				"the catalogue belongs to the status steps alone"
			);
			// Exactly one step of the row is held open.
			let filters: Vec<_> = buttons
				.iter()
				.filter(|b| {
					matches!(
						b.action,
						Command::Fonts(
							FontCommand::StatusFilter(_)
								| FontCommand::Choosers
						)
					)
				})
				.collect();
			assert_eq!(filters.len(), 5);
			assert_eq!(filters.iter().filter(|b| b.active).count(), 1);
			assert_eq!(
				filters.iter().find(|b| b.active).unwrap().action,
				if choosers {
					Command::Fonts(FontCommand::Choosers)
				} else {
					Command::Fonts(FontCommand::StatusFilter(None))
				}
			);
			// A list only hangs from a chooser the page is showing.
			let mut open = Dropdown::new(DropdownId::Font(FontRole::Serif), 0);
			assert_eq!(
				menu(&view, &settings, &fonts, &mut open, (820., 600.))
					.is_some(),
				choosers
			);
		}
	}

	#[test]
	fn the_controls_stay_on_the_panel_and_follow_the_row() {
		let catalog =
			vec![entry("a", State::Missing), entry("b", State::Downloaded)];
		let shown = vec![0, 1];
		let jobs = HashMap::new();
		let settings = ReaderSettings::default();
		let fonts = crate::test_support::fonts();
		for (w, h) in [(360., 740.), (500., 300.), (820., 600.)] {
			let panel = panel_rect(w, h);
			let view = super::super::View {
				choices: {
					let mut c = super::super::Choices::default();
					c.refresh(&crate::test_support::fonts());
					c
				},
				catalog: &catalog,
				shown: shown.clone(),
				jobs: &jobs,
				scroll: 0.,
				note: None,
				status_filter: None,
				choosers: false,
			};
			let buttons =
				buttons(&view, &settings, &fonts, false, w, h, Lang::En);
			assert_eq!(
				buttons.iter().any(
					|b| b.action == Command::Fonts(FontCommand::OpenFolder)
				),
				!cfg!(target_os = "android")
			);
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
		// The catalogue's first row downloads and the second offers to download
		// again, whatever roles the Set step holds.
		let rows = list(820., 600., shown.len(), 0.0, false, CHOOSERS);
		let buttons =
			rows.hit(font_rows(&catalog, &shown, &jobs, rows, Lang::En));
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
		let top = fonts_controls(rows, None, false, false, Lang::En)
			.into_iter()
			.find(|b| b.action == Command::Fonts(FontCommand::DownloadMissing))
			.unwrap();
		assert!(top.enabled);
	}

	/// Every settings tab has to be reachable from this page too, or a reader
	/// who lands here cannot leave it.
	#[test]
	fn the_fonts_page_carries_the_settings_tabs() {
		for (width, height) in [
			(360.0, 600.0),
			(411.0, 600.0),
			(500.0, 300.0),
			(820.0, 600.0),
		] {
			let buttons = fonts_controls(
				list(width, height, 1, 0.0, false, CHOOSERS),
				None,
				false,
				false,
				Lang::En,
			);
			for tab in [PanelTab::Generic, PanelTab::Styles, PanelTab::Fonts] {
				assert!(
					buttons
						.iter()
						.any(|b| b.action == Command::SettingsTab(tab)),
					"{tab:?} is missing"
				);
			}
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
		let (w, h) = (500., 260.);
		let rows = list(w, h, shown.len(), 0.0, false, CHOOSERS);
		assert!(!rows.fits());
		let buttons =
			rows.hit(font_rows(&catalog, &shown, &jobs, rows, Lang::En));
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
		let fixed = fonts_controls(rows, None, false, false, Lang::En);
		assert_eq!(
			fixed
				.iter()
				.any(|b| b.action == Command::Fonts(FontCommand::OpenFolder)),
			!cfg!(target_os = "android")
		);
		assert!(
			fixed
				.iter()
				.any(|b| b.action
					== Command::Fonts(FontCommand::DownloadMissing))
		);
		// A whole row fits as soon as the panel is tall enough for one.
		assert!(list(w, 400., shown.len(), 0.0, false, CHOOSERS).fits());
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
		let top = list(w, h, shown.len(), 0.0, false, CHOOSERS);
		assert!(top.max_scroll() > 0.0);
		let rows = top.hit(font_rows(&catalog, &shown, &jobs, top, Lang::En));
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
		let bottom = list(w, h, shown.len(), f32::MAX, false, CHOOSERS);
		assert_eq!(bottom.scroll, top.max_scroll());
		let rows =
			bottom.hit(font_rows(&catalog, &shown, &jobs, bottom, Lang::En));
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
		// The catalogue view offers cancelling for the running family.
		let rows = list(820., 600., shown.len(), 0.0, false, CHOOSERS);
		let buttons =
			rows.hit(font_rows(&catalog, &shown, &jobs, rows, Lang::En));
		assert!(
			buttons
				.iter()
				.any(|b| b.action == Command::Fonts(FontCommand::Cancel(0)))
		);
		// Bulk actions remain available and report when nothing can start.
		let top = fonts_controls(rows, None, false, false, Lang::En)
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
			for shown in [1, 0] {
				for button in fonts_controls(
					list(820., 600., shown, 0.0, false, CHOOSERS),
					None,
					false,
					false,
					Lang::En,
				)
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
		let text = summary_text(&catalog, &shown, None, Lang::En);
		assert!(text.contains("2 families"), "{text}");
		assert!(text.contains("1 missing"), "{text}");
		assert!(text.contains("2.0 MiB"), "{text}");
		// A filtered page reports only the families it shows.
		let empty = summary_text(&catalog, &[], None, Lang::En);
		assert!(empty.contains("0 families"), "{empty}");
		assert!(!empty.contains("MiB"), "{empty}");
		// A note stands in for the whole line.
		assert_eq!(
			summary_text(&catalog, &shown, Some("Offline"), Lang::En),
			"Offline"
		);
	}

	#[test]
	fn download_progress_precedes_long_filenames() {
		let mut progress = crate::fonts::Progress::queued("a");
		progress.phase = crate::fonts::Phase::Downloading;
		progress.bytes_done = 512;
		progress.files_total = 1;
		progress.files_progress = 0.5;
		progress.current = Some("a-very-long-font-family-filename.otf".into());
		assert!(
			describe_job(&progress, Lang::En).starts_with("Downloading, 50%")
		);
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
		assert!(
			describe_job(&progress, Lang::En).contains("2.0 MiB · 6/18 files")
		);
		let jobs = HashMap::from([("a".into(), progress)]);
		let view = super::super::View {
			choices: {
				let mut c = super::super::Choices::default();
				c.refresh(&crate::test_support::fonts());
				c
			},
			catalog: &catalog,
			shown: vec![0],
			jobs: &jobs,
			scroll: 0.,
			note: None,
			status_filter: None,
			choosers: false,
		};
		let rows = list(820., 600., 1, 0., false, CHOOSERS);
		let button = font_rows(&catalog, &[0], &jobs, rows, Lang::En).remove(0);
		assert_eq!(button.action, Command::Fonts(FontCommand::Cancel(0)));
		assert!(button.icon.is_some());
		assert_eq!(button.rect.w, CONTROL);
		let mut shaper = crate::test_support::shaper();
		let draws = draw_fonts(
			&mut shaper,
			&InteractionState::default(),
			&view,
			&ReaderSettings::default(),
			820.,
			600.,
			None,
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
		let meta = meta_text(&family, Lang::En);
		assert!(meta.contains("OFL-1.1"), "{meta}");
		assert!(meta.contains("Built-in"), "{meta}");
	}

	/// Each role's chooser lists the families that role can shape with, with
	/// the stylesheet's own candidate chain first and named "default".
	#[test]
	fn each_font_row_offers_the_families_its_role_can_shape_with() {
		let fonts = crate::test_support::fonts();
		let latin = markview_core::fonts::families(&fonts, false);
		let han = markview_core::fonts::families(&fonts, true);
		assert!(!latin.is_empty(), "the machine has families");
		// A Han family is a subset of them all, so a face that covers no Han
		// text is offered to no Han role and a Latin-only family cannot be
		// picked for one, while it stays in every Latin list.
		assert!(han.len() < latin.len());
		assert!(han.iter().all(|name| latin.contains(name)));
		for (role, families) in [
			(FontRole::Serif, &latin),
			(FontRole::SansSerif, &latin),
			(FontRole::Monospace, &latin),
			(FontRole::SerifHan, &han),
			(FontRole::SansSerifHan, &han),
			(FontRole::MonospaceHan, &han),
		] {
			let settings = ReaderSettings::default();
			let t = settings.lang();
			let entries = font_options(role, families, 1, &settings, t);
			assert_eq!(entries.len(), 1 + families.len());
			assert_eq!(entries[0].label, t.settings_font_default());
			assert_eq!(entries[0].action, Command::FontFamily(role, None));
			assert!(entries[0].active, "the stylesheet's chain is in force");
			// Every family names itself, and none is in force yet.
			for (index, (entry, family)) in
				entries[1..].iter().zip(families.iter()).enumerate()
			{
				assert_eq!(entry.label, *family);
				assert_eq!(
					entry.action,
					Command::FontFamily(
						role,
						Some(super::super::Selection {
							catalog_generation: 1,
							index
						})
					)
				);
				assert!(!entry.active);
			}
			// A pick marks itself rather than the default entry.
			let mut picked = ReaderSettings::default();
			picked.set_font_family(role, Some(families[0].to_string()));
			let entries = font_options(role, families, 1, &picked, t);
			assert!(!entries[0].active);
			assert!(entries[1].active);
			assert_eq!(
				entries[1].action,
				Command::FontFamily(
					role,
					Some(super::super::Selection {
						catalog_generation: 1,
						index: 0
					})
				)
			);
		}
	}

	/// The closed chooser shows the family in force, and opening its list
	/// starts on that family, exactly as the language row does.
	#[test]
	fn a_font_row_shows_the_family_in_force() {
		let fonts = crate::test_support::fonts();
		let families = markview_core::fonts::families(&fonts, false);
		let jobs = HashMap::new();
		let view = super::super::View {
			choices: {
				let mut c = super::super::Choices::default();
				c.refresh(&crate::test_support::fonts());
				c
			},
			catalog: &[],
			shown: vec![],
			jobs: &jobs,
			scroll: 0.,
			note: None,
			status_filter: None,
			choosers: true,
		};
		let control = |settings: &ReaderSettings| {
			buttons(
				&view,
				settings,
				&fonts,
				false,
				1200.,
				800.,
				settings.lang(),
			)
			.into_iter()
			.find(|button| {
				matches!(
					button.action,
					Command::ToggleDropdown(
						DropdownId::Font(FontRole::Monospace),
						_
					)
				)
			})
			.expect("the monospace chooser")
		};
		let mut settings = ReaderSettings::default();
		settings.set_font_family(
			FontRole::Monospace,
			Some(families[0].to_string()),
		);
		let t = settings.lang();
		let row = control(&settings);
		assert_eq!(row.label, families[0]);
		assert!(row.marker.is_some(), "the chooser is marked");
		let Command::ToggleDropdown(_, highlight) = row.action else {
			unreachable!()
		};
		let entries =
			font_options(FontRole::Monospace, &families, 1, &settings, t);
		assert!(entries[highlight].active);
		// Without a pick the chooser names the default entry instead.
		let settings = ReaderSettings::default();
		assert_eq!(control(&settings).label, t.settings_font_default());
	}

	#[test]
	fn font_choosers_only_offer_resolved_han_roles() {
		let fonts = crate::test_support::fonts();
		let jobs = HashMap::new();
		let view = super::super::View {
			choices: {
				let mut c = super::super::Choices::default();
				c.refresh(&crate::test_support::fonts());
				c
			},
			catalog: &[],
			shown: vec![],
			jobs: &jobs,
			scroll: 0.,
			note: None,
			status_filter: None,
			choosers: true,
		};
		let mut sheet = (*ReaderSettings::default().stylesheet).clone();
		sheet.set_cjk_type(CjkType::Sc);
		let mut custom = sheet.clone();
		custom.merge(&markview_core::style::Stylesheet::parse(
			"format_version=2\nversion=1\n[[fontdef]]\nid='monospace[cjk]'\ntype='JP'\nlookfor=['Custom Han Mono']",
		).unwrap());
		for (sheet, cjk_type, han_mono) in [
			(&sheet, CjkType::None, false),
			(&sheet, CjkType::Sc, true),
			(&sheet, CjkType::Tc, false),
			(&sheet, CjkType::Jp, false),
			(&custom, CjkType::Jp, true),
		] {
			let settings = ReaderSettings {
				cjk_type,
				stylesheet: std::sync::Arc::new(sheet.clone()),
				..Default::default()
			};
			let buttons =
				buttons(&view, &settings, &fonts, false, 1200., 800., Lang::En);
			for (role, offered) in [
				(FontRole::Serif, true),
				(FontRole::SansSerif, true),
				(FontRole::Monospace, true),
				(FontRole::SerifHan, cjk_type != CjkType::None),
				(FontRole::SansSerifHan, cjk_type != CjkType::None),
				(FontRole::MonospaceHan, han_mono),
			] {
				assert_eq!(
					buttons.iter().any(|b| matches!(b.action, Command::ToggleDropdown(DropdownId::Font(shown), _) if shown == role)),
					offered,
					"{cjk_type:?}: {role:?} chooser"
				);
				let mut open = Dropdown::new(DropdownId::Font(role), 0);
				assert_eq!(
					menu(&view, &settings, &fonts, &mut open, (1200., 800.))
						.is_some(),
					offered,
					"{cjk_type:?}: {role:?} menu"
				);
			}
		}
	}

	/// A chooser the scroll has moved out of the clip holds no list, exactly as
	/// a form row the page scrolled away does. The Set step needs a short
	/// window to scroll at all: its six rows fit a tall one whole.
	#[test]
	fn a_scrolled_away_chooser_holds_no_list() {
		let fonts = crate::test_support::fonts();
		let settings = ReaderSettings::default();
		let jobs = HashMap::new();
		let view = |scroll: f32| super::super::View {
			choices: {
				let mut c = super::super::Choices::default();
				c.refresh(&crate::test_support::fonts());
				c
			},
			catalog: &[],
			shown: vec![],
			jobs: &jobs,
			scroll,
			note: None,
			status_filter: None,
			choosers: true,
		};
		let size = (500., 300.);
		let list =
			list(size.0, size.1, 0, 0.0, true, super::roles(&settings).len());
		assert!(list.max_scroll() > 0.0, "the short window scrolls");
		let mut open = Dropdown::new(DropdownId::Font(FontRole::Serif), 0);
		assert!(
			menu(&view(0.), &settings, &fonts, &mut open, size).is_some(),
			"the first chooser anchors its list"
		);
		assert!(
			menu(&view(f32::MAX), &settings, &fonts, &mut open, size).is_none(),
			"the page scrolled the chooser away"
		);
	}
}
