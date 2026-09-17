//! Headless PDF export from the command line.
//!
//! The export re-lays the document at the paper's text measure, breaks it into
//! pages, and writes vector content through `markview-pdf`. Nothing here
//! touches a window, a GPU, or the user's settings.
use crate::{
	cli::{LaunchOptions, MetadataOverrides, PageOverrides},
	document,
	file::read_document,
	images::Images,
	layout::{LayoutEngine, LayoutOptions},
	paginate::{PageGeometry, paginate},
};
use anyhow::{Context, Result};
use log::{info, warn};
use markview_core::style::{PageStyle, Stylesheet};
use markview_pdf::Export;
use std::{path::Path, sync::Arc};

/// The paper the export uses: the print stylesheet with the flags on top.
pub fn styled(sheet: Arc<Stylesheet>, page: &PageOverrides) -> Arc<Stylesheet> {
	let mut sheet = (*sheet).clone();
	if let Some(paper) = &page.paper {
		sheet.page.size = Some(paper.clone());
	}
	if page.landscape {
		sheet.page.landscape = Some(true);
	}
	if let Some(margin) = page.margin {
		sheet.page.margin = Some(margin.to_vec());
	}
	for (slot, value) in page.header.iter().enumerate() {
		if let Some(value) = value {
			slot_of(&mut sheet.page, true, slot, value.clone());
		}
	}
	for (slot, value) in page.footer.iter().enumerate() {
		if let Some(value) = value {
			slot_of(&mut sheet.page, false, slot, value.clone());
		}
	}
	Arc::new(sheet)
}

fn slot_of(page: &mut PageStyle, header: bool, slot: usize, value: String) {
	let target = match (header, slot) {
		(true, 0) => &mut page.header_left,
		(true, 1) => &mut page.header_center,
		(true, _) => &mut page.header_right,
		(false, 0) => &mut page.footer_left,
		(false, 1) => &mut page.footer_center,
		(false, _) => &mut page.footer_right,
	};
	*target = Some(value);
}

pub fn run(path: &Path, args: &LaunchOptions) -> Result<()> {
	let output = args
		.output
		.as_ref()
		.context("--pdf requires --output out.pdf")?;
	let stylesheet = styled(args.options.stylesheet.clone(), &args.page);
	let geometry = PageGeometry::from_style(stylesheet.page())?;
	let document = document::parse(read_document(path)?);
	let mut images = Images::new(args.offline);
	images.prepare(&document, path, 1, false);
	images.wait();
	for entry in images.snapshot.entries.values() {
		if let Some(error) = &entry.error {
			warn!("Image: {error}");
		}
	}
	// The page's text measure replaces the reader's reading column.
	let options = LayoutOptions {
		width: geometry.text_px().0,
		codeblock_wrap: true,
		stylesheet: stylesheet.clone(),
		..args.options.clone()
	};
	let mut engine = LayoutEngine::new();
	engine.validate_stylesheet(&options.stylesheet)?;
	let mut snapshot =
		engine.layout_with_images(&document, &options, &images.snapshot);
	// Syntax highlighting arrives from a worker, and this run has no event
	// loop to lay out again when it does, so wait for it and keep the colors.
	if engine.wait_highlights() {
		snapshot =
			engine.layout_with_images(&document, &options, &images.snapshot);
	}
	let pagination = paginate(&document, &snapshot, &geometry);
	let metadata = metadata_of(&args.metadata, &document, path);
	let bytes = markview_pdf::export(Export {
		snapshot: &snapshot,
		images: &images.snapshot,
		stylesheet: &stylesheet,
		geometry: &geometry,
		pagination: &pagination,
		metadata: metadata.clone(),
		path: path.display().to_string(),
		body_size_px: options.font_size,
		links: args.links,
	})?;
	if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty())
	{
		std::fs::create_dir_all(parent)?;
	}
	std::fs::write(output, &bytes)
		.with_context(|| format!("Cannot write {}", output.display()))?;
	info!(
		"Exported {} pages, {} blocks, {} bytes to {} ({:.1}x{:.1}pt, {} degraded paragraphs, {} formula errors)",
		pagination.pages.len(),
		snapshot.blocks.len(),
		bytes.len(),
		output.display(),
		geometry.width_pt,
		geometry.height_pt,
		snapshot.degraded,
		snapshot.math_errors,
	);
	if let Some(title) = &metadata.title {
		log::debug!("PDF title: {title}");
	}
	Ok(())
}

/// What the information dictionary holds: the flags win, then the document's
/// first heading, then the file's name. Anything the user did not ask for is
/// left out rather than invented.
fn metadata_of(
	overrides: &MetadataOverrides,
	document: &markview_core::document::Document,
	path: &Path,
) -> markview_pdf::Metadata {
	let title = overrides
		.title
		.clone()
		.or_else(|| title_of(document, path))
		.filter(|title| !title.trim().is_empty())
		.map(|title| title.trim().to_owned());
	markview_pdf::Metadata {
		title,
		authors: overrides.authors.clone(),
		subject: overrides.subject.clone(),
		keywords: overrides.keywords.clone(),
		language: overrides.language.clone(),
		creator: overrides.creator.clone(),
	}
}

/// The document's own title: its first heading, or the file's name.
fn title_of(
	document: &markview_core::document::Document,
	path: &Path,
) -> Option<String> {
	for block in &document.blocks {
		if let markview_core::document::BlockKind::Heading { text, .. } =
			&block.kind
		{
			let title = document::plain_text(text);
			if !title.trim().is_empty() {
				return Some(title.trim().to_owned());
			}
		}
	}
	path.file_stem()
		.map(|stem| stem.to_string_lossy().into_owned())
		.filter(|stem| !stem.is_empty())
}
