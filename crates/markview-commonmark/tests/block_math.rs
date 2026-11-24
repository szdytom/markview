use markview_commonmark::{
	Dialect, MarkdownParse, parse, parse_with, reparse_with,
};
use markview_document::{Block, BlockKind, InlineKind};
use proptest::prelude::*;

fn math(block: &Block) -> &str {
	let BlockKind::Paragraph(text) = block.kind() else {
		panic!("expected math paragraph: {block:?}");
	};
	assert_eq!(text.len(), 1);
	let InlineKind::Math {
		latex,
		display: true,
	} = &text[0].kind
	else {
		panic!("expected display math: {text:?}");
	};
	latex
}

fn validate(parsed: &MarkdownParse) {
	assert_eq!(
		parsed.map.validate(&parsed.source, &parsed.document),
		Ok(())
	);
}

#[test]
fn issue_40_array_is_one_literal_display_block_by_default() {
	let body = concat!(
		"\\begin{array}{c}\n",
		"  0111'1001'0001_{BCD} \\\\\n",
		"+ 0110'0101'1000_{BCD} \\\\\n",
		"\\hline\n",
		"  1101'1110'1001_{BCD} \\\\\n",
		"\\end{array}\n",
	);
	let source = format!("$$\n{body}$$\n");
	let parsed = parse(source.clone());
	validate(&parsed);
	assert_eq!(parsed.blocks().len(), 1);
	assert_eq!(math(&parsed.blocks()[0]), body);
	assert_eq!(
		&source[parsed.block_source(&parsed.blocks()[0]).unwrap()],
		source.trim_end()
	);
}

#[test]
fn block_math_toggles_independently_and_reparses_when_only_the_dialect_changes()
{
	assert!(Dialect::default().math_dollars_block);
	let source = "$$\n1\n=\n2\n$$\n";
	for ofm in [false, true] {
		let mut parsed = parse_with(
			source,
			Dialect {
				ofm,
				..Default::default()
			},
		);
		for math_dollars_block in [false, true, false, true] {
			let dialect = Dialect {
				ofm,
				math_dollars_block,
				..Default::default()
			};
			parsed = reparse_with(&parsed, source.into(), dialect);
			validate(&parsed);
			assert_eq!(parsed.blocks(), parse_with(source, dialect).blocks());
			assert_eq!(parsed.blocks().len() == 1, math_dollars_block);
			if math_dollars_block {
				assert_eq!(math(&parsed.blocks()[0]), "1\n=\n2\n");
			}
		}
	}
}

#[test]
fn math_blocks_keep_markdown_literal_and_work_in_containers() {
	let body =
		"# heading\n\n- list\n1. item\n---\n=\n%%comment%%\n<span>&</span>\n";
	for ofm in [false, true] {
		let parsed = parse_with(
			format!("before\n$$\n{body}$$\nafter\n"),
			Dialect {
				ofm,
				..Default::default()
			},
		);
		validate(&parsed);
		assert_eq!(parsed.blocks().len(), 3);
		assert_eq!(math(&parsed.blocks()[1]), body);
	}
	let quoted = parse("> $$\n> a\n> =\n> b\n> $$\n");
	validate(&quoted);
	let BlockKind::Quote { blocks, .. } = quoted.blocks()[0].kind() else {
		panic!("expected quote")
	};
	assert_eq!(math(&blocks[0]), "a\n=\nb\n");
	let details = parse(
		"<details>\n<summary>Math</summary>\n\n$$\na\n=\nb\n$$\n\n</details>\n",
	);
	validate(&details);
	let BlockKind::Details { blocks, .. } = details.blocks()[0].kind() else {
		panic!("expected details")
	};
	assert_eq!(math(&blocks[0]), "a\n=\nb\n");
}

#[test]
fn math_fence_accepts_trailing_metadata() {
	let parsed = parse("```math title=area\nA = w h\n```");
	validate(&parsed);
	assert_eq!(math(&parsed.blocks()[0]), "A = w h\n");
}

#[test]
fn inline_math_preserves_newlines_and_matches_backtick_runs() {
	for (source, expected) in [("$a\n+b$", "a\n+b"), ("$``a`b``$", "a`b")] {
		let parsed = parse(source);
		validate(&parsed);
		let BlockKind::Paragraph(text) = parsed.blocks()[0].kind() else {
			panic!("expected a paragraph");
		};
		assert_eq!(text.len(), 1);
		assert!(
			matches!(&text[0].kind, InlineKind::Math { latex, display: false } if latex == expected)
		);
	}
}

