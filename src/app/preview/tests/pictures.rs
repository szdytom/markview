use super::*;
use markview_interaction::{Point, ViewInput};

fn assert_picture(app: &mut App<Proxy>, size: (u32, u32)) {
	let floating = app.preview.floating_mut().unwrap();
	assert_eq!(floating.padding, [0.0; 4]);
	assert_eq!(floating.body(), floating.rect);
	assert!(floating.scrollbar().is_none());
	assert_eq!(floating.view().max_scroll(), 0.0);
	assert_eq!(floating.snapshot.blocks.len(), 1);
	assert!(floating.snapshot.document_box.is_none());
	let draws: Vec<_> = floating.snapshot.blocks[0].draws().collect();
	assert_eq!(draws.len(), 1);
	let Draw::Image { rect, .. } = draws[0].1 else {
		panic!("picture draw")
	};
	assert_eq!(rect.x, 0.0);
	assert_eq!(rect.y, 0.0);
	assert_eq!((rect.w, rect.h), (floating.rect.w, floating.rect.h));
	assert!((rect.w / rect.h - size.0 as f32 / size.1 as f32).abs() < 0.001);
	let point = Point::new(
		floating.rect.x + rect.w / 2.0,
		floating.rect.y + rect.h / 2.0,
	);
	assert!(floating.view().image(point).is_some());
	floating.input(ViewInput::Scroll {
		point,
		request: markview_interaction::ScrollClock::new(|| {}).wheel(
			100.0,
			Instant::now(),
			markview_interaction::WheelMode::Direct,
		),
	});
	assert_eq!(floating.scroll.offset(), 0.0);
}

#[test]
fn standalone_picture_links_and_sidebar_previews_fit_without_scrolling() {
	for (width, height) in [(1200, 800), (80, 2400), (2400, 80), (16, 12)] {
		let dir = crate::test_support::TestDir::new();
		image::RgbaImage::from_pixel(
			width,
			height,
			image::Rgba([45, 100, 90, 255]),
		)
		.save(dir.path().join("Picture.png"))
		.unwrap();
		for sidebar in [false, true] {
			let mut app = if sidebar {
				folder::sidebar(dir.path(), Theme::Light)
			} else {
				links::file_reader(dir.path(), "Picture.png").0
			};
			let now = if sidebar {
				folder::point(&mut app, "Picture.png");
				let now = Instant::now();
				app.hover_preview(now);
				now
			} else {
				links::hover(&mut app, "Picture.png")
			};
			settle_preview(&mut app, now, |floating| {
				!floating.snapshot.images.decoded().is_empty()
			});
			assert_picture(&mut app, (width, height));
		}
	}
}

#[test]
fn markdown_with_a_picture_named_like_the_internal_source_keeps_its_text() {
	let dir = crate::test_support::TestDir::new();
	image::RgbaImage::from_pixel(40, 30, image::Rgba([45, 100, 90, 255]))
		.save_with_format(
			dir.path().join("markview-picture"),
			image::ImageFormat::Png,
		)
		.unwrap();
	std::fs::write(
		dir.path().join("other.md"),
		"# Heading\n\n![](markview-picture)\n\nFollowing paragraph.\n",
	)
	.unwrap();
	let mut app = folder::sidebar(dir.path(), Theme::Light);
	folder::point(&mut app, "other.md");
	let now = Instant::now();
	app.hover_preview(now);
	settle_preview(&mut app, now, |floating| {
		!floating.snapshot.images.decoded().is_empty()
	});
	let snapshot = &app.preview.floating().unwrap().snapshot;
	assert_eq!(snapshot.images.decoded().len(), 1);
	let text = snapshot.full_reading_text();
	assert!(text.contains("Heading"), "{text:?}");
	assert!(text.contains("Following paragraph."), "{text:?}");
}

#[test]
fn a_picture_decode_failure_after_metadata_keeps_a_visible_error() {
	let mut bytes = std::io::Cursor::new(Vec::new());
	image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
		40,
		30,
		image::Rgb([45, 100, 90]),
	))
	.write_to(&mut bytes, image::ImageFormat::Bmp)
	.unwrap();
	let mut bytes = bytes.into_inner();
	bytes.truncate(bytes.len() - 12);
	let images = crate::images::Images::failure_after_metadata_for_test(&bytes);
	let info = &images.entries["picture"];
	assert_eq!(info.size, Some((40, 30)));
	assert!(!info.loading);
	assert!(images.decoded().is_empty());
	let error = info.error.as_ref().unwrap();
	let document = markview_commonmark::parse(format!(
		"![]({})\n",
		crate::file::STANDALONE_IMAGE_SOURCE
	));
	let options = crate::layout::LayoutOptions {
		width: 420.0,
		preview: true,
		..crate::test_support::options()
	};
	let snapshot = crate::layout::LayoutEngine::new()
		.layout_with_images(&document, &options, &images);
	let labels = |snapshot: &LayoutSnapshot| {
		snapshot
			.blocks
			.iter()
			.flat_map(|block| block.layout.text.iter())
			.map(|node| node.text.as_str())
			.collect::<String>()
	};
	assert!(
		labels(&snapshot).contains(error),
		"the normal layout must contain the decoding error"
	);
	let floating = Floating::new(
		snapshot,
		document.revision(),
		Rect {
			x: 100.0,
			y: 100.0,
			w: 40.0,
			h: 20.0,
		},
		Rect {
			x: 0.0,
			y: 40.0,
			w: 800.0,
			h: 560.0,
		},
		FrameStyle {
			picture: true,
			..Default::default()
		},
	);
	assert!(
		labels(&floating.snapshot).contains(error),
		"the floating preview must keep the decoding error"
	);
	assert!(floating.scrollbar().is_none());
}

