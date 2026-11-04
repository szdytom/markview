//! Regression test for the rejected-preview key: the renderer remembers a
//! preview too large for the glyph atlas so it is not retried every frame, but
//! a different preview scene with the same shape must be tried again.
//!
//! The review's repro replaces the oversized fixture's distinct CJK
//! characters with one repeated character. The block count and total height
//! stay identical, so every old key field collides; only the document content
//! distinguishes the two scenes.

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
			include_bytes!(
				"../../markview-layout/tests/fonts/NotoSerifCJKsc-Regular-subset.otf"
			)
			.as_slice(),
			include_bytes!(
				"../../markview-layout/tests/fonts/NotoSansCJKsc-Regular-subset.otf"
			)
			.as_slice(),
			include_bytes!(
				"../../markview-layout/tests/fonts/NotoSansMonoCJKsc-Regular-subset.otf"
			)
			.as_slice(),
		]
		.into_iter()
		.map(|data| parley::fontique::Blob::new(Arc::new(data)))
		.collect(),
	)
}

fn layout(
	engine: &mut LayoutEngine,
	source: &str,
	width: f32,
	font_size: f32,
	stylesheet: Arc<Stylesheet>,
) -> LayoutSnapshot {
	let document = markview_commonmark::parse(source);
	let options = LayoutOptions {
		width,
		font_size,
		fonts: fonts(),
		stylesheet,
		..Default::default()
	};
	let snapshot = engine.layout(&document, &options);
	if engine.wait_highlights() {
		engine.layout(&document, &options)
	} else {
		snapshot
	}
}

/// Distinct CJK glyphs at every heading size, enough to exhaust one mask
/// atlas even in a card-sized slice.
const CJK: &str = concat!(
	"一丁七三上下不与且世丙両两並个中义之乙九也书了事二于五亚些交享人介仍付代以件任份",
	"优会但位体余作你使例供依侧便保信個倍候値值停側備傳像優儲允元充先免入內全八公六共",
	"关其典内再写准减凑出分切划列则初删利到制刷前剪副力加务动効動匀包化匯區十华单単占",
	"即压原参及发取变叠口句可台右号合同名后向吗含启命和品唯問啟單嗎器四回囲国图圖在地",
	"均块坡型執基報場塊填增墨壓处备変复外多够大失头夹夾好如始子字存学它守完官定实実家",
	"容宽實寬对导将將專尋對小少尚就尾局展嵌工左已布常幅平并広序应底度建开式引张張强当"
);

/// The review's oversized preview shape: one heading per stylesheet size plus
/// a body block.
fn preview_source(text: &str) -> String {
	let mut source = String::new();
	for marker in ["#", "##", "###", "####", "######"] {
		source.push_str(marker);
		source.push(' ');
		source.push_str(text);
		source.push_str("\n\n");
	}
	source.push_str(text);
	source.push_str("\n\n**");
	source.push_str(text);
	source.push_str("**\n\n*");
	source.push_str(text);
	source.push_str("*\n\n`");
	source.push_str(text);
	source.push_str("`\n");
	source
}

fn view<'a>(
	theme: Theme,
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
		theme,
		horizontal,
		selection: None,
		revision: 1,
		hovered_link: None,
		held_overflow: None,
	}
}

/// Bytes that differ, counted so a failed comparison does not print pixels.
fn differing(left: &[u8], right: &[u8]) -> usize {
	left.iter().zip(right).filter(|(a, b)| a != b).count()
		+ left.len().abs_diff(right.len())
}

/// Pixels inside a logical rectangle of a readback.
fn card(readback: &Readback, rect: Rect) -> Vec<u8> {
	let width = readback.width as usize;
	let mut out = Vec::new();
	for y in rect.y as usize..(rect.y + rect.h) as usize {
		for x in rect.x as usize..(rect.x + rect.w) as usize {
			let i = (y * width + x) * 4;
			out.extend_from_slice(&readback.rgba[i..i + 4]);
		}
	}
	out
}

