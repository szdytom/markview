//! Regression test for the preview pass destination viewport: the card rect
//! the chrome authors is translated by `Renderer::ui_origin` (safe-area
//! insets), so the preview pass must compose into the translated rectangle.
//!
//! The card rect starts at y = 0, and the origin is applied only to the
//! destination viewport, so the untranslated and translated rectangles only
//! partially overlap. Rendering the preview above its reserved body would
//! cover the card title.

use markview_layout::{
	background::Direct,
	fonts::FontConfig,
	layout::{LayoutEngine, LayoutOptions},
	scene::{LayoutSnapshot, Rect},
	style::Stylesheet,
};
use markview_render::{Preview, Readback, Renderer, Theme, View};
use std::{collections::HashMap, sync::Arc};

fn fonts() -> FontConfig {
	FontConfig::from_faces(
		0x7072_6576_6965_7700,
		[
			include_bytes!(
				"../../markview-layout/tests/fonts/NotoSerif-Regular-subset.otf"
			)
			.as_slice(),
			include_bytes!(
				"../../markview-layout/tests/fonts/NotoSans-Bold-subset.otf"
			)
			.as_slice(),
			include_bytes!(
				"../../markview-layout/tests/fonts/NotoSansMono-Regular-subset.otf"
			)
			.as_slice(),
		]
		.into_iter()
		.map(|data| parley::fontique::Blob::new(Arc::new(data)))
		.collect(),
	)
}

fn view<'a>(
	scale: f32,
	horizontal: &'a HashMap<(usize, usize), f32>,
) -> View<'a> {
	View {
		width: (400.0 * scale) as u32,
		height: (600.0 * scale) as u32,
		scale,
		left: 20.0,
		top: 20.0,
		bottom: 20.0,
		scroll: 0.0,
		theme: Theme::Light,
		horizontal,
		selection: None,
		revision: 1,
		hovered_link: None,
		held_overflow: None,
	}
}

/// Pixels of a readback inside a device-pixel rectangle.
fn pixels(
	readback: &Readback,
	x0: usize,
	y0: usize,
	x1: usize,
	y1: usize,
) -> Vec<u8> {
	let width = readback.width as usize;
	let mut out = Vec::new();
	for y in y0..y1.min(readback.height as usize) {
		for x in x0..x1.min(width) {
			let i = (y * width + x) * 4;
			out.extend_from_slice(&readback.rgba[i..i + 4]);
		}
	}
	out
}

fn differing(left: &[u8], right: &[u8]) -> usize {
	left.iter().zip(right).filter(|(a, b)| a != b).count()
		+ left.len().abs_diff(right.len())
}

/// The renderer draws the preview pass into `rect`, translated by the UI
/// origin, then reads the target back.
#[allow(clippy::too_many_arguments)]
fn compose(
	renderer: &mut Renderer,
	main: &LayoutSnapshot,
	frame: &View<'_>,
	preview_snapshot: &LayoutSnapshot,
	rect: Rect,
	origin: (f32, f32),
	horizontal: &HashMap<(usize, usize), f32>,
) -> anyhow::Result<Readback> {
	let texture = renderer.offscreen(frame.width, frame.height);
	let target = texture.create_view(&Default::default());
	renderer.set_ui_origin(origin);
	let preview = Preview {
		snapshot: preview_snapshot,
		scroll: 0.0,
		rect,
		theme: Theme::Light,
		horizontal,
	};
	renderer.render_with_preview(main, frame, &[], Some(&preview), &target)?;
	renderer.wait(None)?;
	renderer.read_pixels(&texture)
}

/// One inset case. `rect` is the card body the chrome authored, in the
/// overlay's logical coordinates; the renderer must place it at `rect +
/// origin` before scaling to device pixels.
struct Case {
	scale: f32,
	origin: (f32, f32),
	rect: Rect,
}

