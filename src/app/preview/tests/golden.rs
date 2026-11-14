use super::*;
use crate::{
	render::{Renderer, Theme, View},
	test_support::render_goldens::{Baselines, capture_scenes},
};
use anyhow::{Result, ensure};
use markview_layout::{
	background::Direct,
	image::{ImageInfo, ImageSnapshot, ImageSource, Pixels, ResourceRef},
	style::Stylesheet,
};
use std::{fs, path::Path, sync::Arc};

const CASES: &[(&str, &str, f32)] = &[
	("short", "fit", 0.0),
	("long", "top", 0.0),
	("long", "bottom-selected", 1.0),
	("image", "fit", 0.0),
	("tall-image", "scrolled", 0.5),
];

fn check_fixtures(root: &Path) -> Result<()> {
	let registered: std::collections::BTreeSet<_> = CASES
		.iter()
		.map(|(fixture, _, _)| fixture.to_string())
		.collect();
	let entries = fs::read_dir(root.join("tests/fixtures/hover-preview"))?
		.collect::<std::io::Result<Vec<_>>>()?;
	let fixtures = entries
		.iter()
		.map(|entry| entry.path())
		.filter(|path| path.extension().is_some_and(|ext| ext == "md"))
		.map(|path| path.file_stem().unwrap().to_string_lossy().into_owned())
		.collect();
	ensure!(
		registered == fixtures,
		"Every hover preview fixture must render"
	);
	Ok(())
}

pub(super) fn images(root: &Path) -> Result<ImageSnapshot> {
	let pixels = image::open(
		root.join("crates/markview-render/tests/fixtures/tiles.png"),
	)?
	.into_rgba8();
	let mut images = ImageSnapshot::default();
	images.keys.insert(
		ImageSource::Resource(ResourceRef::document("tiles.png")),
		"tiles.png".into(),
	);
	images.entries.insert(
		"tiles.png".into(),
		ImageInfo {
			version: 1,
			size: Some(pixels.dimensions()),
			..Default::default()
		},
	);
	images.pixels.insert(
		"tiles.png".into(),
		1,
		Arc::new(Pixels {
			width: pixels.width(),
			height: pixels.height(),
			rgba: pixels.into_raw().into(),
		}),
	);
	Ok(images)
}

fn preview_app(
	root: &Path,
	fixture: &str,
	theme: Theme,
	images: &ImageSnapshot,
) -> Result<App<Proxy>> {
	let definition = fs::read_to_string(
		root.join(format!("tests/fixtures/hover-preview/{fixture}.md")),
	)?;
	let mut app = reader_app(&definition, theme, images);
	super::interaction_tests::hover_note(&mut app);
	Ok(app)
}

pub(super) fn reader_app(
	definition: &str,
	theme: Theme,
	images: &ImageSnapshot,
) -> App<Proxy> {
	let (mut app, _events) = app();
	app.preferences.values.theme = theme;
	app.preferences.values.lang = Some(crate::lang::Lang::En);
	app.preferences.values.stylesheet =
		Stylesheet::bundled(theme == Theme::Dark);
	app.ui
		.set_stylesheet(app.preferences.values.stylesheet.clone());
	let source = format!(
		"# Notes\n\nRead this reference[^note].\n\n{}{}",
		"Reader context stays in place.\n\n".repeat(20),
		definition
	);
	let document = crate::loaded::LoadedDocument::markdown(Arc::new(
		markview_commonmark::parse(source),
	));
	let snapshot = crate::layout::LayoutEngine::with_executor(
		Arc::new(Direct),
		Arc::new(|| {}),
	)
	.layout_with_images(&document, &app.options(), images);
	assert_eq!(snapshot.degraded, 0);
	assert_eq!(snapshot.math_errors, 0);
	let viewport = app.viewport();
	app.readers.session.accept(
		crate::worker::ReaderSnapshot {
			document,
			layout: snapshot,
			blocked_images: Default::default(),
			content_version: 1,
			complete: true,
			parse_complete: true,
			remote_deferred: 0,
		},
		viewport,
		None,
	);
	app.readers.session.path = Some("hover-preview.md".into());
	app
}

