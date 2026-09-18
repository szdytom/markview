use markview_core::{
	document::{self, BlockKind, InlineKind},
	image::{ImageInfo, ImageSnapshot},
	layout::{Draw, LayoutEngine, LayoutOptions, LayoutSnapshot},
};

fn resources() -> ImageSnapshot {
	let mut images = ImageSnapshot::default();
	images.entries.insert(
		"test.png".into(),
		ImageInfo {
			version: 1,
			size: Some((100, 140)),
			error: None,
		},
	);
	images
}

/// The committed subset faces, so geometry never depends on the host's fonts.
fn fonts() -> markview_core::fonts::FontConfig {
	markview_core::fonts::FontConfig {
		ignore_system_fonts: true,
		directories: vec![
			std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
				.join("tests/fonts"),
		],
	}
}

fn styled(rules: &str, width: f32) -> LayoutOptions {
	let mut sheet = (*markview_core::style::Stylesheet::bundled(false)).clone();
	sheet.merge(
		&markview_core::style::Stylesheet::parse(&format!(
			"format_version=2\nversion=1\n{rules}"
		))
		.unwrap(),
	);
	LayoutOptions {
		width,
		stylesheet: std::sync::Arc::new(sheet),
		fonts: fonts(),
		..Default::default()
	}
}

#[test]
fn caption_wraps_and_toggling_it_keeps_reading_positions() {
	let doc = document::parse(
		"![alt](test.png \"A long explanatory caption that should occupy several lines beneath the image\")\n\nFollowing paragraph.",
	);
	let mut engine = LayoutEngine::new();
	let hidden = engine.layout_with_images(
		&doc,
		&styled("[[rule]]\nwhen=['img','caption']\nsource='none'", 180.),
		&resources(),
	);
	let selection = hidden.select_all(5).unwrap();
	let options = styled(
		"[[rule]]\nwhen=['img','caption']\nsource='title'\nalign='left'\ncolor='#123456'\nspace_before=1.0\nsize=0.9",
		180.,
	);
	let visible = engine.layout_with_images(&doc, &options, &resources());
	assert!(visible.height > hidden.height + 40.);
	assert_eq!(
		hidden.extract_text(selection, 5),
		"alt\n\nFollowing paragraph."
	);
	assert!(
		visible
			.extract_text(visible.select_all(5).unwrap(), 5)
			.contains("A long explanatory caption")
	);
	assert_eq!(
		hidden.blocks[0].layout.text.len(),
		visible.blocks[0].layout.text.len()
	);
	let image = rects(&visible)[0];
	let glyphs: Vec<_> = visible.blocks[0]
		.layout
		.draws
		.iter()
		.filter_map(|d| {
			if let Draw::Glyph(g) = d {
				Some(g)
			} else {
				None
			}
		})
		.collect();
	assert!(!glyphs.is_empty());
	assert!(glyphs.iter().all(|g| g.y > image.y + image.h
		&& options.stylesheet.paint(g.paint)
			== markview_core::style::Color(0x123456ff).rgba()));
	assert!(glyphs.iter().any(|g| g.y > glyphs[0].y + 20.));
}

#[test]
fn caption_sources_and_image_fields_are_strict_and_cascade() {
	use markview_core::style::{CaptionSource, Stylesheet};
	let mut spec = markview_core::image::ImageSpec {
		src: "test.png".into(),
		alt: "alt".into(),
		title: "title".into(),
		width: None,
		height: None,
	};
	assert_eq!(CaptionSource::TitleOrAlt.text(&spec), Some("title"));
	spec.title = "  ".into();
	assert_eq!(CaptionSource::TitleOrAlt.text(&spec), Some("alt"));
	assert_eq!(CaptionSource::Title.text(&spec), None);
	assert_eq!(CaptionSource::None.text(&spec), None);
	for rule in [
		"[[rule]]\nwhen=['img','caption']\nsource='auto'",
		"[[rule]]\nwhen=['p']\nsource='title'",
		"[[rule]]\nwhen=['img']\nradius=8",
		"[[rule]]\nwhen=['img','caption']\nsize=0",
		"[[rule]]\nwhen=['img']\npadding=-1",
		"[[rule]]\nwhen=['img','caption']\nalign='justify'",
	] {
		assert!(
			Stylesheet::parse(&format!("format_version=2\nversion=1\n{rule}"))
				.is_err(),
			"{rule}"
		);
	}
	let options =
		styled("[[rule]]\nwhen=['img','caption']\nsource='none'", 200.);
	let caption = markview_core::style::ConditionSet::of(
		markview_core::style::Condition::Image,
	)
	.with(markview_core::style::Condition::Caption);
	assert_eq!(
		options.stylesheet.rules.get(&caption).unwrap().source,
		Some(CaptionSource::None)
	);
}