#[test]
fn the_preview_lands_in_the_translated_card_rect() -> anyhow::Result<()> {
	let stylesheet = Stylesheet::bundled(false);
	let mut engine =
		LayoutEngine::with_executor(Arc::new(Direct), Arc::new(|| {}));
	let document = markview_commonmark::parse(
		"# Card body\n\nPreview text with enough words to draw ink near the top of the card.\n",
	);
	let options = LayoutOptions {
		width: 360.0,
		font_size: 18.0,
		fonts: fonts(),
		stylesheet: stylesheet.clone(),
		..Default::default()
	};
	let preview_snapshot = engine.layout(&document, &options);
	let preview_snapshot = if engine.wait_highlights() {
		engine.layout(&document, &options)
	} else {
		preview_snapshot
	};

	let mut renderer = pollster::block_on(Renderer::new(None))?;
	renderer.set_stylesheet(stylesheet);
	let horizontal = HashMap::new();
	let main = LayoutSnapshot::default();

	let cases = [
		// No inset: the viewport is exactly the authored rect.
		Case {
			scale: 1.0,
			origin: (0.0, 0.0),
			rect: Rect {
				x: 40.0,
				y: 0.0,
				w: 200.0,
				h: 100.0,
			},
		},
		// Vertical safe-area inset only.
		Case {
			scale: 1.0,
			origin: (0.0, 48.0),
			rect: Rect {
				x: 40.0,
				y: 0.0,
				w: 200.0,
				h: 100.0,
			},
		},
		// Both axes inset.
		Case {
			scale: 1.0,
			origin: (20.0, 48.0),
			rect: Rect {
				x: 40.0,
				y: 0.0,
				w: 200.0,
				h: 100.0,
			},
		},
		// The same vertical inset at device scale 2: 48 logical -> 96 px.
		Case {
			scale: 2.0,
			origin: (0.0, 48.0),
			rect: Rect {
				x: 40.0,
				y: 0.0,
				w: 200.0,
				h: 100.0,
			},
		},
	];

	for case in &cases {
		let frame = view(case.scale, &horizontal);
		let texture = renderer.offscreen(frame.width, frame.height);
		let target = texture.create_view(&Default::default());
		renderer.set_ui_origin((0.0, 0.0));
		let submission = renderer.render(&main, &frame, &[], &target)?;
		renderer.wait(Some(submission))?;
		let plain = renderer.read_pixels(&texture)?;
		drop(target);

		let (ox, oy) = case.origin;
		let dpi = case.scale;
		let translated = compose(
			&mut renderer,
			&main,
			&frame,
			&preview_snapshot,
			case.rect,
			case.origin,
			&horizontal,
		)?;
		let px = |v: f32| (v * dpi).round() as usize;
		let x0 = px(case.rect.x + ox);
		let y0 = px(case.rect.y + oy);
		let x1 = px(case.rect.x + ox + case.rect.w);
		let y1 = px(case.rect.y + oy + case.rect.h);
		// The translated card really draws ink.
		let card = pixels(&translated, x0, y0, x1, y1);
		let card_plain = pixels(&plain, x0, y0, x1, y1);
		assert!(
			differing(&card, &card_plain) > 100,
			"the translated preview card drew nothing at origin {:?} dpi {}",
			case.origin,
			dpi
		);

		// The untranslated position is where the card title lives: no preview
		// pixels may land there. For a vertical inset that is the band above
		// the translated card, over the authored card width.
		if oy > 0.0 {
			let top = pixels(
				&translated,
				px(case.rect.x),
				0,
				px(case.rect.x + case.rect.w),
				px(case.rect.y + oy),
			);
			let top_plain = pixels(
				&plain,
				px(case.rect.x),
				0,
				px(case.rect.x + case.rect.w),
				px(case.rect.y + oy),
			);
			assert!(
				differing(&top, &top_plain) == 0,
				"the preview drew {} bytes above its translated card at origin {:?} dpi {}",
				differing(&top, &top_plain),
				case.origin,
				dpi
			);
		}
		// For an X inset the authored card's left edge must stay clear too.
		if ox > 0.0 {
			let left = pixels(
				&translated,
				px(case.rect.x),
				px(case.rect.y),
				px(case.rect.x + ox),
				px(case.rect.y + case.rect.h),
			);
			let left_plain = pixels(
				&plain,
				px(case.rect.x),
				px(case.rect.y),
				px(case.rect.x + ox),
				px(case.rect.y + case.rect.h),
			);
			assert!(
				differing(&left, &left_plain) == 0,
				"the preview drew {} bytes left of its translated card at origin {:?} dpi {}",
				differing(&left, &left_plain),
				case.origin,
				dpi
			);
		}
		// Reference: zero origin with the rect pre-translated by the same
		// amount. If the renderer applied the origin twice, these differ.
		let reference_rect = Rect {
			x: case.rect.x + ox,
			y: case.rect.y + oy,
			..case.rect
		};
		let reference = compose(
			&mut renderer,
			&main,
			&frame,
			&preview_snapshot,
			reference_rect,
			(0.0, 0.0),
			&horizontal,
		)?;
		assert_eq!(
			differing(&translated.rgba, &reference.rgba),
			0,
			"preview at origin {:?} dpi {} does not match a pre-translated zero-origin rect",
			case.origin,
			dpi
		);
	}

	// Zero origin is unchanged: the card occupies exactly the authored rect
	// and nothing outside it changes.
	{
		let case = &cases[0];
		let frame = view(case.scale, &horizontal);
		let texture = renderer.offscreen(frame.width, frame.height);
		let target = texture.create_view(&Default::default());
		renderer.set_ui_origin((0.0, 0.0));
		let submission = renderer.render(&main, &frame, &[], &target)?;
		renderer.wait(Some(submission))?;
		let plain = renderer.read_pixels(&texture)?;
		let composed = compose(
			&mut renderer,
			&main,
			&frame,
			&preview_snapshot,
			case.rect,
			(0.0, 0.0),
			&horizontal,
		)?;
		let px = |v: f32| v.round() as usize;
		for y in 0..composed.height as usize {
			for x in 0..composed.width as usize {
				let inside = (px(case.rect.x)..px(case.rect.x + case.rect.w))
					.contains(&x) && (px(case.rect.y)
					..px(case.rect.y + case.rect.h))
					.contains(&y);
				if inside {
					continue;
				}
				let i = (y * composed.width as usize + x) * 4;
				assert_eq!(
					composed.rgba[i..i + 4],
					plain.rgba[i..i + 4],
					"zero-origin preview touched the frame outside its rect at ({x}, {y})"
				);
			}
		}
	}
	Ok(())
}
