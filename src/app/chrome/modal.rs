//! The confirmation for a local file whose type is not known to be inert.
use super::super::Button;
use crate::{
	layout::{Draw, Paint, Rect, TextShaper},
	state::{Command, InteractionState, Modal},
};
use markview_core::style::{ColorField as C, Condition, TextAppearance};
use std::path::{Component, Path, PathBuf};

/// The path size in the confirmation, in logical pixels.
const PATH_SIZE: f32 = 13.0;
/// Marks removed text. Never `…`, which beside a separator reads as `..`.
const MARKER: &str = "[...]";
/// How far a relative display may climb. `../archive/x` is short and clear;
/// `../../../../../srv/x` is shorter than the absolute path and useless.
const MAX_CLIMB: usize = 2;

pub(in crate::app) fn modal_rect(width: f32, height: f32) -> Rect {
	let w = 560.0_f32.min((width - 32.0).max(0.0));
	let h = 240.0_f32.min((height - 32.0).max(0.0));
	Rect {
		x: (width - w) / 2.0,
		y: (height - h) / 2.0,
		w,
		h,
	}
}

/// The reader's home directory, as the platform reports it.
fn home_dir() -> Option<PathBuf> {
	std::env::var_os("HOME")
		.or_else(|| std::env::var_os("USERPROFILE"))
		.map(PathBuf::from)
		.filter(|home| home.is_absolute())
}

/// `path` expressed from `base`, walking up and then down, when they share a
/// root.
fn relative_to(path: &Path, base: &Path) -> Option<PathBuf> {
	let mut from: Vec<Component> = path.components().collect();
	let up: Vec<Component> = base.components().collect();
	// `path` is a file, so its last component is the file name.
	let name = from.pop()?;
	let common = from
		.iter()
		.zip(up.iter())
		.take_while(|(a, b)| a == b)
		.count();
	// Different roots cannot be related, and a root-level file has nothing
	// shorter to say.
	if common == 0 {
		return None;
	}
	let mut out = PathBuf::new();
	let climb = up.len().saturating_sub(common);
	if climb > MAX_CLIMB {
		return None;
	}
	for _ in common..up.len() {
		out.push("..");
	}
	for component in &from[common..] {
		out.push(component.as_os_str());
	}
	out.push(name.as_os_str());
	Some(out)
}

/// The path as the reader has to judge it.
///
/// The file name and the directories next to it carry the decision, so the
/// display prefers, in order: a path relative to the open document, the
/// home-shortened absolute path, and otherwise the longest tail that fits with
/// its front replaced by [`MARKER`]. Only when the file name alone does not fit
/// does elision move inside it, and then it keeps the end, so the extension
/// that decides the class stays visible.
fn visible_path(
	shaper: &mut TextShaper,
	path: &Path,
	document_dir: Option<&Path>,
	max: f32,
) -> String {
	let mut full = path.display().to_string();
	if let Some(home) = home_dir()
		&& let Ok(rest) = path.strip_prefix(&home)
	{
		full = if rest.as_os_str().is_empty() {
			"~".into()
		} else {
			format!("~/{}", rest.display())
		};
	}
	// `./` states the base: a bare `targets/x` could be read as relative to
	// wherever the reader happens to be.
	let relative =
		document_dir
			.and_then(|base| relative_to(path, base))
			.map(|rel| {
				if rel.starts_with("..") {
					rel.display().to_string()
				} else {
					format!("./{}", rel.display())
				}
			});
	let chosen = match relative {
		Some(rel)
			if shaper.text_width(&rel, PATH_SIZE)
				< shaper.text_width(&full, PATH_SIZE) =>
		{
			rel
		}
		_ => full,
	};
	if shaper.text_width(&chosen, PATH_SIZE) <= max {
		return chosen;
	}
	// Drop whole leading segments; the longest tail that fits wins.
	let separator = std::path::MAIN_SEPARATOR.to_string();
	let segments: Vec<&str> = chosen.split(&separator).collect();
	for start in 1..segments.len() {
		let candidate = format!(
			"{MARKER}{separator}{}",
			segments[start..].join(&separator)
		);
		if shaper.text_width(&candidate, PATH_SIZE) <= max {
			return candidate;
		}
	}
	match segments.last() {
		Some(name) => elide_name(shaper, name, max),
		None => MARKER.to_owned(),
	}
}

