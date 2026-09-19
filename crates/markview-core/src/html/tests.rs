use super::*;

#[test]
fn inline_tags_map_and_attributes_are_ignored() {
	assert_eq!(
		inline("<b class=\"x\" style=\"color:red\">"),
		Inline::Open {
			name: "b".into(),
			patch: Patch::Bold
		}
	);
	assert_eq!(
		inline("<strong>"),
		Inline::Open {
			name: "strong".into(),
			patch: Patch::Bold
		}
	);
	assert_eq!(inline("</EM>"), Inline::Close { name: "em".into() });
	assert_eq!(
		inline("<a href=\"/a?x=1&amp;y=2\" title=\"z\">"),
		Inline::Open {
			name: "a".into(),
			patch: Patch::Link("/a?x=1&y=2".into())
		}
	);
	assert_eq!(
		inline("<a name=\"x\">"),
		Inline::Open {
			name: "a".into(),
			patch: Patch::None
		}
	);
	assert_eq!(inline("<br/>"), Inline::Break);
	assert_eq!(inline("<!-- hidden -->"), Inline::Ignore);
	assert_eq!(inline("<!DOCTYPE html>"), Inline::Ignore);
}

#[test]
fn unsupported_markup_keeps_the_source() {
	assert_eq!(inline("<span>"), Inline::Literal);
	assert_eq!(inline("</span>"), Inline::Literal);
	assert!(matches!(inline("<img src=\"a.png\">"), Inline::Image(_)));
	assert_eq!(inline("not a tag"), Inline::Literal);
}

#[test]
fn blocks_map_to_rule_heading_and_paragraph() {
	assert_eq!(block("<hr>\n"), Block::Rule);
	assert_eq!(block("<!-- gone -->\n"), Block::Empty);
	assert_eq!(
		block("<h2>Title</h2>\n"),
		Block::Heading {
			level: 2,
			text: vec![Span {
				image: None,
				text: "Title".into(),
				styles: vec![]
			}]
		}
	);
	assert_eq!(
		block("<p>a <em>b</em> c</p>\n"),
		Block::Paragraph(vec![
			Span {
				image: None,
				text: "a ".into(),
				styles: vec![]
			},
			Span {
				image: None,
				text: "b".into(),
				styles: vec![Patch::Italic]
			},
			Span {
				image: None,
				text: " c".into(),
				styles: vec![]
			},
		])
	);
	// A run of elements is one paragraph, not a heading plus leftovers.
	assert!(matches!(block("<h2>A</h2><p>B</p>\n"), Block::Paragraph(_)));
}

#[test]
fn container_markup_and_unclosed_comments_fall_back() {
	assert_eq!(block("<div class=\"x\">\n"), Block::Unsupported);
	assert_eq!(block("<ul><li>a</li></ul>\n"), Block::Unsupported);
	assert_eq!(block("<!-- open\n"), Block::Empty);
}

#[test]
fn block_text_collapses_whitespace_across_lines() {
	assert_eq!(
		block("<h1>\n  Hello\n  world\n</h1>\n"),
		Block::Heading {
			level: 1,
			text: vec![Span {
				image: None,
				text: "Hello world".into(),
				styles: vec![]
			}]
		}
	);
}

#[test]
fn malformed_markup_is_readable_and_never_panics() {
	for fragment in [
		"<",
		"<>",
		"</>",
		"<b",
		"<!--",
		"<!DOCTYPE",
		"<?php ?>",
		"<a href=\"unterminated",
		"<a href='x",
		"<a href=>",
		"<b >",
		"</b >",
		"<!>",
		"< >",
		"<中文>",
		"<b>中文</b>",
		"<h10>",
		"<H2>",
		"<b title=\"a>b\">",
	] {
		let _ = inline(fragment);
	}
	// A literal `<` inside text stays text instead of eating the tag.
	assert_eq!(
		block("<p>a < b</p>\n"),
		Block::Paragraph(vec![Span {
			image: None,
			text: "a < b".into(),
			styles: vec![]
		}])
	);
	assert_eq!(
		block("<h2>中文</h2>\n"),
		Block::Heading {
			level: 2,
			text: vec![Span {
				image: None,
				text: "中文".into(),
				styles: vec![]
			}]
		}
	);
}

