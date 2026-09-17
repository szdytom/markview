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
