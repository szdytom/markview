use super::*;
use crate::document;
use crate::document::{Inline, InlineKind, TextStyle};

#[test]
fn tables_are_truncated_to_the_configured_limits() {
	let mut source =
		String::from("| a | b | c | d | e | f |\n|---|---|---|---|---|---|\n");
	for i in 0..20 {
		source.push_str(&format!("| {i} | x | x | x | x | x |\n"));
	}
	let doc = document::parse(source);
	let small = LayoutOptions {
		limits: crate::limits::Limits {
			table_columns: 2,
			table_rows: 3,
			table_cells: 4,
			..Default::default()
		},
		..Default::default()
	};
	let truncated = LayoutEngine::new().layout(&doc, &small);
	let full = LayoutEngine::new().layout(&doc, &LayoutOptions::default());
	assert!(truncated.height.is_finite());
	assert!(truncated.height > 0.0);
	assert!(
		truncated.height < full.height,
		"truncated {} vs full {}",
		truncated.height,
		full.height
	);
}

#[test]
fn progressive_prefixes_share_final_geometry_and_can_be_cancelled() {
	let doc = document::parse(
		"A paragraph with **bold**, 中文 and $x^2$.\n\n".repeat(40),
	);
	let options = LayoutOptions::default();
	let mut engine = LayoutEngine::new();
	let mut prefix = None;
	let final_layout = engine
		.layout_progressive(&doc, &options, &Default::default(), |p| {
			if p.blocks.len() == 3 {
				prefix = Some(p.clone());
			}
			true
		})
		.unwrap();
	let prefix = prefix.unwrap();
	for (a, b) in prefix.blocks.iter().zip(&final_layout.blocks) {
		assert_eq!(a.y, b.y);
		assert!(Arc::ptr_eq(&a.layout, &b.layout));
	}
	let full = LayoutEngine::new().layout(&doc, &options);
	assert_eq!(full.height, final_layout.height);
	assert!(full.same_reading_text(&final_layout));
	assert_eq!(full.blocks.len(), final_layout.blocks.len());
	for (a, b) in full.blocks.iter().zip(&final_layout.blocks) {
		assert_eq!(
			(a.y, a.layout.height, a.layout.draws.len()),
			(b.y, b.layout.height, b.layout.draws.len())
		);
	}
	let mut visited = 0;
	assert!(
		engine
			.layout_progressive(&doc, &options, &Default::default(), |p| {
				visited = p.blocks.len();
				visited < 3
			})
			.is_none()
	);
	assert_eq!(visited, 3);
	assert!(engine.layout(&doc, &options).same_reading_text(&full));
}
#[test]
fn links_are_hit_testable_and_survive_reuse() {
	let d = document::parse(
		"See [the manual](https://example.com/manual) and [mail](mailto:a@b.example).\n",
	);
	let mut engine = LayoutEngine::new();
	let opts = LayoutOptions {
		width: 400.0,
		..Default::default()
	};
	let snapshot = engine.layout(&d, &opts);
	let block = &snapshot.blocks[0];
	assert_eq!(block.layout.links.len(), 2);
	assert_eq!(block.layout.links[0].url, "https://example.com/manual");
	assert_eq!(block.layout.links[1].url, "mailto:a@b.example");
	let hit = block.layout.links[0].rect;
	let none = HashMap::new();
	assert_eq!(
		snapshot.link_at(hit.x + 1.0, block.y + hit.y + 1.0, &none),
		Some("https://example.com/manual")
	);
	assert_eq!(
		snapshot.link_at(hit.x - 6.0, block.y + hit.y + 1.0, &none),
		None
	);
	assert_eq!(snapshot.link_at(hit.x + 1.0, block.y - 1.0, &none), None);
	let again = engine.layout(&d, &opts);
	assert_eq!(again.reused, 1);
	assert_eq!(again.blocks[0].layout.links.len(), 2);
}
#[test]
fn heading_anchors_resolve_to_layout_positions() {
	let mut engine = LayoutEngine::new();
	let opts = LayoutOptions::default();
	let doc =
		document::parse("# First\n\nParagraph.\n\n> ## Nested\n\n# First\n");
	let snapshot = engine.layout(&doc, &opts);
	let first = snapshot.anchor_y("first").unwrap();
	let nested = snapshot.anchor_y("nested").unwrap();
	let repeat = snapshot.anchor_y("first-1").unwrap();
	assert!(first < nested && nested < repeat);
	assert!(snapshot.anchor_y("missing").is_none());
	// The nested heading's anchor belongs to the quote that contains it.
	let quote = &snapshot.blocks[2];
	assert!((quote.y..quote.y + quote.layout.height).contains(&nested));
	assert!(quote.layout.anchors.iter().any(|a| a.anchor == "nested"));
	// Reused geometry keeps its anchors.
	let again = engine.layout(&doc, &opts);
	assert_eq!(again.reused, snapshot.blocks.len());
	assert_eq!(again.anchor_y("nested"), Some(nested));
}
#[test]
fn footnote_links_reach_the_note_and_its_number_returns() {
	let mut engine = LayoutEngine::new();
	let opts = LayoutOptions {
		width: 400.0,
		..Default::default()
	};
	let doc = document::parse(
		"First[^a], again[^a], and another[^b].\n\n\
		 [^a]: Alpha note.\n\n\
		 [^b]: Beta note.\n",
	);
	let snapshot = engine.layout(&doc, &opts);
	let first_ref = snapshot.anchor_y("fnref:1").unwrap();
	let note = snapshot.anchor_y("fn:1").unwrap();
	let second_note = snapshot.anchor_y("fn:2").unwrap();
	// The fallback return goes to the first reference, and the notes follow
	// the paragraph that cites them.
	assert!(first_ref < note && note < second_note);
	let links: Vec<&str> = snapshot
		.blocks
		.iter()
		.flat_map(|b| b.layout.links.iter().map(|l| l.url.as_str()))
		.collect();
	assert_eq!(links.iter().filter(|u| **u == "#fn:1").count(), 2);
	assert!(links.contains(&"#fn:2"));
	assert!(links.contains(&"#fnback:1"));
	assert!(links.contains(&"#fnback:2"));
	// Both the reference and the note's number are hit-testable.
	let empty = HashMap::new();
	let hit = |url: &str| {
		snapshot.blocks.iter().enumerate().find_map(|(bi, b)| {
			let link = b.layout.links.iter().find(|l| l.url == url)?;
			let (offset, _) = b.layout.command_view(link.command, bi, &empty);
			Some((
				link.rect.x - offset + link.rect.w * 0.5,
				b.y + link.rect.y + link.rect.h * 0.5,
			))
		})
	};
	let (x, y) = hit("#fn:1").unwrap();
	assert_eq!(snapshot.link_at(x, y, &empty), Some("#fn:1"));
	let (x, y) = hit("#fnback:1").unwrap();
	assert_eq!(snapshot.link_at(x, y, &empty), Some("#fnback:1"));
}
#[test]
fn a_footnote_body_keeps_the_full_column() {
	let mut engine = LayoutEngine::new();
	let doc = document::parse(
		"Text[^a].\n\n[^a]: A note whose body is long enough to wrap across \
		 the full width of the reading column instead of one word per line.\n",
	);
	let snapshot = engine.layout(
		&doc,
		&LayoutOptions {
			width: 400.0,
			..Default::default()
		},
	);
	// The note's label is only a few pixels wide, so measuring the label must
	// not narrow the body that follows it.
	let right = snapshot.blocks[1]
		.layout
		.text
		.iter()
		.flat_map(|node| &node.clusters)
		.map(|c| c.rect.x + c.rect.w)
		.fold(0.0, f32::max);
	assert!(right > 300.0, "the note body only reached x={right}");
	assert_eq!(snapshot.degraded, 0);
}
#[test]
fn a_footnote_number_is_set_like_the_note_body() {
	let mut engine = LayoutEngine::new();
	let doc = document::parse("Text[^a].\n\n[^a]: 字体由系统提供。\n");
	let snapshot = engine.layout(
		&doc,
		&LayoutOptions {
			width: 400.0,
			..Default::default()
		},
	);
	let glyphs = &snapshot.blocks[1].layout.draws;
	let body = glyphs
		.iter()
		.find_map(|d| match d {
			Draw::Glyph(g) => Some(g),
			_ => None,
		})
		.expect("the note body");
	// The number is drawn after the body it leads, but hangs to its left.
	let number = glyphs
		.iter()
		.find_map(|d| match d {
			Draw::Glyph(g) if g.x < body.x => Some(g),
			_ => None,
		})
		.expect("the note's number");
	// It is set at the body's own size and shares the body's first baseline;
	// only an in-text reference is a superscript.
	assert_eq!(number.size, body.size);
	assert!((number.y - body.y).abs() < 0.01);
}
#[test]
fn wrapped_links_produce_one_rect_per_line() {
	let d = document::parse(
		"[an intentionally long linked phrase that wraps](https://example.com)\n",
	);
	let mut engine = LayoutEngine::new();
	let opts = LayoutOptions {
		width: 120.0,
		..Default::default()
	};
	let snapshot = engine.layout(&d, &opts);
	let links = &snapshot.blocks[0].layout.links;
	assert!(links.len() > 1, "expected a wrapped link, got {links:?}");
	assert!(links.iter().all(|l| l.url == "https://example.com"));
	assert!(links.windows(2).all(|w| w[0].rect.y < w[1].rect.y));
}
#[test]
fn long_labels_are_trimmed_to_fit() {
	let mut engine = LayoutEngine::new();
	let short = "https://example.com";
	assert_eq!(engine.fit(short, 11.0, 500.0), short);
	let long = "https://example.com/a/very/long/path/that/keeps/going?with=query&more=1";
	let fitted = engine.fit(long, 11.0, 160.0);
	assert!(engine.text_width(&fitted, 11.0) <= 160.0);
	let (head, tail) = fitted.split_once('…').expect("ellipsis");
	assert!(long.starts_with(head) && long.ends_with(tail));
	assert!(!head.is_empty() && !tail.is_empty());
	assert!(fitted.chars().count() < long.chars().count());
}
#[test]
fn mixed_layout_is_finite_and_reused() {
	let mut engine = LayoutEngine::new();
	let d = document::parse(
		"中文标点（不应落在错误的位置），以及 **English typography** 与 $\\frac{x_1}{y}$ 混排。\n\nSecond paragraph.\n",
	);
	let opts = LayoutOptions {
		width: 280.0,
		..Default::default()
	};
	let a = engine.layout(&d, &opts);
	assert!(a.height.is_finite() && a.height > 50.0);
	assert_eq!(a.math_errors, 0);
	assert!(
		a.blocks[0]
			.layout
			.draws
			.iter()
			.any(|d| matches!(d, Draw::Math { .. }))
	);
	let b = engine.layout(&d, &opts);
	assert_eq!(b.reused, 2);
	let c = engine.layout(
		&d,
		&LayoutOptions {
			width: 400.0,
			..opts
		},
	);
	assert_eq!(c.reused, 0);
}
#[test]
fn math_errors_are_visible_and_copyable_when_enabled() {
	let doc = document::parse("$$S_2^\\*$$");
	let mut engine = LayoutEngine::new();
	let shown = engine.layout(&doc, &LayoutOptions::default());
	let selected = shown.select_all(1).unwrap();
	assert_eq!(shown.math_errors, 1);
	assert!(
		shown
			.extract_text(selected, 1)
			.contains("Undefined control sequence: \\*")
	);

	let mut stylesheet = (*crate::style::Stylesheet::bundled(false)).clone();
	stylesheet.merge(
		&crate::style::Stylesheet::parse(
			"format_version=2\nversion=1\n[[rule]]\nwhen=['error']\nshow=false",
		)
		.unwrap(),
	);
	let hidden = engine.layout(
		&doc,
		&LayoutOptions {
			stylesheet: Arc::new(stylesheet),
			..Default::default()
		},
	);
	assert_eq!(hidden.math_errors, 1);
	assert!(
		!hidden
			.extract_text(hidden.select_all(1).unwrap(), 1)
			.contains("Undefined control sequence")
	);
}
#[test]
fn cjk_boundaries_and_hyphenation() {
	let mut e = LayoutEngine::new();
	let mut out = BlockLayout::default();
	let rich = vec![Inline {
		kind: InlineKind::Text("（中文），排版。 extraordinary".into()),
		style: TextStyle::default(),
		source: 0..0,
	}];
	let images = Default::default();
	let mut context = BlockContext {
		shaper: &mut e.shaper,
		math: &mut e.math,
		images: &images,
		highlight_cache: e.highlights.results(),
	};
	let p = context.prepare(&rich, 18.0, &mut out);
	let units = context.units(&p, 18.0, false, true, 760.0, Default::default());
	for u in &units {
		if u.after.is_some() && u.source.end < p.text.len() {
			assert!(
				!"），。"
					.contains(p.text[u.source.end..].chars().next().unwrap())
			);
			assert_ne!(&p.text[u.source.clone()], "（");
		}
	}
	assert!(
		units
			.iter()
			.any(|u| u.after.is_some_and(|b| b.hyphen_width > 0.0))
	);
}
#[test]
fn content_cache_survives_offsets_but_not_changed_references() {
	let mut e = LayoutEngine::new();
	let opts = LayoutOptions::default();
	let a = document::parse("A [link][id].\n\n[id]: https://one.example\n");
	e.layout(&a, &opts);
	let b = document::parse(
		"Inserted paragraph.\n\nA [link][id].\n\n[id]: https://one.example\n",
	);
	assert_eq!(e.layout(&b, &opts).reused, 1);
	let c = document::parse(
		"Inserted paragraph.\n\nA [link][id].\n\n[id]: https://two.example\n",
	);
	assert_eq!(e.layout(&c, &opts).reused, 1); // Only the inserted paragraph.
}
#[test]
fn anchor_follows_content_and_only_follows_bottom_when_requested() {
	fn snapshot(ids: &[u64]) -> LayoutSnapshot {
		LayoutSnapshot {
			height: ids.len() as f32 * 120.0,
			blocks: ids
				.iter()
				.enumerate()
				.map(|(i, &id)| PlacedBlock {
					id,
					source: 0..0,
					y: i as f32 * 120.0,
					layout: Arc::new(BlockLayout {
						height: 120.0,
						..Default::default()
					}),
				})
				.collect(),
			..Default::default()
		}
	}
	let old = snapshot(&[1, 2, 3, 4, 5, 6]);
	let new = snapshot(&[0, 1, 2, 3, 4, 5, 6]);
	assert_eq!(anchored_scroll(&old, &new, 310.0, 200.0, true), 430.0);
	let appended = snapshot(&[1, 2, 3, 4, 5, 6, 7]);
	assert_eq!(anchored_scroll(&old, &appended, 520.0, 200.0, true), 640.0);
	assert_eq!(anchored_scroll(&old, &appended, 520.0, 200.0, false), 520.0);
}
#[test]
fn wide_blocks_are_scrollable_and_formulas_grow_line_height() {
	let mut e = LayoutEngine::new();
	let opts = LayoutOptions {
		width: 260.0,
		..Default::default()
	};
	let d = document::parse(
		"```\n01234567890123456789012345678901234567890123456789012345678901234567890\n```\n\n| Long column | Another column |\n|--|--|\n| aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa | bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb |\n",
	);
	let layout = e.layout(&d, &opts);
	assert!(layout.blocks.iter().all(|b| !b.layout.overflow.is_empty()));
	let text = e.layout(&document::parse("Plain text."), &opts);
	let math = e.layout(
		&document::parse(
			"Before $\\dfrac{\\dfrac{a}{b}}{\\dfrac{c}{d}}$ after.",
		),
		&opts,
	);
	assert!(math.height > text.height);
	assert_eq!(math.math_errors, 0);
}
#[test]
fn code_blocks_hard_wrap_at_the_column_when_asked() {
	let mut e = LayoutEngine::new();
	let d = document::parse(
		"```\n01234567890123456789012345678901234567890123456789012345678901234567890\n```\n",
	);
	let base = LayoutOptions {
		width: 260.0,
		..Default::default()
	};
	let unwrapped = e.layout(&d, &base);
	assert!(!unwrapped.blocks[0].layout.overflow.is_empty());
	let wrapped = e.layout(
		&d,
		&LayoutOptions {
			codeblock_wrap: true,
			..base
		},
	);
	let block = &wrapped.blocks[0].layout;
	assert!(block.overflow.is_empty());
	let clusters = &block.text[0].clusters;
	let mut rows: Vec<f32> = clusters.iter().map(|c| c.rect.y).collect();
	rows.sort_by(f32::total_cmp);
	rows.dedup_by(|a, b| (*a - *b).abs() < 0.5);
	assert!(rows.len() > 1, "code did not wrap: {rows:?}");
	assert!(clusters.iter().all(|c| c.rect.x + c.rect.w <= 260.01));
	assert!(wrapped.height > unwrapped.height);
}
#[test]
fn overflowing_blocks_reserve_the_configured_scrollbar_gutter() {
	let mut e = LayoutEngine::new();
	let d = document::parse(
		"```\n01234567890123456789012345678901234567890123456789012345678901234567890\n```\n",
	);
	let opts = LayoutOptions {
		width: 260.0,
		..Default::default()
	};
	let base = e.layout(&d, &opts);
	let bundled = crate::style::Stylesheet::bundled(false).scrollbar_gutter();
	assert_eq!(base.blocks[0].layout.overflow[0].gutter, bundled);
	// A wider gutter both reserves more space and grows the block.
	let mut sheet = (*crate::style::Stylesheet::bundled(false)).clone();
	sheet.merge(
		&crate::style::Stylesheet::parse(
			"format_version=2\nversion=1\n[[rule]]\nwhen=['scrollbar']\ngutter=30.0",
		)
		.unwrap(),
	);
	let taller = e.layout(
		&d,
		&LayoutOptions {
			width: 260.0,
			stylesheet: Arc::new(sheet),
			..Default::default()
		},
	);
	assert_eq!(taller.blocks[0].layout.overflow[0].gutter, 30.0);
	let delta = taller.blocks[0].layout.height - base.blocks[0].layout.height;
	assert!((delta - (30.0 - bundled)).abs() < 0.01, "{delta}");
}
#[test]
fn indent_applies_to_text_leading_paragraphs_and_whole_lists() {
	fn first_x(snapshot: &LayoutSnapshot, block: usize, node: usize) -> f32 {
		snapshot.blocks[block].layout.text[node].clusters[0].rect.x
	}
	fn second_line_x(
		snapshot: &LayoutSnapshot,
		block: usize,
		node: usize,
	) -> f32 {
		let clusters = &snapshot.blocks[block].layout.text[node].clusters;
		let first = clusters[0].rect.y;
		clusters
			.iter()
			.find(|c| (c.rect.y - first).abs() > 1.0)
			.expect("wrapped line")
			.rect
			.x
	}
	fn image_x(snapshot: &LayoutSnapshot, block: usize) -> f32 {
		snapshot.blocks[block]
			.layout
			.draws
			.iter()
			.find_map(|d| match d {
				Draw::Image { rect, .. } => Some(rect.x),
				_ => None,
			})
			.expect("image draw")
	}
	fn math_x(snapshot: &LayoutSnapshot, block: usize) -> f32 {
		snapshot.blocks[block]
			.layout
			.draws
			.iter()
			.find_map(|d| match d {
				Draw::Math { x, .. } => Some(*x),
				_ => None,
			})
			.expect("math draw")
	}
	let mut e = LayoutEngine::new();
	let doc = document::parse(
		"Alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi.\n\n\
		 ![missing image](absent.png)\n\n\
		 > Quoted paragraph text.\n\n\
		 - bullet item text that is long enough to wrap onto a second line\n\n\
		 1. ordered item text that is long enough to wrap onto a second line\n\n\
		 $$\\frac{a}{b}$$\n\n\
		 Reference[^1].\n\n\
		 [^1]: Footnote body text.\n",
	);
	let width = 320.0;
	let plain = e.layout(
		&doc,
		&LayoutOptions {
			width,
			..Default::default()
		},
	);
	let one = e.layout(
		&doc,
		&LayoutOptions {
			width,
			paragraph_indent: 1.0,
			..Default::default()
		},
	);
	let two = e.layout(
		&doc,
		&LayoutOptions {
			width,
			paragraph_indent: 2.0,
			..Default::default()
		},
	);
	let d1 = first_x(&one, 0, 0) - first_x(&plain, 0, 0);
	let d2 = first_x(&two, 0, 0) - first_x(&plain, 0, 0);
	assert!(d1 > 1.0, "expected an indent, got {d1}");
	assert!((d2 - 2.0 * d1).abs() < 0.05, "{d1} {d2}");
	// Wrapped lines keep the full measure, so the indent only opens the line.
	assert!((second_line_x(&two, 0, 0) - first_x(&two, 0, 0) + d2).abs() < 0.6);
	// A leading image or display formula keeps its own margin.
	assert_eq!(image_x(&plain, 1), image_x(&two, 1));
	assert_eq!(math_x(&plain, 5), math_x(&two, 5));
	// A quoted paragraph is still prose and gains the indent.
	assert!(first_x(&two, 2, 0) > first_x(&plain, 2, 0));
	// A list indents as a whole: marker and item text move together, and the
	// item's opening and wrapped lines share one margin.
	for block in [3, 4] {
		let marker = |s: &LayoutSnapshot| first_x(s, block, 0);
		let text = |s: &LayoutSnapshot| first_x(s, block, 1);
		assert!((marker(&two) - marker(&plain) - d2).abs() < 0.6);
		assert!((text(&two) - text(&plain) - d2).abs() < 0.6);
		assert!((second_line_x(&two, block, 1) - text(&two)).abs() < 0.6);
	}
	// A footnote stays flush behind its own label.
	assert_eq!(first_x(&plain, 7, 0), first_x(&two, 7, 0));
	// The indent is part of the block cache identity.
	let again = e.layout(
		&doc,
		&LayoutOptions {
			width,
			paragraph_indent: 2.0,
			..Default::default()
		},
	);
	assert_eq!(again.reused, doc.blocks.len());
	let changed = e.layout(
		&doc,
		&LayoutOptions {
			width,
			paragraph_indent: 3.0,
			..Default::default()
		},
	);
	assert_eq!(changed.reused, 0);
}

