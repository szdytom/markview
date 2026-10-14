//! Window-independent export planning: paper geometry, a paperless PNG layout,
//! and the one-shot job descriptions the reader hands to a background thread.
//!
//! The adapters prepare one immutable job through [`Config`]. Window dialogs,
//! command-line parsing and progress presentation stay with their callers.
use crate::{
	file::read_document,
	images::Images,
	layout::{LayoutEngine, LayoutOptions, LayoutSnapshot},
	settings::{ExportFormat, ExportSettings},
};
use anyhow::{Context, Result, bail};
use image::ImageEncoder;
use markview_layout::{
	paginate::{PT_PER_PX, PageGeometry},
	style::{PageStyle, Stylesheet},
};
use std::{
	io::Write,
	path::{Path, PathBuf},
	sync::Arc,
};

pub(crate) mod config;
pub(crate) use config::Config;

/// The document metadata written into the PDF.
#[derive(Default, Clone)]
pub(crate) struct MetadataOverrides {
	pub(crate) title: Option<String>,
	pub(crate) authors: Vec<String>,
	pub(crate) subject: Option<String>,
	pub(crate) keywords: Vec<String>,
	pub(crate) language: Option<String>,
	pub(crate) creator: Option<String>,
}

impl MetadataOverrides {
	pub(crate) fn with_title(title: &str) -> Self {
		let title = title.trim();
		Self {
			title: (!title.is_empty()).then(|| title.to_owned()),
			..Default::default()
		}
	}
}

/// The `[page]` fields the command line overrides on top of the stylesheet.
#[derive(Default, Clone)]
pub(crate) struct PageOverrides {
	pub(crate) paper: Option<String>,
	pub(crate) landscape: Option<bool>,
	pub(crate) margin: Option<[f32; 4]>,
	/// Header and footer slots, left to centre to right.
	pub(crate) header: [Option<String>; 3],
	pub(crate) footer: [Option<String>; 3],
}
/// A PDF job shared by desktop and CLI adapters, independent of launch state.
pub(crate) struct PdfRequest {
	pub(crate) security: crate::security::Security,
	pub(crate) path: PathBuf,
	pub(crate) output: PathBuf,
	pub(crate) options: LayoutOptions,
	pub(crate) geometry: PageGeometry,
	pub(crate) metadata: MetadataOverrides,
	pub(crate) links: bool,
	pub(crate) offline: bool,
}

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
	config::validate_margin(&settings.margin)?;
	PageGeometry::from_style(&page_style(settings))
}

/// Lays the document out once at the export's own measure. This is the PNG
/// half of the work, and it runs on the exporting thread.
#[cfg(test)]
pub(crate) fn png_snapshot(
	path: &Path,
	options: LayoutOptions,
	offline: bool,
) -> Result<LayoutSnapshot> {
	png_snapshot_with_services(
		path,
		options,
		offline,
		std::sync::Arc::new(crate::services::Services::new(4)),
		crate::security::Security::local(crate::security::Trust::Trusted),
	)
}
pub(crate) fn png_snapshot_with_services(
	path: &Path,
	options: LayoutOptions,
	offline: bool,
	services: std::sync::Arc<crate::services::Services>,
	security: crate::security::Security,
) -> Result<LayoutSnapshot> {
	let text = read_document(path)?;
	png_snapshot_text(path, &text, options, offline, services, security)
}

