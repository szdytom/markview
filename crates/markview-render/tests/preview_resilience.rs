//! Regression tests for the hover preview's interaction with the glyph
//! atlas, the tab's saved reading position and the submission index.
//!
//! The preview is a second scene composed into a card over the frame. It must
//! not let an oversized card evict the frame on screen, must not clamp a
//! reading position the reader itself allows, and its GPU work must be
//! covered by the submission index `render_with_preview` returns.

use markview_layout::{
	background::Direct,
	fonts::FontConfig,
	layout::{LayoutEngine, LayoutOptions, scroll_limit},
	scene::{LayoutSnapshot, Rect},
	style::Stylesheet,
};
use markview_render::{Preview, Readback, Renderer, Theme, View};
use std::{collections::HashMap, fmt::Debug, sync::Arc};

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

fn engine() -> LayoutEngine {
	LayoutEngine::with_executor(Arc::new(Direct), Arc::new(|| {}))
}

/// The renderer does not expose the order of a [`wgpu::SubmissionIndex`], but
/// its `Debug` form names the queue's submission number, which is all this
/// test needs to tell which pass a return value covers.
fn submission_number(index: &impl Debug) -> u64 {
	let text = format!("{index:?}");
	let tail = text
		.rsplit_once("index: ")
		.unwrap_or_else(|| panic!("unexpected submission index format: {text}"))
		.1;
	tail.chars()
		.filter(char::is_ascii_digit)
		.collect::<String>()
		.parse()
		.unwrap_or_else(|_| {
			panic!("unexpected submission index format: {text}")
		})
}

/// Bytes that differ, counted so a failed comparison does not print pixels.
fn differing(left: &[u8], right: &[u8]) -> usize {
	left.iter().zip(right).filter(|(a, b)| a != b).count()
		+ left.len().abs_diff(right.len())
}

/// The pixels inside a logical rectangle, at the scale the frames use.
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

/// Distinct CJK glyphs, so a preview made only of headings still rasterizes
/// enough different atlas entries to exhaust one mask atlas.
const CJK: &str = concat!(
	"一丁七三上下不与且世丙両两並个中义之乙九也书了事二于五亚些交享人介仍付代以件任份",
	"优会但位体余作你使例供依侧便保信個倍候値值停側備傳像優儲允元充先免入內全八公六共",
	"关其典内再写准减凑出分切划列则初删利到制刷前剪副力加务动効動匀包化匯區十华单単占",
	"即压原参及发取变叠口句可台右号合同名后向吗含启命和品唯問啟單嗎器四回囲国图圖在地",
	"均块坡型執基報場塊填增墨壓处备変复外多够大失头夹夾好如始子字存学它守完官定实実家",
	"容宽實寬对导将將專尋對小少尚就尾局展嵌工左已布常幅平并広序应底度建开式引张張强当"
);

/// One heading at every size the stylesheet uses, plus body text. The same
/// glyphs at different heading sizes are different atlas entries, and
/// together they overflow the mask atlas even in a card-sized slice.
fn oversized_preview_source() -> String {
	let mut source = String::new();
	for marker in ["#", "##", "###", "####", "######"] {
		source.push_str(marker);
		source.push(' ');
		source.push_str(CJK);
		source.push_str("\n\n");
	}
	source.push_str(CJK);
	source.push_str("\n\n**");
	source.push_str(CJK);
	source.push_str("**\n\n*");
	source.push_str(CJK);
	source.push_str("*\n\n`");
	source.push_str(CJK);
	source.push_str("`\n");
	source
}

