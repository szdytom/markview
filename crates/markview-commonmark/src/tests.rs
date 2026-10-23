use super::*;
use markview_document::{Callout, CalloutKind};
use std::sync::Arc;
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
			.any(|s| s.style.link_url() == Some("https://example.com"))
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
	assert_eq!(style("l").link_url(), Some("/u"));
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
fn a_transparent_container_renders_its_markdown_body_in_place() {
	let doc = parse("<div>\n\n**Hello**\n\n</div>\n");
	assert_eq!(doc.blocks.len(), 1);
	let BlockKind::Paragraph(text) = &doc.blocks[0].kind else {
		panic!("expected a paragraph")
	};
	assert!(text.iter().any(|t| t.style.bold));
	assert_eq!(plain_text(text), "Hello");
	// The body keeps its own range; the group adds nothing.
	let start = doc.source.find("**Hello**").unwrap();
	assert_eq!(doc.blocks[0].source, start..start + "**Hello**".len());
}

#[test]
fn transparent_containers_nest_and_resolve_document_references() {
	let doc = parse(
		"<div>\n\n<section>\n\n[link][ref]\n\n</section>\n\n</div>\n\n[ref]: https://example.com\n",
	);
	assert_eq!(doc.blocks.len(), 1);
	let BlockKind::Paragraph(text) = &doc.blocks[0].kind else {
		panic!("expected a paragraph")
	};
	assert!(
		text.iter()
			.any(|i| i.style.link_url() == Some("https://example.com"))
	);
}

#[test]
fn an_unclosed_transparent_container_keeps_the_html_source() {
	let doc = parse("<div>\n\nBody\n");
	assert!(matches!(
		&doc.blocks[0].kind,
		BlockKind::Code { language, text }
			if language == "HTML source" && text.contains("<div>")
	));
	let BlockKind::Paragraph(body) = &doc.blocks[1].kind else {
		panic!("expected the body paragraph")
	};
	assert_eq!(plain_text(body), "Body");
}

#[test]
fn a_single_block_transparent_tag_renders_its_text() {
	let doc = parse("<div>raw</div>\n");
	assert_eq!(doc.blocks.len(), 1);
	let BlockKind::Paragraph(text) = &doc.blocks[0].kind else {
		panic!("expected a paragraph")
	};
	assert_eq!(plain_text(text), "raw");
}