#[test]
fn a_theme_can_inset_bullet_and_ordered_lists_separately() {
	fn markers(
		sheet: &Arc<crate::style::Stylesheet>,
		indent: f32,
	) -> (f32, f32) {
		let doc = document::parse("- bullet item\n\n1. ordered item\n");
		let mut e = LayoutEngine::new();
		let s = e.layout(
			&doc,
			&LayoutOptions {
				width: 400.0,
				paragraph_indent: indent,
				stylesheet: sheet.clone(),
				..Default::default()
			},
		);
		let x =
			|block: usize| s.blocks[block].layout.text[0].clusters[0].rect.x;
		(x(0), x(1))
	}
	let base = Arc::new(
		crate::style::Stylesheet::parse("format_version=2\nversion=1").unwrap(),
	);
	let theme = Arc::new(
		crate::style::Stylesheet::parse(
			"format_version=2\nversion=1\n[[rule]]\nwhen=['list']\nindent=0.5\n[[rule]]\nwhen=['enum']\nindent=1.5",
		)
		.unwrap(),
	);
	let (flat_bullet, flat_ordered) = markers(&base, 0.0);
	let (themed_bullet, themed_ordered) = markers(&theme, 0.0);
	assert!((themed_bullet - flat_bullet - 0.5 * 18.0).abs() < 0.6);
	assert!((themed_ordered - flat_ordered - 1.5 * 18.0).abs() < 0.6);
	// The roles are independent: `[list]` alone leaves ordered lists flush.
	let bullets = Arc::new(
		crate::style::Stylesheet::parse(
			"format_version=2\nversion=1\n[[rule]]\nwhen=['list']\nindent=1.0",
		)
		.unwrap(),
	);
	let (bullets_bullet, bullets_ordered) = markers(&bullets, 0.0);
	assert!((bullets_bullet - flat_bullet - 18.0).abs() < 0.6);
	assert!((bullets_ordered - flat_ordered).abs() < 0.6);
	// The reader's paragraph indent stacks on the theme inset.
	let (both_bullet, both_ordered) = markers(&theme, 2.0);
	assert!((both_bullet - flat_bullet - 2.5 * 18.0).abs() < 0.6);
	assert!((both_ordered - flat_ordered - 3.5 * 18.0).abs() < 0.6);
}