#[test]
fn other_math_delimiters_still_work_when_block_mode_is_disabled() {
	for math_dollars_block in [false, true] {
		let dialect = Dialect {
			math_dollars_block,
			..Default::default()
		};
		for source in ["$x$", "$$x$$", "\\(x\\)", "\\[x\\]", "```math\nx\n```"]
		{
			let parsed = parse_with(source, dialect);
			validate(&parsed);
			let BlockKind::Paragraph(text) = parsed.blocks()[0].kind() else {
				panic!("expected paragraph")
			};
			assert!(
				matches!(&text[0].kind, InlineKind::Math { latex, .. } if latex.trim() == "x")
			);
		}
	}
}

proptest! {
	#[test]
	fn literal_math_edits_match_full_parses(
		lines in prop::collection::vec(prop::sample::select(vec!["x", "y", "+ a", "- b", "1. c", "---", "=", "", "# 标题", "\\hline"]), 1..15),
		ofm in any::<bool>(), enabled in any::<bool>(),
		newline in prop::sample::select(vec!["\n", "\r\n", "\r"]),
	) {
		let body = format!("{}\n", lines.join("\n"));
		let source = format!("Before.\n\n$$\n{body}$$\n\nAfter.\n").replace('\n', newline);
		let dialect = Dialect { ofm, math_dollars_block: enabled, ..Default::default() };
		let initial = parse_with(source.clone(), dialect);
		let changed = source.replace("Before.", "Changed.");
		let edited = reparse_with(&initial, changed.clone().into(), dialect);
		let full = parse_with(changed, dialect);
		validate(&edited);
		prop_assert_eq!(edited.blocks(), full.blocks());
		if enabled { prop_assert_eq!(math(&edited.blocks()[1]), body); }
	}
}

#[test]
fn edits_across_blank_lines_keep_the_whole_math_block() {
	let source = "Before.\n\n$$\nx\n\ny\n$$\n\nAfter.\n";
	let initial = parse(source);
	for (from, to) in [("x", "z"), ("Before.", "Changed."), ("y", "y\n\nw")] {
		let changed = source.replace(from, to);
		let edited =
			reparse_with(&initial, changed.clone().into(), Dialect::default());
		validate(&edited);
		assert_eq!(edited.blocks(), parse(changed).blocks());
	}
}

#[test]
fn literal_math_definitions_do_not_create_disclosure_links() {
	let source = "<details>\n<summary>More</summary>\n\n[leak]\n\n</details>\n\n$$\nx\n\n[leak]: /unexpected\n\ny\n$$\n";
	let parsed = parse(source);
	validate(&parsed);
	assert!(math(&parsed.blocks()[1]).contains("[leak]: /unexpected"));
	let BlockKind::Details { blocks, .. } = parsed.blocks()[0].kind() else {
		panic!("expected disclosure");
	};
	let BlockKind::Paragraph(text) = blocks[0].kind() else {
		panic!("expected disclosure paragraph");
	};
	assert_eq!(markview_document::plain_text(text), "[leak]");
	assert!(text.iter().all(|inline| inline.link_url().is_none()));
}

#[test]
fn definitions_after_math_resolve_in_disclosures() {
	let source = "<details>\n<summary>More</summary>\n\n[target]\n\n</details>\n\nOutside [target].\n\n$$\nx\n$$\n[target]: /expected\n";
	let parsed = parse(source);
	validate(&parsed);
	assert_eq!(math(&parsed.blocks()[2]), "x\n");
	let BlockKind::Paragraph(outside) = parsed.blocks()[1].kind() else {
		panic!("expected outside paragraph");
	};
	assert!(
		outside
			.iter()
			.any(|inline| inline.link_url() == Some("/expected"))
	);
	let BlockKind::Details { blocks, .. } = parsed.blocks()[0].kind() else {
		panic!("expected disclosure");
	};
	let BlockKind::Paragraph(text) = blocks[0].kind() else {
		panic!("expected disclosure paragraph");
	};
	assert!(
		text.iter()
			.any(|inline| inline.link_url() == Some("/expected"))
	);
}

#[test]
fn prefix_cuts_keep_math_blocks_complete() {
	let source: std::sync::Arc<str> =
		"Before.\n\n$$\nx\n\ny\n$$\n\nAfter.\n".into();
	let full = parse(source.clone());
	assert_eq!(math(&full.blocks()[1]), "x\n\ny\n");
	if let Some(prefix) = markview_commonmark::parse_prefix_with(
		&source,
		source.find('x').unwrap(),
		Dialect::default(),
	) {
		validate(&prefix);
		assert_eq!(prefix.blocks(), &full.blocks()[..prefix.blocks().len()]);
		for (partial, complete) in prefix.blocks().iter().zip(full.blocks()) {
			assert_eq!(
				prefix.block_source(partial),
				full.block_source(complete)
			);
		}
	}
}

