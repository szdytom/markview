use super::GlyphOrigin;

#[test]
fn math_fonts_are_embedded_in_debug_builds() {
	for name in [
		"Main-Regular",
		"Math-Italic",
		"Size1-Regular",
		"Size2-Regular",
	] {
		let bytes =
			ratex_katex_fonts::ttf_bytes(&format!("KaTeX_{name}.ttf")).unwrap();
		assert!(
			matches!(bytes, std::borrow::Cow::Borrowed(_)),
			"{name} must not require the build machine's font directory"
		);
	}
}

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

/// A color emoji face stores `CBDT` strikes instead of outlines, so a download
/// of one is only useful if the scaler decodes those strikes to color. The
/// pinned subset is the same face the builtin `noto-emoji` family offers.
#[test]
fn a_color_bitmap_emoji_decodes_to_color() {
	use swash::scale::{Render, ScaleContext, Source, image::Content};
	let data: &[u8] = include_bytes!(
		"../../../markview-layout/tests/fonts/NotoColorEmoji-subset.ttf"
	);
	let font = swash::FontRef::from_index(data, 0).unwrap();
	let glyph = font.charmap().map('\u{1f600}');
	assert_ne!(glyph, 0, "the subset should map U+1F600");
	let mut context = ScaleContext::new();
	let mut scaler = context.builder(font).size(32.0).build();
	let render = Render::new(&[
		Source::ColorBitmap(swash::scale::StrikeWith::BestFit),
		Source::Outline,
	]);
	let image = render.render(&mut scaler, glyph).unwrap();
	assert_eq!(image.content, Content::Color);
	assert!(image.placement.width > 0 && image.placement.height > 0);
}

