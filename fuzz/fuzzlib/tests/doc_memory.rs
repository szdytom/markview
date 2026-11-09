//! Frozen reader output, including reviewed geometry corrections.
use mvfuzz::behavior;
use std::{fs::File, io::Read, path::Path};
#[test]
fn reader_output_matches_frozen_baselines() {
	for (name, actual) in behavior::records() {
		let path = Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("tests/baselines/doc-memory")
			.join(format!("{name}.json.gz"));
		let mut bytes = Vec::new();
		flate2::read::GzDecoder::new(File::open(path).unwrap())
			.read_to_end(&mut bytes)
			.unwrap();
		let expected = serde_json::from_slice(&bytes).unwrap();
		if let Some(diff) = behavior::difference(&expected, &actual, &name) {
			let artifacts = Path::new(env!("CARGO_MANIFEST_DIR"))
				.join("../../artifacts/doc-memory-regression");
			std::fs::create_dir_all(&artifacts).unwrap();
			std::fs::write(
				artifacts.join(format!("{name}.actual.json")),
				serde_json::to_vec_pretty(&actual).unwrap(),
			)
			.unwrap();
			std::fs::write(artifacts.join(format!("{name}.diff")), &diff)
				.unwrap();
			panic!("{diff}");
		}
	}
}

#[test]
fn semantic_records_distinguish_undrawn_block_attributes() {
	fn list(start: isize) -> markview_commonmark::MarkdownParse {
		use markview_document::{BlockKind, DocumentBuilder, DocumentRevision};
		let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
		let block = builder.block(
			Vec::new(),
			BlockKind::List {
				start: Some(start),
				reversed: false,
				tight: true,
				items: Vec::new(),
			},
		);
		markview_commonmark::MarkdownParse {
			document: builder.finish(vec![block]),
			source: "".into(),
			map: Default::default(),
			dialect: Default::default(),
		}
	}
	let before = behavior::document(&list(3));
	let after = behavior::document(&list(4));
	let difference = behavior::difference(&before, &after, "document").unwrap();
	assert!(difference.contains("attributes.start"), "{difference}");
}
