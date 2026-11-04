use markview_document as document;
use markview_document::{Block, BlockKind, InlineKind};
use std::sync::Arc;

#[test]
fn details_after_front_matter_with_lone_carriage_returns() {
	let source = "---\n\r\r\r\u{fffd}\n---\n<details>\n<d\u{fffd}\u{fffd}\u{fffd}\n\r\r</details>";
	let doc = markview_commonmark::parse(source);
	assert_eq!(doc.blocks().len(), 2);
	let block = &doc.blocks()[1];
	assert!(matches!(block.kind(), BlockKind::Details { .. }));
	assert_eq!(
		doc.block_source(block).unwrap(),
		source.find("<details>").unwrap()..source.len()
	);
	assert_eq!(
		&source[doc.block_source(&doc.blocks()[0]).unwrap().clone()],
		"---\n\r\r\r\u{fffd}\n---"
	);
	let reparsed = markview_commonmark::reparse(&doc, Arc::from(source));
	assert_eq!(reparsed.content_key(), doc.content_key());
	let source: Arc<str> = Arc::from(source);
	for cut in 1..source.len() {
		let _ = markview_commonmark::parse_prefix(&source, cut);
	}
}

#[test]
fn details_closing_range_uses_original_html_bytes() {
	for newline in ["\n", "\r", "\r\n"] {
		for marker in ["", "> "] {
			for prefix in ["<div>é", "<div>\0é", "<details>A</details>"] {
				let source = format!(
					"{marker}<details>{newline}{marker}{newline}{marker}{prefix}{newline}{marker}  </details>é{newline}"
				);
				let doc = markview_commonmark::parse(source.clone());
				let blocks = if marker.is_empty() {
					doc.blocks()
				} else {
					let BlockKind::Quote { blocks, .. } =
						doc.blocks()[0].kind()
					else {
						panic!("expected a quote: {source:?}");
					};
					blocks
				};
				assert!(
					matches!(blocks[0].kind(), BlockKind::Details { .. }),
					"{source:?}"
				);
				let end =
					source.rfind("</details>").unwrap() + "</details>".len();
				assert_eq!(
					doc.block_source(&blocks[0]).unwrap(),
					marker.len()..end,
					"{source:?}"
				);
				assert_eq!(
					&source[doc.block_source(&blocks[0]).unwrap().clone()],
					&source[marker.len()..end]
				);
				let BlockKind::Paragraph(tail) = &blocks[1].kind() else {
					panic!("expected trailing text: {source:?}");
				};
				assert_eq!(document::plain_text(tail.as_slice()), "é");
			}
		}
	}
}

#[test]
fn adjacent_details_have_their_own_source_ranges_and_open_states() {
	for newline in ["\n", "\r", "\r\n"] {
		for marker in ["", "> ", "- "] {
			let continuation = if marker == "- " { "  " } else { marker };
			let source = format!(
				"{marker}<details open><summary>One</summary>é\0A</details>{newline}{continuation}<details openn><summary>Two</summary>B</details>{newline}"
			);
			let doc = markview_commonmark::parse(source.clone());
			let blocks = match doc.blocks()[0].kind() {
				BlockKind::Quote { blocks, .. } => blocks,
				BlockKind::List { items, .. } => &items[0].blocks,
				_ => doc.blocks(),
			};
			assert_eq!(blocks.len(), 2, "{source:?}");
			for (i, (block, opener)) in blocks
				.iter()
				.zip(["<details open>", "<details openn>"])
				.enumerate()
			{
				let start = source.find(opener).unwrap();
				let end = start
					+ source[start..].find("</details>").unwrap()
					+ "</details>".len();
				assert_eq!(
					doc.block_source(block).unwrap(),
					start..end,
					"{source:?}"
				);
				assert_eq!(doc.details_declared(block.id()), Some(i == 0));
				let BlockKind::Details { summary, .. } = &block.kind() else {
					panic!("expected details");
				};
				assert!(summary.iter().all(|inline| &source
					[doc.inline_source(block, inline).unwrap()]
					== ["One", "Two"][i]));
			}
			assert_ne!(blocks[0].id(), blocks[1].id());
		}
	}
}

