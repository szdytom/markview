//! Launch parsing; deterministic diagnostic modes do not load user settings.
use crate::{layout::LayoutOptions, render::Theme, settings::Setting};
use anyhow::{Context, Result, bail};
use std::path::PathBuf;
#[derive(Default, PartialEq, Eq)]
pub(crate) enum Mode {
	#[default]
	Window,
	Render,
	Bench,
	Smoke,
}
impl Mode {
	/// A static image cannot be scrolled sideways, so code blocks wrap by
	/// default whenever a mode writes one.
	pub(crate) fn exports_image(&self) -> bool {
		matches!(self, Self::Render | Self::Smoke)
	}
}
pub(crate) struct LaunchOptions {
	pub(crate) offline: bool,
	pub(crate) mode: Mode,
	pub(crate) path: Option<PathBuf>,
	pub(crate) output: Option<PathBuf>,
	pub(crate) width: u32,
	pub(crate) height: u32,
	pub(crate) scale: f32,
	pub(crate) scroll: f32,
	pub(crate) theme: Option<Theme>,
	pub(crate) style: Option<Vec<String>>,
	pub(crate) cjk_type: Option<markview_core::style::CjkType>,
	pub(crate) install: Option<(PathBuf, bool)>,
	pub(crate) iterations: usize,
	pub(crate) options: LayoutOptions,
	pub(crate) overrides: Vec<Setting>,
}
impl Default for LaunchOptions {
	fn default() -> Self {
		Self {
			offline: false,
			mode: Mode::Window,
			path: None,
			output: None,
			width: 1200,
			height: 800,
			scale: 1.0,
			scroll: 0.0,
			theme: None,
			style: None,
			cjk_type: None,
			install: None,
			iterations: 100,
			options: LayoutOptions::default(),
			overrides: Vec::new(),
		}
	}
}

