use markview_document as document;
use markview_layout::{
	fonts::FontConfig,
	layout::{LayoutEngine, LayoutOptions},
};
use std::sync::Arc;

fn test_options() -> LayoutOptions {
	LayoutOptions {
		fonts: FontConfig {
			ignore_system_fonts: true,
			directories: vec![
				std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
					.join("tests/fonts"),
			],
			..Default::default()
		},
		..Default::default()
	}
}

#[test]
fn target_after_an_image_and_newline_resolves() {
	use markview_document::{BlockKind, InlineKind};
	use markview_layout::image::{ImageInfo, ImageSnapshot};

	let options = test_options();
	let mut engine = LayoutEngine::new();
	for source in [
		"<img>\n<em id='a'>",
		"<img>\n<em id='a'>\n",
		"<img>\n<em id='a'></em>\n",
		"<p><img>\n<em id='a'></em></p>",
		"<p><img> <b> </b><em id='a'></em></p>",
		"<p>Text <b> </b><em id='a'></em></p>",
	] {
		let doc = markview_commonmark::parse(source);
		let baseline =
			markview_commonmark::parse(source.replace(" id='a'", ""));
		let BlockKind::Paragraph(rich) = &doc.blocks[0].kind else {
			panic!("expected a paragraph");
		};
		assert!(rich.iter().all(|inline| {
			!matches!(&inline.kind, InlineKind::Text(text) if text.is_empty())
		}));
		for loaded in [false, true] {
			let mut images = ImageSnapshot::default();
			if loaded {
				images.entries.insert(
					String::new(),
					ImageInfo {
						version: 1,
						size: Some((100, 140)),
						error: None,
					},
				);
			}
			let layout = engine.layout_with_images(&doc, &options, &images);
			let plain = engine.layout_with_images(&baseline, &options, &images);
			assert!(layout.anchor_y("a").is_some(), "{source}");
			assert_eq!(layout.height, plain.height, "{source}");
			assert!(layout.same_reading_text(&plain), "{source}");
		}
	}
}

#[test]
fn html_targets_resolve_across_elements_and_preserve_heading_slugs() {
	let source = "<h2 id='custom'>Heading</h2>\n\n<div id='wrapper'>\n\nText <b id='bold'>bold</b> <a id='new' name='old'></a>end.\n\n</div>\n\n<hr id='rule'>\n\n<img id='image' src='missing.png'>\n\n<svg id='vector' width='10' height='10'></svg>\n\n<widget id='unsupported'>text</widget>\n\n<a name='last'></a>";
	let doc = markview_commonmark::parse(source);
	let layout = LayoutEngine::new().layout(&doc, &test_options());
	for anchor in [
		"custom",
		"heading",
		"wrapper",
		"bold",
		"new",
		"old",
		"rule",
		"image",
		"vector",
		"unsupported",
		"last",
	] {
		assert!(layout.anchor_y(anchor).is_some(), "{anchor}");
	}
	assert_eq!(layout.anchor_y("new"), layout.anchor_y("old"));
}

#[test]
fn inline_and_empty_targets_follow_their_lines_without_adding_text_or_height() {
	for raw in [false, true] {
		let text = "word ".repeat(100);
		let body = format!(
			"{text}<a id='target' name='旧锚点'></a>tail<a id='end'></a>"
		);
		let source = if raw { format!("<p>{body}</p>") } else { body };
		let baseline = source
			.replace("<a id='target' name='旧锚点'></a>", "")
			.replace("<a id='end'></a>", "");
		let options = LayoutOptions {
			width: 240.,
			..test_options()
		};
		let mut engine = LayoutEngine::new();
		let layout =
			engine.layout(&markview_commonmark::parse(source), &options);
		let plain =
			engine.layout(&markview_commonmark::parse(baseline), &options);
		assert_eq!(layout.height, plain.height);
		assert!(layout.same_reading_text(&plain));
		assert!(layout.anchor_y("target").unwrap() > 0.);
		assert_eq!(layout.anchor_y("target"), layout.anchor_y("旧锚点"));
		assert_eq!(layout.anchor_y("target"), layout.anchor_y("end"));
	}
}

#[test]
fn targets_inside_nested_details_report_the_disclosures_to_expand() {
	let doc = markview_commonmark::parse(
		"<details id='outer'><summary id='summary'>Outer</summary>\n\n<details><summary>Inner</summary>\n\n<div id='container'>\n\nBody <a name='hidden'></a>target\n\n</div>\n\n</details>\n\n</details>",
	);
	let enclosing = doc.details_enclosing("hidden");
	assert_eq!(enclosing.len(), 2);
	assert_eq!(doc.details_enclosing("container"), enclosing);
	assert!(doc.details_enclosing("outer").is_empty());
	assert!(doc.details_enclosing("summary").is_empty());
	let mut engine = LayoutEngine::new();
	let collapsed = engine.layout(&doc, &test_options());
	assert!(collapsed.anchor_y("hidden").is_none());
	assert!(collapsed.anchor_y("summary").is_some());
	let options = LayoutOptions {
		details_open: Arc::new(
			enclosing.into_iter().map(|id| (id, true)).collect(),
		),
		..test_options()
	};
	assert!(engine.layout(&doc, &options).anchor_y("hidden").is_some());
}