#[test]
fn benchmark_corpus_needs_no_emergency_greedy_fallback() {
	let mut e = LayoutEngine::new();
	for source in [
		include_str!("../../../../tests/fixtures/ordinary-10k.md"),
		include_str!("../../../../tests/fixtures/math-10k.md"),
	] {
		assert_eq!(source.len(), 10240);
		let d = document::parse(source);
		for width in [350.0, 760.0] {
			let s = e.layout(
				&d,
				&LayoutOptions {
					width,
					..Default::default()
				},
			);
			assert_eq!(s.math_errors, 0);
			assert_eq!(s.degraded, 0);
			assert!(s.blocks.iter().all(|b| b.layout.overflow.is_empty()));
		}
	}
}

#[test]
fn inline_code_chip_covers_justified_spaces() {
	// A stretched space inside inline code must not leave a hole in its chip.
	let doc = document::parse(
		"如果你在推送前执行了 **`git fetch`**，你的检查就会通过。\n",
	);
	for width in [430.0, 450.0, 470.0] {
		let opts = LayoutOptions {
			width,
			..Default::default()
		};
		let snapshot = LayoutEngine::new().layout(&doc, &opts);
		let mut chips: Vec<(f32, f32, f32)> = snapshot.blocks[0]
			.layout
			.draws
			.iter()
			.filter_map(|d| match d {
				crate::scene::Draw::Rect(
					rect,
					crate::scene::Paint::Cascade(
						_,
						crate::style::ColorField::Background,
					),
				) => Some((rect.y, rect.x, rect.w)),
				_ => None,
			})
			.collect();
		assert!(chips.len() >= 9, "width={width}: {chips:?}");
		chips.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
		for pair in chips.windows(2) {
			let (y0, x0, w0) = pair[0];
			let (y1, x1, _) = pair[1];
			if (y0 - y1).abs() < 0.01 {
				assert!(
					(x1 - (x0 + w0)).abs() < 0.01,
					"width={width}: chip seam at {x0}+{w0}"
				);
			}
		}
	}
}

