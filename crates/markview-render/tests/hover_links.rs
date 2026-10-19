//! Hover marks exactly the draw ranges of one link occurrence, so separate
//! links that share an address never highlight together.

use markview_layout::{
	background::Direct,
	fonts::FontConfig,
	layout::{LayoutEngine, LayoutOptions, LayoutSnapshot},
	scene::LinkRef,
	style::Stylesheet,
};
use std::{collections::BTreeSet, sync::Arc};

fn fonts() -> FontConfig {
	FontConfig::from_faces(
		0x686f7665,
		[
			include_bytes!(
				"../../markview-layout/tests/fonts/NotoSerif-Regular-subset.otf"
			)
			.as_slice(),
			include_bytes!(
				"../../markview-layout/tests/fonts/NotoSans-Bold-subset.otf"
			)
			.as_slice(),
			include_bytes!(
				"../../markview-layout/tests/fonts/NotoSansMono-Regular-subset.otf"
			)
			.as_slice(),
		]
		.into_iter()
		.map(|data| parley::fontique::Blob::new(Arc::new(data)))
		.collect(),
	)
}

fn layout(source: &str) -> LayoutSnapshot {
	layout_width(source, 700.0)
}

fn layout_width(source: &str, width: f32) -> LayoutSnapshot {
	let doc = markview_commonmark::parse(source);
	let mut engine =
		LayoutEngine::with_executor(Arc::new(Direct), Arc::new(|| {}));
	let sheet = Stylesheet::bundled(false);
	let options = LayoutOptions {
		width,
		fonts: fonts(),
		stylesheet: sheet.clone(),
		..Default::default()
	};
	let snapshot = engine.layout(&doc, &options);
	if engine.wait_highlights() {
		engine.layout(&doc, &options)
	} else {
		snapshot
	}
}

/// Every occurrence with `url`, in document order.
fn links<'a>(snapshot: &'a LayoutSnapshot, url: &str) -> Vec<LinkRef<'a>> {
	snapshot
		.blocks
		.iter()
		.flat_map(|block| {
			block
				.layout
				.links
				.iter()
				.filter(|link| &*link.url == url)
				.map(|link| LinkRef {
					url: link.url.as_ref(),
					block: block.source.start,
					id: link.id,
				})
		})
		.collect()
}

/// The `(block, command)` draws the renderer marks for one occurrence.
fn marked(
	snapshot: &LayoutSnapshot,
	target: LinkRef<'_>,
) -> BTreeSet<(usize, usize)> {
	let mut marked = BTreeSet::new();
	for block in snapshot
		.blocks
		.iter()
		.filter(|b| b.source.start == target.block)
	{
		for command in 0..block.layout.draws.len() {
			if block.layout.link_hovered(command, target.url, target.id) {
				marked.insert((block.source.start, command));
			}
		}
	}
	marked
}

/// The draws owned by one occurrence, independent of the hover query.
fn expected(
	snapshot: &LayoutSnapshot,
	target: LinkRef<'_>,
) -> BTreeSet<(usize, usize)> {
	let mut expected = BTreeSet::new();
	for block in snapshot
		.blocks
		.iter()
		.filter(|b| b.source.start == target.block)
	{
		for link in block.layout.links.iter().filter(|l| l.id == target.id) {
			for command in link.commands.clone() {
				expected.insert((block.source.start, command));
			}
		}
	}
	expected
}

#[test]
fn hover_marks_only_the_matching_link_occurrences() {
	// `target` appears three times: alone in a quote and a later paragraph,
	// and after another link on a list item's line.
	let snapshot = layout(
		"> A tip that ends in [one](target).\n\n- head: [alpha](first)\n- tail: [beta](second) -> [one](target)\n\n[one](target) again in a later paragraph.\n",
	);
	let targets = links(&snapshot, "target");
	assert_eq!(targets.len(), 3, "expected three target occurrences");
	let second = links(&snapshot, "second");
	assert_eq!(second.len(), 1);
	for (index, target) in targets.iter().enumerate() {
		let marks = marked(&snapshot, *target);
		assert_eq!(
			marks,
			expected(&snapshot, *target),
			"target {index} must mark exactly its own draws"
		);
		assert!(!marks.is_empty(), "target {index} never highlights");
		assert!(
			marks.iter().all(|(block, _)| *block == target.block),
			"target {index} leaked into another block"
		);
		assert!(
			marks.is_disjoint(&marked(&snapshot, second[0])),
			"target {index} and the other address overlap"
		);
	}
	for (index, target) in targets.iter().enumerate() {
		for other in &targets[index + 1..] {
			assert!(
				marked(&snapshot, *target)
					.is_disjoint(&marked(&snapshot, *other)),
				"two occurrences of one address highlight together"
			);
		}
	}
}

#[test]
fn same_url_links_on_one_line_hover_independently() {
	let snapshot = layout("[one](target) and [two](target)\n");
	let targets = links(&snapshot, "target");
	assert_eq!(targets.len(), 2);
	assert_ne!(
		targets[0].id, targets[1].id,
		"separate links need separate occurrences"
	);
	let first = marked(&snapshot, targets[0]);
	let second = marked(&snapshot, targets[1]);
	assert!(!first.is_empty());
	assert!(!second.is_empty());
	assert_eq!(first, expected(&snapshot, targets[0]));
	assert_eq!(second, expected(&snapshot, targets[1]));
	assert!(
		first.is_disjoint(&second),
		"two links with one address highlight the same draws"
	);
	// Adjacent links have no separator between their text inlines, so the
	// parser and layout must still preserve the occurrence boundary.
	let adjacent_snapshot = layout("[one](target)[two](target)\n");
	let adjacent = links(&adjacent_snapshot, "target");
	assert_eq!(adjacent.len(), 2);
	assert_ne!(adjacent[0].id, adjacent[1].id);
	assert!(
		marked(&adjacent_snapshot, adjacent[0])
			.is_disjoint(&marked(&adjacent_snapshot, adjacent[1]))
	);
}

#[test]
fn identical_blocks_keep_their_link_occurrences_apart() {
	// Identical blocks share a block fingerprint, so the hover key must use a
	// position that still distinguishes them.
	let snapshot = layout("[one](target)\n\n[one](target)\n");
	let targets = links(&snapshot, "target");
	assert_eq!(targets.len(), 2);
	assert_ne!(targets[0].block, targets[1].block);
	assert_eq!(
		marked(&snapshot, targets[0]),
		expected(&snapshot, targets[0])
	);
	assert_eq!(
		marked(&snapshot, targets[1]),
		expected(&snapshot, targets[1])
	);
	assert!(
		marked(&snapshot, targets[0])
			.is_disjoint(&marked(&snapshot, targets[1])),
		"identical blocks hover together"
	);
}

#[test]
fn every_fragment_of_a_wrapped_link_shares_an_occurrence() {
	let snapshot = layout_width(
		"[an intentionally long linked phrase that wraps across lines](https://example.com)\n",
		120.0,
	);
	let links = &snapshot.blocks[0].layout.links;
	assert!(links.len() > 1, "expected a wrapped link");
	assert!(
		links.iter().all(|link| link.id == links[0].id),
		"wrapped fragments must share one occurrence"
	);
	let target = LinkRef {
		url: links[0].url.as_ref(),
		block: snapshot.blocks[0].source.start,
		id: links[0].id,
	};
	let marked = marked(&snapshot, target);
	assert_eq!(marked, expected(&snapshot, target));
	assert!(
		links.iter().all(|link| marked
			.contains(&(snapshot.blocks[0].source.start, link.commands.start))),
		"every wrapped fragment must hover together"
	);
}