#[test]
fn image_padding_and_border_reserve_space_and_caption_colors_reuse_layout() {
	let doc = document::parse("![alt](test.png)");
	let mut engine = LayoutEngine::new();
	let options = styled(
		"[[rule]]\nwhen=['img']\npadding=0.5\nborder_width=2\nalign='left'\n[[rule]]\nwhen=['img','caption']\nsource='alt'",
		120.,
	);
	let first = engine.layout_with_images(&doc, &options, &resources());
	let image = rects(&first)[0];
	assert_eq!(image.x, 11.);
	assert!(image.x + image.w <= 109.01);
	let changed = styled(
		"[[rule]]\nwhen=['img']\npadding=0.5\nborder_width=2\nalign='left'\nborder_color='#ff0000'\n[[rule]]\nwhen=['img','caption']\nsource='alt'\ncolor='#00ff00'",
		120.,
	);
	let second = engine.layout_with_images(&doc, &changed, &resources());
	assert_eq!(second.reused, 1);
	assert_eq!(first.height, second.height);
	assert!(second.blocks[0].layout.draws.iter().any(|d| matches!(d,Draw::Glyph(g) if changed.stylesheet.paint(g.paint)==markview_core::style::Color(0x00ff00ff).rgba())));
}

#[test]
fn mixed_inline_images_do_not_gain_captions() {
	let doc = document::parse("before ![alt](test.png \"title\") after");
	let mut engine = LayoutEngine::new();
	let hidden = engine.layout_with_images(
		&doc,
		&styled("[[rule]]\nwhen=['img','caption']\nsource='none'", 400.),
		&resources(),
	);
	let shown = engine.layout_with_images(
		&doc,
		&styled("[[rule]]\nwhen=['img','caption']\nsource='title'", 400.),
		&resources(),
	);
	assert_eq!(hidden.height, shown.height);
	assert_eq!(
		hidden.blocks[0].layout.draws.len(),
		shown.blocks[0].layout.draws.len()
	);
}

fn drag_node(
	snapshot: &LayoutSnapshot,
	node: usize,
) -> markview_core::text::TextSelection {
	let block = &snapshot.blocks[0];
	let clusters = &block.layout.text[node].clusters;
	let first = clusters.iter().min_by_key(|c| c.range.start).unwrap();
	let last = clusters.iter().max_by_key(|c| c.range.end).unwrap();
	let hit = |c: &markview_core::text::TextCluster, right: bool| {
		let x = c.rect.x + c.rect.w * if right { 0.9 } else { 0.1 };
		let y = block.y + c.rect.y + c.rect.h * 0.5;
		assert!(
			snapshot.contains_text(x, y, &Default::default()),
			"visible text must produce an I-beam"
		);
		snapshot
			.hit_test_text(x, y, &Default::default(), 7)
			.unwrap()
	};
	let selection = markview_core::text::TextSelection {
		anchor: hit(first, false),
		focus: hit(last, true),
	};
	assert_eq!(selection.anchor.node, node);
	assert_eq!(selection.focus.node, node);
	assert!(
		!snapshot
			.selection_rects(selection, &Default::default(), 7)
			.is_empty()
	);
	selection
}

#[test]
fn caption_can_be_drag_selected_copied_and_reflowed() {
	let doc = document::parse(
		"![alt](test.png \"Caption text spans several lines and remains selectable\")",
	);
	let mut engine = LayoutEngine::new();
	let first = engine.layout_with_images(
		&doc,
		&styled("[[rule]]\nwhen=['img','caption']\nsource='title'", 180.),
		&resources(),
	);
	let selected = drag_node(&first, 1);
	let caption = "Caption text spans several lines and remains selectable";
	assert_eq!(first.extract_text(selected, 7), caption);
	let next = engine.layout_with_images(
		&doc,
		&styled(
			"[[rule]]\nwhen=['img','caption']\nsource='title'\nsize=1.1",
			300.,
		),
		&resources(),
	);
	let selected = first.rebase_selection(&next, selected, 7, 8).unwrap();
	assert_eq!(next.extract_text(selected, 8), caption);
	let hidden = engine.layout_with_images(
		&doc,
		&styled("[[rule]]\nwhen=['img','caption']\nsource='none'", 300.),
		&resources(),
	);
	assert!(next.rebase_selection(&hidden, selected, 8, 9).is_none());
}

