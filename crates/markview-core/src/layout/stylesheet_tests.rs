use super::*;

#[test]
fn decorations_preserve_text_and_cascade_without_color_reflow() {
	let document = crate::document::parse(
		"# ATTENTION\n\n## Heading\n\n> **First**\n>\n> **Second**\n\n| A | B |\n|---|---|\n| C | D |\n| E | F |\n",
	);
	let source = r##"
format_version = 2
version = 1
[[rule]]
when = ["h1"]
letter_spacing = -0.025
[[rule]]
when = ["h2"]
heading_marker = [1, 1, 0.5]
marker_color = "#123456"
[[rule]]
when = ["blockquote"]
size = 1.0
[[rule]]
when = ["blockquote", "p", "first_child", "strong"]
size = 0.8
[[rule]]
when = ["table", "cell"]
border_edges = [0, 0, 1, 0]
[[rule]]
when = ["table", "cell", "header"]
border_edges = [2, 0, 1, 0]
[[rule]]
when = ["table", "cell", "last_child"]
border_edges = [0, 0, 2, 0]
"##;
	let options = LayoutOptions {
		stylesheet: sheet(source),
		..Default::default()
	};
	let mut engine = LayoutEngine::new();
	let first = engine.layout(&document, &options);
	let selected = first.extract_text(first.select_all(1).unwrap(), 1);
	assert!(selected.contains("ATTENTION") && selected.contains("Second"));
	let mut rules = (*options.stylesheet).clone();
	rules.merge(
		&crate::style::Stylesheet::parse(
			"format_version=2\nversion=1\n[[rule]]\nwhen=['h2']\nmarker_color='#00FF00'",
		)
		.unwrap(),
	);
	let recolored = engine.layout(
		&document,
		&LayoutOptions {
			stylesheet: Arc::new(rules),
			..options.clone()
		},
	);
	assert_eq!(recolored.reused, document.blocks.len());
	assert_eq!(first.height, recolored.height);
	let mut marker = false;
	for draw in &first.blocks[1].layout.draws {
		if let Draw::Rect(
			rect,
			Paint::Scoped(
				_,
				Condition::H2,
				crate::style::ColorField::MarkerColor,
			),
		) = draw
		{
			assert!(rect.w > 0.0 && rect.h > 0.0);
			marker = true;
		}
	}
	assert!(marker);
	let table = &first.blocks[3].layout;
	let edges: Vec<_> = table
		.draws
		.iter()
		.filter_map(|d| match d {
			Draw::Box {
				decoration: Some(d),
				condition: Condition::Cell | Condition::Header,
				..
			} => Some(d.edges),
			_ => None,
		})
		.collect();
	assert_eq!(
		edges,
		vec![
			[2.0, 0.0, 1.0, 0.0],
			[2.0, 0.0, 1.0, 0.0],
			[0.0, 0.0, 1.0, 0.0],
			[0.0, 0.0, 1.0, 0.0],
			[0.0, 0.0, 2.0, 0.0],
			[0.0, 0.0, 2.0, 0.0]
		]
	);
	let quote = &first.blocks[2].layout;
	let letters: Vec<_> = quote
		.draws
		.iter()
		.filter_map(|d| {
			if let Draw::Glyph(g) = d {
				Some(g.size)
			} else {
				None
			}
		})
		.collect();
	assert!(letters.iter().any(|s| (*s - 18.0 * 0.8).abs() < 0.01));
	assert!(letters.iter().any(|s| (*s - 18.0).abs() < 0.01));
	let untracked =
		sheet(&source.replace("letter_spacing = -0.025", "letter_spacing = 0"));
	let normal = engine.layout(
		&document,
		&LayoutOptions {
			stylesheet: untracked,
			..options
		},
	);
	let right = |s: &LayoutSnapshot| {
		s.blocks[0]
			.layout
			.draws
			.iter()
			.filter_map(|d| {
				if let Draw::Glyph(g) = d {
					Some(g.x)
				} else {
					None
				}
			})
			.fold(0.0_f32, f32::max)
	};
	assert!(right(&first) < right(&normal));
}
fn sheet(source: &str) -> Arc<crate::style::Stylesheet> {
	let mut s = (*crate::style::Stylesheet::bundled(false)).clone();
	s.merge(&crate::style::Stylesheet::parse(source).unwrap());
	Arc::new(s)
}
#[test]
fn live_colors_follow_semantics_without_reflow() {
	let doc = crate::document::parse(
		"> *English 中文*\n\n# Heading\n\n***Both*** [*link*](https://example.com)",
	);
	let mut engine = LayoutEngine::new();
	let first = engine.layout(&doc, &LayoutOptions::default());
	let stylesheet = sheet(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['blockquote']\ncolor='#123456'\n[[rule]]\nwhen=['em']\ncolor='#abcdef'\n[[rule]]\nwhen=['em','strong']\ncolor='#654321'\n[[rule]]\nwhen=['link']\ncolor='#102030'",
	);
	let second = engine.layout(
		&doc,
		&LayoutOptions {
			stylesheet: stylesheet.clone(),
			..Default::default()
		},
	);
	assert_eq!(second.reused, doc.blocks.len());
	assert_eq!(first.height, second.height);
	let glyph = second.blocks[0]
		.layout
		.draws
		.iter()
		.find_map(|d| {
			if let Draw::Glyph(g) = d {
				Some(g)
			} else {
				None
			}
		})
		.unwrap();
	assert_eq!(
		stylesheet.paint(glyph.paint),
		crate::style::Color(0xabcdefff).rgba()
	);
	let last = &second.blocks.last().unwrap().layout;
	let colors: Vec<_> = last
		.draws
		.iter()
		.filter_map(|d| {
			if let Draw::Glyph(g) = d {
				Some(stylesheet.paint(g.paint))
			} else {
				None
			}
		})
		.collect();
	assert!(colors.contains(&crate::style::Color(0x654321ff).rgba()));
	assert!(colors.contains(&crate::style::Color(0x102030ff).rgba()));
}
#[test]
fn geometry_changes_invalidate_cache_and_keep_reading_text() {
	let doc = crate::document::parse(
		"# Heading\n\nText\n\n- item\n\n| A | B |\n|---|---|\n| C | D |",
	);
	let mut engine = LayoutEngine::new();
	let first = engine.layout(&doc, &LayoutOptions::default());
	let options = LayoutOptions {
		stylesheet: sheet(
			"format_version=2\nversion=1\n[[rule]]\nwhen=['body']\npadding=1.0\n[[rule]]\nwhen=['h1']\nsize=2.5\n[[rule]]\nwhen=['p']\nline_height=2.0\n[[rule]]\nwhen=['list_item']\npadding=0.5\n[[rule]]\nwhen=['table','cell']\npadding=1.0",
		),
		..Default::default()
	};
	let second = engine.layout(&doc, &options);
	assert_eq!(second.reused, 0);
	assert!(second.height > first.height);
	assert_eq!(
		first.extract_text(first.select_all(1).unwrap(), 1),
		second.extract_text(second.select_all(1).unwrap(), 1)
	);
	assert!(
		second.blocks[0]
			.layout
			.draws
			.iter()
			.filter_map(|d| if let Draw::Glyph(g) = d {
				Some(g.size)
			} else {
				None
			})
			.all(|size| size == 45.)
	);
	assert!(second.blocks[2].layout.draws.iter().any(|d| matches!(
		d,
		Draw::Box {
			condition: Condition::ListItem,
			..
		}
	)));
}

