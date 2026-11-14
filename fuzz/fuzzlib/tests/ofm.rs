use markview_commonmark::parse_with;
use markview_document::AnchorId;
use mvfuzz::{oracle, pdf_oracle, pipeline, reading, seam};

const MARKDOWN: &str = "[[#^Block-ID|jump]] [encoded](#%5EBlock-ID)\n\n<details><summary>Folded</summary>\n\nTarget ^Block-ID\n\n</details>\n";

#[test]
fn presets_reach_block_ids_and_preserve_gfm_literals() {
	for dialect in pipeline::dialects() {
		let parsed = parse_with(MARKDOWN, dialect);
		oracle::assert_source_ranges(&parsed);
		let targets = seam::fragment_targets(&parsed);
		let block_id = targets.iter().find(|target| {
			target.anchor == AnchorId::Fragment("^Block-ID".into())
		});
		assert_eq!(block_id.is_some(), dialect.ofm);
		if let Some(target) = block_id {
			assert_eq!(target.chain.len(), 1);
			assert_eq!(parsed.details_enclosing("^Block-ID"), target.chain);
		}
	}
	let modes: std::collections::HashSet<_> = (0..64)
		.map(|n| {
			pipeline::dialect_for(&format!("{MARKDOWN}\n<!-- {n} -->")).ofm
		})
		.collect();
	assert_eq!(
		modes.len(),
		2,
		"raw pipeline inputs must reach both presets"
	);
}

#[test]
fn ofm_pipeline_reaches_real_layout_and_pdf_link_oracles() {
	pipeline::warmup();
	let md = (0..64)
		.map(|n| format!("{MARKDOWN}\n<!-- {n} -->"))
		.find(|md| pipeline::dialect_for(md).ofm)
		.unwrap();
	let (parsed, options, snapshot) = pipeline::parse_layout(&md);
	assert!(parsed.dialect.ofm && options.ofm);
	assert!(snapshot.blocks.len() >= 2);
	let (bytes, document, snapshot, pagination, geometry) =
		pipeline::export_pdf(&md);
	assert!(document.dialect.ofm);
	assert!(snapshot.anchor_y("^Block-ID").is_some());
	assert!(pagination.anchors.contains_key("^Block-ID"));
	let structure = pdf_oracle::Structure::load(&bytes).unwrap();
	structure.assert_links(&snapshot, &geometry, &pagination);
}

#[test]
fn reading_generator_checks_hidden_ids_against_plain_field_expectations() {
	pipeline::warmup();
	for generated in [false, true] {
		for body in [b"".as_slice(), b"alpha", MARKDOWN.as_bytes()] {
			let mut input = b"RDP1".to_vec();
			input.extend([u8::from(generated), 0, 0, 7, 0, 1, 2, 1]);
			input.extend(body);
			reading::check(&input);
		}
	}
}