#[test]
fn a_quoted_transparent_container_is_not_quoted_again() {
	let doc = parse("> <div>\n>\n> Body\n>\n> </div>\n");
	assert_eq!(doc.blocks.len(), 1);
	let BlockKind::Quote { blocks, .. } = &doc.blocks[0].kind else {
		panic!("expected the enclosing quote")
	};
	assert_eq!(blocks.len(), 1);
	let BlockKind::Paragraph(body) = &blocks[0].kind else {
		panic!("expected the body paragraph")
	};
	assert_eq!(plain_text(body), "Body");
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
fn the_outline_lists_every_heading_in_reading_order() {
	let doc = parse(
		"# Start\n\n> ## Quoted\n\n- ### Listed\n\nText.\n\n# Start\n\n<div>\n\n</div>\n",
	);
	let outline = doc.outline();
	let seen: Vec<(&str, u8, &str)> = outline
		.iter()
		.map(|e| (e.text.as_str(), e.level, e.anchor.as_str()))
		.collect();
	assert_eq!(
		seen,
		[
			("Start", 1, "start"),
			("Quoted", 2, "quoted"),
			("Listed", 3, "listed"),
			("Start", 1, "start-1"),
		]
	);
	// A heading nested in a container carries the anchor links resolve.
	let BlockKind::Quote { blocks, .. } = &doc.blocks[1].kind else {
		panic!("expected a quote")
	};
	let BlockKind::Heading { anchor, .. } = &blocks[0].kind else {
		panic!("expected a quoted heading")
	};
	assert_eq!(outline[1].anchor, *anchor);
	// A document without headings has an empty outline.
	assert!(parse("Just a paragraph.\n").outline().is_empty());
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
	assert_eq!(reference.style.link_url(), Some("#fn:1"));
	assert!(reference.style.footnote_ref && reference.style.superscript);

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

#[test]
fn a_single_newline_breaks_only_in_the_hard_break_dialect() {
	use crate::document::plain_text;
	fn paragraph(doc: &Document) -> &RichText {
		let BlockKind::Paragraph(rich) = &doc.blocks[0].kind else {
			panic!("not a paragraph");
		};
		rich
	}
	// Off, a newline is the space CommonMark reads.
	let soft = parse("one\ntwo\n");
	assert_eq!(plain_text(paragraph(&soft)), "one two");
	assert!(
		paragraph(&soft)
			.iter()
			.all(|i| !matches!(i.kind, InlineKind::LineBreak { .. }))
	);
	// On, the same newline ends its line like a typed hard break.
	let hard = parse_with(
		"one\ntwo\n",
		Dialect {
			hard_breaks: true,
			..Dialect::default()
		},
	);
	assert_eq!(plain_text(paragraph(&hard)), "one\ntwo");
	assert!(
		paragraph(&hard).iter().any(|i| matches!(
			&i.kind,
			InlineKind::LineBreak { justify: false }
		))
	);
}

#[test]
fn a_disclosure_body_keeps_the_documents_hard_break_dialect() {
	use crate::document::plain_text;
	let doc = parse_with(
		"<details>\n<summary>Note</summary>\n\none\ntwo\n\n</details>\n",
		Dialect {
			hard_breaks: true,
			..Dialect::default()
		},
	);
	let BlockKind::Details { blocks, .. } = &doc.blocks[0].kind else {
		panic!("not a disclosure");
	};
	let BlockKind::Paragraph(rich) = &blocks[0].kind else {
		panic!("not a paragraph");
	};
	assert_eq!(plain_text(rich), "one\ntwo");
}

#[test]
fn a_highlight_mark_parses_only_in_the_dialect() {
	fn paragraph(doc: &Document) -> &RichText {
		let BlockKind::Paragraph(rich) = &doc.blocks[0].kind else {
			panic!("not a paragraph");
		};
		rich
	}
	let highlighted = Dialect {
		highlight_marks: true,
		..Dialect::default()
	};
	// Off, `==` stays the literal source.
	let literal = parse("a ==marked== b\n");
	assert_eq!(plain_text(paragraph(&literal)), "a ==marked== b");
	assert!(paragraph(&literal).iter().all(|i| !i.style.mark));
	// On, the same source reads as a mark; OFM lite parses it this way.
	let marked = parse_with("a ==marked== b\n", highlighted);
	assert_eq!(plain_text(paragraph(&marked)), "a marked b");
	assert!(paragraph(&marked).iter().any(|i| i.style.mark
		&& matches!(&i.kind, InlineKind::Text(t) if t == "marked")));
	// Emphasis inside a mark keeps its own style.
	let nested = parse_with("a ==**bold**== b\n", highlighted);
	assert_eq!(plain_text(paragraph(&nested)), "a bold b");
	assert!(
		paragraph(&nested)
			.iter()
			.any(|i| i.style.mark && i.style.bold)
	);
}

/// An edit the incremental path must handle: the result has to equal a full
/// parse block for block, source range for source range, in either dialect.
fn assert_incremental(before: &str, after: &str) {
	for hard_breaks in [false, true] {
		let dialect = Dialect {
			hard_breaks,
			..Dialect::default()
		};
		let previous = parse_with(before, dialect);
		let got = parse_incremental(&previous, Arc::from(after), dialect)
			.unwrap_or_else(|| {
				panic!("no fast path for {before:?} -> {after:?}")
			});
		let expected = parse_with(after, dialect);
		assert_eq!(
			got.content_id, expected.content_id,
			"{before:?} -> {after:?} (hard breaks: {hard_breaks})"
		);
		assert_eq!(
			got.blocks, expected.blocks,
			"{before:?} -> {after:?} (hard breaks: {hard_breaks})"
		);
	}
}

/// Every edit, whether or not it takes the fast path, must be correct.
fn assert_edit(before: &str, after: &str) {
	for hard_breaks in [false, true] {
		let dialect = Dialect {
			hard_breaks,
			..Dialect::default()
		};
		let previous = parse_with(before, dialect);
		let got = parse_incremental(&previous, Arc::from(after), dialect)
			.unwrap_or_else(|| parse_with(after, dialect));
		let expected = parse_with(after, dialect);
		assert_eq!(
			got.content_id, expected.content_id,
			"{before:?} -> {after:?} (hard breaks: {hard_breaks})"
		);
		assert_eq!(
			got.blocks, expected.blocks,
			"{before:?} -> {after:?} (hard breaks: {hard_breaks})"
		);
	}
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

#[test]
fn multiline_inline_ranges_use_their_closing_lines_indent() {
	for newline in ["\n", "\r\n", "\r"] {
		for (open, close) in [
			("$a", "b$"),
			("$$a", "b$$"),
			("`a", "b`"),
			("\\(a", "b\\)"),
			("<img", "src='x'>"),
		] {
			for first_indent in 0..=3 {
				for last_indent in 0..=3 {
					for prefix in ["", "> ", "- "] {
						let continuation =
							if prefix == "- " { "  " } else { prefix };
						let source = format!(
							"{prefix}Lead{newline}{continuation}{}{open}{newline}{continuation}{}{close} suffix{newline}{newline}Tail{newline}",
							" ".repeat(first_indent),
							" ".repeat(last_indent),
						);
						let doc = parse(source.as_str());
						let block = match &doc.blocks[0].kind {
							BlockKind::Quote { blocks, .. } => &blocks[0],
							BlockKind::List { items, .. } => {
								&items[0].blocks[0]
							}
							_ => &doc.blocks[0],
						};
						let BlockKind::Paragraph(rich) = &block.kind else {
							panic!("not a paragraph: {source:?}");
						};
						let start = source.find(open).unwrap();
						let end = source.find(close).unwrap() + close.len();
						let inline = rich
							.iter()
							.find(|i| i.source.start == start)
							.unwrap();
						assert_eq!(inline.source, start..end, "{source:?}");
					}
				}
			}
		}
	}
}

#[test]
fn incremental_multiline_inlines_match_full_parses() {
	for newline in ["\n", "\r\n", "\r"] {
		for (open, close) in [("$a", "b$"), ("`a", "b`"), ("\\(a", "b\\)")] {
			for first_indent in 0..=3 {
				for last_indent in 0..=3 {
					for tail in ["", "Tail"] {
						let before = format!(
							"{}{open}{newline}{}{close}{newline}{newline}{tail}",
							" ".repeat(first_indent),
							" ".repeat(last_indent),
						);
						let after = format!("Lead{newline}{before}");
						assert_incremental(&before, &after);
						assert_incremental(&after, &before);
						assert_incremental(
							&before,
							&format!("First{newline}{newline}{before}"),
						);
					}
				}
			}
		}
	}
}

#[test]
fn incremental_inline_ranges_match_the_minimized_fuzz_input() {
	let before = String::from_utf8_lossy(b"  $##o\r\x04\0\xd8$\n");
	assert_incremental(&before, &format!("$x^2$\n{before}"));
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
fn thematic_break_ranges_exclude_trailing_blank_lines() {
	for newline in ["\n", "\r\n", "\r"] {
		for marker in ["----", "***", "___", "- - -", "  ---- "] {
			let before = format!("a{newline}{newline}{marker}");
			let expected = parse(before.as_str()).blocks.pop().unwrap();
			assert!(matches!(expected.kind, BlockKind::Rule));
			assert_eq!(
				&before[expected.source.clone()],
				marker.trim_start_matches(' ')
			);
			for blank in ["", " ", "  ", "\t", " \t", newline] {
				let source = format!("{before}{newline}{blank}");
				let full = parse(source.as_str());
				assert_eq!(
					full.blocks.last().unwrap(),
					&expected,
					"{source:?}"
				);
				let prefix =
					parse_prefix(&Arc::from(source), before.len()).unwrap();
				assert_eq!(prefix.blocks, full.blocks);
			}
		}
	}
}

#[test]
fn incremental_thematic_breaks_match_full_parses_with_trailing_blanks() {
	// The minimized fuzz input duplicates the blank line before `----`.
	assert_incremental("a\n\n\n----\n ", "a\n\n\n\n----\n ");
	for newline in ["\n", "\r\n", "\r"] {
		for marker in ["----", "***", "___"] {
			for blank in ["", " ", "  ", newline] {
				let before =
					format!("a{newline}{newline}{marker}{newline}{blank}");
				for after in [
					before.replacen('a', "alpha", 1),
					format!("a{newline}{before}"),
					format!("{newline}{before}"),
					format!("a{newline}{newline}____{newline}{blank}"),
					format!("{before} {newline}"),
					format!("{before}{newline}body"),
				] {
					assert_incremental(&before, &after);
					assert_incremental(&after, &before);
				}
			}
		}
	}
}

#[test]
fn a_bom_at_a_window_start_falls_back_to_a_full_parse() {
	// A fuzz finding: the parser drops a BOM only at the very start of a
	// document, and the incremental window is parsed as its own document.
	// A window that opens on a mid-document BOM would parse its first block
	// shorter than the full parse does.
	let before =
		"\u{feff}**bold** __ital__~~gone~~\n\n\u{feff}second paragraph text.\n";
	assert_edit(before, &format!("{before}\n## heading now\n"));
}

#[test]
fn a_lone_carriage_return_keeps_source_ranges_on_the_right_line() {
	// A fuzz finding: comrak ends a line at `\r` as well as `\n`, so a
	// leading carriage return puts the content on line 2. The line table must
	// follow comrak, or every range after the break is computed from the
	// wrong offset and the incremental window misclassifies the result.
	let before = "\r\0  $   \0li\0\n";
	let doc = parse(before);
	assert!(
		doc.blocks.iter().all(|b| b.source.start < b.source.end),
		"every block keeps a nonempty range: {:?}",
		doc.blocks
			.iter()
			.map(|b| b.source.clone())
			.collect::<Vec<_>>()
	);
	assert_eq!(
		doc.blocks[0].source.start, 1,
		"the range starts after the \\r"
	);
	assert_edit(before, &format!("{before}## heading now\n"));
}

#[test]
fn a_quoted_details_body_survives_a_lone_carriage_return() {
	// The `<details>` body is cut out of the raw source and its enclosing `>`
	// markers stripped per line, so that helper must break lines where the
	// parser does. A lone `\r` ends a line there too; missing it left the
	// `>` of the next line in the body, which reparsed as a quote one level
	// too deep.
	let cr = String::from("> <details>\n>\n> a\r> b\n>\n> </details>\n");
	let lf = cr.replace('\r', "\n");
	let quotes = |doc: &Document| {
		fn count(blocks: &[Block], n: &mut usize) {
			for block in blocks {
				match &block.kind {
					BlockKind::Quote { blocks, .. } => {
						*n += 1;
						count(blocks, n);
					}
					BlockKind::Details { blocks, .. } => count(blocks, n),
					_ => {}
				}
			}
		}
		let mut n = 0;
		count(&doc.blocks, &mut n);
		n
	};
	let from_cr = quotes(&parse(cr));
	let from_lf = quotes(&parse(lf));
	assert_eq!(
		from_cr, from_lf,
		"a lone \r must nest the details body like \n"
	);
}

#[test]
fn a_reference_behind_a_lone_carriage_return_still_resolves_in_a_prefix() {
	// `definitions()` feeds a prefix parse the definitions that follow the
	// cut. Walking `\n`-only lines glued `a\r[x]: /u` into one line, so the
	// marker was never a column-zero `[`: the prefix kept a literal `[x]`
	// while the full parse linked it.
	let lf = String::from("see [x].\n\none\n\ntwo\n\n[x]: /u\n");
	let cr = lf.replace("[x]: /u", "a\r[x]: /u");
	for (tag, src) in [("lf", lf), ("cr", cr)] {
		let source: Arc<str> = Arc::from(src);
		let prefix = parse_prefix(&source, 10).expect("prefix");
		let inline = format!("{:?}", prefix.blocks[0].kind);
		assert!(
			!inline.contains("\"[x]\""),
			"{tag}: the prefix kept an unresolved reference: {inline}"
		);
	}
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
			parse_incremental(
				&parse(plain),
				Arc::from(after),
				Dialect::default()
			)
			.is_none(),
			"expected a fallback for {after:?}"
		);
		assert_edit(plain, after);
	}
	// A document that only becomes plain still falls back the first time.
	assert!(
		parse_incremental(
			&parse("- item\n"),
			Arc::from(plain),
			Dialect::default()
		)
		.is_none()
	);
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
				Dialect::default(),
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
fn prefix_parse_resolves_an_indented_definition() {
	// A definition may sit up to three spaces deep, which the parser reads and
	// the prefix's own scan used to skip: the prefix block stayed a literal
	// `[foo]` where the full parse links it.
	assert_prefix_matches(
		"[foo]\n\nfirst line of the paragraph here is long\nsecond line of the paragraph is also long\n\n  [foo]: /url\n",
		3,
	);
}

#[test]
fn prefix_parse_keeps_a_rejected_definition_from_swallowing_the_next() {
	// `[foo]: /a "unterminated` is not a definition: the full parse reads it as
	// a paragraph and takes the definition after the blank line. Appending both
	// lines adjacently made the first one a paragraph that swallowed the
	// second, so the prefix block kept a literal `[foo]`.
	let source = "[foo]\n\n[foo]: /a \"unterminated\n\n[foo]: /b\n";
	assert_prefix_matches(source, 3);
	// A cut whose prefix covers the rejected line: it still holds no
	// destination, so the later definition must be appended rather than
	// shadowed by the label the rejected line spells.
	let source: Arc<str> = Arc::from(source);
	let full = parse(source.as_ref());
	let prefix = parse_prefix(&source, 6).expect("a prefix");
	assert_eq!(prefix.blocks, full.blocks[..prefix.blocks.len()]);
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
			Arc::from(before.replace("Two", "TWO")),
			Dialect::default()
		)
		.is_none()
	);
	assert_edit(before, &before.replace("Two", "TWO"));
	// The same for an ordered marker with an empty item.
	let ordered = "1.\nFirst\n\nTwo\n\nEnd\n";
	assert!(
		parse_incremental(
			&parse(ordered),
			Arc::from(ordered.replace("Two", "TWO")),
			Dialect::default()
		)
		.is_none()
	);
	assert_edit(ordered, &ordered.replace("Two", "TWO"));
	// A marker with content is rejected the same way as before.
	let listed = "- item\n\nText.\n";
	assert!(
		parse_incremental(
			&parse(listed),
			Arc::from(listed.replace("Text", "TEXT")),
			Dialect::default()
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

/// The summary rich text and body blocks of the only `<details>` in `doc`.
fn details(document: &Document) -> (bool, String, &[Block]) {
	let BlockKind::Details {
		open,
		summary,
		blocks,
		..
	} = &document.blocks[0].kind
	else {
		panic!("expected a details block")
	};
	(*open, plain_text(summary), blocks)
}

#[test]
fn details_inline_form() {
	let doc = parse("<details><summary>Title</summary>Body</details>\n");
	assert_eq!(doc.blocks.len(), 1);
	let (open, summary, blocks) = details(&doc);
	assert!(!open);
	assert_eq!(summary, "Title");
	assert_eq!(blocks.len(), 1);
	let BlockKind::Paragraph(body) = &blocks[0].kind else {
		panic!("expected a paragraph body")
	};
	assert_eq!(plain_text(body), "Body");
}

#[test]
fn a_details_body_with_a_lone_carriage_return_keeps_its_ranges() {
	// The body is re-parsed on its own (`snippet`), which needs the same
	// comrak-compatible line table the document parse uses; before that fix a
	// lone carriage return shifted every range after it, degenerating the
	// second block's range to an empty one at the end of the body.
	let doc =
		parse("<details>\n<summary>S</summary>First\r\rSecond\n\n</details>\n");
	let (_, _, blocks) = details(&doc);
	assert_eq!(blocks.len(), 2);
	assert_eq!(
		blocks[1].source.start,
		doc.source.find("Second").unwrap(),
		"the second block starts after the lone \\r line"
	);
}

#[test]
fn an_indented_details_closer_keeps_the_element_range_on_the_tag() {
	// A fuzz finding: the closing tag's comrak block reports its sourcepos
	// at the tag, but the block's literal starts at the line head, so the
	// element's end must be measured through the literal or an indented
	// `</details>` drags the range past the tag and out of the source.
	let doc = parse("<details open>\n\n  </details>\n");
	assert_eq!(doc.blocks[0].source, 0..28);
	let (open, summary, blocks) = details(&doc);
	assert!(open);
	assert!(summary.is_empty());
	assert!(blocks.is_empty());
}

#[test]
fn an_inline_details_body_is_not_markdown() {
	// Content in the opening tag's own HTML block is raw, so Markdown
	// markers stay literal; only a body after a blank line is Markdown.
	let doc = parse("<details><summary>S</summary>**Bold**</details>\n");
	let (_, _, blocks) = details(&doc);
	let BlockKind::Paragraph(body) = &blocks[0].kind else {
		panic!("expected a paragraph body")
	};
	assert_eq!(plain_text(body), "**Bold**");
}

#[test]
fn a_commented_summary_closer_is_not_markup() {
	// The summary scanner must skip comments and opaque elements, or a
	// commented `</summary>` steals the real element.
	let doc = parse(
		"<details><summary><!-- </summary> -->Hello</summary>Body</details>\n",
	);
	let (_, summary, blocks) = details(&doc);
	assert_eq!(summary, "Hello");
	let BlockKind::Paragraph(body) = &blocks[0].kind else {
		panic!("expected a body paragraph")
	};
	assert_eq!(plain_text(body), "Body");
}

#[test]
fn a_stray_close_does_not_replay_the_whole_block() {
	// Only the unmatched fragment falls back; later containers still render.
	let doc = parse("<div>one</div></section><details>two</details>\n");
	assert_eq!(doc.blocks.len(), 3);
	let BlockKind::Paragraph(one) = &doc.blocks[0].kind else {
		panic!("expected the first paragraph")
	};
	assert_eq!(plain_text(one), "one");
	assert!(matches!(
		&doc.blocks[1].kind,
		BlockKind::Code { language, text }
			if language == "HTML source" && text.trim() == "</section>"
	));
	let BlockKind::Details {
		summary, blocks, ..
	} = &doc.blocks[2].kind
	else {
		panic!("expected the second disclosure")
	};
	assert_eq!(plain_text(summary), "");
	let BlockKind::Paragraph(two) = &blocks[0].kind else {
		panic!("expected the second body")
	};
	assert_eq!(plain_text(two), "two");
}

#[test]
fn nested_html_containers_stop_at_the_block_depth_limit() {
	// An unbounded frame stack used to build a tree deep enough to overflow
	// the recursive passes that walk it.
	let n = 2000;
	let mut source = String::new();
	for _ in 0..n {
		source.push_str("<details>\n<summary>s</summary>\n\n");
	}
	source.push_str("x\n\n");
	for _ in 0..n {
		source.push_str("</details>\n\n");
	}
	let doc = parse(source);
	fn depth(blocks: &[Block]) -> usize {
		blocks
			.iter()
			.map(|block| match &block.kind {
				BlockKind::Details { blocks, .. } => 1 + depth(blocks),
				_ => 1,
			})
			.max()
			.unwrap_or(0)
	}
	let limit = crate::limits::Limits::default().block_depth;
	assert!(depth(&doc.blocks) <= limit + 1, "{}", depth(&doc.blocks));
}

#[test]
fn a_summary_keeps_markdown_literal() {
	// GFM does not parse Markdown inside `<summary>`, so the markers stay.
	let doc = parse("<details><summary>**Bold**</summary>Body</details>\n");
	let (_, summary, _) = details(&doc);
	assert_eq!(summary, "**Bold**");
}

#[test]
fn details_multiblock_body_is_markdown() {
	let doc = parse(
		"<details>\n<summary>More</summary>\n\nMarkdown **body** with a list:\n\n- one\n- two\n\n</details>\n",
	);
	assert_eq!(doc.blocks.len(), 1);
	let (open, summary, blocks) = details(&doc);
	assert!(!open);
	assert_eq!(summary, "More");
	assert!(matches!(&blocks[0].kind, BlockKind::Paragraph(p)
			if p.iter().any(|i| i.style.bold)));
	assert!(
		matches!(&blocks[1].kind, BlockKind::List { items, .. } if items.len() == 2)
	);
}

#[test]
fn details_open_attribute_starts_expanded() {
	let doc = parse(
		"<details open>\n<summary>More</summary>\n\nBody\n\n</details>\n",
	);
	let (open, summary, blocks) = details(&doc);
	assert!(open);
	assert_eq!(summary, "More");
	assert_eq!(blocks.len(), 1);
}

#[test]
fn details_nest() {
	let doc = parse(
		"<details>\n<summary>Outer</summary>\n\n<details>\n<summary>Inner</summary>\n\nDeep\n\n</details>\n\n</details>\n",
	);
	assert_eq!(doc.blocks.len(), 1);
	let (_, summary, blocks) = details(&doc);
	assert_eq!(summary, "Outer");
	assert_eq!(blocks.len(), 1);
	let BlockKind::Details {
		summary,
		blocks,
		open,
		..
	} = &blocks[0].kind
	else {
		panic!("expected a nested details block")
	};
	assert!(!open);
	assert_eq!(plain_text(summary), "Inner");
	assert_eq!(blocks.len(), 1);
	let BlockKind::Paragraph(deep) = &blocks[0].kind else {
		panic!("expected the nested body")
	};
	assert_eq!(plain_text(deep), "Deep");
}

#[test]
fn nested_openers_in_the_opening_block_still_nest() {
	// Outer and inner open before the first blank line, so the closing scan
	// must start at the depth the opening block already left; otherwise the
	// outer element ends at the inner closing tag.
	let doc = parse(
		"<details>\n<summary>Outer</summary>\n<details>\n<summary>Inner</summary>\n\nDeep\n\n</details>\n</details>\n",
	);
	assert_eq!(doc.blocks.len(), 1);
	let (_, summary, blocks) = details(&doc);
	assert_eq!(summary, "Outer");
	assert_eq!(blocks.len(), 1);
	let BlockKind::Details {
		summary, blocks, ..
	} = &blocks[0].kind
	else {
		panic!("expected the nested details")
	};
	assert_eq!(plain_text(summary), "Inner");
	let BlockKind::Paragraph(deep) = &blocks[0].kind else {
		panic!("expected the nested body")
	};
	assert_eq!(plain_text(deep), "Deep");
}

#[test]
fn a_quoted_details_body_is_not_quoted_again() {
	// Comrak strips the enclosing `>` markers from the opener and the closing
	// tag, so the raw body between them must lose the same markers; otherwise
	// the body becomes a quote inside the disclosure.
	let doc = parse(
		"> <details>\n> <summary>More</summary>\n>\n> Body\n>\n> </details>\n",
	);
	assert_eq!(doc.blocks.len(), 1);
	let BlockKind::Quote { blocks, .. } = &doc.blocks[0].kind else {
		panic!("expected the enclosing quote")
	};
	assert_eq!(blocks.len(), 1);
	let BlockKind::Details {
		summary, blocks, ..
	} = &blocks[0].kind
	else {
		panic!("expected the details element")
	};
	assert_eq!(plain_text(summary), "More");
	assert_eq!(blocks.len(), 1);
	let BlockKind::Paragraph(body) = &blocks[0].kind else {
		panic!("expected a direct paragraph body")
	};
	assert_eq!(plain_text(body), "Body");
	// A quote written inside the body is still a quote.
	let doc = parse(
		"> <details>\n> <summary>More</summary>\n>\n> > Quoted\n>\n> </details>\n",
	);
	let BlockKind::Quote { blocks, .. } = &doc.blocks[0].kind else {
		panic!("expected the enclosing quote")
	};
	let BlockKind::Details { blocks, .. } = &blocks[0].kind else {
		panic!("expected the details element")
	};
	let BlockKind::Quote { blocks, .. } = &blocks[0].kind else {
		panic!("expected the quote inside the body")
	};
	let BlockKind::Paragraph(quoted) = &blocks[0].kind else {
		panic!("expected the quoted paragraph")
	};
	assert_eq!(plain_text(quoted), "Quoted");
}

#[test]
fn a_details_body_resolves_document_wide_references() {
	// The definition follows the closing tag, so a snippet parsed on its own
	// would leave the reference as literal text.
	let doc = parse(
		"<details>\n<summary>Link</summary>\n\n[link][ref] and ![img][ref]\n\n</details>\n\n[ref]: https://example.com \"Title\"\n",
	);
	let (_, summary, blocks) = details(&doc);
	assert_eq!(summary, "Link");
	let BlockKind::Paragraph(body) = &blocks[0].kind else {
		panic!("expected the body paragraph")
	};
	let link = body
		.iter()
		.find(|inline| inline.style.link.is_some())
		.expect("the reference link");
	assert_eq!(link.style.link_url(), Some("https://example.com"));
	let image = body
		.iter()
		.find_map(|inline| match &inline.kind {
			InlineKind::Image(image) => Some(image),
			_ => None,
		})
		.expect("the reference image");
	assert_eq!(image.src, "https://example.com");
	assert_eq!(image.alt, "img");
}

#[test]
fn a_details_body_resolves_document_wide_footnotes() {
	// A note defined after the closing tag must still number the body's
	// reference, and the note block keeps the same number.
	let doc = parse(
		"<details>\n<summary>Note</summary>\n\nBody[^n]\n\n</details>\n\n[^n]: The note.\n",
	);
	let (_, _, blocks) = details(&doc);
	let BlockKind::Paragraph(body) = &blocks[0].kind else {
		panic!("expected the body paragraph")
	};
	assert!(
		body.iter()
			.any(|inline| matches!(inline.kind, InlineKind::FootnoteRef(1)))
	);
	let BlockKind::Footnote { label, .. } = &doc.blocks[1].kind else {
		panic!("expected the note")
	};
	assert_eq!(label, "1");
	// The body's note keeps the number the whole document gave it, not the
	// first number the snippet alone would assign.
	let doc = parse(
		"First[^a]\n\n<details>\n<summary>Note</summary>\n\nBody[^b]\n\n</details>\n\n[^a]: A.\n[^b]: B.\n",
	);
	let BlockKind::Details { blocks, .. } = &doc.blocks[1].kind else {
		panic!("expected the details element")
	};
	let BlockKind::Paragraph(body) = &blocks[0].kind else {
		panic!("expected the body paragraph")
	};
	assert!(
		body.iter()
			.any(|inline| matches!(inline.kind, InlineKind::FootnoteRef(2)))
	);
	let BlockKind::Footnote { label, .. } = &doc.blocks[3].kind else {
		panic!("expected the second note")
	};
	assert_eq!(label, "2");
	// A note the body declares appears once at the end of the document,
	// and a reference outside it resolves to that note.
	let doc = parse(
		"Outside[^n]\n\n<details>\n<summary>Note</summary>\n\n[^n]: Declared in the body.\n\n</details>\n",
	);
	let BlockKind::Paragraph(outside) = &doc.blocks[0].kind else {
		panic!("expected the outside paragraph")
	};
	assert!(
		outside
			.iter()
			.any(|inline| matches!(inline.kind, InlineKind::FootnoteRef(1)))
	);
	let BlockKind::Details { blocks, .. } = &doc.blocks[1].kind else {
		panic!("expected the details element")
	};
	assert!(blocks.is_empty());
	let BlockKind::Footnote { label, .. } = &doc.blocks[2].kind else {
		panic!("expected the declared note")
	};
	assert_eq!(label, "1");
}

#[test]
fn unmatched_details_keeps_the_html_source() {
	let doc = parse("<details>\n<summary>More</summary>\n\nBody\n");
	let BlockKind::Code { language, text } = &doc.blocks[0].kind else {
		panic!("expected the literal fallback")
	};
	assert_eq!(language, "HTML source");
	assert!(text.contains("<summary>More</summary>"));
	assert_eq!(doc.blocks.len(), 2);
	let BlockKind::Paragraph(body) = &doc.blocks[1].kind else {
		panic!("expected the body paragraph")
	};
	assert_eq!(plain_text(body), "Body");
}

#[test]
fn stray_details_close_keeps_the_html_source() {
	let doc = parse("</details>\n");
	let BlockKind::Code { language, text } = &doc.blocks[0].kind else {
		panic!("expected the literal fallback")
	};
	assert_eq!(language, "HTML source");
	assert!(text.contains("</details>"));
}

#[test]
fn details_id_is_stable_and_unique_per_element() {
	let source = "<details>\n<summary>One</summary>\n\nA\n\n</details>\n\n<details>\n<summary>Two</summary>\n\nB\n\n</details>\n";
	let doc = parse(source);
	assert_eq!(doc.blocks.len(), 2);
	let first = doc.blocks[0].id;
	let second = doc.blocks[1].id;
	assert_ne!(first, second);
	// Re-parsing identical text keeps the identity, which the cache needs.
	let again = parse(source);
	assert_eq!(again.blocks[0].id, first);
	assert_eq!(again.blocks[1].id, second);
	assert_eq!(again.details_declared(first), Some(false));
	assert_eq!(again.details_declared(second), Some(false));
	assert_eq!(again.details_declared(first + 1), None);
}

#[test]
fn details_enclosing_names_the_containers_of_a_hidden_anchor() {
	let doc = parse(
		"# Intro\n\n<details>\n<summary>Outer</summary>\n\n<details>\n<summary>Inner</summary>\n\n### Deep\n\n</details>\n\n</details>\n\n# Later\n",
	);
	let outer = doc.blocks[1].id;
	let BlockKind::Details { blocks, .. } = &doc.blocks[1].kind else {
		panic!("expected the outer details")
	};
	let inner = blocks[0].id;
	// Outermost first, so opening them in order expands the whole path.
	assert_eq!(doc.details_enclosing("deep"), [outer, inner]);
	// A heading outside every disclosure names none, and so does a missing one.
	assert!(doc.details_enclosing("intro").is_empty());
	assert!(doc.details_enclosing("later").is_empty());
	assert!(doc.details_enclosing("missing").is_empty());
}

#[test]
fn a_footnote_defined_in_details_has_no_disclosure_chain() {
	let doc = parse(
		"<details>\n<summary>More</summary>\n\nBody[^a].\n\n[^a]: Note.\n\n</details>\n",
	);
	// The definition is labelled by its index, as the reference is.
	assert!(doc.details_enclosing("fn:1").is_empty());
}

#[test]
fn identical_details_get_independent_identities() {
	let source = "<details>\n<summary>Same</summary>\n\nBody\n\n</details>\n\n<details>\n<summary>Same</summary>\n\nBody\n\n</details>\n";
	let doc = parse(source);
	assert_eq!(doc.blocks.len(), 2);
	assert_ne!(doc.blocks[0].id, doc.blocks[1].id);
	// Identical reading content still shares its semantic layout identity.
	assert_eq!(doc.blocks[0].content_key, doc.blocks[1].content_key);
	let again = parse(source);
	assert_eq!(again.blocks[0].id, doc.blocks[0].id);
	assert_eq!(again.blocks[1].id, doc.blocks[1].id);
}

#[test]
fn details_keeps_adjacent_elements_in_one_block() {
	let doc = parse(
		"<details><summary>One</summary>A</details>\n<details><summary>Two</summary>B</details>\n",
	);
	assert_eq!(doc.blocks.len(), 2);
	let (_, first, _) = details(&doc);
	assert_eq!(first, "One");
	// The second element is parsed from the remainder of the same block.
	let BlockKind::Details {
		summary, blocks, ..
	} = &doc.blocks[1].kind
	else {
		panic!("expected the second details block")
	};
	assert_eq!(plain_text(summary), "Two");
	let BlockKind::Paragraph(body) = &blocks[0].kind else {
		panic!("expected the second body")
	};
	assert_eq!(plain_text(body), "B");
}

#[test]
fn an_inline_nested_details_keeps_the_outer_remainder() {
	let doc = parse(
		"<details>\n<summary>Outer</summary>\n<details><summary>Inner</summary>Deep</details>\nTail\n</details>\n",
	);
	assert_eq!(doc.blocks.len(), 1);
	let (_, summary, blocks) = details(&doc);
	assert_eq!(summary, "Outer");
	assert_eq!(blocks.len(), 2);
	let BlockKind::Details { summary, .. } = &blocks[0].kind else {
		panic!("expected the nested details")
	};
	assert_eq!(plain_text(summary), "Inner");
	let BlockKind::Paragraph(tail) = &blocks[1].kind else {
		panic!("expected the text after the nested element")
	};
	assert_eq!(plain_text(tail), "Tail");
}

/// Every reading text of a block list, in document order, so a test can tell
/// whether an element's content survived parsing.
fn all_text(blocks: &[Block]) -> String {
	let mut out = String::new();
	for block in blocks {
		match &block.kind {
			BlockKind::Details {
				summary, blocks, ..
			} => {
				out.push_str(&plain_text(summary));
				out.push(' ');
				out.push_str(&all_text(blocks));
			}
			BlockKind::Paragraph(text) | BlockKind::Heading { text, .. } => {
				out.push_str(&plain_text(text));
				out.push(' ');
			}
			BlockKind::Quote { blocks, .. }
			| BlockKind::Footnote { blocks, .. } => {
				out.push_str(&all_text(blocks));
			}
			BlockKind::List { items, .. } => {
				for item in items {
					out.push_str(&all_text(&item.blocks));
				}
			}
			_ => {}
		}
	}
	out
}

#[test]
fn nested_details_render_when_their_closing_tags_share_a_block() {
	// Comrak groups consecutive `</details>` lines into one HTML block; the
	// tag scan must still tell which opener each close belongs to.
	let doc = parse(
		"<details>\n<summary>Outer</summary>\n\n<details>\n<summary>Inner</summary>\n\nDeep\n\n</details>\n</details>\n",
	);
	assert_eq!(doc.blocks.len(), 1);
	let (_, summary, blocks) = details(&doc);
	assert_eq!(summary, "Outer");
	assert_eq!(blocks.len(), 1);
	let BlockKind::Details {
		summary, blocks, ..
	} = &blocks[0].kind
	else {
		panic!("expected the nested details")
	};
	assert_eq!(plain_text(summary), "Inner");
	let BlockKind::Paragraph(deep) = &blocks[0].kind else {
		panic!("expected the nested body")
	};
	assert_eq!(plain_text(deep), "Deep");
}

#[test]
fn an_inline_element_sharing_the_outer_close_block_still_nests() {
	// The nested element and the outer closing tag arrive in one HTML block,
	// so the outer body must end at the outer tag, not swallow the inner one.
	let doc = parse(
		"<details>\n<summary>Outer</summary>\n\n<details><summary>Inner</summary>Deep</details>\n</details>\n",
	);
	assert_eq!(doc.blocks.len(), 1);
	let (_, summary, blocks) = details(&doc);
	assert_eq!(summary, "Outer");
	assert_eq!(blocks.len(), 1);
	let BlockKind::Details {
		summary, blocks, ..
	} = &blocks[0].kind
	else {
		panic!("expected the nested details")
	};
	assert_eq!(plain_text(summary), "Inner");
	let BlockKind::Paragraph(deep) = &blocks[0].kind else {
		panic!("expected the nested body")
	};
	assert_eq!(plain_text(deep), "Deep");
}

#[test]
fn content_after_a_closing_tag_is_kept() {
	// The closing tag shares its block with trailing text, which is a sibling
	// of the element and must not be dropped with the block.
	let doc = parse(
		"<details>\n<summary>Outer</summary>\n\nBody\n\n</details> trailing\n",
	);
	assert_eq!(doc.blocks.len(), 2);
	let (_, summary, blocks) = details(&doc);
	assert_eq!(summary, "Outer");
	let BlockKind::Paragraph(body) = &blocks[0].kind else {
		panic!("expected the body")
	};
	assert_eq!(plain_text(body), "Body");
	let BlockKind::Paragraph(tail) = &doc.blocks[1].kind else {
		panic!("expected the trailing text")
	};
	assert_eq!(plain_text(tail), "trailing");
}

#[test]
fn a_summary_belongs_to_its_own_details_element() {
	// The first element is complete and has no summary; the second element's
	// summary must not be adopted by it, and `A` must survive.
	let doc = parse(
		"<details>A</details>\n<details><summary>B</summary>C</details>\n",
	);
	assert_eq!(doc.blocks.len(), 2);
	let (_, summary, blocks) = details(&doc);
	assert_eq!(summary, "");
	let BlockKind::Paragraph(a) = &blocks[0].kind else {
		panic!("expected the first body")
	};
	assert_eq!(plain_text(a), "A");
	let BlockKind::Details {
		summary, blocks, ..
	} = &doc.blocks[1].kind
	else {
		panic!("expected the second details")
	};
	assert_eq!(plain_text(summary), "B");
	let BlockKind::Paragraph(c) = &blocks[0].kind else {
		panic!("expected the second body")
	};
	assert_eq!(plain_text(c), "C");
}

#[test]
fn a_nested_summary_is_not_the_outer_summary() {
	let doc = parse(
		"<details><details><summary>Inner</summary>Deep</details>Tail</details>\n",
	);
	let (_, summary, blocks) = details(&doc);
	assert_eq!(summary, "");
	assert_eq!(blocks.len(), 2);
	let BlockKind::Details {
		summary,
		blocks: inner,
		..
	} = &blocks[0].kind
	else {
		panic!("expected the nested details")
	};
	assert_eq!(plain_text(summary), "Inner");
	let BlockKind::Paragraph(deep) = &inner[0].kind else {
		panic!("expected the nested body")
	};
	assert_eq!(plain_text(deep), "Deep");
	let BlockKind::Paragraph(tail) = &blocks[1].kind else {
		panic!("expected the tail")
	};
	assert_eq!(plain_text(tail), "Tail");
}

#[test]
fn nested_and_adjacent_details_never_merge_or_drop_content() {
	for (source, fragments) in [
		(
			"<details><summary>O</summary>A<details><summary>I</summary>B</details>C</details>\n",
			vec!["O", "A", "I", "B", "C"],
		),
		(
			"<details>\n<summary>O</summary>\n\n<details>\n<summary>I</summary>\n\nB\n\n</details>\n</details>\n",
			vec!["O", "I", "B"],
		),
		(
			"<details>A</details>\n<details><summary>B</summary>C</details>\n",
			vec!["A", "B", "C"],
		),
		(
			"<details>\n<summary>O</summary>\n\nBody\n\n</details> tail\n",
			vec!["O", "Body", "tail"],
		),
		(
			"<details>\n<summary>O</summary>\n\n<details><summary>I</summary>B</details>C\n\n</details>\n",
			vec!["O", "I", "B", "C"],
		),
		(
			"<details>\n<summary>O</summary>\nLead\n\nBody\n\n</details>\n",
			vec!["O", "Lead", "Body"],
		),
		(
			"<details>\n<summary>A</summary>\n\n<details>\n<summary>B</summary>\n\n<details>\n<summary>C</summary>\n\nDeep\n\n</details>\n</details>\n</details>\n",
			vec!["A", "B", "C", "Deep"],
		),
	] {
		let doc = parse(source);
		let text = all_text(&doc.blocks);
		for fragment in fragments {
			assert!(
				text.contains(fragment),
				"{source:?}: {fragment:?} missing from {text:?}"
			);
		}
		// No element fell back to literal HTML source.
		assert!(
			!has_html_source(&doc.blocks),
			"{source:?}: literal HTML remained"
		);
	}
}

/// Whether any block in the tree is the literal raw-HTML fallback.
fn has_html_source(blocks: &[Block]) -> bool {
	blocks.iter().any(|block| match &block.kind {
		BlockKind::Code { language, .. } => language == "HTML source",
		BlockKind::Details { blocks, .. }
		| BlockKind::Quote { blocks, .. }
		| BlockKind::Footnote { blocks, .. } => has_html_source(blocks),
		BlockKind::List { items, .. } => {
			items.iter().any(|item| has_html_source(&item.blocks))
		}
		_ => false,
	})
}

#[test]
fn many_adjacent_details_do_not_reach_the_nesting_limit() {
	// Adjacent elements are siblings, so a long run must not charge each one
	// the nesting budget; the default limit is 256.
	let count = 400;
	let mut source = String::new();
	for i in 0..count {
		source.push_str(&format!(
			"<details><summary>S{i}</summary>B{i}</details>\n"
		));
	}
	let doc = parse(source.as_str());
	assert_eq!(doc.blocks.len(), count);
	for (i, block) in doc.blocks.iter().enumerate() {
		let BlockKind::Details { summary, .. } = &block.kind else {
			panic!("element {i} rendered as {:?}", block.kind)
		};
		assert_eq!(plain_text(summary), format!("S{i}"));
	}
}

#[test]
fn front_matter_is_a_collapsed_yaml_source_block() {
	let doc = parse(
		"---\ntitle: Notes\ncount: 3\ntags: [a, b]\ndraft: false\n---\n\nBody\n",
	);
	assert_eq!(doc.blocks.len(), 2);
	let BlockKind::FrontMatter { open, blocks } = &doc.blocks[0].kind else {
		panic!("{:?}", doc.blocks[0].kind)
	};
	// Metadata is not prose, so it starts collapsed.
	assert!(!open);
	// Nothing read the YAML: the delimiters are gone and the rest is source,
	// whatever shape it has.
	let [
		Block {
			kind: BlockKind::Code { language, text },
			source,
			..
		},
	] = blocks.as_slice()
	else {
		panic!("{:?}", blocks)
	};
	assert_eq!(language, "yaml");
	assert_eq!(text, "title: Notes\ncount: 3\ntags: [a, b]\ndraft: false");
	// The block is code to the highlighter, so it starts a highlight job.
	let mut code = Vec::new();
	doc.blocks[0].code_blocks(&mut code);
	assert_eq!(code, [("yaml", text.as_str())]);
	// The source block is nested in the front matter rather than parsed on its
	// own, so it has an identity of its own to be cached under.
	assert_eq!(source.start, doc.blocks[0].source.start);
	assert!(blocks[0].id != 0 && blocks[0].id != doc.blocks[0].id);
}

#[test]
fn a_nested_front_matter_stays_source() {
	let doc = parse("---\ntitle: Notes\nauthor:\n  name: A\n---\n\nBody\n");
	assert_eq!(doc.blocks.len(), 2);
	let BlockKind::FrontMatter { blocks, .. } = &doc.blocks[0].kind else {
		panic!("{:?}", doc.blocks[0].kind)
	};
	let BlockKind::Code { text, .. } = &blocks[0].kind else {
		panic!("{:?}", blocks[0].kind)
	};
	assert_eq!(text.trim(), "title: Notes\nauthor:\n  name: A");
	// It is drawn as a `yaml` code block, so it is code to the highlighter.
	let mut code = Vec::new();
	doc.blocks[0].code_blocks(&mut code);
	assert_eq!(code, [("yaml", text.as_str())]);
	// Not a mapping at all is source too.
	let doc = parse("---\n- one\n- two\n---\n\nBody\n");
	let BlockKind::FrontMatter { blocks, .. } = &doc.blocks[0].kind else {
		panic!("{:?}", doc.blocks[0].kind)
	};
	let BlockKind::Code { text, .. } = &blocks[0].kind else {
		panic!("{:?}", blocks[0].kind)
	};
	assert_eq!(text, "- one\n- two");
}

#[test]
fn front_matter_only_opens_a_document() {
	// An unclosed fence is an ordinary rule, not metadata.
	let doc = parse("---\ntitle: Notes\n\nBody\n");
	assert!(!matches!(doc.blocks[0].kind, BlockKind::FrontMatter { .. }));
	// Dashes further down stay a rule.
	let doc = parse("Body\n\n---\n\ntitle: Notes\n");
	assert!(
		!doc.blocks
			.iter()
			.any(|b| matches!(b.kind, BlockKind::FrontMatter { .. }))
	);
	// An empty block draws nothing.
	let doc = parse("---\n\n---\nBody\n");
	assert!(
		!doc.blocks
			.iter()
			.any(|b| matches!(b.kind, BlockKind::FrontMatter { .. }))
	);
}

#[test]
fn only_a_delimiter_line_closes_front_matter() {
	// A line that merely opens with `---` is content: comrak closes on the
	// second delimiter, so the body holds the first one too. Nothing parses
	// the YAML, so the line between the delimiters is never lost.
	let source = "---\ntitle: a\n---extra\n---\n\nBody\n";
	let doc = parse(source);
	let BlockKind::FrontMatter { blocks, .. } = &doc.blocks[0].kind else {
		panic!("{:?}", doc.blocks[0].kind)
	};
	let BlockKind::Code { text, .. } = &blocks[0].kind else {
		panic!("{:?}", blocks[0].kind)
	};
	assert_eq!(text, "title: a\n---extra");
	assert!(source.contains(text.as_str()));
	// A value that opens with dashes mid-line is content as well.
	let doc = parse("---\ntitle: a\n---\n\nBody\n");
	let BlockKind::FrontMatter { blocks, .. } = &doc.blocks[0].kind else {
		panic!("{:?}", doc.blocks[0].kind)
	};
	let BlockKind::Code { text, .. } = &blocks[0].kind else {
		panic!("{:?}", blocks[0].kind)
	};
	assert_eq!(text, "title: a");
}

#[test]
fn a_closing_delimiter_at_the_end_of_the_file_still_closes() {
	// The file ends on the delimiter, so no trailing line ending remains to
	// cut the body at. Dropping the metadata here would be silent.
	let doc = parse("---\ntitle: Notes\n---");
	let BlockKind::FrontMatter { blocks, .. } = &doc.blocks[0].kind else {
		panic!("{:?}", doc.blocks[0].kind)
	};
	let BlockKind::Code { text, .. } = &blocks[0].kind else {
		panic!("{:?}", blocks[0].kind)
	};
	assert_eq!(text, "title: Notes");
}

#[test]
fn the_reader_owns_the_front_matter_disclosure_state() {
	// The source declares no state, so the block starts collapsed and the
	// reader's own choice, which lives in the layout options, is what opens
	// it. A `<details>` element with no `open` attribute is the same shape.
	let doc = parse("---\ntitle: Notes\n---\n\nBody\n");
	assert_eq!(doc.details_declared(doc.blocks[0].id), Some(false));
}

#[test]
fn an_edit_inside_front_matter_keeps_it_metadata() {
	// Front matter is one block however many blank lines it holds, so the
	// blank-line window the incremental path replaces can never hold part of
	// it. This edit has to fall back to a full parse.
	let before: Arc<str> =
		"---\ntitle: Notes\n\nauthor: Alice\n---\n\nBody\n".into();
	let edited: Arc<str> =
		"---\ntitle: Notes\n\nauthor: Bob\n---\n\nBody\n".into();
	let updated = reparse(&parse(before), edited.clone());
	let full = parse(edited);
	let metadata = |doc: &Document| {
		doc.blocks.iter().find_map(|block| match &block.kind {
			BlockKind::FrontMatter { blocks, .. } => match &blocks[0].kind {
				BlockKind::Code { text, .. } => Some(text.clone()),
				_ => None,
			},
			_ => None,
		})
	};
	assert_eq!(metadata(&updated), metadata(&full));
	assert!(metadata(&updated).is_some_and(|yaml| yaml.contains("Bob")));
}

#[test]
fn an_attribute_after_a_stray_slash_does_not_split_a_character() {
	// A fuzz finding: `<details /\u{a0}open>` reaches `has_attribute` and
	// `<p><img /\u{a0}src=x>` reaches `attribute`; both split the two-byte
	// space the stripped `/` exposed.
	assert!(parse("<details /\u{a0}open>\n").blocks.iter().all(|b| {
		match &b.kind {
			BlockKind::Paragraph(text) => text.iter().all(
				|i| !matches!(&i.kind, InlineKind::Text(t) if t.contains('\u{a0}')),
			),
			_ => true,
		}
	}));
	let doc = parse("<p><img /\u{a0}src=x>\n");
	let mut images = Vec::new();
	for block in &doc.blocks {
		block.images(&mut images);
	}
	let [image] = images.as_slice() else {
		panic!("expected the image the raw block declares")
	};
	assert_eq!(image.src, "x");
}

#[test]
fn inline_source_ranges_never_run_backwards() {
	// A fuzz finding: comrak reports the paragraph after a link reference
	// definition at the definition's own columns, so a `SoftBreak` span can
	// end before the text before it starts; merging used to move the merged
	// range's end backwards and leave `17..16`.
	fn check(blocks: &[Block]) {
		for block in blocks {
			let text: &RichText = match &block.kind {
				BlockKind::Paragraph(text)
				| BlockKind::Heading { text, .. } => text,
				BlockKind::Quote { blocks, .. }
				| BlockKind::Footnote { blocks, .. }
				| BlockKind::FrontMatter { blocks, .. } => {
					check(blocks);
					continue;
				}
				BlockKind::Details {
					summary, blocks, ..
				} => {
					for inline in summary {
						assert!(inline.source.start <= inline.source.end);
					}
					check(blocks);
					continue;
				}
				BlockKind::List { items, .. } => {
					for item in items {
						check(&item.blocks);
					}
					continue;
				}
				BlockKind::Table { rows, .. } => {
					for row in rows {
						for cell in row {
							for inline in cell {
								assert!(
									inline.source.start <= inline.source.end
								);
							}
						}
					}
					continue;
				}
				BlockKind::Code { .. } | BlockKind::Rule => continue,
			};
			for inline in text {
				assert!(
					inline.source.start <= inline.source.end,
					"inverted inline range {:?} in {:?}",
					inline.source,
					block.source
				);
			}
		}
	}
	let doc = parse("[foo]: d\n   d\n[foo]: d\nc\n[foo]: d");
	check(&doc.blocks);
}

#[test]
fn a_definition_line_inside_a_paragraph_is_not_a_definition() {
	// A fuzz finding: `[bar]: /baz` cannot interrupt the paragraph `Foo`, so
	// it is text. Extracting it as a definition resolved `[bar]` into a link
	// the document never had — in the prefix path and in a `<details>` body.
	let source: Arc<str> = Arc::from("Foo\n[bar]: /baz\n\n[bar]\n");
	let full = parse(source.as_ref());
	let BlockKind::Paragraph(first) = &full.blocks[0].kind else {
		panic!("expected a paragraph")
	};
	assert!(first.iter().all(|i| i.style.link.is_none()));
	let prefix = parse_prefix(&source, source.len() - 1).expect("a prefix");
	assert_eq!(prefix.blocks, full.blocks);
}

#[test]
fn a_definition_after_a_leaf_block_still_counts() {
	// The paragraph rule must not reject a definition that follows a block
	// which leaves nothing open: a heading ends the paragraph, so `[x]` in
	// the prefix resolves through the definition after the cut.
	let source: Arc<str> = Arc::from("See [x].\n\n# H\n[x]: url\n");
	let full = parse(source.as_ref());
	let prefix = parse_prefix(&source, 8).expect("a prefix");
	assert_eq!(prefix.blocks, full.blocks[..prefix.blocks.len()]);
	let BlockKind::Paragraph(text) = &prefix.blocks[0].kind else {
		panic!("expected a paragraph")
	};
	assert!(text.iter().any(|i| i.style.link_url() == Some("url")));
}

#[test]
fn a_prefix_never_cuts_through_front_matter() {
	// The opening `---` is a delimiter, not a thematic break: until the
	// closing delimiter arrives the document puts no block there at all.
	let source: Arc<str> = Arc::from("---\n\n---");
	assert!(parse_prefix(&source, 1).is_none());
	assert!(parse_prefix(&source, 3).is_none());
	let full = parse(source.as_ref());
	let prefix = parse_prefix(&source, 5).expect("a prefix past the closer");
	assert_eq!(prefix.blocks, full.blocks);
}

#[test]
fn a_prefix_never_cuts_through_front_matter_whose_second_line_is_a_delimiter() {
	// Comrak scans for the closing delimiter from the end of the opening line,
	// so a `---` on the line right below the opener is body text: the document
	// is one front-matter block, and a cut inside it made of the opener a
	// prefix that published the `---` as a thematic break.
	for source in ["---\n---\n\n---\n", "---\n---\n---\n"] {
		let source: Arc<str> = Arc::from(source);
		let full = parse(source.as_ref());
		assert!(matches!(full.blocks[0].kind, BlockKind::FrontMatter { .. }));
		for cut in 1..source.len() {
			if let Some(prefix) = parse_prefix(&source, cut) {
				assert_eq!(
					prefix.blocks,
					full.blocks[..prefix.blocks.len()],
					"cut {cut}"
				);
			}
		}
	}
}

#[test]
fn a_prefix_never_cuts_through_an_open_container() {
	// The full parse drops the opener into a transparent container, so a
	// prefix that ends inside it would show the raw `<div>` block instead.
	let source: Arc<str> = Arc::from("<div>\n\nHello\n\nWorld\n\n</div>\n");
	assert!(parse_prefix(&source, 10).is_none());
	assert!(parse_prefix(&source, 15).is_none());
	let full = parse(source.as_ref());
	let prefix = parse_prefix(&source, source.len() - 1)
		.expect("a prefix past the closer");
	assert_eq!(prefix.blocks, full.blocks);
}

#[test]
fn a_stray_container_close_does_not_hide_a_later_opener() {
	// The stray `</div>` must not drive the open depth below zero, or the
	// prefix guard lets a cut inside the real container publish raw HTML.
	let source: Arc<str> =
		Arc::from("</div>\n\n<div>\n\nHello\n\nWorld\n\n</div>\n");
	assert!(parse_prefix(&source, 15).is_none());
	let full = parse(source.as_ref());
	assert_eq!(full.blocks.len(), 3);
	let prefix = parse_prefix(&source, source.len() - 1)
		.expect("a prefix past the closer");
	assert_eq!(prefix.blocks, full.blocks);
}

#[test]
fn a_commented_container_tag_is_not_markup() {
	// A tag inside an HTML comment must not open or close the real
	// container, which would otherwise steal the wrapper's closer.
	for body in ["<!-- <div> -->", "<!-- </div> -->"] {
		let doc = parse(format!("<div>\n\n{body}\n\nHello\n\n</div>\n"));
		assert_eq!(doc.blocks.len(), 1);
		let BlockKind::Paragraph(text) = &doc.blocks[0].kind else {
			panic!("expected the body paragraph")
		};
		assert_eq!(plain_text(text), "Hello");
	}
}

#[test]
fn opaque_elements_hide_their_contents_from_container_matching() {
	// A container tag written inside raw text, RCDATA, foreign content
	// or `<pre>` is not markup, so it must not close the real container.
	for body in [
		"<script></div></script>",
		"<style><div></style>",
		"<pre></div></pre>",
		"<textarea></div></textarea>",
		"<title></div></title>",
	] {
		let doc = parse(format!("<div>\n\n{body}\n\nHello\n\n</div>\n"));
		assert!(
			!doc.blocks.iter().any(|b| matches!(
				&b.kind,
				BlockKind::Code { text, .. } if text.trim() == "<div>"
			)),
			"{body}: the outer container fell back to source"
		);
		assert!(
			doc.blocks.iter().any(|b| matches!(
				&b.kind,
				BlockKind::Paragraph(t) if plain_text(t) == "Hello"
			)),
			"{body}: the body is missing"
		);
	}
	// The same skip keeps a `<details>` element open.
	let doc = parse(
		"<details>\n<summary>S</summary>\n\n<script></details></script>\n\nBody\n\n</details>\n",
	);
	assert_eq!(doc.blocks.len(), 1);
	let BlockKind::Details { blocks, .. } = &doc.blocks[0].kind else {
		panic!("expected the details element")
	};
	assert!(blocks.iter().any(|b| matches!(
		&b.kind,
		BlockKind::Paragraph(t) if plain_text(t) == "Body"
	)));
}

#[test]
fn unmatched_container_openers_keep_the_html_source() {
	// No closer follows either opener, so both stay literal and the text
	// between them still renders.
	let doc = parse("<div>\n\n<div>\n\nText\n");
	assert_eq!(doc.blocks.len(), 3);
	assert!(matches!(
		&doc.blocks[0].kind,
		BlockKind::Code { language, .. } if language == "HTML source"
	));
	assert!(matches!(
		&doc.blocks[1].kind,
		BlockKind::Code { language, .. } if language == "HTML source"
	));
	let BlockKind::Paragraph(text) = &doc.blocks[2].kind else {
		panic!("expected the body paragraph")
	};
	assert_eq!(plain_text(text), "Text");
}

#[test]
fn a_prefix_keeps_the_line_ending_a_list_marker_needs() {
	// `1.` with no line ending parses as a paragraph; with one it is the
	// empty ordered item the document has.
	let source: Arc<str> = Arc::from("1.\n");
	let full = parse(source.as_ref());
	assert!(matches!(full.blocks[0].kind, BlockKind::List { .. }));
	let prefix = parse_prefix(&source, 1).expect("a prefix");
	assert_eq!(prefix.blocks, full.blocks);
}

#[test]
fn obsidian_wikilinks_keep_labels_sources_and_heading_targets() {
	let source = "[[Notes/First note|First]] [[Second.md#A heading|Section]] [[#本节]] `[[code]]`\n";
	let doc = parse(source);
	let BlockKind::Paragraph(text) = &doc.blocks[0].kind else {
		panic!()
	};
	assert_eq!(plain_text(text), "First Section #本节 [[code]]");
	let links: Vec<_> = text
		.iter()
		.filter_map(|span| span.style.link_url())
		.collect();
	assert_eq!(links, ["Notes/First note", "Second.md#a-heading", "#本节"]);
	for span in text {
		assert!(source.is_char_boundary(span.source.start));
		assert!(span.source.end <= source.len());
	}
	assert!(text.last().unwrap().style.link_url().is_none());
	let changed =
		reparse(&doc, source.replace("First note", "Other note").into());
	let BlockKind::Paragraph(text) = &changed.blocks[0].kind else {
		panic!()
	};
	assert_eq!(text[0].style.link_url(), Some("Notes/Other note"));
}

#[test]
fn wikilinks_escape_literal_percents_without_changing_markdown_urls() {
	let doc = parse(
		"[[Note %41]] [[Note %41.md#A heading|Section]] [[Note %2541]] [Markdown](Note%20%41.md)",
	);
	let BlockKind::Paragraph(text) = &doc.blocks[0].kind else {
		panic!()
	};
	assert_eq!(plain_text(text), "Note %41 Section Note %2541 Markdown");
	let links: Vec<_> = text
		.iter()
		.filter_map(|span| span.style.link_url())
		.collect();
	assert_eq!(
		links,
		[
			"Note %2541",
			"Note %2541.md#a-heading",
			"Note %252541",
			"Note%20%41.md"
		]
	);
}

/// The OFM lite dialect: highlight marks, no hard breaks, Obsidian extras.
fn ofm() -> Dialect {
	Dialect {
		highlight_marks: true,
		ofm: true,
		..Dialect::default()
	}
}

fn callout_of(block: &Block) -> (&Callout, &[Block]) {
	let BlockKind::Quote {
		callout: Some(callout),
		blocks,
		..
	} = &block.kind
	else {
		panic!("not a callout: {:?}", block.kind);
	};
	(callout, blocks)
}

#[test]
fn obsidian_callouts_read_every_type_and_alias() {
	let cases = [
		("note", CalloutKind::Note),
		("abstract", CalloutKind::Abstract),
		("summary", CalloutKind::Abstract),
		("TLDR", CalloutKind::Abstract),
		("info", CalloutKind::Info),
		("todo", CalloutKind::Todo),
		("tip", CalloutKind::Tip),
		("hint", CalloutKind::Tip),
		("IMPORTANT", CalloutKind::Tip),
		("success", CalloutKind::Success),
		("check", CalloutKind::Success),
		("done", CalloutKind::Success),
		("question", CalloutKind::Question),
		("help", CalloutKind::Question),
		("Faq", CalloutKind::Question),
		("warning", CalloutKind::Warning),
		("caution", CalloutKind::Warning),
		("attention", CalloutKind::Warning),
		("failure", CalloutKind::Failure),
		("fail", CalloutKind::Failure),
		("missing", CalloutKind::Failure),
		("danger", CalloutKind::Danger),
		("error", CalloutKind::Danger),
		("bug", CalloutKind::Bug),
		("example", CalloutKind::Example),
		("quote", CalloutKind::Quote),
		("cite", CalloutKind::Quote),
		("something-custom", CalloutKind::Note),
	];
	for (name, kind) in cases {
		let doc = parse_with(format!("> [!{name}] Title\n> Body\n"), ofm());
		let (callout, blocks) = callout_of(&doc.blocks[0]);
		assert_eq!(callout.kind, kind, "{name}");
		assert_eq!(callout.title, "Title");
		assert_eq!(callout.fold, None);
		let BlockKind::Paragraph(body) = &blocks[0].kind else {
			panic!("{name}: body {:?}", blocks);
		};
		assert_eq!(plain_text(body), "Body");
	}
}

#[test]
fn a_callout_without_a_title_uses_its_type_name() {
	let doc = parse_with("> [!info]\n> Default title.\n", ofm());
	let (callout, blocks) = callout_of(&doc.blocks[0]);
	assert_eq!(callout.title, "Info");
	assert_eq!(blocks.len(), 1);
	// A marker alone in its paragraph leaves no empty paragraph behind.
	let doc = parse_with("> [!faq]\n>\n> Body.\n", ofm());
	let (callout, blocks) = callout_of(&doc.blocks[0]);
	assert_eq!(
		(callout.kind, callout.title.as_str()),
		(CalloutKind::Question, "Faq")
	);
	assert_eq!(blocks.len(), 1);
}

#[test]
fn a_foldable_callout_wraps_its_body_in_a_disclosure() {
	let doc = parse_with(
		"> [!faq]- Folded\n> Hidden.\n> - one\n\n> [!tip]+ Open\n> Shown.\n",
		ofm(),
	);
	let (folded, blocks) = callout_of(&doc.blocks[0]);
	assert_eq!(folded.fold, Some(false));
	let BlockKind::Details {
		open,
		summary,
		blocks: body,
		..
	} = &blocks[0].kind
	else {
		panic!("no disclosure: {blocks:?}");
	};
	assert!(!open);
	assert!(plain_text(summary).ends_with("Folded"));
	assert_eq!(body.len(), 2);
	let (open_callout, blocks) = callout_of(&doc.blocks[1]);
	assert_eq!(open_callout.fold, Some(true));
	assert!(matches!(
		blocks[0].kind,
		BlockKind::Details { open: true, .. }
	));
	assert_eq!(doc.details_declared(blocks[0].id), Some(true));
}

#[test]
fn callouts_nest_and_keep_inline_markup_in_the_body() {
	let doc = parse_with(
		"> [!note] Outer\n> Outer text.\n> > [!warning] Inner\n> > **bold** ==mark==\n",
		ofm(),
	);
	let (outer, blocks) = callout_of(&doc.blocks[0]);
	assert_eq!(outer.title, "Outer");
	let inner = blocks
		.iter()
		.find(|b| matches!(b.kind, BlockKind::Quote { .. }))
		.expect("nested callout");
	let (inner, body) = callout_of(inner);
	assert_eq!(inner.kind, CalloutKind::Warning);
	let BlockKind::Paragraph(text) = &body[0].kind else {
		panic!()
	};
	assert!(text.iter().any(|i| i.style.bold));
	assert!(text.iter().any(|i| i.style.mark));
}

#[test]
fn gfm_keeps_github_alerts_and_plain_quotes() {
	let doc = parse("> [!NOTE]\n> Alert.\n\n> [!faq] Not an alert\n> Quote.\n");
	assert!(matches!(
		&doc.blocks[0].kind,
		BlockKind::Quote { label: Some(l), callout: None, .. } if l == "Note"
	));
	let BlockKind::Quote {
		label: None,
		callout: None,
		blocks,
	} = &doc.blocks[1].kind
	else {
		panic!()
	};
	let BlockKind::Paragraph(text) = &blocks[0].kind else {
		panic!()
	};
	assert!(plain_text(text).starts_with("[!faq] Not an alert"));
	// In OFM lite a GitHub alert is just another callout type.
	let doc = parse_with("> [!NOTE]\n> Alert.\n", ofm());
	let (callout, _) = callout_of(&doc.blocks[0]);
	assert_eq!(
		(callout.kind, callout.title.as_str()),
		(CalloutKind::Note, "Note")
	);
}

#[test]
fn a_quote_that_only_looks_like_a_callout_stays_a_quote() {
	for source in ["> [!] empty\n", "> [!note]x glued\n", "> text [!note]\n"] {
		let doc = parse_with(source, ofm());
		assert!(
			matches!(
				doc.blocks[0].kind,
				BlockKind::Quote { callout: None, .. }
			),
			"{source}"
		);
	}
}

#[test]
fn escaped_or_styled_callout_markers_stay_quotes() {
	for source in [
		"> \\[!note] Escaped\n> Body\n",
		"> `[!note]` Code\n> Body\n",
		"> **[!note]** Bold\n> Body\n",
	] {
		let doc = parse_with(source, ofm());
		let BlockKind::Quote {
			callout: None,
			blocks,
			..
		} = &doc.blocks[0].kind
		else {
			panic!("became a callout: {source}");
		};
		let BlockKind::Paragraph(text) = &blocks[0].kind else {
			panic!("{source}: {blocks:?}");
		};
		assert!(plain_text(text).contains("[!note]"), "{source}");
	}
}

#[test]
fn obsidian_html_tags_style_only_in_the_dialect() {
	fn paragraph(doc: &Document) -> &RichText {
		let BlockKind::Paragraph(rich) = &doc.blocks[0].kind else {
			panic!("not a paragraph: {:?}", doc.blocks[0].kind);
		};
		rich
	}
	let source = "H<sub>2</sub>O x<sup>2</sup> <mark>hi</mark> <u>u</u> <ins>i</ins> <small>s</small>\n";
	let doc = parse_with(source, ofm());
	let text = paragraph(&doc);
	assert_eq!(plain_text(text), "H2O x2 hi u i s");
	assert!(text.iter().any(|i| i.style.subscript
		&& matches!(&i.kind, InlineKind::Text(t) if t == "2")));
	assert!(text.iter().any(|i| i.style.superscript));
	assert!(
		text.iter().any(|i| i.style.mark
			&& matches!(&i.kind, InlineKind::Text(t) if t == "hi"))
	);
	// GFM keeps them as literal source, as before.
	let gfm = parse(source);
	let text = paragraph(&gfm);
	assert!(plain_text(text).contains("<sub>"));
	assert!(plain_text(text).contains("<mark>"));
	assert!(text.iter().all(|i| !i.style.subscript && !i.style.mark));
	// A raw HTML block reads the same tags.
	let doc = parse_with("<p><mark>m</mark> H<sub>2</sub></p>\n", ofm());
	let text = paragraph(&doc);
	assert!(text.iter().any(|i| i.style.mark));
	assert!(text.iter().any(|i| i.style.subscript));
}

#[test]
fn editing_around_callouts_matches_a_full_parse() {
	let before = "# T\n\nIntro.\n\n> [!faq]- Folded\n> Hidden.\n\n> [!tip] Tip\n> Shown.\n\nOutro.\n";
	for after in [
		"# T\n\nIntro, edited.\n\n> [!faq]- Folded\n> Hidden.\n\n> [!tip] Tip\n> Shown.\n\nOutro.\n",
		"# T\n\nIntro.\n\n> [!faq]+ Folded\n> Hidden.\n\n> [!tip] Tip\n> Shown.\n\nOutro.\n",
		"# T\n\nIntro.\n\n> [!faq]- Folded\n> Hidden.\n\n> [!bug] Tip\n> Shown more.\n\nOutro.\n",
	] {
		let previous = parse_with(before, ofm());
		let got = reparse_with(&previous, Arc::from(after), ofm());
		let expected = parse_with(after, ofm());
		assert_eq!(got.blocks, expected.blocks, "{after:?}");
		assert_eq!(got.content_id, expected.content_id);
	}
}

/// The plain reading text of the document's blocks.
fn reading(doc: &Document) -> String {
	let mut out = String::new();
	fn walk(blocks: &[Block], out: &mut String) {
		for block in blocks {
			match &block.kind {
				BlockKind::Paragraph(text)
				| BlockKind::Heading { text, .. } => {
					out.push_str(&plain_text(text));
					out.push('\n');
				}
				BlockKind::Code { text, .. } => {
					out.push_str(text);
					out.push('\n');
				}
				BlockKind::Quote { blocks, .. }
				| BlockKind::Footnote { blocks, .. }
				| BlockKind::Details { blocks, .. }
				| BlockKind::FrontMatter { blocks, .. } => walk(blocks, out),
				BlockKind::List { items, .. } => {
					for item in items {
						walk(&item.blocks, out);
					}
				}
				BlockKind::Table { rows, .. } => {
					for row in rows {
						for cell in row {
							out.push_str(&plain_text(cell));
							out.push('\t');
						}
						out.push('\n');
					}
				}
				BlockKind::Rule => out.push('\n'),
			}
		}
	}
	walk(&doc.blocks, &mut out);
	out
}

/// Every image spec in the document, in source order.
fn images(doc: &Document) -> Vec<String> {
	let mut out = Vec::new();
	fn walk(blocks: &[Block], out: &mut Vec<String>) {
		for block in blocks {
			match &block.kind {
				BlockKind::Paragraph(text)
				| BlockKind::Heading { text, .. } => {
					for inline in text {
						if let InlineKind::Image(image) = &inline.kind {
							out.push(image.src.to_string());
						}
					}
				}
				BlockKind::Quote { blocks, .. }
				| BlockKind::Footnote { blocks, .. } => walk(blocks, out),
				BlockKind::Details {
					summary, blocks, ..
				} => {
					for inline in summary {
						if let InlineKind::Image(image) = &inline.kind {
							out.push(image.src.to_string());
						}
					}
					walk(blocks, out);
				}
				BlockKind::FrontMatter { blocks, .. } => walk(blocks, out),
				BlockKind::List { items, .. } => {
					for item in items {
						walk(&item.blocks, out);
					}
				}
				BlockKind::Table { rows, .. } => {
					for row in rows {
						for cell in row {
							for inline in cell {
								if let InlineKind::Image(image) = &inline.kind {
									out.push(image.src.to_string());
								}
							}
						}
					}
				}
				_ => {}
			}
		}
	}
	walk(&doc.blocks, &mut out);
	out
}

#[test]
fn ofm_hides_inline_comments_everywhere() {
	for source in [
		"Heading %%note%% text\n",
		"# Heading %%note%% text\n",
		"- item %%note%% two\n",
		"**bold %%note%% tail**\n",
		"> quoted %%note%% text\n",
		"| a %%note%% | b |\n|---|---|\n| c | d %%x%% |\n",
		"%%%%\n",
	] {
		let doc = parse_with(source, ofm());
		let text = reading(&doc);
		assert!(!text.contains('%'), "{source}: {text:?}");
		assert!(!text.contains("note"), "{source}: {text:?}");
	}
	// An empty comment leaves no paragraph behind.
	assert!(parse_with("%%%%\n", ofm()).blocks.is_empty());
	// Trailing Markdown after a comment still parses.
	let doc = parse_with("%%comment%% _italic_\n", ofm());
	let BlockKind::Paragraph(p) = &doc.blocks[0].kind else {
		panic!("{:?}", doc.blocks[0].kind);
	};
	assert_eq!(plain_text(p), "italic");
	assert!(p.iter().any(|i| i.style.italic));
}

#[test]
fn ofm_inline_comment_spacing_keeps_words_apart() {
	let doc = parse_with("A %%note%% B\n", ofm());
	let BlockKind::Paragraph(p) = &doc.blocks[0].kind else {
		panic!();
	};
	assert_eq!(plain_text(p), "A B");
	// Obsidian joins text that the comment glued together.
	let doc = parse_with("word%%note%%word\n", ofm());
	let BlockKind::Paragraph(p) = &doc.blocks[0].kind else {
		panic!();
	};
	assert_eq!(plain_text(p), "wordword");
}

#[test]
fn gfm_keeps_percent_markers_literal() {
	let source = "A %%note%% B\n\n%%\nblock\n%%\ntail\n";
	let doc = parse(source);
	let text = reading(&doc);
	assert!(text.contains("%%note%%"), "{text:?}");
	assert!(text.contains("%%"), "{text:?}");
	assert_eq!(doc.blocks, parse_with(source, Dialect::default()).blocks);
}

#[test]
fn ofm_block_comments_span_blank_lines_and_blocks() {
	let source = "Intro.\n\n%%\n# Hidden\n\n[hidden]: https://hidden.test\n\n![hidden](hidden.png)\n\n$$x$$\n\n<div id=\"hidden-anchor\">hidden</div>\n%% Visible tail\n\nOutro.\n";
	let doc = parse_with(source, ofm());
	let text = reading(&doc);
	assert!(!text.contains("Hidden"), "{text:?}");
	assert!(!text.contains("hidden.test"), "{text:?}");
	assert!(!text.contains("hidden.png"), "{text:?}");
	assert!(text.contains("Visible tail"), "{text:?}");
	assert!(text.contains("Outro."), "{text:?}");
	assert_eq!(images(&doc), Vec::<String>::new());
	assert!(doc.blocks.iter().all(|block| block.anchors.is_empty()));
	// The hidden refdef does not resolve; the link stays literal text.
	let doc = parse_with("%% [x]: https://hidden.test\n%%\n\n[x]\n", ofm());
	let BlockKind::Paragraph(p) = &doc.blocks[0].kind else {
		panic!();
	};
	assert_eq!(plain_text(p), "[x]");
	assert!(p.iter().all(|i| i.style.link_url().is_none()));
}

#[test]
fn ofm_unclosed_block_comment_hides_the_rest() {
	let doc = parse_with(
		"Visible.\n\n%%\neverything below\n# Hidden heading\n",
		ofm(),
	);
	let text = reading(&doc);
	assert_eq!(text, "Visible.\n");
}

#[test]
fn ofm_unpaired_inline_marker_stays_literal() {
	let doc = parse_with("A %% note continues\n\nnext paragraph\n", ofm());
	let text = reading(&doc);
	assert!(text.contains("%%"), "{text:?}");
	assert!(text.contains("next paragraph"), "{text:?}");
	// An escaped marker never opens a comment.
	let doc = parse_with("A \\%% literal %% and text\n", ofm());
	let text = reading(&doc);
	assert!(text.contains("%%"), "{text:?}");
}

#[test]
fn ofm_comments_in_code_stay_literal() {
	// Inline code, single and multi backtick.
	for source in ["`%%code%%` text\n", "``a `%%x%%` b`` text\n"] {
		let doc = parse_with(source, ofm());
		let BlockKind::Paragraph(p) = &doc.blocks[0].kind else {
			panic!("{source}");
		};
		assert!(plain_text(p).contains("%%"), "{source}");
		assert!(p.iter().any(|i| i.style.code), "{source}");
	}
	// Fenced code, backtick and tilde.
	for source in ["```\n%%code%%\n```\n", "~~~\n%%code%%\n~~~\n"] {
		let doc = parse_with(source, ofm());
		let BlockKind::Code { text, .. } = &doc.blocks[0].kind else {
			panic!("{source}: {:?}", doc.blocks[0].kind);
		};
		assert!(text.contains("%%"), "{source}");
	}
	// Indented code.
	let doc = parse_with("para.\n\n    %%code%%\n", ofm());
	let BlockKind::Code { text, .. } = &doc.blocks[1].kind else {
		panic!("{:?}", doc.blocks[1].kind);
	};
	assert!(text.contains("%%"));
	// Code inside a comment does not swallow the closing delimiter.
	let doc = parse_with("%%\n```\nfence inside\n%%\nVisible text.\n", ofm());
	let text = reading(&doc);
	assert_eq!(text, "Visible text.\n");
	assert!(
		!doc.blocks
			.iter()
			.any(|b| matches!(b.kind, BlockKind::Code { .. })),
		"a hidden fence must not become a real code block"
	);
	// A comment inside a quote-nested fence stays code.
	let doc = parse_with("> [!note] Title\n> ```\n> %%stay%%\n> ```\n", ofm());
	let BlockKind::Quote { blocks, .. } = &doc.blocks[0].kind else {
		panic!();
	};
	let BlockKind::Code { text, .. } = &blocks[0].kind else {
		panic!("{:?}", blocks[0].kind);
	};
	assert!(text.contains("%%stay%%"));
}

#[test]
fn ofm_comments_keep_source_positions_and_bytes() {
	let source = "a %%隐藏note%% 中文\n\nb %%x%% c\n";
	let doc = parse_with(source, ofm());
	assert_eq!(&*doc.source, source);
	let BlockKind::Paragraph(p) = &doc.blocks[0].kind else {
		panic!();
	};
	assert_eq!(plain_text(p), "a 中文");
	// A range may cover the hidden span the way a quote's range covers its
	// markers, but it must stay on UTF-8 boundaries of the original source
	// and start and end on the visible bytes.
	let visible = &doc.source[p[0].source.clone()];
	assert!(
		visible.starts_with("a ") && visible.ends_with("中文"),
		"{visible:?}"
	);
	let BlockKind::Paragraph(p) = &doc.blocks[1].kind else {
		panic!();
	};
	assert_eq!(plain_text(p), "b c");
	assert_eq!(&doc.source[p[0].source.clone()], "b %%x%% c");
	// CRLF sources keep whole ranges too.
	let doc = parse_with("a %%x%%\r\nb\r\n", ofm());
	let BlockKind::Paragraph(p) = &doc.blocks[0].kind else {
		panic!();
	};
	assert_eq!(plain_text(p), "a b");
}

#[test]
fn ofm_hidden_constructs_do_not_alter_numbering_or_anchors() {
	// A hidden footnote reference and heading leave visible numbering and
	// anchors untouched.
	let source = "Visible one[^a].\n\n%% Hidden [^b] and # Hidden heading %%\n\nVisible two[^b].\n\n[^a]: Note A\n[^b]: Note B\n";
	let doc = parse_with(source, ofm());
	let BlockKind::Paragraph(p) = &doc.blocks[0].kind else {
		panic!();
	};
	let refs: Vec<_> = p
		.iter()
		.filter_map(|i| match &i.kind {
			InlineKind::FootnoteRef(n) => Some(*n),
			_ => None,
		})
		.collect();
	assert_eq!(refs, vec![1]);
	let BlockKind::Paragraph(p) = &doc.blocks[1].kind else {
		panic!("{:?}", doc.blocks[1].kind);
	};
	let refs: Vec<_> = p
		.iter()
		.filter_map(|i| match &i.kind {
			InlineKind::FootnoteRef(n) => Some(*n),
			_ => None,
		})
		.collect();
	assert_eq!(refs, vec![2], "hidden note must not take a number");
	// Headings after a hidden one keep clean slugs.
	let doc = parse_with("%% # Heading %%\n\n# 中文 Heading\n", ofm());
	let BlockKind::Heading { anchor, .. } = &doc.blocks[0].kind else {
		panic!("{:?}", doc.blocks[0].kind);
	};
	assert!(!anchor.is_empty());
	assert!(!doc.blocks[0].anchors.iter().any(|a| a.contains("hidden")));
}

#[test]
fn ofm_front_matter_with_markers_is_metadata() {
	let source =
		"---\ntitle: %%not a comment%%\n---\n\nBody %%hidden%% text.\n";
	let doc = parse_with(source, ofm());
	let BlockKind::FrontMatter { blocks, .. } = &doc.blocks[0].kind else {
		panic!("{:?}", doc.blocks[0].kind);
	};
	let BlockKind::Code { text, .. } = &blocks[0].kind else {
		panic!();
	};
	assert!(text.contains("%%not a comment%%"));
	let BlockKind::Paragraph(p) = &doc.blocks[1].kind else {
		panic!();
	};
	assert_eq!(plain_text(p), "Body text.");
}

#[test]
fn ofm_comments_inside_details_and_callouts() {
	let source = "<details>\n<summary>Summary %%hidden%%</summary>\n\nBody %%gone%% here.\n\n</details>\n";
	let doc = parse_with(source, ofm());
	let BlockKind::Details {
		summary, blocks, ..
	} = &doc.blocks[0].kind
	else {
		panic!("{:?}", doc.blocks[0].kind);
	};
	assert_eq!(plain_text(summary), "Summary %%hidden%%");
	let BlockKind::Paragraph(p) = &blocks[0].kind else {
		panic!("{:?}", blocks[0].kind);
	};
	assert_eq!(plain_text(p), "Body here.");
	// Callout bodies hide comments too.
	let doc = parse_with("> [!tip] Title\n> Body %%gone%% kept.\n", ofm());
	let (callout, blocks) = callout_of(&doc.blocks[0]);
	assert_eq!(callout.title, "Title");
	let BlockKind::Paragraph(p) = &blocks[0].kind else {
		panic!();
	};
	assert_eq!(plain_text(p), "Body kept.");
}

#[test]
fn ofm_comment_edits_match_a_full_parse() {
	let before = "# T\n\nIntro.\n\n%%\nhidden block\n%%\n\nOutro.\n";
	for after in [
		"# T\n\nIntro, edited.\n\n%%\nhidden block\n%%\n\nOutro.\n",
		"# T\n\nIntro.\n\n%%\nhidden changed\n%%\n\nOutro.\n",
		"# T\n\nIntro.\n\nOutro.\n",
		"# T\n\nIntro.\n\n%%\nunclosed now\n",
	] {
		let previous = parse_with(before, ofm());
		// Comment documents leave the incremental fast path.
		assert!(
			parse_incremental(&previous, Arc::from(after), ofm()).is_none()
		);
		let got = reparse_with(&previous, Arc::from(after), ofm());
		let expected = parse_with(after, ofm());
		assert_eq!(got.blocks, expected.blocks, "{after:?}");
		assert_eq!(got.content_id, expected.content_id);
	}
}

#[test]
fn ofm_comment_prefixes_never_expose_hidden_text() {
	let source: Arc<str> =
		Arc::from("Intro.\n\n%%\nhidden across\n\nlines\n%%\n\nTail.\n");
	let full = parse_with(source.as_ref(), ofm());
	// A cut before the comment shows only the blocks the full parse has.
	assert!(parse_prefix_with(&source, 8, ofm()).is_none());
	assert_eq!(plain_text_of(&full.blocks[0]), "Intro.");
	// A cut inside the comment refuses a prefix: no transient exposure.
	assert!(parse_prefix_with(&source, 20, ofm()).is_none());
	let source: Arc<str> = Arc::from("Intro. %%hidden tail\n");
	let full = parse_with(source.as_ref(), ofm());
	// An unclosed inline marker stays literal, so the prefix still matches.
	assert!(parse_prefix_with(&source, 9, ofm()).is_none());
	assert!(reading(&full).contains("%%hidden tail"));
}

/// `plain_text` over one block's first paragraph-level rich text.
fn plain_text_of(block: &Block) -> String {
	match &block.kind {
		BlockKind::Paragraph(p) | BlockKind::Heading { text: p, .. } => {
			plain_text(p)
		}
		_ => String::new(),
	}
}

#[test]
fn ofm_comments_review_code_boundaries() {
	for source in [
		"`first\n%%KEEP%%\nlast`\n",
		"> ```\n> %%KEEP%%\n> ```\n",
		"- ```\n  %%KEEP%%\n  ```\n",
	] {
		assert!(
			reading(&parse_with(source, ofm())).contains("%%KEEP%%"),
			"{source}"
		);
	}
	for source in [
		"\\` %%SECRET%% \\`\n",
		"> ```\n> literal\n\n%%SECRET%%\n\nVisible\n",
		"- ```\n  literal\n\n%%SECRET%%\n\nVisible\n",
		"%% ` fake %% Visible\n",
		"%%\n```\n%%\n`%%KEEP%%` Visible\n",
	] {
		let text = reading(&parse_with(source, ofm()));
		assert!(!text.contains("SECRET"), "{source}: {text}");
		if source.contains("Visible") {
			assert!(text.contains("Visible"), "{source}: {text}");
		}
		if source.contains("KEEP") {
			assert!(text.contains("%%KEEP%%"), "{source}: {text}");
		}
	}
}

#[test]
fn ofm_comments_review_single_scan_and_opaque() {
	assert_eq!(
		reading(&parse_with("%%closed%% %% unclosed\n\nVisible", ofm())),
		"%% unclosed\nVisible\n"
	);
	for source in [
		"$a%%KEEP%%b$",
		"$$\na%%KEEP%%b\n$$",
		"<code>%%KEEP%%</code>",
		"<pre>%%KEEP%%</pre>",
	] {
		assert_eq!(
			parse_with(source, ofm()).blocks,
			parse_with(
				source,
				Dialect {
					ofm: false,
					..ofm()
				}
			)
			.blocks,
			"{source}"
		);
	}
	let doc = parse_with(
		"%%\n[x]: https://hidden.test\n%%\n\n<details><summary>S</summary>\n\n[label][x] %%SECRET%%\n\n</details>",
		ofm(),
	);
	assert!(reading(&doc).contains("[label][x]"));
	assert!(!reading(&doc).contains("SECRET"));
}

#[test]
fn ofm_comments_remain_hidden_in_depth_fallbacks() {
	let source = format!(
		"{}Visible %%SECRET%% text.\n\n{}",
		"<details><summary>S</summary>\n\n".repeat(300),
		"</details>\n\n".repeat(300)
	);
	let doc = parse_with(source.as_str(), ofm());
	assert!(!reading(&doc).contains("SECRET"));
	fn check(blocks: &[Block], source: &str) {
		for block in blocks {
			assert!(source.get(block.source.clone()).is_some());
			match &block.kind {
				BlockKind::Paragraph(text)
				| BlockKind::Heading { text, .. } => {
					for inline in text {
						assert!(source.get(inline.source.clone()).is_some());
					}
				}
				BlockKind::Details {
					summary, blocks, ..
				} => {
					for inline in summary {
						assert!(source.get(inline.source.clone()).is_some());
					}
					check(blocks, source);
				}
				BlockKind::Quote { blocks, .. }
				| BlockKind::FrontMatter { blocks, .. }
				| BlockKind::Footnote { blocks, .. } => check(blocks, source),
				BlockKind::List { items, .. } => {
					for item in items {
						check(&item.blocks, source);
					}
				}
				BlockKind::Table { rows, .. } => {
					for row in rows {
						for cell in row {
							for inline in cell {
								assert!(
									source.get(inline.source.clone()).is_some()
								);
							}
						}
					}
				}
				_ => {}
			}
		}
	}
	check(&doc.blocks, source.as_str());
	let source = "> [!note] 标题\r\n> 中文 %%隐藏%% tail\r\n\r\n<details><summary>中文 %%隐藏%%</summary>\r\n\r\n中文 %%隐藏%% tail\r\n\r\n</details>\r\n";
	let doc = parse_with(source, ofm());
	check(&doc.blocks, source);
	assert_eq!(&*doc.source, source);
	assert!(!reading(&doc).contains("隐藏"));
}

#[test]
fn ofm_comments_protect_code_in_html_snippets() {
	// A fenced block and a backtick span both protect `%%` markers when the
	// body reads as Markdown, because comrak scans the code span closed
	// before the comment handler runs.
	let source =
		"<details>\n<summary>S</summary>\n\n```\n%%KEEP%%\n```\n\n</details>\n";
	assert!(
		reading(&parse_with(source, ofm())).contains("%%KEEP%%"),
		"{source}"
	);
	let source = "<details>\n<summary>S</summary>\n\n`first\n%%KEEP%%\nlast`\n\n</details>\n";
	assert!(
		reading(&parse_with(source, ofm())).contains("%%KEEP%%"),
		"{source}"
	);
	// A one-line `<details>` keeps its body as raw HTML, including `%%`.
	let source =
		"<details><summary>S</summary>\n`first\n%%GONE%%\nlast`\n</details>";
	let text = reading(&parse_with(source, ofm()));
	assert!(text.contains("%%GONE%%"), "{source}: {text}");
	assert!(text.contains("first"), "{source}: {text}");
}

#[test]
fn ofm_block_comments_follow_nested_list_content_lines() {
	let source =
		"- outer\n  - inner\n\n    %%\n    SECRET\n\n    %%\n    Visible\n";
	let text = reading(&parse_with(source, ofm()));
	assert!(!text.contains("SECRET"), "{text}");
	assert!(text.contains("Visible"), "{text}");
}

#[test]
fn ofm_comments_respect_escaped_html_and_paragraph_code_boundaries() {
	for source in [
		"\\<code> %%SECRET%% </code>",
		"`unpaired\n\n%%SECRET%%\n\nVisible `",
	] {
		let text = reading(&parse_with(source, ofm()));
		assert!(!text.contains("SECRET"), "{source}: {text}");
	}
}

#[test]
fn ofm_comments_do_not_let_hidden_html_own_visible_code() {
	// A fenced comment block ends at its `%%` close; the code span and the
	// text after it stay visible.
	let source = "%%\n<script>\n%%\n`%%KEEP%%` Visible";
	let text = reading(&parse_with(source, ofm()));
	assert!(text.contains("%%KEEP%%"), "{source}: {text}");
	assert!(text.contains("Visible"), "{source}: {text}");
	// An inline comment on the first line leaves the ``` mid-line, so it
	// reads as literal text rather than opening a fence; the `%%KEEP%%` on
	// its own line is another inline comment and hides with it.
	let source = "%%x%% ```\n%%KEEP%%\n```\nVisible";
	let text = reading(&parse_with(source, ofm()));
	assert!(!text.contains("KEEP"), "{source}: {text}");
	assert!(text.contains("Visible"), "{source}: {text}");
}

#[test]
fn ofm_review02_html_code_context() {
	for source in [
		"`<code>`\n\n%%SECRET%%\n\n</code>\n",
		"<!-- <code> -->\n\n%%SECRET%%\n\n</code>\n",
	] {
		let text = reading(&parse_with(source, ofm()));
		assert!(!text.contains("SECRET"), "{source}: {text}");
	}
}

#[test]
fn ofm_review02_html_code_closures() {
	for source in [
		"<code>%%KEEP%%</code >\n",
		"<code>%%KEEP%%\n",
		"<pre>\n%%KEEP%%\n",
		"<CODE>%%KEEP%%</CODE \t>\n",
	] {
		let doc = parse_with(source, ofm());
		assert!(
			reading(&doc).contains("%%KEEP%%"),
			"{source}: {}",
			reading(&doc)
		);
		assert_eq!(
			doc.blocks,
			parse_with(
				source,
				Dialect {
					ofm: false,
					..ofm()
				}
			)
			.blocks,
			"{source}"
		);
	}
}

#[test]
fn ofm_review02_many_comments_preserve_visible_code() {
	for unit in [
		"text %%SECRET%% ",
		"`%%KEEP%%` text %%SECRET%%\n",
		"%%SECRET%%\n",
		"%%\nSECRET plain text\n%%\n\n",
	] {
		let source = unit.repeat(2000);
		let text = reading(&parse_with(source.as_str(), ofm()));
		assert!(!text.contains("SECRET"));
		assert_eq!(
			text.matches("%%KEEP%%").count(),
			if unit.contains("KEEP") { 2000 } else { 0 }
		);
	}
}

#[test]
fn ofm_review02_html_code_scope_boundaries() {
	let source = "<code>%%KEEP%%\n\n%%SECRET%%\n";
	let text = reading(&parse_with(source, ofm()));
	assert!(text.contains("%%KEEP%%"), "{text}");
	assert!(!text.contains("SECRET"), "{text}");
	let source = "<code><!-- </code> -->%%KEEP%%</code>\n";
	assert!(reading(&parse_with(source, ofm())).contains("%%KEEP%%"));
}

#[test]
fn ofm_review02_plain_block_exposes_indented_code() {
	let source = "%%\nplain hidden words\n%% \n    %%KEEP%%\n";
	let text = reading(&parse_with(source, ofm()));
	assert!(text.contains("%%KEEP%%"), "{text}");
}

#[test]
fn ofm_inline_html_code_ignores_markdown_code_closers() {
	for source in [
		"<code>`</code>` %%KEEP%%</code>\n",
		"<code><code>`</code>`</code > %%KEEP%%</code>\n",
		"<code>`</code>` %%KEEP%%\n",
		"<code><!-- </code> --> %%KEEP%%</code>\r\n",
	] {
		let dialect = ofm();
		let control = parse_with(
			source,
			Dialect {
				ofm: false,
				..dialect
			},
		);
		let text = reading(&control);
		assert!(text.contains("%%KEEP%%"), "control: {source}: {text}");
		let doc = parse_with(source, dialect);
		assert_eq!(doc.blocks, control.blocks, "{source}");
		assert_eq!(&*doc.source, source);
	}
	let source = "<code>`</code>` %%KEEP%%</code > %%SECRET%%\n";
	let doc = parse_with(source, ofm());
	let text = reading(&doc);
	assert!(text.contains("%%KEEP%%"), "{text}");
	assert!(!text.contains("SECRET"), "{text}");
}

#[test]
fn ofm_html_code_scopes_follow_markdown_recursion_child_open() {
	for (open, close) in [
		("*", "*"),
		("**", "**"),
		("~~", "~~"),
		("[", "](https://example.test)"),
	] {
		let dialect = ofm();
		let control_dialect = Dialect {
			ofm: false,
			..dialect
		};
		let source = format!("{open}<code>inside{close} %%SECRET%%\n");
		let control = parse_with(source.as_str(), control_dialect);
		let text = reading(&control);
		assert!(text.contains("%%SECRET%%"), "{source}: {text}");
		let BlockKind::Paragraph(inlines) = &control.blocks[0].kind else {
			panic!();
		};
		assert!(inlines.iter().any(|inline| matches!(&inline.kind, InlineKind::Text(text) if text.contains("SECRET")) && !inline.style.code), "{source}: {inlines:?}");
		let doc = parse_with(source.as_str(), dialect);
		let expected =
			parse_with(source.replace("%%SECRET%%", ""), control_dialect);
		assert_eq!(reading(&doc), reading(&expected), "{source}");
		assert!(!reading(&doc).contains("SECRET"), "{source}");
		assert_eq!(&*doc.source, source);
	}
}

#[test]
fn ofm_html_code_scopes_follow_markdown_recursion_child_close() {
	for (open, close) in [
		("*", "*"),
		("**", "**"),
		("~~", "~~"),
		("[", "](https://example.test)"),
	] {
		let dialect = ofm();
		let control_dialect = Dialect {
			ofm: false,
			..dialect
		};
		let source = format!("<code>{open}inside </code>{close} %%KEEP%%\n");
		let control = parse_with(source.as_str(), control_dialect);
		let BlockKind::Paragraph(inlines) = &control.blocks[0].kind else {
			panic!();
		};
		assert!(inlines.iter().any(|inline| matches!(&inline.kind, InlineKind::Text(text) if text.contains("KEEP")) && inline.style.code), "{source}: {inlines:?}");
		let doc = parse_with(source.as_str(), dialect);
		assert_eq!(doc.blocks, control.blocks, "{source}");
		assert_eq!(&*doc.source, source);
	}
}

#[test]
fn ofm_comments_preserve_upstream_wikilinks() {
	let source = "Before %%SECRET%% [[Notes/A#A heading|Visible]] after.\n";
	for dialect in [Dialect::default(), ofm()] {
		let doc = parse_with(source, dialect);
		let BlockKind::Paragraph(text) = &doc.blocks[0].kind else {
			panic!()
		};
		assert_eq!(
			text.iter()
				.filter_map(|span| span.style.link_url())
				.collect::<Vec<_>>(),
			["Notes/A#a-heading"]
		);
		assert_eq!(plain_text(text).contains("SECRET"), !dialect.ofm);
		assert!(plain_text(text).contains("Visible"));
		assert_eq!(&*doc.source, source);
		for span in text {
			assert!(source.get(span.source.clone()).is_some());
		}
	}
}

#[test]
fn ofm_comments_restore_link_occurrences_to_original_bytes() {
	for (body, marker) in [
		("[one **bold**](/same) [two](/same)", "["),
		(
			"<p><a href=\"/same\">one <b>bold</b></a> <a href=\"/same\">two</a></p>",
			"<a ",
		),
		("[[same|one]] [[same|two]]", "[["),
	] {
		for prefix in ["中文 %%隐藏%%\r\n\r\n", "%%隐藏%% "] {
			let source = format!("{prefix}{body}\n");
			let doc = parse_with(source.as_str(), ofm());
			let expected = source
				.match_indices(marker)
				.map(|(at, _)| at)
				.collect::<Vec<_>>();
			let mut origins = Vec::new();
			for block in &doc.blocks {
				let BlockKind::Paragraph(text) = &block.kind else {
					continue;
				};
				for span in text {
					if let Some(link) = &span.style.link
						&& origins.last() != Some(&link.origin())
					{
						origins.push(link.origin());
					}
				}
			}
			assert_eq!(origins, expected, "{source}");
		}
	}
}