pub(crate) fn png_snapshot_text(
	path: &Path,
	text: &str,
	options: LayoutOptions,
	offline: bool,
	services: Arc<crate::services::Services>,
	security: crate::security::Security,
) -> Result<LayoutSnapshot> {
	let mut engine = LayoutEngine::with_executor(
		services.handle.cpu.clone(),
		std::sync::Arc::new(|| {}),
	);
	engine.validate_stylesheet(&options.stylesheet)?;
	let document = markview_commonmark::parse_with(
		text.to_owned(),
		markview_commonmark::Dialect {
			hard_breaks: options.hard_breaks,
			highlight_marks: options.highlight_marks,
		},
	);
	let mut images = Images::shared(offline, options.fonts.clone(), &services);
	images.set_security(security.for_content(&document.source));
	images.prepare(
		&document,
		path,
		1,
		false,
		&options.stylesheet,
		&options.fonts,
	);
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
pub(crate) fn geometry_summary(
	settings: &ExportSettings,
	lang: crate::lang::Lang,
) -> Result<String> {
	let geometry = geometry(settings)?;
	Ok(match settings.format {
		ExportFormat::Pdf => {
			let [_, _, width, height] = geometry.text_pt();
			lang.export_summary_pdf(
				format!("{:.0}", width / MM_TO_PT),
				format!("{:.0}", height / MM_TO_PT),
			)
		}
		ExportFormat::Png => {
			let width_px =
				(geometry.width_pt / PT_PER_PX * settings.scale).round();
			lang.export_summary_png(format!("{width_px:.0}"))
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
	// An exclusive create under a random name: no name here is predictable
	// enough to pre-place a symlink at, and the bytes go to the handle this
	// call owns rather than to whatever some path resolves to. `0o666` lets
	// the umask pick the mode a plain write would have used; `tempfile`'s own
	// default would make every export owner-only.
	let temp = markview_platform::export_tempfile(parent);
	let mut temp = temp
		.with_context(|| format!("Cannot write beside {}", path.display()))?;
	temp.write_all(bytes)
		.with_context(|| format!("Cannot write {}", path.display()))?;
	// Moving the inner error out drops the temporary path with this call, so a
	// failed replace never leaves the file behind.
	temp.persist(path)
		.map_err(|error| error.error)
		.with_context(|| format!("Cannot replace {}", path.display()))?;
	Ok(())
}

#[cfg(test)]
mod tests;

/// Draws one strip into the whole image's RGBA buffer.
#[expect(clippy::too_many_arguments, reason = "one strip's explicit geometry")]
pub(crate) fn draw_tile(
	renderer: &mut crate::render::Renderer,
	snapshot: &LayoutSnapshot,
	plan: &PngPlan,
	stylesheet: &Arc<Stylesheet>,
	tile: PngTile,
	scale: f32,
	left: f32,
	theme: crate::render::Theme,
	rgba: &mut [u8],
) -> anyhow::Result<()> {
	let horizontal = std::collections::HashMap::new();
	let view = crate::render::View {
		selection: None,
		revision: 0,
		width: plan.width_px,
		height: tile.height_px,
		scale,
		scroll: tile.scroll,
		left,
		top: 0.0,
		bottom: 0.0,
		theme,
		horizontal: &horizontal,
		hovered_link: None,
		hovered_overflow: None,
		held_overflow: None,
	};
	let target = renderer.offscreen(plan.width_px, tile.height_px);
	let target_view = target.create_view(&Default::default());
	let height = plan.height_px as f32 / scale;
	let tile_top = tile.y_px as f32 / scale;
	let tile_bottom = tile_top + tile.height_px as f32 / scale;
	let bands: Vec<_> = [
		(false, &stylesheet.page().header),
		(true, &stylesheet.page().footer),
	]
	.into_iter()
	.filter_map(|(bottom, edge)| {
		let (width, color) = edge.rule(height * PT_PER_PX)?;
		let width = width / PT_PER_PX;
		let start = if bottom { height - width } else { 0.0 };
		let top = start.max(tile_top);
		let end = (start + width).min(tile_bottom);
		(end > top).then_some(markview_layout::scene::Draw::Rect(
			markview_layout::scene::Rect {
				x: 0.0,
				y: top - tile_top,
				w: plan.width_px as f32 / scale,
				h: end - top,
			},
			markview_layout::scene::Paint::Color(color),
		))
	})
	.collect();
	let origin = renderer.set_ui_origin((0.0, 0.0));
	let submission = renderer.render_with_stylesheet(
		snapshot,
		&view,
		&bands,
		&[],
		&target_view,
		stylesheet.clone(),
	);
	renderer.set_ui_origin(origin);
	renderer.wait(Some(submission?))?;
	let pixels = renderer.read_pixels(&target)?;
	let start = tile.y_px as usize * plan.width_px as usize * 4;
	rgba[start..start + pixels.rgba.len()].copy_from_slice(&pixels.rgba);
	Ok(())
}

pub(crate) fn write_png(
	path: &Path,
	rgba: &[u8],
	width: u32,
	height: u32,
) -> anyhow::Result<()> {
	let mut bytes = Vec::new();
	image::codecs::png::PngEncoder::new(&mut bytes).write_image(
		rgba,
		width,
		height,
		image::ExtendedColorType::Rgba8,
	)?;
	write_atomic(path, &bytes)
}
