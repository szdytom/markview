use markview_document::{
	Block, BlockKind, CellAlign, CellKind, DocumentBuilder, DocumentRevision,
	Inline, InlineKind, Table, TableCell, TableColumn, TableGroup,
	TableGroupKind, TableRow, TextStyle,
};
use markview_layout::{
	fonts::FontConfig,
	layout::{LayoutEngine, LayoutOptions},
	search::{SearchIndex, SearchOptions},
};
use std::sync::{
	Arc,
	atomic::{AtomicU32, Ordering},
};

fn make(kind: BlockKind, anchors: Vec<String>) -> Block {
	static NEXT: AtomicU32 = AtomicU32::new(0);
	let after = NEXT.fetch_add(1, Ordering::Relaxed);
	let mut builder = DocumentBuilder::continuing(
		DocumentRevision::new(1),
		(after > 0).then(|| markview_document::BlockId::new(after).unwrap()),
	);
	builder.block(anchors, kind)
}

fn para(text: &str) -> Block {
	let text = vec![Inline {
		link: None,
		kind: InlineKind::Text(text.into()),
		style: TextStyle::default(),
	}];
	make(BlockKind::Paragraph(text), vec!["cell".into()])
}

fn cell(text: &str, kind: CellKind, rowspan: u32, colspan: u32) -> TableCell {
	TableCell {
		kind,
		rowspan,
		colspan,
		align: None,
		blocks: vec![para(text)],
	}
}

fn table() -> BlockKind {
	let head = TableGroup {
		kind: TableGroupKind::Head,
		rows: vec![
			TableRow {
				cells: vec![
					cell("Kind", CellKind::Header, 2, 1),
					cell("Rates", CellKind::Header, 1, 2),
				],
			},
			TableRow {
				cells: vec![
					cell("A", CellKind::Header, 1, 1),
					cell("B", CellKind::Header, 1, 1),
				],
			},
		],
	};
	let body = TableGroup {
		kind: TableGroupKind::Body,
		rows: vec![TableRow {
			cells: vec![
				cell("x", CellKind::Data, 1, 1),
				cell("one", CellKind::Data, 1, 1),
				cell("two", CellKind::Data, 1, 1),
			],
		}],
	};
	let foot = TableGroup {
		kind: TableGroupKind::Foot,
		rows: vec![TableRow {
			cells: vec![cell("Total", CellKind::Data, 1, 3)],
		}],
	};
	let caption = vec![Inline {
		link: None,
		kind: InlineKind::Text("Coverage".into()),
		style: TextStyle::default(),
	}];
	BlockKind::Table(Table {
		caption: Some(caption),
		columns: vec![
			TableColumn {
				align: Some(CellAlign::Left),
			},
			TableColumn { align: None },
			TableColumn { align: None },
		],
		groups: vec![head, body, foot],
	})
}

fn options() -> LayoutOptions {
	LayoutOptions {
		width: 260.,
		fonts: FontConfig::from_faces(
			0x7461626c65,
			vec![parley::fontique::Blob::new(Arc::new(
				include_bytes!("fonts/NotoSerif-Regular-subset.otf").as_slice(),
			))],
		),
		..Default::default()
	}
}

#[test]
fn a_table_without_cells_still_lays_out_its_caption() {
	let caption = vec![Inline {
		link: None,
		kind: InlineKind::Text("Only caption".into()),
		style: TextStyle::default(),
	}];
	let table = make(
		BlockKind::Table(Table {
			caption: Some(caption),
			columns: Vec::new(),
			groups: Vec::new(),
		}),
		Vec::new(),
	);
	let document =
		DocumentBuilder::new(DocumentRevision::new(1)).finish(vec![table]);
	let snapshot = LayoutEngine::new().layout(&document, &options());
	assert!(
		snapshot.full_reading_text().contains("Only caption"),
		"the caption is not lost: {:?}",
		snapshot.full_reading_text()
	);
}

