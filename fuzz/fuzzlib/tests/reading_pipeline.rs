use mvfuzz::{pipeline, reading};

#[test]
fn literal_model_rejects_lost_repeated_and_misbound_hits() {
	let expected = reading::literal_ranges("alpha ALPHA alpha", "alpha", true);
	assert_eq!(expected, [0..5, 6..11, 12..17]);
	for broken in [
		vec![0..5, 12..17],
		vec![0..5, 6..11, 6..11, 12..17],
		vec![1..6, 6..11, 12..17],
	] {
		assert_ne!(expected, broken);
	}
	assert!(reading::literal_ranges("alpha", "", false).is_empty());
	assert_eq!(reading::literal_ranges("aaaaa", "aa", false), [0..2, 2..4]);
}

#[test]
fn shaped_reading_survives_disclosure_navigation_and_reflow() {
	pipeline::warmup();
	for md in [
		"alpha alpha\nsecond line 中文 e\u{301} 👩‍💻 مرحبا\n\n[alpha](https://example.com)",
		"<details>\n<summary>summary</summary>\n\nalpha hidden\n\n</details>",
		"| left | right |\n| --- | --- |\n| alpha | alpha alpha |",
		"alpha &amp; alpha\n\n$x + y$ ![alt](image.png)",
	] {
		reading::check(md.as_bytes());
	}
	for field in 0..8 {
		let mut input = b"RDP1".to_vec();
		input.extend([field * 2 + 1, 0, 0, 7, 0, 1, 2, 3]);
		input.extend(0..32);
		reading::check(&input);
	}
}

#[test]
fn plain_line_starts_round_trip_through_the_source_index() {
	use markview_layout::{layout::LayoutOptions, source::SourceIndex};
	use std::collections::HashMap;

	pipeline::warmup();
	let md = "alpha\n\nbravo\n\ncharlie\n";
	let parsed = markview_commonmark::parse(md);
	let source =
		SourceIndex::new(parsed.source.clone(), &parsed.document, &parsed.map);
	let options = LayoutOptions {
		width: 480.,
		fonts: pipeline::pinned_fonts(),
		..Default::default()
	};
	let mut engine = pipeline::differential_engine();
	let snapshot = engine.layout(&parsed.document, &options);
	for at in [0, 7, 14] {
		let position = source
			.source_to_preview(&snapshot, &HashMap::new(), at)
			.expect("a plain paragraph has a preview");
		assert_eq!(position.source.start, at);
		let back = source
			.preview_to_source(
				&snapshot,
				&HashMap::new(),
				position.rect.y + position.rect.h / 2.,
			)
			.expect("a plain paragraph maps back");
		assert_eq!(back.source.start, at, "line {at} lost its source");
	}
}

#[test]
fn boundary_mode_keeps_a_hit_near_sixty_four_kibibytes() {
	pipeline::warmup();
	let mut input = b"RDB1".to_vec();
	input.extend([255, 7, 0, 0, 0, 0, 0, 0]);
	input.extend(b"tail alpha");
	reading::check(&input);
}
