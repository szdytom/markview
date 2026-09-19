use super::*;
#[test]
fn gfm_and_raw_html() {
	let doc = parse(
		"# 中文\n\n- [x] done\n- [ ] todo\n\n| A | B |\n|:-|--:|\n| x | $x^2$ |\n\n~~gone~~ https://example.com <b>raw</b>\n",
	);
	assert_eq!(doc.blocks.len(), 4);
	let BlockKind::List { items, .. } = &doc.blocks[1].kind else {
		panic!()
	};
	assert_eq!(items[0].checked, Some(true));
	assert_eq!(items[1].checked, Some(false));
	let BlockKind::Table { align, .. } = &doc.blocks[2].kind else {
		panic!()
	};
	assert_eq!(align[1], CellAlign::Right);
	let BlockKind::Paragraph(p) = &doc.blocks[3].kind else {
		panic!()
	};
	assert!(p.iter().any(|s| s.style.strike));
	assert!(
		p.iter()
			.any(|s| s.style.link.as_deref() == Some("https://example.com"))
	);
	assert!(p.iter().any(|s| s.style.bold
		&& matches!(&s.kind, InlineKind::Text(t) if t == "raw")));
}
#[test]
fn html_comments_disappear_and_attributes_are_ignored() {
	let doc = parse(
		"A <!-- hidden --> B <b class=\"x\" style=\"y\">bold</b> <em>i</em> <del>d</del> <code>c</code> <sup>s</sup> <a href=\"/u\">l</a>.\n",
	);
	let BlockKind::Paragraph(p) = &doc.blocks[0].kind else {
		panic!()
	};
	assert_eq!(plain_text(p), "A B bold i d c s l.");
	let style = |text: &str| {
		p.iter()
			.find(|s| matches!(&s.kind, InlineKind::Text(t) if t == text))
			.unwrap_or_else(|| panic!("missing {text}"))
			.style
			.clone()
	};
	assert!(style("bold").bold);
	assert!(style("i").italic);
	assert!(style("d").strike);
	assert!(style("c").code);
	assert!(style("s").superscript);
	assert_eq!(style("l").link.as_deref(), Some("/u"));
}
#[test]
fn html_blocks_become_rule_heading_and_paragraph() {
	let doc = parse(
		"<h2>Title <em>here</em></h2>\n\n<hr>\n\n<p>Body</p>\n\n<!-- gone -->\n",
	);
	assert_eq!(doc.blocks.len(), 3);
	let BlockKind::Heading { level, text, .. } = &doc.blocks[0].kind else {
		panic!()
	};
	assert_eq!(*level, 2);
	assert_eq!(plain_text(text), "Title here");
	assert!(text.iter().any(|s| s.style.italic));
	assert!(matches!(doc.blocks[1].kind, BlockKind::Rule));
	assert!(
		matches!(&doc.blocks[2].kind, BlockKind::Paragraph(p) if plain_text(p) == "Body")
	);
}
#[test]
fn unsupported_html_keeps_the_source() {
	let doc = parse("<div class=\"x\">\n\nspan <span>s</span>\n");
	assert!(matches!(
		&doc.blocks[0].kind,
		BlockKind::Code { language, text }
			if language == "HTML source" && text.contains("div")
	));
	let BlockKind::Paragraph(p) = &doc.blocks[1].kind else {
		panic!()
	};
	assert!(plain_text(p).contains("<span>s</span>"));
}
#[test]
fn deeply_nested_emphasis_is_bounded_and_keeps_the_text() {
	// Regression for a 12 KB document that used to abort the process: comrak
	// builds this AST iteratively, but Markview used to walk it recursively.
	let n = 6000;
	let doc = parse(format!("{}a{}", "*".repeat(n), "*".repeat(n)));
	assert_eq!(doc.blocks.len(), 1);
	let BlockKind::Paragraph(text) = &doc.blocks[0].kind else {
		panic!("expected a paragraph")
	};
	assert_eq!(plain_text(text), "a");
}
#[test]
fn heading_slugs_follow_the_github_rules() {
	assert_eq!(heading_slug("Getting Started"), "getting-started");
	assert_eq!(heading_slug("Hello, World!"), "hello-world");
	assert_eq!(heading_slug("C++ & Rust"), "c--rust");
	assert_eq!(heading_slug("  spaced  out  "), "--spaced--out--");
	assert_eq!(heading_slug("snake_case-name"), "snake_case-name");
	assert_eq!(heading_slug("中文标题"), "中文标题");
	assert_eq!(heading_slug("Привет 你好"), "привет-你好");
	assert_eq!(heading_slug("😄 emoji"), "-emoji");
}

