use super::*;
use crate::{
	document::{
		AnchorId, BlockId, DocumentBuilder, DocumentRevision, FootnoteId,
		FootnoteReference, Inline, InlineKind, Link, LinkOccurrenceId,
		RichText, TextStyle,
	},
	scene::{HeadingAnchor, LinkRect, LinkTarget, Paint, Rect},
};
use proptest::prelude::*;

fn id(value: u32) -> LinkOccurrenceId {
	LinkOccurrenceId::new(value).unwrap()
}

fn rich(ids: &[LinkOccurrenceId], specs: &[(bool, usize, u8)]) -> RichText {
	ids.iter()
		.zip(specs)
		.enumerate()
		.flat_map(|(i, (id, &(note, repeats, _)))| {
			let inline = Inline {
				link: (!note).then(|| {
					Link::new(format!("https://example.test/{i}"), *id)
				}),
				kind: if note {
					InlineKind::FootnoteRef(FootnoteReference {
						definition: FootnoteId::new(format!("n{i}")),
						number: i as u32 + 1,
						occurrence: *id,
					})
				} else {
					InlineKind::Text("word ".to_owned())
				},
				style: TextStyle::default(),
			};
			std::iter::repeat_n(inline, if note { 1 } else { repeats })
		})
		.collect()
}

fn paragraph(ids: &[LinkOccurrenceId], specs: &[(bool, usize, u8)]) -> Block {
	let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
	let block =
		builder.block(Vec::new(), BlockKind::Paragraph(rich(ids, specs)));
	builder.finish(vec![block]).into_blocks().pop().unwrap()
}