#[test]
fn placeholder_text_and_elided_errors_have_character_hit_geometry() {
	for width in [65., 400.] {
		let doc = document::parse(format!(
			"<img src='test.png' width='{width}' alt=''>"
		));
		let mut images = resources();
		let reason =
			"Cannot open image: /a/very/long/中文/directory/missing.png";
		images.entries.get_mut("test.png").unwrap().error = Some(reason.into());
		let snapshot = LayoutEngine::new().layout_with_images(
			&doc,
			&styled("[[rule]]\nwhen=['img','caption']\nsource='none'", 500.),
			&images,
		);
		let selected = drag_node(&snapshot, 0);
		assert_eq!(snapshot.extract_text(selected, 7), reason);
		assert!(snapshot.blocks[0].layout.text[0].clusters.iter().all(
			|c| !matches!(
				snapshot.blocks[0].layout.draws[c.command],
				Draw::Image { .. }
			)
		));
	}
}

#[test]
fn placeholder_wraps_and_justifies_its_reason_into_the_box() {
	const REASON: &str = "Image host resolves to a local or private address";
	for (width, wrapped) in [(160., true), (60., false)] {
		let doc = document::parse(format!(
			"<img src='test.png' width='{width}' alt=''>"
		));
		let mut images = resources();
		let failed = images.entries.get_mut("test.png").unwrap();
		failed.error = Some(REASON.into());
		// A source that never decoded has no intrinsic size, like a real failure.
		failed.size = None;
		let mut options =
			styled("[[rule]]\nwhen=['img','caption']\nsource='none'", 500.);
		let snapshot =
			LayoutEngine::new().layout_with_images(&doc, &options, &images);
		let image = rects(&snapshot)[0];
		let clusters = &snapshot.blocks[0].layout.text[0].clusters;
		let baselines: std::collections::HashSet<i32> = clusters
			.iter()
			.map(|c| (c.rect.y + c.rect.h).round() as i32)
			.collect();
		assert_eq!(baselines.len() > 1, wrapped, "width {width}");
		assert!(
			clusters
				.iter()
				.all(|c| c.rect.y + c.rect.h <= image.y + image.h + 0.5
					&& c.rect.x >= image.x
					&& c.rect.x + c.rect.w <= image.x + image.w + 0.5),
			"placeholder text left its box at width {width}"
		);
		assert_eq!(
			snapshot.extract_text(snapshot.select_all(7).unwrap(), 7),
			REASON
		);
		if wrapped {
			// The placeholder goes through the paragraph engine, so it is
			// justified like body text: every line but the last reaches the
			// measure, and turning justification off leaves a ragged edge.
			let reach = |snapshot: &LayoutSnapshot| {
				let clusters = &snapshot.blocks[0].layout.text[0].clusters;
				let top =
					clusters.iter().map(|c| c.rect.y).fold(f32::MAX, f32::min);
				clusters
					.iter()
					.filter(|c| c.rect.y < top + 1.)
					.map(|c| c.rect.x + c.rect.w)
					.fold(0., f32::max)
			};
			options.justify = false;
			let ragged =
				LayoutEngine::new().layout_with_images(&doc, &options, &images);
			assert!(
				(reach(&snapshot) - (image.x + image.w - 6.)).abs() < 1.,
				"justified line left the measure at width {width}"
			);
			assert!(reach(&snapshot) > reach(&ragged) + 2.);
		}
	}
}

#[test]
fn placeholder_hyphenates_like_body_text() {
	// `hyphenation` cannot fit the 48px measure whole, so only the shared
	// paragraph engine's hyphenation can keep it inside the box.
	let doc =
		document::parse("<img src='test.png' width='60' height='96' alt=''>");
	let mut images = resources();
	let failed = images.entries.get_mut("test.png").unwrap();
	failed.error = Some("hyphenation".into());
	failed.size = None;
	let options =
		styled("[[rule]]\nwhen=['img','caption']\nsource='none'", 500.);
	let snapshot =
		LayoutEngine::new().layout_with_images(&doc, &options, &images);
	let image = rects(&snapshot)[0];
	let clusters = &snapshot.blocks[0].layout.text[0].clusters;
	let baselines: std::collections::HashSet<i32> = clusters
		.iter()
		.map(|c| (c.rect.y + c.rect.h).round() as i32)
		.collect();
	assert!(baselines.len() > 1, "the placeholder did not hyphenate");
	assert!(clusters.iter().all(|c| c.rect.x >= image.x
		&& c.rect.x + c.rect.w <= image.x + image.w + 0.5));
	assert_eq!(
		snapshot.extract_text(snapshot.select_all(7).unwrap(), 7),
		"hyphenation"
	);
}

