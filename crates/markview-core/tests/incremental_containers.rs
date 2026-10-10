use markview_core::{
	document,
	fonts::FontConfig,
	image::{ImageInfo, ImageSnapshot},
	layout::{LayoutEngine, LayoutOptions},
	scene::{Draw, LayoutSnapshot, Rect},
	search::{SearchIndex, SearchOptions},
	style::Stylesheet,
};
use std::{collections::BTreeMap, sync::Arc};

fn options() -> LayoutOptions {
	LayoutOptions {
		width: 340.,
		fonts: FontConfig::from_faces(
			0x6e657374,
			vec![parley::fontique::Blob::new(Arc::new(
				include_bytes!("fonts/NotoSerif-Regular-subset.otf").as_slice(),
			))],
		),
		..Default::default()
	}
}

fn close(a: f32, b: f32) {
	assert!((a - b).abs() < 0.01, "{a} != {b}");
}

fn rect(a: Rect, b: Rect) {
	for (a, b) in [a.x, a.y, a.w, a.h].into_iter().zip([b.x, b.y, b.w, b.h]) {
		close(a, b);
	}
}

fn same_geometry(a: &LayoutSnapshot, b: &LayoutSnapshot) {
	close(a.height, b.height);
	assert_eq!((a.degraded, a.math_errors), (b.degraded, b.math_errors));
	assert_eq!(a.full_reading_text(), b.full_reading_text());
	assert_eq!(
		a.extract_text(a.select_all(1).unwrap(), 1),
		b.extract_text(b.select_all(1).unwrap(), 1)
	);
	let (a, b) = (a.flattened(), b.flattened());
	assert_eq!(a.blocks.len(), b.blocks.len());
	for (a, b) in a.blocks.iter().zip(&b.blocks) {
		close(a.y, b.y);
		let (a, b) = (&a.layout, &b.layout);
		assert_eq!(a.text.len(), b.text.len());
		for (a, b) in a.text.iter().zip(&b.text) {
			assert_eq!(
				(&a.text, a.separator, a.search_field, &a.search_ranges),
				(&b.text, b.separator, b.search_field, &b.search_ranges)
			);
			assert_eq!(a.clusters.len(), b.clusters.len());
			for (a, b) in a.clusters.iter().zip(&b.clusters) {
				assert_eq!(
					(&a.range, a.command, a.rtl, a.atomic),
					(&b.range, b.command, b.rtl, b.atomic)
				);
				rect(a.rect, b.rect);
			}
		}
		assert_eq!(a.draws.len(), b.draws.len());
		for (a, b) in a.draws.iter().zip(&b.draws) {
			assert_eq!(std::mem::discriminant(a), std::mem::discriminant(b));
			match (a, b) {
				(Draw::Glyph(a), Draw::Glyph(b)) => {
					assert_eq!(
						(a.id, &a.coords, a.paint, a.synthetic_italic),
						(b.id, &b.coords, b.paint, b.synthetic_italic)
					);
					close(a.x, b.x);
					close(a.y, b.y);
					close(a.size, b.size);
				}
				(
					Draw::Box { rect: a, .. }
					| Draw::Rect(a, _)
					| Draw::Image { rect: a, .. },
					Draw::Box { rect: b, .. }
					| Draw::Rect(b, _)
					| Draw::Image { rect: b, .. },
				) => rect(*a, *b),
				(
					Draw::Math { x: ax, y: ay, .. }
					| Draw::Icon { x: ax, y: ay, .. },
					Draw::Math { x: bx, y: by, .. }
					| Draw::Icon { x: bx, y: by, .. },
				) => {
					close(*ax, *bx);
					close(*ay, *by);
				}
				(
					Draw::Polygon {
						center: a,
						points: ap,
						..
					},
					Draw::Polygon {
						center: b,
						points: bp,
						..
					},
				) => {
					close(a[0], b[0]);
					close(a[1], b[1]);
					assert_eq!(ap, bp);
				}
				_ => panic!("unexpected draw"),
			}
		}
		assert_eq!(a.links.len(), b.links.len());
		for (a, b) in a.links.iter().zip(&b.links) {
			assert_eq!((a.command, &a.url), (b.command, &b.url));
			rect(a.rect, b.rect);
		}
		assert_eq!(a.overflow.len(), b.overflow.len());
		for (a, b) in a.overflow.iter().zip(&b.overflow) {
			assert_eq!(a.commands, b.commands);
			rect(a.rect, b.rect);
			close(a.content_width, b.content_width);
		}
		assert_eq!(a.anchors.len(), b.anchors.len());
		for (a, b) in a.anchors.iter().zip(&b.anchors) {
			assert_eq!(a.anchor, b.anchor);
			close(a.y, b.y);
		}
		assert_eq!(a.page_constraints.len(), b.page_constraints.len());
		for (a, b) in a.page_constraints.iter().zip(&b.page_constraints) {
			assert_eq!(
				(a.orphans, a.widows, a.keep_together),
				(b.orphans, b.widows, b.keep_together)
			);
			close(a.top, b.top);
			close(a.bottom, b.bottom);
		}
		assert_eq!(a.inline_decorations.len(), b.inline_decorations.len());
		for ((ac, ar), (bc, br)) in
			a.inline_decorations.iter().zip(&b.inline_decorations)
		{
			assert_eq!(ac, bc);
			close(ar.start, br.start);
			close(ar.end, br.end);
		}
	}
}

