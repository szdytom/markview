//! Window-independent export planning: paper geometry, a paperless PNG layout,
//! and the one-shot job descriptions the reader hands to a background thread.
//!
//! Nothing here touches a window, a GPU or the reader's settings. The reader
//! borrows [`ExportSettings`] for a single job, so an export can never reflow
//! the document on screen.
use crate::{
	cli::{LaunchOptions, MetadataOverrides, Mode, PageOverrides},
	document,
	file::read_document,
	images::Images,
	layout::{LayoutEngine, LayoutOptions, LayoutSnapshot},
	settings::{ExportFormat, ExportSettings, FontDefOverride},
};
use anyhow::{Context, Result, bail};
use markview_core::{
	fonts::FontConfig,
	paginate::{PT_PER_PX, PageGeometry},
	style::{CjkType, PageStyle, Stylesheet},
};
use std::{
	path::{Path, PathBuf},
	sync::Arc,
};

/// A single PNG never allocates more pixels than this. At four bytes each the
/// stitched image is at most roughly 256 MiB, and a document beyond it is asked
/// to use a smaller scale or the PDF export instead.
pub(crate) const MAX_PNG_PIXELS: u64 = 64_000_000;

/// Millimetres to PDF points.
const MM_TO_PT: f32 = 72.0 / 25.4;

/// The paper an export writes on: the panel's `[page]` overrides on top of the
/// bundled print sheet's defaults.
pub(crate) fn page_style(settings: &ExportSettings) -> PageStyle {
	PageStyle {
		size: Some(settings.paper.clone()),
		landscape: Some(settings.landscape),
		margin: Some(settings.margin.to_vec()),
		..Default::default()
	}
}

pub(crate) fn geometry(settings: &ExportSettings) -> Result<PageGeometry> {
	PageGeometry::from_style(&page_style(settings))
}

/// The layout options both formats share. `width` is the paper's text measure
/// for a PNG and is replaced by the page geometry for a PDF.
pub(crate) fn layout_options(
	settings: &ExportSettings,
	width: f32,
	stylesheet: Arc<Stylesheet>,
	fonts: FontConfig,
) -> LayoutOptions {
	LayoutOptions {
		width,
		font_size: settings.font_size,
		paragraph_indent: settings.paragraph_indent,
		codeblock_wrap: true,
		force_open: true,
		stylesheet,
		fonts,
		..Default::default()
	}
}

/// The stylesheet an export starts from: the bundled print sheet with the
/// export's own styles layered on it, the reader's CJK variant, and the
/// reader's font overrides. The reader's theme never applies here.
pub(crate) fn export_stylesheet(
	style: &[String],
	cjk: CjkType,
	overrides: &[FontDefOverride],
) -> Result<Arc<Stylesheet>> {
	let ids = (!style.is_empty()).then_some(style);
	let sheet = crate::stylesheet::load_for_pdf(
		ids,
		crate::stylesheet::directory().as_deref(),
		cjk,
	)?;
	crate::stylesheet::apply_font_overrides(sheet, overrides)
}

/// A PDF export described as the command line would have described it, so the
/// reader and `--pdf` share one implementation.
pub(crate) fn pdf_launch(
	path: PathBuf,
	output: PathBuf,
	settings: &ExportSettings,
	fonts: FontConfig,
	cjk: CjkType,
	overrides: &[FontDefOverride],
	offline: bool,
) -> Result<LaunchOptions> {
	let stylesheet = export_stylesheet(&settings.style, cjk, overrides)?;
	Ok(LaunchOptions {
		offline,
		mode: Mode::Pdf,
		path: Some(path),
		output: Some(output),
		options: layout_options(settings, 0.0, stylesheet, fonts),
		page: PageOverrides {
			paper: Some(settings.paper.clone()),
			landscape: settings.landscape,
			margin: Some(settings.margin),
			..Default::default()
		},
		metadata: MetadataOverrides::default(),
		links: true,
		..Default::default()
	})
}

