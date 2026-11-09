use std::sync::{Arc, Mutex};

use markview_layout::{
	background::{Executor, Task},
	layout::{LayoutEngine, LayoutOptions},
	scene::{Draw, LinkId},
	style::Stylesheet,
};
use mvfuzz::{oracle, pipeline};

#[derive(Default)]
struct Deferred(Mutex<Vec<Task>>);

impl Executor for Deferred {
	fn try_submit(&self, task: Task) -> Result<(), Task> {
		self.0.lock().unwrap().push(task);
		Ok(())
	}
}

#[test]
fn layout_fingerprint_tracks_link_commands_and_occurrences() {
	let md = "[**bold** normal](https://example.com)";
	let doc = markview_commonmark::parse(md);
	let mut engine = pipeline::differential_engine();
	let snapshot = engine.layout(&doc, &pipeline::options_for(md));
	let expected = oracle::layout(&snapshot);
	assert_eq!(
		expected,
		oracle::layout(&engine.layout(&doc, &pipeline::options_for(md)))
	);

	for field in 0..3 {
		let mut changed = snapshot.flattened();
		let link = Arc::get_mut(&mut changed.blocks[0].layout)
			.unwrap()
			.links
			.get_mut(0)
			.unwrap();
		match field {
			0 => link.commands.start += 1,
			1 => link.commands.end += 1,
			2 => link.id = LinkId::Special(usize::MAX),
			_ => unreachable!(),
		}
		assert_ne!(expected, oracle::layout(&changed), "link field {field}");
	}
}

#[test]
fn code_highlights_change_only_paint_and_differentials_start_settled() {
	for code in ["~\0~", "\0", "let value = 42;"] {
		let md = format!("```rust\n{code}\n```\n");
		let doc = markview_commonmark::parse(md.clone());
		let options = pipeline::options_for(&md);
		let executor = Arc::new(Deferred::default());
		let mut delayed =
			LayoutEngine::with_executor(executor.clone(), Arc::new(|| {}));
		let mut cold = delayed.layout(&doc, &options);
		for task in executor.0.lock().unwrap().drain(..) {
			task.run();
		}
		assert!(delayed.wait_highlights());
		let settled = delayed.layout(&doc, &options);
		assert_ne!(oracle::layout(&cold), oracle::layout(&settled));
		delayed.release_document();

		// Copy only paints; every other fingerprinted field must already match.
		for (a, b) in cold.blocks.iter_mut().zip(&settled.blocks) {
			for (a, b) in Arc::get_mut(&mut a.layout)
				.unwrap()
				.draws
				.iter_mut()
				.zip(&b.layout.draws)
			{
				if let (Draw::Glyph(a), Draw::Glyph(b)) = (a, b) {
					a.paint = b.paint;
				}
			}
		}
		assert_eq!(oracle::layout(&cold), oracle::layout(&settled));

		let mut engine = pipeline::differential_engine();
		let first = engine.layout(&doc, &options);
		let cached = engine.layout(&doc, &options);
		let fresh = pipeline::differential_engine().layout(&doc, &options);
		assert_eq!(oracle::layout(&first), oracle::layout(&settled));
		assert_eq!(oracle::layout(&first), oracle::layout(&cached));
		assert_eq!(oracle::layout(&cached), oracle::layout(&fresh));
		assert_eq!(cached.reused, cached.blocks.len());
	}
}

#[test]
fn shrinkable_histories_reach_the_same_complete_state() {
	pipeline::warmup();
	let body = "[alpha](https://example.org)\n\n![pixels](image.png)\n\n<details>\n<summary>summary</summary>\n\nhidden alpha\n\n</details>\n";
	let operations = [
		0, 0, 0, 3, 0, 0, 2, 5, 2, 0, 5, 73, 3, 1, 9, 0, 4, 1, 6, 0, 7, 0, 8,
		0, 3, 0,
	];
	let mut input = b"LDH1".to_vec();
	input.push((operations.len() / 2) as u8);
	input.extend(operations);
	input.extend(body.as_bytes());
	mvfuzz::history::check(&input);
	// Removing an operation preserves the remaining history and its body.
	input.drain(5..7);
	input[4] -= 1;
	mvfuzz::history::check(&input);
}

#[test]
fn moving_cached_paragraphs_between_disclosures_preserves_layout() {
	let mut input = b"LDH1\x01\x03\x49".to_vec();
	input.extend_from_slice(b"<details>hidden alpha\n\n<\n<details></details>>\n\nhidden alpha\n</details>");
	mvfuzz::history::check(&input);
}

// `crash-653ba585e31da44c5215d94587efb109c6ba994f` laid out a disclosure
// inside a figure that was re-parsed from the same source; the reused
// paragraphs kept a baseline rounded through the old absolute origin.
#[test]
fn reused_nested_leaf_keeps_a_cold_pass_baseline() {
	let mut input = b"LDH1\x01\x03\x01".to_vec();
	input.extend_from_slice(b"<details>a\n\na\n</details>");
	mvfuzz::history::check(&input);
}

// A framed child can sit above its parent's origin to trim its leading
// spacing; a local leaf measurement must not raise the parent's height.
#[test]
fn trimmed_leading_spacing_keeps_translated_extents() {
	let mut sheet = (*Stylesheet::bundled(false)).clone();
	sheet.merge(
		&Stylesheet::parse(
			"format_version = 2\nversion = 1\n[[rule]]\nwhen = ['blockquote']\npadding = 0\nborder_width = 0\nspace_after = 0\n",
		)
		.unwrap(),
	);
	let options = LayoutOptions {
		stylesheet: Arc::new(sheet),
		fonts: pipeline::pinned_fonts(),
		..Default::default()
	};
	let mut engine = pipeline::differential_engine();
	for source in [
		"> ## Heading\n",
		"> ## Heading\n>\n> <a id=\"a\"></a>\n",
	] {
		let doc = markview_commonmark::parse(source);
		engine.layout(&doc, &options);
		let warm = engine.layout(&doc, &options);
		let cold = pipeline::differential_engine().layout(&doc, &options);
		assert_eq!(
			oracle::layout(&warm),
			oracle::layout(&cold),
			"{source}"
		);
	}
}
