use markview_layout::{
	background::Direct,
	fonts::FontConfig,
	layout::{LayoutEngine, LayoutOptions},
	style::Stylesheet,
};
use markview_render::{Renderer, Theme, View};
use std::{collections::HashMap, sync::Arc, time::Duration};

fn fonts() -> FontConfig {
	FontConfig::from_faces(
		0x72656e646572,
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
	theme: Theme,
	scale: f32,
	horizontal: &'a HashMap<(usize, usize), f32>,
) -> View<'a> {
	View {
		width: (400.0 * scale) as u32,
		height: (500.0 * scale) as u32,
		scale,
		left: 20.0,
		top: 20.0,
		bottom: 20.0,
		scroll: 0.0,
		theme,
		horizontal,
		selection: None,
		revision: 1,
		hovered_link: None,
		held_overflow: None,
	}
}

#[test]
fn markdown_renders_visible_and_identical_cached_frames() -> anyhow::Result<()>
{
	let doc = markview_commonmark::parse(
		"# Rendering\n\nA paragraph with $x^2$.\n\n- first\n- second\n\n```rust\nlet answer = 42;\n```\n",
	);
	let mut engine =
		LayoutEngine::with_executor(Arc::new(Direct), Arc::new(|| {}));
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	eprintln!("Render test adapter: {}", renderer.adapter_name);
	let horizontal = HashMap::new();
	for (theme, scale) in [(Theme::Light, 1.0), (Theme::Dark, 1.25)] {
		let sheet = Stylesheet::bundled(theme == Theme::Dark);
		renderer.set_stylesheet(sheet.clone());
		let options = LayoutOptions {
			width: 360.0,
			fonts: fonts(),
			stylesheet: sheet,
			..Default::default()
		};
		let snapshot = engine.layout(&doc, &options);
		let snapshot = if engine.wait_highlights() {
			engine.layout(&doc, &options)
		} else {
			snapshot
		};
		let view = view(theme, scale, &horizontal);
		let target = renderer.offscreen(view.width, view.height);
		let submission = renderer.render(
			&snapshot,
			&view,
			&[],
			&target.create_view(&Default::default()),
		)?;
		renderer.wait(Some(submission))?;
		let first = renderer.read_pixels(&target)?;
		assert_eq!((first.width, first.height), (view.width, view.height));
		let background = &first.rgba[..4];
		let visible = first
			.rgba
			.as_chunks::<4>()
			.0
			.iter()
			.filter(|p| p.as_slice() != background)
			.count();
		assert!(visible > 1000, "Markdown frame is empty: {visible} pixels");
		let cached = engine.layout(&doc, &options);
		assert_eq!(cached.reused, cached.blocks.len());
		let submission = renderer.render(
			&cached,
			&view,
			&[],
			&target.create_view(&Default::default()),
		)?;
		renderer.wait(Some(submission))?;
		assert_eq!(first.rgba, renderer.read_pixels(&target)?.rgba);
	}
	Ok(())
}

