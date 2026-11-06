use std::sync::Arc;

use markview_layout::scene::Draw;
use mvfuzz::{parallel, pipeline};

#[test]
fn differential_exercises_workers_cache_reflows_and_cancelled_edits() {
	for md in [
		"",
		"A single paragraph.",
		"---\ntitle: Test\n---\n\n# Title\n\nRepeated.\n\nRepeated.\n",
		"# 标题\n\n中文 English العربية **bold** [link](#标题).\n\n> Quote\n>\n> - nested item\n\n<details>\n<summary>Closed</summary>\n\nHidden **text**.\n\n<details open>\n<summary>Nested</summary>\n\n![Alt](image.png)\n\n</details>\n\n</details>\n\n| A | B |\n|---|---|\n| $x$ | Text[^f] |\n\n[^f]: $y$\n\n```rust\nlet a = 1;\n```\n",
	] {
		parallel::check(md);
	}
}

#[test]
#[should_panic(expected = "visible serial/parallel scene differs")]
fn differential_detects_a_search_binding_change() {
	let doc = markview_commonmark::parse("Reading text.");
	let snapshot = pipeline::differential_engine()
		.layout(&doc, &pipeline::options_for("test"));
	let mut changed = snapshot.flattened();
	let block = Arc::get_mut(&mut changed.blocks[0].layout).unwrap();
	block.text[0].search_ranges.push((0..1, 1..2));
	parallel::assert_equivalent(&snapshot, &changed);
}

#[test]
#[should_panic(expected = "retained serial/parallel block")]
fn differential_detects_hidden_geometry_changes() {
	let doc = markview_commonmark::parse(
		"<details>\n<summary>Closed</summary>\n\nHidden body.\n\n</details>\n",
	);
	let mut options = pipeline::options_for("hidden");
	options.force_open = false;
	let a = pipeline::differential_engine().layout(&doc, &options);
	let mut b = pipeline::differential_engine().layout(&doc, &options);
	let block = &mut b.blocks[0];
	let hidden = block
		.layout
		.draws
		.iter()
		.enumerate()
		.find_map(|(i, draw)| {
			(matches!(draw, Draw::Glyph(_))
				&& block.command_offset(i).is_none())
			.then_some(i)
		})
		.unwrap();
	let Draw::Glyph(glyph) = Arc::get_mut(&mut block.layout)
		.unwrap()
		.draws
		.get_mut(hidden)
		.unwrap()
	else {
		unreachable!();
	};
	glyph.y += 1.;
	parallel::assert_equivalent(&a, &b);
}