#[test]
fn headings_carry_anchors_and_repeats_get_suffixes() {
	let doc = parse(
		"# Getting Started\n\n## Getting Started\n\n### 中文 标题\n\n> ## Nested Heading\n",
	);
	let anchors: Vec<&str> = doc.blocks[..3]
		.iter()
		.map(|b| match &b.kind {
			BlockKind::Heading { anchor, .. } => anchor.as_str(),
			_ => panic!("expected a top-level heading"),
		})
		.collect();
	assert_eq!(
		anchors,
		["getting-started", "getting-started-1", "中文-标题"]
	);
	let BlockKind::Quote { blocks, .. } = &doc.blocks[3].kind else {
		panic!()
	};
	let BlockKind::Heading { anchor, .. } = &blocks[0].kind else {
		panic!()
	};
	assert_eq!(anchor, "nested-heading");
	// Suffixes keep counting past a repeated base, and a heading whose own slug
	// already ends in a number does not steal the next suffix.
	let doc = parse("# Same\n\n# Same\n\n# Same\n\n# Same-1\n");
	let anchors: Vec<&str> = doc
		.blocks
		.iter()
		.map(|b| match &b.kind {
			BlockKind::Heading { anchor, .. } => anchor.as_str(),
			_ => panic!("expected a top-level heading"),
		})
		.collect();
	assert_eq!(anchors, ["same", "same-1", "same-2", "same-1-1"]);
}

#[test]
fn content_id_tracks_semantics_not_source_spelling() {
	assert_eq!(
		parse("Hello **world**\n").content_id,
		parse("Hello __world__\n").content_id
	);
	assert_ne!(
		parse("Hello\n").content_id,
		parse("Hello there\n").content_id
	);
	assert_ne!(parse("A\n\nB\n").content_id, parse("B\n\nA\n").content_id);
}
#[test]
fn identity_survives_insertion_and_ranges_are_utf8() {
	let a = parse("你好 **world**\n");
	let b = parse("New paragraph.\n\n你好 **world**\n");
	assert_eq!(a.blocks[0].id, b.blocks[1].id);
	assert_eq!(&b.source[b.blocks[1].source.clone()], "你好 **world**");
}
#[test]
fn incomplete_fence_and_math_do_not_drop_text() {
	let d = parse("```rust\nlet x = 1;\n");
	assert!(
		matches!(&d.blocks[0].kind, BlockKind::Code { text, .. } if text.contains("let x"))
	);
	let d = parse("Cost \\$5, unfinished $x\n");
	assert!(
		matches!(&d.blocks[0].kind, BlockKind::Paragraph(p) if plain_text(p).contains("$x"))
	);
}
#[test]
fn tab_indented_fence_in_a_list_keeps_its_columns() {
	// A list marker consumes two columns of the leading tab, so the fence's
	// indentation is a column count; measuring it in bytes left the rest of
	// the tab behind as a leading space in the code block.
	let d = parse(
		"- item:\n\n\t```text\n\t<type>: <short, lowercase summary>\n\t```\n",
	);
	let BlockKind::List { items, .. } = &d.blocks[0].kind else {
		panic!("expected a list")
	};
	let BlockKind::Code { text, .. } = &items[0].blocks[1].kind else {
		panic!("expected a code block")
	};
	assert_eq!(text, "<type>: <short, lowercase summary>\n");
}
#[test]
fn mermaid_fences_become_diagram_images_for_both_fence_styles() {
	// The info string's first word selects the language; the rest is ignored,
	// so `mermaid title="x"` is still a diagram.
	let d = parse(
		"```mermaid title=\"x\"\ngraph TD\n A-->B\n```\n\n~~~mermaid\nsequenceDiagram\n~~~\n",
	);
	assert_eq!(d.blocks.len(), 2);
	for block in &d.blocks {
		let BlockKind::Paragraph(text) = &block.kind else {
			panic!("expected a diagram paragraph")
		};
		let [
			Inline {
				kind: InlineKind::Image(image),
				..
			},
		] = text.as_slice()
		else {
			panic!("expected one image")
		};
		assert!(image.src.starts_with(crate::image::MERMAID_SCHEME));
	}
	let BlockKind::Paragraph(first) = &d.blocks[0].kind else {
		panic!()
	};
	let InlineKind::Image(image) = &first[0].kind else {
		panic!()
	};
	assert!(image.src.contains("A-->B"));
}

