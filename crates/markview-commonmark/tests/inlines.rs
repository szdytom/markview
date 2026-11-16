use markview_commonmark::{Dialect, MarkdownParse, parse_with};
use markview_document::{BlockKind, Inline, InlineKind, plain_text};
use proptest::prelude::*;

fn parse(source: &str) -> MarkdownParse {
	let parsed = parse_with(
		source,
		Dialect {
			ofm: true,
			..Dialect::default()
		},
	);
	assert_eq!(parsed.map.validate(source, &parsed.document), Ok(()));
	parsed
}

fn paragraph(parsed: &MarkdownParse) -> &[Inline] {
	let BlockKind::Paragraph(rich) = parsed.blocks()[0].kind() else {
		panic!("expected paragraph")
	};
	rich
}

fn anchors(rich: &[Inline]) -> Vec<&str> {
	rich.iter()
		.filter_map(|inline| match &inline.kind {
			InlineKind::Anchor(name) => Some(name.as_str()),
			_ => None,
		})
		.collect()
}

#[test]
fn misnested_html_closes_restore_the_matching_scope() {
	let parsed = parse("head <b>bold <em>both</b> plain</em> tail");
	let rich = paragraph(&parsed);
	assert_eq!(plain_text(rich), "head bold both plain tail");
	for (word, bold, italic) in [
		("head", false, false),
		("bold", true, false),
		("both", true, true),
		("plain", false, false),
		("tail", false, false),
	] {
		let inline = rich.iter().find(|inline| {
			matches!(&inline.kind, InlineKind::Text(text) if text.contains(word))
		}).unwrap();
		assert_eq!((inline.style.bold, inline.style.italic), (bold, italic));
	}
}

#[test]
fn pending_comment_spacing_survives_skipped_tags_and_recursive_children() {
	for (source, expected) in [
		("A %%hidden%%<b></b> B", "A B"),
		("A %%hidden%%**B %%inner%%** C", "A B C"),
		("A %%hidden%%<svg/> B", "A  B"),
		("A %%hidden%%<a id='gap'></a> B", "A B"),
		("A %%hidden%%", "A"),
		("*A %%hidden%%* tail", "A  tail"),
	] {
		assert_eq!(plain_text(paragraph(&parse(source))), expected, "{source}");
	}
}

#[test]
fn opaque_boundaries_cross_markdown_recursion_and_filter_svg_anchors() {
	let source = "head *<script><svg id='hidden-svg'><text id='hidden-text'>x</text></svg><a id='hidden'>inside*</script> <svg id='shown-svg'/> <a id='shown'></a> tail";
	let parsed = parse(source);
	let rich = paragraph(&parsed);
	assert_eq!(anchors(rich), ["shown-svg", "shown"]);
	assert_eq!(
		rich.iter()
			.filter(|inline| matches!(inline.kind, InlineKind::Image(_)))
			.count(),
		2
	);
	let tail = rich.iter().find(|inline| matches!(&inline.kind, InlineKind::Text(text) if text.contains("tail"))).unwrap();
	assert!(!tail.style.italic);
	let images: Vec<_> = rich
		.iter()
		.filter(|inline| matches!(inline.kind, InlineKind::Image(_)))
		.collect();
	assert!(images[0].style.italic);
	assert!(!images[1].style.italic);
	for image in images {
		let raw =
			&source[parsed.inline_source(&parsed.blocks()[0], image).unwrap()];
		assert!(raw.starts_with("<svg"));
		assert!(raw.ends_with("</svg>") || raw.ends_with("/>"));
	}
}

#[test]
fn image_alt_text_has_independent_style_and_anchor_state() {
	let source = "<b>![<a id='alt'>label</a> <svg id='alt-svg'/>](image.png)</b> <a id='shown'></a> tail";
	let parsed = parse(source);
	let rich = paragraph(&parsed);
	assert_eq!(anchors(rich), ["shown"]);
	let image = rich
		.iter()
		.find(|inline| matches!(inline.kind, InlineKind::Image(_)))
		.unwrap();
	let InlineKind::Image(spec) = &image.kind else {
		unreachable!()
	};
	assert_eq!(spec.alt.trim(), "label");
	assert!(image.style.bold);
	assert_eq!(
		&source[parsed.inline_source(&parsed.blocks()[0], image).unwrap()],
		"![<a id='alt'>label</a> <svg id='alt-svg'/>](image.png)"
	);
}