/// Elides inside one name, keeping its end so the extension survives.
fn elide_name(shaper: &mut TextShaper, name: &str, max: f32) -> String {
	let chars: Vec<char> = name.chars().collect();
	if chars.is_empty() {
		return MARKER.to_owned();
	}
	let build = |head: usize, tail: usize| {
		let head: String = chars[..head].iter().collect();
		let tail: String = chars[chars.len() - tail..].iter().collect();
		format!("{head}{MARKER}{tail}")
	};
	let mut tail = chars.len().min(24);
	while tail > 0 && shaper.text_width(&build(0, tail), PATH_SIZE) > max {
		tail -= 1;
	}
	if tail == 0 {
		return if shaper.text_width(MARKER, PATH_SIZE) <= max {
			MARKER.to_owned()
		} else {
			String::new()
		};
	}
	let mut head = 0;
	while head + tail < chars.len()
		&& shaper.text_width(&build(head + 1, tail), PATH_SIZE) <= max
	{
		head += 1;
	}
	build(head, tail)
}

fn panel_appearance(shaper: &mut TextShaper) -> TextAppearance {
	shaper.stylesheet.text(
		&shaper
			.stylesheet
			.text(&TextAppearance::default(), Condition::Ui),
		Condition::Panel,
	)
}

fn button_width(shaper: &mut TextShaper, label: &str) -> f32 {
	let old = shaper.appearance.clone();
	shaper.appearance = panel_appearance(shaper);
	let width = shaper.text_width(label, 13.0) + 26.0;
	shaper.appearance = old;
	width
}

/// The modal's controls: "Open folder" is the default, "Open anyway" is the
/// deliberate action, and "Close" dismisses without doing anything.
pub(in crate::app) fn modal_buttons(
	shaper: &mut TextShaper,
	interaction: &InteractionState,
	width: f32,
	height: f32,
) -> Vec<Button> {
	if interaction.modal.is_none() {
		return Vec::new();
	}
	let rect = modal_rect(width, height);
	let close = button_width(shaper, "Close");
	let folder = button_width(shaper, "Open folder");
	let anyway = button_width(shaper, "Open anyway");
	let row = rect.y + rect.h - 46.0;
	vec![
		Button {
			label: "Open folder",
			action: Command::ModalOpenFolder,
			rect: Rect {
				x: rect.x + rect.w - 20.0 - folder,
				y: row,
				w: folder,
				h: 30.0,
			},
		},
		Button {
			label: "Open anyway",
			action: Command::ModalConfirm,
			rect: Rect {
				x: rect.x + rect.w - 28.0 - folder - anyway,
				y: row,
				w: anyway,
				h: 30.0,
			},
		},
		Button {
			label: "Close",
			action: Command::ModalDismiss,
			rect: Rect {
				x: rect.x + rect.w - 20.0 - close,
				y: rect.y + 14.0,
				w: close,
				h: 28.0,
			},
		},
	]
}

