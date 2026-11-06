use super::*;
use proptest::prelude::*;

fn occurrence(value: u32) -> LinkOccurrenceId {
	LinkOccurrenceId::new(value).unwrap()
}

proptest! {
	#[test]
	fn link_origin_projection_prefers_explicit_links_and_collapses_only_adjacent_occurrences(
		parts in prop::collection::vec((0u8..4, prop::option::of(1u32..8), 1u32..8), 0..32),
	) {
		let mut expected = Vec::new();
		let inlines: Vec<_> = parts.iter().map(|&(kind, link, reference)| {
			let projected = if kind == 0 { None } else { link.or((kind == 1).then_some(reference)) };
			expected.extend(projected.map(occurrence));
			Inline {
				kind: match kind { 0 => InlineKind::Anchor("anchor".to_owned()), 1 => InlineKind::FootnoteRef(FootnoteReference { definition: FootnoteId::new("note"), number: 1, occurrence: occurrence(reference) }), 2 => InlineKind::Text(String::new()), _ => InlineKind::Text("text".to_owned()) },
				link: link.map(|id| Link::new("https://a.test", occurrence(id))), style: TextStyle::default(),
			}
		}).collect();
		expected.dedup();
		prop_assert_eq!(inline_link_origins(&inlines), expected);
	}

	#[test]
	fn semantic_cache_keys_ignore_bijective_occurrence_and_block_id_renaming(
		parts in prop::collection::vec((0usize..4, 0u8..3, ".{0,12}"), 0..20),
	) {
		let build = |after, revision, ids: [u32; 4]| {
			let mut builder = DocumentBuilder::continuing(DocumentRevision::new(revision), BlockId::new(after));
			let inlines = parts.iter().map(|(group, kind, text)| Inline {
				link: (*kind != 1).then(|| Link::new("https://example.test", occurrence(ids[*group]))),
				kind: if *kind == 0 { InlineKind::Text(text.clone()) } else {
					InlineKind::FootnoteRef(FootnoteReference {
						definition: FootnoteId::new(format!("note-{group}")), number: *group as u32 + 1, occurrence: occurrence(ids[*group]),
					})
				},
				style: TextStyle::default(),
			}).collect();
			let child = builder.block(vec!["paragraph".to_owned()], BlockKind::Paragraph(inlines));
			let parent = builder.block(Vec::new(), BlockKind::Quote { callout: None, blocks: vec![child] });
			builder.finish(vec![parent])
		};
		let a = build(0, 1, [1, 2, 3, 4]);
		let b = build(100, 2, [17, 5, 200, 2]);
		prop_assert_ne!(a.blocks()[0].id(), b.blocks()[0].id());
		prop_assert_ne!(a.revision(), b.revision());
		prop_assert_eq!(a.content_key(), b.content_key());
		prop_assert_eq!(a.blocks()[0].content_key(), b.blocks()[0].content_key());
	}

	#[test]
	fn reading_projection_uses_text_alt_formula_and_reference_labels(
		parts in prop::collection::vec((0u8..7, ".{0,20}", any::<u32>()), 0..24),
	) {
		let mut inlines = Vec::new();
		let mut expected = String::new();
		for (kind, text, number) in parts {
			let (kind, projection) = match kind {
				0 => (InlineKind::Text(text.clone()), text),
				1 => (InlineKind::Math { latex: text.clone(), display: true }, text),
				2 => (InlineKind::Image(Box::new(ImageSpec {
					source: image::ImageSource::Resource(ResourceRef::document("not reading text")),
					alt: text.clone(), title: "not reading text".to_owned(), width: None, height: None,
				})), text),
				3 => (InlineKind::FootnoteRef(FootnoteReference {
					definition: FootnoteId::new(text), number, occurrence: occurrence(1),
				}), format!("[{number}]")),
				4 => (InlineKind::LineBreak(LineBreak::Hard), "\n".to_owned()),
				5 => (InlineKind::LineBreak(LineBreak::Forced), "\n".to_owned()),
				_ => (InlineKind::Anchor(text), String::new()),
			};
			expected.push_str(&projection);
			inlines.push(Inline { kind, link: Some(Link::new("not reading text", occurrence(1))), style: TextStyle { bold: true, code: true, ..TextStyle::default() } });
		}
		prop_assert_eq!(plain_text(&inlines), expected.as_str());
		prop_assert_eq!(plain_text_len(&inlines), expected.len());
		let field = FieldText::Rich(&inlines);
		prop_assert_eq!(field.len(), expected.len());
		prop_assert_eq!(field.is_empty(), expected.is_empty());
		prop_assert_eq!(FieldText::Code(&expected).len(), expected.len());
	}

	#[test]
	fn addresses_and_occurrences_are_separate(url in ".{1,64}", ids in prop::collection::vec(1u32..17, 0..32)) {
		let first = Link::new(url.clone(), occurrence(1));
		let independent = Link::new(url.clone(), occurrence(1));
		prop_assert!(!Arc::ptr_eq(first.shared(), independent.shared()));
		prop_assert!(first.same_address(&independent));
		prop_assert!(first.same_occurrence(&independent));
		prop_assert_eq!(first.hash_key(), independent.hash_key());
		let mut current = first.clone();
		for id in ids {
			current = current.with_occurrence(occurrence(id));
			prop_assert_eq!(current.as_str(), url.as_str());
			prop_assert_eq!(current.occurrence(), occurrence(id));
			prop_assert!(Arc::ptr_eq(current.shared(), first.shared()));
			prop_assert_eq!(current.same_occurrence(&first), id == 1);
			prop_assert!(current.same_occurrence(&current.clone()));
			prop_assert!(current.same_address(&independent));
			prop_assert_eq!(current.hash_key(), first.hash_key());
		}
	}
	#[test]
	fn identity_uses_address_and_explicit_occurrence(a in ".{0,32}", b in ".{0,32}", c in ".{0,32}", ids in prop::array::uniform3(1u32..4)) {
		let texts = [a, b, c];
		let links: Vec<_> = texts
			.iter()
			.zip(ids)
			.map(|(text, id)| Link::new(text.clone(), occurrence(id)))
			.collect();
		for i in 0..3 {
			for j in 0..3 {
				prop_assert_eq!(
					links[i].same_address(&links[j]),
					texts[i] == texts[j]
				);
				prop_assert_eq!(
					links[i].same_occurrence(&links[j]),
					texts[i] == texts[j] && ids[i] == ids[j]
				);
				if texts[i] == texts[j] {
					prop_assert_eq!(links[i].hash_key(), links[j].hash_key());
				}
			}
		}
	}
}