/// The drawn clusters of the first block, grouped into lines by their vertical
/// position and ordered left to right.
fn drawn_lines(snapshot: &LayoutSnapshot) -> Vec<Vec<(String, Rect)>> {
	let mut rows: Vec<Vec<(String, Rect)>> = Vec::new();
	for node in &snapshot.blocks[0].layout.text {
		for cluster in &node.clusters {
			let text = node.text[cluster.range.clone()].to_string();
			let row = rows.iter_mut().find(|row| {
				row.first()
					.is_some_and(|(_, r)| (r.y - cluster.rect.y).abs() < 0.5)
			});
			match row {
				Some(row) => row.push((text, cluster.rect)),
				None => rows.push(vec![(text, cluster.rect)]),
			}
		}
	}
	for row in &mut rows {
		row.sort_by(|a, b| a.1.x.total_cmp(&b.1.x));
	}
	rows
}

#[test]
fn cjk_punctuation_gives_back_its_blank_half_at_a_line_edge() {
	// The full stop closes a wrapped line and the paragraph, and also appears
	// mid-line, so the two can be compared directly.
	// One line, so the first full stop sits inside it and the second one closes
	// it, and the two can be compared directly.
	let source = "甲。乙丙丁戊。\n";
	let doc = document::parse(source);
	let opts = LayoutOptions {
		width: 400.0,
		justify: false,
		..Default::default()
	};
	let snapshot = LayoutEngine::new().layout(&doc, &opts);
	let rows = drawn_lines(&snapshot);
	let mut edge = Vec::new();
	let mut middle = Vec::new();
	for row in &rows {
		for (i, (text, rect)) in row.iter().enumerate() {
			if text == "。" {
				if i + 1 == row.len() {
					edge.push(rect.w);
				} else {
					middle.push(rect.w);
				}
			}
		}
	}
	assert_eq!(rows.len(), 1, "{rows:?}");
	assert_eq!(edge.len(), 1, "{rows:?}");
	assert_eq!(middle.len(), 1, "{rows:?}");
	assert!(
		(edge[0] * 2.0 - middle[0]).abs() < 0.01,
		"edge {} against the full {}",
		edge[0],
		middle[0]
	);
}