#[test]
fn fences_that_merely_mention_mermaid_stay_code() {
	let d = parse("```mermaidish\nnot a diagram\n```\n");
	let BlockKind::Code { language, text } = &d.blocks[0].kind else {
		panic!("expected a code block")
	};
	assert_eq!(language, "mermaidish");
	assert_eq!(text, "not a diagram\n");
}

#[test]
fn a_diagram_reads_as_its_source_without_a_caption() {
	use crate::style::CaptionSource;
	let d = parse("```mermaid\ngraph TD\n A-->B\n```\n");
	let BlockKind::Paragraph(p) = &d.blocks[0].kind else {
		panic!("expected a diagram paragraph")
	};
	let InlineKind::Image(image) = &p[0].kind else {
		panic!("expected one image")
	};
	// The fence source is semantic reading text, so selecting or copying the
	// diagram keeps it even though the image draws no caption.
	assert_eq!(image.reading.as_deref(), Some("graph TD\n A-->B\n"));
	assert_eq!(plain_text(p), "graph TD\n A-->B\n");
	assert!(image.alt.is_empty() && image.title.is_empty());
	for source in [
		CaptionSource::Alt,
		CaptionSource::Title,
		CaptionSource::TitleOrAlt,
	] {
		assert_eq!(source.text(image), None, "{source:?}");
	}
}

#[test]
fn latex_delimiters_produce_math() {
	let d = parse("Inline \\(a+b\\) and display \\[c+d\\].\n");
	let BlockKind::Paragraph(p) = &d.blocks[0].kind else {
		panic!()
	};
	assert!(p.iter().any(|s| matches!(
		&s.kind,
		InlineKind::Math { latex, display: false } if latex == "a+b"
	)));
	assert!(p.iter().any(|s| matches!(
		&s.kind,
		InlineKind::Math { latex, display: true } if latex == "c+d"
	)));
}
#[test]
fn cjk_friendly_emphasis_closes_next_to_cjk_text() {
	let d = parse("**この文は重要です。**但这句话并不重要。\n");
	let BlockKind::Paragraph(p) = &d.blocks[0].kind else {
		panic!()
	};
	assert_eq!(
		plain_text(p),
		"この文は重要です。但这句话并不重要。",
		"the closing run must not leak into the text"
	);
	assert!(p.iter().any(|s| s.style.bold
		&& matches!(&s.kind, InlineKind::Text(t) if t == "この文は重要です。")));
	assert!(p.iter().any(|s| !s.style.bold
		&& matches!(&s.kind, InlineKind::Text(t) if t == "但这句话并不重要。")));
}
#[test]
fn resolved_references_invalidate_semantics_and_footnotes_use_numbers() {
	let a = parse("A [link][id].\n\n[id]: https://one.example\n");
	let b = parse("A [link][id].\n\n[id]: https://two.example\n");
	assert_eq!(a.blocks[0].id, b.blocks[0].id);
	assert_ne!(a.blocks[0].content_key, b.blocks[0].content_key);
	let d = parse("See [^name].\n\n[^name]: The footnote.\n");
	let BlockKind::Paragraph(p) = &d.blocks[0].kind else {
		panic!()
	};
	assert!(plain_text(p).contains("[1]"));
	// The reference is a footnote jump, not a link, so it keeps its own look.
	let reference = p
		.iter()
		.find(|i| matches!(i.kind, InlineKind::FootnoteRef(1)))
		.expect("footnote reference");
	assert_eq!(reference.style.link.as_deref(), Some("#fn:1"));
	assert!(reference.style.footnote_ref && reference.style.superscript);
	assert!(
		!reference
			.style
			.conditions()
			.any(|c| c == crate::style::Condition::Link)
	);
	assert!(d.blocks.iter().any(
		|b| matches!(&b.kind, BlockKind::Footnote { label, .. } if label == "1")
	));
}

