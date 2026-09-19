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
	Latency,
	Smoke,
	Pdf,
}
impl Mode {
	/// A static image or a sheet of paper cannot be scrolled sideways, so code
	/// blocks wrap by default whenever a mode writes one.
	pub(crate) fn wraps_code_blocks(&self) -> bool {
		matches!(self, Self::Render | Self::Smoke | Self::Pdf)
	}
}

/// The document metadata the command line writes into the PDF.
#[derive(Default, Clone)]
pub(crate) struct MetadataOverrides {
	pub(crate) title: Option<String>,
	pub(crate) authors: Vec<String>,
	pub(crate) subject: Option<String>,
	pub(crate) keywords: Vec<String>,
	pub(crate) language: Option<String>,
	pub(crate) creator: Option<String>,
}

/// The `[page]` fields the command line overrides on top of the stylesheet.
#[derive(Default)]
pub(crate) struct PageOverrides {
	pub(crate) paper: Option<String>,
	pub(crate) landscape: bool,
	pub(crate) margin: Option<[f32; 4]>,
	/// Header and footer slots, left to centre to right.
	pub(crate) header: [Option<String>; 3],
	pub(crate) footer: [Option<String>; 3],
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
	pub(crate) validate: Option<PathBuf>,
	pub(crate) iterations: usize,
	pub(crate) options: LayoutOptions,
	pub(crate) overrides: Vec<Setting>,
	pub(crate) page: PageOverrides,
	pub(crate) metadata: MetadataOverrides,
	pub(crate) links: bool,
	pub(crate) watch: bool,
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
			validate: None,
			iterations: 100,
			options: LayoutOptions::default(),
			overrides: Vec::new(),
			page: PageOverrides::default(),
			metadata: MetadataOverrides::default(),
			links: true,
			watch: false,
		}
	}
}

pub(crate) fn arguments() -> Result<Option<LaunchOptions>> {
	parse_arguments(std::env::args_os().skip(1))
}