fn capture_popup(
	renderer: &mut Renderer,
	app: &mut App<Proxy>,
	scale: f32,
) -> Result<image::RgbaImage> {
	let overlay = app.overlay();
	let theme = app.preferences.values.theme;
	let scene = app.preview.scene(&app.readers, theme).unwrap();
	let crop = Rect {
		x: 200.0,
		y: 40.0,
		w: 720.0,
		h: 540.0,
	};
	let floating = app.preview.floating().unwrap();
	for rect in [floating.anchor, floating.rect] {
		ensure!(
			crop.contains(rect.x, rect.y)
				&& crop.contains(rect.x + rect.w, rect.y + rect.h),
			"the reference and full popup must stay inside the golden frame"
		);
	}
	let geometry = app.view_geometry();
	let view = View {
		width: (geometry.width * scale).round() as u32,
		height: (geometry.height * scale).round() as u32,
		scale,
		left: geometry.left,
		top: geometry.top,
		bottom: geometry.bottom,
		scroll: geometry.scroll,
		theme,
		horizontal: &app.readers.session.display.horizontal,
		selection: None,
		revision: app.readers.session.accepted_revision,
		hovered_link: None,
		held_overflow: None,
	};
	renderer.set_stylesheet(app.preferences.values.stylesheet.clone());
	renderer.set_fonts(app.options().fonts);
	let actual = capture_scenes(
		renderer,
		&app.readers.session.display.snapshot,
		&view,
		&overlay,
		std::slice::from_ref(&scene),
	)?;
	// Exclude unrelated chrome and paths while keeping the reference and full popup.
	Ok(image::imageops::crop_imm(
		&actual,
		(crop.x * scale) as u32,
		(crop.y * scale) as u32,
		(crop.w * scale) as u32,
		(crop.h * scale) as u32,
	)
	.to_image())
}

#[test]
fn footnote_previews_match_rendering_baselines() -> Result<()> {
	if !markview_platform::test_support::desktop_goldens() {
		return Ok(());
	}
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	check_fixtures(root)?;
	let mut baselines = Baselines::new(
		root.join("tests/goldens/hover-preview"),
		root.join("artifacts/hover-preview-goldens"),
	)?;
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	ensure!(
		renderer.adapter_name.starts_with("llvmpipe")
			&& renderer.adapter_name.ends_with("(Vulkan, Cpu)"),
		"Preview baselines require Lavapipe"
	);
	let images = images(root)?;
	for (theme, scale, suffix) in [
		(Theme::Light, 1.0, "light"),
		(Theme::Dark, 1.25, "dark-125"),
	] {
		for &(fixture, variant, fraction) in CASES {
			let mut app = preview_app(root, fixture, theme, &images)?;
			let floating = app.preview.floating_mut().unwrap();
			let max_scroll = floating.view().max_scroll();
			assert_eq!(
				max_scroll > 0.0,
				matches!(fixture, "long" | "tall-image")
			);
			assert!(floating.snapshot.width <= 420.0);
			if fixture == "short" {
				assert!(floating.snapshot.width < 200.0);
			}
			if fraction == 1.0 {
				floating.input(markview_interaction::ViewInput::SelectAll);
			}
			floating.pan(None, 0.0, max_scroll * fraction);
			let actual = capture_popup(&mut renderer, &mut app, scale)?;
			if fixture.contains("image") {
				assert!(
					actual
						.pixels()
						.filter(|p| p[0] > 180 && p[1] < 120 && p[2] < 120)
						.count()
						> 100,
					"image pixels must be present, not a placeholder"
				);
			}
			baselines
				.record(&format!("{fixture}-{variant}-{suffix}"), &actual)?;
		}
	}
	baselines.finish()
}