proptest! {
	#[test]
	fn inline_svg_consumption_preserves_overlapping_runs_and_link_origins(
		text in "[a-z中é😀]{1,16}",
		marker in prop::sample::select(vec!["*", "**", "`", "$"]),
		newline in prop::sample::select(vec!["\n", "\r\n", "\r"]),
	) {
		let source = format!("lead <b><svg><text>{marker}hidden{newline}text</text></svg> {text}{marker}</b> <a id='after'></a>[next](target) [last](target)");
		let parsed = parse(&source);
		let rich = paragraph(&parsed);
		prop_assert_eq!(plain_text(rich), format!("lead  {text} next last"));
		prop_assert_eq!(anchors(rich), ["after"]);
		let image = rich.iter().find(|inline| matches!(inline.kind, InlineKind::Image(_))).unwrap();
		prop_assert!(image.style.bold);
		prop_assert!(!image.style.code);
		let svg_end = source.find("</svg>").unwrap() + "</svg>".len();
		prop_assert_eq!(parsed.inline_source(&parsed.blocks()[0], image), Some(source.find("<svg").unwrap()..svg_end));
		let expected_tail = format!(" {text}");
		let tail = rich.iter().find(|inline| match &inline.kind {
			InlineKind::Text(value) => value == &expected_tail,
			InlineKind::Math { latex, .. } => latex == &expected_tail,
			_ => false,
		}).unwrap();
		prop_assert!(tail.style.bold);
		prop_assert_eq!(tail.style.code, marker == "`");
		prop_assert_eq!(tail.style.italic, marker == "*");
		let range = parsed.inline_source(&parsed.blocks()[0], tail).unwrap();
		let closing_len = usize::from(matches!(marker, "`" | "$"));
		prop_assert_eq!(range, svg_end..svg_end + 1 + text.len() + closing_len);
		let links: Vec<_> = rich.iter().filter_map(|inline| inline.link.as_ref()).collect();
		prop_assert_eq!(links.len(), 2);
		prop_assert_eq!(links[0].as_str(), "target");
		prop_assert_eq!(links[1].as_str(), "target");
		prop_assert_ne!(links[0].occurrence(), links[1].occurrence());
	}
}

#[test]
fn mismatched_strikethrough_does_not_stop_following_emphasis() {
	let parsed = parse("~~a~ *b*");
	let rich = paragraph(&parsed);
	assert_eq!(plain_text(rich), "~~a~ b");
	assert!(rich.iter().any(|inline| inline.style.italic
		&& matches!(&inline.kind, InlineKind::Text(text) if text == "b")));
	assert!(rich.iter().all(|inline| !inline.style.strike));
}

#[test]
fn email_entities_stay_text_and_wrapped_domains_autolink() {
	let parsed = parse("foo&#x40;bar.com _www.example.com_");
	let rich = paragraph(&parsed);
	assert_eq!(plain_text(rich), "foo@bar.com www.example.com");
	let links: Vec<_> = rich.iter().filter_map(Inline::link_url).collect();
	assert_eq!(links, ["http://www.example.com"]);
	assert!(rich.iter().any(|inline| inline.style.italic
		&& inline.link_url() == Some("http://www.example.com")));
}

#[test]
fn task_markers_require_literal_brackets_and_following_whitespace() {
	let parsed = parse("- [x]*a*\n- &#91;x] b\n- [x]\t*c*\n- [ ]\n");
	let BlockKind::List { items, .. } = parsed.blocks()[0].kind() else {
		panic!("expected list");
	};
	let checked: Vec<_> = items.iter().map(|item| item.checked).collect();
	assert_eq!(checked, [None, None, Some(true), Some(false)]);
	let BlockKind::Paragraph(rich) = items[0].blocks[0].kind() else {
		panic!("expected paragraph");
	};
	assert_eq!(plain_text(rich), "[x]a");
	assert!(rich.iter().any(|inline| inline.style.italic));
}
