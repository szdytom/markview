use mvfuzz::{pipeline, web_layout};

#[test]
fn spans_disclosures_overflow_and_pagination_reach_the_consumers() {
	pipeline::warmup();
	let (document, expected, _) = web_layout::grid(&[0; 24]);
	let mut options = markview_layout::layout::LayoutOptions {
		width: 80.,
		force_open: true,
		fonts: pipeline::pinned_fonts(),
		..Default::default()
	};
	let mut engine = pipeline::differential_engine();
	let open = engine.layout(&document, &options);
	assert!(!open.blocks[0].layout.overflow.is_empty());
	assert_eq!(
		web_layout::markers(&open.full_reading_text()),
		expected.iter().map(String::as_str).collect::<Vec<_>>()
	);
	options.force_open = false;
	let closed = engine.layout(&document, &options);
	assert!(
		!web_layout::markers(&closed.flattened().full_reading_text())
			.contains(&"mk3")
	);
	let flat = open.flattened();
	let pages = markview_layout::paginate::paginate(
		&document,
		&flat,
		&markview_layout::paginate::PageGeometry {
			width_pt: 300.,
			height_pt: 120.,
			margin_pt: [10.; 4],
		},
	);
	assert!(pages.pages.len() > 1);
	for bytes in [
		&[1][..],
		&[1, 0, 1, 2][..],
		&[2, 0, 1, 2, 3][..],
		&[3, 5, 12, 99][..],
		&[0, 5, 12, 99][..],
	] {
		web_layout::check(bytes);
	}
}

#[test]
fn marker_oracle_distinguishes_loss_duplication_and_cell_order() {
	let expected = web_layout::markers("mk1\tmk2\nmk10");
	assert_eq!(expected, ["mk1", "mk2", "mk10"]);
	for broken in ["mk1 mk10", "mk1 mk2 mk2 mk10", "mk2 mk1 mk10"] {
		assert_ne!(expected, web_layout::markers(broken));
	}
}