#[test]
fn placeholder_updates_preserve_surrounding_text_selection_but_clear_changed_text()
 {
	use markview_core::text::{Affinity, TextPosition, TextSelection};
	let doc = document::parse("![alt](test.png) trailing words");
	let options =
		styled("[[rule]]\nwhen=['img','caption']\nsource='none'", 500.);
	let mut engine = LayoutEngine::new();
	let pending = engine.layout(&doc, &options);
	let text = &pending.blocks[0].layout.text[0].text;
	let start = text.find("trailing").unwrap();
	let position = |offset| TextPosition {
		revision: 7,
		block: 0,
		node: 0,
		offset,
		affinity: Affinity::Before,
	};
	let selected = TextSelection {
		anchor: position(start),
		focus: position(text.len()),
	};
	let loading = TextSelection {
		anchor: position(0),
		focus: position(start),
	};
	let ready = engine.layout_with_images(&doc, &options, &resources());
	let rebased = pending.rebase_selection(&ready, selected, 7, 7).unwrap();
	assert_eq!(ready.extract_text(rebased, 7), "trailing words");
	assert!(pending.rebase_selection(&ready, loading, 7, 7).is_none());
}
fn rects(snapshot: &LayoutSnapshot) -> Vec<markview_core::layout::Rect> {
	snapshot
		.blocks
		.iter()
		.flat_map(|b| {
			b.layout.draws.iter().filter_map(|d| {
				if let Draw::Image { rect, .. } = d {
					Some(*rect)
				} else {
					None
				}
			})
		})
		.collect()
}
fn no_overlap(snapshot: &LayoutSnapshot) {
	for block in &snapshot.blocks {
		for draw in &block.layout.draws {
			if let Draw::Image { rect, .. } = draw {
				assert!(rect.y + rect.h <= block.layout.height + 0.01);
				for node in &block.layout.text {
					for c in &node.clusters {
						if !matches!(
							block.layout.draws[c.command],
							Draw::Image { .. }
						) {
							assert!(
								rect.intersect(c.rect).is_none(),
								"image {rect:?} overlaps text {:?}",
								c.rect
							);
						}
					}
				}
			}
		}
	}
}

#[test]
fn markdown_references_and_html_preserve_image_semantics() {
	let doc = document::parse(
		"[![**alt**][pic]](https://example.com)\n\n[pic]: test.png \"Title\"\n\n<img title='src=wrong' src='a&amp;b.png' alt='&lt;图&gt;' width='120' height='0'>",
	);
	let mut images = Vec::new();
	for b in &doc.blocks {
		b.images(&mut images);
	}
	assert_eq!(images.len(), 2);
	assert_eq!(images[0].alt, "alt");
	assert_eq!(images[0].title, "Title");
	assert_eq!(images[1].src, "a&b.png");
	assert_eq!(images[1].alt, "<图>");
	assert_eq!(images[1].width, Some(120));
	assert_eq!(images[1].height, None);
	let BlockKind::Paragraph(rich) = &doc.blocks[0].kind else {
		panic!()
	};
	assert!(matches!(rich[0].kind, InlineKind::Image(_)));
	assert_eq!(rich[0].style.link.as_deref(), Some("https://example.com"));
}

#[test]
fn single_image_centers_and_shrinks_without_changing_copy() {
	let doc = document::parse("![图示](test.png)");
	let mut engine = LayoutEngine::new();
	for width in [400., 60.] {
		let snapshot = engine.layout_with_images(
			&doc,
			&LayoutOptions {
				width,
				fonts: fonts(),
				..Default::default()
			},
			&resources(),
		);
		let r = rects(&snapshot)[0];
		assert!((r.x - (width - r.w) / 2.).abs() < 0.01);
		assert!(r.w <= width);
		assert!((r.h / r.w - 1.4).abs() < 0.01);
		let selection = snapshot.select_all(1).unwrap();
		assert_eq!(snapshot.extract_text(selection, 1), "图示\n图示");
		no_overlap(&snapshot);
	}
}

