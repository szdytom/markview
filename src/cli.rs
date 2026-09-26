//! Launch parsing; deterministic diagnostic modes do not load user settings.
//!
//! Every mode is a subcommand and the reader window is what a bare invocation
//! opens, so `markview notes.md` still reads a document while `markview render
//! notes.md --output preview.png` names what it does.
use crate::{
	export::{MetadataOverrides, PageOverrides, PdfRequest},
	layout::LayoutOptions,
	render::Theme,
	settings::{ExportSettings, Setting},
};
use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use std::{ffi::OsString, path::PathBuf};

#[derive(Default, PartialEq, Eq)]
pub(crate) enum Mode {
	#[default]
	Window,
	Render,
	Bench,
	Latency,
	Smoke,
	Pdf,
	StylesheetList,
	Serve,
	Fonts,
}
impl Mode {
	/// A static image or a sheet of paper cannot be scrolled sideways, so code
	/// blocks wrap by default whenever a mode writes one.
	pub(crate) fn wraps_code_blocks(&self) -> bool {
		matches!(self, Self::Render | Self::Smoke | Self::Pdf)
	}
	/// A mode that draws an artifact — the reader, a PNG or a sheet of paper —
	/// shapes with the personal download directory too, so an export matches
	/// what the reader shows. Measurement runs keep the pinned set, so a
	/// download cannot move a benchmark.
	pub(crate) fn uses_personal_fonts(&self) -> bool {
		matches!(self, Self::Window | Self::Render | Self::Smoke | Self::Pdf)
	}
}

/// What `markview fonts` was asked to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FontsCommand {
	/// List the declared families and what the download directory holds.
	List {
		style: Option<String>,
		file: Option<PathBuf>,
		all: bool,
	},
	/// Download the named families, or every missing one.
	Download {
		style: Option<String>,
		file: Option<PathBuf>,
		families: Vec<String>,
		force: bool,
		dry_run: bool,
		jobs: usize,
	},
	/// Print the download directory.
	Path,
	/// Check the download directory against the declarations.
	Verify {
		style: Option<String>,
		file: Option<PathBuf>,
	},
}

pub(crate) struct LaunchOptions {
	pub(crate) offline: bool,
	pub(crate) state_dir: Option<PathBuf>,
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
	pub(crate) list_stylesheets: bool,
	pub(crate) fonts: Option<FontsCommand>,
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
			state_dir: None,
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
			list_stylesheets: false,
			fonts: None,
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

/// The command line.
#[derive(Parser)]
#[command(
	name = "markview",
	version,
	about = "Native Markdown reading",
	long_about = None,
	after_help = AFTER_HELP
)]
struct Cli {
	/// The document to read; without one the reader opens empty.
	file: Option<PathBuf>,
	#[command(flatten)]
	reading: Reading,
	/// Never touch the network.
	#[arg(long, global = true)]
	offline: bool,
	#[command(subcommand)]
	command: Option<Command>,
}

/// How a document is read, drawn or measured.
#[derive(Args, Clone)]
struct Reading {
	/// A stylesheet to apply; repeats in priority order.
	#[arg(long = "style", value_name = "ID")]
	style: Vec<String>,
	/// The dark reader theme.
	#[arg(long)]
	dark: bool,
	/// The light reader theme.
	#[arg(long)]
	light: bool,
	/// A directory of font files; repeats.
	#[arg(long = "fonts", value_name = "DIR")]
	fonts: Vec<PathBuf>,
	/// Shape with `--fonts` alone, never with the machine's own fonts.
	#[arg(long = "ignore-system-fonts")]
	ignore_system_fonts: bool,
	/// Window width in logical pixels.
	#[arg(long, value_name = "N")]
	width: Option<f32>,
	/// Window height in logical pixels.
	#[arg(long, value_name = "N")]
	height: Option<f32>,
	/// Reading column width in logical pixels.
	#[arg(long = "column", value_name = "N")]
	column: Option<f32>,
	/// Body text size in logical pixels.
	#[arg(long = "font-size", value_name = "N")]
	font_size: Option<f32>,
	/// Where to scroll, as a fraction of the document.
	#[arg(long, value_name = "N")]
	scroll: Option<f32>,
	/// First-line paragraph indent in em units.
	#[arg(long = "paragraph-indent", value_name = "N")]
	paragraph_indent: Option<f32>,
	/// Which CJK convention to read with.
	#[arg(long = "cjk-type", value_name = "SC|TC|JP|none")]
	cjk_type: Option<String>,
	/// Left-align body text instead of justifying it.
	#[arg(long)]
	left: bool,
	/// Do not hyphenate.
	#[arg(long = "no-hyphens")]
	no_hyphens: bool,
	/// Shape greedily, for a typography comparison.
	#[arg(long)]
	greedy: bool,
}
#[derive(Subcommand)]
// The parsed command line is built once and dropped; a variant is never held
// in a collection, so the size difference between them costs nothing.
#[expect(
	clippy::large_enum_variant,
	reason = "one parsed command, then dropped"
)]
enum Command {
	/// Serve editor-owned buffers for PDF and PNG export over JSON lines.
	Serve {
		/// Private engine storage; never use the desktop reader settings.
		#[arg(long)]
		state_dir: PathBuf,
	},
	/// Render one document to a PNG image.
	Render(RenderArgs),
	/// Export one document to a PDF.
	Pdf(PdfArgs),
	/// Measure layout throughput.
	Bench(BenchArgs),
	/// Measure first-frame latency.
	Latency(BenchArgs),
	/// Render the window once and save it.
	Smoke(SmokeArgs),
	/// Manage installed stylesheets.
	Ss {
		#[command(subcommand)]
		action: SsAction,
	},
	/// Inspect and download font families.
	Fonts {
		#[command(subcommand)]
		action: FontsAction,
	},
}

