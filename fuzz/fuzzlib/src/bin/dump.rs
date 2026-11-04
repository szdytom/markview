//! Dump the blocks, source ranges, and content id of a fuzz input through
//! the full parse, the prefix parse, and the incremental reparse the
//! `reparse` target applies. For crash triage; the output points at which
//! entry point diverges.
//!
//! Run: `cargo run -p mvfuzz --bin dump -- <file>`

use std::sync::Arc;

fn main() {
	let path = std::env::args().nth(1).expect("usage: dump <file>");
	let data = std::fs::read(&path).unwrap();
	let md0 = String::from_utf8_lossy(&data).into_owned();

	let doc0 = markview_commonmark::parse(md0.clone());
	eprintln!(
		"full parse ({} bytes): {} blocks",
		md0.len(),
		doc0.document.blocks().len()
	);
	for b in doc0.document.blocks() {
		eprintln!("  src={:?} id={:016x}", doc0.block_source(b), b.id().get());
	}

	// Same source through reparse (no edit): must be identical.
	let same = markview_commonmark::reparse(&doc0, Arc::from(md0.clone()));
	eprintln!(
		"reparse(same): {} blocks, same content id: {}",
		same.document.blocks().len(),
		same.document.content_key() == doc0.document.content_key()
	);
	for b in same.document.blocks() {
		eprintln!("  src={:?} id={:016x}", same.block_source(b), b.id().get());
	}

	// The deterministic edit the `reparse` target applies.
	let edit = mvfuzz::edit::apply_deterministic_edit(&data);
	let md1 = edit.md1;
	let full = markview_commonmark::parse(md1.clone());
	let incr = markview_commonmark::reparse(&doc0, Arc::from(md1.clone()));
	eprintln!(
		"edit ({}): full={} incremental={} equal={}",
		edit.description,
		full.document.blocks().len(),
		incr.document.blocks().len(),
		full.document.content_key() == incr.document.content_key()
	);
	for b in incr.document.blocks() {
		eprintln!("  src={:?} id={:016x}", incr.block_source(b), b.id().get());
	}
}