#[test]
fn a_line_break_survives_as_a_break_not_as_text() {
	// A hard break and an explicit `<br>` both read as one line break, so
	// copying a document keeps the line the author wrote.
	use crate::document::plain_text;
	let doc = parse("one  \ntwo<br>three\n");
	let BlockKind::Paragraph(rich) = &doc.blocks[0].kind else {
		panic!("not a paragraph");
	};
	assert_eq!(plain_text(rich), "one\ntwo\nthree");
	// Breaking is not the same as writing the character.
	assert!(
		rich.iter().any(|i| matches!(
			&i.kind,
			InlineKind::LineBreak { justify: false }
		))
	);
	assert!(
		rich.iter().any(|i| matches!(
			&i.kind,
			InlineKind::LineBreak { justify: true }
		))
	);
}

/// An edit the incremental path must handle: the result has to equal a full
/// parse block for block, source range for source range.
fn assert_incremental(before: &str, after: &str) {
	let previous = parse(before);
	let got = parse_incremental(&previous, Arc::from(after))
		.unwrap_or_else(|| panic!("no fast path for {before:?} -> {after:?}"));
	let expected = parse(after);
	assert_eq!(
		got.content_id, expected.content_id,
		"{before:?} -> {after:?}"
	);
	assert_eq!(got.blocks, expected.blocks, "{before:?} -> {after:?}");
}

/// Every edit, whether or not it takes the fast path, must be correct.
fn assert_edit(before: &str, after: &str) {
	let previous = parse(before);
	let got = parse_incremental(&previous, Arc::from(after))
		.unwrap_or_else(|| parse(after));
	let expected = parse(after);
	assert_eq!(
		got.content_id, expected.content_id,
		"{before:?} -> {after:?}"
	);
	assert_eq!(got.blocks, expected.blocks, "{before:?} -> {after:?}");
}

#[test]
fn reparse_reuses_the_fast_path_and_falls_back_to_a_full_parse() {
	let previous = parse(PARAGRAPHS);
	let edited = PARAGRAPHS.replace("beta", "betaX");
	let got = reparse(&previous, Arc::from(edited.as_str()));
	assert_eq!(got.blocks, parse(edited.as_str()).blocks);
	// A list keeps its meaning across blank lines, so the edit takes the full
	// parse and still matches one.
	let replaced = reparse(&parse("- one\n- two\n"), Arc::from(PARAGRAPHS));
	assert_eq!(replaced.blocks, parse(PARAGRAPHS).blocks);
}

const PARAGRAPHS: &str =
	"# Title\n\nAlpha beta gamma.\n\nDelta epsilon zeta.\n\nEta theta iota.\n";