#[derive(Args)]
struct RenderArgs {
	/// The document to render.
	file: PathBuf,
	/// Where to write the image.
	#[arg(short = 'o', long, value_name = "PNG")]
	output: PathBuf,
	/// Image scale.
	#[arg(long, value_name = "N")]
	scale: Option<f32>,
	#[command(flatten)]
	reading: Reading,
}

#[derive(Args)]
struct SmokeArgs {
	/// The document to draw.
	file: PathBuf,
	/// Where to write the window image.
	#[arg(short = 'o', long, value_name = "PNG")]
	output: Option<PathBuf>,
	#[command(flatten)]
	reading: Reading,
}

#[derive(Args)]
struct BenchArgs {
	/// The document to measure.
	file: PathBuf,
	/// How many layout passes to run.
	#[arg(long, value_name = "N", default_value_t = 100)]
	iterations: usize,
	/// Where to write the JSON metrics.
	#[arg(short = 'o', long, value_name = "FILE")]
	output: Option<PathBuf>,
	#[command(flatten)]
	reading: Reading,
}

#[derive(Args)]
struct PdfArgs {
	/// The document to export.
	file: PathBuf,
	/// Where to write the PDF.
	#[arg(short = 'o', long, value_name = "PDF")]
	output: PathBuf,
	/// Paper size: a3, a4, a5, a6, b5, letter, legal, tabloid, or WIDTHxHEIGHT.
	#[arg(long, value_name = "SIZE")]
	paper: Option<String>,
	/// Print landscape.
	#[arg(long)]
	landscape: bool,
	/// Margins in millimetres: one, two, or four values.
	#[arg(long, value_name = "MM[,MM...]")]
	margin: Option<String>,
	/// Centred header text.
	#[arg(long, value_name = "TEXT")]
	header: Option<String>,
	/// Left header text.
	#[arg(long = "header-left", value_name = "TEXT")]
	header_left: Option<String>,
	/// Right header text.
	#[arg(long = "header-right", value_name = "TEXT")]
	header_right: Option<String>,
	/// Centred footer text.
	#[arg(long, value_name = "TEXT")]
	footer: Option<String>,
	/// Left footer text.
	#[arg(long = "footer-left", value_name = "TEXT")]
	footer_left: Option<String>,
	/// Right footer text.
	#[arg(long = "footer-right", value_name = "TEXT")]
	footer_right: Option<String>,
	/// Leave links unannotated.
	#[arg(long = "no-links")]
	no_links: bool,
	/// Re-export whenever the document or a local image changes.
	#[arg(long)]
	watch: bool,
	/// PDF title.
	#[arg(long, value_name = "TEXT")]
	title: Option<String>,
	/// An author; repeats.
	#[arg(long, value_name = "NAME")]
	author: Vec<String>,
	/// PDF subject.
	#[arg(long, value_name = "TEXT")]
	subject: Option<String>,
	/// Comma-separated PDF keywords.
	#[arg(long, value_name = "A,B")]
	keywords: Option<String>,
	/// The document's language, as an RFC 3066 tag.
	#[arg(long, value_name = "TAG")]
	language: Option<String>,
	/// The PDF creator.
	#[arg(long, value_name = "TEXT")]
	creator: Option<String>,
	#[command(flatten)]
	reading: Reading,
}

