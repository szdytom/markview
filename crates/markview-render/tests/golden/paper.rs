use anyhow::{Result, ensure};
use image::RgbaImage;
use markview_layout::{
	background::Direct,
	fonts::FontConfig,
	image::ImageSnapshot,
	layout::LayoutEngine,
	paginate::{PageGeometry, paginate},
	style::Stylesheet,
};
use markview_pdf::{Export, Metadata};
use std::{fs, path::Path, process::Command, sync::Arc};

use super::cases::Case;

pub fn frames(
	case: &Case,
	root: &Path,
	fonts: &FontConfig,
	images: &ImageSnapshot,
	artifacts: &Path,
) -> Result<Vec<(String, RgbaImage)>> {
	let mut options = case.options(root)?;
	let mut sheet = (*options.stylesheet).clone();
	sheet.merge(&Stylesheet::parse(
		"format_version=2\nversion=1\n[page]\nsize='A5'\nmargin=[15,12,15,12]\n\
		header_left='{title}'\nheader_right='{page}/{pages}'\nfooter_center='阅读 {page}'\n\
		[page.header]\nrule_width=0.5\nrule_color='#315D86'\n\
		[page.footer]\nrule_width=1.0\nrule_color='#69747E'",
	)?);
	if case.variant == "mvss-page" {
		sheet.merge(&Stylesheet::parse(&fs::read_to_string(
			root.join("tests/fixtures/mvss-page.mvss.toml"),
		)?)?);
	}
	let geometry = PageGeometry::from_style(sheet.page())?;
	options.width = geometry.text_px().0;
	options.font_size = 16.;
	options.fonts = fonts.clone();
	options.force_open = true;
	options.hide_front_matter = true;
	options.stylesheet = Arc::new(sheet);
	let doc = markview_commonmark::parse_with(
		fs::read_to_string(
			root.join(format!("tests/fixtures/{}.md", case.fixture)),
		)?,
		markview_commonmark::Dialect {
			ofm: case.fixture == "callouts",
			..Default::default()
		},
	);
	let mut engine =
		LayoutEngine::with_executor(Arc::new(Direct), Arc::new(|| {}));
	engine.layout_with_images(&doc, &options, images);
	engine.wait_highlights();
	let snapshot = engine.layout_with_images(&doc, &options, images);
	let pagination = paginate(&doc, &snapshot, &geometry);
	ensure!(pagination.pages.len() > 1, "Paper fixture must paginate");
	let bytes = markview_pdf::export(Export {
		outline: doc.outline_entries(),
		snapshot: &snapshot,
		images,
		prepared_images: None,
		stylesheet: &options.stylesheet,
		geometry: &geometry,
		pagination: &pagination,
		metadata: Metadata {
			title: Some("Reading 阅读".into()),
			..Default::default()
		},
		path: "theme-sampler.md".into(),
		body_size_px: options.font_size,
		links: true,
		fonts: fonts.clone(),
	})?;
	let name = if case.variant == "mvss-page" {
		"pdf-custom".into()
	} else if case.fixture == "callouts" {
		format!("pdf-callouts-{}", case.theme)
	} else {
		format!("pdf-{}", case.theme)
	};
	let pdf = artifacts.join(format!("{name}.pdf"));
	fs::write(&pdf, bytes)?;
	let output = Command::new("pdftoppm")
		.args(["-r", "96", "-png"])
		.arg(pdf)
		.arg(artifacts.join(&name))
		.output()?;
	ensure!(
		output.status.success(),
		"pdftoppm: {}",
		String::from_utf8_lossy(&output.stderr)
	);
	let mut frames = Vec::new();
	for page in 1..=pagination.pages.len() {
		let name = format!("{name}-{page}");
		frames.push((
			name.clone(),
			image::open(artifacts.join(format!("{name}.png")))?.into_rgba8(),
		));
	}
	Ok(frames)
}
