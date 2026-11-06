//! G6, Tier 1 + Tier 2 + Tier 3: the whole PDF export path. Oracle (O1, O2,
//! O5.4): no panic within budgets; the same input exports to identical bytes
//! (I6); and the bytes must read back as a PDF whose *structure* agrees with
//! the document it came from — page count, link destinations, per-page text
//! and page geometry, not just the writer's own bookkeeping.
#![no_main]

use libfuzzer_sys::fuzz_target;
use mvfuzz::{budget, pdf_oracle, pipeline};

fuzz_target!(|data: &[u8]| {
	let budget = budget::Budget::pdf().from_env();
	let md = String::from_utf8_lossy(data);
	if md.len() > 48 * 1024 {
		return;
	}
	let guard = budget::InputGuard::new();
	let (a, _, snapshot, pagination, geometry) = pipeline::export_pdf(&md);
	let (b, ..) = pipeline::export_pdf(&md);
	assert_eq!(a, b, "two exports of the same input differ");

	// Structured readback: the bytes are a PDF, and every structural claim
	// the exporter's inputs imply holds in the re-parsed output.
	let structure =
		pdf_oracle::Structure::load(&a).unwrap_or_else(|e| panic!("{e}"));
	structure.assert_page_count(&pagination);
	structure.assert_well_formed(pagination.pages.len());
	structure.assert_page_geometry();
	structure.assert_pages_have_content(&pagination);
	structure.assert_text_round_trip(&snapshot);
	structure.assert_links(&snapshot, &geometry, &pagination);

	if md.contains("![") || md.contains("<img") {
		// The same document with precompressed image resources must read back
		// with the same structure; byte equality across encoders is not asked.
		let (p, _, snapshot, pagination, geometry) =
			pipeline::export_pdf_prepared(&md);
		let structure =
			pdf_oracle::Structure::load(&p).unwrap_or_else(|e| panic!("{e}"));
		structure.assert_page_count(&pagination);
		structure.assert_well_formed(pagination.pages.len());
		structure.assert_page_geometry();
		structure.assert_pages_have_content(&pagination);
		structure.assert_text_round_trip(&snapshot);
		structure.assert_links(&snapshot, &geometry, &pagination);
	}

	guard.finish(&budget, md.len());
});