#[test]
fn details_footnotes_appear_once_at_the_end_of_the_document() {
	let sources = [
		(
			"|**bold** __ita__ ~~gone~~ `codeT|\n---\n<details open>\n\n[^1]: a footnote body\nsee [^1] and the text\n\nte\n</details>\n& C",
			vec!["a footnote body"],
		),
		(
			"Outside[^n]\n\n<details>\n\n[^n]: The note.\n\n</details>\n",
			vec!["The note."],
		),
		(
			"Outside[^n]\n\n[^n]: First.\n\n<details>\n\n[^n]: Last.\n\n</details>\n",
			vec!["Last."],
		),
		(
			"<details>\n\nBody[^n]\n\n[^n]: The note.\n\n</details>\n",
			vec!["The note."],
		),
		(
			"First[^a]\n\n<details>\n\nBody[^b]\n\n[^b]: Body note.\n\n</details>\n\n[^a]: Outside note.\n",
			vec!["Outside note.", "Body note."],
		),
		(
			"Outside[^n]\n\n<details>\n\n[^n]:\n    # Note title\n\n</details>\n",
			vec!["Note title"],
		),
	];
	fn references(blocks: &[Block], out: &mut Vec<u32>) {
		for block in blocks {
			match &block.kind() {
				BlockKind::Paragraph(text) => {
					out.extend(text.iter().filter_map(|inline| {
						if let InlineKind::FootnoteRef(reference) = &inline.kind
						{
							Some(reference.number)
						} else {
							None
						}
					}))
				}
				BlockKind::Details { blocks, .. } => {
					assert!(blocks.iter().all(|block| !matches!(
						block.kind(),
						BlockKind::Footnote { .. }
					)));
					references(blocks, out);
				}
				BlockKind::Footnote { blocks, .. } => references(blocks, out),
				_ => {}
			}
		}
	}
	for (source, expected) in sources {
		let doc = markview_commonmark::parse(source);
		let mut refs = Vec::new();
		references(doc.blocks(), &mut refs);
		assert!(!refs.is_empty(), "{source:?}");
		refs.sort_unstable();
		refs.dedup();
		let notes = &doc.blocks()[doc.blocks().len() - refs.len()..];
		assert_eq!(notes.len(), expected.len(), "{source:?}");
		for ((note, number), expected) in notes.iter().zip(refs).zip(expected) {
			let BlockKind::Footnote {
				number: note_number,
				blocks,
				..
			} = note.kind()
			else {
				panic!("expected a note at the document's end: {source:?}");
			};
			assert_eq!(*note_number, number, "{source:?}");
			assert!(
				blocks.iter().any(|block| match &block.kind() {
					BlockKind::Paragraph(text)
					| BlockKind::Heading { text, .. } =>
						document::plain_text(text).contains(expected),
					_ => false,
				}),
				"{source:?}"
			);
			assert!(doc.details_enclosing("fn:1").is_empty());
		}
		assert!(
			&doc.blocks()[..doc.blocks().len() - notes.len()].iter().all(
				|block| !matches!(block.kind(), BlockKind::Footnote { .. })
			)
		);
		let reparsed = markview_commonmark::reparse(&doc, Arc::from(source));
		assert_eq!(doc.content_key(), reparsed.content_key());
		if source.contains("# Note title") {
			assert_eq!(doc.outline()[0].anchor, "note-title");
		}
	}
}

