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
fn every_outline_anchor_resolves_like_a_fragment_link() {
	let doc = document::parse(
		"# First\n\nBody.\n\n> ## Quoted\n\n- ### Listed\n\n# First\n",
	);
	let snapshot = LayoutEngine::new().layout(&doc, &LayoutOptions::default());
	let mut last = f32::NEG_INFINITY;
	for entry in doc.outline() {
		let y = snapshot.anchor_y(&entry.anchor).unwrap_or_else(|| {
			panic!("unresolved outline anchor {}", entry.anchor)
		});
		assert!(y >= last, "outline order is not reading order");
		last = y;
	}
}
#[test]
fn a_quote_bar_is_centered_on_the_text_it_frames() {
	// The quote bar runs down the box's left edge, so a box that kept the
	// outer spacing of its children would hang past the text on one side and
	// stop short on the other.
	for source in [
		"> Quoted paragraph text.\n",
		"> ## Quoted heading\n>\n> Quoted paragraph text.\n",
	] {
		let doc = document::parse(source);
		let snapshot =
			LayoutEngine::new().layout(&doc, &LayoutOptions::default());
		let quote = &snapshot.blocks[0];
		let rect = quote
			.layout
			.draws
			.iter()
			.find_map(|d| match d {
				Draw::Box {
					rect,
					condition: Condition::Blockquote,
					..
				} => Some(*rect),
				_ => None,
			})
			.expect("blockquote box");
		let (top, bottom) = quote
			.layout
			.text
			.iter()
			.flat_map(|node| &node.clusters)
			.fold((f32::INFINITY, f32::NEG_INFINITY), |(top, bottom), c| {
				(c.rect.y.min(top), (c.rect.y + c.rect.h).max(bottom))
			});
		let (above, below) = (top - rect.y, rect.y + rect.h - bottom);
		assert!(
			(above - below).abs() < 0.5,
			"{source:?}: the quote bar sits {above} above the text and \
			 {below} below it"
		);
	}
}
#[test]
fn list_items_keep_the_paragraph_space_between_them() {
	// An item's own box carries no spacing in the bundled themes, so the
	// paragraph's trailing space is what separates one item from the next. A
	// quote hugs its content; a list item must not.
	let opts = LayoutOptions::default();
	let doc = document::parse("- First item\n- Second item\n");
	let snapshot = LayoutEngine::new().layout(&doc, &opts);
	// A bullet is a drawn shape, so each item contributes one text node at the
	// indented margin.
	let items: Vec<Rect> = snapshot.blocks[0]
		.layout
		.text
		.iter()
		.map(|node| node.clusters[0].rect)
		.collect();
	assert_eq!(items.len(), 2);
	let gap = items[1].y - items[0].y;
	assert!(
		gap > items[0].h + opts.font_size * 0.5,
		"the items are {gap} apart"
	);
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
fn consecutive_footnote_references_merge_into_one_clickable_group() {
	let doc = document::parse(
		"Text[^a][^b].\n\n\
		 [^a]: Alpha note.\n\n\
		 [^b]: Beta note.\n",
	);
	let mut engine = LayoutEngine::new();
	let snapshot = engine.layout(
		&doc,
		&LayoutOptions {
			width: 400.0,
			..Default::default()
		},
	);
	let block = &snapshot.blocks[0];
	assert!(block.layout.text[0].text.contains("[1,2]"));
	let links: Vec<&str> =
		block.layout.links.iter().map(|l| l.url.as_str()).collect();
	assert_eq!(links, ["#fn:1", "#fn:2"]);
	// Both numbers register the anchor a scrolled-to note returns to.
	assert!(snapshot.anchor_y("fnref:1").is_some());
	assert!(snapshot.anchor_y("fnref:2").is_some());
	// Only the numbers are hit targets; the brackets and comma are not.
	let reading = &block.layout.text[0].text;
	let clusters = &block.layout.text[0].clusters;
	let point = |glyph: &str| {
		let c = clusters
			.iter()
			.find(|c| &reading[c.range.clone()] == glyph)
			.unwrap_or_else(|| panic!("no {glyph} cluster"));
		(
			c.rect.x + c.rect.w * 0.5,
			block.y + c.rect.y + c.rect.h * 0.5,
		)
	};
	let empty = HashMap::new();
	let hit = |glyph: &str| {
		let (x, y) = point(glyph);
		snapshot.link_at(x, y, &empty)
	};
	assert_eq!(hit("["), None);
	assert_eq!(hit("1"), Some("#fn:1"));
	assert_eq!(hit(","), None);
	assert_eq!(hit("2"), Some("#fn:2"));
	assert_eq!(hit("]"), None);
}
#[test]
fn whitespace_between_footnote_references_still_merges() {
	let doc = document::parse(
		"Text[^a] [^b].\n\n\
		 [^a]: Alpha note.\n\n\
		 [^b]: Beta note.\n",
	);
	let mut engine = LayoutEngine::new();
	let snapshot = engine.layout(
		&doc,
		&LayoutOptions {
			width: 400.0,
			..Default::default()
		},
	);
	// The space between the two references becomes the comma.
	assert!(snapshot.blocks[0].layout.text[0].text.contains("[1,2]"));
}
#[test]
fn every_digit_of_a_grouped_number_is_clickable() {
	let mut source = String::from("Notes");
	for n in 1..=9 {
		source.push_str(&format!(" [^{n}],"));
	}
	source.push_str(" [^10][^11].\n\n");
	for n in 1..=11 {
		source.push_str(&format!("[^{n}]: Note {n}.\n\n"));
	}
	let doc = document::parse(source);
	let mut engine = LayoutEngine::new();
	let snapshot = engine.layout(
		&doc,
		&LayoutOptions {
			width: 760.0,
			..Default::default()
		},
	);
	let block = &snapshot.blocks[0];
	let reading = &block.layout.text[0].text;
	let group = reading.find("[10,11]").expect("the merged group");
	let empty = HashMap::new();
	let hit = |offset: usize| {
		let c = block.layout.text[0]
			.clusters
			.iter()
			.find(|c| c.range.start == offset)
			.unwrap_or_else(|| panic!("no cluster at {offset}"));
		snapshot.link_at(
			c.rect.x + c.rect.w * 0.5,
			block.y + c.rect.y + c.rect.h * 0.5,
			&empty,
		)
	};
	// Both digits of each number are hit targets, not just the first.
	assert_eq!(hit(group + 1), Some("#fn:10"));
	assert_eq!(hit(group + 2), Some("#fn:10"));
	assert_eq!(hit(group + 4), Some("#fn:11"));
	assert_eq!(hit(group + 5), Some("#fn:11"));
	assert_eq!(hit(group), None);
	assert_eq!(hit(group + 3), None);
	assert_eq!(hit(group + 6), None);
	// A number returns to its reference once, from its first digit.
	let anchors = |label: &str| {
		block
			.layout
			.anchors
			.iter()
			.filter(|a| a.anchor == label)
			.count()
	};
	assert_eq!(anchors("fnref:10"), 1);
	assert_eq!(anchors("fnref:11"), 1);
}
#[test]
fn a_lone_footnote_reference_stays_whole_clickable() {
	let doc = document::parse("Text[^a].\n\n[^a]: A note.\n");
	let mut engine = LayoutEngine::new();
	let snapshot = engine.layout(
		&doc,
		&LayoutOptions {
			width: 400.0,
			..Default::default()
		},
	);
	let block = &snapshot.blocks[0];
	assert!(block.layout.text[0].text.contains("[1]"));
	let link = &block.layout.links[0];
	let reading = &block.layout.text[0].text;
	let bracket = block.layout.text[0]
		.clusters
		.iter()
		.find(|c| &reading[c.range.clone()] == "[")
		.expect("the opening bracket");
	// A single reference keeps its bracket pair in the hit target.
	assert!(link.rect.x <= bracket.rect.x);
	let y = block.y + link.rect.y + link.rect.h * 0.5;
	let empty = HashMap::new();
	assert_eq!(
		snapshot.link_at(bracket.rect.x + 1.0, y, &empty),
		Some("#fn:1")
	);
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
		marker_depth: 0,
		enum_depth: 0,
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
	let appended = snapshot(&[1, 2, 3, 4, 5, 6, 7, 8]);
	// The old limit is 653.33, so 700 sits in the tail it reserves: following
	// holds the bottom there, while not following keeps the block under the
	// reader, which the appended block leaves where it was.
	assert_eq!(
		anchored_scroll(&old, &appended, 700.0, 200.0, true),
		scroll_limit(appended.height, 200.0)
	);
	assert_eq!(anchored_scroll(&old, &appended, 700.0, 200.0, false), 700.0);
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
	// A list indents as a whole: the leading marker and the item text move
	// together, and the item's opening and wrapped lines share one margin.
	for block in [3, 4] {
		let last = |s: &LayoutSnapshot| s.blocks[block].layout.text.len() - 1;
		let marker = |s: &LayoutSnapshot| first_x(s, block, 0);
		let text = |s: &LayoutSnapshot| first_x(s, block, last(s));
		assert!((marker(&two) - marker(&plain) - d2).abs() < 0.6);
		assert!((text(&two) - text(&plain) - d2).abs() < 0.6);
		assert!(
			(second_line_x(&two, block, last(&two)) - text(&two)).abs() < 0.6
		);
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
		let x = |block: usize| {
			s.blocks[block].layout.text.last().unwrap().clusters[0]
				.rect
				.x
		};
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
fn a_theme_aligns_list_markers_in_their_column() {
	fn marker_x(sheet: &crate::style::Stylesheet, task: bool) -> f32 {
		let doc = document::parse(if task {
			"- [x] task item\n"
		} else {
			"- bullet item\n"
		});
		let snapshot = LayoutEngine::new().layout(
			&doc,
			&LayoutOptions {
				width: 400.0,
				stylesheet: Arc::new(sheet.clone()),
				..Default::default()
			},
		);
		// Every marker is drawn as a polygon; its leftmost vertex is the
		// marker's left edge.
		snapshot.blocks[0]
			.layout
			.draws
			.iter()
			.filter_map(|draw| match draw {
				Draw::Polygon { center, points, .. } => Some(
					points
						.iter()
						.map(|p| center[0] + p[0])
						.fold(f32::INFINITY, f32::min),
				),
				_ => None,
			})
			.fold(f32::INFINITY, f32::min)
	}
	let aligned = |condition: &str, align: &str| {
		let mut sheet = (*crate::style::Stylesheet::bundled(false)).clone();
		sheet.merge(
			&crate::style::Stylesheet::parse(&format!(
				"format_version=2\nversion=1\n[[rule]]\nwhen=['{condition}']\nalign=\"{align}\""
			))
			.unwrap(),
		);
		sheet
	};
	for (condition, task) in [("marker", false), ("task_marker", true)] {
		let default = marker_x(&crate::style::Stylesheet::bundled(false), task);
		let left = marker_x(&aligned(condition, "left"), task);
		let center = marker_x(&aligned(condition, "center"), task);
		let right = marker_x(&aligned(condition, "right"), task);
		assert!(
			left < center && center < right,
			"{condition}: {left} {center} {right}"
		);
		assert!(
			(default - center).abs() < 0.01,
			"{condition} should default to centered, got {default} vs {center}"
		);
	}
}

#[test]
fn a_bullet_is_drawn_and_never_copied() {
	let snapshot = LayoutEngine::new().layout(
		&document::parse("- one\n- [x] two\n"),
		&LayoutOptions::default(),
	);
	let layout = &snapshot.blocks[0].layout;
	// Bullets and checkboxes are geometry, not reading text.
	assert!(
		layout
			.draws
			.iter()
			.any(|d| matches!(d, Draw::Polygon { .. }))
	);
	assert!(layout.text.iter().all(|node| !node.text.contains('•')));
	assert!(layout.text.iter().all(|node| !node.text.contains("[x]")));
	let text = snapshot.extract_text(snapshot.select_all(1).unwrap(), 1);
	assert_eq!(text, "one\ntwo");
}

#[test]
fn a_task_checkbox_is_a_drawn_box_a_theme_styles() {
	let mut sheet = (*crate::style::Stylesheet::bundled(false)).clone();
	sheet.merge(
		&crate::style::Stylesheet::parse(
			"format_version=2\nversion=1\n[[rule]]\nwhen=['task_marker']\nsize=1.5\nborder_width=3.0\nradius=5.0\ncolor=\"#FFFFFF\"\nbackground=\"#FFFFFF\"\naccent=\"#C0392B\"\nborder_color=\"#123456\"",
		)
		.unwrap(),
	);
	let snapshot = LayoutEngine::new().layout(
		&document::parse("- [x] done\n- [ ] todo\n"),
		&LayoutOptions {
			width: 400.0,
			stylesheet: Arc::new(sheet.clone()),
			..Default::default()
		},
	);
	use crate::style::ColorField;
	let span = |points: &[[f32; 2]], axis: usize| {
		let (lo, hi) = points
			.iter()
			.fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| {
				(lo.min(p[axis]), hi.max(p[axis]))
			});
		hi - lo
	};
	let mut surface = 0;
	let mut accents = 0;
	let mut rings = 0;
	let mut checks = 0;
	let mut box_geometry = None;
	for draw in &snapshot.blocks[0].layout.draws {
		let Draw::Polygon {
			center,
			points,
			paint,
		} = draw
		else {
			continue;
		};
		match paint {
			// Every box keeps the surface fill; a completed one adds the
			// accent over it.
			Paint::Scoped(_, _, ColorField::Background) => {
				surface += 1;
				assert_eq!(
					sheet.paint(*paint),
					crate::style::Color(0xFFFFFFFF).rgba()
				);
			}
			Paint::Scoped(_, _, ColorField::Accent) => {
				accents += 1;
				box_geometry = Some((*center, points.clone()));
				assert_eq!(
					sheet.paint(*paint),
					crate::style::Color(0xC0392BFF).rgba()
				);
			}
			Paint::Scoped(_, _, ColorField::BorderColor) => {
				rings += 1;
				assert_eq!(
					sheet.paint(*paint),
					crate::style::Color(0x123456FF).rgba()
				);
				// The ring is the outer contour plus the inner one cut into
				// it, so both halves are present.
				assert_eq!(points.len() % 2, 0);
			}
			Paint::Cascade(..) => {
				checks += 1;
				assert_eq!(points.len(), 6);
				assert_eq!(
					sheet.paint(*paint),
					crate::style::Color(0xFFFFFFFF).rgba()
				);
			}
			other => panic!("unexpected checkbox paint {other:?}"),
		}
	}
	assert_eq!((surface, accents, rings, checks), (2, 1, 2, 1));
	// The theme sets the box's side; the check is centered and stays inside.
	let (center, points) = box_geometry.expect("a filled completed box");
	let side = 18.0 * 1.5 * 0.72;
	assert!(
		(span(&points, 0) - side).abs() < 0.5,
		"{}",
		span(&points, 0)
	);
	assert!((span(&points, 1) - side).abs() < 0.5);
	let check = snapshot.blocks[0]
		.layout
		.draws
		.iter()
		.find_map(|draw| match draw {
			Draw::Polygon { center, points, .. } if points.len() == 6 => {
				Some((*center, points.clone()))
			}
			_ => None,
		})
		.expect("a check");
	assert!((check.0[0] - center[0]).abs() < 0.01);
	assert!((check.0[1] - center[1]).abs() < 0.01);
	for point in check.1.iter() {
		assert!(point[0].abs() <= side * 0.5 && point[1].abs() <= side * 0.5);
	}
}

#[test]
fn a_checkbox_outline_is_hollow() {
	// A pending checkbox draws only its outline, so the ring must not fill
	// its own middle.
	let (side, border) = (13.0, 1.5);
	let ring = super::blocks::rounded_square_ring(side, 2.5, border);
	let contains = |p: [f32; 2]| {
		let mut inside = false;
		let mut j = ring.len() - 1;
		for i in 0..ring.len() {
			let (a, b) = (ring[i], ring[j]);
			if (a[1] > p[1]) != (b[1] > p[1])
				&& p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
			{
				inside = !inside;
			}
			j = i;
		}
		inside
	};
	assert!(!contains([0.0, 0.0]), "the middle is hollow");
	// The middle of the bottom band is ink, and a point past the box is not.
	assert!(contains([0.0, side * 0.5 - border * 0.5]));
	assert!(!contains([0.0, side * 0.5 + 1.0]));
}

#[test]
fn a_rounded_box_keeps_its_sides_straight() {
	// Every corner carries both of its endpoints, so the edges between
	// corners are the axis-aligned sides rather than chords across a missing
	// arc. Each extreme of the box therefore holds two vertices.
	let (side, radius) = (13.0, 4.0);
	let points = super::blocks::rounded_square(side, radius);
	let half = side / 2.;
	let at = |value: f32, axis: usize| {
		points
			.iter()
			.filter(|p| (p[axis] - value).abs() < 0.01)
			.count()
	};
	assert_eq!(at(half, 0), 2, "right side");
	assert_eq!(at(-half, 0), 2, "left side");
	assert_eq!(at(half, 1), 2, "bottom side");
	assert_eq!(at(-half, 1), 2, "top side");
}

#[test]
fn a_check_is_the_same_shape_at_any_box_size() {
	// The check is vector geometry, so it scales with its box instead of
	// depending on whether a font happens to carry a check glyph.
	let bounds = |side: f32| {
		let points = super::blocks::check_points(side);
		let span = |axis: usize| {
			let (lo, hi) = points
				.iter()
				.fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| {
					(lo.min(p[axis]), hi.max(p[axis]))
				});
			(lo, hi - lo)
		};
		(span(0), span(1))
	};
	let (x, y) = bounds(13.0);
	// The mark stays inside its box and does not share the box's proportions.
	assert!(x.0 > -6.5 && x.0 + x.1 < 6.5);
	assert!(y.0 > -6.5 && y.0 + y.1 < 6.5);
	assert!(x.1 > y.1, "a check is wider than it is tall");
	let (big_x, big_y) = bounds(26.0);
	assert!((big_x.1 - x.1 * 2.0).abs() < 0.01);
	assert!((big_y.1 - y.1 * 2.0).abs() < 0.01);
}

#[test]
fn a_theme_picks_the_bullet_shape() {
	// The bullet's own vertices and bounding box, in marker-local units.
	let shape_of = |shape: &str| -> (usize, f32, f32) {
		let mut sheet = (*crate::style::Stylesheet::bundled(false)).clone();
		sheet.merge(
			&crate::style::Stylesheet::parse(&format!(
				"format_version=2\nversion=1\n[[rule]]\nwhen=['marker']\nshape=\"{shape}\""
			))
			.unwrap(),
		);
		let snapshot = LayoutEngine::new().layout(
			&document::parse("- item\n"),
			&LayoutOptions {
				width: 400.0,
				stylesheet: Arc::new(sheet),
				..Default::default()
			},
		);
		snapshot.blocks[0]
			.layout
			.draws
			.iter()
			.find_map(|draw| match draw {
				Draw::Polygon { points, .. } => {
					let span = |axis: usize| {
						points.iter().map(|p| p[axis].abs()).fold(0.0, f32::max)
							* 2.0
					};
					Some((points.len(), span(0), span(1)))
				}
				_ => None,
			})
			.expect("bullet polygon")
	};
	assert_eq!(shape_of("disc").0, 64);
	assert_eq!(shape_of("square").0, 4);
	assert_eq!(shape_of("triangle").0, 3);
	assert_eq!(shape_of("diamond").0, 4);
	assert_eq!(shape_of("plus").0, 12);
	let (_, width, height) = shape_of("minus");
	assert!(height < width / 2., "a dash is flat: {width} x {height}");
}

#[test]
fn a_shape_cycle_follows_the_bullet_nesting_depth() {
	let sheet = {
		let mut sheet = (*crate::style::Stylesheet::bundled(false)).clone();
		sheet.merge(
			&crate::style::Stylesheet::parse(
				"format_version=2\nversion=1\n[[rule]]\nwhen=['marker']\nshape=['plus','minus']",
			)
			.unwrap(),
		);
		Arc::new(sheet)
	};
	// One vertex count per bullet, in reading order.
	let shapes = |source: &str| -> Vec<usize> {
		let snapshot = LayoutEngine::new().layout(
			&document::parse(source),
			&LayoutOptions {
				width: 400.0,
				stylesheet: sheet.clone(),
				..Default::default()
			},
		);
		snapshot
			.blocks
			.iter()
			.flat_map(|b| b.layout.draws.iter())
			.filter_map(|draw| match draw {
				Draw::Polygon { points, .. } => Some(points.len()),
				_ => None,
			})
			.collect()
	};
	// Three bullet levels cycle plus, minus, plus.
	assert_eq!(shapes("- one\n  - two\n    - three\n"), [12, 4, 12]);
	// An ordered level is transparent to the cycle.
	assert_eq!(shapes("- one\n  1. two\n     - three\n"), [12, 4]);
}

/// The bundled stylesheet with one `enum` rule merged on top.
fn ordered_sheet(rule: &str) -> Arc<crate::style::Stylesheet> {
	let mut sheet = (*crate::style::Stylesheet::bundled(false)).clone();
	sheet.merge(
		&crate::style::Stylesheet::parse(&format!(
			"format_version=2\nversion=1\n[[rule]]\nwhen=['enum']\n{rule}"
		))
		.unwrap(),
	);
	Arc::new(sheet)
}

#[test]
fn a_theme_numbers_ordered_lists() {
	let snapshot = LayoutEngine::new().layout(
		&document::parse("1. one\n2. two\n"),
		&LayoutOptions {
			width: 400.0,
			stylesheet: ordered_sheet("numbering=\"a)\""),
			..Default::default()
		},
	);
	// A formatted number is reading text, so it copies as the theme writes it.
	assert_eq!(
		snapshot.extract_text(snapshot.select_all(1).unwrap(), 1),
		"a) one\nb) two"
	);
}

#[test]
fn a_numbering_pattern_gives_each_nesting_level_its_symbol() {
	let snapshot = LayoutEngine::new().layout(
		&document::parse("1. outer\n   1. inner\n"),
		&LayoutOptions {
			width: 400.0,
			stylesheet: ordered_sheet("numbering=\"1.a.\""),
			..Default::default()
		},
	);
	assert_eq!(
		snapshot.extract_text(snapshot.select_all(1).unwrap(), 1),
		"1. outer\na. inner"
	);
}

#[test]
fn a_theme_aligns_ordered_numbers_in_their_column() {
	fn number_x(rule: &str) -> f32 {
		let snapshot = LayoutEngine::new().layout(
			&document::parse("1. ordered item\n"),
			&LayoutOptions {
				width: 400.0,
				stylesheet: ordered_sheet(rule),
				..Default::default()
			},
		);
		// The number draws before the item text, so it owns the first glyph.
		snapshot.blocks[0]
			.layout
			.draws
			.iter()
			.find_map(|draw| match draw {
				Draw::Glyph(glyph) => Some(glyph.x),
				_ => None,
			})
			.expect("number glyphs")
	}
	let left = number_x("align=\"left\"");
	let center = number_x("align=\"center\"");
	let right = number_x("align=\"right\"");
	assert!(left < center && center < right, "{left} {center} {right}");
	// A number without an `enum` alignment follows `marker`'s, which the
	// bundled styles center.
	assert!((number_x("numbering=\"1.\"") - center).abs() < 0.01);
}

#[test]
fn a_wide_numbering_format_widens_the_marker_column() {
	// The reserve is in logical pixels, so a large reader size is exactly when
	// a fixed column would let a number run into its item text.
	let layout = |numbering: &str| {
		let source: String = (1..=8).map(|n| format!("{n}. item\n")).collect();
		LayoutEngine::new().layout(
			&document::parse(source.as_str()),
			&LayoutOptions {
				width: 400.0,
				font_size: 30.0,
				stylesheet: ordered_sheet(&format!(
					"numbering=\"{numbering}\""
				)),
				..Default::default()
			},
		)
	};
	let text_x = |snapshot: &LayoutSnapshot| {
		snapshot.blocks[0].layout.text[1].clusters[0].rect.x
	};
	let decimal = layout("1.");
	let roman = layout("I.");
	assert!(
		text_x(&roman) > text_x(&decimal),
		"roman numbers need more room: {} vs {}",
		text_x(&decimal),
		text_x(&roman)
	);
	// Every number stays clear of the text that follows it.
	for snapshot in [&decimal, &roman] {
		for pair in snapshot.blocks[0].layout.text.chunks(2) {
			let (label, text) =
				(pair[0].clusters[0].rect, pair[1].clusters[0].rect);
			assert!(
				label.x + label.w <= text.x + 0.01,
				"the number {label:?} overlaps the text {text:?}"
			);
		}
	}
}

#[test]
fn a_huge_list_start_cannot_expand_a_symbolic_numbering() {
	// `999999999.` is a valid list start and `*` repeats a symbol every six
	// items, so the marker falls back to decimal instead of building a label
	// hundreds of megabytes long.
	let snapshot = LayoutEngine::new().layout(
		&document::parse("999999999. item\n"),
		&LayoutOptions {
			width: 400.0,
			stylesheet: ordered_sheet("numbering=\"*\""),
			..Default::default()
		},
	);
	assert_eq!(
		snapshot.extract_text(snapshot.select_all(1).unwrap(), 1),
		"999999999 item"
	);
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
		let block = &snapshot.blocks[0].layout;
		let mut chips: Vec<(f32, f32, f32)> = block
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
		assert!(!chips.is_empty(), "width={width}: {chips:?}");
		// The run's clusters share one chip, so the stretched space between its
		// two words is covered rather than left as a hole.
		let node = &block.text[0];
		let start = node.text.find("git fetch").expect("the code text");
		let code = start..start + "git fetch".len();
		let first = node
			.clusters
			.iter()
			.find(|c| c.range.start == code.start)
			.unwrap_or_else(|| panic!("width={width}: no cluster at {start}"));
		let last = node
			.clusters
			.iter()
			.rfind(|c| code.contains(&c.range.start))
			.unwrap_or_else(|| panic!("width={width}: no code clusters"));
		let chip = match &block.draws[first.command] {
			crate::scene::Draw::Rect(rect, _) => *rect,
			other => panic!("width={width}: expected a chip, found {other:?}"),
		};
		let covered = last.rect.x + last.rect.w - first.rect.x;
		assert!(
			chip.w >= covered - 0.01,
			"width={width}: chip {chip:?} leaves the run uncovered ({covered})"
		);
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

#[test]
fn an_inline_code_chip_pads_its_text_and_pushes_its_neighbours() {
	// `padding` on `code` insets the run's glyphs inside a wider chip, moves
	// the text after the chip along with it, and makes the chip taller, so a
	// code run beside CJK no longer reads cramped.
	let layout = |padding: &str| -> (Rect, Rect, f32, f32, Rect) {
		let mut sheet = (*crate::style::Stylesheet::bundled(false)).clone();
		sheet.merge(&crate::style::Stylesheet::parse(padding).unwrap());
		let doc = document::parse("x `code` y\n");
		let snapshot = LayoutEngine::new().layout(
			&doc,
			&LayoutOptions {
				width: 2000.0,
				stylesheet: Arc::new(sheet),
				..Default::default()
			},
		);
		let block = &snapshot.blocks[0].layout;
		let node = &block.text[0];
		let cluster = |start: usize| {
			node.clusters
				.iter()
				.find(|c| c.range.start == start)
				.unwrap_or_else(|| panic!("no cluster at {start}"))
		};
		let first = cluster(2);
		// The chip is pushed just before the glyphs of the code's first cluster.
		let chip = match &block.draws[first.command] {
			Draw::Rect(rect, _) => *rect,
			other => panic!("expected the chip, found {other:?}"),
		};
		let glyph = match &block.draws[first.command + 1] {
			Draw::Glyph(glyph) => glyph.x,
			other => panic!("expected a glyph, found {other:?}"),
		};
		(first.rect, cluster(5).rect, glyph, cluster(7).rect.x, chip)
	};

	// The bundled styles pad the chip, so the baseline zeroes it again.
	let bare = layout(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['code']\npadding=[0.0,0.0,0.0,0.0]",
	);
	let (first, last, glyph, after, chip) = layout(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['code']\npadding=[0.1,0.3,0.1,0.3]",
	);
	let side = 0.3 * 18.0;
	// The chip keeps its left edge, widens by one side's padding, insets its
	// glyphs, and carries the following space and word along with it. It covers
	// the whole run, so a later cluster's box is inside the same rectangle.
	assert!((chip.x - first.x).abs() < 0.01, "{chip:?} {first:?}");
	assert!(
		chip.w >= last.x + last.w - first.x - 0.01,
		"{chip:?} {first:?} {last:?}"
	);
	assert!(
		(first.w - (bare.0.w + side)).abs() < 0.01,
		"{first:?} {:?} side={side}",
		bare.0
	);
	assert!(
		(last.w - (bare.1.w + side)).abs() < 0.01,
		"{last:?} {:?}",
		bare.1
	);
	assert!(
		(glyph - (bare.2 + side)).abs() < 0.01,
		"{glyph} {} {side}",
		bare.2
	);
	assert!(
		(after - (bare.3 + 2.0 * side)).abs() < 0.01,
		"{after} {} {side}",
		bare.3
	);
	// Both vertical sides pad the chip without changing the line height.
	assert!(
		(chip.h - (bare.4.h + 2.0 * 0.1 * 18.0)).abs() < 0.01,
		"{chip:?} {:?}",
		bare.4
	);
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

/// The distinct syntax colors of each code line, in source order.
fn code_line_colors(snapshot: &LayoutSnapshot) -> Vec<Vec<u32>> {
	let mut rows: Vec<(f32, Vec<u32>)> = Vec::new();
	for draw in &snapshot.blocks[0].layout.draws {
		let Draw::Glyph(glyph) = draw else {
			continue;
		};
		let Paint::Color(color) = glyph.paint else {
			continue;
		};
		match rows.iter_mut().find(|(y, _)| (y - glyph.y).abs() < 0.5) {
			Some((_, colors)) => colors.push(color.0),
			None => rows.push((glyph.y, vec![color.0])),
		}
	}
	rows.into_iter()
		.map(|(_, mut colors)| {
			colors.sort_unstable();
			colors.dedup();
			colors
		})
		.collect()
}

#[test]
fn a_code_comment_does_not_color_the_lines_after_it() {
	// `syntect` pops a comment scope at the line terminator, so handing it a
	// bare line leaks that comment into every following line of the block.
	let doc =
		document::parse("```python\nalpha = 1\n# a note\nbeta = 2\n```\n");
	let opts = LayoutOptions::default();
	let mut engine = LayoutEngine::new();
	engine.layout(&doc, &opts);
	assert!(engine.wait_highlights(), "highlight jobs must report");
	let lines = code_line_colors(&engine.layout(&doc, &opts));
	assert_eq!(lines.len(), 3, "one entry per code line: {lines:?}");
	assert_eq!(
		lines[2], lines[0],
		"the line after a comment lost its colors: {lines:?}"
	);
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

#[test]
fn compression_moves_the_ink_with_the_blank_it_spends() {
	// A left-side blank — the quarter em before a Han character after Latin, or
	// the blank left half of an opening bracket — has to leave with the glyph
	// when the line is compressed. Shortening only the advance would drag the
	// following character under the box that was just emptied.
	for text in ["abc汉字\n", "abc（中\n"] {
		let wide = lines_of(text, 4000.0, false);
		let total = right_of(&wide[0]);
		let tight = |row: &[(String, Rect)]| {
			row.iter()
				.position(|(t, _)| t == "汉" || t == "（")
				.unwrap_or_else(|| panic!("{text:?}: {row:?}"))
		};
		let doc = document::parse(text);
		let snapshot = LayoutEngine::new().layout(
			&doc,
			&LayoutOptions {
				width: total - 1.0,
				font_size: 18.0,
				..Default::default()
			},
		);
		let rows = drawn_lines(&snapshot);
		assert_eq!(rows.len(), 1, "{text:?}: {rows:?}");
		let glyphs: Vec<f32> = snapshot.blocks[0]
			.layout
			.draws
			.iter()
			.filter_map(|draw| match draw {
				crate::scene::Draw::Glyph(g) => Some(g.x),
				_ => None,
			})
			.collect();
		// Every cluster holds one glyph, drawn left to right.
		assert_eq!(rows[0].len(), glyphs.len(), "{text:?}");
		let at = tight(&rows[0]);
		let (_, rect) = &rows[0][at];
		// The mark fills its em box from the pen to the right edge, so the box
		// must end by the time the next cluster starts.
		assert!(
			glyphs[at] + 18.0 <= rect.x + rect.w + 0.01,
			"{text:?}: the box runs into what follows: {} against {}",
			glyphs[at] + 18.0,
			rect.x + rect.w
		);
		// The blank really was spent, not just nudged.
		let uncompressed = wide[0][tight(&wide[0])].1.w;
		assert!(
			rect.w < uncompressed - 0.1,
			"{text:?}: {rect:?} was not compressed from {uncompressed}"
		);
	}
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
			marker_depth: 0,
			enum_depth: 0,
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
fn a_long_inline_code_run_wraps_instead_of_overflowing() {
	// A code run with no spaces is otherwise one unbreakable box, so it would
	// run past the measure and force the block to scroll. Its own break rule
	// lets it wrap at a character instead.
	let doc = document::parse(
		"Take `a_very_long_identifier_without_any_spaces_at_all` here.\n",
	);
	let snapshot = LayoutEngine::new().layout(
		&doc,
		&LayoutOptions {
			width: 320.0,
			..Default::default()
		},
	);
	let block = &snapshot.blocks[0].layout;
	assert_eq!(snapshot.degraded, 0);
	assert!(
		block.overflow.is_empty(),
		"inline code overflowed: {:?}",
		block.overflow
	);
	let rows = drawn_lines(&snapshot);
	assert!(rows.len() > 1, "the code run did not wrap: {rows:?}");
	// A break inside code is not a hyphenation, so no hyphen is drawn.
	assert!(
		rows.iter().flatten().all(|(text, _)| !text.contains('-')),
		"{rows:?}"
	);
}

#[test]
fn inline_code_breaks_for_free_at_word_edges_and_cheaply_inside_a_word() {
	// Every boundary between two code characters is a break, and none draws a
	// hyphen. Word edges — `foo|=|bar()|+|quz(1,|2)` — are where a whole-word
	// selection stops, so they are free; splitting an identifier or a number
	// still costs a little, which keeps a real word space ahead of it.
	let doc = document::parse(
		"Plain `foo=bar()+quz(1,2)` and `code_span`, `baz`) end.\n",
	);
	let rich = match &doc.blocks[0].kind {
		document::BlockKind::Paragraph(rich) => rich,
		other => panic!("expected a paragraph, found {other:?}"),
	};
	let mut e = LayoutEngine::new();
	let mut out = BlockLayout::default();
	let images = Default::default();
	let mut context = BlockContext {
		shaper: &mut e.shaper,
		math: &mut e.math,
		images: &images,
		highlight_cache: e.highlights.results(),
		marker_depth: 0,
		enum_depth: 0,
	};
	let p = context.prepare(rich, 18.0, &mut out);
	let breaks: Vec<(usize, f64, f32)> = context
		.units(&p, 18.0, false, true, 760.0, Default::default())
		.iter()
		.filter_map(|u| {
			u.after.map(|b| (u.source.end, b.penalty, b.hyphen_width))
		})
		.collect();
	let penalty = |text: &str, start: usize, offset: usize| {
		let end = start + offset;
		let found =
			breaks
				.iter()
				.find(|(at, ..)| *at == end)
				.unwrap_or_else(|| {
					panic!("no break at {text:?}+{offset}: {breaks:?}")
				});
		assert_eq!(found.2, 0.0, "a code break draws no hyphen: {breaks:?}");
		found.1
	};

	let code = "foo=bar()+quz(1,2)";
	let start = p.text.find(code).expect("the code text");
	for offset in 1..code.len() {
		penalty(code, start, offset);
	}
	for offset in [3, 4, 9, 10, 16] {
		assert_eq!(penalty(code, start, offset), 0.0, "free edge in {code:?}");
	}
	for offset in [1, 2, 5, 6, 11, 12] {
		assert_eq!(
			penalty(code, start, offset),
			inline::CODE_BREAK_PENALTY,
			"split inside a word in {code:?}"
		);
	}

	// An identifier has no free edge, so every split of it carries the cost.
	let word = "code_span";
	let start = p.text.find(word).expect("the code text");
	for offset in 1..word.len() {
		assert_eq!(
			penalty(word, start, offset),
			inline::CODE_BREAK_PENALTY,
			"{word:?}"
		);
	}

	// The run's trailing edge is the surrounding text's to decide: a comma or a
	// closing bracket after code keeps the segmenter's prohibition, so it can
	// never be detached onto a line of its own.
	for edge in [word, "baz"] {
		let start = p.text.find(edge).expect("the code text");
		let end = start + edge.len();
		assert!(
			breaks.iter().all(|(at, ..)| *at != end),
			"a break detached {:?}: {breaks:?}",
			&p.text[end..]
		);
	}

	// The plain words keep their ordinary free breaks, and a hyphen still costs
	// more than a code break.
	assert!(
		breaks.iter().any(|(_, penalty, _)| *penalty == 0.0),
		"{breaks:?}"
	);
	assert!(inline::CODE_BREAK_PENALTY < inline::hyphen_penalty(5, 5));
}

#[test]
fn code_never_detaches_following_punctuation() {
	// A code run's trailing edge belongs to the surrounding text, so a comma or
	// a closing bracket after code keeps the segmenter's prohibition and never
	// starts a line, whatever width the column has. Greedy breaking takes the
	// farthest break that fits, so it would detach the mark if one were offered.
	let doc = document::parse(
		"A sentence with `some_identifier`, then `baz`) and more words.\n",
	);
	for greedy in [false, true] {
		for width in (120..=760).step_by(4) {
			let width = width as f32;
			let snapshot = LayoutEngine::new().layout(
				&doc,
				&LayoutOptions {
					width,
					greedy,
					..Default::default()
				},
			);
			for row in drawn_lines(&snapshot) {
				let first =
					row.first().map(|(text, _)| text.as_str()).unwrap_or("");
				assert!(
					!first.starts_with(',') && !first.starts_with(')'),
					"greedy={greedy} width={width}: {row:?}"
				);
			}
		}
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
			marker_depth: 0,
			enum_depth: 0,
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
fn a_quote_break_keeps_the_punctuation_prohibition() {
	// Relaxing the quotation-mark rule must not let a break through that the
	// surrounding punctuation forbids on its own.
	let firsts = |source: &str, width: f32| -> Vec<char> {
		lines_of(source, width, false)
			.iter()
			.filter_map(|row| {
				row.iter()
					.map(|(t, _)| t.as_str())
					.collect::<String>()
					.chars()
					.next()
			})
			.collect()
	};
	let lasts = |source: &str, width: f32| -> Vec<char> {
		lines_of(source, width, false)
			.iter()
			.filter_map(|row| {
				row.iter()
					.map(|(t, _)| t.as_str())
					.collect::<String>()
					.chars()
					.next_back()
			})
			.collect()
	};
	// A closing quote may not hand a full stop — or a closing bracket — to the
	// next line.
	let text = "这是一个“中文的测试例子”。这是中文。这是中文。\n";
	let starts = firsts(text, 213.0);
	assert!(
		!starts.iter().any(|c| matches!(c, '。' | '，' | '）' | '”')),
		"{starts:?}"
	);
	// An opening quote may not strand the opening bracket before it.
	let text = "中文（“引言”）测试文字，补充足够的字符以便换行处理。\n";
	for width in [60.0, 108.0, 144.0] {
		let ends = lasts(text, width);
		assert!(!ends.iter().any(|c| matches!(c, '（' | '“')), "{ends:?}");
		let starts = firsts(text, width);
		assert!(
			!starts.iter().any(|c| matches!(c, '）' | '，' | '。' | '”')),
			"{starts:?}"
		);
	}
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
		marker_depth: 0,
		enum_depth: 0,
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
	// Wide enough that the first line reaches its first comma under the pinned
	// test face, so the closing mark is what hangs.
	let width = 220.0;
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

#[test]
fn a_localized_edit_reuses_every_unchanged_block() {
	// More than the old 256-entry cap, with distinct content throughout, so a
	// retained cache is the only thing that can reuse them.
	let mut source = String::new();
	for i in 0..300 {
		source.push_str(&format!(
			"Paragraph number {i} with distinct words.\n\n"
		));
	}
	let opts = LayoutOptions::default();
	let mut engine = LayoutEngine::new();
	let before = engine.layout(&document::parse(source.clone()), &opts);
	assert_eq!(before.blocks.len(), 300);
	assert_eq!(before.reused, 0);
	let edited =
		document::parse(source.replace("number 150 ", "number 150 edited "));
	let after = engine.layout(&edited, &opts);
	assert_eq!(after.reused, before.blocks.len() - 1);
}

#[test]
fn syntax_colors_settle_without_thrashing_the_block_cache() {
	// One entry per code block, past the 256-entry point where the highlight
	// cache used to clear itself and re-enqueue all of them forever.
	let mut source = String::new();
	for i in 0..300 {
		source.push_str(&format!("```rust\nlet value_{i} = {i};\n```\n\n"));
	}
	let doc = document::parse(source);
	let opts = LayoutOptions::default();
	let mut engine = LayoutEngine::new();
	assert_eq!(engine.layout(&doc, &opts).reused, 0);
	assert!(engine.wait_highlights(), "highlight jobs must report");
	// Each block gains its colors once, then keeps them across passes.
	assert_eq!(engine.layout(&doc, &opts).reused, 0);
	assert_eq!(engine.layout(&doc, &opts).reused, doc.blocks.len());
}

/// Layout options that force the `<details>` block with `id` to `open`.
fn with_details(
	mut options: LayoutOptions,
	id: u64,
	open: bool,
) -> LayoutOptions {
	let mut map = std::collections::BTreeMap::new();
	map.insert(id, open);
	options.details_open = Arc::new(map);
	options
}

/// Every reading text node of a snapshot, in document order.
fn reading_text(snapshot: &LayoutSnapshot) -> String {
	let Some(selection) = snapshot.select_all(1) else {
		return String::new();
	};
	snapshot.extract_text(selection, 1)
}

const DETAILS_DOC: &str = "<details>\n<summary>More</summary>\n\nHidden **body** text here.\n\n</details>\n\nAfter.\n";

#[test]
fn collapsed_details_lays_out_no_body() {
	let doc = document::parse(DETAILS_DOC);
	let id = doc.blocks[0].id;
	let mut engine = LayoutEngine::new();
	let collapsed = engine.layout(&doc, &LayoutOptions::default());
	assert!(reading_text(&collapsed).contains("More"));
	assert!(!reading_text(&collapsed).contains("Hidden"));
	assert!(
		collapsed.blocks[0]
			.layout
			.text
			.iter()
			.all(|node| !node.text.contains("Hidden"))
	);
	let expanded =
		engine.layout(&doc, &with_details(LayoutOptions::default(), id, true));
	assert!(reading_text(&expanded).contains("Hidden"));
	assert!(expanded.height > collapsed.height);
}

#[test]
fn details_toggle_is_stable_and_reuses_other_blocks() {
	let doc = document::parse(DETAILS_DOC);
	let id = doc.blocks[0].id;
	let closed = LayoutOptions::default();
	let mut engine = LayoutEngine::new();
	let collapsed = engine.layout(&doc, &closed);
	let expanded = engine.layout(&doc, &with_details(closed.clone(), id, true));
	// Only the toggled block is re-laid out; the block after it is reused.
	assert_eq!(expanded.reused, 1);
	let again = engine.layout(&doc, &closed);
	assert_eq!(again.height, collapsed.height);
	assert_eq!(reading_text(&again), reading_text(&collapsed));
	let reopened = engine.layout(&doc, &with_details(closed, id, true));
	assert_eq!(reopened.height, expanded.height);
}

#[test]
fn details_summary_is_hit_testable_but_the_body_is_not() {
	let doc = document::parse(DETAILS_DOC);
	let id = doc.blocks[0].id;
	let url = document::details_url(id);
	let none = HashMap::new();
	let mut engine = LayoutEngine::new();
	let collapsed = engine.layout(&doc, &LayoutOptions::default());
	let block = &collapsed.blocks[0];
	assert_eq!(collapsed.blocks[0].layout.links.len(), 1);
	let hit = block.layout.links[0].rect;
	assert_eq!(
		collapsed.link_at(hit.x + 1.0, block.y + hit.y + 1.0, &none),
		Some(url.as_str())
	);
	// A collapsed element is only its summary line, so nothing below it hits.
	assert_eq!(
		collapsed.link_at(100.0, block.y + block.layout.height + 4.0, &none),
		None
	);
	let expanded =
		engine.layout(&doc, &with_details(LayoutOptions::default(), id, true));
	let block = &expanded.blocks[0];
	let hit = block.layout.links[0].rect;
	assert_eq!(
		expanded.link_at(hit.x + 1.0, block.y + hit.y + 1.0, &none),
		Some(url.as_str())
	);
	assert_eq!(
		expanded.link_at(100.0, block.y + block.layout.height - 2.0, &none),
		None
	);
}

#[test]
fn anchors_after_a_collapsed_details_resolve() {
	let doc = document::parse(
		"<details>\n<summary>More</summary>\n\nHidden body.\n\n</details>\n\n# Later\n",
	);
	let id = doc.blocks[0].id;
	let mut engine = LayoutEngine::new();
	let collapsed = engine.layout(&doc, &LayoutOptions::default());
	assert_eq!(collapsed.blocks.len(), 2);
	let at = collapsed.anchor_y("later").expect("the heading follows");
	assert!(at >= collapsed.blocks[1].y);
	let expanded =
		engine.layout(&doc, &with_details(LayoutOptions::default(), id, true));
	assert!(expanded.anchor_y("later").unwrap() > at);
	// The heading sits inside the body it is nested in, not after it.
	assert!(collapsed.blocks.iter().all(|block| {
		block.layout.anchors.iter().all(|a| a.anchor != "nested")
	}));
}

#[test]
fn a_details_open_attribute_starts_expanded() {
	let doc = document::parse(
		"<details open>\n<summary>More</summary>\n\nShown body.\n\n</details>\n",
	);
	let id = doc.blocks[0].id;
	let mut engine = LayoutEngine::new();
	let snapshot = engine.layout(&doc, &LayoutOptions::default());
	assert!(reading_text(&snapshot).contains("Shown body"));
	// The reader can override the source and collapse it again.
	let collapsed =
		engine.layout(&doc, &with_details(LayoutOptions::default(), id, false));
	assert!(!reading_text(&collapsed).contains("Shown body"));
	assert!(collapsed.height < snapshot.height);
}

#[test]
fn nested_details_toggle_invalidates_its_container() {
	let doc = document::parse(
		"<details open>\n<summary>Outer</summary>\n\n<details><summary>Inner</summary>Deep</details>\n\n</details>\n\nAfter.\n",
	);
	let document::BlockKind::Details { blocks, .. } = &doc.blocks[0].kind
	else {
		panic!("expected the outer details")
	};
	let inner = blocks[0].id;
	let mut engine = LayoutEngine::new();
	let collapsed = engine.layout(&doc, &LayoutOptions::default());
	assert!(!reading_text(&collapsed).contains("Deep"));
	let expanded = engine
		.layout(&doc, &with_details(LayoutOptions::default(), inner, true));
	assert!(reading_text(&expanded).contains("Deep"));
	assert!(expanded.height > collapsed.height);
	// The container that frames the toggled element is laid out again; only
	// the trailing block is reused.
	assert_eq!(expanded.reused, 1);
}

#[test]
fn identical_details_toggle_independently() {
	let source = "<details>\n<summary>Same</summary>\n\nBody\n\n</details>\n\n<details>\n<summary>Same</summary>\n\nBody\n\n</details>\n";
	let doc = document::parse(source);
	let first = doc.blocks[0].id;
	let second = doc.blocks[1].id;
	assert_ne!(first, second);
	let mut engine = LayoutEngine::new();
	let one = engine
		.layout(&doc, &with_details(LayoutOptions::default(), first, true));
	assert_eq!(reading_text(&one).matches("Body").count(), 1);
	// Matching states no longer share geometry: each element's identity is
	// part of the key, because the placed links bind a summary to its own id.
	let both = LayoutOptions {
		details_open: Arc::new(
			[(first, true), (second, true)].into_iter().collect(),
		),
		..Default::default()
	};
	let same = engine.layout(&doc, &both);
	assert_eq!(reading_text(&same).matches("Body").count(), 2);
	assert_eq!(same.reused, 1);
	// A second pass with the same states reuses both, now that each has its
	// own entry.
	assert_eq!(engine.layout(&doc, &both).reused, 2);
}

#[test]
fn identical_details_links_point_at_the_element_that_was_hit() {
	let source = "<details>\n<summary>Same</summary>\n\nBody\n\n</details>\n\n<details>\n<summary>Same</summary>\n\nBody\n\n</details>\n";
	let doc = document::parse(source);
	let mut engine = LayoutEngine::new();
	let options = LayoutOptions {
		details_open: Arc::new(
			[(doc.blocks[0].id, true), (doc.blocks[1].id, true)]
				.into_iter()
				.collect(),
		),
		..Default::default()
	};
	let snapshot = engine.layout(&doc, &options);
	let empty = HashMap::new();
	for (i, block) in snapshot.blocks.iter().enumerate() {
		let link = block
			.layout
			.links
			.iter()
			.find(|link| link.url.starts_with(document::DETAILS_SCHEME))
			.unwrap_or_else(|| panic!("block {i} has no summary link"));
		assert_eq!(document::details_id(&link.url), Some(doc.blocks[i].id));
		let rect = link.rect;
		let hit = snapshot.link_at(
			rect.x + rect.w * 0.5,
			block.y + rect.y + rect.h * 0.5,
			&empty,
		);
		assert_eq!(
			hit.and_then(document::details_id),
			Some(doc.blocks[i].id),
			"clicking block {i} must toggle block {i}"
		);
	}
}

#[test]
fn identical_nested_details_bind_their_own_summaries() {
	let source = "<details open>\n<summary>Outer</summary>\n\n<details open><summary>Inner</summary>Deep</details>\n\n</details>\n\n<details open>\n<summary>Outer</summary>\n\n<details open><summary>Inner</summary>Deep</details>\n\n</details>\n";
	let doc = document::parse(source);
	let nested = |i: usize| {
		let document::BlockKind::Details { blocks, .. } = &doc.blocks[i].kind
		else {
			panic!("expected an outer details")
		};
		blocks[0].id
	};
	let snapshot = LayoutEngine::new().layout(&doc, &LayoutOptions::default());
	for i in [0, 1] {
		let urls: Vec<&str> = snapshot.blocks[i]
			.layout
			.links
			.iter()
			.map(|link| link.url.as_str())
			.collect();
		let own = document::details_url(doc.blocks[i].id);
		let inner = document::details_url(nested(i));
		let other = document::details_url(nested(1 - i));
		assert!(urls.contains(&own.as_str()), "{urls:?}");
		assert!(urls.contains(&inner.as_str()), "{urls:?}");
		assert!(!urls.contains(&other.as_str()), "{urls:?}");
	}
}

#[test]
fn a_details_body_does_not_inherit_the_summary_appearance() {
	let mut sheet = (*crate::style::Stylesheet::bundled(false)).clone();
	sheet.merge(
		&crate::style::Stylesheet::parse(
			"format_version=2\nversion=1\n[[rule]]\nwhen=['summary']\nsize=2.0",
		)
		.unwrap(),
	);
	let options = LayoutOptions {
		stylesheet: Arc::new(sheet),
		..Default::default()
	};
	let doc = document::parse(
		"<details open>\n<summary>Head</summary>\n\nBody text.\n\n</details>\n",
	);
	let snapshot = LayoutEngine::new().layout(&doc, &options);
	let sizes: Vec<f32> = snapshot.blocks[0]
		.layout
		.draws
		.iter()
		.filter_map(|draw| match draw {
			Draw::Glyph(glyph) => Some(glyph.size),
			_ => None,
		})
		.collect();
	assert!(sizes.contains(&36.0), "summary size missing: {sizes:?}");
	assert!(sizes.contains(&18.0), "body size missing: {sizes:?}");
}

#[test]
fn disclosure_hover_ranges_are_ordered_and_bounded() {
	/// The draw commands the renderer marks hovered for `url`.
	fn hovered(layout: &BlockLayout, url: &str) -> Vec<usize> {
		(0..layout.draws.len())
			.filter(|&i| {
				layout.links.iter().enumerate().any(|(n, link)| {
					link.url == url
						&& link.command <= i
						&& layout
							.links
							.get(n + 1)
							.map_or(i < layout.draws.len(), |next| {
								i < next.command
							})
				})
			})
			.collect()
	}
	let doc = document::parse(
		"<details open>\n<summary>Summary</summary>\n\nBody [link](https://example.com/b).\n\n</details>\n",
	);
	let id = doc.blocks[0].id;
	let snapshot = LayoutEngine::new()
		.layout(&doc, &with_details(LayoutOptions::default(), id, true));
	let block = &snapshot.blocks[0];
	// Every range ends where the next link's command begins, so no range is
	// inverted.
	assert!(
		block
			.layout
			.links
			.windows(2)
			.all(|w| w[0].command <= w[1].command),
		"{:?}",
		block.layout.links
	);
	let summary = hovered(&block.layout, &document::details_url(id));
	let body = hovered(&block.layout, "https://example.com/b");
	assert!(!summary.is_empty());
	assert!(!body.is_empty(), "the body link never highlights");
	// The summary range stops at the first body command, so pointing into the
	// content cannot highlight it.
	assert!(summary.iter().all(|i| *i < block.layout.links[1].command));
	assert!(summary.iter().all(|i| !body.contains(i)));
}

#[test]
fn a_details_body_reference_link_lays_out_as_a_link() {
	// The definition follows the element, so the body only becomes a link when
	// its parse carried the document's reference context.
	let doc = document::parse(
		"<details open>\n<summary>Summary</summary>\n\nBody [link][ref].\n\n</details>\n\n[ref]: https://example.com/b\n",
	);
	let id = doc.blocks[0].id;
	let snapshot = LayoutEngine::new()
		.layout(&doc, &with_details(LayoutOptions::default(), id, true));
	let urls: Vec<&str> = snapshot.blocks[0]
		.layout
		.links
		.iter()
		.map(|link| link.url.as_str())
		.collect();
	assert!(urls.contains(&"https://example.com/b"), "{urls:?}");
}

#[test]
fn a_diagram_theme_change_is_a_new_layout_request() {
	let options = |mermaid: &str| LayoutOptions {
		stylesheet: Arc::new(
			crate::style::Stylesheet::parse(&format!(
				"format_version=2\nversion=1\n[mermaid]\n{mermaid}"
			))
			.unwrap(),
		),
		..Default::default()
	};
	let dark = options("theme='dark'");
	assert_eq!(dark, options("theme='dark'"));
	// Colors are not geometry, but the image scheduler keys work by these
	// options, so a new theme must look like a new request.
	assert_ne!(dark, options("theme='dark'\nbackground='#101418'"));
}

#[test]
fn a_named_font_definition_is_part_of_the_diagram_request() {
	let options = |lookfor: &str| LayoutOptions {
		stylesheet: Arc::new(
			crate::style::Stylesheet::parse(&format!(
				"format_version=2\nversion=1\n\
				 [[fontdef]]\nid='reading'\nlookfor=[{lookfor}]\n\
				 [mermaid]\nfont_family=['reading']"
			))
			.unwrap(),
		),
		..Default::default()
	};
	// The rules are identical, so only the definition the diagram names can
	// tell these two requests apart.
	assert_eq!(
		options("'Noto Serif'").stylesheet.layout_key(),
		options("'Source Han Serif'").stylesheet.layout_key()
	);
	assert_eq!(options("'Noto Serif'"), options("'Noto Serif'"));
	assert_ne!(options("'Noto Serif'"), options("'Source Han Serif'"));
}
