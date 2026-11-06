//! Complex webpage structures consumed by layout, reading and pagination.
use crate::{coverage, oracle, page_oracle, pipeline, webgen, webpage};
use markview_document::{
	Block, BlockKind, CellKind, Document, DocumentBuilder, DocumentRevision,
	Inline, InlineKind, Table, TableCell, TableColumn, TableGroup,
	TableGroupKind, TableRow,
};
use markview_layout::{
	layout::{LayoutOptions, LayoutSnapshot},
	paginate::{PageGeometry, paginate},
	scene::Rect,
};

/// Marker comparison is independent of whitespace, line wrapping and fonts.
/// Tokens are delimited to distinguish `mk1` from `mk10`.
pub fn markers(text: &str) -> Vec<&str> {
	text.split(|c: char| !c.is_ascii_alphanumeric())
		.filter(|word| {
			word.strip_prefix("mk").is_some_and(|n| {
				!n.is_empty() && n.bytes().all(|c| c.is_ascii_digit())
			})
		})
		.collect()
}

fn rich(value: String) -> Vec<Inline> {
	vec![Inline {
		kind: InlineKind::Text(value),
		style: Default::default(),
		link: None,
	}]
}

/// A legal two-by-two table nested inside a cell.
fn nested_table(
	builder: &mut DocumentBuilder,
	expected: &mut Vec<String>,
	next: &mut usize,
) -> Block {
	let mut rows = Vec::new();
	for _ in 0..2 {
		let mut cells = Vec::new();
		for _ in 0..2 {
			*next += 1;
			let marker = format!("mk{next}");
			expected.push(marker.clone());
			let block = builder.block(
				vec![],
				BlockKind::Paragraph(rich(format!(
					"{marker} nested table cell"
				))),
			);
			cells.push(TableCell {
				kind: CellKind::Data,
				rowspan: 1,
				colspan: 1,
				align: None,
				blocks: vec![block],
			});
		}
		rows.push(TableRow { cells });
	}
	builder.block(
		vec![],
		BlockKind::Table(Table {
			caption: None,
			columns: vec![TableColumn { align: None }; 2],
			groups: vec![TableGroup {
				kind: TableGroupKind::Body,
				rows,
			}],
		}),
	)
}

/// A legal grid with one two-row cell, one two-column header, nested tables
/// and figures, and independently folded bodies. This bypasses extraction so
/// all retained-flow paths survive.
pub fn grid(data: &[u8]) -> (Document, Vec<String>, bool) {
	let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
	let mut expected = Vec::new();
	let mut next = 0;
	let mut content = |builder: &mut DocumentBuilder, folded: bool| {
		next += 1;
		let marker = format!("mk{next}");
		expected.push(marker.clone());
		if folded {
			next += 1;
			let body_marker = format!("mk{next}");
			expected.push(body_marker.clone());
			let body = builder.block(
				vec![],
				BlockKind::Paragraph(rich(format!(
					"{body_marker} nested body of the cell with several words"
				))),
			);
			next += 1;
			let figure_marker = format!("mk{next}");
			expected.push(figure_marker.clone());
			let figure_body = builder.block(
				vec![],
				BlockKind::Paragraph(rich(format!(
					"{figure_marker} figure body"
				))),
			);
			next += 1;
			let caption_marker = format!("mk{next}");
			expected.push(caption_marker.clone());
			let figure = builder.block(
				vec![],
				BlockKind::Figure {
					blocks: vec![figure_body],
					caption: Some(rich(format!(
						"{caption_marker} figure caption"
					))),
				},
			);
			let table = nested_table(builder, &mut expected, &mut next);
			builder.block(
				vec![],
				BlockKind::Details {
					default_open: false,
					summary: rich(marker),
					blocks: vec![body, figure, table],
				},
			)
		} else {
			builder.block(
				vec![],
				BlockKind::Paragraph(rich(format!("{marker} ordinary cell"))),
			)
		}
	};
	let mut cell =
		|builder: &mut DocumentBuilder, rowspan, colspan, folded| TableCell {
			kind: CellKind::Data,
			rowspan,
			colspan,
			align: None,
			blocks: vec![content(builder, folded)],
		};
	let header = TableGroup {
		kind: TableGroupKind::Head,
		rows: vec![TableRow {
			cells: vec![cell(&mut builder, 1, 2, false)],
		}],
	};
	let mut rows = vec![
		TableRow {
			cells: vec![
				cell(&mut builder, 2, 1, true),
				cell(&mut builder, 1, 1, false),
			],
		},
		TableRow {
			cells: vec![cell(&mut builder, 1, 1, true)],
		},
	];
	for byte in data.iter().take(24) {
		rows.push(TableRow {
			cells: vec![
				cell(&mut builder, 1, 1, byte % 2 == 0),
				cell(&mut builder, 1, 1, byte % 3 == 0),
			],
		});
	}
	let table = builder.block(
		vec!["grid".into()],
		BlockKind::Table(Table {
			caption: None,
			columns: vec![TableColumn { align: None }; 2],
			groups: vec![
				header,
				TableGroup {
					kind: TableGroupKind::Body,
					rows,
				},
			],
		}),
	);
	(builder.finish(vec![table]), expected, false)
}