#[derive(Subcommand)]
enum SsAction {
	/// List the bundled and installed stylesheets.
	List,
	/// Install one stylesheet file.
	Install {
		/// The stylesheet to install.
		file: PathBuf,
		/// Replace an installed sheet whose version is equal or lower.
		#[arg(long)]
		force: bool,
	},
	/// Parse one stylesheet without installing it.
	Validate {
		/// The stylesheet to check.
		file: PathBuf,
	},
}

#[derive(Subcommand)]
enum FontsAction {
	/// List the declared families and what the download directory holds.
	List {
		#[command(flatten)]
		selection: Selection,
		/// Include families that are already installed or on disk.
		#[arg(long)]
		all: bool,
	},
	/// Download font families.
	Download {
		/// The families to download; every missing one when empty.
		families: Vec<String>,
		#[command(flatten)]
		selection: Selection,
		/// Download again even when the family is already present.
		#[arg(long)]
		force: bool,
		/// Report what would be downloaded without fetching anything.
		#[arg(long = "dry-run")]
		dry_run: bool,
		/// How many transfers may run at once.
		#[arg(long, value_name = "N", default_value_t = 4)]
		jobs: usize,
	},
	/// Print the download directory.
	Path,
	/// Check the download directory against the declarations.
	Verify {
		#[command(flatten)]
		selection: Selection,
	},
}

/// Which declared families a `fonts` command looks at.
#[derive(Args)]
struct Selection {
	/// Take the families from one installed stylesheet.
	#[arg(long = "style", value_name = "ID")]
	style: Option<String>,
	/// Take the families from a stylesheet file instead of the installed ones.
	#[arg(long = "file", value_name = "FILE.mvss.toml")]
	file: Option<PathBuf>,
}

const AFTER_HELP: &str = "\
Keyboard: Ctrl+O open · Ctrl+B outline · Ctrl+T styles · Ctrl+E export
          Ctrl+ , settings · Ctrl+ +/- font size · Ctrl+[ / ] column width
          Ctrl+L alignment · Ctrl+H hyphenation

Fonts: --fonts DIR adds a directory of font files and may be repeated.
       --ignore-system-fonts shapes with those directories alone, so the
       fonts installed on the machine cannot change the result.
       A stylesheet's [[font-family]] tables offer families to download;
       run `markview fonts list` to see them and `markview fonts download`
       to fetch what is missing. The reader downloads from its Fonts page.

Images: local files, file:, http(s): and data: URIs; bitmap and SVG.
        An image alone in its block is centered, otherwise it is inline.
        Animated images show their first frame; --offline blocks the network.
";

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

/// The old flag form a subcommand replaced, with what to write instead.
fn legacy_hint(args: &[OsString]) -> Option<String> {
	for arg in args {
		let text = arg.to_string_lossy();
		let sub = match text.as_ref() {
			"--render" => "render",
			"--pdf" => "pdf",
			"--bench" => "bench",
			"--bench-latency" => "latency",
			"--smoke-test" => "smoke",
			_ => continue,
		};
		return Some(format!(
			"{text} is a subcommand now: write `markview {sub} FILE ...`; run `markview {sub} --help`"
		));
	}
	None
}

fn parse_arguments(
	args: impl IntoIterator<Item = OsString>,
) -> Result<Option<LaunchOptions>> {
	let args: Vec<OsString> = args.into_iter().collect();
	if let Some(hint) = legacy_hint(&args) {
		bail!("{hint}");
	}
	let cli = match Cli::try_parse_from(
		std::iter::once(OsString::from("markview")).chain(args),
	) {
		Ok(cli) => cli,
		Err(error)
			if matches!(
				error.kind(),
				clap::error::ErrorKind::DisplayHelp
					| clap::error::ErrorKind::DisplayVersion
			) =>
		{
			let _ = error.print();
			return Ok(None);
		}
		Err(error) => bail!("{}", error.to_string().trim_end()),
	};
	let mut out = LaunchOptions {
		offline: cli.offline,
		..LaunchOptions::default()
	};
	out.path = cli.file;
	apply_reading(&mut out, &cli.reading)?;
	if let Some(command) = cli.command {
		apply_command(&mut out, command)?;
	}
	finish(out)
}