#[test]
fn han_next_to_latin_gains_a_quarter_em() {
	let size = 18.0;
	let doc = document::parse("汉字abc汉字\n");
	let opts = LayoutOptions {
		width: 400.0,
		font_size: size,
		justify: false,
		..Default::default()
	};
	let snapshot = LayoutEngine::new().layout(&doc, &opts);
	let rows = drawn_lines(&snapshot);
	let line: &Vec<(String, f32)> = &rows[0]
		.iter()
		.map(|(text, rect)| (text.clone(), rect.w))
		.collect();
	let text: String = line.iter().map(|(t, _)| t.as_str()).collect();
	assert_eq!(text, "汉字abc汉字");
	let plain = line[0].1;
	let gap = size * 0.25;
	// Only the two clusters that face a Latin letter widen.
	assert!((line[1].1 - plain - gap).abs() < 0.01, "{line:?}");
	assert!((line[5].1 - plain - gap).abs() < 0.01, "{line:?}");
	assert!((line[6].1 - plain).abs() < 0.01, "{line:?}");
}

#[test]
fn a_justified_cjk_line_reaches_the_measure() {
	let doc = document::parse(
		"这是一段用于测试中文排版效果的文字，它应当填满整行并且每个字之间的\
		间距都保持均匀，标点也应当正确处理。\n",
	);
	let width = 300.0;
	let opts = LayoutOptions {
		width,
		..Default::default()
	};
	let snapshot = LayoutEngine::new().layout(&doc, &opts);
	let rows = drawn_lines(&snapshot);
	assert!(rows.len() > 2, "{rows:?}");
	// Only the paragraph's last line is allowed to fall short.
	for row in &rows[..rows.len() - 1] {
		let (_, last) = row.last().unwrap();
		let right = last.x + last.w;
		assert!((right - width).abs() < 1.0, "{right} in {row:?}");
	}
	assert!(snapshot.blocks[0].layout.overflow.is_empty());
}

#[test]
fn a_line_opening_punctuation_hangs_left() {
	let doc = document::parse(
		"（中文）测试行首右对齐标点的悬挂效果，再补一些字凑长度让它换行。\n",
	);
	let opts = LayoutOptions {
		width: 200.0,
		..Default::default()
	};
	let snapshot = LayoutEngine::new().layout(&doc, &opts);
	let block = &snapshot.blocks[0].layout;
	let hang = block.draws.iter().find_map(|draw| match draw {
		crate::scene::Draw::Glyph(g) if g.x < -0.01 => Some(g.x),
		_ => None,
	});
	assert!(hang.is_some_and(|x| x < -1.0), "nothing hangs: {hang:?}");
	assert!(block.overflow.is_empty());
}

/// Lay out one paragraph and return its drawn lines.
fn lines_of(
	source: &str,
	width: f32,
	justify: bool,
) -> Vec<Vec<(String, Rect)>> {
	let doc = document::parse(source);
	let opts = LayoutOptions {
		width,
		justify,
		..Default::default()
	};
	drawn_lines(&LayoutEngine::new().layout(&doc, &opts))
}

/// The widths of the first line, in order.
fn widths_of(source: &str, width: f32, justify: bool) -> Vec<f32> {
	lines_of(source, width, justify)[0]
		.iter()
		.map(|(_, rect)| rect.w)
		.collect()
}

/// The right edge of a line.
fn right_of(line: &[(String, Rect)]) -> f32 {
	let (_, rect) = line.last().unwrap();
	rect.x + rect.w
}

// The cases below are borrowed from Typst's inline layout suite, where each one
// is a reference image. Here each asserts a geometric invariant instead, so it
// holds whatever font the machine happens to provide.

#[test]
fn typst_cjk_latin_spacing_covers_digits_and_skips_punctuation() {
	// `tests/suite/layout/inline/cjk.typ`, `text-cjk-latin-spacing`: the gap
	// separates Han from Latin letters and digits, and never from CJK
	// punctuation.
	let gap = LayoutOptions::default().font_size * 0.25;
	let plain = widths_of("中中\n", 4000.0, false)[0];
	let digit = widths_of("中1\n", 4000.0, false)[0];
	let comma = widths_of("中，\n", 4000.0, false)[0];
	assert!((digit - plain - gap).abs() < 0.01, "{plain} {digit}");
	assert!((comma - plain).abs() < 0.01, "{plain} {comma}");

	// `中12文1中，文`: the Han inside the digits faces one on each side.
	let line = widths_of("中12文1中，文\n", 4000.0, false);
	assert_eq!(line.len(), 8, "{line:?}");
	let last = line[7];
	assert!((line[0] - line[5]).abs() < 0.01, "{line:?}");
	assert!((line[3] - last - 2.0 * gap).abs() < 0.01, "{line:?}");
	assert!((line[5] - last - gap).abs() < 0.01, "{line:?}");
}

#[test]
fn typst_cjk_latin_spacing_stops_at_a_line_break() {
	// `cjk.typ`, `issue-2538-cjk-latin-spacing-before-linebreak`: a break
	// between the two scripts drops the gap, so neither line gains a stray
	// quarter em at its edge.
	let gap = LayoutOptions::default().font_size * 0.25;
	let base = widths_of("甲国\n", 4000.0, false)[1];
	// Two trailing spaces are a Markdown hard break.
	let rows = lines_of("甲国  \nT国\n", 400.0, false);
	assert_eq!(rows.len(), 2, "{rows:?}");
	assert!((rows[0][1].1.w - base).abs() < 0.01, "{rows:?}");
	assert!((rows[1][1].1.w - base - gap).abs() < 0.01, "{rows:?}");
}