#[test]
fn incremental_parse_matches_a_full_parse_for_plain_edits() {
	assert_incremental(PARAGRAPHS, &PARAGRAPHS.replace("beta", "betaX"));
	assert_incremental(PARAGRAPHS, &PARAGRAPHS.replace("epsilon ", ""));
	assert_incremental(PARAGRAPHS, &PARAGRAPHS.replace("# Title", "# Titles"));
	assert_incremental(PARAGRAPHS, &PARAGRAPHS.replace("iota.", "iota!"));
	// A new line inside a paragraph, and a new paragraph at the end.
	assert_incremental(
		PARAGRAPHS,
		&PARAGRAPHS.replace("gamma.", "gamma.\nMore."),
	);
	assert_incremental(PARAGRAPHS, &format!("{PARAGRAPHS}\nKappa lambda.\n"));
	// Insertion at the very front.
	assert_incremental(PARAGRAPHS, &format!("Start. {PARAGRAPHS}"));
	// An appended line joins the last paragraph rather than starting one.
	assert_edit(
		PARAGRAPHS,
		&PARAGRAPHS.replace("iota.\n", "iota.\nKappa.\n"),
	);
}

#[test]
fn incremental_parse_keeps_heading_anchors_unique() {
	let before = "# Same\n\nOne.\n\n# Same\n\nTwo.\n";
	assert_incremental(before, "# Same\n\nOne.\n\n# Same\n\nTwo!\n");
	// A new copy of a heading renumbers the ones after it.
	assert_incremental(before, "# Same\n\nOne.\n\n# Same\n\nTwo.\n\n# Same\n");
	// Renaming a heading frees its slug for a later one.
	assert_incremental(before, "# Renamed\n\nOne.\n\n# Same\n\nTwo.\n");
}

#[test]
fn incremental_parse_handles_repeated_edits() {
	let mut source = String::from(PARAGRAPHS);
	for step in 0..8 {
		let next = source.replace("Alpha", &format!("Alpha{step} "));
		assert_edit(&source, &next);
		source = next;
	}
}

#[test]
fn block_constructs_fall_back_to_a_full_parse() {
	let plain = "# Title\n\nAlpha.\n\nBeta.\n";
	for after in [
		"- item\n\nAlpha.\n\nBeta.\n",
		"> quote\n\nAlpha.\n\nBeta.\n",
		"```rust\nlet x = 1;\n```\n\nAlpha.\n",
		"    indented code\n\nAlpha.\n",
		"<div>raw</div>\n\nAlpha.\n",
		"| A | B |\n|:-|--:|\n| 1 | 2 |\n\nAlpha.\n",
		"[id]: https://example.com\n\nAlpha.\n",
		"Alpha[^note].\n\n[^note]: Note.\n",
	] {
		assert!(
			parse_incremental(&parse(plain), Arc::from(after)).is_none(),
			"expected a fallback for {after:?}"
		);
		assert_edit(plain, after);
	}
	// A document that only becomes plain still falls back the first time.
	assert!(parse_incremental(&parse("- item\n"), Arc::from(plain)).is_none());
}

#[test]
fn incremental_parse_matches_a_full_parse_under_random_edits() {
	for seed in [
		0x2545_f491_4f6c_dd1du64,
		0x9e37_79b9_7f4a_7c15,
		0xdead_beef_cafe_f00d,
	] {
		// A deterministic xorshift keeps the corpus reproducible.
		let mut state = seed;
		let mut next = move || {
			state ^= state << 13;
			state ^= state >> 7;
			state ^= state << 17;
			state
		};
		let words =
			["alpha", "beta", "gamma", "中文", "delta", "epsilon", "zeta"];
		let mut source = String::from("# Title\n\n");
		for _ in 0..10 {
			if next() % 4 == 0 {
				source.push_str("## Same\n\n");
			}
			source.push_str(words[(next() % 7) as usize]);
			source.push(' ');
			source.push_str(words[(next() % 7) as usize]);
			source.push_str(".\n\n");
		}
		let inserts = ["x", "新增", "\n", "\n\n", "# ", " ", "😀"];
		let mut fast = 0;
		let mut edits = 0;
		for _ in 0..400 {
			let mut after = source.clone();
			let bounds: Vec<usize> = after
				.char_indices()
				.map(|(i, _)| i)
				.chain(std::iter::once(after.len()))
				.collect();
			let at = bounds[(next() as usize) % bounds.len()];
			if next() % 3 != 0 {
				after
					.insert_str(at, inserts[(next() as usize) % inserts.len()]);
			} else {
				let count = after.chars().count();
				if count == 0 {
					continue;
				}
				let which = (next() as usize) % count;
				let start = after.char_indices().nth(which).unwrap().0;
				let end =
					start + after[start..].chars().next().unwrap().len_utf8();
				after.replace_range(start..end, "");
			}
			edits += 1;
			if parse_incremental(
				&parse(source.as_str()),
				Arc::from(after.as_str()),
			)
			.is_some()
			{
				fast += 1;
			}
			assert_edit(&source, &after);
			source = after;
		}
		assert!(
			fast * 2 > edits,
			"seed {seed:#x}: the fast path only handled {fast}/{edits} edits"
		);
	}
}