#[test]
fn clicking_a_fitted_picture_preview_opens_the_image_viewer() {
	let dir = crate::test_support::TestDir::new();
	image::RgbaImage::from_pixel(80, 2400, image::Rgba([45, 100, 90, 255]))
		.save(dir.path().join("Picture.png"))
		.unwrap();
	let (mut app, events) = links::file_reader(dir.path(), "Picture.png");
	let now = links::hover(&mut app, "Picture.png");
	settle_preview(&mut app, now, |floating| {
		!floating.snapshot.images.decoded().is_empty()
	});
	let rect = app.preview.floating().unwrap().rect;
	app.interaction
		.pointer
		.move_to((rect.x + rect.w / 2.0, rect.y + rect.h / 2.0));
	app.pointer_pressed();
	app.pointer_released();
	settle(&mut app, &events);
	app.apply_anchor();
	assert!(app.interaction.overlays.viewer_open());
	assert_eq!(
		app.readers.session.path,
		Some(dir.path().join("Picture.png").canonicalize().unwrap())
	);
}

#[test]
fn standalone_picture_previews_render_complete_images() -> anyhow::Result<()> {
	let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("artifacts/preview");
	std::fs::create_dir_all(&directory)?;
	let mut renderer = pollster::block_on(crate::render::Renderer::new(None))?;
	for (theme, name, scale) in [
		(Theme::Light, "light", 1.0),
		(Theme::Dark, "dark-125", 1.25),
	] {
		for (width, height, shape) in
			[(960, 640, "landscape"), (360, 1200, "portrait")]
		{
			let dir = crate::test_support::TestDir::new();
			let svg =
				include_str!("../../../../tests/fixtures/image-preview.svg")
					.replacen(
						"width=\"960\" height=\"640\"",
						&format!("width=\"{width}\" height=\"{height}\""),
						1,
					);
			std::fs::write(dir.path().join("Landscape.svg"), svg)?;
			let mut app = folder::sidebar(dir.path(), theme);
			folder::point(&mut app, "Landscape.svg");
			let now = Instant::now();
			app.hover_preview(now);
			settle_preview(&mut app, now, |floating| {
				!floating.snapshot.images.decoded().is_empty()
			});
			assert_picture(&mut app, (width, height));
			let rect = app.preview.floating().unwrap().rect;
			let frame = golden::capture_frame(&mut renderer, &mut app, scale)?;
			for x in [rect.x + 2.0, rect.x + rect.w - 2.0] {
				for (y, expected) in [
					(rect.y + 2.0, [230u8, 226, 207, 255]),
					(rect.y + rect.h - 2.0, [38, 78, 81, 255]),
				] {
					let pixel =
						frame.get_pixel((x * scale) as u32, (y * scale) as u32);
					assert!(
						pixel.0.iter().zip(expected).all(
							|(actual, expected)| actual.abs_diff(expected) <= 2
						),
						"the complete image must reach every corner: {pixel:?}"
					);
				}
			}
			frame
				.save(directory.join(format!("picture-{shape}-{name}.png")))?;
		}
	}
	Ok(())
}

#[test]
fn a_fractional_width_picture_keeps_its_right_edge() -> anyhow::Result<()> {
	let dir = crate::test_support::TestDir::new();
	std::fs::write(
		dir.path().join("Bands.svg"),
		r##"<svg xmlns="http://www.w3.org/2000/svg" width="41" height="2400"><rect width="41" height="2400" fill="#ff0000"/><rect x="32.8" width="8.2" height="2400" fill="#0000ff"/></svg>"##,
	)?;
	let mut app = folder::sidebar(dir.path(), Theme::Light);
	folder::point(&mut app, "Bands.svg");
	let now = Instant::now();
	app.hover_preview(now);
	settle_preview(&mut app, now, |floating| {
		!floating.snapshot.images.decoded().is_empty()
	});
	assert_picture(&mut app, (41, 2400));
	let rect = app.preview.floating().unwrap().rect;
	assert!((rect.w - 5.466667).abs() < 0.0001);
	let mut renderer = pollster::block_on(crate::render::Renderer::new(None))?;
	let frame = golden::capture_frame(&mut renderer, &mut app, 1.0)?;
	let x = (rect.x + rect.w).floor() as u32 - 1;
	let sample = (x as f32 + 0.5 - rect.x) / rect.w;
	assert!(
		(0.8..1.0).contains(&sample),
		"the pixel center must lie inside the right-hand band"
	);
	let pixel = frame.get_pixel(x, (rect.y + rect.h / 2.0) as u32);
	assert!(
		pixel[2] > pixel[0],
		"the right-hand blue band must survive: {pixel:?}"
	);
	Ok(())
}