#[test]
fn typst_adjacent_closing_marks_hug_the_line_edges() {
	// `cjk.typ`, `cjk-punctuation-adjustment-2`: a mark that carries its ink on
	// one side gives back the blank half at a line edge, and only there.
	let padded = widths_of("中《书名〈章节〉》中\n", 4000.0, false);
	let bare = widths_of("《书名〈章节〉》\n", 4000.0, false);
	assert_eq!(bare.len(), 8, "{bare:?}");
	assert_eq!(padded.len(), 10, "{padded:?}");
	assert!(
		(bare[0] * 2.0 - padded[1]).abs() < 0.01,
		"{bare:?} {padded:?}"
	);
	assert!(
		(bare[7] * 2.0 - padded[8]).abs() < 0.01,
		"{bare:?} {padded:?}"
	);
	assert!((bare[3] - padded[4]).abs() < 0.01, "{bare:?} {padded:?}");
	assert!((bare[6] - padded[7]).abs() < 0.01, "{bare:?} {padded:?}");
}

#[test]
fn typst_punctuation_shrinkability_makes_a_line_fit() {
	// `justify.typ`, `justify-punctuation-adjustment`: a run of closing marks
	// can tighten enough to keep a line that would otherwise break earlier.
	let natural = widths_of("中，，，文\n", 4000.0, false);
	let (han, mark) = (natural[0], natural[1]);
	// Just short of the four opening clusters, past what the trailing mark's own
	// half can give back.
	let target = han + 3.0 * mark - 0.9 * mark;
	let rows = lines_of("中，，，文\n", target, true);
	assert_eq!(rows.len(), 2, "{rows:?}");
	assert_eq!(rows[0].len(), 4, "{rows:?}");
	assert!((right_of(&rows[0]) - target).abs() < 0.5, "{rows:?}");
	assert!(rows[0][1].1.w < mark, "nothing compressed: {rows:?}");
}

#[test]
fn typst_a_hard_break_line_is_not_justified() {
	// `justify.typ`, `justify-manual-linebreak`: a line that ends on a hard
	// break keeps its natural width, and is not stretched to the measure.
	let width = 100.0;
	let hard = lines_of("A B C  \nD E F  \nG\n", width, true);
	assert_eq!(hard.len(), 3, "{hard:?}");
	// The same words without hard breaks wrap, and the full line does fill.
	let free = lines_of("A B C D E F G\n", width, true);
	assert!(free.len() > 1, "{free:?}");
	let justified = right_of(&free[0]);
	assert!((justified - width).abs() < 1.0, "{free:?}");
	for row in &hard[..2] {
		assert!(right_of(row) < justified - 20.0, "{row:?}");
	}
}

#[test]
fn typst_cjk_gaps_are_stretched_evenly() {
	// `justify.typ`, `issue-6062-justify-cjk-latin-spacing`: an underfull line
	// is closed by sharing the slack, so every CJK cluster that shares ends up
	// with the same advance rather than one gap taking it all.
	let width = 130.0;
	let rows = lines_of("ああああああああああああ\n", width, true);
	assert!(rows.len() > 1, "{rows:?}");
	for row in &rows[..rows.len() - 1] {
		assert!((right_of(row) - width).abs() < 1.0, "{row:?}");
		let shared: Vec<f32> = row[..row.len() - 1]
			.iter()
			.map(|(_, rect)| rect.w)
			.collect();
		let first = shared[0];
		for advance in &shared {
			assert!((advance - first).abs() < 0.01, "{row:?}");
		}
	}

	// The same holds for the mixed line of the issue once the quarter em that
	// separates a kana from a Latin letter is set aside, so the gap is stretched
	// together with the text around it rather than on its own.
	let gap = LayoutOptions::default().font_size * 0.25;
	let rows = lines_of("ああaa aaああ ああaa aaああ\n", 150.0, true);
	assert!(rows.len() > 1, "{rows:?}");
	for row in &rows[..rows.len() - 1] {
		let texts: Vec<&str> =
			row.iter().map(|(text, _)| text.as_str()).collect();
		let mut base: Option<f32> = None;
		for (i, (text, rect)) in row.iter().enumerate() {
			if i + 1 == row.len()
				|| !text.chars().all(crate::microtype::is_han_kana)
			{
				continue;
			}
			let word_spaced = |t: Option<&str>| {
				t.and_then(|t| t.chars().next())
					.is_some_and(crate::microtype::is_word_spaced)
			};
			let facing = word_spaced(i.checked_sub(1).map(|i| texts[i])) as u8
				+ word_spaced(texts.get(i + 1).copied()) as u8;
			let advance = rect.w - gap * facing as f32;
			match base {
				Some(base) => assert!((advance - base).abs() < 0.01, "{row:?}"),
				None => base = Some(advance),
			}
		}
	}
}

#[test]
fn typst_chinese_prose_justifies_by_sharing_the_slack_evenly() {
	// `justify.typ`, `justify-chinese`. Real prose from Wikipedia, including
	// enumeration commas and a closing full stop.
	let width = 240.0;
	let source = "中文维基百科使用汉字书写，汉字是汉族或华人的共同文字，是中国大陆、\
		新加坡、马来西亚、台湾、香港、澳门的唯一官方文字或官方文字之一。\n";
	let rows = lines_of(source, width, true);
	assert!(rows.len() > 2, "{rows:?}");
	for row in &rows[..rows.len() - 1] {
		assert!((right_of(row) - width).abs() < 1.0, "{row:?}");
		assert!(row[0].1.x.abs() < 0.01, "leading space: {row:?}");
		// Every Han inside the line is set to the same measure; the cluster that
		// closes the line carries no share.
		let han: Vec<f32> = row[..row.len() - 1]
			.iter()
			.filter(|(text, _)| text.chars().all(crate::microtype::is_han_kana))
			.map(|(_, rect)| rect.w)
			.collect();
		if let Some(first) = han.first() {
			for advance in &han {
				assert!((advance - first).abs() < 0.01, "{row:?}");
			}
		}
	}
}