fn apply_command(out: &mut LaunchOptions, command: Command) -> Result<()> {
	match command {
		Command::Serve { state_dir } => {
			out.mode = Mode::Serve;
			out.state_dir = Some(state_dir);
			Ok(())
		}
		Command::Render(args) => {
			out.mode = Mode::Render;
			out.path = Some(args.file);
			out.output = Some(args.output);
			if let Some(scale) = args.scale {
				check_number("--scale", scale)?;
				out.scale = scale.clamp(0.5, 4.0);
			}
			apply_reading(out, &args.reading)
		}
		Command::Smoke(args) => {
			out.mode = Mode::Smoke;
			out.path = Some(args.file);
			out.output = args.output;
			apply_reading(out, &args.reading)
		}
		Command::Bench(args) => {
			out.mode = Mode::Bench;
			out.path = Some(args.file);
			out.output = args.output;
			out.iterations = args.iterations.clamp(1, 10_000);
			apply_reading(out, &args.reading)
		}
		Command::Latency(args) => {
			out.mode = Mode::Latency;
			out.path = Some(args.file);
			out.output = args.output;
			out.iterations = args.iterations.clamp(1, 10_000);
			apply_reading(out, &args.reading)
		}
		Command::Pdf(args) => {
			out.mode = Mode::Pdf;
			out.path = Some(args.file);
			out.output = Some(args.output);
			out.links = !args.no_links;
			out.watch = args.watch;
			out.page.landscape = args.landscape;
			if let Some(paper) = args.paper {
				if markview_core::style::parse_paper_size(&paper).is_none() {
					bail!(
						"Invalid paper {paper:?}; use a3, a4, a5, a6, b5, letter, legal, tabloid or WIDTHxHEIGHT in millimetres"
					);
				}
				out.page.paper = Some(paper);
			}
			if let Some(margin) = args.margin {
				out.page.margin = Some(parse_margin(&margin)?);
			}
			for (slot, value) in [
				(0, args.header_left),
				(1, args.header),
				(2, args.header_right),
			] {
				if let Some(value) = value {
					template("--header", &value)?;
					out.page.header[slot] = Some(value);
				}
			}
			for (slot, value) in [
				(0, args.footer_left),
				(1, args.footer),
				(2, args.footer_right),
			] {
				if let Some(value) = value {
					template("--footer", &value)?;
					out.page.footer[slot] = Some(value);
				}
			}
			out.metadata.title = args.title;
			out.metadata.authors = args
				.author
				.into_iter()
				.filter(|name| !name.trim().is_empty())
				.collect();
			out.metadata.subject = args.subject;
			out.metadata.keywords = args
				.keywords
				.unwrap_or_default()
				.split(',')
				.map(|word| word.trim())
				.filter(|word| !word.is_empty())
				.map(str::to_owned)
				.collect();
			if let Some(language) = args.language {
				if !is_language_tag(&language) {
					bail!(
						"Invalid language {language:?}; use an RFC 3066 tag such as en, zh-CN or ja"
					);
				}
				out.metadata.language = Some(language);
			}
			out.metadata.creator = args.creator;
			apply_reading(out, &args.reading)
		}
		Command::Ss { action } => {
			match action {
				SsAction::List => {
					out.mode = Mode::StylesheetList;
					out.list_stylesheets = true;
				}
				SsAction::Install { file, force } => {
					out.install = Some((file, force))
				}
				SsAction::Validate { file } => out.validate = Some(file),
			}
			Ok(())
		}
		Command::Fonts { action } => {
			out.mode = Mode::Fonts;
			out.fonts = Some(match action {
				FontsAction::List { selection, all } => FontsCommand::List {
					style: selection.style,
					file: selection.file,
					all,
				},
				FontsAction::Download {
					families,
					selection,
					force,
					dry_run,
					jobs,
				} => FontsCommand::Download {
					style: selection.style,
					file: selection.file,
					families,
					force,
					dry_run,
					jobs: jobs.clamp(1, 64),
				},
				FontsAction::Path => FontsCommand::Path,
				FontsAction::Verify { selection } => FontsCommand::Verify {
					style: selection.style,
					file: selection.file,
				},
			});
			Ok(())
		}
	}
}

