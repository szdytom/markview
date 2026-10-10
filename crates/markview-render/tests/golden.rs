#![cfg(target_os = "linux")]

use anyhow::Result;
#[path = "../../../tests/support/render_goldens.rs"]
mod comparison;
use comparison::{Baselines, capture, compare};
use image::{Rgba, RgbaImage};
#[path = "golden/cases.rs"]
mod cases;
#[path = "golden/paper.rs"]
mod paper;

use markview_core::{
	background::Direct,
	document,
	fonts::FontConfig,
	image::{ImageInfo, ImageSnapshot, Pixels},
	layout::LayoutEngine,
	text::{Affinity, TextPosition, TextSelection},
};
use markview_render::{Renderer, Theme, View};
use std::{collections::HashMap, fs, path::Path, sync::Arc};

#[test]
fn markdown_matches_rendering_baselines() -> Result<()> {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	let mut baselines = Baselines::new(
		root.join("tests/goldens"),
		root.join("../../artifacts/render-goldens"),
	)?;
	let mut files = fs::read_dir(root.join("../markview-core/tests/fonts"))?
		.chain(fs::read_dir(root.join("tests/fonts"))?)
		.collect::<std::io::Result<Vec<_>>>()?;
	files.sort_by_key(|file| file.file_name());
	let faces = files
		.into_iter()
		.map(|file| {
			fs::read(file.path())
				.map(|data| parley::fontique::Blob::new(Arc::new(data)))
		})
		.collect::<std::io::Result<Vec<_>>>()?;
	let fonts = FontConfig::from_faces(0x676f6c64656e, faces);
	let tiles = image::load_from_memory(include_bytes!("fixtures/tiles.png"))?
		.into_rgba8();
	let mut images = ImageSnapshot::default();
	images.entries.insert(
		"tiles.png".into(),
		ImageInfo {
			version: 1,
			size: Some(tiles.dimensions()),
			error: None,
		},
	);
	images.pixels.insert(
		"tiles.png".into(),
		1,
		Arc::new(Pixels {
			width: tiles.width(),
			height: tiles.height(),
			rgba: tiles.into_raw().into(),
		}),
	);
	images.entries.insert(
		"failed.png".into(),
		ImageInfo {
			version: 1,
			error: Some("Image is unavailable".into()),
			size: None,
		},
	);
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	assert!(
		renderer.adapter_name.starts_with("llvmpipe")
			&& renderer.adapter_name.ends_with("(Vulkan, Cpu)"),
		"Golden tests require Lavapipe; select its ICD with VK_DRIVER_FILES. Got {}",
		renderer.adapter_name
	);
	eprintln!("Golden adapter: {}", renderer.adapter_name);
	let cases = cases::cases();
	let registered: std::collections::BTreeSet<_> =
		cases.iter().map(|case| case.fixture.to_owned()).collect();
	let mut fixtures = std::collections::BTreeSet::new();
	for entry in fs::read_dir(root.join("tests/fixtures"))? {
		let path = entry?.path();
		if path.extension().is_some_and(|ext| ext == "md") {
			fixtures.insert(
				path.file_stem().unwrap().to_string_lossy().into_owned(),
			);
		}
	}
	assert_eq!(registered, fixtures, "Every Markdown fixture must render");
	let mut names = std::collections::HashSet::new();
	for case in cases {
		let name = &case.name;
		assert!(names.insert(name.clone()), "Duplicate case {name}");
		let (width, height, scale) = (case.width, case.height, case.scale);
		let doc = document::parse(fs::read_to_string(
			root.join(format!("tests/fixtures/{}.md", case.fixture)),
		)?);
		let mut options = case.options(root)?;
		if let Some(states) = case.disclosures {
			fn disclosures(blocks: &[document::Block], ids: &mut Vec<u64>) {
				for block in blocks {
					match &block.kind {
						document::BlockKind::Details { blocks, .. } => {
							ids.push(block.id);
							disclosures(blocks, ids);
						}
						document::BlockKind::Quote { blocks, .. } => {
							disclosures(blocks, ids)
						}
						document::BlockKind::List { items, .. } => {
							for item in items {
								disclosures(&item.blocks, ids);
							}
						}
						_ => {}
					}
				}
			}
			let mut ids = Vec::new();
			disclosures(&doc.blocks, &mut ids);
			assert_eq!(ids.len(), states.len());
			options.details_open =
				Arc::new(ids.into_iter().zip(states.iter().copied()).collect());
		}
		options.fonts = fonts.clone();
		renderer.set_stylesheet(options.stylesheet.clone());
		let mut engine =
			LayoutEngine::with_executor(Arc::new(Direct), Arc::new(|| {}));
		if matches!(
			case.cjk,
			markview_core::style::CjkType::Tc
				| markview_core::style::CjkType::Jp
		) {
			let region = format!("{:?}", case.cjk).to_lowercase();
			let expected = fs::read(root.join(format!(
				"tests/fonts/NotoSansMonoCJK{region}-Regular-subset.otf"
			)))?;
			let probe = engine.layout(&document::parse("`中文`"), &options);
			let glyphs: Vec<_> = probe
				.blocks
				.iter()
				.flat_map(|block| &block.layout.draws)
				.filter_map(|draw| match draw {
					markview_core::scene::Draw::Glyph(glyph) => Some(glyph),
					_ => None,
				})
				.collect();
			assert!(!glyphs.is_empty());
			assert!(
				glyphs
					.iter()
					.all(|glyph| glyph.font.data.data() == expected),
				"{name}: inline code must use the pinned regional monospace face"
			);
		}
		let snapshot = engine.layout_with_images(&doc, &options, &images);
		let snapshot = if engine.wait_highlights() {
			engine.layout_with_images(&doc, &options, &images)
		} else {
			snapshot
		};
		if case.fixture != "overflow" && case.variant != "no-hyphenation" {
			assert_eq!(snapshot.degraded, 0, "{name}: layout degraded");
		}
		if case.fixture != "diagnostics" {
			assert_eq!(
				snapshot.math_errors, 0,
				"{name}: unexpected math errors"
			);
		}
		let mut horizontal = HashMap::new();
		if case.variant == "scrolled" {
			for (bi, block) in snapshot.blocks.iter().enumerate() {
				for (oi, _) in block.layout.overflow.iter().enumerate() {
					horizontal.insert((bi, oi), 90.);
				}
			}
			assert!(!horizontal.is_empty(), "{name}: nothing overflows");
		}
		let selection =
			if case.fixture == "details-flow" && case.variant == "selected" {
				snapshot.select_all(1)
			} else {
				(case.variant == "selected").then_some(TextSelection {
					anchor: TextPosition {
						revision: 1,
						block: 1,
						node: 0,
						offset: 0,
						affinity: Affinity::Before,
					},
					focus: TextPosition {
						revision: 1,
						block: 2,
						node: 0,
						offset: 10,
						affinity: Affinity::After,
					},
				})
			};
		let view = View {
			width: (width * scale) as u32,
			height: (height * scale) as u32,
			scale,
			left: 20.,
			top: 20.,
			bottom: 20.,
			scroll: 0.,
			theme: if case.theme == "dark" {
				Theme::Dark
			} else {
				Theme::Light
			},
			horizontal: &horizontal,
			selection,
			revision: 1,
			hovered_link: (case.variant == "hovered")
				.then_some("https://example.invalid"),
			hovered_overflow: (case.variant == "scrolled").then_some((1, 0)),
			held_overflow: None,
		};
		assert!(
			snapshot.height <= height - 40.,
			"{name}: fixture is clipped ({} > {})",
			snapshot.height,
			height - 40.
		);
		let actual = capture(&mut renderer, &snapshot, &view, &[])?;
		let background = actual.get_pixel(0, 0);
		assert!(
			actual.pixels().filter(|pixel| *pixel != background).count() > 1000,
			"{name}: frame is empty"
		);
		baselines.record(name, &actual)?;
		if markview_core::style::Stylesheet::PDF_THEMES.contains(&case.theme)
			&& case.device == markview_core::style::Media::Desktop
			&& case.variant != "mvss-media"
		{
			for (name, actual) in paper::frames(
				&case,
				root,
				&fonts,
				&images,
				&baselines.artifacts,
			)? {
				baselines.record(&name, &actual)?;
			}
		}
	}
	baselines.finish()
}