#[test]
fn incremental_parse_keeps_utf8_boundaries() {
	// A byte-wise common prefix can end inside a multi-byte character.
	assert_edit("Café au lait.\n\nSecond.\n", "Cafè au lait.\n\nSecond.\n");
	assert_edit("Café au lait.\n\nSecond.\n", "Café.\n\nSecond.\n");
	assert_edit("One 😀 emoji.\n\nTwo.\n", "One 😀😀 emoji.\n\nTwo.\n");
	assert_edit("One 😀 emoji.\n\nTwo.\n", "One emoji.\n\nTwo.\n");
}

#[test]
fn incremental_parse_handles_line_endings_and_single_blocks() {
	// CRLF sources: a blank line is still a group boundary.
	assert_edit("One.\r\n\r\nTwo.\r\n", "One!\r\n\r\nTwo.\r\n");
	assert_edit("One.\r\n\r\nTwo.\r\n", "One.\r\n\r\nTwo!\r\n");
	assert_edit("One.\r\n", "One.\r\n\r\nTwo.\r\n");
	// A document with a single block has no neighbour to lean on.
	assert_edit("Only one paragraph.\n", "Only one paragraph!\n");
	assert_edit("Only one paragraph.\n", "Only one paragraph.\nMore.\n");
	assert_incremental("Only one paragraph.\n", "Only one paragraph!\n");
}

#[test]
fn prefix_parse_matches_the_start_of_a_full_parse() {
	let source: Arc<str> = Arc::from(PARAGRAPHS);
	let full = parse(source.as_ref());
	let prefix = parse_prefix(&source, 20).expect("a prefix");
	assert!(
		!prefix.blocks.is_empty() && prefix.blocks.len() < full.blocks.len()
	);
	assert_eq!(prefix.blocks, full.blocks[..prefix.blocks.len()]);
	assert_eq!(&*prefix.source, &*source);
	assert_eq!(prefix.content_id, content_identity(&prefix.blocks));
	// A cut inside the first heading still yields that heading alone.
	assert_eq!(parse_prefix(&source, 7).unwrap().blocks, full.blocks[..1]);
	// The whole source is not a prefix.
	assert!(parse_prefix(&source, source.len()).is_none());
	assert!(parse_prefix(&source, 0).is_none());
}

/// A cut between blocks: the prefix is exactly the start of the full parse, so
/// a reference or note in it resolves the way the full parse resolves it.
fn assert_prefix_matches(source: &str, bytes: usize) {
	let source: Arc<str> = Arc::from(source);
	let full = parse(source.as_ref());
	let prefix = parse_prefix(&source, bytes).expect("a prefix");
	assert!(
		!prefix.blocks.is_empty() && prefix.blocks.len() < full.blocks.len()
	);
	assert_eq!(prefix.blocks, full.blocks[..prefix.blocks.len()]);
}

