//! Regression tests for the image texture budget and version retention.
//!
//! The renderer keeps a bounded cache of document image textures. A hover
//! preview must not evict the textures the main frame published as current,
//! and a preview of the active tab (same document generation) must validate
//! texture versions exactly like the main frame does.

use markview_layout::{
	image::{ImageInfo, ImagePixels, ImageSnapshot, Pixels},
	scene::{BlockLayout, Draw, LayoutSnapshot, PlacedBlock, Rect},
	style::Stylesheet,
};
use markview_render::{Preview, Renderer, Theme, View};
use std::{collections::HashMap, sync::Arc};

/// One decoded picture. The renderer uploads the bytes it is given, so the
/// square side is the whole texture size regardless of how it is drawn.
fn picture(side: u32) -> Arc<Pixels> {
	let bytes = side as usize * side as usize * 4;
	Arc::new(Pixels {
		width: side,
		height: side,
		rgba: Arc::from(vec![0x40u8; bytes]),
	})
}

/// A placed block holding one image draw `w` by `h` logical pixels.
fn image_block(
	id: u64,
	alias: &str,
	version: u64,
	y: f32,
	w: f32,
	h: f32,
) -> PlacedBlock {
	let layout = Arc::new(BlockLayout {
		draws: vec![Draw::Image {
			src: alias.to_owned(),
			version,
			rect: Rect {
				x: 0.0,
				y: 0.0,
				w,
				h,
			},
			title: String::new(),
		}]
		.into(),
		height: h,
		width: w,
		..Default::default()
	});
	PlacedBlock {
		flow: Arc::new(layout.resolve_flow(&Default::default(), false)),
		id: markview_layout::document::BlockId::new(id as u32 + 1).unwrap(),
		content: Default::default(),
		y,
		layout,
	}
}

fn snapshot(
	generation: u64,
	blocks: Vec<PlacedBlock>,
	pixels: &Arc<ImagePixels>,
	entries: HashMap<String, ImageInfo>,
	width: f32,
	height: f32,
) -> LayoutSnapshot {
	LayoutSnapshot {
		images: ImageSnapshot {
			generation,
			keys: HashMap::new(),
			entries,
			pixels: pixels.clone(),
		},
		blocks,
		width,
		height,
		..Default::default()
	}
}

fn view<'a>(
	width: u32,
	height: u32,
	horizontal: &'a HashMap<(usize, usize), f32>,
) -> View<'a> {
	View {
		selection: None,
		revision: 0,
		width,
		height,
		scale: 1.0,
		scroll: 0.0,
		left: 0.0,
		top: 0.0,
		bottom: 0.0,
		theme: Theme::Light,
		horizontal,
		hovered_link: None,
		held_overflow: None,
	}
}