#[test]
fn comparisons_reject_pixel_changes_dimensions_and_missing_baselines()
-> Result<()> {
	let dir = tempfile::tempdir()?;
	let baseline = dir.path().join("baseline.png");
	let expected = RgbaImage::from_pixel(2, 2, Rgba([40, 50, 60, 255]));
	expected.save(&baseline)?;
	let original = fs::read(&baseline)?;
	compare(&baseline, &expected, dir.path(), "identical")?;
	let mut changed = expected.clone();
	changed.get_pixel_mut(1, 0).0[0] += 1;
	compare(&baseline, &changed, dir.path(), "rounding")?;
	changed.get_pixel_mut(1, 0).0 = [42, 48, 62, 253];
	compare(&baseline, &changed, dir.path(), "rounding-limit")?;
	changed.get_pixel_mut(1, 0).0[0] += 1;
	let error = compare(&baseline, &changed, dir.path(), "pixel").unwrap_err();
	assert!(
		error
			.to_string()
			.contains("1 differing pixels, bounds (1, 0, 1, 0)")
	);
	let diff = image::open(dir.path().join("pixel-diff.png"))?.into_rgba8();
	assert_eq!(diff.get_pixel(1, 0).0, [255, 0, 255, 255]);
	assert_eq!(diff.get_pixel(0, 0).0, [0, 0, 0, 255]);
	assert_eq!(
		image::open(dir.path().join("pixel-expected.png"))?.into_rgba8(),
		expected
	);
	let smaller = RgbaImage::from_pixel(1, 2, Rgba([40, 50, 60, 255]));
	assert!(compare(&baseline, &smaller, dir.path(), "dimensions").is_err());
	assert_eq!(fs::read(&baseline)?, original);
	let missing = dir.path().join("missing.png");
	assert!(compare(&missing, &expected, dir.path(), "missing").is_err());
	assert!(!missing.exists());
	Ok(())
}