#[test]
fn typst_japanese_prose_justifies_without_overflow() {
	// `justify.typ`, `justify-japanese`. Japanese mixes scripts inside a line,
	// so Typst settles for "at least a bit sensible" here and so does this.
	let width = 240.0;
	let source = "ウィキペディア（英: Wikipedia）は、世界中のボランティアの共同作業に\
		よって執筆及び作成されるフリーの多言語インターネット百科事典である。\n";
	let doc = document::parse(source);
	let opts = LayoutOptions {
		width,
		..Default::default()
	};
	let snapshot = LayoutEngine::new().layout(&doc, &opts);
	assert_eq!(snapshot.degraded, 0);
	assert!(snapshot.blocks.iter().all(|b| b.layout.overflow.is_empty()));
	let rows = drawn_lines(&snapshot);
	assert!(rows.len() > 2, "{rows:?}");
	for row in &rows[..rows.len() - 1] {
		// A justified line reaches the measure, though a closing mark may hang
		// past it by part of its own advance.
		let right = right_of(row);
		let mark = row.last().unwrap().1.w;
		assert!(
			right >= width - 1.0 && right <= width + mark,
			"{right} in {row:?}"
		);
		assert!(row[0].1.x.abs() < 0.01, "leading space: {row:?}");
	}
}

#[test]
fn typst_hyphenation_can_be_turned_off_for_a_passage() {
	// `hyphenate.typ`, `hyphenate-off-temporarily` and `hyphenate-punctuation`:
	// a word is handed to the hyphenator as a word, and a passage that should
	// not hyphenate — inline code or a link — is left whole. Typst reads the
	// same behaviour off a reference image; here the hyphenation points are read
	// straight out of the measured units.
	let points = |styled: Option<TextStyle>| {
		let mut e = LayoutEngine::new();
		let mut out = BlockLayout::default();
		let style = styled.unwrap_or_default();
		let rich = vec![
			Inline {
				kind: InlineKind::Text("networks".into()),
				style,
				source: 0..0,
			},
			Inline {
				kind: InlineKind::Text(" networks,".into()),
				style: TextStyle::default(),
				source: 0..0,
			},
		];
		let images = Default::default();
		let mut context = BlockContext {
			shaper: &mut e.shaper,
			math: &mut e.math,
			images: &images,
			highlight_cache: e.highlights.results(),
		};
		let p = context.prepare(&rich, 18.0, &mut out);
		let units =
			context.units(&p, 18.0, false, true, 760.0, Default::default());
		let mut points = Vec::new();
		for unit in &units {
			if unit.after.is_some_and(|b| b.hyphen_width > 0.0) {
				points.push(unit.source.end);
			}
		}
		(p.text.clone(), points)
	};

	// A plain word hyphenates, and only ever between two letters.
	let (text, plain) = points(None);
	assert!(!plain.is_empty(), "no hyphenation point in {text:?}");
	for end in &plain {
		let before = text[..*end].chars().next_back();
		let after = text[*end..].chars().next();
		assert!(before.is_some_and(|c| c.is_ascii_alphabetic()), "{text:?}");
		assert!(after.is_some_and(|c| c.is_ascii_alphabetic()), "{text:?}");
	}

	// The same word set as a link, or as inline code, keeps its hyphenation.
	for style in [
		TextStyle {
			link: Some("http://example.com".into()),
			..Default::default()
		},
		TextStyle {
			code: true,
			..Default::default()
		},
	] {
		let (text, styled) = points(Some(style));
		assert!(
			styled.iter().all(|end| *end >= 8),
			"{text:?} hyphenated a styled word: {styled:?}"
		);
		// The plain word after it still hyphenates.
		assert!(styled.iter().any(|end| *end >= 8), "{text:?} {styled:?}");
	}
}

#[test]
fn typst_curly_quotes_break_like_cjk_brackets() {
	// `inline/cjk.typ` and Typst's custom ICU segmenter: a CJK run must be able
	// to break before an opening curly quote and after a closing one, or a
	// quoted phrase glues the text around it together. The full-width CJK
	// brackets already behave that way, so the two must agree.
	let points = |text: &str| -> Vec<usize> {
		let mut e = LayoutEngine::new();
		let mut out = BlockLayout::default();
		let rich = vec![Inline {
			kind: InlineKind::Text(text.into()),
			style: TextStyle::default(),
			source: 0..0,
		}];
		let images = Default::default();
		let mut context = BlockContext {
			shaper: &mut e.shaper,
			math: &mut e.math,
			images: &images,
			highlight_cache: e.highlights.results(),
		};
		let p = context.prepare(&rich, 18.0, &mut out);
		context
			.units(&p, 18.0, false, false, 760.0, Default::default())
			.iter()
			.filter(|u| u.after.is_some())
			.map(|u| u.source.end)
			.collect()
	};

	let curly = "中文“引号”测试";
	let bracket = "中文「引号」测试";
	let after = |text: &str, c: char| text.find(c).unwrap();
	let curly_points = points(curly);
	assert!(
		curly_points.contains(&after(curly, '“')),
		"no break before the opening quote: {curly_points:?}"
	);
	assert!(
		curly_points.contains(&(after(curly, '”') + '”'.len_utf8())),
		"no break after the closing quote: {curly_points:?}"
	);
	// The quote still clings to the phrase it belongs to.
	assert!(!curly_points.contains(&(after(curly, '“') + '“'.len_utf8())));
	assert!(!curly_points.contains(&after(curly, '”')));

	// The native brackets reach the same shape from ICU alone, so the override
	// leaves no gap between the two conventions.
	assert_eq!(
		curly_points.len(),
		points(bracket).len(),
		"{curly_points:?}"
	);
}

#[test]
fn the_cjk_convention_decides_punctuation_at_a_line_end() {
	// A comma-like mark is left aligned on the mainland and in Japan, so it
	// gives back its blank right half at a line end, and centered in Taiwan,
	// where it does not.
	use crate::style::CjkType;
	// The convention travels inside the stylesheet, which is also what selects
	// the `[cjk]` font definition, so this is the path the reader uses.
	let width = |cjk| {
		let mut sheet = (*crate::style::Stylesheet::bundled(false)).clone();
		sheet.set_cjk_type(cjk);
		let doc = document::parse("甲，\n");
		let opts = LayoutOptions {
			width: 400.0,
			justify: false,
			stylesheet: std::sync::Arc::new(sheet),
			..Default::default()
		};
		let snapshot = LayoutEngine::new().layout(&doc, &opts);
		let rows = drawn_lines(&snapshot);
		rows[0].last().unwrap().1.w
	};
	let mainland = width(CjkType::Sc);
	let japan = width(CjkType::Jp);
	let taiwan = width(CjkType::Tc);
	assert!(
		(mainland * 2.0 - taiwan).abs() < 0.01,
		"{mainland} {taiwan}"
	);
	assert!((mainland - japan).abs() < 0.01, "{mainland} {japan}");
	// Turning the CJK font variant off keeps the common convention.
	assert!((width(CjkType::None) - mainland).abs() < 0.01);
}