#[test]
fn inline_images_share_their_line_and_never_wrap_text_beside_them() {
	for width in [420., 280.] {
		let snapshot = LayoutEngine::new().layout_with_images(
			&document::parse("前文 ![alt](test.png) 后文"),
			&LayoutOptions {
				width,
				fonts: fonts(),
				..Default::default()
			},
			&resources(),
		);
		let block = &snapshot.blocks[0];
		let rect = rects(&snapshot)[0];
		let clusters = &block.layout.text[0].clusters;
		// The image is an atomic inline box: the text before it stays on the
		// line, the text after it follows the box, and the line grows to fit.
		assert!(
			clusters
				.iter()
				.any(|c| c.rect.x + c.rect.w <= rect.x + 0.01),
			"no text before the image"
		);
		assert!(
			clusters.iter().any(|c| c.rect.x >= rect.x + rect.w - 0.01),
			"no text after the image"
		);
		assert!(block.layout.height >= rect.h);
		no_overlap(&snapshot);
	}
}

#[test]
fn image_titles_are_hit_testable_inside_their_box() {
	let snapshot = LayoutEngine::new().layout_with_images(
		&document::parse("![alt](test.png \"标题\")"),
		&LayoutOptions {
			width: 400.,
			fonts: fonts(),
			..Default::default()
		},
		&resources(),
	);
	let rect = rects(&snapshot)[0];
	let block = &snapshot.blocks[0];
	let horizontal = std::collections::HashMap::new();
	let title = |x, y| snapshot.image_title_at(x, y, &horizontal);
	assert_eq!(
		title(rect.x + rect.w / 2., block.y + rect.y + rect.h / 2.),
		Some("标题")
	);
	assert_eq!(
		title(rect.x + rect.w + 4., block.y + rect.y + rect.h / 2.),
		None
	);
	assert_eq!(
		title(rect.x + rect.w / 2., block.y + rect.y + rect.h + 4.),
		None
	);
}

#[test]
fn narrow_columns_multiple_images_math_and_containers_do_not_overlap() {
	let text="正文与图片之间必须保留空隙，数学公式 $\\frac{1}{\\frac{2}{3}}$ 会增加行高。".repeat(4);
	for width in [120., 280., 600.] {
		for greedy in [false, true] {
			for source in [
				format!("![one](test.png) {text} ![two](test.png) {text}"),
				format!("> ![one](test.png) {text}"),
				format!("- ![one](test.png) {text}"),
				format!(
					"| 图文 | 后列 |\n|---|---|\n| ![one](test.png) {text} | tail |"
				),
				"![one](test.png) ![two](test.png)".into(),
			] {
				let snapshot = LayoutEngine::new().layout_with_images(
					&document::parse(source),
					&LayoutOptions {
						width,
						greedy,
						fonts: fonts(),
						..Default::default()
					},
					&resources(),
				);
				assert!(snapshot.height.is_finite());
				no_overlap(&snapshot);
				let images = rects(&snapshot);
				for (i, a) in images.iter().enumerate() {
					for b in &images[i + 1..] {
						assert!(a.intersect(*b).is_none());
					}
				}
			}
		}
	}
}

#[test]
fn loading_failure_and_success_keep_logical_selection_and_targeted_cache() {
	let doc = document::parse(
		"![alt](test.png) surrounding text\n\nunchanged paragraph",
	);
	let mut e = LayoutEngine::new();
	let options = LayoutOptions {
		fonts: fonts(),
		..Default::default()
	};
	let pending = e.layout(&doc, &options);
	let selection = pending.select_all(7).unwrap();
	let mut images = resources();
	let ready = e.layout_with_images(&doc, &options, &images);
	assert_eq!(ready.reused, 1);
	assert!(
		pending
			.extract_text(selection, 7)
			.contains("Loading image…")
	);
	assert!(pending.rebase_selection(&ready, selection, 7, 7).is_none());
	images.entries.get_mut("test.png").unwrap().error =
		Some("broken image".into());
	let failed = e.layout_with_images(&doc, &options, &images);
	assert_eq!(failed.reused, 1);
	assert!(
		failed
			.extract_text(failed.select_all(7).unwrap(), 7)
			.contains("broken image")
	);
}