/// Whether `value` reads as an RFC 3066 language tag: letters, digits, and
/// hyphens, starting and ending with an alphanumeric subtag.
fn is_language_tag(value: &str) -> bool {
	!value.is_empty()
		&& value.len() <= 64
		&& !value.starts_with('-')
		&& !value.ends_with('-')
		&& !value.contains("--")
		&& value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
		&& value
			.chars()
			.next()
			.is_some_and(|c| c.is_ascii_alphabetic())
}
fn parse_arguments(
	args: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<Option<LaunchOptions>> {
	let mut out = LaunchOptions::default();
	let mut args = args.into_iter().peekable();
	if args.peek().is_some_and(|a| a == "ss") {
		args.next();
		match args.next().as_deref() {
			Some(sub) if sub == "install" => {
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
			}
			Some(sub) if sub == "validate" => {
				let path = args.next().map(PathBuf::from);
				if args.next().is_some() {
					bail!("Validate one stylesheet at a time");
				}
				out.validate = Some(
					path.context("ss validate requires a stylesheet path")?,
				);
			}
			_ => bail!(
				"Usage: markview ss install FILE.mvss.toml [--force]\n       markview ss validate FILE.mvss.toml"
			),
		}
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
				crate::logging::report(format_args!(
					"Markview — native Markdown reading\n\nmarkview [FILE] [--style ID ...]\nmarkview ss install FILE.mvss.toml [--force]\nmarkview ss validate FILE.mvss.toml\nmarkview --render FILE --output preview.png [--dark] [--scale 2]\nmarkview --pdf FILE --output out.pdf [--paper a4] [--landscape] [--watch]\nmarkview --bench FILE [--iterations 100] [--output metrics.json]\nmarkview --bench-latency FILE [--iterations 100] [--output latency.json]\nmarkview --smoke-test FILE [--output window.png]\n\nOptions: --width N --height N --column N --font-size N --scroll N\n         --paragraph-indent N --cjk-type SC|TC|JP|none --scale N\n         --style ID --dark --light --left --no-hyphens --greedy --offline\n\nFonts: --fonts DIR adds a directory of font files and may be repeated.\n       --ignore-system-fonts shapes with those directories alone, so the\n       fonts installed on the machine cannot change the result.\n\nPDF: --paper a4|a5|letter|legal|WIDTHxHEIGHT --landscape --margin MM[,MM...]\n     --header TEXT --header-left/right TEXT --footer TEXT --footer-left/right TEXT\n     --no-links --watch; --watch re-exports whenever the document or one of its\n     local images changes, until you stop it. The slots take {{page}} {{pages}}\n     {{title}} and {{path}}. The export always starts from the bundled print\n     stylesheet, and --style layers on it.\n\nMetadata: --title TEXT --author NAME (repeatable) --subject TEXT\n          --keywords A,B --language TAG --creator TEXT\n          A title defaults to the first heading, then the file name;\n          Producer stays Markview <version>, and no creation date is\n          ever written.\n\nImages: local files, file:, http(s): and data: URIs; bitmap and SVG.\n        An image alone in its block is centered, otherwise it is inline.\n        Animated images show their first frame; --offline blocks the network.\n\nKeyboard: Ctrl+O open · Ctrl+T styles · Ctrl+ +/- font size\n          Ctrl+[ / ] column width · Ctrl+L alignment · Ctrl+H hyphenation\n          arrows / PageUp / PageDown / Home / End scroll\n          drag the scrollbar · Shift+wheel scroll wide blocks · Tab/Enter toolbar\n          click web/mail/local links; local .md links open in a new tab\n          click a footnote reference to reach its note and its number to return\n          drag / Shift+click select · Ctrl+A all · Ctrl+C copy · Ctrl+, settings\n\n--render and --bench use the real GPU pipeline offscreen.\n--greedy is a typography comparison mode."
				));
				return Ok(None);
			}
			"--render" => out.mode = Mode::Render,
			"--offline" => out.offline = true,
			"--bench" => out.mode = Mode::Bench,
			"--bench-latency" => out.mode = Mode::Latency,
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
			"--pdf" => out.mode = Mode::Pdf,
			"--landscape" => out.page.landscape = true,
			"--no-links" => out.links = false,
			"--watch" => out.watch = true,
			"--paper" => {
				let value = args
					.next()
					.context("--paper requires a size")?
					.to_string_lossy()
					.into_owned();
				if markview_core::style::parse_paper_size(&value).is_none() {
					bail!(
						"Invalid paper {value:?}; use a4, a5, letter, legal or WIDTHxHEIGHT in millimetres"
					);
				}
				out.page.paper = Some(value);
			}
			"--margin" => {
				let value = args
					.next()
					.context("--margin requires millimetres")?
					.to_string_lossy()
					.into_owned();
				let numbers: Vec<f32> = value
					.split([',', ' ', '\t'])
					.filter(|part| !part.is_empty())
					.map(|part| part.parse::<f32>())
					.collect::<Result<_, _>>()
					.context("Invalid margin")?;
				let margin = match numbers.as_slice() {
					[all] => [*all; 4],
					[vertical, horizontal] => {
						[*vertical, *horizontal, *vertical, *horizontal]
					}
					[top, right, bottom, left] => {
						[*top, *right, *bottom, *left]
					}
					_ => bail!(
						"--margin takes 1, 2 or 4 millimetres: --margin 20,25"
					),
				};
				if margin
					.iter()
					.any(|value| !value.is_finite() || *value < 0.0)
				{
					bail!("Invalid margin");
				}
				out.page.margin = Some(margin);
			}
			"--header" | "--header-left" | "--header-right" | "--footer"
			| "--footer-left" | "--footer-right" => {
				let value = args
					.next()
					.with_context(|| format!("{text} requires text"))?
					.to_string_lossy()
					.into_owned();
				if !markview_core::paginate::template_is_valid(&value) {
					bail!(
						"{text}: unknown placeholder; use {{page}}, {{pages}}, {{title}} or {{path}}"
					);
				}
				let (header, slot) = match text.as_ref() {
					"--header" => (true, 1),
					"--header-left" => (true, 0),
					"--header-right" => (true, 2),
					"--footer" => (false, 1),
					"--footer-left" => (false, 0),
					_ => (false, 2),
				};
				if header {
					out.page.header[slot] = Some(value);
				} else {
					out.page.footer[slot] = Some(value);
				}
			}
			"--left" => out.options.justify = false,
			"--no-hyphens" => out.options.hyphenate = false,
			"--greedy" => out.options.greedy = true,
			"--title" | "--subject" | "--language" | "--creator" => {
				let value = args
					.next()
					.with_context(|| format!("{text} requires text"))?
					.to_string_lossy()
					.into_owned();
				match text.as_ref() {
					"--title" => out.metadata.title = Some(value),
					"--subject" => out.metadata.subject = Some(value),
					"--language" => {
						if !is_language_tag(&value) {
							bail!(
								"Invalid language {value:?}; use an RFC 3066 tag such as en, zh-CN or ja"
							);
						}
						out.metadata.language = Some(value);
					}
					_ => out.metadata.creator = Some(value),
				}
			}
			// An author is one person, so a repeated flag writes a list; a
			// keyword list reads better comma-separated inside one flag.
			"--author" => {
				let value = args
					.next()
					.context("--author requires a name")?
					.to_string_lossy()
					.into_owned();
				if !value.trim().is_empty() {
					out.metadata.authors.push(value);
				}
			}
			"--keywords" => {
				let value = args
					.next()
					.context("--keywords requires a list")?
					.to_string_lossy()
					.into_owned();
				out.metadata.keywords.extend(
					value
						.split(',')
						.map(|word| word.trim())
						.filter(|word| !word.is_empty())
						.map(str::to_owned),
				);
			}
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
			"--fonts" => {
				let value =
					args.next().context("--fonts requires a directory")?;
				let directory = PathBuf::from(value);
				if !directory.is_dir() {
					bail!(
						"--fonts: {} is not a directory",
						directory.display()
					);
				}
				out.options.fonts.directories.push(directory);
			}
			"--ignore-system-fonts" => {
				out.options.fonts.ignore_system_fonts = true
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
	if out.mode == Mode::Pdf {
		if out.output.is_none() {
			bail!("--pdf requires --output out.pdf");
		}
		if out.theme.is_some() {
			bail!(
				"--pdf prints the sheet of paper, not the window; use --style to change its colors"
			);
		}
		if out.watch
			&& out
				.path
				.as_deref()
				.zip(out.output.as_deref())
				.is_some_and(|(path, output)| same_target(path, output))
		{
			bail!("--watch cannot write the PDF over the document it watches");
		}
	} else if out.watch {
		bail!("--watch re-exports on every save; it applies to --pdf");
	}
	Ok(Some(out))
}

/// Whether two paths name the same file once the filesystem resolves them:
/// absolute and relative spellings, `.` and `..`, and directory symlinks all
/// collapse to one answer, so an output cannot overwrite its own document.
fn same_target(a: &std::path::Path, b: &std::path::Path) -> bool {
	resolved(a) == resolved(b)
}

/// The real path `path` names, resolving symlinks and `.`/`..` for every
/// component the filesystem can. A path that does not exist yet, which is the
/// normal case for an output, is resolved through its nearest existing
/// ancestor; a tail that is missing anywhere is folded lexically.
fn resolved(path: &std::path::Path) -> std::path::PathBuf {
	use std::path::Component;
	let absolute = if path.is_absolute() {
		path.to_owned()
	} else {
		std::env::current_dir().unwrap_or_default().join(path)
	};
	// Fold the tail into the nearest ancestor the filesystem can resolve. A
	// `..` must not stop the search: macOS spells its temporary directory
	// through the `/var` symlink, so canonicalizing the prefix even when the
	// missing tail steps back is what makes `/var/...` and `/private/var/...`
	// one path.
	let mut tail: Vec<Component> = Vec::new();
	let mut at = absolute.as_path();
	loop {
		if let Ok(real) = std::fs::canonicalize(at) {
			let mut out = real;
			for part in tail.iter().rev() {
				match part {
					Component::CurDir => {}
					Component::ParentDir => {
						out.pop();
					}
					other => out.push(other.as_os_str()),
				}
			}
			return normalize(&out);
		}
		match at.components().next_back() {
			Some(part @ (Component::ParentDir | Component::Normal(_))) => {
				tail.push(part);
				at = at.parent().unwrap_or(std::path::Path::new(""));
			}
			_ => return normalize(&absolute),
		}
	}
}

/// Lexical `.`/`..` folding, for the part of a path that does not exist.
fn normalize(path: &std::path::Path) -> std::path::PathBuf {
	use std::path::Component;
	let mut out = std::path::PathBuf::new();
	for part in path.components() {
		match part {
			Component::CurDir => {}
			Component::ParentDir => {
				out.pop();
			}
			other => out.push(other.as_os_str()),
		}
	}
	out
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
	fn font_flags_choose_which_faces_are_loaded() {
		let first = tempfile::tempdir().unwrap();
		let second = tempfile::tempdir().unwrap();
		let args = parse_arguments(
			[
				"--pdf",
				"a.md",
				"-o",
				"a.pdf",
				"--fonts",
				first.path().to_str().unwrap(),
				"--fonts",
				second.path().to_str().unwrap(),
				"--ignore-system-fonts",
			]
			.map(Into::into),
		)
		.unwrap()
		.unwrap();
		assert!(args.options.fonts.ignore_system_fonts);
		assert_eq!(
			args.options.fonts.directories,
			vec![first.path().to_path_buf(), second.path().to_path_buf()]
		);
		// These choose faces, they do not override a reader setting.
		assert!(args.overrides.is_empty());
		// A directory that does not exist can never supply a face.
		assert!(
			parse_arguments(
				["--fonts", "does-not-exist-anywhere"].map(Into::into)
			)
			.is_err()
		);
		assert!(parse_arguments(["--fonts"].map(Into::into)).is_err());
	}
	#[test]
	fn only_image_export_modes_wrap_code_blocks_by_default() {
		assert!(Mode::Render.wraps_code_blocks());
		assert!(Mode::Smoke.wraps_code_blocks());
		assert!(Mode::Pdf.wraps_code_blocks());
		assert!(!Mode::Window.wraps_code_blocks());
		assert!(!Mode::Bench.wraps_code_blocks());
	}
	#[test]
	fn pdf_mode_takes_a_page_a_path_and_furniture_text() {
		let args = parse_arguments(
			[
				"--pdf",
				"sample.md",
				"--output",
				"sample.pdf",
				"--paper",
				"letter",
				"--landscape",
				"--margin",
				"10,15",
				"--header-left",
				"Draft",
				"--footer",
				"{page}/{pages}",
				"--no-links",
			]
			.map(Into::into),
		)
		.unwrap()
		.unwrap();
		assert!(args.mode == Mode::Pdf);
		assert_eq!(args.page.paper.as_deref(), Some("letter"));
		assert!(args.page.landscape);
		assert_eq!(args.page.margin, Some([10.0, 15.0, 10.0, 15.0]));
		assert_eq!(args.page.header[0].as_deref(), Some("Draft"));
		assert_eq!(args.page.header[1], None);
		assert_eq!(args.page.footer[1].as_deref(), Some("{page}/{pages}"));
		assert!(!args.links);

		// `--header` fills the centre slot.
		let args = parse_arguments(
			["--pdf", "a.md", "-o", "a.pdf", "--header", "{title}"]
				.map(Into::into),
		)
		.unwrap()
		.unwrap();
		assert_eq!(args.page.header[1].as_deref(), Some("{title}"));
	}
	#[test]
	fn pdf_metadata_flags_take_lists_and_reject_bad_tags() {
		let args = parse_arguments(
			[
				"--pdf",
				"a.md",
				"-o",
				"a.pdf",
				"--title",
				"A paper",
				"--author",
				"Ada",
				"--author",
				"Grace",
				"--subject",
				"Testing",
				"--keywords",
				"markdown, typography",
				"--language",
				"zh-CN",
				"--creator",
				"Editor",
			]
			.map(Into::into),
		)
		.unwrap()
		.unwrap();
		assert_eq!(args.metadata.title.as_deref(), Some("A paper"));
		assert_eq!(args.metadata.authors, ["Ada", "Grace"]);
		assert_eq!(args.metadata.subject.as_deref(), Some("Testing"));
		assert_eq!(args.metadata.keywords, ["markdown", "typography"]);
		assert_eq!(args.metadata.language.as_deref(), Some("zh-CN"));
		assert_eq!(args.metadata.creator.as_deref(), Some("Editor"));

		// A missing value is an error, not an empty field.
		for bad in [
			&["--pdf", "a.md", "-o", "a.pdf", "--title"][..],
			&["--pdf", "a.md", "-o", "a.pdf", "--author"],
			&["--pdf", "a.md", "-o", "a.pdf", "--keywords"],
			&["--pdf", "a.md", "-o", "a.pdf", "--language"],
			&["--pdf", "a.md", "-o", "a.pdf", "--language", "-x"],
			&["--pdf", "a.md", "-o", "a.pdf", "--language", "zh--CN"],
		] {
			assert!(
				parse_arguments(bad.iter().map(Into::into)).is_err(),
				"{bad:?}"
			);
		}
		// Empty entries add nothing rather than empty metadata.
		let args = parse_arguments(
			[
				"--pdf",
				"a.md",
				"-o",
				"a.pdf",
				"--author",
				" ",
				"--keywords",
				"a,,b",
			]
			.map(Into::into),
		)
		.unwrap()
		.unwrap();
		assert!(args.metadata.authors.is_empty());
		assert_eq!(args.metadata.keywords, ["a", "b"]);
	}
	#[test]
	fn pdf_mode_needs_an_output_and_rejects_reader_theme_flags() {
		assert!(
			parse_arguments(["--pdf", "sample.md"].map(Into::into)).is_err()
		);
		assert!(
			parse_arguments(
				["--pdf", "sample.md", "-o", "a.pdf", "--dark"].map(Into::into)
			)
			.is_err()
		);
		for bad in [
			["--pdf", "a.md", "-o", "a.pdf", "--paper", "tabloidish"],
			["--pdf", "a.md", "-o", "a.pdf", "--paper", "0x0"],
			["--pdf", "a.md", "-o", "a.pdf", "--margin", "1,2,3"],
			["--pdf", "a.md", "-o", "a.pdf", "--margin", "-1"],
			["--pdf", "a.md", "-o", "a.pdf", "--footer", "{date}"],
			["--pdf", "a.md", "-o", "a.pdf", "--header", "{page"],
		] {
			assert!(
				parse_arguments(bad.iter().map(Into::into)).is_err(),
				"{bad:?}"
			);
		}
	}
	#[test]
	fn watch_re_exports_a_pdf_and_never_over_its_own_document() {
		let args = parse_arguments(
			["--pdf", "a.md", "-o", "a.pdf", "--watch"].map(Into::into),
		)
		.unwrap()
		.unwrap();
		assert!(args.watch);
		assert!(
			!parse_arguments(["--pdf", "a.md", "-o", "a.pdf"].map(Into::into))
				.unwrap()
				.unwrap()
				.watch
		);
		for bad in [
			&["--watch", "a.md"][..],
			&["--render", "a.md", "-o", "a.png", "--watch"],
			&["--pdf", "a.md", "-o", "a.md", "--watch"],
			&["--pdf", "a.md", "-o", "./a.md", "--watch"],
			&["--pdf", "a.md", "--watch"],
		] {
			assert!(
				parse_arguments(bad.iter().map(Into::into)).is_err(),
				"{bad:?}"
			);
		}
		assert!(same_target(
			std::path::Path::new("a.md"),
			std::path::Path::new("./a.md")
		));
		assert!(!same_target(
			std::path::Path::new("a.md"),
			std::path::Path::new("b.md")
		));
	}
	#[test]
	fn same_target_resolves_equivalent_paths_to_one_file() {
		let dir = tempfile::tempdir().unwrap();
		let file = dir.path().join("a.md");
		std::fs::write(&file, "x").unwrap();
		// A missing component folds lexically, so an unnormalized spelling of
		// the same file is refused too.
		assert!(same_target(&file, &dir.path().join("./a.md")));
		assert!(same_target(&file, &dir.path().join("sub/../a.md")));
		assert!(!same_target(&file, &dir.path().join("b.md")));
		// A relative spelling of a missing path still lands in one place.
		assert!(same_target(
			std::path::Path::new("missing.md"),
			std::path::Path::new("./missing.md")
		));
		#[cfg(unix)]
		{
			let link = dir.path().join("link");
			std::os::unix::fs::symlink(dir.path(), &link).unwrap();
			assert!(same_target(&file, &link.join("a.md")));
			// A `..` in the missing tail must not stop the prefix from being
			// canonicalized, or a symlinked ancestor such as macOS's `/var`
			// leaves the two spellings on different paths.
			assert!(same_target(&file, &link.join("sub/../a.md")));
		}
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
	#[test]
	fn latency_benchmark_needs_a_path_and_keeps_its_arguments() {
		let args = parse_arguments(
			["--bench-latency", "sample.md", "--iterations", "7"]
				.map(Into::into),
		)
		.unwrap()
		.unwrap();
		assert!(args.mode == Mode::Latency);
		assert_eq!(args.iterations, 7);
		assert!(!args.mode.wraps_code_blocks());
		assert!(parse_arguments(["--bench-latency"].map(Into::into)).is_err());
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
		let args =
			parse_arguments(["ss", "validate", "a.mvss.toml"].map(Into::into))
				.unwrap()
				.unwrap();
		assert_eq!(args.validate, Some(PathBuf::from("a.mvss.toml")));
		assert!(args.install.is_none());
		assert!(args.path.is_none());
		for bad in [
			&["ss", "validate"][..],
			&["ss", "validate", "a.mvss.toml", "b.mvss.toml"],
			&["ss", "polish", "a.mvss.toml"],
		] {
			assert!(
				parse_arguments(bad.iter().map(Into::into)).is_err(),
				"{bad:?}"
			);
		}
	}
}