#[test]
fn a_footnote_reference_hidden_by_details_reports_its_disclosure() {
	// The definition is hoisted out of the body, so only the reference sits
	// under the disclosure and only `fnref:<number>` names it.
	let doc = markview_commonmark::parse(
		"<details><summary>S</summary>\n\nText[^f]\n\n</details>\n\n[^f]: note\n",
	);
	let enclosing = doc.details_enclosing("fnref:1");
	assert_eq!(enclosing.len(), 1);
	assert!(doc.details_enclosing("fn:1").is_empty());
	let mut engine = LayoutEngine::new();
	let collapsed = engine.layout(&doc, &test_options());
	assert!(collapsed.anchor_y("fnref:1").is_none());
	let options = LayoutOptions {
		details_open: Arc::new(
			enclosing.into_iter().map(|id| (id, true)).collect(),
		),
		..test_options()
	};
	assert!(engine.layout(&doc, &options).anchor_y("fnref:1").is_some());
}

#[test]
fn only_real_html_registers_targets_and_anchor_changes_invalidate_cached_layouts()
 {
	let source = "`<a id='code'></a>`\n\n```html\n<div id='fence'>\n```\n\n<!-- <a id='comment'></a> -->\n\n<script id='script'><a id='script-body'></a></script>\n\nText <b ID='中文&amp;目标'>bold</b> <a id='' name='legacy'></a>\n\n<a name='duplicate'></a>first\n\n<a name='duplicate'></a>second";
	let doc = markview_commonmark::parse(source);
	let mut engine = LayoutEngine::new();
	let options = test_options();
	let layout = engine.layout(&doc, &options);
	for anchor in ["code", "fence", "comment", "script-body", ""] {
		assert!(layout.anchor_y(anchor).is_none(), "{anchor}");
	}
	for anchor in ["script", "中文&目标", "legacy", "duplicate"] {
		assert!(layout.anchor_y(anchor).is_some(), "{anchor}");
	}
	let changed = markview_commonmark::reparse(
		&doc,
		Arc::from(source.replace("中文&amp;目标", "changed")),
	);
	let layout = engine.layout(&changed, &options);
	assert!(layout.anchor_y("中文&目标").is_none());
	assert!(layout.anchor_y("changed").is_some());
}

#[test]
fn pagination_keeps_empty_targets_and_uses_the_first_duplicate() {
	use markview_layout::paginate::{PageGeometry, paginate};
	let doc = markview_commonmark::parse(
		"<a name='start'></a>\n\n<div id='duplicate'>\n\nText <b id='duplicate'>bold</b>\n\n</div>\n\n<a name='end'></a>",
	);
	let layout = LayoutEngine::new().layout(&doc, &test_options());
	let pages = paginate(
		&doc,
		&layout,
		&PageGeometry {
			width_pt: 595.,
			height_pt: 842.,
			margin_pt: [40.; 4],
		},
	);
	for anchor in ["start", "duplicate", "end"] {
		assert!(pages.anchors.contains_key(anchor), "{anchor}");
	}
	let (page, y) = pages.anchors["duplicate"];
	assert_eq!(page, 0);
	assert!(y <= pages.anchors["end"].1);
	assert_eq!(
		layout.anchor_y("duplicate"),
		Some(layout.blocks[1].y + layout.blocks[1].layout.anchors[0].y)
	);
}

#[test]
fn targets_after_hard_breaks_remain_on_their_own_line() {
	let doc = markview_commonmark::parse(
		"<p><b id='first'>one</b><br><a id='second'></a>two<br><a id='third'></a>three</p>",
	);
	let layout = LayoutEngine::new().layout(&doc, &test_options());
	assert!(
		layout.anchor_y("first").unwrap() < layout.anchor_y("second").unwrap()
	);
	assert!(
		layout.anchor_y("second").unwrap() < layout.anchor_y("third").unwrap()
	);
}

#[test]
fn summary_descendant_targets_keep_their_inline_positions() {
	use markview_layout::paginate::{PageGeometry, paginate};
	for prefix in ["first<br>".to_string(), "word ".repeat(100)] {
		let doc = markview_commonmark::parse(format!(
			"<details id='details'><summary id='summary'>{prefix}<b id='target'>second</b></summary>Body</details>"
		));
		let layout = LayoutEngine::new().layout(
			&doc,
			&LayoutOptions {
				width: 240.,
				..test_options()
			},
		);
		let targets: Vec<_> = layout.blocks[0]
			.layout
			.anchors
			.iter()
			.filter(|a| a.anchor == "target")
			.collect();
		assert_eq!(targets.len(), 1);
		assert!(
			layout.anchor_y("target").unwrap()
				> layout.anchor_y("summary").unwrap()
		);
		assert!(doc.details_enclosing("target").is_empty());
		let pages = paginate(
			&doc,
			&layout,
			&PageGeometry {
				width_pt: 595.,
				height_pt: 842.,
				margin_pt: [40.; 4],
			},
		);
		assert!(pages.anchors["target"].1 > pages.anchors["summary"].1);
	}
}