proptest! {
	#[test]
	fn remapping_rebinds_logical_occurrences_without_mutating_shared_geometry(
		specs in prop::collection::vec((any::<bool>(), 1usize..4, 0u8..4), 0..16),
		base in 1u32..17, rotation in any::<usize>(),
	) {
		let old: Vec<_> = (1..=specs.len() as u32).map(id).collect();
		let current: Vec<_> = (0..specs.len()).map(|i| id(base + ((i + rotation % specs.len()) % specs.len()) as u32)).collect();
		let block = paragraph(&current, &specs);
		prop_assert_eq!(paragraph(&old, &specs).content_key(), block.content_key());
		let mut layout = BlockLayout { link_occurrences: old.clone(), height: 100.0, width: 200.0, degraded: 3, math_errors: 4, ..Default::default() };
		for (i, &(note, _, fragments)) in specs.iter().enumerate() {
			for fragment in 0..fragments {
				let command = layout.draws.len();
				let rect = Rect { x: 2.0, y: (i * 4 + usize::from(fragment)) as f32, w: 10.0, h: 1.0 };
				layout.draws.push(crate::scene::Draw::Rect(rect, Paint::Text));
				let target = if note { LinkTarget::FootnoteDefinition(FootnoteId::new(format!("n{i}"))) } else { LinkTarget::Url(format!("https://example.test/{i}").into()) };
				layout.links.push(LinkRect { commands: command..command + 1, id: LinkId::Inline(old[i]), rect, url: target.display_url(), target });
			}
			if note {
				layout.anchors.push(HeadingAnchor { anchor: AnchorId::FootnoteReference { definition: FootnoteId::new(format!("n{i}")), occurrence: old[i] }, y: i as f32 });
			}
		}
		let special = LinkTarget::Disclosure(BlockId::new(99).unwrap());
		layout.links.push(LinkRect { commands: 0..0, id: LinkId::Special(7), rect: Rect::default(), url: special.display_url(), target: special });
		layout.anchors.push(HeadingAnchor { anchor: AnchorId::Fragment("stable".to_owned()), y: 42.0 });
		let shared = Arc::new(layout);
		let remapped = remap_link_ids(shared.clone(), &block, &crate::limits::Limits::default());
		prop_assert_eq!(&remapped.link_occurrences, &current);
		prop_assert_eq!(&shared.link_occurrences, &old);
		prop_assert_eq!((remapped.height, remapped.width, remapped.degraded, remapped.math_errors), (100.0, 200.0, 3, 4));
		prop_assert_eq!(remapped.links.len(), shared.links.len());
		prop_assert_eq!(remapped.draws.len(), shared.draws.len());
		for (actual, original) in remapped.draws.iter().zip(&shared.draws) {
			let (crate::scene::Draw::Rect(a, paint_a), crate::scene::Draw::Rect(b, paint_b)) = (actual, original) else { unreachable!() };
			prop_assert_eq!((a, paint_a), (b, paint_b));
		}
		for (actual, original) in remapped.links.iter().zip(&shared.links) {
			let expected = match original.id { LinkId::Inline(old) => LinkId::Inline(current[old.get() as usize - 1]), special => special };
			prop_assert_eq!(actual.id, expected);
			prop_assert_eq!((&actual.url, &actual.target, &actual.commands, actual.rect), (&original.url, &original.target, &original.commands, original.rect));
		}
		for (actual, original) in remapped.anchors.iter().zip(&shared.anchors) {
			let expected = match &original.anchor {
				AnchorId::FootnoteReference { definition, occurrence } => AnchorId::FootnoteReference { definition: definition.clone(), occurrence: current[occurrence.get() as usize - 1] },
				other => other.clone(),
			};
			prop_assert_eq!(&actual.anchor, &expected);
			prop_assert_eq!(actual.y, original.y);
		}
		let again = remap_link_ids(remapped.clone(), &block, &crate::limits::Limits::default());
		prop_assert!(Arc::ptr_eq(&again, &remapped));
		prop_assert_eq!(Arc::ptr_eq(&shared, &remapped), old == current);
	}

	#[test]
	fn cached_paragraphs_keep_current_link_and_reference_ids(
		specs in prop::collection::vec((any::<bool>(), 1usize..3, 0u8..4), 1..7),
		bases in prop::collection::vec(1u32..100, 1..5),
	) {
		let mut engine = crate::layout::LayoutEngine::new();
		let options = LayoutOptions::default();
		for (step, base) in bases.into_iter().enumerate() {
			let ids: Vec<_> = (0..specs.len()).map(|i| id(base + i as u32)).collect();
			let document = DocumentBuilder::new(DocumentRevision::new(step as u64 + 1)).finish(vec![paragraph(&ids, &specs)]);
			let snapshot = engine.layout(&document, &options);
			prop_assert_eq!(snapshot.reused, usize::from(step > 0));
			let geometry = &snapshot.blocks[0].layout;
			prop_assert_eq!(&geometry.link_occurrences, &ids);
			for link in &geometry.links {
				let index = match &link.target {
					LinkTarget::Url(url) => url.rsplit('/').next().unwrap().parse::<usize>().unwrap(),
					LinkTarget::FootnoteDefinition(note) => note.as_str()[1..].parse::<usize>().unwrap(),
					_ => unreachable!(),
				};
				prop_assert_eq!(link.id, LinkId::Inline(ids[index]));
			}
		}
	}

	#[test]
	fn a_cached_figure_keeps_its_body_and_caption_occurrences(base in 1u32..17) {
		let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
		let body = builder.block(Vec::new(), BlockKind::Paragraph(rich(&[id(base)], &[(false, 1, 1)])));
		let mut caption = rich(&[id(base + 100)], &[(false, 1, 1)]);
		caption[0].link = Some(Link::new("https://example.test/caption", id(base + 100)));
		let figure = builder.block(Vec::new(), BlockKind::Figure { blocks: vec![body], caption: Some(caption) });
		let document = builder.finish(vec![figure]);
		let mut engine = crate::layout::LayoutEngine::new();
		let options = LayoutOptions::default();
		let initial = engine.layout(&document, &options);
		let cached = engine.layout(&document, &options);
		prop_assert_eq!(cached.reused, 1);
		let before: Vec<_> = initial.blocks[0].layout.links.iter().map(|link| (link.url.clone(), link.id)).collect();
		let after: Vec<_> = cached.blocks[0].layout.links.iter().map(|link| (link.url.clone(), link.id)).collect();
		prop_assert_eq!(after, before);
	}
}