#[test]
fn merged_table_borders_have_single_stroke_coverage_at_every_scale()
-> anyhow::Result<()> {
	use markview_layout::{scene::Draw, style::Condition};
	let doc = markview_commonmark::parse(concat!(
		"+-------+-------+-------+\n",
		"| Wide          | Tall  |\n",
		"+-------+-------+       +\n",
		"| Left  | Mid   | cell  |\n",
		"+       +-------+-------+\n",
		"| cell  | Wide again    |\n",
		"+-------+-------+-------+\n",
	));
	let mut sheet = (*Stylesheet::bundled(false)).clone();
	sheet.merge(&Stylesheet::parse(concat!(
		"format_version=2\nversion=1\n",
		"[[rule]]\nwhen=['body']\nbackground='#FFFFFF'\n",
		"[[rule]]\nwhen=['table','cell']\nborder_width=2\n",
		"border_color='#00000080'\nbackground='#FFFFFF'\n",
	))?);
	let sheet = Arc::new(sheet);
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	renderer.set_stylesheet(sheet.clone());
	let options = LayoutOptions {
		width: 360.,
		fonts: fonts(),
		stylesheet: sheet,
		..Default::default()
	};
	let snapshot = LayoutEngine::new().layout(&doc, &options).flattened();
	let cells: Vec<_> = snapshot.blocks[0]
		.layout
		.draws
		.iter()
		.filter_map(|draw| match draw {
			Draw::Box {
				rect,
				condition: Condition::Cell,
				..
			} => Some(rect),
			_ => None,
		})
		.collect();
	assert_eq!(cells.len(), 5);
	let horizontal = HashMap::new();
	for scale in [1., 1.25, 1.5, 2.] {
		let view = view(Theme::Light, scale, &horizontal);
		let target = renderer.offscreen(view.width, view.height);
		let submission = renderer.render(
			&snapshot,
			&view,
			&[],
			&target.create_view(&Default::default()),
		)?;
		renderer.wait(Some(submission))?;
		let pixels = renderer.read_pixels(&target)?;
		let x = ((cells[0].x + cells[0].w / 2. + view.left) * scale) as u32;
		let y = ((cells[0].y + 0.5 + view.top) * scale) as u32;
		// The outer edge provides one stroke's sRGB intensity.
		let single = pixels.rgba[(y * pixels.width + x) as usize * 4];
		assert!(single > 0 && single < 255);
		let darkness = f32::from(255 - single);
		let mut checked = 0;
		for a in &cells {
			for b in &cells {
				let overlap_x = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
				let overlap_y = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
				let sample = if (a.y + a.h - b.y).abs() < 0.001
					&& overlap_x > 0.001
				{
					Some((a.x.max(b.x) + overlap_x / 2., b.y, true))
				} else if (a.x + a.w - b.x).abs() < 0.001 && overlap_y > 0.001 {
					Some((b.x, a.y.max(b.y) + overlap_y / 2., false))
				} else {
					None
				};
				let Some((x, y, horizontal)) = sample else {
					continue;
				};
				let x = ((x + view.left) * scale).floor() as u32;
				let y = ((y + view.top) * scale).floor() as u32;
				let mut coverage = 0;
				for offset in -8..=8 {
					let (x, y) = if horizontal {
						(x, y.checked_add_signed(offset).unwrap())
					} else {
						(x.checked_add_signed(offset).unwrap(), y)
					};
					let at = (y * pixels.width + x) as usize * 4;
					let pixel = &pixels.rgba[at..at + 4];
					// A translucent stroke exposes any accidental second paint.
					assert!(
						pixel[0] >= single.saturating_sub(1),
						"border painted twice: {pixel:?}"
					);
					coverage += 255 - u32::from(pixel[0]);
				}
				assert!(
					(coverage as f32 - darkness * 2. * scale).abs() <= darkness,
					"scale={scale}: border coverage {coverage} at ({x}, {y})"
				);
				checked += 1;
			}
		}
		assert_eq!(checked, 8);
	}
	Ok(())
}

#[test]
fn segmented_borders_stay_below_cell_text_with_zero_padding()
-> anyhow::Result<()> {
	let doc = markview_commonmark::parse(concat!(
		"+-------+-------+\n",
		"| HHHH  | Top   |\n",
		"+       +-------+\n",
		"|       | Mid   |\n",
		"+       +-------+\n",
		"|       | Last  |\n",
		"+-------+-------+\n",
	));
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	let mut frames = Vec::new();
	let horizontal = HashMap::new();
	let view = view(Theme::Light, 1., &horizontal);
	let mut tall = Default::default();
	for widths in [[0, 0, 0], [8, 12, 16]] {
		let mut sheet = (*Stylesheet::bundled(false)).clone();
		sheet.merge(&Stylesheet::parse(&format!(concat!(
			"format_version=2\nversion=1\n",
			"[[rule]]\nwhen=['body']\ncolor='#000000'\nbackground='#FFFFFF'\n",
			"[[rule]]\nwhen=['table','cell']\npadding=0\nborder_width={}\nborder_color='#FF0000'\n",
			"[[rule]]\nwhen=['table','first_child','cell']\nborder_width={}\n",
			"[[rule]]\nwhen=['table','last_child','cell']\nborder_width={}\n",
		), widths[0], widths[1], widths[2]))?);
		let sheet = Arc::new(sheet);
		renderer.set_stylesheet(sheet.clone());
		let snapshot = LayoutEngine::new()
			.layout(
				&doc,
				&LayoutOptions {
					width: 360.,
					font_size: 36.,
					fonts: fonts(),
					stylesheet: sheet,
					..Default::default()
				},
			)
			.flattened();
		tall = snapshot.blocks[0]
			.layout
			.draws
			.iter()
			.find_map(|draw| match draw {
				markview_layout::scene::Draw::Box {
					rect,
					condition: markview_layout::style::Condition::Cell,
					..
				} => Some(rect),
				_ => None,
			})
			.unwrap();
		let target = renderer.offscreen(view.width, view.height);
		let submission = renderer.render(
			&snapshot,
			&view,
			&[],
			&target.create_view(&Default::default()),
		)?;
		renderer.wait(Some(submission))?;
		frames.push(renderer.read_pixels(&target)?.rgba);
	}
	let mut checked = 0;
	for y in (tall.y + view.top).ceil() as u32
		..(tall.y + tall.h + view.top).floor() as u32
	{
		for x in (tall.x + view.left).ceil() as u32
			..(tall.x + view.left + 12.).floor() as u32
		{
			let at = (y * view.width + x) as usize * 4;
			if frames[0][at..at + 3].iter().all(|&c| c < 20) {
				assert!(
					frames[1][at] <= frames[0][at].saturating_add(2),
					"a border covered text at ({x}, {y})"
				);
				checked += 1;
			}
		}
	}
	assert!(
		checked > 10,
		"the glyph must overlap the border strip: {checked}, {tall:?}"
	);
	Ok(())
}

