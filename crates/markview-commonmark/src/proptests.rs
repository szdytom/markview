mod raw_html;

use super::*;
use markview_document::{
	BlockSource, LinkOccurrenceId, content_identity, visit_blocks,
};
use proptest::{prelude::*, test_runner::TestCaseResult};
use std::sync::Arc;

fn word() -> impl Strategy<Value = String> {
	prop::collection::vec(
		prop::sample::select(vec!["a", "b", "你", "é", "😀"]),
		1..6,
	)
	.prop_map(|parts| parts.concat())
}

fn dialect() -> impl Strategy<Value = Dialect> {
	(any::<bool>(), any::<bool>(), any::<bool>()).prop_map(
		|(hard_breaks, highlight_marks, ofm)| Dialect {
			hard_breaks,
			highlight_marks,
			ofm,
		},
	)
}

fn origins(
	block: &Block,
) -> Vec<(Option<LinkOccurrenceId>, Option<LinkOccurrenceId>)> {
	let mut origins = Vec::new();
	visit_fields(block, &mut Vec::new(), &|| false, &mut |_, _, value, _| {
		if let FieldText::Rich(text) = value {
			for inline in text {
				let note = match &inline.kind {
					InlineKind::FootnoteRef(reference) => {
						Some(reference.occurrence)
					}
					_ => None,
				};
				origins.push((
					inline.link.as_ref().map(|link| link.occurrence()),
					note,
				));
			}
		}
	});
	origins
}

fn mapping(
	entry: Option<&BlockSource>,
) -> Option<(
	&Option<std::ops::Range<usize>>,
	&[markview_document::FieldSource],
)> {
	entry.map(|entry| (&entry.source, entry.fields.as_slice()))
}

fn same_prefix(actual: &MarkdownParse, full: &MarkdownParse) -> TestCaseResult {
	prop_assert!(actual.blocks().len() <= full.blocks().len());
	let expected = &full.blocks()[..actual.blocks().len()];
	prop_assert_eq!(actual.blocks(), expected);
	prop_assert_eq!(actual.content_key(), content_identity(expected));
	prop_assert_eq!(actual.map.revision(), Some(actual.revision()));
	prop_assert_eq!(
		actual.map.validate(&actual.source, &actual.document),
		Ok(())
	);
	for (a, b) in actual.blocks().iter().zip(expected) {
		prop_assert_eq!(origins(a), origins(b));
		let mut left = Vec::new();
		let mut right = Vec::new();
		visit_blocks(a, &mut |block| left.push(block.id()));
		visit_blocks(b, &mut |block| right.push(block.id()));
		prop_assert_eq!(left.len(), right.len());
		for (a, b) in left.into_iter().zip(right) {
			prop_assert_eq!(
				mapping(actual.map.block(a)),
				mapping(full.map.block(b))
			);
		}
	}
	Ok(())
}

fn render(sections: &[(bool, String)], newline: &str) -> String {
	sections
		.iter()
		.map(|(heading, text)| {
			if *heading {
				format!("## {text}{newline}{newline}")
			} else {
				format!(
					"{text} [link](https://example.test) **bold**{newline}{newline}"
				)
			}
		})
		.collect()
}

