use super::*;
use proptest::prelude::*;

#[test]
fn text_start_tracks_the_icon_center_instead_of_the_tooltip_width() {
	let anchor = Rect {
		x: 200.,
		y: 100.,
		w: 28.,
		h: 28.,
	};
	for control_width in [16., 28., 40.] {
		let anchor = Rect {
			w: control_width,
			..anchor
		};
		for width in [40., 120., 280.] {
			let below = place(anchor, (width, 33.), (800., 600.));
			assert_eq!(below.x + PAD, anchor.x + anchor.w / 2.);
			assert_eq!(below.y, anchor.y + anchor.h + GAP);
			let bottom = Rect { y: 560., ..anchor };
			let above = place(bottom, (width, 33.), (800., 600.));
			assert_eq!(above.x, below.x);
			assert_eq!(above.y + above.h, bottom.y - GAP);
		}
	}
}

#[test]
fn placement_flips_above_bottom_controls_and_clamps_at_edges() {
	let anchor = Rect {
		x: 780.,
		y: 560.,
		w: 20.,
		h: 32.,
	};
	let rect = place(anchor, (120., 33.), (800., 600.));
	assert_eq!(rect.x, 672.);
	assert_eq!(rect.y + rect.h, anchor.y - GAP);
	let top = place(
		Rect {
			x: 0.,
			y: 0.,
			..anchor
		},
		(120., 33.),
		(800., 600.),
	);
	assert_eq!(top.x, MARGIN);
	assert_eq!(top.y, 32. + GAP);
}

#[test]
fn drawing_wraps_localized_text_and_restores_the_shaper() {
	let mut ui = crate::test_support::shaper();
	let old = format!("{:?}", ui.appearance);
	let anchor = Rect {
		x: 0.,
		y: 0.,
		w: 28.,
		h: 28.,
	};
	for text in [
		"Copy code",
		"复制代码",
		"A long tooltip label ".repeat(12).as_str(),
		"第一行\n第二行",
	] {
		let out = draw(&mut ui, text, anchor, (320., 200.));
		let [Draw::Clipped { rect, draws }] = out.as_slice() else {
			panic!("clipped tooltip")
		};
		assert!(rect.x >= MARGIN && rect.x + rect.w <= 320. - MARGIN);
		assert!(rect.y >= MARGIN && rect.y + rect.h <= 200. - MARGIN);
		assert!(matches!(
			draws.first(),
			Some(Draw::Box {
				condition: Condition::Panel,
				..
			})
		));
		assert!(draws.iter().any(|draw| matches!(draw, Draw::Glyph(_))));
		assert_eq!(format!("{:?}", ui.appearance), old);
	}
}

proptest! {
	#[test]
	fn placement_always_fits_the_viewport(
		window in (1f32..2000., 1f32..2000.),
		size in (0f32..1000., 0f32..1000.),
		anchor in (-1000f32..3000., -1000f32..3000., 0f32..200., 0f32..200.),
	) {
		let rect = place(Rect { x: anchor.0, y: anchor.1, w: anchor.2, h: anchor.3 }, size, window);
		prop_assert!(rect.x >= 0. && rect.y >= 0.);
		prop_assert!(rect.w >= 0. && rect.h >= 0.);
		prop_assert!(rect.x + rect.w <= window.0 + 0.001);
		prop_assert!(rect.y + rect.h <= window.1 + 0.001);
		let margin = MARGIN.min(window.0 / 2.).min(window.1 / 2.);
		if rect.x > margin + 0.001 && rect.x + rect.w < window.0 - margin - 0.001 {
			prop_assert!((rect.x + PAD - (anchor.0 + anchor.2 / 2.)).abs() < 0.001);
		}
	}
}

#[test]
fn tooltip_gpu_ink_stays_inside_its_surface_in_light_and_dark()
-> anyhow::Result<()> {
	use crate::{
		render::{Renderer, Theme, View},
		test_support::render_goldens::capture,
	};
	use markview_layout::style::Stylesheet;
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("artifacts/tooltips");
	std::fs::create_dir_all(&output)?;
	for (name, dark) in [("light", false), ("dark", true)] {
		let mut ui = crate::test_support::shaper();
		let sheet = Stylesheet::bundled(dark);
		ui.set_stylesheet(sheet.clone());
		renderer.set_stylesheet(sheet);
		for (language, text, scale) in
			[("en", "Copy code", 1.), ("zh", "复制代码", 1.25)]
		{
			let draws = draw(
				&mut ui,
				text,
				Rect {
					x: 360.,
					y: 140.,
					w: 28.,
					h: 28.,
				},
				(400., 180.),
			);
			let Draw::Clipped {
				rect,
				draws: content,
			} = &draws[0]
			else {
				unreachable!()
			};
			let surface = vec![Draw::Clipped {
				rect: *rect,
				draws: content[..1].to_vec(),
			}];
			let view = View {
				width: (400. * scale) as u32,
				height: (180. * scale) as u32,
				scale,
				left: 0.,
				top: 0.,
				bottom: 0.,
				scroll: 0.,
				theme: if dark { Theme::Dark } else { Theme::Light },
				horizontal: &Default::default(),
				selection: None,
				revision: 1,
				hovered_link: None,
				held_overflow: None,
			};
			let empty =
				capture(&mut renderer, &Default::default(), &view, &[])?;
			let background =
				capture(&mut renderer, &Default::default(), &view, &surface)?;
			let actual =
				capture(&mut renderer, &Default::default(), &view, &draws)?;
			let mut ink = 0;
			let mut fill = 0;
			for (x, y, pixel) in actual.enumerate_pixels() {
				if pixel != background.get_pixel(x, y) {
					assert!(rect.contains(
						(x as f32 + 0.5) / scale,
						(y as f32 + 0.5) / scale
					));
					ink += 1;
				}
				if background.get_pixel(x, y) != empty.get_pixel(x, y) {
					fill += 1;
				}
			}
			assert!(ink > 20, "{name}-{language} needs visible text");
			assert!(fill > 100, "{name}-{language} needs a visible surface");
			actual.save(output.join(format!("{name}-{language}.png")))?;
		}
	}
	Ok(())
}
