use super::{Images, fonts::DiagramFonts};
use crate::test_support::render_goldens::{Baselines, capture};
use crate::{
	document,
	layout::{LayoutEngine, LayoutOptions},
	render::{Renderer, Theme, View},
};
use anyhow::{Result, ensure};
use markview_core::{
	background::Direct,
	fonts::FontConfig,
	image::{ImageInfo, ImageSnapshot},
	style::{CjkType, Stylesheet},
};
use std::{collections::HashMap, fs, path::Path, sync::Arc};

#[test]
fn diagrams_match_rendering_baselines() -> Result<()> {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	let mut baselines = Baselines::new(
		root.join("tests/goldens/diagrams"),
		root.join("artifacts/diagram-goldens"),
	)?;
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	ensure!(
		renderer.adapter_name.starts_with("llvmpipe")
			&& renderer.adapter_name.ends_with("(Vulkan, Cpu)"),
		"Diagram baselines require Lavapipe"
	);
	let fonts = crate::test_support::fonts();
	assert!(fonts.ignore_system_fonts);
	let faces = DiagramFonts::get(&fonts, &[]).faces();
	let svg_fonts = FontConfig::from_faces(
		0x737667,
		[
			"Noto Serif CJK SC",
			"Noto Sans CJK SC",
			"Noto Sans Mono CJK SC",
		]
		.into_iter()
		.map(|family| {
			faces
				.iter()
				.find(|face| {
					face.family == family && face.weight == 400 && !face.italic
				})
				.unwrap()
				.bytes()
				.clone()
		})
		.collect(),
	);
	let mut images = Images::with_cache(true, None);
	let mut default_svg = None;
	for preset in ["default", "dark", "forest", "neutral", "modern", "custom"] {
		let mut sheet = (*Stylesheet::bundled(preset == "dark")).clone();
		sheet.set_cjk_type(CjkType::Sc);
		let rules = if preset == "custom" {
			fs::read_to_string(
				root.join("tests/fixtures/diagrams/custom.mvss.toml"),
			)?
		} else {
			format!(
				"format_version=2\nversion=1\n[mermaid]\ntheme='{preset}'\nfont_family=['serif']"
			)
		};
		sheet.merge(&Stylesheet::parse(&rules)?);
		let sheet = Arc::new(sheet);
		renderer.set_stylesheet(sheet.clone());
		for kind in ["flowchart", "sequence", "git", "pie", "svg"] {
			let name = format!("{kind}-{preset}");
			let path = root.join(format!("tests/fixtures/diagrams/{kind}.md"));
			let doc = document::parse(fs::read_to_string(&path)?);
			let image_snapshot = if kind == "svg" {
				svg_snapshot(&doc, &path, &sheet, &svg_fonts)?
			} else {
				images.prepare(&doc, &path, 1, false, &sheet, &fonts);
				images.wait();
				images.snapshot.clone()
			};
			ensure!(
				!image_snapshot.entries.is_empty(),
				"{name}: no images parsed"
			);
			for info in image_snapshot.entries.values() {
				ensure!(
					info.error.is_none() && info.size.is_some(),
					"{name}: image failed: {info:?}"
				);
			}
			ensure!(
				image_snapshot.decoded().len() == image_snapshot.entries.len(),
				"{name}: pixels did not settle"
			);
			let options = LayoutOptions {
				width: 720.,
				stylesheet: sheet.clone(),
				fonts: fonts.clone(),
				..Default::default()
			};
			let mut engine =
				LayoutEngine::with_executor(Arc::new(Direct), Arc::new(|| {}));
			let snapshot =
				engine.layout_with_images(&doc, &options, &image_snapshot);
			ensure!(snapshot.height < 960., "{name}: clipped diagram");
			let horizontal = HashMap::new();
			let view = View {
				width: 760,
				height: 1000,
				scale: 1.,
				left: 20.,
				top: 20.,
				bottom: 20.,
				scroll: 0.,
				theme: Theme::Light,
				horizontal: &horizontal,
				selection: None,
				revision: 1,
				hovered_link: None,
				hovered_overflow: None,
				held_overflow: None,
			};
			let actual = capture(&mut renderer, &snapshot, &view, &[])?;
			if kind == "svg" && preset == "default" {
				default_svg = Some(actual.clone());
			} else if kind == "svg" && preset == "custom" {
				ensure!(
					Some(&actual) != default_svg.as_ref(),
					"Custom SVG font mappings must change the rendered text"
				);
			}
			baselines.record(&name, &actual)?;
		}
	}
	baselines.finish()
}

fn svg_snapshot(
	doc: &document::Document,
	path: &Path,
	sheet: &Stylesheet,
	config: &FontConfig,
) -> Result<ImageSnapshot> {
	// Standalone SVGs normally use system fonts; baselines supply pinned faces.
	assert!(config.ignore_system_fonts);
	let mappings = sheet.svg_generic_font_families();
	let families = mappings
		.iter()
		.flat_map(|(_, names)| names.clone())
		.collect::<Vec<_>>();
	let fonts = DiagramFonts::get_for(config, &[], &families, &mappings);
	let theme = super::diagram::resolve(sheet, None);
	let mut specs = Vec::new();
	for block in &doc.blocks {
		block.images(&mut specs);
	}
	let mut snapshot = ImageSnapshot::default();
	for spec in specs {
		let source = super::source::source(&spec.src, path)?;
		let bytes = super::source::fetch(&source, true, None, &theme)?;
		let decoded =
			super::decode(&bytes, None, Some((&fonts, "")), &mappings)?;
		snapshot.entries.insert(
			spec.src.clone(),
			ImageInfo {
				version: 1,
				size: Some(decoded.intrinsic),
				error: None,
			},
		);
		snapshot.pixels.insert(spec.src.clone(), 1, decoded.pixels);
	}
	Ok(snapshot)
}