#[test]
fn prefix_parse_resolves_definitions_that_follow_the_cut() {
	assert_prefix_matches("See [the note][n].\n\nMore.\n\n[n]: https://x\n", 8);
	assert_prefix_matches("See[^n].\n\nMore.\n\n[^n]: Note.\n", 4);
	// Definitions in either order still number the references they appear in.
	assert_prefix_matches(
		"A[^a] and B[^b].\n\nMore.\n\n[^b]: B.\n[^a]: A.\n",
		6,
	);
	// A `[x]: ...` line inside a fence is text, not a definition.
	assert_prefix_matches("See [x].\n\n```\n[x]: https://x\n```\n", 8);
	// An open fence at the cut swallows anything appended, so the bare prefix
	// is parsed; its paragraph still matches the full parse.
	let fenced: Arc<str> = Arc::from("Text.\n\n```\ncode\n\nmore\n");
	let full = parse(fenced.as_ref());
	let prefix = parse_prefix(&fenced, 12).expect("a prefix");
	assert_eq!(prefix.blocks[0], full.blocks[0]);
	assert!(matches!(prefix.blocks[1].kind, BlockKind::Code { .. }));
}

#[test]
fn prefix_parse_accepts_containers_and_cuts_them_at_a_boundary() {
	let source: Arc<str> = Arc::from(
		"# T\n\n- a\n- b\n\n```rust\nfn main() {}\n\nmore();\n```\n\nEnd.\n",
	);
	let full = parse(source.as_ref());
	// A cut inside the list group still yields the whole list.
	let prefix = parse_prefix(&source, 12).expect("a prefix");
	assert_eq!(prefix.blocks, full.blocks[..prefix.blocks.len()]);
	assert!(matches!(prefix.blocks[1].kind, BlockKind::List { .. }));
	// A cut inside the fence yields the part of the code block that exists.
	let prefix = parse_prefix(&source, 30).expect("a prefix");
	assert_eq!(prefix.blocks[..2], full.blocks[..2]);
	assert!(matches!(prefix.blocks[2].kind, BlockKind::Code { .. }));
}

#[test]
fn incremental_parse_rejects_a_bare_list_marker() {
	// `-` alone opens an empty list item, so the document is not a run of leaf
	// blocks and an edit must not splice the list away.
	let before = "-\nFirst\n\n  Two\n\nEnd\n";
	assert!(
		parse_incremental(
			&parse(before),
			Arc::from(before.replace("Two", "TWO"))
		)
		.is_none()
	);
	assert_edit(before, &before.replace("Two", "TWO"));
	// The same for an ordered marker with an empty item.
	let ordered = "1.\nFirst\n\nTwo\n\nEnd\n";
	assert!(
		parse_incremental(
			&parse(ordered),
			Arc::from(ordered.replace("Two", "TWO"))
		)
		.is_none()
	);
	assert_edit(ordered, &ordered.replace("Two", "TWO"));
	// A marker with content is rejected the same way as before.
	let listed = "- item\n\nText.\n";
	assert!(
		parse_incremental(
			&parse(listed),
			Arc::from(listed.replace("Text", "TEXT"))
		)
		.is_none()
	);
}

#[test]
fn incremental_parse_keeps_unicode_space_lines_in_their_paragraph() {
	// NBSP and other Unicode spaces are content, not blank lines, so the group
	// must not be cut through the paragraph that holds them.
	assert_incremental(
		"One\n\u{a0}\nTwo\n\nEnd\n",
		"One\n\u{a0}\nTWO\n\nEnd\n",
	);
	assert_incremental(
		"One\n\u{3000}\nTwo\n\nEnd\n",
		"One\n\u{3000}\nTwo!\n\nEnd\n",
	);
	// Markdown's own blank lines are still boundaries: the same edit in the
	// second paragraph re-parses only that paragraph's group.
	assert_incremental(
		"One\n\u{a0}\nTwo\n\nThree\n\nEnd\n",
		"One\n\u{a0}\nTwo\n\nTHREE\n\nEnd\n",
	);
}