/// Notes hoisted out of disclosures are numbered with the document's own.
///
/// The marker column those numbers need is a layout concern, pinned by
/// `footnote_bodies_share_the_same_left_edge` in `markview-layout`.
#[test]
fn notes_discovered_in_details_share_document_numbering() {
	let mut source =
		String::from("First[^outside]\n\n[^outside]: Outside note.\n\n");
	for i in 0..9 {
		source.push_str(&format!(
			"<details>\n\nBody[^n{i}]\n\n[^n{i}]: Note {i}.\n\n</details>\n\n"
		));
	}
	let doc = markview_commonmark::parse(source);
	let notes = &doc.blocks()[doc.blocks().len() - 10..];
	for (i, note) in notes.iter().enumerate() {
		let BlockKind::Footnote { number, .. } = note.kind() else {
			panic!("expected a note");
		};
		assert_eq!(*number, (i + 1) as u32);
	}
	for (i, block) in doc.blocks()[1..10].iter().enumerate() {
		let BlockKind::Details { blocks, .. } = &block.kind() else {
			panic!("expected details");
		};
		let BlockKind::Paragraph(text) = &blocks[0].kind() else {
			panic!("expected a paragraph");
		};
		assert!(text.iter().any(|inline| matches!(
			&inline.kind,
			InlineKind::FootnoteRef(reference)
				if reference.number == i as u32 + 2
		)));
	}
}

#[test]
fn a_note_can_retain_its_source_label_after_its_reference_is_removed() {
	let doc = markview_commonmark::parse(
		"[^unused]: hi[^x]\n\n[^x]: note[^y]\n\n[^y]: end\n",
	);
	let labels: Vec<_> = doc
		.blocks()
		.iter()
		.map(|block| {
			let BlockKind::Footnote { id, number, .. } = &block.kind() else {
				panic!("expected a note");
			};
			(id.as_str(), *number)
		})
		.collect();
	assert_eq!(labels, [("x", 0), ("y", 2)]);
}

#[test]
fn incomplete_prefixes_defer_hoisted_notes() {
	let source: Arc<str> =
		Arc::from("<details>\nBody[^a]\n[^a]: Note.\n</details>\n\nTail\n");
	let full = markview_commonmark::parse(source.clone());
	let prefix = markview_commonmark::parse_prefix(&source, 10).unwrap();
	assert_eq!(prefix.blocks(), &full.blocks()[..1]);
	for cut in 1..source.find("Tail").unwrap() {
		if let Some(prefix) = markview_commonmark::parse_prefix(&source, cut) {
			assert_eq!(
				prefix.blocks(),
				&full.blocks()[..prefix.blocks().len()],
				"cut={cut}"
			);
		}
	}
	let complete =
		markview_commonmark::parse_prefix(&source, source.len() - 1).unwrap();
	assert_eq!(complete.blocks(), full.blocks());
}

#[test]
fn hoisted_note_headings_follow_final_reading_order() {
	for quote in ["", "> "] {
		let source = format!(
			"<details>\n\nBody[^a]\n\n[^a]:\n    {quote}# Topic\n\n</details>\n\n{quote}# Topic\n"
		);
		let doc = markview_commonmark::parse(source);
		let anchors: Vec<_> = doc
			.outline()
			.into_iter()
			.map(|heading| heading.anchor)
			.collect();
		assert_eq!(anchors, ["topic", "topic-1"]);
		assert!(doc.details_enclosing("topic").is_empty());
		let expected = markview_commonmark::parse(format!(
			"Body[^a]\n\n{quote}# Topic\n\n[^a]:\n    {quote}# Topic\n"
		));
		assert_eq!(
			doc.blocks().last().unwrap().content_key(),
			expected.blocks().last().unwrap().content_key()
		);
	}
}

#[test]
fn quoted_multiline_details_tags_keep_original_offsets() {
	for newline in ["\n", "\r", "\r\n"] {
		for marker in ["> ", "> > "] {
			for body in ["Body", "é\0Body"] {
				for blank in ["", "\n"] {
					let separator = if blank.is_empty() {
						String::new()
					} else {
						format!("{marker}{newline}")
					};
					let source = format!(
						"{marker}<details>{newline}{separator}{marker}{body}{newline}{separator}{marker}</details{newline}{marker}>{newline}"
					);
					let doc = markview_commonmark::parse(source.clone());
					let mut block = &doc.blocks()[0];
					while let BlockKind::Quote { blocks, .. } = &block.kind() {
						block = &blocks[0];
					}
					assert!(
						matches!(block.kind(), BlockKind::Details { .. }),
						"{source:?}"
					);
					assert_eq!(
						doc.block_source(block).unwrap(),
						marker.len()..source.rfind('>').unwrap() + 1,
						"{source:?}"
					);
				}
			}
		}
	}
}