#[test]
fn a_different_preview_with_the_same_shape_is_tried_again() -> anyhow::Result<()>
{
	let stylesheet = Stylesheet::bundled(false);
	let mut engine =
		LayoutEngine::with_executor(Arc::new(Direct), Arc::new(|| {}));
	let main = layout(
		&mut engine,
		"# Preview identity\n\nA short page that fits the glyph atlases.\n",
		360.0,
		18.0,
		stylesheet.clone(),
	);
	let oversized = layout(
		&mut engine,
		&preview_source(CJK),
		760.0,
		40.0,
		stylesheet.clone(),
	);
	let repeated = "一".repeat(CJK.chars().count());
	let fitted = layout(
		&mut engine,
		&preview_source(&repeated),
		760.0,
		40.0,
		stylesheet.clone(),
	);

	// The old key fields collide: same presentation, width, block count and
	// height. Only the content identity differs.
	assert!(
		oversized.content_id.get() != 0,
		"engine layouts carry a content id"
	);
	assert!(
		fitted.content_id.get() != 0,
		"engine layouts carry a content id"
	);
	assert_ne!(
		oversized.content_id, fitted.content_id,
		"replacing the preview text must change the content id"
	);
	assert_eq!(
		oversized.presentation_key, fitted.presentation_key,
		"the fixture was meant to keep the presentation identity"
	);
	assert_eq!(oversized.blocks.len(), fitted.blocks.len());
	assert_eq!(oversized.width, fitted.width);
	assert_eq!(
		oversized.height, fitted.height,
		"the replacement content must keep the snapshot height; got {} vs {}",
		oversized.height, fitted.height
	);

	let mut renderer = pollster::block_on(Renderer::new(None))?;
	renderer.set_stylesheet(stylesheet);
	let horizontal = HashMap::new();
	let frame = view(Theme::Light, 2.0, &horizontal);
	let texture = renderer.offscreen(frame.width, frame.height);
	let target = texture.create_view(&Default::default());

	let submission = renderer.render(&main, &frame, &[], &target)?;
	renderer.wait(Some(submission))?;
	let plain = renderer.read_pixels(&texture)?.rgba;

	let rect = Rect {
		x: 40.0,
		y: 40.0,
		w: 60.0,
		h: 1000.0,
	};
	let oversized_preview = Preview {
		snapshot: &oversized,
		scroll: 0.0,
		rect,
		theme: Theme::Light,
		horizontal: &horizontal,
	};
	renderer.render_with_preview(
		&main,
		&frame,
		&[],
		Some(&oversized_preview),
		&target,
	)?;
	renderer.wait(None)?;
	let rejected = renderer.raster_stats();
	// The fixture really must be too large: the failed attempt leaves entries
	// it could not place in the atlas.
	assert!(
		rejected.rasterized > rejected.entries as u64 + 100,
		"the oversized fixture did not exhaust the glyph atlas: {rejected:?}"
	);

	// The unchanged rejected scene is still remembered: no growth on repeats.
	for _ in 0..2 {
		renderer.render_with_preview(
			&main,
			&frame,
			&[],
			Some(&oversized_preview),
			&target,
		)?;
		renderer.wait(None)?;
	}
	let repeated_rejected = renderer.raster_stats();
	assert_eq!(
		repeated_rejected.rasterized, rejected.rasterized,
		"an unchanged rejected preview was rasterized again: {rejected:?} -> {repeated_rejected:?}"
	);
	assert_eq!(
		repeated_rejected.resets, rejected.resets,
		"an unchanged rejected preview evicted the atlas again: {rejected:?} -> {repeated_rejected:?}"
	);

	// Same block count, same height, different text: the card must compose.
	let fitted_preview = Preview {
		snapshot: &fitted,
		scroll: 0.0,
		rect,
		theme: Theme::Light,
		horizontal: &horizontal,
	};
	renderer.render_with_preview(
		&main,
		&frame,
		&[],
		Some(&fitted_preview),
		&target,
	)?;
	renderer.wait(None)?;
	let composed = renderer.read_pixels(&texture)?;
	let changed = differing(
		&card(
			&Readback {
				width: frame.width,
				height: frame.height,
				rgba: plain,
			},
			rect,
		),
		&card(&composed, rect),
	);
	assert!(
		changed > 100,
		"the replacement preview was skipped, leaving the card blank ({changed} pixels differ)"
	);
	Ok(())
}