/// Main-frame textures must survive the preview's budget eviction: the next
/// main frame may not have to re-upload them, and their published demand must
/// stay `needs_pixels: false`.
#[test]
fn preview_budget_eviction_keeps_the_main_frame_textures() -> anyhow::Result<()>
{
	const SIDE: u32 = 4096;
	const IMAGE: u64 = SIDE as u64 * SIDE as u64 * 4;
	let stylesheet = Stylesheet::bundled(false);
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	renderer.set_stylesheet(stylesheet);

	let horizontal = HashMap::new();
	let frame = view(600, 600, &horizontal);
	let main_pixels = Arc::new(ImagePixels::default());
	let mut main_blocks = Vec::new();
	let mut main_entries = HashMap::new();
	for i in 0..3u64 {
		let alias = format!("main-{i}.png");
		main_pixels.insert(alias.clone(), 1, picture(SIDE));
		main_entries.insert(
			alias.clone(),
			ImageInfo {
				loading: false,
				version: 1,
				size: Some((SIDE, SIDE)),
				error: None,
				svg: false,
			},
		);
		main_blocks.push(image_block(
			i,
			&alias,
			1,
			120.0 * i as f32,
			200.0,
			100.0,
		));
	}
	let main =
		snapshot(1, main_blocks, &main_pixels, main_entries, 600.0, 360.0);

	let preview_pixels = Arc::new(ImagePixels::default());
	let mut preview_blocks = Vec::new();
	let mut preview_entries = HashMap::new();
	for i in 0..2u64 {
		let alias = format!("preview-{i}.png");
		preview_pixels.insert(alias.clone(), 1, picture(SIDE));
		preview_entries.insert(
			alias.clone(),
			ImageInfo {
				loading: false,
				version: 1,
				size: Some((SIDE, SIDE)),
				error: None,
				svg: false,
			},
		);
		preview_blocks.push(image_block(
			i,
			&alias,
			1,
			120.0 * i as f32,
			200.0,
			100.0,
		));
	}
	let preview_snapshot = snapshot(
		2,
		preview_blocks,
		&preview_pixels,
		preview_entries,
		600.0,
		240.0,
	);

	let target = renderer.offscreen(frame.width, frame.height);
	let target = target.create_view(&Default::default());

	let submission = renderer.render(&main, &frame, &[], &target)?;
	renderer.wait(Some(submission))?;
	let main_bytes = 3 * IMAGE;
	assert!(
		renderer.gpu_bytes() >= main_bytes,
		"the main frame did not upload its three pictures: {} < {main_bytes}",
		renderer.gpu_bytes()
	);

	let preview = Preview {
		snapshot: &preview_snapshot,
		scroll: 0.0,
		rect: Rect {
			x: 0.0,
			y: 0.0,
			w: 200.0,
			h: 150.0,
		},
		theme: Theme::Light,
		horizontal: &horizontal,
	};
	renderer.render_with_preview(
		&main,
		&frame,
		&[],
		Some(&preview),
		&target,
	)?;
	renderer.wait(None)?;
	// The preview is over budget; it may decline its second picture, but it
	// must not free the main frame's textures to make room.
	let composed = renderer.gpu_bytes();
	assert!(
		composed >= main_bytes,
		"the preview evicted the main frame's textures: {composed} < {main_bytes}"
	);

	// The composed frame published `needs_pixels: false` for the main
	// pictures, so the loader may drop their decoded CPU pixels. Without the
	// GPU textures the next frame would have to ask for them again.
	main_pixels.replace(HashMap::new());
	renderer.render(&main, &frame, &[], &target)?;
	renderer.wait(None)?;
	let demand = main_pixels.demand(main.images.generation);
	for i in 0..3 {
		let alias = format!("main-{i}.png");
		let demand = demand.get(&alias).unwrap_or_else(|| {
			panic!("the next main frame did not draw {alias}: {demand:?}")
		});
		assert!(
			!demand.needs_pixels,
			"the next main frame re-demanded pixels for {alias}: {demand:?}"
		);
	}
	Ok(())
}

/// A preview of the active tab shares the main generation, so its texture
/// versions must be validated like the main frame's instead of being kept
/// for their generation alone. Each reload must replace, not accumulate.
#[test]
fn preview_reloads_of_the_active_tab_do_not_accumulate_textures()
-> anyhow::Result<()> {
	let stylesheet = Stylesheet::bundled(false);
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	renderer.set_stylesheet(stylesheet);

	let horizontal = HashMap::new();
	let frame = view(200, 200, &horizontal);
	let pixels = Arc::new(ImagePixels::default());
	let alias = "active.png";
	let target = renderer.offscreen(frame.width, frame.height);
	let target = target.create_view(&Default::default());
	let mut stable = None;
	for version in 1..=4u64 {
		pixels.insert(alias.to_owned(), version, picture(512));
		let snapshot = snapshot(
			1,
			vec![image_block(0, alias, version, 0.0, 100.0, 100.0)],
			&pixels,
			HashMap::from([(
				alias.to_owned(),
				ImageInfo {
					loading: false,
					version,
					size: Some((512, 512)),
					error: None,
					svg: false,
				},
			)]),
			200.0,
			200.0,
		);
		// Hovering the active tab: the preview draws the same snapshot, so the
		// guest generation equals the main generation.
		let preview = Preview {
			snapshot: &snapshot,
			scroll: 0.0,
			rect: Rect {
				x: 0.0,
				y: 0.0,
				w: 100.0,
				h: 100.0,
			},
			theme: Theme::Light,
			horizontal: &horizontal,
		};
		renderer.render_with_preview(
			&snapshot,
			&frame,
			&[],
			Some(&preview),
			&target,
		)?;
		renderer.wait(None)?;
		let bytes = renderer.gpu_bytes();
		if let Some(stable) = stable {
			assert_eq!(
				bytes, stable,
				"textures grew after reloading {alias} at version {version}: the obsolete version was kept"
			);
		} else {
			assert!(
				bytes >= 512 * 512 * 4,
				"the picture was not uploaded: {bytes}"
			);
			stable = Some(bytes);
		}
	}
	Ok(())
}
