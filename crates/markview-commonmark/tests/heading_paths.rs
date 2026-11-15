use markview_commonmark::{
	Dialect, parse_incremental, parse_prefix, parse_with,
};
use markview_document::{BlockKind, plain_text};
use proptest::prelude::*;
use std::sync::Arc;

fn target(source: &str, link: &str) -> Option<String> {
	let doc = parse_with(format!("{link}\n\n{source}"), Dialect::default());
	let BlockKind::Paragraph(text) = doc.blocks()[0].kind() else {
		panic!("expected a link");
	};
	let url = text.iter().find_map(|span| span.link_url()).unwrap();
	let fragment = url.split_once('#').unwrap().1.replace("%25", "%");
	let block = doc.anchor_block(fragment.as_str())?;
	let BlockKind::Heading { anchor, .. } = block.kind() else {
		panic!("expected a heading");
	};
	Some(anchor.clone())
}

#[test]
fn heading_paths_distinguish_repeated_children_without_changing_slug_links() {
	let source = "# First\n\n## Child heading\n\n# Second\n\n## Child heading\n\n### Leaf\n";
	for (link, expected) in [
		("[[#First#Child heading]]", "child-heading"),
		("[[note#Second#Child heading|Alias]]", "child-heading-1"),
		("[[#Second#Child heading#Leaf]]", "leaf"),
		("[[#Second#Leaf]]", "leaf"),
		("[[#Second##Child heading#]]", "child-heading-1"),
		("[[#Child heading#Leaf]]", "leaf"),
		("[[#Child heading]]", "child-heading"),
		("[[#child-heading-1]]", "child-heading-1"),
	] {
		assert_eq!(target(source, link).as_deref(), Some(expected), "{link}");
	}
	for link in [
		"[[#Missing#Child heading]]",
		"[[#Second#Missing]]",
		"[[#First#Second]]",
		"[[#Leaf#Child heading]]",
	] {
		assert_eq!(target(source, link), None, "{link}");
	}
}

#[test]
fn heading_paths_normalize_case_punctuation_and_whitespace_like_obsidian() {
	let source = "# BEFORE\n\n## A.B\n\n# **Parent**  100%\n\n### A.B\n\n#### 中文 `Child`\n\n# Parent 100\n\n## AB\n";
	for (link, expected) in [
		("[[#parent 100%#A.B]]", "ab-1"),
		("[[#PARENT 100#A B]]", "ab-1"),
		("[[#Parent 100%#A.B#中文 Child]]", "中文-child"),
		("[[#Parent 100#AB]]", "ab-2"),
	] {
		assert_eq!(target(source, link).as_deref(), Some(expected), "{link}");
	}
}

#[test]
fn heading_paths_reveal_the_disclosures_of_the_selected_child() {
	let doc = parse_with(
		"# First\n\n## Child\n\n# Second\n\n<details><summary>Folded</summary>\n\n## Child\n\nBody\n\n</details>",
		Dialect::default(),
	);
	let selected = doc.anchor_block("Second#Child").unwrap();
	assert_eq!(selected.id(), doc.anchor_block("child-1").unwrap().id());
	assert_eq!(doc.details_enclosing("Second#Child").len(), 1);
	assert!(doc.details_enclosing("First#Child").is_empty());
}

#[test]
fn explicit_fragment_ids_take_precedence_over_heading_paths() {
	let doc = parse_with(
		"<div id='Parent#Child'>Explicit target</div>\n\n# Parent\n\n## Child\n",
		Dialect::default(),
	);
	let explicit = doc.anchor_block("Parent#Child").unwrap();
	assert_ne!(explicit.id(), doc.anchor_block("child").unwrap().id());
	assert_eq!(
		doc.resolve_anchor("Parent#Child").fragment(),
		Some("Parent#Child")
	);
	assert_eq!(doc.resolve_anchor("parent#child").fragment(), Some("child"));
}

#[test]
fn heading_paths_follow_incremental_parent_renames_and_prefix_loading() {
	let source = "# First\n\n## Child\n\n# Second\n\n## Child\n\nTail\n";
	let before = parse_with(source, Dialect::default());
	let changed: Arc<str> = source.replace("Second", "Renamed").into();
	let incremental =
		parse_incremental(&before, changed.clone(), Dialect::default())
			.expect("a local heading edit supports incremental parsing");
	assert!(incremental.anchor_block("Second#Child").is_none());
	let selected = incremental.anchor_block("Renamed#Child").unwrap();
	assert_eq!(
		selected.id(),
		incremental.anchor_block("child-1").unwrap().id()
	);
	assert_eq!(
		incremental.map.validate(&changed, &incremental.document),
		Ok(())
	);
	let changed_level: Arc<str> =
		changed.replace("# Renamed", "## Renamed").into();
	let reparsed =
		markview_commonmark::reparse(&incremental, changed_level.clone());
	assert!(reparsed.anchor_block("Renamed#Child").is_none());
	assert_eq!(
		reparsed.document.content_key(),
		parse_with(changed_level, Dialect::default())
			.document
			.content_key()
	);
	for bytes in 1..changed.len() {
		let Some(prefix) = parse_prefix(&changed, bytes) else {
			continue;
		};
		assert_eq!(
			prefix.anchor_block("Renamed#Child").is_some(),
			prefix
				.outline_entries()
				.iter()
				.any(|entry| entry.anchor == "child-1"),
		);
	}
}

proptest! {
	#[test]
	fn heading_paths_select_the_named_branch_among_duplicate_children(
		branches in 2usize..20,
		selected in any::<usize>(),
		levels in prop::collection::vec(2u8..7, 1..5),
	) {
		let selected = selected % branches;
		let mut levels = levels;
		levels.sort_unstable();
		levels.dedup();
		let mut source = String::new();
		for branch in 0..branches {
			source.push_str(&format!("# Branch {branch}\n\n"));
			for level in &levels {
				source.push_str(&format!("{} Child {level}\n\n", "#".repeat(usize::from(*level))));
			}
		}
		let doc = parse_with(source, Dialect::default());
		let mut path = format!("Branch {selected}");
		for level in levels {
			path.push_str(&format!("#Child {level}"));
			let block = doc.anchor_block(path.as_str()).expect("the selected branch exists");
			let BlockKind::Heading { text, anchor, .. } = block.kind() else { unreachable!() };
			prop_assert_eq!(plain_text(text), format!("Child {level}"));
			let expected = if selected == 0 { format!("child-{level}") } else { format!("child-{level}-{selected}") };
			prop_assert_eq!(anchor, &expected);
		}
	}
}