#[test]
fn color_glyphs_preserve_rgb_and_share_paint_order() {
	use super::{Entry, RasterKey, color::ColorAtlas};
	use crate::{Renderer, Theme, View};
	use markview_document::fingerprint;
	use markview_layout::scene::{Draw, Glyph, LayoutSnapshot, Paint, Rect};
	let mut renderer = pollster::block_on(Renderer::new(None)).unwrap();
	assert_eq!(renderer.raster.color_bytes(), 0);
	let mut atlas = ColorAtlas::new(
		&renderer.gpu.device,
		&renderer.images.pipeline,
		super::COLOR_ATLAS_SIZE,
	);
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
		synthetic_bold: false,
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
			synthetic_bold: false,
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
		held_overflow: None,
	};
	for dark in [false, true] {
		view.theme = if dark { Theme::Dark } else { Theme::Light };
		let target = renderer.offscreen(8, 8);
		let overlay = [
			Draw::Glyph(glyph.clone()),
			Draw::Glyph(Glyph {
				x: 6.,
				paint: Paint::Color(markview_layout::style::Color(0xff000000)),
				..glyph.clone()
			}),
			Draw::Rect(
				Rect {
					x: 2.,
					y: 3.,
					w: 1.,
					h: 1.,
				},
				Paint::Color(markview_layout::style::Color(0x000000ff)),
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

#[test]
fn prewarm_prepares_the_next_screenful_before_the_frame_needs_it() {
	use crate::{RasterStats, Renderer, Theme, View};
	use markview_layout::scene::{
		BlockLayout, Draw, Glyph, LayoutSnapshot, Paint, PlacedBlock,
	};
	use std::{collections::HashMap, sync::Arc, time::Duration};

	let mut renderer = pollster::block_on(Renderer::new(None)).unwrap();
	let font = parley::FontData::new(
		ratex_katex_fonts::ttf_bytes("KaTeX_Main-Regular.ttf")
			.unwrap()
			.into_owned()
			.into(),
		0,
	);
	let charmap = swash::FontRef::from_index(font.data.data(), 0)
		.unwrap()
		.charmap();
	// One letter per glyph, laid out eight to a row: two screenfuls of
	// distinct glyphs, the one on screen and the one below it.
	let screenful = |letters: &str, y: f32| PlacedBlock {
		flow: Default::default(),
		id: y as u64,
		source: 0..0,
		y,
		layout: Arc::new(BlockLayout {
			draws: letters
				.chars()
				.enumerate()
				.map(|(i, ch)| {
					Draw::Glyph(Glyph {
						font: font.clone(),
						coords: Vec::new().into(),
						id: charmap.map(ch),
						size: 12.0,
						x: (i % 8) as f32 * 14.0,
						y: (i / 8) as f32 * 14.0,
						synthetic_italic: false,
						synthetic_bold: false,
						paint: Paint::Text,
					})
				})
				.collect(),
			height: 60.0,
			width: 120.0,
			..Default::default()
		}),
	};
	let snapshot = LayoutSnapshot {
		blocks: vec![
			screenful("ABCDEFGHIJKLMNOPQRSTUVWX", 0.0),
			screenful("abcdefghijklmnopqrstuvwx", 60.0),
		],
		height: 120.0,
		width: 120.0,
		..Default::default()
	};
	let horizontal = HashMap::new();
	let mut view = View {
		selection: None,
		revision: 0,
		width: 120,
		height: 60,
		scale: 1.,
		scroll: 0.,
		left: 0.,
		top: 0.,
		bottom: 0.,
		theme: Theme::Light,
		horizontal: &horizontal,
		hovered_link: None,
		held_overflow: None,
	};
	let stats = |r: &Renderer| -> RasterStats { r.raster_stats() };
	let target = renderer.offscreen(120, 60);
	let draw = |renderer: &mut Renderer, view: &View<'_>| {
		let submission = renderer
			.render(
				&snapshot,
				view,
				&[],
				&target.create_view(&Default::default()),
			)
			.unwrap();
		renderer.wait(Some(submission)).unwrap();
	};
	draw(&mut renderer, &view);
	let on_screen = stats(&renderer).rasterized;
	assert!(on_screen > 0, "the first frame rasterized nothing");

	// Prewarming prepares the screenful below.
	while renderer.prewarm(&snapshot, &view, Duration::from_millis(50)) {}
	let prepared = stats(&renderer).rasterized;
	assert!(prepared > on_screen, "prewarming rasterized nothing");

	// Scrolling to it therefore adds no rasterization at all.
	view.scroll = 60.;
	draw(&mut renderer, &view);
	assert_eq!(
		stats(&renderer).rasterized,
		prepared,
		"the scroll frame rasterized glyphs prewarming should have prepared"
	);

	// An atlas reset can upload the same bitmaps again without asking Swash
	// to rasterize them a second time.
	renderer.raster.reset_mask(&renderer.gpu.queue);
	draw(&mut renderer, &view);
	assert_eq!(
		stats(&renderer).rasterized,
		prepared,
		"a reset re-rasterized glyphs that were still in the CPU cache"
	);
}

#[test]
fn prewarming_leaves_the_visible_image_demand_alone() {
	use crate::{Renderer, Theme, View};
	use markview_layout::{
		image::{ImageInfo, ImagePixels, ImageSnapshot},
		scene::{BlockLayout, Draw, LayoutSnapshot, PlacedBlock, Rect},
	};
	use std::{collections::HashMap, sync::Arc, time::Duration};

	let mut renderer = pollster::block_on(Renderer::new(None)).unwrap();
	let pixels = Arc::new(ImagePixels::default());
	for source in ["above.png", "below.png"] {
		pixels.insert(
			source.into(),
			1,
			Arc::new(markview_layout::image::Pixels {
				width: 40,
				height: 40,
				rgba: vec![255; 40 * 40 * 4].into(),
			}),
		);
	}
	let entries = ["above.png", "on-screen.png", "below.png"]
		.into_iter()
		.map(|src| {
			(
				src.to_string(),
				ImageInfo {
					version: 1,
					size: Some((40, 40)),
					error: None,
					svg: false,
				},
			)
		})
		.collect();
	let image_block = |src: &str, y: f32| PlacedBlock {
		flow: Default::default(),
		id: y as u64,
		source: 0..0,
		y,
		layout: Arc::new(BlockLayout {
			draws: vec![Draw::Image {
				src: src.into(),
				version: 1,
				rect: Rect {
					x: 0.,
					y: 0.,
					w: 40.,
					h: 40.,
				},
				title: String::new(),
			}],
			height: 40.,
			width: 120.,
			..Default::default()
		}),
	};
	let snapshot = LayoutSnapshot {
		images: ImageSnapshot {
			entries,
			pixels: pixels.clone(),
			..Default::default()
		},
		blocks: vec![
			image_block("above.png", 0.),
			image_block("on-screen.png", 60.),
			image_block("below.png", 120.),
		],
		height: 180.,
		width: 120.,
		..Default::default()
	};
	let horizontal = HashMap::new();
	let view = View {
		selection: None,
		revision: 0,
		width: 120,
		height: 60,
		scale: 1.,
		scroll: 60.,
		left: 0.,
		top: 0.,
		bottom: 0.,
		theme: Theme::Light,
		horizontal: &horizontal,
		hovered_link: None,
		held_overflow: None,
	};
	let demanded = |pixels: &ImagePixels| {
		let mut srcs: Vec<String> = pixels.demand(0).keys().cloned().collect();
		srcs.sort();
		srcs
	};
	let target = renderer.offscreen(120, 60);
	let submission = renderer
		.render(
			&snapshot,
			&view,
			&[],
			&target.create_view(&Default::default()),
		)
		.unwrap();
	renderer.wait(Some(submission)).unwrap();
	assert_eq!(demanded(&pixels), ["on-screen.png"]);
	// A prewarm pass builds the frame below, which must not replace the image
	// demand of the frame the reader is looking at.
	let above = (snapshot.images.generation, "above.png".to_string(), 1);
	let below = (snapshot.images.generation, "below.png".to_string(), 1);
	assert!(!renderer.images.contains(&above));
	assert!(!renderer.images.contains(&below));
	while renderer.prewarm(&snapshot, &view, Duration::from_millis(50)) {}
	assert!(renderer.images.contains(&above));
	assert!(renderer.images.contains(&below));
	assert_eq!(demanded(&pixels), ["on-screen.png"]);
}

#[test]
fn scrolling_icons_reuse_rasters_at_every_device_pixel_phase() {
	use crate::{Renderer, Theme, View};
	use markview_layout::scene::Rect;
	use ratex_types::PathCommand;
	let mut renderer = pollster::block_on(Renderer::new(None)).unwrap();
	let commands = [
		PathCommand::MoveTo { x: 0.2, y: 0.3 },
		PathCommand::LineTo { x: 0.8, y: 0.5 },
		PathCommand::LineTo { x: 0.2, y: 0.7 },
		PathCommand::Close,
	];
	let horizontal = Default::default();
	for scale in [1., 1.25, 1.6, 2.] {
		for fill in [false, true] {
			renderer.clear_raster_cache();
			let view = View {
				width: 800,
				height: 600,
				scale,
				left: 0.,
				top: 0.,
				bottom: 0.,
				scroll: 0.,
				theme: Theme::Light,
				horizontal: &horizontal,
				selection: None,
				revision: 0,
				hovered_link: None,
				held_overflow: None,
			};
			let clip = Rect {
				x: 0.,
				y: 0.,
				w: 800. / scale,
				h: 600. / scale,
			};
			let mut draw = |shift: i32| {
				for px in 0..4 {
					for py in 0..4 {
						renderer.geometry.clear();
						renderer.raster.icon(
							&renderer.gpu.queue,
							&mut renderer.geometry,
							&commands,
							fill,
							0.08,
							(30. + shift as f32 + px as f32 / 4.) / scale,
							(40. + shift as f32 * 2. + py as f32 / 4.) / scale,
							18.,
							[1.; 4],
							clip,
							&view,
						);
					}
				}
				renderer.raster.stats()
			};
			let warm = draw(0);
			for shift in -40..40 {
				let moved = draw(shift);
				assert_eq!(
					moved.rasterized, warm.rasterized,
					"integer translation must reuse icon coverage: scale={scale}, fill={fill}, shift={shift}"
				);
				assert_eq!(moved.entries, warm.entries);
			}
		}
	}
}

#[test]
fn high_dpi_uses_a_larger_atlas_when_the_adapter_allows_it() {
	assert_eq!(
		super::atlas_sizes(1.0, 8192),
		(super::ATLAS_SIZE, super::COLOR_ATLAS_SIZE)
	);
	assert_eq!(super::atlas_sizes(2.0, 8192), (4096, 1024));
	assert_eq!(
		super::atlas_sizes(2.0, 2048),
		(super::ATLAS_SIZE, super::COLOR_ATLAS_SIZE)
	);
}

#[test]
fn resetting_one_atlas_keeps_the_other_cache() {
	use super::{Entry, RasterKey};
	use crate::Renderer;
	let mut renderer = pollster::block_on(Renderer::new(None)).unwrap();
	let mask = RasterKey::Path {
		path: 1,
		size: 8,
		tx: 0,
		ty: 0,
		w: 1,
		h: 1,
	};
	let color = RasterKey::Path {
		path: 2,
		size: 8,
		tx: 0,
		ty: 0,
		w: 1,
		h: 1,
	};
	renderer.raster.cache.insert(
		mask.clone(),
		Entry {
			color: false,
			..Default::default()
		},
	);
	renderer.raster.cache.insert(
		color.clone(),
		Entry {
			color: true,
			..Default::default()
		},
	);
	renderer.raster.reset_color();
	assert!(renderer.raster.cache.contains_key(&mask));
	assert!(!renderer.raster.cache.contains_key(&color));
	renderer.raster.reset_mask(&renderer.gpu.queue);
	assert!(!renderer.raster.cache.contains_key(&mask));
}

#[test]
fn idle_repack_rebuilds_the_current_screen_without_a_scroll_frame() {
	use crate::{Renderer, Theme, View};
	use markview_layout::scene::{
		BlockLayout, Draw, Glyph, LayoutSnapshot, Paint, PlacedBlock,
	};
	use std::{collections::HashMap, sync::Arc, time::Duration};
	let mut renderer = pollster::block_on(Renderer::new(None)).unwrap();
	let font = parley::FontData::new(
		ratex_katex_fonts::ttf_bytes("KaTeX_Main-Regular.ttf")
			.unwrap()
			.into_owned()
			.into(),
		0,
	);
	let charmap = swash::FontRef::from_index(font.data.data(), 0)
		.unwrap()
		.charmap();
	let screenful = |letters: &str, y: f32| PlacedBlock {
		flow: Default::default(),
		id: y as u64,
		source: 0..0,
		y,
		layout: Arc::new(BlockLayout {
			draws: letters
				.chars()
				.enumerate()
				.map(|(i, ch)| {
					Draw::Glyph(Glyph {
						font: font.clone(),
						coords: Vec::new().into(),
						id: charmap.map(ch),
						size: 12.0,
						x: (i % 8) as f32 * 14.0,
						y: (i / 8) as f32 * 14.0,
						synthetic_italic: false,
						synthetic_bold: false,
						paint: Paint::Text,
					})
				})
				.collect(),
			height: 60.0,
			width: 120.0,
			..Default::default()
		}),
	};
	let snapshot = LayoutSnapshot {
		blocks: vec![
			screenful("ABCDEFGHIJKLMNOPQRSTUVWX", 0.0),
			screenful("abcdefghijklmnopqrstuvwx", 60.0),
		],
		height: 120.0,
		width: 120.0,
		..Default::default()
	};
	let horizontal = HashMap::new();
	let view = View {
		selection: None,
		revision: 0,
		width: 120,
		height: 60,
		scale: 1.,
		scroll: 0.,
		left: 0.,
		top: 0.,
		bottom: 0.,
		theme: Theme::Light,
		horizontal: &horizontal,
		hovered_link: None,
		held_overflow: None,
	};
	let target = renderer.offscreen(120, 60);
	let submission = renderer
		.render(
			&snapshot,
			&view,
			&[],
			&target.create_view(&Default::default()),
		)
		.unwrap();
	renderer.wait(Some(submission)).unwrap();
	let before = renderer.raster_stats();
	// Simulate the shelf having reached the repack threshold.
	renderer.raster.shelf = (2, super::ATLAS_SIZE * 95 / 100, 2);
	renderer.maintain(&snapshot, &view);
	while renderer.prewarm(&snapshot, &view, Duration::from_millis(50)) {}
	let after = renderer.raster_stats();
	assert!(
		after.resets > before.resets,
		"idle repack did not run: {before:?} -> {after:?}"
	);
	assert!(
		after.entries >= before.entries,
		"idle repack lost the current screen: {before:?} -> {after:?}"
	);
}

#[test]
fn synthetic_bold_and_italic_have_distinct_cached_masks() {
	use crate::Renderer;
	use markview_layout::scene::{Glyph, Paint};
	let mut renderer = pollster::block_on(Renderer::new(None)).unwrap();
	let font = parley::FontData::new(
		ratex_katex_fonts::ttf_bytes("KaTeX_Main-Regular.ttf")
			.unwrap()
			.into_owned()
			.into(),
		0,
	);
	let id = swash::FontRef::from_index(font.data.data(), 0)
		.unwrap()
		.charmap()
		.map('A');
	let mut glyph = Glyph {
		font,
		coords: Vec::new().into(),
		id,
		size: 32.,
		x: 0.,
		y: 0.,
		synthetic_italic: false,
		synthetic_bold: false,
		paint: Paint::Text,
	};
	for (bold, italic) in
		[(false, false), (true, false), (false, true), (true, true)]
	{
		glyph.synthetic_bold = bold;
		glyph.synthetic_italic = italic;
		let entry = renderer
			.raster
			.glyph(
				&renderer.gpu.device,
				&renderer.images.pipeline,
				&renderer.gpu.queue,
				&glyph,
				1.,
				0,
			)
			.unwrap();
		assert!(entry.w > 0 && entry.h > 0);
	}
	assert_eq!(renderer.raster.cache.len(), 4);
}