/// A prewarm pass prepares the screenful below, which the reader is not
/// looking at. It must still fill the atlases, and it must leave both the
/// frame and the paint order of the next one exactly as they were.
#[test]
fn prewarm_prepares_atlases_without_a_frame() -> anyhow::Result<()> {
	let source = (0..40)
		.map(|i| {
			format!(
				"# Heading {i}\n\nParagraph number {i} with $x^{{{i}}}$.\n\n"
			)
		})
		.collect::<String>();
	let doc = markview_commonmark::parse(source.as_str());
	let mut engine =
		LayoutEngine::with_executor(Arc::new(Direct), Arc::new(|| {}));
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	let sheet = Stylesheet::bundled(false);
	renderer.set_stylesheet(sheet.clone());
	let options = LayoutOptions {
		width: 360.0,
		fonts: fonts(),
		stylesheet: sheet,
		..Default::default()
	};
	let snapshot = engine.layout(&doc, &options);
	let horizontal = HashMap::new();
	let view = view(Theme::Light, 1.0, &horizontal);
	let target = renderer.offscreen(view.width, view.height);
	let submission = renderer.render(
		&snapshot,
		&view,
		&[],
		&target.create_view(&Default::default()),
	)?;
	renderer.wait(Some(submission))?;
	let frame = renderer.read_pixels(&target)?.rgba;
	assert!(renderer.vertices() > 0, "the frame emitted no vertices");
	let rasterized = renderer.raster_stats().rasterized;
	renderer.prewarm(&snapshot, &view, Duration::from_millis(20));
	assert!(
		renderer.raster_stats().rasterized > rasterized,
		"the prewarm pass rasterized nothing"
	);
	assert_eq!(
		renderer.vertices(),
		0,
		"the prewarm pass emitted vertices for a frame it discards"
	);
	let submission = renderer.render(
		&snapshot,
		&view,
		&[],
		&target.create_view(&Default::default()),
	)?;
	renderer.wait(Some(submission))?;
	assert_eq!(frame, renderer.read_pixels(&target)?.rgba);
	Ok(())
}

/// A hover preview is composed from the scene itself, scaled into its card by
/// the pass viewport, and it must leave the frame below it untouched.
#[test]
fn preview_scene_composes_above_the_frame_only_in_its_rect()
-> anyhow::Result<()> {
	let doc = markview_commonmark::parse(
		"# Preview\n\nA paragraph of text the hover card shows small, with enough words to fill the card.\n\n- first\n- second\n",
	);
	let mut engine =
		LayoutEngine::with_executor(Arc::new(Direct), Arc::new(|| {}));
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	let sheet = Stylesheet::bundled(false);
	renderer.set_stylesheet(sheet.clone());
	let options = LayoutOptions {
		width: 360.0,
		fonts: fonts(),
		stylesheet: sheet,
		..Default::default()
	};
	let snapshot = engine.layout(&doc, &options);
	let horizontal = HashMap::new();
	let empty = markview_layout::scene::LayoutSnapshot::default();
	let frame = view(Theme::Light, 1.0, &horizontal);
	let target = renderer.offscreen(frame.width, frame.height);
	let view = target.create_view(&Default::default());
	let rect = markview_layout::scene::Rect {
		x: 40.0,
		y: 300.0,
		w: 200.0,
		h: 125.0,
	};
	let preview = markview_render::Preview {
		snapshot: &snapshot,
		scroll: 0.0,
		rect,
		theme: Theme::Light,
		horizontal: &horizontal,
	};
	renderer.render(&empty, &frame, &[], &view)?;
	renderer.wait(None)?;
	let plain = renderer.read_pixels(&target)?.rgba;
	renderer.render_with_preview(&empty, &frame, &[], Some(&preview), &view)?;
	renderer.wait(None)?;
	let composed = renderer.read_pixels(&target)?.rgba;
	let width = frame.width as usize;
	let background = &plain[..4];
	let inside = |i: usize| {
		let x = (i % width) as f32;
		let y = (i / width) as f32;
		rect.contains(x, y)
	};
	let changed = composed
		.as_chunks::<4>()
		.0
		.iter()
		.enumerate()
		.filter(|(i, pixel)| inside(*i) && pixel.as_slice() != background)
		.count();
	assert!(changed > 500, "the preview scene drew nothing: {changed}");
	for (i, (before, after)) in plain
		.as_chunks::<4>()
		.0
		.iter()
		.zip(composed.as_chunks::<4>().0)
		.enumerate()
	{
		if !inside(i) {
			assert_eq!(
				before, after,
				"the preview touched the frame at pixel {i}"
			);
		}
	}
	// Closing the card releases the layer; the next frame is the plain one.
	renderer.render(&empty, &frame, &[], &view)?;
	renderer.wait(None)?;
	assert_eq!(plain, renderer.read_pixels(&target)?.rgba);
	Ok(())
}