#[test]
fn prefixes_after_closed_math_still_use_bounded_parsing() {
	let source: std::sync::Arc<str> =
		"Before.\n\n$$\nx\n\ny\n$$\n\nAfter.\n\nTail.\n".into();
	let full = parse(source.clone());
	let prefix = markview_commonmark::parse_prefix_with(
		&source,
		source.find("After.").unwrap(),
		Dialect::default(),
	)
	.expect("a prefix after the closed formula");
	validate(&prefix);
	assert_eq!(prefix.blocks(), &full.blocks()[..3]);
}

proptest! {
	#[test]
	fn bounded_math_matches_full_blocks_at_every_cut(
		first in "[a-z]{1,6}", second in "[a-z]{1,6}",
		container in 0..4, ofm in any::<bool>(),
		newline in prop::sample::select(vec!["\n", "\r\n", "\r"]),
	) {
		let body = format!("$$\n{first}\n\n{second}\n$$\n");
		let body = match container {
			0 => body,
			1 => body.lines().map(|line| format!("> {line}\n")).collect(),
			2 => format!("- $$\n  {first}\n\n  {second}\n  $$\n"),
			_ => format!("<details>\n<summary>Math</summary>\n\n{body}\n</details>\n"),
		};
		let source: std::sync::Arc<str> = format!("Before.\n\n{body}\nAfter.\n\nTail.\n").replace('\n', newline).into();
		let dialect = Dialect { ofm, ..Default::default() };
		let full = parse_with(source.clone(), dialect);
		for cut in 1..source.len() {
			if let Some(prefix) = markview_commonmark::parse_prefix_with(&source, cut, dialect) {
				validate(&prefix);
				prop_assert!(prefix.blocks().len() <= full.blocks().len());
				prop_assert_eq!(prefix.blocks(), &full.blocks()[..prefix.blocks().len()]);
				for (partial, complete) in prefix.blocks().iter().zip(full.blocks()) {
					prop_assert_eq!(prefix.block_source(partial), full.block_source(complete));
				}
			}
		}
	}
}

#[test]
fn literal_math_definitions_do_not_create_prefix_links() {
	let source: std::sync::Arc<str> =
		"[leak]\n\nMore.\n\n$$\nx\n\n[leak]: /unexpected\n\ny\n$$\n".into();
	let full = parse(source.clone());
	assert!(math(&full.blocks()[2]).contains("[leak]: /unexpected"));
	let BlockKind::Paragraph(text) = full.blocks()[0].kind() else {
		panic!("expected paragraph")
	};
	assert_eq!(markview_document::plain_text(text), "[leak]");
	assert!(text.iter().all(|inline| inline.link_url().is_none()));
	let prefix =
		markview_commonmark::parse_prefix_with(&source, 3, Dialect::default())
			.unwrap_or_else(|| parse(source.clone()));
	validate(&prefix);
	assert_eq!(prefix.blocks()[0], full.blocks()[0]);
}

#[test]
fn definitions_after_math_resolve_in_prefixes() {
	let source: std::sync::Arc<str> =
		"[target]\n\nMore.\n\n$$\nx\n$$\n[target]: /expected\n".into();
	let full = parse(source.clone());
	assert_eq!(math(&full.blocks()[2]), "x\n");
	let BlockKind::Paragraph(text) = full.blocks()[0].kind() else {
		panic!("expected paragraph")
	};
	assert!(
		text.iter()
			.any(|inline| inline.link_url() == Some("/expected"))
	);
	let prefix =
		markview_commonmark::parse_prefix_with(&source, 3, Dialect::default())
			.unwrap_or_else(|| parse(source.clone()));
	validate(&prefix);
	assert_eq!(prefix.blocks()[0], full.blocks()[0]);
}