/// Lays the document out once at the export's own measure. This is the PNG
/// half of the work, and it runs on the exporting thread.
pub(crate) fn png_snapshot(
	path: &Path,
	options: LayoutOptions,
	offline: bool,
) -> Result<LayoutSnapshot> {
	let mut engine = LayoutEngine::new();
	engine.validate_stylesheet(&options.stylesheet)?;
	let document = document::parse(read_document(path)?);
	let mut images = Images::new(offline);
	images.prepare(&document, path, 1, false);
	images.wait();
	for entry in images.snapshot.entries.values() {
		if let Some(error) = &entry.error {
			log::warn!("Image: {error}");
		}
	}
	let mut snapshot =
		engine.layout_with_images(&document, &options, &images.snapshot);
	// Highlighting arrives from a worker; an export has no later frame to
	// settle it, so it waits and keeps the colors.
	if engine.wait_highlights() {
		snapshot =
			engine.layout_with_images(&document, &options, &images.snapshot);
	}
	Ok(snapshot)
}

/// One horizontal strip of the PNG, in device pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PngTile {
	pub y_px: u32,
	pub height_px: u32,
	/// Page-local scroll offset the strip is drawn with, in layout pixels.
	pub scroll: f32,
}

/// How a document becomes one PNG.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PngPlan {
	pub width_px: u32,
	pub height_px: u32,
	pub tiles: Vec<PngTile>,
}

/// Splits a whole document into GPU-sized strips.
///
/// The image is the paper's full width and the text measure plus the top and
/// bottom margins tall. Strips overlap nothing: each one covers an exact band
/// of device pixels, and the backgrounds are opaque, so the seam is invisible.
pub(crate) fn plan(
	geometry: &PageGeometry,
	content_height: f32,
	scale: f32,
	max_tile: u32,
) -> Result<PngPlan> {
	let [top, _, bottom, _] = geometry.margin_pt;
	let margin_px = |pt: f32| pt / PT_PER_PX;
	let to_px = |layout: f32| (layout * scale).round().max(1.0) as u32;
	let width_px = to_px(geometry.width_pt / PT_PER_PX);
	let height_px = to_px(margin_px(top) + content_height + margin_px(bottom));
	let max_tile = max_tile.max(1);
	// A strip is split by height alone, so the full page must fit across.
	if width_px > max_tile {
		bail!(
			"PNG would be {width_px} px wide, past the {max_tile} px GPU limit; use a smaller scale"
		);
	}
	if u64::from(width_px) * u64::from(height_px) > MAX_PNG_PIXELS {
		bail!(
			"PNG would be {width_px}×{height_px} px; use a smaller scale or export a PDF"
		);
	}
	let mut tiles = Vec::new();
	let mut y = 0;
	while y < height_px {
		let height_px = max_tile.min(height_px - y);
		tiles.push(PngTile {
			y_px: y,
			height_px,
			scroll: y as f32 / scale - margin_px(top),
		});
		y += height_px;
	}
	Ok(PngPlan {
		width_px,
		height_px,
		tiles,
	})
}

/// The result the export panel derives from the current settings: the measure a
/// PDF will set, or how wide a PNG will be. It never restates a row's own
/// value, and doubles as the panel's validation line.
pub(crate) fn geometry_summary(settings: &ExportSettings) -> Result<String> {
	let geometry = geometry(settings)?;
	Ok(match settings.format {
		ExportFormat::Pdf => {
			let [_, _, width, height] = geometry.text_pt();
			format!("text {:.0}×{:.0} mm", width / MM_TO_PT, height / MM_TO_PT,)
		}
		ExportFormat::Png => {
			let width_px =
				(geometry.width_pt / PT_PER_PX * settings.scale).round();
			format!("{width_px:.0} px wide")
		}
	})
}

/// Writes `bytes` beside `path` and renames them into place, so a viewer that
/// opens the file never sees one that is still being written.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
	let parent = path
		.parent()
		.filter(|parent| !parent.as_os_str().is_empty())
		.unwrap_or(Path::new("."));
	std::fs::create_dir_all(parent)?;
	let name = path
		.file_name()
		.map(|name| name.to_string_lossy().into_owned())
		.unwrap_or_default();
	let temp = parent.join(format!(".{name}.{}.tmp", std::process::id()));
	std::fs::write(&temp, bytes)
		.with_context(|| format!("Cannot write {}", temp.display()))?;
	if let Err(error) = std::fs::rename(&temp, path) {
		let _ = std::fs::remove_file(&temp);
		return Err(error)
			.with_context(|| format!("Cannot replace {}", path.display()));
	}
	Ok(())
}

#[cfg(test)]
mod tests;