#[test]
fn invisible_targets_preserve_normalized_whitespace() {
	for source in [
		"one <a id='target'></a> two",
		"one <a id='target'></a> <a name='other'></a> two",
		"<p>one <a id='target'></a> two</p>",
		"<p>one <a id='target'></a><a name='other'></a>  two</p>",
		"<p>one <a id='target'></a> <b>two</b></p>",
		"<p>one <a id='target'></a> <br>two</p>",
		"<details><summary>one <a id='target'></a> two</summary>Body</details>",
	] {
		let baseline = source
			.replace(" id='target'", "")
			.replace(" name='other'", "");
		let mut engine = LayoutEngine::new();
		let options = LayoutOptions {
			width: 100.,
			..test_options()
		};
		let layout =
			engine.layout(&markview_commonmark::parse(source), &options);
		let plain =
			engine.layout(&markview_commonmark::parse(baseline), &options);
		assert!(layout.same_reading_text(&plain), "{source}");
		assert_eq!(layout.height, plain.height, "{source}");
		assert!(layout.anchor_y("target").is_some(), "{source}");
	}
	let doc = markview_commonmark::parse("<p>one <a id='target'></a> two</p>");
	let document::BlockKind::Paragraph(text) = &doc.blocks[0].kind else {
		panic!("expected paragraph")
	};
	assert_eq!(document::plain_text(text), "one two");
}

#[test]
fn inline_opaque_contents_do_not_register_targets() {
	for tag in ["script", "style", "textarea", "title", "pre", "math"] {
		for body in [
			"<a id='inner' name='legacy'></a>",
			"*<a id='inner' name='legacy'></a>*",
			"<svg id='inner'></svg>",
		] {
			let source = format!(
				"Text <{tag} id='outer'>{body}</{tag}> <a id='after'></a>tail"
			);
			let doc = markview_commonmark::parse(source.clone());
			let layout = LayoutEngine::new().layout(&doc, &test_options());
			for anchor in ["outer", "after"] {
				assert!(
					layout.anchor_y(anchor).is_some(),
					"{source}: {anchor}"
				);
			}
			for anchor in ["inner", "legacy"] {
				assert!(
					layout.anchor_y(anchor).is_none(),
					"{source}: {anchor}"
				);
			}
		}
	}
}

#[test]
fn empty_line_targets_register_before_the_next_visible_line() {
	for greedy in [false, true] {
		for blanks in [
			"<a id='blank'></a><br>",
			"<a id='blank'></a><br><a id='another'></a><br>",
		] {
			let doc = markview_commonmark::parse(format!(
				"<p><b id='one'>one</b><br>{blanks}<b id='two'>two</b></p>"
			));
			let options = LayoutOptions {
				greedy,
				..test_options()
			};
			let mut engine = LayoutEngine::new();
			let layout = engine.layout(&doc, &options);
			let one = layout.anchor_y("one").unwrap();
			let blank = layout.anchor_y("blank").unwrap();
			let two = layout.anchor_y("two").unwrap();
			assert!(one < blank && blank < two, "{greedy}: {blanks}");
			if let Some(another) = layout.anchor_y("another") {
				assert!(blank < another && another < two);
			}
			let baseline = markview_commonmark::parse(
				doc.source
					.replace("<a id='blank'></a>", "")
					.replace("<a id='another'></a>", ""),
			);
			let plain = engine.layout(&baseline, &options);
			assert_eq!(layout.height, plain.height);
			assert!(layout.same_reading_text(&plain));
		}
	}
}

#[test]
fn standalone_and_additional_summaries_keep_their_element_targets() {
	use markview_layout::paginate::{PageGeometry, paginate};
	for source in [
		"<summary id='target'>Summary</summary>",
		"<details open><summary>First</summary><summary id='target'>Second</summary></details>",
		"<details><summary>First</summary><summary id='target'>Second</summary></details>",
	] {
		let doc = markview_commonmark::parse(source);
		let enclosing = doc.details_enclosing("target");
		let options = LayoutOptions {
			details_open: Arc::new(
				enclosing.into_iter().map(|id| (id, true)).collect(),
			),
			..test_options()
		};
		let layout = LayoutEngine::new().layout(&doc, &options);
		assert!(layout.anchor_y("target").is_some(), "{source}");
		let mut code = Vec::new();
		for block in &doc.blocks {
			block.code_blocks(&mut code);
		}
		assert!(
			code.iter()
				.any(|(_, text)| text.contains("<summary id='target'>")),
			"{source}"
		);
		let pages = paginate(
			&doc,
			&layout,
			&PageGeometry {
				width_pt: 595.,
				height_pt: 842.,
				margin_pt: [40.; 4],
			},
		);
		assert!(pages.anchors.contains_key("target"), "{source}");
	}
}