#[test]
fn spans_groups_caption_blocks_and_field_order_layout_once() {
	let table = make(table(), Vec::new());
	let document =
		DocumentBuilder::new(DocumentRevision::new(1)).finish(vec![table]);
	let snapshot = LayoutEngine::new().layout(&document, &options());
	let mut text = snapshot.full_reading_text();
	for expected in ["Kind", "Rates", "A", "B", "x", "one", "two", "Total"] {
		assert_eq!(
			text.matches(expected).count(),
			1,
			"{expected} appears once: {text}"
		);
	}
	assert!(
		text.ends_with("Coverage"),
		"the caption closes the reading text: {text:?}"
	);
	// The caption is a field of the same block, after the cells.
	let index = SearchIndex::new(&document);
	for query in ["Kind", "Rates", "one", "Total", "Coverage"] {
		assert_eq!(
			index.find(query, SearchOptions::default()).len(),
			1,
			"{query}"
		);
	}
	let all = snapshot.select_all(1).unwrap();
	text = snapshot.extract_text(all, 1);
	assert!(text.contains("Kind\tRates"));
	assert!(text.contains("Total"));
	assert!(text.contains("Coverage"));
}

#[test]
fn narrow_tables_overflow_instead_of_shrinking_text() {
	let table = make(table(), Vec::new());
	let document =
		DocumentBuilder::new(DocumentRevision::new(1)).finish(vec![table]);
	let snapshot = LayoutEngine::new().layout(
		&document,
		&LayoutOptions {
			width: 80.,
			..options()
		},
	);
	let block = &snapshot.blocks[0];
	assert!(!block.layout.overflow.is_empty());
	assert!(block.layout.overflow[0].content_width > block.layout.width);
	assert!(snapshot.height.is_finite());
}

#[test]
fn pagination_covers_every_table_band_once_across_pages() {
	let table = make(table(), Vec::new());
	let document =
		DocumentBuilder::new(DocumentRevision::new(1)).finish(vec![table]);
	let snapshot = LayoutEngine::new().layout(&document, &options());
	let geometry = markview_layout::paginate::PageGeometry {
		width_pt: 300.,
		height_pt: 120.,
		margin_pt: [10.; 4],
	};
	let pages =
		markview_layout::paginate::paginate(&document, &snapshot, &geometry);
	assert!(pages.pages.len() > 1, "the table should need several pages");
	let mut seen = std::collections::BTreeMap::new();
	for page in &pages.pages {
		for item in page {
			if item.block != 0 {
				continue;
			}
			for band in item.bands.clone() {
				*seen.entry(band).or_insert(0) += 1;
			}
		}
	}
	let max = seen.keys().copied().max().unwrap_or(0);
	assert_eq!(seen.len(), max + 1, "a band was skipped");
	assert!(seen.values().all(|count| *count == 1), "a band repeated");
}

#[test]
fn ordered_markers_follow_explicit_values_and_reversed_start() {
	use markview_document::ListItem;
	fn list(start: isize, reversed: bool, numbers: &[Option<isize>]) -> Block {
		let items = numbers
			.iter()
			.map(|number| ListItem {
				checked: None,
				number: *number,
				blocks: vec![para("item")],
			})
			.collect();
		make(
			BlockKind::List {
				start: Some(start),
				reversed,
				tight: true,
				items,
			},
			Vec::new(),
		)
	}
	let forward = make(
		BlockKind::List {
			start: Some(1),
			reversed: false,
			tight: true,
			items: vec![
				ListItem {
					checked: None,
					number: Some(5),
					blocks: vec![para("first")],
				},
				ListItem {
					checked: None,
					number: None,
					blocks: vec![para("second")],
				},
			],
		},
		Vec::new(),
	);
	let reverse = list(3, true, &[None, None, None]);
	let negative = list(1, true, &[None, None, None]);
	let document = DocumentBuilder::new(DocumentRevision::new(1))
		.finish(vec![forward, reverse, negative]);
	let snapshot = LayoutEngine::new().layout(&document, &options());
	let text = snapshot.full_reading_text();
	assert!(text.contains("5. first"), "{text}");
	assert!(text.contains("6. second"), "{text}");
	assert!(text.contains("3. item"), "{text}");
	assert!(text.contains("2. item"), "{text}");
	assert!(text.contains("1. item"), "{text}");
	assert!(text.contains("0. item"), "{text}");
	assert!(text.contains("-1. item"), "{text}");
}