fn apply_reading(out: &mut LaunchOptions, reading: &Reading) -> Result<()> {
	for id in &reading.style {
		crate::stylesheet::validate_id(id)?;
	}
	if !reading.style.is_empty() {
		out.style = Some(reading.style.clone());
		out.overrides.push(Setting::Theme);
	}
	if reading.dark {
		out.theme = Some(Theme::Dark);
		out.overrides.push(Setting::Theme);
	}
	if reading.light {
		out.theme = Some(Theme::Light);
		out.overrides.push(Setting::Theme);
	}
	for directory in &reading.fonts {
		if !directory.is_dir() {
			bail!("--fonts: {} is not a directory", directory.display());
		}
		out.options.fonts.directories.push(directory.clone());
	}
	if reading.ignore_system_fonts {
		out.options.fonts.ignore_system_fonts = true;
	}
	if let Some(width) = reading.width {
		check_number("--width", width)?;
		out.width = (width as u32).clamp(320, 8192);
	}
	if let Some(height) = reading.height {
		check_number("--height", height)?;
		out.height = (height as u32).clamp(240, 8192);
	}
	if let Some(column) = reading.column {
		check_number("--column", column)?;
		out.options.width = column.clamp(240.0, 1600.0);
		out.overrides.push(Setting::Width);
	}
	if let Some(size) = reading.font_size {
		check_number("--font-size", size)?;
		out.options.font_size = size.clamp(10.0, 40.0);
		out.overrides.push(Setting::FontSize);
	}
	if let Some(scroll) = reading.scroll {
		check_number("--scroll", scroll)?;
		out.scroll = scroll;
	}
	if let Some(indent) = reading.paragraph_indent {
		check_number("--paragraph-indent", indent)?;
		out.options.paragraph_indent = indent.clamp(0.0, 4.0);
		out.overrides.push(Setting::ParagraphIndent);
	}
	if let Some(name) = &reading.cjk_type {
		out.cjk_type =
			Some(markview_core::style::CjkType::from_name(name).with_context(
				|| format!("Invalid CJK type {name}; use SC, TC, JP or none"),
			)?);
		out.overrides.push(Setting::CjkType);
	}
	if reading.left {
		out.options.justify = false;
		out.overrides.push(Setting::Justify);
	}
	if reading.no_hyphens {
		out.options.hyphenate = false;
		out.overrides.push(Setting::Hyphenate);
	}
	if reading.greedy {
		out.options.greedy = true;
	}
	Ok(())
}

fn check_number(flag: &str, value: f32) -> Result<()> {
	if !value.is_finite() || value < 0.0 {
		bail!("Invalid value for {flag}");
	}
	Ok(())
}

/// Millimetres: one value for every side, two for vertical and horizontal, or
/// four for top, right, bottom and left.
fn parse_margin(value: &str) -> Result<[f32; 4]> {
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
		[top, right, bottom, left] => [*top, *right, *bottom, *left],
		_ => bail!("--margin takes 1, 2 or 4 millimetres: --margin 20,25"),
	};
	if margin
		.iter()
		.any(|value| !value.is_finite() || *value < 0.0)
	{
		bail!("Invalid margin");
	}
	Ok(margin)
}

/// A page-furniture slot may only name the placeholders the page writer knows.
fn template(flag: &str, value: &str) -> Result<()> {
	if !markview_core::paginate::template_is_valid(value) {
		bail!(
			"{flag}: unknown placeholder; use {{page}}, {{pages}}, {{title}} or {{path}}"
		);
	}
	Ok(())
}