pub(crate) fn arguments() -> Result<Option<LaunchOptions>> {
	parse_arguments(std::env::args_os().skip(1))
}
fn parse_arguments(
	args: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<Option<LaunchOptions>> {
	let mut out = LaunchOptions::default();
	let mut args = args.into_iter().peekable();
	if args.peek().is_some_and(|a| a == "ss") {
		args.next();
		if args.next().as_deref() != Some(std::ffi::OsStr::new("install")) {
			bail!("Usage: markview ss install FILE.mvss.toml [--force]");
		}
		let mut path = None;
		let mut force = false;
		for arg in args {
			if arg == "--force" {
				force = true;
			} else if path.is_none() {
				path = Some(PathBuf::from(arg));
			} else {
				bail!("Install one stylesheet at a time");
			}
		}
		out.install = Some((
			path.context("ss install requires a stylesheet path")?,
			force,
		));
		return Ok(Some(out));
	}
	while let Some(arg) = args.next() {
		let text = arg.to_string_lossy();
		match text.as_ref() {
			"--dark" | "--light" | "--style" => {
				out.overrides.push(Setting::Theme)
			}
			"--font-size" => out.overrides.push(Setting::FontSize),
			"--column" => out.overrides.push(Setting::Width),
			"--left" => out.overrides.push(Setting::Justify),
			"--no-hyphens" => out.overrides.push(Setting::Hyphenate),
			"--paragraph-indent" => {
				out.overrides.push(Setting::ParagraphIndent)
			}
			"--cjk-type" => out.overrides.push(Setting::CjkType),
			_ => {}
		}
		match text.as_ref() {
			"-h" | "--help" => {
				println!(
					"Markview — native Markdown reading\n\nmarkview [FILE] [--style ID ...]\nmarkview ss install FILE.mvss.toml [--force]\nmarkview --render FILE --output preview.png [--dark] [--scale 2]\nmarkview --bench FILE [--iterations 100] [--output metrics.json]\nmarkview --smoke-test FILE [--output window.png]\n\nOptions: --width N --height N --column N --font-size N --scroll N\n         --paragraph-indent N --cjk-type SC|TC|JP|none --scale N\n         --style ID --dark --light --left --no-hyphens --greedy --offline\n\nImages: local files, file:, http(s): and data: URIs; bitmap and SVG.\n        An image alone in its block is centered, otherwise it is inline.\n        Animated images show their first frame; --offline blocks the network.\n\nKeyboard: Ctrl+O open · Ctrl+T styles · Ctrl+ +/- font size\n          Ctrl+[ / ] column width · Ctrl+L alignment · Ctrl+H hyphenation\n          arrows / PageUp / PageDown / Home / End scroll\n          drag the scrollbar · Shift+wheel scroll wide blocks · Tab/Enter toolbar\n          click web/mail/local links; local .md links open in a new tab\n          click a footnote reference to reach its note and its number to return\n          drag / Shift+click select · Ctrl+A all · Ctrl+C copy · Ctrl+, settings\n\n--render and --bench use the real GPU pipeline offscreen.\n--greedy is a typography comparison mode."
				);
				return Ok(None);
			}
			"--render" => out.mode = Mode::Render,
			"--offline" => out.offline = true,
			"--bench" => out.mode = Mode::Bench,
			"--smoke-test" => out.mode = Mode::Smoke,
			"--output" | "-o" => {
				out.output = Some(
					args.next().context("--output requires a path")?.into(),
				)
			}
			"--style" => {
				let id = args
					.next()
					.context("--style requires an ID")?
					.into_string()
					.map_err(|_| {
						anyhow::anyhow!("Stylesheet ID must be UTF-8")
					})?;
				crate::stylesheet::validate_id(&id)?;
				out.style.get_or_insert_with(Vec::new).push(id);
			}
			"--dark" => out.theme = Some(Theme::Dark),
			"--light" => out.theme = Some(Theme::Light),
			"--left" => out.options.justify = false,
			"--no-hyphens" => out.options.hyphenate = false,
			"--greedy" => out.options.greedy = true,
			"--cjk-type" => {
				let value =
					args.next().context("--cjk-type requires a name")?;
				let name = value.to_string_lossy();
				out.cjk_type = Some(
					markview_core::style::CjkType::from_name(&name)
						.with_context(|| {
							format!(
								"Invalid CJK type {name}; use SC, TC, JP or none"
							)
						})?,
				);
			}
			"--width" | "--height" | "--column" | "--font-size" | "--scale"
			| "--scroll" | "--iterations" | "--paragraph-indent" => {
				let value = args
					.next()
					.with_context(|| format!("{text} requires a number"))?;
				let number: f32 = value
					.to_string_lossy()
					.parse()
					.context("Invalid number")?;
				if !number.is_finite() || number < 0.0 {
					bail!("Invalid value for {text}");
				}
				match text.as_ref() {
					"--width" => out.width = (number as u32).clamp(320, 8192),
					"--height" => out.height = (number as u32).clamp(240, 8192),
					"--column" => {
						out.options.width = number.clamp(240.0, 1600.0)
					}
					"--font-size" => {
						out.options.font_size = number.clamp(10.0, 40.0)
					}
					"--scale" => out.scale = number.clamp(0.5, 4.0),
					"--scroll" => out.scroll = number,
					"--paragraph-indent" => {
						out.options.paragraph_indent = number.clamp(0.0, 4.0)
					}
					"--iterations" => {
						out.iterations = (number as usize).clamp(1, 10_000)
					}
					_ => {}
				}
			}
			"--" => {
				out.path = args.next().map(PathBuf::from);
				if args.next().is_some() {
					bail!("Open one document at a time");
				}
				break;
			}
			_ if text.starts_with('-') => {
				bail!("Unknown option {text}; use --help")
			}
			_ => {
				if out.path.is_some() {
					bail!("Open one document at a time");
				}
				out.path = Some(arg.into());
			}
		}
	}
	if out.style.is_some() && out.theme.is_some() {
		bail!("--style conflicts with --light and --dark");
	}
	if out.mode != Mode::Window && out.path.is_none() {
		bail!("This mode requires a Markdown file");
	}
	if out.mode == Mode::Render && out.output.is_none() {
		bail!("--render requires --output preview.png");
	}
	Ok(Some(out))
}

#[cfg(test)]
mod tests {
	use super::*;
	use markview_core::style::CjkType;
	#[test]
	fn cjk_type_is_parsed_case_insensitively_and_overrides_the_file() {
		for (name, expected) in [
			("SC", CjkType::Sc),
			("tc", CjkType::Tc),
			("Jp", CjkType::Jp),
			("none", CjkType::None),
		] {
			let args = parse_arguments(
				["--cjk-type", name, "sample.md"].map(Into::into),
			)
			.unwrap()
			.unwrap();
			assert_eq!(args.cjk_type, Some(expected), "{name}");
			assert_eq!(args.overrides, vec![Setting::CjkType], "{name}");
		}
		assert!(
			parse_arguments(
				["--cjk-type", "klingon", "sample.md"].map(Into::into)
			)
			.is_err()
		);
		assert!(parse_arguments(["--cjk-type"].map(Into::into)).is_err());
	}
	#[test]
	fn only_image_export_modes_wrap_code_blocks_by_default() {
		assert!(Mode::Render.exports_image());
		assert!(Mode::Smoke.exports_image());
		assert!(!Mode::Window.exports_image());
		assert!(!Mode::Bench.exports_image());
	}
	#[test]
	fn explicit_settings_and_headless_mode_are_distinct() {
		let args = parse_arguments(
			[
				"--render",
				"sample.md",
				"--output",
				"sample.png",
				"--dark",
				"--font-size",
				"23",
			]
			.map(Into::into),
		)
		.unwrap()
		.unwrap();
		assert!(args.mode == Mode::Render);
		assert_eq!(args.overrides, vec![Setting::Theme, Setting::FontSize]);
		assert_eq!(args.options.font_size, 23.0);
		let args = parse_arguments(["sample.md"].map(Into::into))
			.unwrap()
			.unwrap();
		assert!(args.overrides.is_empty());
		let args = parse_arguments(
			["--paragraph-indent", "2", "sample.md"].map(Into::into),
		)
		.unwrap()
		.unwrap();
		assert_eq!(args.overrides, vec![Setting::ParagraphIndent]);
		assert_eq!(args.options.paragraph_indent, 2.0);
		assert!(
			parse_arguments(["--paragraph-indent", "-1"].map(Into::into))
				.is_err()
		);
		assert!(
			parse_arguments(["--font-size", "NaN"].map(Into::into)).is_err()
		);
	}
}

#[cfg(test)]
mod stylesheet_tests {
	use super::*;
	#[test]
	fn stylesheet_arguments_are_ordered_and_install_is_independent() {
		let args = parse_arguments(
			[
				"--render",
				"sample.md",
				"--output",
				"/tmp/sample.png",
				"--style",
				"a",
				"--style",
				"dark",
			]
			.map(Into::into),
		)
		.unwrap()
		.unwrap();
		assert_eq!(args.style, Some(vec!["a".into(), "dark".into()]));
		for flags in [
			["--style", "a", "--dark"],
			["--light", "--style", "a"],
			["--style", "../a", "sample.md"],
		] {
			assert!(parse_arguments(flags.map(Into::into)).is_err());
		}
		let args = parse_arguments(
			["ss", "install", "a.mvss.toml", "--force"].map(Into::into),
		)
		.unwrap()
		.unwrap();
		assert_eq!(args.install, Some((PathBuf::from("a.mvss.toml"), true)));
		assert!(args.path.is_none());
	}
}