#[test]
fn a_collapsed_cell_disclosure_does_not_shift_the_next_cell() {
	let summary = vec![Inline {
		link: None,
		kind: InlineKind::Text("Summary".into()),
		style: TextStyle::default(),
	}];
	let details = make(
		BlockKind::Details {
			default_open: false,
			summary,
			blocks: vec![para(
				"Hidden body that would occupy several lines when expanded.",
			)],
		},
		Vec::new(),
	);
	let right = para("Right cell");
	let cell = |blocks| TableCell {
		kind: CellKind::Data,
		rowspan: 1,
		colspan: 1,
		align: None,
		blocks,
	};
	let table = make(
		BlockKind::Table(Table {
			caption: None,
			columns: vec![TableColumn { align: None }; 2],
			groups: vec![TableGroup {
				kind: TableGroupKind::Body,
				rows: vec![TableRow {
					cells: vec![cell(vec![details]), cell(vec![right])],
				}],
			}],
		}),
		Vec::new(),
	);
	let document =
		DocumentBuilder::new(DocumentRevision::new(1)).finish(vec![table]);
	let snapshot = LayoutEngine::new().layout(&document, &options());
	let block = &snapshot.blocks[0];
	assert!(block.height() > 0.);
	let text = snapshot.full_reading_text();
	assert!(text.contains("Right cell"));
	let y = |needle: &str| {
		block
			.layout
			.text
			.iter()
			.find(|text| text.text == needle)
			.and_then(|text| text.clusters.first())
			.map(|cluster| cluster.rect.y)
			.unwrap_or_else(|| panic!("missing {needle}"))
	};
	// The row origin is the summary line the disclosure opens with; the other
	// cell must start on it, not above it and not below the hidden body.
	let (origin, right) = (y("Summary"), y("Right cell"));
	// The two cells use different first-line metrics (summary vs paragraph),
	// so allow less than one line of ascender difference while still rejecting
	// the multi-line shift a collapsed body would cause.
	assert!(
		(right - origin).abs() < 2.0,
		"the collapsed disclosure shifted the right cell to {right} from the row origin {origin}"
	);
}

/// The absolute text band of one node in the first block, from the selection
/// geometry the reader highlights with.
fn text_band(
	snapshot: &markview_layout::layout::LayoutSnapshot,
	node: usize,
) -> (f32, f32) {
	let mut selection = snapshot.select_all(1).unwrap();
	let length = snapshot.blocks[0].layout.text[node].text.len();
	selection.anchor = snapshot
		.semantic_at(0, node, 0, markview_layout::text::Affinity::Before)
		.unwrap();
	selection.focus = snapshot
		.semantic_at(0, node, length, markview_layout::text::Affinity::After)
		.unwrap();
	let rects = snapshot.selection_rects(selection, &Default::default(), 1);
	assert!(!rects.is_empty(), "node {node} has no placed text");
	let top = rects.iter().map(|rect| rect.y).fold(f32::MAX, f32::min);
	let bottom = rects
		.iter()
		.map(|rect| rect.y + rect.h)
		.fold(f32::MIN, f32::max);
	(top, bottom)
}