pub(in crate::app) fn draw_modal(
	shaper: &mut TextShaper,
	interaction: &InteractionState,
	width: f32,
	height: f32,
) -> Vec<Draw> {
	let Some(Modal::OpenLocal {
		path, document_dir, ..
	}) = &interaction.modal
	else {
		return Vec::new();
	};
	shaper.appearance = panel_appearance(shaper);
	let rect = modal_rect(width, height);
	let text = Paint::Styled(Condition::Panel, C::Color);
	let muted = Paint::Styled(Condition::Panel, C::Muted);
	let x = rect.x + 20.0;
	let mut out = vec![
		Draw::Rect(
			Rect {
				x: 0.0,
				y: 0.0,
				w: width,
				h: height,
			},
			Paint::Scrim,
		),
		Draw::Rect(
			Rect {
				x: rect.x - 5.0,
				y: rect.y + 6.0,
				w: rect.w + 10.0,
				h: rect.h + 4.0,
			},
			Paint::Shadow,
		),
		Draw::Box {
			rect,
			chain: Condition::Panel.chain(),
			condition: Condition::Panel,
			radius: 0.,
			border: 1.,
			left_only: false,
		},
	];
	out.extend(shaper.label("Open this file?", 20.0, x, rect.y + 36.0, text));
	// The canonical path, not the link label, which the document controls.
	let close = button_width(shaper, "Close");
	let shown = visible_path(
		shaper,
		path,
		document_dir.as_deref(),
		rect.w - 48.0 - close,
	);
	out.extend(shaper.label(&shown, PATH_SIZE, x, rect.y + 62.0, muted));
	let kind = path
		.extension()
		.and_then(|e| e.to_str())
		.map(str::to_ascii_lowercase)
		.unwrap_or_else(|| "no extension".into());
	out.extend(shaper.label(
		&format!("Type · {kind}"),
		13.0,
		x,
		rect.y + 84.0,
		muted,
	));
	for (i, line) in [
		"Markview will hand this file to the system default application.",
		"That application may execute code, so continue only if you trust the file.",
	]
	.into_iter()
	.enumerate()
	{
		out.extend(shaper.label(
			line,
			13.0,
			x,
			rect.y + 114.0 + i as f32 * 20.0,
			muted,
		));
	}
	for button in modal_buttons(shaper, interaction, width, height) {
		out.push(Draw::Box {
			rect: button.rect,
			chain: Condition::Button.chain(),
			condition: Condition::Button,
			radius: 0.,
			border: 1.,
			left_only: false,
		});
		// The same focus ring, hover fill and press fill as the settings panel.
		if interaction.focus == Some(button.action) {
			out.push(Draw::Rect(
				button.rect,
				Paint::Styled(Condition::Button, C::FocusColor),
			));
			out.push(Draw::Rect(
				Rect {
					x: button.rect.x + 1.0,
					y: button.rect.y + 1.0,
					w: (button.rect.w - 2.0).max(0.0),
					h: (button.rect.h - 2.0).max(0.0),
				},
				Paint::Styled(
					Condition::Button,
					if interaction.pressed == Some(button.action) {
						C::ActiveBackground
					} else {
						C::Background
					},
				),
			));
		} else if button
			.rect
			.contains(interaction.cursor.0, interaction.cursor.1)
		{
			out.push(Draw::Rect(
				button.rect,
				Paint::Styled(Condition::Button, C::HoverBackground),
			));
		} else {
			out.push(Draw::Rect(
				button.rect,
				Paint::Styled(Condition::Button, C::Background),
			));
		}
		let label_x = button.rect.x
			+ (button.rect.w - shaper.text_width(button.label, 13.0)) / 2.0;
		out.extend(shaper.label(
			button.label,
			13.0,
			label_x,
			button.rect.y + button.rect.h / 2.0 + 5.0,
			Paint::Styled(Condition::Button, C::Color),
		));
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::path::PathBuf;
	#[test]
	fn modal_offers_all_three_answers_inside_its_panel() {
		let mut shaper = TextShaper::new();
		let interaction = InteractionState {
			modal: Some(Modal::OpenLocal {
				path: PathBuf::from("/tmp/archive/untrusted.pdf"),
				dir: PathBuf::from("/tmp/archive"),
				document_dir: None,
			}),
			..Default::default()
		};
		for (width, height) in [(600.0, 400.0), (1200.0, 800.0)] {
			let rect = modal_rect(width, height);
			let buttons =
				modal_buttons(&mut shaper, &interaction, width, height);
			for action in [
				Command::ModalOpenFolder,
				Command::ModalConfirm,
				Command::ModalDismiss,
			] {
				assert!(buttons.iter().any(|b| b.action == action));
			}
			for button in buttons {
				assert!(
					rect.contains(button.rect.x, button.rect.y),
					"{:?}",
					button.rect
				);
				assert!(rect.contains(
					button.rect.x + button.rect.w,
					button.rect.y + button.rect.h
				));
			}
		}
	}
	#[test]
	fn an_empty_interaction_has_no_modal_buttons() {
		let mut shaper = TextShaper::new();
		let interaction = InteractionState::default();
		assert!(
			modal_buttons(&mut shaper, &interaction, 800.0, 600.0).is_empty()
		);
		assert!(draw_modal(&mut shaper, &interaction, 800.0, 600.0).is_empty());
	}
	#[test]
	fn a_long_path_loses_its_front_and_keeps_the_file_name() {
		let mut shaper = TextShaper::new();
		// Built by joining so every separator is the platform's own.
		let path: PathBuf =
			["home", "someone", "Downloads", "archive", "nested"]
				.iter()
				.collect::<PathBuf>()
				.join("payload.desktop");
		// Plenty of room: nothing is removed.
		assert_eq!(
			visible_path(&mut shaper, &path, None, 5000.0),
			path.display().to_string()
		);
		// Narrow: the file name and its nearest directory survive, and the
		// marker can never be mistaken for a parent directory. The limit is
		// measured, so the test does not depend on a font's exact metrics.
		let sep = std::path::MAIN_SEPARATOR;
		let expected = format!("{MARKER}{sep}nested{sep}payload.desktop");
		let max = shaper.text_width(&expected, PATH_SIZE) + 1.0;
		assert_eq!(visible_path(&mut shaper, &path, None, max), expected);
		// A file name too long to fit loses its own front, not its extension.
		let name =
			PathBuf::from("a-file-name-longer-than-the-whole-panel-x.pdf");
		let expected = format!("{MARKER}.pdf");
		let max = shaper.text_width(&expected, PATH_SIZE) + 1.0;
		assert_eq!(visible_path(&mut shaper, &name, None, max), expected);
		// The home directory is one glyph, so more of the tail fits.
		if let Some(home) = home_dir() {
			let under = home.join("Downloads/payload.desktop");
			assert_eq!(
				visible_path(&mut shaper, &under, None, 5000.0),
				"~/Downloads/payload.desktop"
			);
		}
	}
	#[test]
	fn a_shorter_path_relative_to_the_document_wins() {
		let mut shaper = TextShaper::new();
		let document_dir =
			PathBuf::from("/home/someone/Documents/notes/reading");
		let sibling = document_dir.join("targets/payload.desktop");
		assert_eq!(
			visible_path(&mut shaper, &sibling, Some(&document_dir), 5000.0),
			format!("./targets{0}payload.desktop", std::path::MAIN_SEPARATOR)
		);
		let up = document_dir.join("../archive/payload.desktop");
		assert_eq!(
			visible_path(&mut shaper, &up, Some(&document_dir), 5000.0),
			format!(
				"..{0}archive{0}payload.desktop",
				std::path::MAIN_SEPARATOR
			)
		);
		// A far away target stays absolute, because that is shorter.
		let far = PathBuf::from("/etc/init.d/payload.desktop");
		let shown =
			visible_path(&mut shaper, &far, Some(&document_dir), 5000.0);
		assert!(shown.starts_with("..") || shown.starts_with('/'), "{shown}");
		// The relative form is still elided from the front when it is long.
		let deep = document_dir
			.join("a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/payload.desktop");
		let narrow =
			visible_path(&mut shaper, &deep, Some(&document_dir), 150.0);
		assert!(narrow.starts_with(MARKER), "{narrow}");
		assert!(narrow.ends_with("payload.desktop"), "{narrow}");
	}
}