#[test]
fn edits_and_insertions_match_fresh_layout_search_and_selection() {
	let source = "<details open>\n<summary>Outer</summary>\n\nChange me.\n\n> Quoted **needle** and $x^2$.\n>\n> Second quoted paragraph.\n\n1. Repeated paragraph.\n\n   Repeated paragraph.\n\n2. Repeated paragraph.\n\n<details>\n<summary>Inner</summary>\n\n## Target\n\nA [needle](https://example.invalid).\n\n| Header | Another |\n|---|---|\n| needle | Cell |\n\n```text\nneedle with a very long line that should overflow the narrow column and scroll horizontally\n```\n\n</details>\n\nReference[^a].\n\n</details>\n\n[^a]: Footnote needle.\n\nFollowing.\n";
	let opts = options();
	let mut engine = LayoutEngine::new();
	engine.layout(&document::parse(source), &opts);
	for replacement in [
		"A longer paragraph that wraps across multiple lines. ".repeat(8),
		"Inserted paragraph.\n\nChange me.".into(),
		"Change me.".into(),
	] {
		let doc = document::parse(source.replace("Change me.", &replacement));
		let mut reused = engine.layout(&doc, &opts);
		let mut fresh = LayoutEngine::new().layout(&doc, &opts);
		for force in [false, true, false] {
			reused.set_disclosures(&BTreeMap::new(), force);
			fresh.set_disclosures(&BTreeMap::new(), force);
			same_geometry(&reused, &fresh);
			for hit in
				SearchIndex::new(&doc).find("needle", SearchOptions::default())
			{
				assert_eq!(
					reused.search_selection(&hit, 1),
					fresh.search_selection(&hit, 1)
				);
				if let Some(selection) = reused.search_selection(&hit, 1) {
					assert_eq!(reused.extract_text(selection, 1), "needle");
				}
			}
		}
	}
}

#[test]
fn inherited_child_styles_resources_and_width_invalidate_nested_geometry() {
	let source = "<details open>\n<summary>Summary</summary>\n\n> First paragraph with enough text to wrap.\n>\n> Second paragraph.\n\n![image](test.png)\n\nFinal paragraph.\n\n</details>\n";
	let mut opts = options();
	let mut sheet = (*opts.stylesheet).clone();
	sheet.merge(&Stylesheet::parse("format_version=2\nversion=1\n[[rule]]\nwhen=['details','first_child']\nsize=1.4\n[[rule]]\nwhen=['blockquote','p','last_child']\nletter_spacing=0.05\n[[rule]]\nwhen=['p']\norphans=2\nwidows=2\n").unwrap());
	opts.stylesheet = Arc::new(sheet);
	let mut engine = LayoutEngine::new();
	let mut images = ImageSnapshot::default();
	for (source, width, size) in [
		(source.to_owned(), 340., (80, 60)),
		(
			source.replace("> First", "Inserted paragraph.\n\n> First"),
			340.,
			(80, 60),
		),
		(source.to_owned(), 240., (120, 160)),
		(source.to_owned(), 240., (120, 80)),
	] {
		opts.width = width;
		images.entries.insert(
			"test.png".into(),
			ImageInfo {
				version: width as u64,
				size: Some(size),
				error: None,
			},
		);
		let doc = document::parse(source);
		let reused = engine.layout_with_images(&doc, &opts, &images);
		let fresh =
			LayoutEngine::new().layout_with_images(&doc, &opts, &images);
		same_geometry(&reused, &fresh);
	}
}
