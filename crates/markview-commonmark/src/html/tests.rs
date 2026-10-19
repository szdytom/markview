use super::*;

/// The block scanner with the scope budget out of the way, so each test
/// states one rule; the budget has its own test.
fn block(source: &str) -> Block {
	super::block(source, usize::MAX, 0)
}

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
				anchor: None,
				image: None,
				text: "Title".into(),
				style: Style::default()
			}]
		}
	);
	assert_eq!(
		block("<p>a <em>b</em> c</p>\n"),
		Block::Paragraph(vec![
			Span {
				anchor: None,
				image: None,
				text: "a ".into(),
				style: Style::default()
			},
			Span {
				anchor: None,
				image: None,
				text: "b".into(),
				style: Style {
					italic: true,
					..Style::default()
				}
			},
			Span {
				anchor: None,
				image: None,
				text: " c".into(),
				style: Style::default()
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
				anchor: None,
				image: None,
				text: "Hello world".into(),
				style: Style::default()
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
			anchor: None,
			image: None,
			text: "a < b".into(),
			style: Style::default()
		}])
	);
	assert_eq!(
		block("<h2>中文</h2>\n"),
		Block::Heading {
			level: 2,
			text: vec![Span {
				anchor: None,
				image: None,
				text: "中文".into(),
				style: Style::default()
			}]
		}
	);
}

#[test]
fn tag_scanning_skips_html_comments() {
	for comment in ["<!-- <div> -->", "<!-- x > <div> -->", "<!-- </div> -->"] {
		let source = format!("{comment}<p>a</p>");
		let found: Vec<&str> = tags(&source)
			.map(|(start, len)| &source[start..start + len])
			.collect();
		assert_eq!(found, ["<p>", "</p>"], "{comment}");
	}
	// An unclosed comment swallows the rest of the block.
	assert!(tags("<!-- <div>").next().is_none());
}

#[test]
fn tag_scanning_skips_opaque_element_contents() {
	for body in [
		"<script></div></script>",
		"<style><div></style>",
		"<pre></div></pre>",
		"<textarea></div></textarea>",
		"<title></div></title>",
		"<svg></div></svg>",
		"<math></div></math>",
	] {
		let source = format!("{body}<p>a</p>");
		let found: Vec<&str> = tags(&source)
			.map(|(start, len)| &source[start..start + len])
			.collect();
		assert_eq!(found, ["<p>", "</p>"], "{body}");
	}
	// An unclosed opaque element swallows the rest of the block.
	assert!(tags("<pre></div>").next().is_none());
}

#[test]
fn a_stray_close_does_not_close_a_later_container() {
	assert!(!has_open_container("</div>\n"));
	assert!(has_open_container("</div>\n\n<div>\n"));
	assert!(has_open_container("<details>\n"));
}

#[test]
fn a_multibyte_space_exposed_by_a_stray_slash_is_skipped_whole() {
	// A fuzz finding: stripping the `/` of ` /<nbsp>open` exposes a two-byte
	// space, and the "starts with a separator" branch skipped it a byte at a
	// time, splitting the character.
	assert!(!has_attribute(" /", "open"));
	assert!(has_attribute(" /\u{a0}open", "open"));
	assert_eq!(attribute(" /\u{a0}src=x", "src").as_deref(), Some("x"));
	assert_eq!(attribute(" /\u{a0}src", "src"), None);
}

#[test]
fn block_style_follows_the_open_tags() {
	let bold = Style {
		bold: true,
		..Style::default()
	};
	// A close restores the style its contents inherited.
	assert_eq!(
		block("<b><i>x</i>y</b>\n"),
		Block::Paragraph(vec![
			Span {
				anchor: None,
				image: None,
				text: "x".into(),
				style: Style {
					bold: true,
					italic: true,
					..Style::default()
				}
			},
			Span {
				anchor: None,
				image: None,
				text: "y".into(),
				style: bold.clone()
			},
		])
	);
	// Closing an outer tag drops the styles the tags inside it added.
	assert_eq!(
		block("<b><i>x</b>y\n"),
		Block::Paragraph(vec![
			Span {
				anchor: None,
				image: None,
				text: "x".into(),
				style: Style {
					bold: true,
					italic: true,
					..Style::default()
				}
			},
			Span {
				anchor: None,
				image: None,
				text: "y".into(),
				style: Style::default()
			},
		])
	);
	// Repeating a tag adds nothing, and the order of two different tags does
	// not reach the run, so runs that resolve alike share one span.
	assert_eq!(
		block("<b><b>x\n"),
		Block::Paragraph(vec![Span {
			anchor: None,
			image: None,
			text: "x".into(),
			style: bold
		}])
	);
	assert_eq!(
		block("<b><i>a</i></b><i><b>b\n"),
		Block::Paragraph(vec![Span {
			anchor: None,
			image: None,
			text: "ab".into(),
			style: Style {
				bold: true,
				italic: true,
				..Style::default()
			}
		}])
	);
}

#[test]
fn block_links_nest_and_images_carry_the_run_style() {
	let linked = |url: &str| Style {
		link: Some(crate::document::Link::new(url, 0)),
		..Style::default()
	};
	// A nested link overrides the outer one, which comes back on its close.
	assert_eq!(
		block("<a href=\"/a\">x<a href=\"/b\">y</a>z</a>\n"),
		Block::Paragraph(vec![
			Span {
				anchor: None,
				image: None,
				text: "x".into(),
				style: linked("/a")
			},
			Span {
				anchor: None,
				image: None,
				text: "y".into(),
				style: linked("/b")
			},
			Span {
				anchor: None,
				image: None,
				text: "z".into(),
				style: linked("/a")
			},
		])
	);
	// An anchor without `href` keeps the link its contents inherited.
	assert_eq!(
		block("<a href=\"/a\">x<a name=\"n\">y</a>z</a>\n"),
		Block::Paragraph(vec![
			Span {
				anchor: None,
				image: None,
				text: "x".into(),
				style: linked("/a")
			},
			Span {
				anchor: Some("n".into()),
				image: None,
				text: String::new(),
				style: linked("/a")
			},
			Span {
				anchor: None,
				image: None,
				text: "yz".into(),
				style: linked("/a")
			},
		])
	);
	// An image is its own run and keeps the style around it.
	let parsed = block("<b><img src=\"a.png\"></b>\n");
	let Block::Paragraph(spans) = parsed else {
		panic!("expected a paragraph");
	};
	assert_eq!(spans.len(), 1);
	assert!(spans[0].image.is_some());
	assert!(spans[0].style.bold);
}

#[test]
fn open_tags_are_budgeted() {
	// Past the budget the block is kept as source, like any other markup the
	// scanner cannot represent.
	assert_eq!(
		super::block(
			"<b><b><b>x</b>
",
			2,
			0
		),
		Block::Unsupported
	);
	assert!(matches!(
		super::block(
			"<b><b>x</b>
",
			2,
			0
		),
		Block::Paragraph(_)
	));
}