#[test]
fn invisible_targets_preserve_styled_run_geometry() {
	use markview_layout::style::Stylesheet;
	let mut stylesheet = (*Stylesheet::bundled(false)).clone();
	stylesheet.merge(
		&Stylesheet::parse(
			"format_version=2\nversion=1\n[[rule]]\nwhen=['code']\npadding=[0,1,0,1]",
		)
		.unwrap(),
	);
	for width in [100., 240.] {
		for source in [
			"<code>one<a id='x'></a>two</code>",
			"Text <code>one<a id='x'></a>two</code> tail",
			"<code>one<a id='x'></a>two<a name='y'></a>three</code>",
			"<b>of<a id='x'></a>fice</b>",
		] {
			let baseline =
				source.replace(" id='x'", "").replace(" name='y'", "");
			let options = LayoutOptions {
				width,
				stylesheet: Arc::new(stylesheet.clone()),
				..test_options()
			};
			let mut engine = LayoutEngine::new();
			let anchored =
				engine.layout(&markview_commonmark::parse(source), &options);
			let plain =
				engine.layout(&markview_commonmark::parse(baseline), &options);
			assert_eq!(anchored.height, plain.height, "{width}: {source}");
			assert!(anchored.same_reading_text(&plain));
			let rects = |layout: &markview_layout::scene::LayoutSnapshot| {
				layout
					.blocks
					.iter()
					.flat_map(|b| &b.layout.text)
					.flat_map(|node| &node.clusters)
					.map(|cluster| {
						(
							cluster.rect.x,
							cluster.rect.y,
							cluster.rect.w,
							cluster.rect.h,
						)
					})
					.collect::<Vec<_>>()
			};
			assert_eq!(rects(&anchored), rects(&plain), "{width}: {source}");
			assert!(anchored.anchor_y("x").is_some());
		}
	}
}

#[test]
fn heading_slugs_win_collisions_with_descendant_targets() {
	use markview_layout::paginate::{PageGeometry, paginate};
	let geometry = PageGeometry {
		width_pt: 595.,
		height_pt: 842.,
		margin_pt: [40.; 4],
	};
	for source in [
		"# first<br>second<a id='firstsecond'></a>",
		"<h1>first<br>second<a id='firstsecond'></a></h1>",
	] {
		let doc = markview_commonmark::parse(source);
		let baseline =
			markview_commonmark::parse(source.replace(" id='firstsecond'", ""));
		let mut engine = LayoutEngine::new();
		let options = test_options();
		let anchored = engine.layout(&doc, &options);
		let plain = engine.layout(&baseline, &options);
		assert_eq!(
			anchored.anchor_y("firstsecond"),
			plain.anchor_y("firstsecond"),
			"{source}"
		);
		let targets: Vec<_> = anchored.blocks[0]
			.layout
			.anchors
			.iter()
			.filter(|a| a.anchor == "firstsecond")
			.collect();
		assert_eq!(targets.len(), 2);
		assert!(targets[0].y < targets[1].y);
		assert_eq!(
			paginate(&doc, &anchored, &geometry).anchors["firstsecond"],
			paginate(&baseline, &plain, &geometry).anchors["firstsecond"]
		);
	}
}

#[test]
fn invisible_container_edges_preserve_visible_child_spacing() {
	use markview_layout::style::Stylesheet;
	for custom in [false, true] {
		let mut sheet = (*Stylesheet::bundled(false)).clone();
		if custom {
			sheet.merge(&Stylesheet::parse("format_version=2\nversion=1\n[[rule]]\nwhen=['p']\nspace_before=1\nspace_after=1").unwrap());
		}
		for source in [
			"> <a id='before'></a>\n>\n> # Heading\n>\n> Text\n>\n> <a id='after'></a>",
			"<details open><summary>Title</summary>\n\n<a id='before'></a>\n\n# Heading\n\nText\n\n<a id='after'></a>\n\n</details>",
			"- # Heading\n\n  Text\n\n  <a id='after'></a>",
		] {
			let baseline = source
				.replace("<a id='before'></a>", "")
				.replace("<a id='after'></a>", "");
			let mut engine = LayoutEngine::new();
			let options = LayoutOptions {
				stylesheet: Arc::new(sheet.clone()),
				..test_options()
			};
			let layout =
				engine.layout(&markview_commonmark::parse(source), &options);
			let plain =
				engine.layout(&markview_commonmark::parse(baseline), &options);
			assert_eq!(layout.height, plain.height, "{custom}: {source}");
			assert_eq!(
				layout.anchor_y("heading"),
				plain.anchor_y("heading"),
				"{custom}: {source}"
			);
			assert!(layout.same_reading_text(&plain));
		}
	}
}