#[test]
fn details_close_matches_nested_elements() {
	let Details::Inline { body, rest, .. } = details(
		"<details><summary>Outer</summary>\
		 A<details><summary>Inner</summary>B</details>C</details>",
	) else {
		panic!("expected a complete element")
	};
	assert_eq!(body, "A<details><summary>Inner</summary>B</details>C");
	assert!(rest.is_empty());
}

#[test]
fn details_keeps_adjacent_elements_in_one_block() {
	let Details::Inline { body, rest, .. } =
		details("<details>A</details>\n<details>B</details>")
	else {
		panic!("expected a complete element")
	};
	assert_eq!(body, "A");
	assert_eq!(rest, "\n<details>B</details>");
}

#[test]
fn self_closing_details_do_not_nest() {
	let Details::Inline { body, .. } =
		details("<details><summary>S</summary>a<details/>b</details>")
	else {
		panic!("expected a complete element")
	};
	assert_eq!(body, "a<details/>b");
}

#[test]
fn odd_details_openers_never_panic() {
	for source in [
		"<details class=\"x\"/>",
		"<details/>",
		"<details open/>",
		"<details =\"x\"/>",
		"<details a=b/>",
		"<details open",
		"<details '>",
		"<details>",
		"<details><summary>",
		"<details></details></details>",
	] {
		let _ = details(source);
		let _ = block(source);
	}
	// The inline path reads attributes too, and a self-closing tag can leave
	// nothing after its value.
	for source in [
		"<img class=\"x\"/>",
		"<img src/>",
		"<a class=\"x\"/>",
		"<a href/>",
		"<b class=x/>",
	] {
		for (start, len) in tags(source) {
			let _ = inline(&source[start..start + len]);
		}
	}
}

#[test]
fn close_tag_counts_tags_that_share_a_block() {
	let source = "</details>\n</details>\n";
	// The first close leaves one element open; the second one closes it.
	let (depth, close) = close_tag(source, 2);
	assert_eq!(depth, 0);
	let close = close.expect("the outer close");
	assert_eq!(close.start, source.rfind("</details>").unwrap());
	assert_eq!(&source[close], "</details>");
	// Starting one element lower, the first tag is the match.
	let (depth, close) = close_tag(source, 1);
	assert_eq!(depth, 0);
	assert_eq!(close.map(|range| range.start), Some(0));
	// An unbalanced block reports what is still open and no match.
	assert_eq!(close_tag("</details>\n", 2).0, 1);
	assert!(close_tag("<details>\n", 1).1.is_none());
}

#[test]
fn an_open_element_reports_nested_openers() {
	// The opening block leaves the inner element open too, so the closing scan
	// must start one level deeper.
	let Details::Open { depth, lead, .. } = details(
		"<details>\n<summary>Outer</summary>\n<details>\n<summary>Inner</summary>",
	) else {
		panic!("expected an open element")
	};
	assert_eq!(depth, 2);
	assert!(lead.contains("<details>"));
	// A plain opener leaves only itself open.
	let Details::Open { depth, .. } =
		details("<details>\n<summary>Only</summary>")
	else {
		panic!("expected an open element")
	};
	assert_eq!(depth, 1);
}

#[test]
fn a_summary_belongs_to_the_element_that_declares_it() {
	// The first element is complete and has no summary; the second element's
	// summary must not be adopted by it.
	let Details::Inline {
		summary,
		body,
		rest,
		..
	} = details("<details>A</details>\n<details><summary>B</summary>C</details>")
	else {
		panic!("expected a complete element")
	};
	assert_eq!(summary, None);
	assert_eq!(body, "A");
	assert_eq!(rest, "\n<details><summary>B</summary>C</details>");
	// A nested element's summary belongs to the nested element.
	let Details::Inline { summary, body, .. } =
		details("<details><details><summary>I</summary>B</details>C</details>")
	else {
		panic!("expected a complete element")
	};
	assert_eq!(summary, None);
	assert_eq!(body, "<details><summary>I</summary>B</details>C");
}