#[test]
fn literal_math_definitions_do_not_create_svg_snippet_links() {
	let parsed =
		parse("<svg/>\n[leak]\n\n$$\nx\n\n[leak]: /unexpected\n\ny\n$$\n");
	validate(&parsed);
	assert!(
		math(parsed.blocks().last().unwrap()).contains("[leak]: /unexpected")
	);
	let inlines: Vec<_> = parsed
		.blocks()
		.iter()
		.filter_map(|block| {
			if let BlockKind::Paragraph(text) = block.kind() {
				Some(text.iter())
			} else {
				None
			}
		})
		.flatten()
		.collect();
	assert!(
		inlines
			.iter()
			.any(|inline| matches!(inline.kind, InlineKind::Image(_)))
	);
	let references: Vec<_> = inlines.iter().filter(|inline| {
		matches!(&inline.kind, InlineKind::Text(text) if text.contains("leak"))
	}).collect();
	assert!(!references.is_empty());
	assert!(references.iter().all(|inline| inline.link_url().is_none()));
}

#[test]
fn bom_math_edits_keep_the_formula() {
	for newline in ["\n", "\r", "\r\n"] {
		for ofm in [false, true] {
			for closing in ["", "$$\n"] {
				let source = format!("\u{feff}$$\nx\n\ny\n{closing}")
					.replace('\n', newline);
				let dialect = Dialect {
					ofm,
					..Default::default()
				};
				let initial = parse_with(source.as_str(), dialect);
				assert_eq!(initial.blocks().len(), 1);
				assert_eq!(math(&initial.blocks()[0]), "x\n\ny\n");
				let changed = source.replace('y', "z");
				let edited =
					reparse_with(&initial, changed.clone().into(), dialect);
				validate(&edited);
				let full = parse_with(changed, dialect);
				assert_eq!(edited.blocks(), full.blocks());
			}
		}
	}
}

#[test]
fn real_definitions_after_math_resolve_in_svg_snippets() {
	let parsed = parse("<svg/>\n[target]\n\n$$\nx\n$$\n[target]: /expected\n");
	validate(&parsed);
	let BlockKind::Paragraph(text) = parsed.blocks()[1].kind() else {
		panic!("expected SVG trailing text");
	};
	assert!(
		text.iter()
			.any(|inline| inline.link_url() == Some("/expected"))
	);
}

#[test]
fn svg_snippets_keep_footnote_definitions_that_open_with_math() {
	let parsed = parse(
		"Outside[^math].\n\n<svg/>\nNote[^math].\n\n[^math]: $$\n    x\n    $$\n",
	);
	validate(&parsed);
	let BlockKind::Footnote { blocks, .. } =
		parsed.blocks().last().unwrap().kind()
	else {
		panic!("expected a math footnote");
	};
	assert_eq!(math(&blocks[0]), "x\n");
	let BlockKind::Paragraph(text) = parsed.blocks()[2].kind() else {
		panic!("expected SVG trailing text");
	};
	assert!(
		text.iter()
			.any(|inline| matches!(inline.kind, InlineKind::FootnoteRef(_)))
	);
}

#[test]
fn adjacent_grid_formulas_keep_overlapping_source_lines_with_definitions() {
	for grid in [
		"+----+----+\n| $$ | $$ |\n| x  | y  |\n| $$ | $$ |\n+----+----+\n",
		"+----+----+\n| $$ |    |\n| x  | $$ |\n| $$ | y  |\n|    | $$ |\n+----+----+\n",
	] {
		for newline in ["\n", "\r", "\r\n"] {
			let grid = grid.replace('\n', newline);
			let arena = comrak::Arena::new();
			let mut options = comrak::Options::default();
			options.extension.grid_table = true;
			options.extension.math_dollars = true;
			options.extension.math_dollars_block = true;
			let root = comrak::parse_document(&arena, &grid, &options);
			let math_lines: Vec<_> = root
				.descendants()
				.filter_map(|node| {
					let data = node.data.borrow();
					matches!(data.value, comrak::nodes::NodeValue::MathBlock(_))
						.then_some(
							data.sourcepos.start.line..=data.sourcepos.end.line,
						)
				})
				.collect();
			assert_eq!(math_lines.len(), 2);
			assert!(math_lines[0].contains(math_lines[1].start()));
			for tail in ["", "\n[ref]: /url\n"] {
				for ofm in [false, true] {
					let parsed = parse_with(
						format!("{grid}{tail}"),
						Dialect {
							ofm,
							..Default::default()
						},
					);
					validate(&parsed);
					let BlockKind::Table(table) = parsed.blocks()[0].kind()
					else {
						panic!("expected grid table");
					};
					let cells = &table.groups[0].rows[0].cells;
					assert_eq!(math(&cells[0].blocks[0]), "x\n");
					assert_eq!(math(&cells[1].blocks[0]), "y\n");
				}
			}
		}
	}
}