#[test]
fn quoted_details_in_wide_list_items_are_not_duplicated_or_lost() {
	for newline in ["\n", "\r", "\r\n"] {
		for (prefix, continuation) in
			[("100. > ", "     > "), ("> 100. > ", ">      > ")]
		{
			let source = format!(
				"{prefix}<details>A</details>{newline}{continuation}<details>B</details>{newline}"
			);
			let doc = markview_commonmark::parse(source.clone());
			let mut blocks = doc.blocks();
			loop {
				match &blocks[0].kind() {
					BlockKind::List { items, .. } => blocks = &items[0].blocks,
					BlockKind::Quote {
						blocks: children, ..
					} => blocks = children,
					_ => break,
				}
			}
			assert_eq!(blocks.len(), 2, "{source:?}");
			for (block, text) in blocks.iter().zip(["A", "B"]) {
				let BlockKind::Details { blocks: body, .. } = &block.kind()
				else {
					panic!("expected details: {source:?}");
				};
				let BlockKind::Paragraph(content) = &body[0].kind() else {
					panic!("expected body text");
				};
				assert_eq!(document::plain_text(content), text);
				assert_eq!(
					&source[doc.block_source(block).unwrap().clone()],
					format!("<details>{text}</details>")
				);
			}
		}
	}
}

#[test]
fn a_note_defined_in_one_disclosure_can_be_referenced_from_another() {
	for reversed in [false, true] {
		let definition = "<details>\n\n[^n]: Note.\n\n</details>\n\n";
		let reference = "<details>\n\nBody[^n]\n\n</details>\n\n";
		let source = if reversed {
			format!("{reference}{definition}")
		} else {
			format!("{definition}{reference}")
		};
		let source: Arc<str> = Arc::from(source);
		let doc = markview_commonmark::parse(source.clone());
		assert_eq!(doc.blocks().len(), 3);
		let BlockKind::Footnote { number, blocks, .. } = doc.blocks()[2].kind()
		else {
			panic!("expected a final note");
		};
		assert_eq!(*number, 1);
		let BlockKind::Paragraph(text) = &blocks[0].kind() else {
			panic!("expected note text");
		};
		assert_eq!(document::plain_text(text), "Note.");
		for cut in 1..source.len() {
			if let Some(prefix) =
				markview_commonmark::parse_prefix(&source, cut)
			{
				assert_eq!(
					prefix.blocks(),
					&doc.blocks()[..prefix.blocks().len()]
				);
			}
		}
	}
	let unused = markview_commonmark::parse(
		"<details>\n\n[^n]: Unused.\n\n</details>\n",
	);
	assert_eq!(unused.blocks().len(), 1);
}

#[test]
fn a_prefix_defers_when_later_references_can_renumber_disclosure_notes() {
	for opener in ["<details>", "<DETAILS>"] {
		let source: Arc<str> = Arc::from(format!(
			"{opener}\nBody[^a]\n[^a]: A.\n</details>\n\nLater[^b].\n\n[^b]: B.\n"
		));
		let full = markview_commonmark::parse(source.clone());
		for cut in 1..source.len() {
			if let Some(prefix) =
				markview_commonmark::parse_prefix(&source, cut)
			{
				assert_eq!(
					prefix.blocks(),
					&full.blocks()[..prefix.blocks().len()]
				);
			}
		}
	}
}