/// The checks that depend on the whole command, once every option is known.
fn finish(mut out: LaunchOptions) -> Result<Option<LaunchOptions>> {
	if out.style.is_some() && out.theme.is_some() {
		bail!("--style conflicts with --light and --dark");
	}
	// A drawing run shapes with the personal download directory, so an export
	// matches the reader on the same machine; a pinned run keeps its set.
	if out.mode.uses_personal_fonts() {
		crate::fonts::join_download_directory(
			&mut out.options.fonts,
			crate::fonts::directory(),
		);
	}
	// Paper is set at 12 pt unless the command line names a size; the reader
	// and the other diagnostic modes keep their own default.
	if out.mode == Mode::Pdf && !out.overrides.contains(&Setting::FontSize) {
		out.options.font_size = ExportSettings::DEFAULT_FONT_SIZE_PX;
	}
	if matches!(
		out.mode,
		Mode::Window | Mode::StylesheetList | Mode::Fonts | Mode::Serve
	) {
		return Ok(Some(out));
	}
	if out.path.is_none() {
		bail!("This mode requires a Markdown file");
	}
	if out.mode == Mode::Render && out.output.is_none() {
		bail!("render requires --output preview.png");
	}
	if out.mode == Mode::Pdf {
		if out.output.is_none() {
			bail!("pdf requires --output out.pdf");
		}
		if out.theme.is_some() {
			bail!(
				"pdf prints the sheet of paper, not the window; use --style to change its colors"
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
		bail!("--watch re-exports on every save; it applies to pdf");
	}
	Ok(Some(out))
}

/// Whether two paths name the same file once the filesystem resolves them:
/// absolute and relative spellings, `.` and `..`, and directory symlinks all
/// collapse to one answer, so an output cannot overwrite its own document.
pub(crate) fn same_target(a: &std::path::Path, b: &std::path::Path) -> bool {
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

impl LaunchOptions {
	pub(crate) fn pdf_request(&self) -> Result<PdfRequest> {
		Ok(PdfRequest {
			path: self
				.path
				.clone()
				.context("PDF export requires a document")?,
			output: self
				.output
				.clone()
				.context("--pdf requires --output out.pdf")?,
			options: self.options.clone(),
			page: self.page.clone(),
			metadata: self.metadata.clone(),
			links: self.links,
			offline: self.offline,
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::path::Path;

	fn parse(args: &[&str]) -> LaunchOptions {
		parse_arguments(args.iter().map(OsString::from))
			.unwrap()
			.expect("a command")
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
	fn drawing_modes_shape_with_the_personal_download_directory() {
		for mode in [Mode::Window, Mode::Render, Mode::Smoke, Mode::Pdf] {
			assert!(mode.uses_personal_fonts());
		}
		for mode in [
			Mode::Bench,
			Mode::Latency,
			Mode::StylesheetList,
			Mode::Fonts,
		] {
			assert!(!mode.uses_personal_fonts());
		}
		// A drawing run folds the directory in where this machine has one.
		let args = parse(&["pdf", "a.md", "--output", "out.pdf"]);
		match crate::fonts::directory() {
			Some(dir) if dir.is_dir() => {
				assert!(args.options.fonts.directories.contains(&dir));
			}
			_ => assert!(args.options.fonts.directories.is_empty()),
		}
		// A pinned run keeps exactly the set it named.
		let args = parse(&[
			"pdf",
			"a.md",
			"--output",
			"out.pdf",
			"--ignore-system-fonts",
		]);
		assert!(args.options.fonts.ignore_system_fonts);
		assert!(args.options.fonts.directories.is_empty());
	}

	#[test]
	fn a_bare_path_opens_the_reader_with_its_options() {
		let args = parse(&[
			"notes.md",
			"--style",
			"dark",
			"--font-size",
			"23",
			"--column",
			"700",
			"--offline",
			"--no-hyphens",
		]);
		assert!(args.mode == Mode::Window);
		assert_eq!(args.path.as_deref(), Some(Path::new("notes.md")));
		assert!(args.offline);
		assert_eq!(args.style, Some(vec!["dark".to_string()]));
		assert_eq!(args.options.font_size, 23.0);
		assert_eq!(args.options.width, 700.0);
		assert!(!args.options.hyphenate);
		// Each flag records that the command line spoke for one setting; the
		// order they were written in is not part of the answer.
		let mut overrides = args.overrides.clone();
		overrides.sort_by_key(|setting| format!("{setting:?}"));
		assert_eq!(
			overrides,
			vec![
				Setting::FontSize,
				Setting::Hyphenate,
				Setting::Theme,
				Setting::Width
			]
		);
		// Nothing to read is still a valid launch.
		assert!(parse(&[]).path.is_none());
		assert!(parse(&["notes.md"]).overrides.is_empty());
	}

	#[test]
	fn a_legacy_mode_flag_names_the_subcommand_that_replaced_it() {
		for (flag, sub) in [
			("--render", "render"),
			("--pdf", "pdf"),
			("--bench", "bench"),
			("--bench-latency", "latency"),
			("--smoke-test", "smoke"),
		] {
			let text = match parse_arguments([flag, "a.md"].map(OsString::from))
			{
				Ok(_) => panic!("{flag} is rejected"),
				Err(error) => format!("{error:#}"),
			};
			assert!(text.contains(sub), "{text}");
			assert!(text.contains("subcommand"), "{text}");
		}
	}

	#[test]
	fn render_needs_a_document_and_an_output() {
		let args = parse(&[
			"render",
			"a.md",
			"--output",
			"preview.png",
			"--scale",
			"2",
			"--dark",
		]);
		assert!(args.mode == Mode::Render);
		assert_eq!(args.output.as_deref(), Some(Path::new("preview.png")));
		assert_eq!(args.scale, 2.0);
		assert!(args.theme == Some(Theme::Dark));
		assert!(
			parse_arguments(["render", "a.md"].map(OsString::from)).is_err(),
			"an image needs somewhere to go"
		);
		assert!(
			parse_arguments(["render"].map(OsString::from)).is_err(),
			"an image needs a document"
		);
	}

	#[test]
	fn pdf_takes_a_page_furniture_and_metadata() {
		let args = parse(&[
			"pdf",
			"a.md",
			"--output",
			"out.pdf",
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
		]);
		assert!(args.mode == Mode::Pdf);
		let args = args.pdf_request().unwrap();
		assert_eq!(args.path, PathBuf::from("a.md"));
		assert_eq!(args.output, PathBuf::from("out.pdf"));
		assert_eq!(args.page.paper.as_deref(), Some("letter"));
		assert!(args.page.landscape);
		assert_eq!(args.page.margin, Some([10.0, 15.0, 10.0, 15.0]));
		assert_eq!(args.page.header[0].as_deref(), Some("Draft"));
		assert_eq!(args.page.header[1], None);
		assert_eq!(args.page.footer[1].as_deref(), Some("{page}/{pages}"));
		assert!(!args.links);
		assert_eq!(args.metadata.title.as_deref(), Some("A paper"));
		assert_eq!(args.metadata.authors, ["Ada", "Grace"]);
		assert_eq!(args.metadata.subject.as_deref(), Some("Testing"));
		assert_eq!(args.metadata.keywords, ["markdown", "typography"]);
		assert_eq!(args.metadata.language.as_deref(), Some("zh-CN"));
		assert_eq!(args.metadata.creator.as_deref(), Some("Editor"));
		// Paper is 12 pt unless the command line names a size.
		assert_eq!(
			args.options.font_size,
			ExportSettings::DEFAULT_FONT_SIZE_PX
		);
	}

	#[test]
	fn the_page_flags_are_validated() {
		for bad in [
			vec!["pdf", "a.md", "-o", "o.pdf", "--paper", "huge"],
			vec!["pdf", "a.md", "-o", "o.pdf", "--margin", "1,2,3"],
			vec!["pdf", "a.md", "-o", "o.pdf", "--margin", "-4"],
			vec!["pdf", "a.md", "-o", "o.pdf", "--header", "{nope}"],
			vec!["pdf", "a.md", "-o", "o.pdf", "--language", "-x"],
			vec!["pdf", "a.md", "--paper", "a4"],
			vec!["pdf", "a.md", "-o", "o.pdf", "--dark"],
		] {
			assert!(
				parse_arguments(bad.iter().map(OsString::from)).is_err(),
				"{bad:?}"
			);
		}
	}

	#[test]
	fn watch_re_exports_a_pdf_but_never_over_its_own_document() {
		let dir = tempfile::tempdir().unwrap();
		let document = dir.path().join("a.md");
		std::fs::write(&document, "# a\n").unwrap();
		let args = parse(&[
			"pdf",
			document.to_str().unwrap(),
			"--output",
			dir.path().join("out.pdf").to_str().unwrap(),
			"--watch",
		]);
		assert!(args.watch);
		assert!(same_target(&document, &dir.path().join("sub/../a.md")));
		assert!(!same_target(&document, &dir.path().join("out.pdf")));
	}

	#[test]
	fn font_flags_choose_which_faces_are_loaded() {
		let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
		let fonts = root.join("crates/markview-core/tests/fonts");
		let args = parse(&[
			"a.md",
			"--fonts",
			fonts.to_str().unwrap(),
			"--ignore-system-fonts",
		]);
		assert!(args.options.fonts.ignore_system_fonts);
		assert_eq!(args.options.fonts.directories, vec![fonts]);
		assert!(args.overrides.is_empty());
		assert!(
			parse_arguments(
				["a.md", "--fonts", "/nonexistent"].map(OsString::from)
			)
			.is_err()
		);
		assert!(
			parse_arguments(["a.md", "--fonts"].map(OsString::from)).is_err(),
			"a directory is required"
		);
	}

	#[test]
	fn cjk_type_is_parsed_case_insensitively() {
		for (name, expected) in [
			("sc", markview_core::style::CjkType::Sc),
			("TC", markview_core::style::CjkType::Tc),
			("Jp", markview_core::style::CjkType::Jp),
			("none", markview_core::style::CjkType::None),
		] {
			let args = parse(&["a.md", "--cjk-type", name]);
			assert_eq!(args.cjk_type, Some(expected), "{name}");
			assert_eq!(args.overrides, vec![Setting::CjkType], "{name}");
		}
		assert!(
			parse_arguments(
				["a.md", "--cjk-type", "klingon"].map(OsString::from)
			)
			.is_err()
		);
		assert!(
			parse_arguments(["a.md", "--cjk-type"].map(OsString::from))
				.is_err()
		);
	}

	#[test]
	fn bench_keeps_its_iterations_and_output() {
		let args =
			parse(&["bench", "a.md", "--iterations", "7", "-o", "m.json"]);
		assert!(args.mode == Mode::Bench);
		assert_eq!(args.iterations, 7);
		assert_eq!(args.output.as_deref(), Some(Path::new("m.json")));
		assert!(!args.mode.wraps_code_blocks());

		let args = parse(&["latency", "a.md", "--iterations", "3"]);
		assert!(args.mode == Mode::Latency);
		assert_eq!(args.iterations, 3);
		assert!(!args.mode.wraps_code_blocks());
		assert!(
			parse_arguments(["bench"].map(OsString::from)).is_err(),
			"a benchmark needs a document"
		);
	}

	#[test]
	fn stylesheet_commands_are_independent() {
		let args = parse(&["ss", "list"]);
		assert!(matches!(args.mode, Mode::StylesheetList));
		assert!(args.list_stylesheets);
		assert!(args.path.is_none());

		let args = parse(&["ss", "validate", "a.mvss.toml"]);
		assert_eq!(args.validate.as_deref(), Some(Path::new("a.mvss.toml")));

		let args = parse(&["ss", "install", "a.mvss.toml", "--force"]);
		assert_eq!(args.install, Some((PathBuf::from("a.mvss.toml"), true)));
		assert!(parse_arguments(["ss"].map(OsString::from)).is_err());
	}

	#[test]
	fn fonts_commands_name_their_selection() {
		// No selector means every missing family.
		let args = parse(&["fonts", "download"]);
		assert!(args.mode == Mode::Fonts);
		assert_eq!(
			args.fonts,
			Some(FontsCommand::Download {
				style: None,
				file: None,
				families: Vec::new(),
				force: false,
				dry_run: false,
				jobs: 4,
			})
		);
		// Named families, one stylesheet, and a re-download.
		let args = parse(&[
			"fonts",
			"download",
			"noto-serif",
			"--style",
			"paper",
			"--force",
			"--dry-run",
			"--jobs",
			"2",
		]);
		assert_eq!(
			args.fonts,
			Some(FontsCommand::Download {
				style: Some("paper".into()),
				file: None,
				families: vec!["noto-serif".into()],
				force: true,
				dry_run: true,
				jobs: 2,
			})
		);
		// A draft stylesheet can be used without installing it.
		let args = parse(&["fonts", "list", "--file", "draft.mvss.toml"]);
		assert_eq!(
			args.fonts,
			Some(FontsCommand::List {
				style: None,
				file: Some(PathBuf::from("draft.mvss.toml")),
				all: false,
			})
		);
		let args = parse(&["fonts", "path"]);
		assert_eq!(args.fonts, Some(FontsCommand::Path));
		let args = parse(&["fonts", "verify"]);
		assert_eq!(
			args.fonts,
			Some(FontsCommand::Verify {
				style: None,
				file: None,
			})
		);
		assert!(parse_arguments(["fonts"].map(OsString::from)).is_err());
	}

	#[test]
	fn a_path_is_only_read_when_the_command_needs_one() {
		assert!(parse(&["ss", "list"]).path.is_none());
		assert!(parse(&["fonts", "path"]).path.is_none());
		assert_eq!(parse(&["a.md"]).path.as_deref(), Some(Path::new("a.md")));
	}

	#[test]
	fn same_target_resolves_equivalent_paths_to_one_file() {
		let dir = tempfile::tempdir().unwrap();
		let file = dir.path().join("a.md");
		std::fs::write(&file, "# a\n").unwrap();
		assert!(same_target(&file, &dir.path().join("./a.md")));
		assert!(same_target(&file, &dir.path().join("sub/../a.md")));
		assert!(!same_target(&file, &dir.path().join("b.md")));
	}
}
