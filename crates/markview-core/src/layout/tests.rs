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
	let units = context.units(&p, 18.0, false, true, 760.0);
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