#[test]
fn a_hyphen_near_a_word_edge_costs_more_than_one_in_the_middle() {
	// The penalty is graded by the distance from either edge, so a break that
	// leaves a stub is worth avoiding even at a slightly better ratio.
	assert_eq!(inline::hyphen_penalty(5, 5), 50.0);
	assert_eq!(inline::hyphen_penalty(2, 3), 87.5);
	assert_eq!(inline::hyphen_penalty(2, 5), 72.5);
	assert!(inline::hyphen_penalty(1, 1) > inline::hyphen_penalty(2, 3));
	assert!(inline::hyphen_penalty(2, 3) > inline::hyphen_penalty(5, 5));

	// And it reaches the break search. `hy-phen-ation` offers a point three
	// characters in and one in the middle, which must cost less.
	let mut e = LayoutEngine::new();
	let mut out = BlockLayout::default();
	let rich = vec![Inline {
		kind: InlineKind::Text("hyphenation".into()),
		style: TextStyle::default(),
		source: 0..0,
	}];
	let images = Default::default();
	let mut context = BlockContext {
		shaper: &mut e.shaper,
		math: &mut e.math,
		images: &images,
		highlight_cache: e.highlights.results(),
	};
	let p = context.prepare(&rich, 18.0, &mut out);
	let found: Vec<(usize, f64)> = context
		.units(&p, 18.0, false, true, 760.0, Default::default())
		.iter()
		.filter_map(|u| {
			u.after
				.filter(|b| b.hyphen_width > 0.0)
				.map(|b| (u.source.end, b.penalty))
		})
		.collect();
	assert!(found.len() >= 2, "{found:?}");
	let cheapest = found.iter().min_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
	let dearest = found.iter().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
	assert!(dearest.1 > cheapest.1, "{found:?}");
	// The cheapest break is the one in the middle of the word.
	assert_eq!(cheapest.0, "hyphen".len());
}

#[test]
fn typst_the_last_line_can_be_shrunk() {
	// `justify.typ`, `justify-shrink-last-line`: a closing line that slightly
	// overflows gives back spacing and stays on one line, even though it is not
	// stretched the way the justified lines above it are. The text ends on a
	// letter so that nothing hangs into the margin here.
	let text = "A short line of text here\n";
	let natural = {
		let doc = document::parse(text);
		let opts = LayoutOptions {
			width: 4000.0,
			justify: false,
			..Default::default()
		};
		let rows = drawn_lines(&LayoutEngine::new().layout(&doc, &opts));
		right_of(&rows[0])
	};
	// Less than a pixel of overflow would go unnoticed, so take a full one.
	let width = natural - 0.9;
	assert!(natural > width + 0.5);
	let doc = document::parse(text);
	let snapshot = LayoutEngine::new().layout(
		&doc,
		&LayoutOptions {
			width,
			..Default::default()
		},
	);
	let rows = drawn_lines(&snapshot);
	assert_eq!(rows.len(), 1, "{rows:?}");
	assert!(snapshot.blocks.iter().all(|b| b.layout.overflow.is_empty()));
	assert!((right_of(&rows[0]) - width).abs() < 0.01, "{rows:?}");
}

#[test]
fn typst_a_closing_mark_hangs_into_the_end_margin() {
	// `overhang.typ`: the last glyph of a line gives back the blank side of its
	// own advance, which is what makes a justified line read as flush instead
	// of stopping a notch short of the margin.
	let text = "The first clause ends here, and the second clause carries on for a \
		while, then the third and final clause closes the sentence.\n";
	let width = 200.0;
	let doc = document::parse(text);
	let snapshot = LayoutEngine::new().layout(
		&doc,
		&LayoutOptions {
			width,
			..Default::default()
		},
	);
	let rows = drawn_lines(&snapshot);
	assert!(rows.len() > 1, "{rows:?}");
	// The comma's own advance, measured where nothing is tight. The drawn mark
	// may be compressed with the rest of the line, so it is not the reference.
	let wide = {
		let doc = document::parse(text);
		let opts = LayoutOptions {
			width: 4000.0,
			justify: false,
			..Default::default()
		};
		drawn_lines(&LayoutEngine::new().layout(&doc, &opts))
	};
	let mark = wide[0]
		.iter()
		.find(|(text, _)| text == ",")
		.map(|(_, rect)| rect.w)
		.expect("no comma");
	// The first line closes on that comma, so it reaches past the measure by
	// the blank the mark carries on its right.
	assert_eq!(rows[0].last().unwrap().0, ",");
	assert!(
		(right_of(&rows[0]) - width - 0.8 * mark).abs() < 0.01,
		"right {} width {width} mark {mark}",
		right_of(&rows[0])
	);
	// A line that ends on a letter stays inside it.
	assert!(right_of(&rows[1]) < width + 0.01, "{:?}", rows[1]);
	assert!(snapshot.blocks.iter().all(|b| b.layout.overflow.is_empty()));
}

#[test]
fn an_explicit_html_break_justifies_the_line_it_ends() {
	// Typst has both a plain manual break and `linebreak(justify: true)`: an
	// author who asks for a break may still want the line flush. Markdown's own
	// hard break says the opposite, so the two are told apart — `<br>` is the
	// explicit request, two trailing spaces are not.
	let width = 100.0;
	let plain = lines_of("A B C D  \nE F G H  \nI\n", width, true);
	let explicit = lines_of("A B C D<br>E F G H<br>I\n", width, true);
	assert_eq!(plain.len(), 3, "{plain:?}");
	assert_eq!(explicit.len(), 3, "{explicit:?}");
	for row in &plain[..2] {
		assert!(right_of(row) < width - 20.0, "{row:?}");
	}
	for row in &explicit[..2] {
		assert!((right_of(row) - width).abs() < 1.0, "{row:?}");
	}
	// The closing line is the end of the paragraph, so it stays as it is.
	assert!(right_of(&explicit[2]) < width - 20.0, "{explicit:?}");
}

#[test]
fn footnote_bodies_share_the_same_left_edge() {
	let fixture = include_str!("../../../../tests/fixtures/footnote.md");
	// Ten notes make one- and two-digit numbers, whose glyphs differ in
	// width; every body should still start at the same x.
	let mut many = String::from("Notes[^1]");
	for n in 2..=10 {
		many.push_str(&format!(" [^{n}]"));
	}
	for n in 1..=10 {
		many.push_str(&format!("\n\n[^{n}]: Note {n}.\n"));
	}
	let cases: [(&str, usize); 2] = [(fixture, 4), (many.as_str(), 10)];
	for (source, notes) in cases {
		let doc = document::parse(source);
		let mut engine = LayoutEngine::new();
		let snapshot = engine.layout(
			&doc,
			&LayoutOptions {
				width: 400.0,
				..Default::default()
			},
		);
		let left = |block: usize| {
			snapshot.blocks[block]
				.layout
				.text
				.first()
				.and_then(|n| n.clusters.first())
				.map(|c| c.rect.x)
				.expect("a note body")
		};
		for block in 2..=notes {
			assert!(
				(left(block) - left(1)).abs() < 0.01,
				"note {block} starts at {}",
				left(block)
			);
		}
	}
}
