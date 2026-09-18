use super::GlyphOrigin;

#[test]
fn outline_colors_convert_to_straight_alpha() {
	let mut rgba = [64, 32, 0, 128, 90, 80, 70, 255, 1, 2, 3, 0];
	super::color::unpremultiply(&mut rgba);
	assert_eq!(rgba, [128, 64, 0, 128, 90, 80, 70, 255, 0, 0, 0, 0]);
}

#[test]
fn glyph_texels_align_at_integer_and_fractional_dpi() {
	for scale in [1.0, 1.25, 1.6, 2.0, 3.0] {
		for n in -2000..2000 {
			let x = n as f32 / 37.0;
			let y = n as f32 / 29.0;
			let origin = GlyphOrigin::new(x, y, scale);
			assert_eq!(origin.x.fract(), 0.0);
			assert_eq!(origin.y.fract(), 0.0);
			assert!(origin.phase < 4);
			let raster_x = origin.x + origin.phase as f32 / 4.0;
			assert!((raster_x - x * scale).abs() <= 0.12501);
			assert!((origin.y - y * scale).abs() <= 0.50001);
		}
	}
}

#[test]
fn subpixel_phase_carries_across_pixel_and_zero_boundaries() {
	for (x, expected_x, expected_phase) in [
		(0.99, 1.0, 0),
		(0.74, 0.0, 3),
		(-0.26, -1.0, 3),
		(-0.01, 0.0, 0),
		(-1.01, -1.0, 0),
	] {
		let origin = GlyphOrigin::new(x, 0.0, 1.0);
		assert_eq!(origin.x, expected_x);
		assert_eq!(origin.phase, expected_phase);
	}
}

#[test]
fn whole_pixel_translation_reuses_raster_phase() {
	for phase in 0..4 {
		let x = phase as f32 / 4.0;
		for shift in -10..10 {
			let origin = GlyphOrigin::new(x + shift as f32, 0.0, 1.0);
			assert_eq!(origin.phase, phase);
			assert_eq!(origin.x, shift as f32);
		}
	}
}

#[test]
#[ignore = "requires a GPU; validates color glyph pixels and atlas reset"]
fn color_glyphs_preserve_rgb_and_share_paint_order() {
	use super::{Entry, RasterKey, color::ColorAtlas};
	use crate::{Renderer, Theme, View};
	use markview_core::{
		document::fingerprint,
		scene::{Draw, Glyph, LayoutSnapshot, Paint, Rect},
	};
	let mut renderer = pollster::block_on(Renderer::new(None)).unwrap();
	assert_eq!(renderer.raster.color_bytes(), 0);
	let mut atlas =
		ColorAtlas::new(&renderer.gpu.device, &renderer.images.pipeline);
	let large = Entry {
		w: 510,
		h: 510,
		..Default::default()
	};
	assert!(
		atlas
			.insert(&renderer.gpu.queue, large, &vec![255; 510 * 510 * 4])
			.is_some()
	);
	assert!(
		atlas
			.insert(
				&renderer.gpu.queue,
				Entry {
					w: 2,
					h: 2,
					..Default::default()
				},
				&[255; 16]
			)
			.is_none()
	);
	atlas.reset();
	let entry = atlas
		.insert(
			&renderer.gpu.queue,
			Entry {
				w: 2,
				h: 2,
				..Default::default()
			},
			&[
				255, 0, 0, 255, 255, 255, 255, 255, 0, 255, 0, 255, 0, 0, 255,
				255,
			],
		)
		.unwrap();
	assert_eq!((entry.x, entry.y), (1, 1));
	renderer.raster.color_atlas = Some(atlas);
	assert_eq!(renderer.raster.color_bytes(), 1024 * 1024);
	let font = parley::FontData::new(
		ratex_katex_fonts::ttf_bytes("KaTeX_Main-Regular.ttf")
			.unwrap()
			.into_owned()
			.into(),
		0,
	);
	let glyph = Glyph {
		font,
		coords: Vec::new().into(),
		id: 1,
		size: 2.,
		x: 2.,
		y: 2.,
		synthetic_italic: false,
		paint: Paint::Text,
	};
	// Inject a synthetic RGBA glyph so this test needs no installed Emoji font.
	renderer.raster.cache.insert(
		RasterKey::Glyph {
			font: glyph.font.data.id(),
			index: glyph.font.index,
			id: glyph.id,
			size: 8,
			phase: 0,
			coords: fingerprint(&glyph.coords),
			synthetic: false,
		},
		entry,
	);
	let horizontal = Default::default();
	let mut view = View {
		selection: None,
		revision: 0,
		width: 8,
		height: 8,
		scale: 1.,
		scroll: 0.,
		left: 0.,
		top: 0.,
		bottom: 0.,
		theme: Theme::Light,
		horizontal: &horizontal,
		hovered_link: None,
		hovered_overflow: None,
		held_overflow: None,
	};
	for dark in [false, true] {
		view.theme = if dark { Theme::Dark } else { Theme::Light };
		let target = renderer.offscreen(8, 8);
		let overlay = [
			Draw::Glyph(glyph.clone()),
			Draw::Glyph(Glyph {
				x: 6.,
				paint: Paint::Color(markview_core::style::Color(0xff000000)),
				..glyph.clone()
			}),
			Draw::Rect(
				Rect {
					x: 2.,
					y: 3.,
					w: 1.,
					h: 1.,
				},
				Paint::Color(markview_core::style::Color(0x000000ff)),
			),
		];
		let submission = renderer
			.render(
				&LayoutSnapshot::default(),
				&view,
				&overlay,
				&target.create_view(&Default::default()),
			)
			.unwrap();
		renderer.wait(Some(submission)).unwrap();
		let path = std::env::temp_dir().join(format!(
			"markview-color-glyph-{}-{dark}.png",
			std::process::id()
		));
		renderer.save_png(&target, &path).unwrap();
		let pixels = tiny_skia::Pixmap::load_png(&path).unwrap();
		std::fs::remove_file(path).unwrap();
		let pixel = |x: usize, y: usize| {
			&pixels.data()[(y * 8 + x) * 4..(y * 8 + x + 1) * 4]
		};
		assert_eq!(pixel(2, 2), [255, 0, 0, 255]);
		assert_eq!(pixel(3, 2), [255, 255, 255, 255]);
		assert_eq!(pixel(2, 3), [0, 0, 0, 255]);
		assert_eq!(pixel(3, 3), [0, 0, 255, 255]);
		assert_eq!(pixel(6, 2), pixel(0, 0));
	}
	renderer.clear_raster_cache();
	assert!(renderer.raster.cache.is_empty());
}