#[test]
fn compound_rules_reach_the_layout_chain() {
	let doc = crate::document::parse("# 标题 `code`\n\n正文 `code`\n");
	let stylesheet = sheet(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['code']\nbackground='#111111'\n[[rule]]\nwhen=['h1','code']\nbackground='#222222'",
	);
	let snapshot = LayoutEngine::new().layout(
		&doc,
		&LayoutOptions {
			stylesheet: stylesheet.clone(),
			..Default::default()
		},
	);
	let backgrounds: Vec<_> = snapshot
		.blocks
		.iter()
		.flat_map(|b| b.layout.draws.iter())
		.filter_map(|d| match d {
			Draw::Rect(_, paint) => Some(stylesheet.paint(*paint)),
			_ => None,
		})
		.collect();
	// The heading's code picks up the compound; the paragraph's does not.
	assert!(
		backgrounds.contains(&crate::style::Color(0x222222ff).rgba()),
		"{backgrounds:?}"
	);
	assert!(
		backgrounds.contains(&crate::style::Color(0x111111ff).rgba()),
		"{backgrounds:?}"
	);
}

#[test]
fn code_labels_and_list_markers_keep_their_own_size() {
	use crate::style::{Condition, chain_of};
	let options = LayoutOptions::default();
	let sheet = options.stylesheet.clone();
	let sizes = |source: &str| -> Vec<f32> {
		let doc = crate::document::parse(source);
		LayoutEngine::new()
			.layout(&doc, &options)
			.blocks
			.iter()
			.flat_map(|b| b.layout.draws.iter())
			.filter_map(|d| match d {
				Draw::Glyph(g) => Some(g.size),
				_ => None,
			})
			.collect()
	};
	let label = sheet.element_rule(
		chain_of(&[Condition::Body, Condition::CodeBlock, Condition::Label]),
		Condition::Label,
	);
	let expected = options.font_size * label.size.unwrap();
	let glyphs = sizes("```rust\nfn main() {}\n```\n");
	assert!(
		glyphs.iter().any(|s| (s - expected).abs() < 0.01),
		"label size {expected} missing from {glyphs:?}"
	);
	let marker = sheet.element_rule(
		chain_of(&[
			Condition::Body,
			Condition::List,
			Condition::ListItem,
			Condition::Marker,
		]),
		Condition::Marker,
	);
	let expected = options.font_size * marker.size.unwrap();
	let glyphs = sizes("1. item\n");
	assert!(
		glyphs.iter().any(|s| (s - expected).abs() < 0.01),
		"marker size {expected} missing from {glyphs:?}"
	);
}

#[test]
fn box_colors_follow_the_same_context_as_geometry() {
	use crate::style::ColorField;
	let doc = crate::document::parse("> quoted\n");
	let stylesheet = sheet(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['p']\nbackground='#111111'\n[[rule]]\nwhen=['blockquote','p']\nbackground='#123456'",
	);
	let snapshot = LayoutEngine::new().layout(
		&doc,
		&LayoutOptions {
			stylesheet: stylesheet.clone(),
			..Default::default()
		},
	);
	let boxes: Vec<_> = snapshot
		.blocks
		.iter()
		.flat_map(|b| b.layout.draws.iter())
		.filter_map(|d| match d {
			Draw::Box {
				chain, condition, ..
			} => Some(stylesheet.paint(Paint::Scoped(
				*chain,
				*condition,
				ColorField::Background,
			))),
			_ => None,
		})
		.collect();
	assert!(
		boxes.contains(&crate::style::Color(0x123456ff).rgba()),
		"{boxes:?}"
	);
	assert!(
		!boxes.contains(&crate::style::Color(0x111111ff).rgba()),
		"{boxes:?}"
	);
}