/// A plain short-LTR grid whose cell text must stay inside its own column.
pub fn plain_grid(data: &[u8]) -> (Document, Vec<String>, bool) {
	let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
	let mut expected = Vec::new();
	let mut rows = Vec::new();
	for row in 0..2 + data.len().min(4) {
		let mut cells = Vec::new();
		for column in 0..3 {
			let marker = format!("mk{}", row * 3 + column + 1);
			expected.push(marker.clone());
			let block = builder.block(
				vec![],
				BlockKind::Paragraph(rich(format!("{marker} left"))),
			);
			cells.push(TableCell {
				kind: CellKind::Data,
				rowspan: 1,
				colspan: 1,
				align: None,
				blocks: vec![block],
			});
		}
		rows.push(TableRow { cells });
	}
	let table = builder.block(
		vec![],
		BlockKind::Table(Table {
			caption: None,
			columns: vec![TableColumn { align: None }; 3],
			groups: vec![TableGroup {
				kind: TableGroupKind::Body,
				rows,
			}],
		}),
	);
	(builder.finish(vec![table]), expected, true)
}

/// The bounding rectangle of one marker's text clusters.
fn marker_rect(snapshot: &LayoutSnapshot, marker: &str) -> Option<Rect> {
	for block in &snapshot.blocks {
		for node in &block.layout.text {
			let mut from = 0;
			while let Some(found) = node.text[from..].find(marker) {
				let start = from + found;
				let end = start + marker.len();
				let boundary = |at: &str| {
					at.chars().next().is_none_or(|c| !c.is_alphanumeric())
				};
				if boundary(&node.text[..start]) && boundary(&node.text[end..])
				{
					let mut rect: Option<Rect> = None;
					for cluster in &node.clusters {
						if cluster.range.start < end
							&& cluster.range.end > start
						{
							rect = Some(match rect {
								Some(old) => Rect {
									x: old.x.min(cluster.rect.x),
									y: old.y.min(cluster.rect.y),
									w: (old.x + old.w)
										.max(cluster.rect.x + cluster.rect.w)
										- old.x.min(cluster.rect.x),
									h: (old.y + old.h)
										.max(cluster.rect.y + cluster.rect.h)
										- old.y.min(cluster.rect.y),
								},
								None => cluster.rect,
							});
						}
					}
					if rect.is_some() {
						return rect;
					}
				}
				from = end;
			}
		}
	}
	None
}

/// Short LTR cells in the same grid keep their painted text disjoint.
fn assert_cell_geometry(snapshot: &LayoutSnapshot, expected: &[String]) {
	let rects: Vec<_> = expected
		.iter()
		.filter_map(|marker| {
			marker_rect(snapshot, marker).map(|rect| (marker, rect))
		})
		.collect();
	for (index, (a, ra)) in rects.iter().enumerate() {
		for (b, rb) in &rects[index + 1..] {
			let overlap_x =
				ra.x < rb.x + rb.w - 0.5 && rb.x < ra.x + ra.w - 0.5;
			let overlap_y =
				ra.y < rb.y + rb.h - 0.5 && rb.y < ra.y + ra.h - 0.5;
			assert!(
				!overlap_x || !overlap_y,
				"cell text {a:?} at {ra:?} overlaps {b:?} at {rb:?}"
			);
		}
	}
}