#[test]
fn empty_html_headings_are_invisible_targets() {
	for source in [
		"<h1 id='target'></h1>\n\nText",
		"<h2><a name='target'></a></h2>\n\nText",
	] {
		let doc = markview_commonmark::parse(source);
		let mut engine = LayoutEngine::new();
		let options = test_options();
		let layout = engine.layout(&doc, &options);
		let plain =
			engine.layout(&markview_commonmark::parse("Text"), &options);
		assert_eq!(layout.height, plain.height);
		assert_eq!(
			layout.extract_text(layout.select_all(1).unwrap(), 1),
			plain.extract_text(plain.select_all(1).unwrap(), 1)
		);
		assert!(doc.outline().is_empty());
		assert!(layout.anchor_y("target").is_some());
	}
}

#[test]
fn footnote_groups_traverse_invisible_targets() {
	for gap in [
		"<a id='target'></a>",
		" <a id='target'></a> ",
		"<a id='target'></a><a name='legacy'></a>",
	] {
		let source =
			format!("Text[^a]{gap}[^b]\n\n[^a]: First\n\n[^b]: Second");
		let baseline = source
			.replace(" id='target'", "")
			.replace(" name='legacy'", "");
		let mut engine = LayoutEngine::new();
		let options = test_options();
		let layout =
			engine.layout(&markview_commonmark::parse(source), &options);
		let plain =
			engine.layout(&markview_commonmark::parse(baseline), &options);
		let copied = layout.extract_text(layout.select_all(1).unwrap(), 1);
		assert!(copied.starts_with("Text[1,2]"), "{copied}");
		assert_eq!(copied, plain.extract_text(plain.select_all(1).unwrap(), 1));
		assert_eq!(layout.height, plain.height);
		assert!(layout.anchor_y("target").is_some());
	}
}

#[test]
fn unsupported_summary_descendants_keep_targets() {
	use markview_layout::paginate::{PageGeometry, paginate};
	let doc = markview_commonmark::parse(
		"<details><summary id='summary'><span id='target'>Title</span></summary>Body</details>",
	);
	let layout = LayoutEngine::new().layout(&doc, &test_options());
	assert!(layout.anchor_y("target").is_some());
	assert!(doc.details_enclosing("target").is_empty());
	let pages = paginate(
		&doc,
		&layout,
		&PageGeometry {
			width_pt: 595.,
			height_pt: 842.,
			margin_pt: [40.; 4],
		},
	);
	assert!(pages.anchors.contains_key("target"));
}

#[test]
fn opaque_target_exclusion_survives_blank_lines_and_html_blocks() {
	for tag in ["script", "style", "textarea", "title", "pre", "math"] {
		for inner in [
			"<a id='inner'></a>body",
			"body <a id='inner'></a>",
			"<svg id='inner'></svg>",
			"<div id='inner'>body</div>",
			"<style><a id='inner'>body",
			"<details><summary><span id='inner'>body</span></summary></details>",
		] {
			let doc = markview_commonmark::parse(format!(
				"Text <{tag} id='outer'>start\n\n{inner}\n\n</{tag}>\n\nText <a id='after'></a>end"
			));
			let layout = LayoutEngine::new().layout(&doc, &test_options());
			assert!(layout.anchor_y("outer").is_some(), "{tag}: {inner}");
			assert!(layout.anchor_y("inner").is_none(), "{tag}: {inner}");
			assert!(layout.anchor_y("after").is_some(), "{tag}: {inner}");
		}
	}
}

#[test]
fn opaque_html_edits_match_full_parses() {
	for tag in ["pre", "script", "style", "textarea", "title", "math"] {
		let source = format!(
			"Text <{tag}>start\n\nbody <a id='inner'></a>text\n\nend </{tag}> <a id='after'></a>"
		);
		let previous = markview_commonmark::parse(source.clone());
		for changed in [
			source.replace("body", "changed"),
			source.replace(&format!("<{tag}>"), ""),
		] {
			assert!(
				markview_commonmark::parse_incremental(
					&previous,
					Arc::from(changed.clone()),
					markview_commonmark::Dialect::default()
				)
				.is_none()
			);
			let reparsed = markview_commonmark::reparse(
				&previous,
				Arc::from(changed.clone()),
			);
			let full = markview_commonmark::parse(changed);
			assert_eq!(reparsed.content_id, full.content_id, "{tag}");
			let mut engine = LayoutEngine::new();
			let options = test_options();
			let updated = engine.layout(&reparsed, &options);
			let expected = engine.layout(&full, &options);
			for target in ["inner", "after"] {
				assert_eq!(
					updated.anchor_y(target),
					expected.anchor_y(target),
					"{tag}: {target}"
				);
			}
		}
		let plain =
			markview_commonmark::parse(source.replace(&format!("<{tag}>"), ""));
		assert!(
			markview_commonmark::parse_incremental(
				&plain,
				Arc::from(source),
				markview_commonmark::Dialect::default()
			)
			.is_none()
		);
	}
}