#[test]
fn nested_tables_in_parallel_cells_stack_inside_the_document_band() {
	// A table nested in a quote nested in a cell: the inner rows must stack by
	// their own heights, and the row's other cell must still start on the same
	// line as the nested content.
	let inner_row = |text| TableRow {
		cells: vec![cell(text, CellKind::Data, 1, 1)],
	};
	let inner = make(
		BlockKind::Table(Table {
			caption: None,
			columns: vec![TableColumn { align: None }],
			groups: vec![TableGroup {
				kind: TableGroupKind::Body,
				rows: vec![inner_row("Nested top"), inner_row("Nested bottom")],
			}],
		}),
		Vec::new(),
	);
	let quote = make(
		BlockKind::Quote {
			callout: None,
			blocks: vec![inner],
		},
		Vec::new(),
	);
	let cell = |blocks| TableCell {
		kind: CellKind::Data,
		rowspan: 1,
		colspan: 1,
		align: None,
		blocks,
	};
	let table = make(
		BlockKind::Table(Table {
			caption: None,
			columns: vec![TableColumn { align: None }; 2],
			groups: vec![TableGroup {
				kind: TableGroupKind::Body,
				rows: vec![TableRow {
					cells: vec![cell(vec![quote]), cell(vec![para("Sibling")])],
				}],
			}],
		}),
		Vec::new(),
	);
	let document =
		DocumentBuilder::new(DocumentRevision::new(1)).finish(vec![table]);
	let snapshot = LayoutEngine::new().layout(&document, &options());
	let block = &snapshot.blocks[0];
	let node = |needle: &str| {
		block
			.layout
			.text
			.iter()
			.position(|text| text.text == needle)
			.unwrap_or_else(|| panic!("missing {needle}"))
	};
	let top = text_band(&snapshot, node("Nested top"));
	let bottom = text_band(&snapshot, node("Nested bottom"));
	let sibling = text_band(&snapshot, node("Sibling"));
	// The nested rows stack by their own heights inside the quote's padding.
	assert!(
		bottom.0 >= top.1,
		"nested rows overlap: {top:?} then {bottom:?}"
	);
	// The other cell starts at the row origin, not below the nested content.
	assert!(
		sibling.0 <= top.0,
		"the left cell's nested height pushed the right cell down: {sibling:?} vs {top:?}"
	);
	// Every piece stays inside the block's own band.
	let band = block.y..block.y + block.height();
	for (name, (start, end)) in [
		("nested top", top),
		("nested bottom", bottom),
		("sibling", sibling),
	] {
		assert!(
			band.start <= start && end <= band.end,
			"{name} {start}..{end} escapes the block band {band:?}"
		);
	}
}

#[test]
fn rowspan_does_not_merge_rows_in_reading_text() {
	let cell = |text, rowspan| TableCell {
		kind: CellKind::Data,
		rowspan,
		colspan: 1,
		align: None,
		blocks: vec![para(text)],
	};
	let table = make(
		BlockKind::Table(Table {
			caption: None,
			columns: vec![TableColumn { align: None }; 2],
			groups: vec![TableGroup {
				kind: TableGroupKind::Body,
				rows: vec![
					TableRow {
						cells: vec![cell("A", 2), cell("B", 1)],
					},
					TableRow {
						cells: vec![cell("C", 1)],
					},
				],
			}],
		}),
		Vec::new(),
	);
	let document =
		DocumentBuilder::new(DocumentRevision::new(1)).finish(vec![table]);
	let snapshot = LayoutEngine::new().layout(&document, &options());
	assert!(snapshot.full_reading_text().contains("A\tB\nC"));
}

#[test]
fn parallel_cell_content_keeps_the_same_row_origin() {
	let quote = |text| {
		make(
			BlockKind::Quote {
				callout: None,
				blocks: vec![para(text)],
			},
			Vec::new(),
		)
	};
	let cell = |blocks| TableCell {
		kind: CellKind::Data,
		rowspan: 1,
		colspan: 1,
		align: None,
		blocks,
	};
	let table = make(
		BlockKind::Table(Table {
			caption: None,
			columns: vec![TableColumn { align: None }; 2],
			groups: vec![TableGroup {
				kind: TableGroupKind::Body,
				rows: vec![TableRow {
					cells: vec![
						cell(vec![quote("Left")]),
						cell(vec![quote("Right")]),
					],
				}],
			}],
		}),
		Vec::new(),
	);
	let document =
		DocumentBuilder::new(DocumentRevision::new(1)).finish(vec![table]);
	let snapshot = LayoutEngine::new().layout(&document, &options());
	let block = &snapshot.blocks[0];
	let y = |needle: &str| {
		block
			.layout
			.text
			.iter()
			.find(|text| text.text == needle)
			.and_then(|text| text.clusters.first())
			.map(|cluster| cluster.rect.y)
			.unwrap_or_else(|| panic!("missing {needle}"))
	};
	assert!((y("Left") - y("Right")).abs() < 1.0);
}
