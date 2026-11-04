//! Reproduce a reparse divergence: applies the same deterministic edit as
//! the `reparse` fuzz target and prints field-by-field differences.
//!
//! Run: `cargo run -p mvfuzz --bin repro -- <file>`

use std::sync::Arc;

fn main() {
	let args: Vec<String> = std::env::args().collect();
	let data = std::fs::read(&args[1]).unwrap();
	let md0 = String::from_utf8_lossy(&data).into_owned();
	let edit = mvfuzz::edit::apply_deterministic_edit(&data);
	let md1 = edit.md1;
	eprintln!("edit: {}", edit.description);

	let doc0 = markview_commonmark::parse(md0);
	let full = markview_commonmark::parse(md1.clone());
	let incr = markview_commonmark::reparse(&doc0, Arc::from(md1));

	println!(
		"content_id full={:#x} incr={:#x}",
		full.document.content_key().get(),
		incr.document.content_key().get()
	);
	println!(
		"blocks: full={} incr={}",
		full.document.blocks().len(),
		incr.document.blocks().len()
	);
	for (i, (f, x)) in full
		.document
		.blocks()
		.iter()
		.zip(incr.document.blocks())
		.enumerate()
	{
		if f != x {
			println!("block {i} differs:");
			println!(
				"  full: id={:#x} source={:?} key={:#x}",
				f.id().get(),
				full.block_source(f),
				f.content_key().get()
			);
			println!(
				"  incr: id={:#x} source={:?} key={:#x}",
				x.id().get(),
				incr.block_source(x),
				x.content_key().get()
			);
			println!("  full kind: {:?}", f.kind());
			println!("  incr kind: {:?}", x.kind());
		}
	}
}