#[test]
fn image_alt_html_does_not_change_document_targets() {
	for tag in ["script", "pre", "style", "textarea", "title", "math"] {
		let doc = markview_commonmark::parse(format!(
			"![<{tag} id='alt'>](missing.png) Text <a id='after'></a>end\n\nLater <a id='later'></a>text"
		));
		let layout = LayoutEngine::new().layout(&doc, &test_options());
		assert!(layout.anchor_y("alt").is_none(), "{tag}");
		assert!(layout.anchor_y("after").is_some(), "{tag}");
		assert!(layout.anchor_y("later").is_some(), "{tag}");
		let document::BlockKind::Paragraph(text) = &doc.blocks[0].kind else {
			panic!("expected paragraph")
		};
		assert!(text.iter().any(|i| matches!(&i.kind, document::InlineKind::Image(image) if image.alt == format!("<{tag} id='alt'>"))));
	}
}

#[test]
fn targets_on_both_sides_of_breaks_keep_their_lines() {
	for greedy in [false, true] {
		for source in [
			"<p><b id='first'>one</b><a id='end'></a><br><a id='start'></a>two</p>",
			"Text <b id='first'>one</b><a id='end'></a><br><a id='start'></a>two",
			"<p><b id='first'>one</b><a id='end'></a><br><a id='blank'></a><br><a id='start'></a>two</p>",
			"<p><b id='first'>one</b> <a id='end'></a><br><a id='start'></a>two</p>",
		] {
			let layout = LayoutEngine::new().layout(
				&markview_commonmark::parse(source),
				&LayoutOptions {
					greedy,
					..test_options()
				},
			);
			assert_eq!(
				layout.anchor_y("first"),
				layout.anchor_y("end"),
				"{greedy}: {source}"
			);
			assert!(
				layout.anchor_y("end").unwrap()
					< layout.anchor_y("start").unwrap()
			);
			if let Some(blank) = layout.anchor_y("blank") {
				assert!(layout.anchor_y("end").unwrap() < blank);
				assert!(blank < layout.anchor_y("start").unwrap());
			}
		}
	}
}

#[test]
fn root_child_styles_ignore_invisible_targets_and_reuse_geometry() {
	use markview_layout::style::Stylesheet;
	let mut sheet = (*Stylesheet::bundled(false)).clone();
	sheet.merge(&Stylesheet::parse("format_version=2\nversion=1\n[[rule]]\nwhen=['p','first_child']\nsize=2\n[[rule]]\nwhen=['p','last_child']\nspace_after=2").unwrap());
	let options = LayoutOptions {
		stylesheet: Arc::new(sheet),
		..test_options()
	};
	for body in ["Text", "First\n\nMiddle\n\nLast"] {
		let mut engine = LayoutEngine::new();
		let plain = engine.layout(&markview_commonmark::parse(body), &options);
		for source in [
			format!("<a id='before'></a>\n\n{body}"),
			format!("{body}\n\n<a id='after'></a>"),
			format!("<a id='before'></a>\n\n{body}\n\n<a id='after'></a>"),
		] {
			let doc = markview_commonmark::parse(source.clone());
			let layout = engine.layout(&doc, &options);
			let fresh = LayoutEngine::new().layout(&doc, &options);
			assert_eq!(layout.height, plain.height, "{source}");
			assert_eq!(fresh.height, plain.height, "{source}");
			let visible: Vec<_> = layout
				.blocks
				.iter()
				.filter(|b| !b.layout.text.is_empty())
				.collect();
			assert_eq!(visible.len(), plain.blocks.len());
			for (actual, expected) in visible.iter().zip(&plain.blocks) {
				assert_eq!(actual.y, expected.y);
				assert!(
					Arc::ptr_eq(&actual.layout, &expected.layout),
					"{source}"
				);
			}
			let restored =
				engine.layout(&markview_commonmark::parse(body), &options);
			assert_eq!(restored.height, plain.height);
		}
	}
}

#[test]
fn block_html_opaque_scopes_exclude_later_targets() {
	for tag in ["title", "textarea", "pre", "math", "script", "style"] {
		for prefix in ["", "<div>\n", "> "] {
			let source = format!(
				"<{tag} id='outer'>start\n\nText <a id='inner'></a>body\n\n</{tag}>\n\nText <a id='after'></a>end"
			);
			let source = match prefix {
				"<div>\n" => format!("{prefix}{source}\n\n</div>"),
				"> " => source
					.lines()
					.map(|line| format!("> {line}"))
					.collect::<Vec<_>>()
					.join("\n"),
				_ => source,
			};
			let layout = LayoutEngine::new().layout(
				&markview_commonmark::parse(source.clone()),
				&test_options(),
			);
			assert!(layout.anchor_y("outer").is_some(), "{source}");
			assert!(layout.anchor_y("inner").is_none(), "{source}");
			assert!(layout.anchor_y("after").is_some(), "{source}");
		}
	}
	for source in [
		"<!-- <title> -->\n\nText <a id='after'></a>end",
		"<div data-tag='<title>'>\n\nText <a id='after'></a>end\n\n</div>",
	] {
		let layout = LayoutEngine::new()
			.layout(&markview_commonmark::parse(source), &test_options());
		assert!(layout.anchor_y("after").is_some(), "{source}");
	}
}

