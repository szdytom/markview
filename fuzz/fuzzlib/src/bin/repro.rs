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

	let doc0 = markview_core::document::parse(md0);
	let full = markview_core::document::parse(md1.clone());
	let incr = markview_core::document::reparse(&doc0, Arc::from(md1));

	println!(
		"content_id full={:#x} incr={:#x}",
		full.content_id, incr.content_id
	);
	println!(
		"blocks: full={} incr={}",
		full.blocks.len(),
		incr.blocks.len()
	);
	for (i, (f, x)) in full.blocks.iter().zip(&incr.blocks).enumerate() {
		if f != x {
			println!("block {i} differs:");
			println!(
				"  full: id={:#x} source={:?} key={:#x}",
				f.id, f.source, f.content_key
			);
			println!(
				"  incr: id={:#x} source={:?} key={:#x}",
				x.id, x.source, x.content_key
			);
			println!("  full kind: {:?}", f.kind);
			println!("  incr kind: {:?}", x.kind);
		}
	}
}