/// Modes: generated article, direct grid, plain direct grid, or raw HTML. The
/// remaining bytes independently control structure; widths and fold states
/// have fixed bounds.
pub fn check(data: &[u8]) {
	if data.len() > 8192 {
		return;
	}
	let mode = data.first().copied().unwrap_or(0) % 4;
	let body = data.get(1..).unwrap_or_default();
	let (document, expected, plain) = match mode {
		0 => {
			let generated = webgen::generate(body);
			if generated.html.len() > 128 * 1024 {
				return;
			}
			let article = markview_webpage::extract(
				&generated.html,
				webpage::base_for(body),
			)
			.expect("generated article");
			webgen::assert_generated(&article, &generated);
			coverage::hit(coverage::Hit::WebGenerated);
			(article.document, Some(generated.markers), false)
		}
		1 => {
			let (document, expected, _) = grid(body);
			coverage::hit(coverage::Hit::WebDirectGrid);
			(document, Some(expected), false)
		}
		2 => {
			let (document, expected, plain) = plain_grid(body);
			coverage::hit(coverage::Hit::WebCellGeometry);
			(document, Some(expected), plain)
		}
		_ => {
			let Ok(article) = markview_webpage::extract(
				&String::from_utf8_lossy(body),
				webpage::base_for(body),
			) else {
				return;
			};
			webpage::assert_article(&article);
			(article.document, None, false)
		}
	};
	let mut engine = pipeline::differential_engine();
	for (step, width) in [760., 80., 180., 760.].into_iter().enumerate() {
		let options = LayoutOptions {
			width,
			force_open: step % 2 == 0,
			fonts: pipeline::pinned_fonts(),
			..Default::default()
		};
		let warm = engine.layout(&document, &options);
		let cold = pipeline::differential_engine().layout(&document, &options);

		assert_eq!(
			oracle::layout(&warm),
			oracle::layout(&cold),
			"mode={mode} step={step}"
		);
		assert_eq!(warm.full_reading_text(), cold.full_reading_text());
		oracle::assert_snapshot_finite(&warm);
		if options.force_open
			&& let Some(expected) = &expected
		{
			let reading = warm
				.blocks
				.iter()
				.flat_map(|block| &block.layout.text)
				.filter(|node| node.search_field.is_some())
				.map(|node| node.text.as_str())
				.collect::<Vec<_>>()
				.join(" ");
			let actual = markers(&reading);
			if mode == 1 || mode == 2 {
				assert_eq!(
					actual,
					expected.iter().map(String::as_str).collect::<Vec<_>>(),
					"cell marker order/ownership: step={step}"
				);
			} else {
				// Captions follow cells in the reading projection, whereas
				// HTML places a table's caption before its cells.
				let mut actual = actual;
				actual.sort_unstable();
				let mut expected: Vec<_> =
					expected.iter().map(String::as_str).collect();
				expected.sort_unstable();
				assert_eq!(
					actual, expected,
					"lost/duplicated marker: step={step}"
				);
			}
			if plain && width >= 480. {
				assert_cell_geometry(&warm, expected);
			}
		}
		if !plain && !warm.blocks.is_empty() {
			coverage::hit(coverage::Hit::WebSpanLayout);
		}
		let geometry = PageGeometry {
			width_pt: 300.,
			height_pt: 120.,
			margin_pt: [10.; 4],
		};
		let warm = warm.flattened();
		let cold = cold.flattened();
		let a = paginate(&document, &warm, &geometry);
		let b = paginate(&document, &cold, &geometry);
		assert_eq!(a.pages, b.pages, "page fragments: mode={mode} step={step}");
		assert_eq!(
			a.anchors, b.anchors,
			"anchor ownership: mode={mode} step={step}"
		);
		page_oracle::assert_pagination(&a);
		if a.pages.len() > 1 {
			coverage::hit(coverage::Hit::WebMultipage);
		}
		if warm.blocks.iter().any(|block| {
			block.layout.text.iter().any(|node| !node.text.is_empty())
		}) {
			coverage::hit(coverage::Hit::WebReadingProjection);
		}
		if warm
			.blocks
			.iter()
			.any(|block| !block.layout.overflow.is_empty())
		{
			coverage::hit(coverage::Hit::WebOverflow);
		}
	}
}