#[test]
fn soft_wrap_targets_stay_on_their_side_of_discarded_spaces() {
	for greedy in [false, true] {
		for raw in [false, true] {
			let body = "<b id='first'>one</b><a id='end'></a> <a id='start'></a><b id='second'>two</b> three four";
			let source = if raw {
				format!("<p>{body}</p>")
			} else {
				body.to_string()
			};
			let options = LayoutOptions {
				width: 60.,
				greedy,
				..test_options()
			};
			let layout = LayoutEngine::new()
				.layout(&markview_commonmark::parse(source.clone()), &options);
			assert_eq!(
				layout.anchor_y("first"),
				layout.anchor_y("end"),
				"{greedy}: {source}"
			);
			assert_eq!(
				layout.anchor_y("second"),
				layout.anchor_y("start"),
				"{greedy}: {source}"
			);
			assert!(
				layout.anchor_y("end").unwrap()
					< layout.anchor_y("start").unwrap()
			);
		}
	}
}

#[test]
fn inline_non_tag_html_is_ignored_without_changing_targets() {
	for fragment in [
		"<!-- user's comment -->",
		"<!-- <b id='false'> -->",
		"<!-- user's <script> -->",
		"<?user's <script>?>",
		"<!DOCTYPE html>",
		"<![CDATA[ user's <script> ]]>",
	] {
		let source = format!(
			"Text {fragment} <a id='after'></a>tail\n\nLater <a id='later'></a>end"
		);
		let doc = markview_commonmark::parse(source);
		let layout = LayoutEngine::new().layout(&doc, &test_options());
		assert_eq!(
			layout.extract_text(layout.select_all(1).unwrap(), 1),
			"Text tail\n\nLater end",
			"{fragment}"
		);
		assert!(layout.anchor_y("false").is_none(), "{fragment}");
		assert!(layout.anchor_y("after").is_some(), "{fragment}");
		assert!(layout.anchor_y("later").is_some(), "{fragment}");
	}
}

#[test]
fn invisible_math_prefixes_preserve_copied_paragraph_separators() {
	let mut engine = LayoutEngine::new();
	let options = test_options();
	let baseline = engine.layout(
		&markview_commonmark::parse("Before\n\n$$a$$\n\nAfter"),
		&options,
	);
	for prefix in [
		"<a id='x'></a>",
		"<a name='x'></a>",
		"<a id='x'></a><a id='other'></a>",
	] {
		for suffix in ["", "<a id='end'></a>"] {
			let doc = markview_commonmark::parse(format!(
				"Before\n\n{prefix}$$a$${suffix}\n\nAfter"
			));
			let layout = engine.layout(&doc, &options);
			assert_eq!(
				layout.extract_text(layout.select_all(1).unwrap(), 1),
				"Before\n\na\n\nAfter",
				"{prefix}: {suffix}"
			);
			assert_eq!(layout.height, baseline.height);
			assert!(layout.anchor_y("x").is_some());
			assert!(
				layout
					.blocks
					.iter()
					.flat_map(|b| &b.layout.text)
					.all(|node| !node.text.is_empty())
			);
		}
	}
}

#[test]
fn opaque_elements_close_after_literal_less_than_signs() {
	for tag in ["pre", "math", "svg"] {
		for prefix in ["", "Text "] {
			let source = format!(
				"{prefix}<{tag} id='outer'>2 < 3 < 4</{tag}>\n\n<a id='after'></a>end"
			);
			let layout = LayoutEngine::new().layout(
				&markview_commonmark::parse(source.clone()),
				&test_options(),
			);
			assert!(layout.anchor_y("outer").is_some(), "{source}");
			assert!(layout.anchor_y("after").is_some(), "{source}");
		}
	}
}

#[test]
fn non_element_html_regions_do_not_open_opaque_scopes() {
	for region in [
		"<![CDATA[ <script id='false'> ]]>",
		"<?xml user's <script id='false'>?>",
		"<!-- user's <script id='false'> -->",
		"<!DOCTYPE data \"<script id='false'>\">",
	] {
		let source = format!("{region}\n\n<a id='after'></a>end");
		let layout = LayoutEngine::new()
			.layout(&markview_commonmark::parse(source), &test_options());
		assert!(layout.anchor_y("false").is_none(), "{region}");
		assert!(layout.anchor_y("after").is_some(), "{region}");
	}
}

#[test]
fn raw_html_comments_after_text_do_not_register_targets() {
	for region in [
		"<!-- <a id='false'></a> -->",
		"<!-- user's <a id='false'></a> -->",
		"<![CDATA[ <a id='false'></a> ]]>",
		"<?xml <a id='false'></a>?>",
	] {
		let source = format!("<p>before {region} <a id='after'></a>after</p>");
		let layout = LayoutEngine::new()
			.layout(&markview_commonmark::parse(source), &test_options());
		assert!(layout.anchor_y("false").is_none(), "{region}");
		assert!(layout.anchor_y("after").is_some(), "{region}");
		assert_eq!(
			layout.extract_text(layout.select_all(1).unwrap(), 1),
			"before after",
			"{region}"
		);
	}
}