proptest! {
	#[test]
	fn footnote_records_use_first_reference_numbers_and_last_case_folded_definitions(
		definitions in prop::collection::vec((0usize..4, any::<bool>()), 0..10),
		references in prop::collection::vec((0usize..6, any::<bool>()), 1..12),
		newline in prop::sample::select(vec!["\n", "\r\n", "\r"]),
	) {
		let label = |id, upper| format!("{}{id}", if upper { "N" } else { "n" });
		let mut bodies = [const { None }; 4];
		for (index, &(id, _)) in definitions.iter().enumerate() { bodies[id] = Some(format!("definition{index}")); }
		let mut order = Vec::new();
		let mut expected_text = Vec::new();
		let mut expected_refs = Vec::new();
		let mut source = Vec::new();
		for (index, &(id, upper)) in references.iter().enumerate() {
			source.push(format!("r{index}[^{label}]", label = label(id, upper)));
			if id < bodies.len() && bodies[id].is_some() {
				if !order.contains(&id) { order.push(id); }
				let number = order.iter().position(|&entry| entry == id).unwrap() as u32 + 1;
				expected_text.push(format!("r{index}[{number}]"));
				expected_refs.push((FootnoteId::new(label(id, false)), number));
			} else { expected_text.push(format!("r{index}[^{label}]", label = label(id, upper))); }
		}
		let mut source = format!("{}{newline}{newline}", source.join(" "));
		for (index, &(id, upper)) in definitions.iter().enumerate() {
			source.push_str(&format!("[^{label}]: definition{index}{newline}{newline}", label = label(id, upper)));
		}
		let parsed = parse(source.clone());
		prop_assert_eq!(parsed.map.validate(&source, &parsed.document), Ok(()));
		let BlockKind::Paragraph(text) = parsed.blocks()[0].kind() else { panic!("reference paragraph missing") };
		prop_assert_eq!(plain_text(text), expected_text.join(" "));
		let actual_refs: Vec<_> = text.iter().filter_map(|inline| match &inline.kind { InlineKind::FootnoteRef(reference) => Some((reference.definition.clone(), reference.number)), _ => None }).collect();
		prop_assert_eq!(actual_refs, expected_refs.clone());
		let occurrences: std::collections::HashSet<_> = text.iter().filter_map(|inline| match &inline.kind { InlineKind::FootnoteRef(reference) => Some(reference.occurrence), _ => None }).collect();
		prop_assert_eq!(occurrences.len(), expected_refs.len());
		let notes: Vec<_> = parsed.blocks().iter().filter_map(|block| match block.kind() {
			BlockKind::Footnote { id, number, blocks } => {
				let BlockKind::Paragraph(text) = blocks[0].kind() else { panic!("note body missing") };
				Some((id.clone(), *number, plain_text(text)))
			}, _ => None,
		}).collect();
		let expected: Vec<_> = order.into_iter().enumerate().map(|(index, id)| (FootnoteId::new(label(id, false)), index as u32 + 1, bodies[id].clone().unwrap())).collect();
		prop_assert_eq!(notes, expected);
	}

	#[test]
	fn wiki_links_preserve_unicode_labels_escape_literal_percents_and_normalize_heading_targets(
		path in word(), label in word(), suffix in "[0-9A-F]{2}", heading in 0u16..1000,
		newline in prop::sample::select(vec!["\n", "\r\n", "\r"]),
	) {
		let expected_url = format!("Notes/{path} %25{suffix}.md#section-{heading}");
		let path = format!("Notes/{path} %{suffix}.md");
		let source = format!("[[{path}#Section {heading}|{label}]] `[[{path}]]`{newline}");
		let parsed = parse(source.clone());
		prop_assert_eq!(parsed.map.validate(&source, &parsed.document), Ok(()));
		let BlockKind::Paragraph(text) = parsed.blocks()[0].kind() else { return Err(TestCaseError::fail("wiki link paragraph was lost")); };
		prop_assert_eq!(plain_text(text), format!("{label} [[{path}]]"));
		let links: Vec<_> = text.iter().filter_map(Inline::link_url).collect();
		prop_assert_eq!(links, vec![expected_url]);
		prop_assert!(text.last().unwrap().style.code);
		prop_assert!(text.last().unwrap().link.is_none());
	}

	#[test]
	fn callout_aliases_and_folding_keep_titles_bodies_styles_and_declared_state(
		alias in prop::sample::select(vec![("note", CalloutKind::Note), ("summary", CalloutKind::Abstract), ("TLDR", CalloutKind::Abstract), ("info", CalloutKind::Info), ("todo", CalloutKind::Todo), ("hint", CalloutKind::Tip), ("IMPORTANT", CalloutKind::Tip), ("check", CalloutKind::Success), ("done", CalloutKind::Success), ("help", CalloutKind::Question), ("Faq", CalloutKind::Question), ("attention", CalloutKind::Warning), ("missing", CalloutKind::Failure), ("error", CalloutKind::Danger), ("bug", CalloutKind::Bug), ("example", CalloutKind::Example), ("cite", CalloutKind::Quote), ("custom-name", CalloutKind::Note)]),
		fold in 0usize..3, title in word(), body in word(), changed in word(),
		newline in prop::sample::select(vec!["\n", "\r\n", "\r"]),
	) {
		let dialect = Dialect { ofm: true, highlight_marks: true, ..Default::default() };
		let render = |body: &str| format!("> [!{}]{} {title}{newline}> **{body}** =={body}=={newline}", alias.0, ["", "+", "-"][fold]);
		let first = parse_with(render(&body), dialect);
		for parsed in [first.clone(), reparse_with(&first, render(&changed).into(), dialect)] {
			let source_body = if parsed.source == first.source { &body } else { &changed };
			prop_assert_eq!(parsed.map.validate(&parsed.source, &parsed.document), Ok(()));
			let BlockKind::Quote { callout: Some(callout), blocks } = parsed.blocks()[0].kind() else { return Err(TestCaseError::fail("callout was lost")); };
			prop_assert_eq!(callout.kind, alias.1);
			prop_assert_eq!(callout.fold, [None, Some(true), Some(false)][fold]);
			let (summary, blocks) = if fold == 0 { (callout.title.as_ref().unwrap(), blocks) } else {
				prop_assert!(callout.title.is_none());
				let BlockKind::Details { summary, blocks: body, default_open } = blocks[0].kind() else { return Err(TestCaseError::fail("foldable callout lost its disclosure")); };
				prop_assert_eq!(*default_open, fold == 1);
				prop_assert_eq!(parsed.details_declared(blocks[0].id()), Some(fold == 1));
				(summary, body)
			};
			prop_assert_eq!(plain_text(summary), title.clone());
			let BlockKind::Paragraph(text) = blocks[0].kind() else { return Err(TestCaseError::fail("callout body was lost")); };
			prop_assert_eq!(plain_text(text), format!("{source_body} {source_body}"));
			prop_assert!(text.iter().any(|inline| inline.style.bold && plain_text(std::slice::from_ref(inline)) == *source_body));
			prop_assert!(text.iter().any(|inline| inline.style.mark && plain_text(std::slice::from_ref(inline)) == *source_body));
		}
	}

	#[test]
	fn leaf_edits_take_the_fast_path_and_preserve_unedited_block_ids(
		mut sections in prop::collection::vec((any::<bool>(), word()), 3..7),
		edits in prop::collection::vec((any::<usize>(), any::<bool>(), word()), 1..9),
		dialect in dialect(), newline in prop::sample::select(vec!["\n", "\r\n", "\r"]),
	) {
		let mut previous = parse_with(render(&sections, newline), dialect);
		for (index, heading, text) in edits {
			let index = index % sections.len();
			sections[index] = (heading, text);
			let source: Arc<str> = render(&sections, newline).into();
			let next = parse_incremental(&previous, source.clone(), dialect).expect("leaf edit must use the fast path");
			let full = parse_with(source.clone(), dialect);
			same_prefix(&next, &full)?;
			prop_assert_eq!(next.blocks().len(), full.blocks().len());
			prop_assert_eq!(&next.source, &source);
			prop_assert_ne!(next.revision(), previous.revision());
			if previous.source != source {
				for i in 0..sections.len() {
					if i != index { prop_assert_eq!(next.blocks()[i].id(), previous.blocks()[i].id()); }
				}
			}
			previous = next;
		}
	}

	#[test]
	fn arbitrary_short_edit_sequences_reparse_like_a_full_parse(
		initial in prop::collection::vec(0usize..16, 0..12),
		edits in prop::collection::vec((any::<usize>(), any::<usize>(), 0usize..16), 1..10),
		dialect in dialect(),
	) {
		let tokens = ["a", "你", "é", "😀", "\n", "\n\n", "# ", "- ", "> ", "**b**", "`x`", "$x$", "[x](https://example.test)", "<details>", "</details>", "[^n]"];
		let mut source: String = initial.iter().map(|&i| tokens[i]).collect();
		let mut previous = parse_with(source.clone(), dialect);
		for (a, b, token) in edits {
			let boundaries: Vec<_> = source.char_indices().map(|(i, _)| i).chain([source.len()]).collect();
			let a = boundaries[a % boundaries.len()];
			let b = boundaries[b % boundaries.len()];
			source.replace_range(a.min(b)..a.max(b), tokens[token]);
			let next = reparse_with(&previous, source.clone().into(), dialect);
			let full = parse_with(source.clone(), dialect);
			same_prefix(&next, &full)?;
			prop_assert_eq!(next.blocks().len(), full.blocks().len());
			prop_assert_eq!(next.source.as_ref(), source.as_str());
			previous = next;
		}
	}

	#[test]
	fn prefix_cuts_inside_unicode_leaf_blocks_match_the_full_document(
		sections in prop::collection::vec((any::<bool>(), word()), 2..7),
		dialect in dialect(), newline in prop::sample::select(vec!["\n", "\r\n", "\r"]),
	) {
		let source: Arc<str> = render(&sections, newline).into();
		let full = parse_with(source.clone(), dialect);
		prop_assert!(parse_prefix_with(&source, 0, dialect).is_none());
		prop_assert!(parse_prefix_with(&source, source.len(), dialect).is_none());
		prop_assert!(parse_prefix_with(&source, source.len() + 1, dialect).is_none());
		let mut start = 0;
		for (i, section) in sections.iter().enumerate() {
			let block = render(std::slice::from_ref(section), newline);
			let content_end = start + block.len() - 2 * newline.len();
			for cut in start + 1..=content_end {
				let prefix = parse_prefix_with(&source, cut, dialect).expect("a leaf prefix is available");
				prop_assert_eq!(prefix.blocks().len(), i + 1);
				prop_assert_eq!(&prefix.source, &source);
				same_prefix(&prefix, &full)?;
			}
			start += block.len();
		}
	}

	#[test]
	fn front_matter_keeps_verbatim_payload_without_interpreting_yaml(
		values in prop::collection::vec(word(), 0..8), suffix in any::<bool>(), dialect in dialect(),
	) {
		let mut lines = vec!["key: [not necessarily valid YAML".to_owned()];
		lines.extend(values.into_iter().map(|value| format!("  - {value}")));
		let payload = lines.join("\n");
		let source = format!("---\n{payload}\n---{}", if suffix { "\n\nBody\n" } else { "" });
		let parsed = parse_with(source.clone(), dialect);
		prop_assert_eq!(parsed.map.validate(&source, &parsed.document), Ok(()));
		let BlockKind::FrontMatter { default_open, blocks } = parsed.blocks()[0].kind() else {
			return Err(TestCaseError::fail("front matter was not retained"));
		};
		prop_assert!(!default_open);
		prop_assert_eq!(blocks.len(), 1);
		prop_assert_eq!(blocks[0].kind(), &BlockKind::Code { language: "yaml".to_owned(), text: payload });
		prop_assert_eq!(parsed.blocks().len(), 1 + usize::from(suffix));
	}

	#[test]
	fn prefix_resolves_link_and_note_definitions_after_the_cut(
		label in "[a-z]{1,8}", destination in "[a-z]{1,8}", indent in 0usize..4,
		dialect in dialect(), newline in prop::sample::select(vec!["\n", "\r\n", "\r"]),
	) {
		let source: Arc<str> = format!(
			"See [{label}][{label}] and [^{label}].{newline}{newline}Tail.{newline}{newline}{}[{label}]: https://example.test/{destination}{newline}{newline}[^{label}]: Note.{newline}", " ".repeat(indent)
		).into();
		let full = parse_with(source.clone(), dialect);
		let prefix = parse_prefix_with(&source, 1, dialect).expect("reference prefix is available");
		prop_assert_eq!(prefix.blocks().len(), 1);
		same_prefix(&prefix, &full)?;
	}

	#[test]
	fn front_matter_preserves_delimiter_like_lines_before_a_terminated_closer(value in word(), body in any::<bool>()) {
		let payload = format!("key: {value}\n---extra");
		let source = format!("---\n{payload}\n---\n{}", if body { "\nBody\n" } else { "" });
		let parsed = parse(source);
		let BlockKind::FrontMatter { blocks, .. } = parsed.blocks()[0].kind() else {
			return Err(TestCaseError::fail("terminated front matter was not retained"));
		};
		prop_assert_eq!(blocks[0].kind(), &BlockKind::Code { language: "yaml".to_owned(), text: payload });
	}

	#[test]
	fn front_matter_with_a_delimiter_like_line_can_close_at_eof(value in word()) {
		let payload = format!("key: {value}\n---extra");
		let source = format!("---\n{payload}\n---");
		let parsed = parse(source.clone());
		prop_assert_eq!(parsed.map.validate(&source, &parsed.document), Ok(()));
		let BlockKind::FrontMatter { blocks, .. } = parsed.blocks()[0].kind() else {
			return Err(TestCaseError::fail("EOF front matter was lost"));
		};
		prop_assert_eq!(blocks[0].kind(), &BlockKind::Code { language: "yaml".to_owned(), text: payload });
	}
}