/// Collects every footnote reference number in reading order.
fn footnote_refs(blocks: &[Block], out: &mut Vec<u32>) {
	for block in blocks {
		match &block.kind() {
			BlockKind::Paragraph(text) => {
				out.extend(text.iter().filter_map(|inline| {
					if let InlineKind::FootnoteRef(reference) = &inline.kind {
						Some(reference.number)
					} else {
						None
					}
				}))
			}
			BlockKind::Details { blocks, .. }
			| BlockKind::Footnote { blocks, .. }
			| BlockKind::Quote { blocks, .. } => footnote_refs(blocks, out),
			_ => {}
		}
	}
}

#[test]
fn case_folded_footnote_labels_share_one_number() {
	// Comrak folds label case and keeps the later duplicate definition, so
	// `[^n]` and `[^N]` are one note and the one inside the disclosure wins.
	let sources = [
		(
			"Outside[^n]\n\n[^N]: First.\n\n<details>\n\nBody[^n]\n\n[^n]: Second.\n\n</details>\n",
			"Second.",
		),
		("Outside[^n]\n\n[^N]: First.\n", "First."),
		(
			"Outside[^n]\n\n[^n]: First.\n\n<details>\n\nBody[^n]\n\n</details>\n",
			"First.",
		),
	];
	for (source, expected) in sources {
		let doc = markview_commonmark::parse(source);
		let notes: Vec<_> = doc
			.blocks()
			.iter()
			.filter(|block| matches!(block.kind(), BlockKind::Footnote { .. }))
			.collect();
		assert_eq!(notes.len(), 1, "{source:?}");
		let BlockKind::Footnote { number, blocks, .. } = &notes[0].kind()
		else {
			panic!("expected a note: {source:?}");
		};
		assert_eq!(*number, 1, "{source:?}");
		let BlockKind::Paragraph(text) = &blocks[0].kind() else {
			panic!("expected note text: {source:?}");
		};
		assert_eq!(document::plain_text(text), expected, "{source:?}");
		let mut refs = Vec::new();
		footnote_refs(doc.blocks(), &mut refs);
		refs.sort_unstable();
		refs.dedup();
		assert_eq!(refs, [1], "{source:?}");
		let reparsed = markview_commonmark::reparse(&doc, Arc::from(source));
		assert_eq!(reparsed.content_key(), doc.content_key(), "{source:?}");
	}
}

#[test]
fn prefixes_ignore_details_inside_code() {
	for (source, cut) in [
		("Use `<details>` to expand.\n\nRest\n", "expand"),
		("Use `<details>` to expand.\n", "expand"),
		("```\n<details>\n```\n\nRest\n", "<details>"),
	] {
		let source: Arc<str> = Arc::from(source);
		let full = markview_commonmark::parse(source.clone());
		let cut = source.find(cut).unwrap();
		let prefix = markview_commonmark::parse_prefix(&source, cut)
			.expect("code is not raw HTML");
		assert_eq!(prefix.blocks(), &full.blocks()[..prefix.blocks().len()]);
		for cut in 1..source.len() {
			if !source.is_char_boundary(cut) {
				continue;
			}
			if let Some(prefix) =
				markview_commonmark::parse_prefix(&source, cut)
			{
				assert_eq!(
					prefix.blocks(),
					&full.blocks()[..prefix.blocks().len()],
					"{source:?} cut={cut}"
				);
			}
		}
	}
}

#[test]
fn a_prefix_inside_open_details_still_defers() {
	let source: Arc<str> = Arc::from("<details>\n\nBody\n\nTail\n");
	let cut = source.find("Body").unwrap() + "Body".len();
	assert!(markview_commonmark::parse_prefix(&source, cut).is_none());

	let closed: Arc<str> =
		Arc::from("<details>\n\nBody\n\n</details>\n\nTail\n");
	let cut = closed.find("</details>").unwrap() + "</details>".len();
	assert!(markview_commonmark::parse_prefix(&closed, cut).is_some());

	// A closed fence does not mask a real opener after it.
	let fenced: Arc<str> =
		Arc::from("```\n<details>\n```\n\n<details>\n\nBody\n");
	let cut = fenced.rfind("Body").unwrap() + "Body".len();
	assert!(markview_commonmark::parse_prefix(&fenced, cut).is_none());
}