#[test]
fn svg_snippets_preserve_original_opaque_extents() {
	for tag in ["pre", "math", "title"] {
		let source = format!(
			"Text <{tag} id='outer'><svg id='inside'>\n\n</svg>\n\nBody <a id='body'></a>text\n\n</{tag}>\n\nText <a id='after'></a>end"
		);
		for quoted in [false, true] {
			let source = if quoted {
				source
					.lines()
					.map(|l| format!("> {l}"))
					.collect::<Vec<_>>()
					.join("\n")
			} else {
				source.clone()
			};
			let layout = LayoutEngine::new().layout(
				&markview_commonmark::parse(source.clone()),
				&test_options(),
			);
			assert!(layout.anchor_y("outer").is_some(), "{source}");
			assert!(layout.anchor_y("inside").is_none(), "{source}");
			assert!(layout.anchor_y("body").is_none(), "{source}");
			assert!(layout.anchor_y("after").is_some(), "{source}");
		}
	}
	for tag in ["pre", "math", "title"] {
		let source = format!(
			"Text <svg id='visible'>\n\n</svg> Text <{tag} id='outer'>\n\nBody <a id='body'></a>text\n\n</{tag}>\n\nText <a id='after'></a>end"
		);
		let layout = LayoutEngine::new().layout(
			&markview_commonmark::parse(source.clone()),
			&test_options(),
		);
		assert!(layout.anchor_y("visible").is_some(), "{source}");
		assert!(layout.anchor_y("outer").is_some(), "{source}");
		assert!(layout.anchor_y("body").is_none(), "{source}");
		assert!(layout.anchor_y("after").is_some(), "{source}");
	}
	for prefix in [
		"Text `<pre>`",
		"![<pre>](missing.png)",
		"Text <!-- <pre> -->",
	] {
		let source = format!(
			"{prefix}<svg id='visible'>\n\n</svg>\n\nText <a id='after'></a>end"
		);
		let layout = LayoutEngine::new()
			.layout(&markview_commonmark::parse(source), &test_options());
		assert!(layout.anchor_y("visible").is_some(), "{prefix}");
		assert!(layout.anchor_y("after").is_some(), "{prefix}");
	}
}

#[test]
fn footnote_targets_win_collisions_with_descendants() {
	use markview_layout::paginate::{PageGeometry, paginate};
	let source = "Text[^a]\n\n[^a]: First<br>second<a id='fn:1'></a>";
	let doc = markview_commonmark::parse(source);
	let baseline = markview_commonmark::parse(source.replace(" id='fn:1'", ""));
	let mut engine = LayoutEngine::new();
	let options = test_options();
	let layout = engine.layout(&doc, &options);
	let plain = engine.layout(&baseline, &options);
	assert_eq!(layout.anchor_y("fn:1"), plain.anchor_y("fn:1"));
	let targets: Vec<_> = layout
		.blocks
		.iter()
		.flat_map(|b| &b.layout.anchors)
		.filter(|a| a.anchor == "fn:1")
		.collect();
	assert_eq!(targets.len(), 2);
	assert!(targets[0].y < targets[1].y);
	let geometry = PageGeometry {
		width_pt: 595.,
		height_pt: 842.,
		margin_pt: [40.; 4],
	};
	assert_eq!(
		paginate(&doc, &layout, &geometry).anchors["fn:1"],
		paginate(&baseline, &plain, &geometry).anchors["fn:1"]
	);
}

#[test]
fn summary_opaque_offsets_use_the_original_body_range() {
	for body in [
		"</pre>中文中文中文中文",
		"</pre><a id='after'></a>text",
		"<a id='hidden'></a></pre>中文<a id='after'></a>标题",
		"<a id='hidden'></a>\n</pre>\n中文<a id='after'></a>标题",
	] {
		for quoted in [false, true] {
			let source = format!(
				"Text <pre>\n\n<details open><summary>{body}</summary></details>"
			);
			let source = if quoted {
				source
					.lines()
					.map(|l| format!("> {l}"))
					.collect::<Vec<_>>()
					.join("\n")
			} else {
				source
			};
			let doc = markview_commonmark::parse(source.clone());
			let layout = LayoutEngine::new().layout(&doc, &test_options());
			assert!(layout.anchor_y("hidden").is_none(), "{source}");
			if body.contains("id='after'") {
				assert!(layout.anchor_y("after").is_some(), "{source}");
				let pages = markview_layout::paginate::paginate(
					&doc,
					&layout,
					&markview_layout::paginate::PageGeometry {
						width_pt: 595.,
						height_pt: 842.,
						margin_pt: [40.; 4],
					},
				);
				assert!(pages.anchors.contains_key("after"), "{source}");
			}
		}
	}
}