/// An oversized preview cannot be drawn, but an unchanged one must not be
/// retried every frame: each retry evicts the frame on screen and rasterizes
/// the whole card again, which is the review's CPU-burning repro.
#[test]
fn oversized_preview_is_rejected_once_and_leaves_the_frame_alone()
-> anyhow::Result<()> {
	let stylesheet = Stylesheet::bundled(false);
	let mut engine = engine();
	let main = layout(
		&mut engine,
		"# Preview resilience\n\nA short page that fits the glyph atlases.\n",
		360.0,
		18.0,
		stylesheet.clone(),
	);
	let oversized = layout(
		&mut engine,
		&oversized_preview_source(),
		760.0,
		40.0,
		stylesheet.clone(),
	);
	assert!(
		oversized.blocks.len() >= 5,
		"the fixture laid out only {} blocks",
		oversized.blocks.len()
	);
	eprintln!(
		"main {:.0}x{:.0}, preview {:.0}x{:.0} over {} blocks",
		main.width,
		main.height,
		oversized.width,
		oversized.height,
		oversized.blocks.len()
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
	let before = renderer.raster_stats();

	// The card aspect fixes the slice the scene shows. A tall card keeps every
	// heading block inside that slice.
	let preview = Preview {
		interaction: None,
		snapshot: &oversized,
		scroll: 0.0,
		rect: Rect {
			x: 40.0,
			y: 40.0,
			w: 60.0,
			h: 1000.0,
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
	let first = renderer.raster_stats();
	eprintln!("after first preview: {first:?} (before {before:?})");
	assert!(
		first.rasterized > before.rasterized,
		"the preview rasterized nothing: {before:?} -> {first:?}"
	);
	// Entries the preview could not place in the atlas. A card that fits
	// leaves none.
	assert!(
		first.rasterized - first.entries as u64 > 100,
		"the preview did not exhaust the glyph atlas: {first:?}"
	);

	// The same scene, unchanged: the renderer has nothing new to do.
	renderer.render_with_preview(
		&main,
		&frame,
		&[],
		Some(&preview),
		&target,
	)?;
	renderer.wait(None)?;
	let second = renderer.raster_stats();
	// One cleanup rebuild of the frame would be acceptable; more than one is
	// the repeated eviction the review reports.
	assert!(
		second.resets <= first.resets + 1,
		"rejecting the preview reset the atlas {} times: {first:?} -> {second:?}",
		second.resets - first.resets
	);
	for _ in 0..2 {
		renderer.render_with_preview(
			&main,
			&frame,
			&[],
			Some(&preview),
			&target,
		)?;
		renderer.wait(None)?;
	}
	let repeated = renderer.raster_stats();
	assert_eq!(
		repeated.rasterized, second.rasterized,
		"an unchanged oversized preview was rasterized again: {second:?} -> {repeated:?}"
	);
	assert_eq!(
		repeated.resets, second.resets,
		"an unchanged oversized preview evicted the atlas again: {second:?} -> {repeated:?}"
	);

	// The target must still show exactly the plain frame.
	let submission = renderer.render(&main, &frame, &[], &target)?;
	renderer.wait(Some(submission))?;
	assert_eq!(
		plain,
		renderer.read_pixels(&texture)?.rgba,
		"the rejected preview corrupted the main frame"
	);
	Ok(())
}

/// The reader may scroll a document until its last line sits a third of a
/// viewport below the top, so a saved offset can be past `height - slice` for
/// a card. The preview must show that offset, not jump back to earlier text.
#[test]
fn preview_keeps_a_valid_trailing_space_scroll() -> anyhow::Result<()> {
	let stylesheet = Stylesheet::bundled(false);
	let mut engine = engine();
	let source = (0..40)
		.map(|i| format!("Paragraph {i} with enough words to fill a line.\n\n"))
		.collect::<String>();
	let snapshot =
		layout(&mut engine, &source, 360.0, 18.0, stylesheet.clone());
	let horizontal = HashMap::new();
	let frame = view(Theme::Light, 1.0, &horizontal);
	// The chrome's body rectangle: 268 logical px wide with a 0.625 ratio.
	let rect = Rect {
		x: 40.0,
		y: 60.0,
		w: 268.0,
		h: 167.5,
	};
	let page = snapshot.width.max(1.0);
	let slice = (rect.h * page / rect.w).max(1.0);
	let clamped = (snapshot.height - slice).max(0.0);
	// The reader's own limit: `height - viewport / 3`.
	let valid = scroll_limit(snapshot.height, 500.0);
	assert!(
		valid > clamped && valid < snapshot.height,
		"fixture does not reach past the old clamp: height {} slice {slice} clamped {clamped} valid {valid}",
		snapshot.height
	);
	eprintln!(
		"preview height {} slice {slice}; clamped {clamped}, valid {valid}",
		snapshot.height
	);

	let mut renderer = pollster::block_on(Renderer::new(None))?;
	renderer.set_stylesheet(stylesheet);
	let texture = renderer.offscreen(frame.width, frame.height);
	let target = texture.create_view(&Default::default());
	let empty = LayoutSnapshot::default();
	let mut compose = |scroll: f32| -> anyhow::Result<Vec<u8>> {
		let preview = Preview {
			interaction: None,
			snapshot: &snapshot,
			scroll,
			rect,
			theme: Theme::Light,
			horizontal: &horizontal,
		};
		renderer.render_with_preview(
			&empty,
			&frame,
			&[],
			Some(&preview),
			&target,
		)?;
		renderer.wait(None)?;
		Ok(card(&renderer.read_pixels(&texture)?, rect))
	};
	let at_clamped = compose(clamped)?;
	let at_valid = compose(valid)?;
	let changed = differing(&at_clamped, &at_valid);
	assert!(
		changed > 0,
		"the preview drew the same card at the valid trailing-space offset ({valid}) as at the old clamped offset ({clamped})"
	);
	Ok(())
}

/// `wait(Some(index))` must cover the composed frame, so the returned index is
/// the preview pass when one is submitted and the main one when it is skipped.
#[test]
fn render_with_preview_returns_the_last_submission() -> anyhow::Result<()> {
	let stylesheet = Stylesheet::bundled(false);
	let mut engine = engine();
	let snapshot = layout(
		&mut engine,
		"# Composed\n\nA short preview body for the hover card.\n",
		360.0,
		18.0,
		stylesheet.clone(),
	);
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	renderer.set_stylesheet(stylesheet);
	let horizontal = HashMap::new();
	let frame = view(Theme::Light, 1.0, &horizontal);
	let plain_texture = renderer.offscreen(frame.width, frame.height);
	let plain_target = plain_texture.create_view(&Default::default());
	let composed_texture = renderer.offscreen(frame.width, frame.height);
	let composed_target = composed_texture.create_view(&Default::default());

	let submission = renderer.render(&snapshot, &frame, &[], &plain_target)?;
	renderer.wait(Some(submission))?;
	let plain = renderer.read_pixels(&plain_texture)?.rgba;

	let rect = Rect {
		x: 60.0,
		y: 60.0,
		w: 200.0,
		h: 125.0,
	};
	let preview = Preview {
		interaction: None,
		snapshot: &snapshot,
		scroll: 0.0,
		rect,
		theme: Theme::Light,
		horizontal: &horizontal,
	};
	let composed = renderer.render_with_preview(
		&snapshot,
		&frame,
		&[],
		Some(&preview),
		&composed_target,
	)?;
	renderer.wait(Some(composed.clone()))?;
	// The next submit is one past the composed frame when the index covers the
	// preview pass, and two past when it names only the main frame.
	let after_composed =
		renderer.render(&snapshot, &frame, &[], &plain_target)?;
	assert_eq!(
		submission_number(&after_composed),
		submission_number(&composed) + 1,
		"render_with_preview returned an index that misses the preview pass"
	);
	renderer.wait(Some(after_composed))?;
	let composed_pixels = renderer.read_pixels(&composed_texture)?;
	let plain_card = card(
		&Readback {
			width: frame.width,
			height: frame.height,
			rgba: plain,
		},
		rect,
	);
	let composed_card = card(&composed_pixels, rect);
	assert!(
		differing(&plain_card, &composed_card) > 0,
		"the preview pass drew nothing; the index contract was not exercised"
	);

	// A skipped preview submits only the main frame.
	let skipped = renderer.render_with_preview(
		&snapshot,
		&frame,
		&[],
		None,
		&plain_target,
	)?;
	renderer.wait(Some(skipped.clone()))?;
	let after_skipped =
		renderer.render(&snapshot, &frame, &[], &plain_target)?;
	assert_eq!(
		submission_number(&after_skipped),
		submission_number(&skipped) + 1,
		"a skipped preview must still return the main submission index"
	);
	Ok(())
}
