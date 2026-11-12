use markview_commonmark::{
	Dialect, MarkdownParse, parse, parse_with, reparse_with,
};
use markview_document::{
	Block, BlockKind, CellAlign, CellKind, Table, TableGroupKind, plain_text,
};
use proptest::prelude::*;

fn table(block: &Block) -> &Table {
	let BlockKind::Table(table) = block.kind() else {
		panic!("expected table: {block:?}");
	};
	table
}

fn paragraph(block: &Block) -> String {
	let BlockKind::Paragraph(text) = block.kind() else {
		panic!("expected paragraph: {block:?}");
	};
	plain_text(text)
}

fn validate(parsed: &MarkdownParse) {
	assert_eq!(
		parsed.map.validate(&parsed.source, &parsed.document),
		Ok(())
	);
}

#[test]
fn grid_tables_default_on_and_toggle_independently_of_presets() {
	let source = "+---+---+\n| a | b |\n+===+===+\n| c | d |\n+---+---+\n";
	assert!(Dialect::default().grid_tables);
	for ofm in [false, true] {
		for grid_tables in [false, true] {
			let parsed = parse_with(
				source,
				Dialect {
					ofm,
					grid_tables,
					..Default::default()
				},
			);
			validate(&parsed);
			assert_eq!(
				matches!(parsed.blocks()[0].kind(), BlockKind::Table(_)),
				grid_tables
			);
			if grid_tables {
				let table = table(&parsed.blocks()[0]);
				assert_eq!(table.groups[0].kind, TableGroupKind::Head);
				assert_eq!(
					table.groups[0].rows[0].cells[0].kind,
					CellKind::Header
				);
				assert_eq!(
					paragraph(&table.groups[1].rows[0].cells[1].blocks[0]),
					"d"
				);
			}
		}
	}
}

#[test]
fn grid_sections_alignment_and_block_content_share_the_table_model() {
	let parsed = parse(concat!(
		"+-----+-----+-----+\n",
		"| a   | b   | c   |\n",
		"+:====+====:+:===:+\n",
		"| d   | - e | f   |\n",
		"|     | - g |     |\n",
		"|     |     | h   |\n",
		"+=====+=====+=====+\n",
		"| i   | j   | k   |\n",
		"+=====+=====+=====+\n",
	));
	validate(&parsed);
	let table = table(&parsed.blocks()[0]);
	assert_eq!(
		table.columns.iter().map(|c| c.align).collect::<Vec<_>>(),
		vec![
			Some(CellAlign::Left),
			Some(CellAlign::Right),
			Some(CellAlign::Center)
		]
	);
	assert_eq!(
		table.groups.iter().map(|g| g.kind).collect::<Vec<_>>(),
		vec![
			TableGroupKind::Head,
			TableGroupKind::Body,
			TableGroupKind::Foot
		]
	);
	let cells = &table.groups[1].rows[0].cells;
	assert!(
		matches!(cells[1].blocks[0].kind(), BlockKind::List { items, .. } if items.len() == 2)
	);
	assert_eq!(cells[2].blocks.len(), 2);
	assert_eq!(paragraph(&cells[2].blocks[1]), "h");
	assert_eq!(table.groups[2].rows[0].cells[0].kind, CellKind::Data);
}

#[test]
fn grid_spanning_cells_keep_their_logical_positions() {
	let parsed = parse(concat!(
		"+---+---+---+\n",
		"| AAAAA | B |\n",
		"+---+---+ B +\n",
		"| D | E | B |\n",
		"+ D +---+---+\n",
		"| D | CCCCC |\n",
		"+---+---+---+\n",
	));
	validate(&parsed);
	let rows = &table(&parsed.blocks()[0]).groups[0].rows;
	assert_eq!(rows.len(), 3);
	assert_eq!(rows[0].cells[0].colspan, 2);
	assert_eq!(rows[0].cells[1].rowspan, 2);
	assert_eq!(rows[1].cells[0].rowspan, 2);
	assert_eq!(rows[2].cells[0].colspan, 2);
	assert_eq!(paragraph(&rows[2].cells[0].blocks[0]), "CCCCC");
}

#[test]
fn nested_grids_and_details_keep_the_extension_choice() {
	for grid_tables in [false, true] {
		let parsed = parse_with(
			"<details open>\n<summary>Grid</summary>\n\n+---------------+\n| +---+         |\n| | a |         |\n| +---+         |\n+---------------+\n\n</details>\n",
			Dialect {
				grid_tables,
				..Default::default()
			},
		);
		validate(&parsed);
		let BlockKind::Details { blocks, .. } = parsed.blocks()[0].kind()
		else {
			panic!("expected details")
		};
		assert_eq!(
			matches!(blocks[0].kind(), BlockKind::Table(_)),
			grid_tables
		);
		if grid_tables {
			let nested =
				&table(&blocks[0]).groups[0].rows[0].cells[0].blocks[0];
			assert_eq!(
				paragraph(&table(nested).groups[0].rows[0].cells[0].blocks[0]),
				"a"
			);
		}
	}
}

#[test]
fn grid_inline_sources_survive_multiline_cells_containers_and_line_endings() {
	for newline in ["\n", "\r\n", "\r"] {
		let source = "> +-------+-------+\n> | **é** | first |\n> | 中    | last  |\n> +-------+-------+\n".replace('\n', newline);
		let parsed = parse(source);
		validate(&parsed);
		let BlockKind::Quote { blocks, .. } = parsed.blocks()[0].kind() else {
			panic!("expected quote")
		};
		for cell in &table(&blocks[0]).groups[0].rows[0].cells {
			let block = &cell.blocks[0];
			let BlockKind::Paragraph(text) = block.kind() else {
				panic!("expected paragraph")
			};
			for inline in text {
				if let markview_document::InlineKind::Text(text) = &inline.kind
					&& !text.trim().is_empty()
				{
					let range = parsed.inline_source(block, inline).unwrap();
					assert_eq!(&parsed.source[range], text);
				}
			}
		}
	}
}

#[test]
fn multiline_grid_sources_do_not_match_text_in_adjacent_cells() {
	let source = "+-------+-------+\n| first | last  |\n| last  | first |\n+-------+-------+\n";
	let parsed = parse(source);
	validate(&parsed);
	let block = &parsed.blocks()[0];
	let field = parsed.map.block(block.id()).unwrap().fields[0].field;
	let target = parsed
		.map
		.semantic_to_source(block.id(), field, 6..10)
		.unwrap();
	let start = source.rfind("last").unwrap();
	assert_eq!(target.source, start..start + 4);
}

proptest! {
	#[test]
	fn grid_edits_match_full_parses_and_preserve_cell_sources(
		values in prop::collection::vec("[a-z]{1,8}", 1..8),
		ofm in any::<bool>(), grid_tables in any::<bool>(),
		newline in prop::sample::select(vec!["\n", "\r\n", "\r"]),
	) {
		let render = |tail| {
			let mut source = String::from("Before.\n\n+----------+\n");
			for value in &values {
				source.push_str(&format!("| {value:8} |\n+----------+\n"));
			}
			source.push_str(tail);
			source.replace('\n', newline)
		};
		let dialect = Dialect { ofm, grid_tables, ..Default::default() };
		let first = parse_with(render("\nAfter.\n"), dialect);
		let changed = render("\nChanged.\n");
		let parsed = reparse_with(&first, changed.clone().into(), dialect);
		let full = parse_with(changed, dialect);
		validate(&parsed);
		prop_assert_eq!(parsed.blocks(), full.blocks());
		if grid_tables {
			let rows = &table(&parsed.blocks()[1]).groups[0].rows;
			prop_assert_eq!(rows.len(), values.len());
			for (row, expected) in rows.iter().zip(values) {
				let block = &row.cells[0].blocks[0];
				prop_assert_eq!(paragraph(block), expected.as_str());
				let range = parsed.block_source(block).unwrap();
				prop_assert_eq!(&parsed.source[range], expected.as_str());
			}
		}
	}
}

fn grid_cell(body: &str) -> String {
	let border = format!("+{}+{}+\n", "-".repeat(42), "-".repeat(12));
	let mut source = border.clone();
	for line in body.lines() {
		source.push_str(&format!("| {line:40} | {:10} |\n", "neighbor"));
	}
	source.push_str(&border);
	source
}

#[test]
fn grid_math_keeps_delimiters_display_mode_and_source_ranges() {
	use markview_document::InlineKind;
	for (body, latex, display) in [
		(r"Inline $x^2+y^2$.", "x^2+y^2", false),
		(r"Inline \(x^2+y^2\).", "x^2+y^2", false),
		(r"Inline $`x^2+y^2`$.", "x^2+y^2", false),
		("$$x^2+y^2$$", "x^2+y^2", true),
		("$$\nx^2+y^2\n$$", "x^2+y^2", true),
		("\\[\nx^2+y^2\n\\]", "x^2+y^2", true),
		("```math\nx^2+y^2\n```", "x^2+y^2", true),
	] {
		for ofm in [false, true] {
			for newline in ["\n", "\r\n", "\r"] {
				let source = grid_cell(body).replace('\n', newline);
				let parsed = parse_with(
					source,
					Dialect {
						ofm,
						..Default::default()
					},
				);
				validate(&parsed);
				let cells = &table(&parsed.blocks()[0]).groups[0].rows[0].cells;
				assert_eq!(cells[0].blocks.len(), 1, "{body:?}");
				let block = &cells[0].blocks[0];
				let BlockKind::Paragraph(text) = block.kind() else {
					panic!("expected math paragraph")
				};
				let math: Vec<_> = text
					.iter()
					.filter(|inline| {
						matches!(inline.kind, InlineKind::Math { .. })
					})
					.collect();
				assert_eq!(math.len(), 1, "{body:?}: {text:?}");
				assert!(
					matches!(&math[0].kind, InlineKind::Math { latex: actual, display: actual_display } if actual.trim() == latex && *actual_display == display),
					"{body:?}: {math:?}"
				);
				let range = parsed.inline_source(block, math[0]).unwrap();
				assert!(parsed.source[range].contains(latex), "{body:?}");
				assert!(!plain_text(text).contains("neighbor"));
			}
		}
	}
}

#[test]
fn grid_footnotes_resolve_global_and_cell_definitions_and_hoist_once() {
	use markview_document::{FieldText, InlineKind, visit_fields};
	let body =
		"Cell[^outer] and [^inner] again[^outer].\n\n[^inner]: Inside $a+b$.";
	for ofm in [false, true] {
		for newline in ["\n", "\r\n", "\r"] {
			let source = format!("Before[^first].\n\n{}\nAfter[^inner].\n\n[^outer]: Outside.\n[^first]: First.\n", grid_cell(body)).replace('\n', newline);
			let parsed = parse_with(
				source,
				Dialect {
					ofm,
					..Default::default()
				},
			);
			validate(&parsed);
			let notes: Vec<_> = parsed
				.blocks()
				.iter()
				.filter_map(|block| match block.kind() {
					BlockKind::Footnote { id, number, blocks } => {
						Some((id, *number, blocks))
					}
					_ => None,
				})
				.collect();
			assert_eq!(notes.len(), 3);
			assert_eq!(
				notes
					.iter()
					.map(|(_, number, _)| *number)
					.collect::<Vec<_>>(),
				[1, 2, 3]
			);
			assert_eq!(paragraph(&notes[2].2[0]), "Inside a+b.");
			let table_block = &parsed.blocks()[1];
			let cell = &table(table_block).groups[0].rows[0].cells[0];
			assert_eq!(
				cell.blocks.len(),
				1,
				"the definition belongs at the document's end"
			);
			let mut references = Vec::new();
			visit_fields(
				table_block,
				&mut Vec::new(),
				&|| false,
				&mut |_, _, field, _| {
					if let FieldText::Rich(text) = field {
						for inline in text {
							if let InlineKind::FootnoteRef(reference) =
								&inline.kind
							{
								references.push(reference.clone());
								let range = parsed
									.inline_source(table_block, inline)
									.unwrap();
								assert!(matches!(
									&parsed.source[range],
									"[^outer]" | "[^inner]"
								));
							}
						}
					}
				},
			);
			assert_eq!(
				references.iter().map(|r| r.number).collect::<Vec<_>>(),
				[2, 3, 2]
			);
			assert_eq!(references[0].definition, references[2].definition);
			assert_ne!(references[0].occurrence, references[2].occurrence);
			assert_eq!(&references[1].definition, notes[2].0);
		}
	}
}

proptest! {
	#[test]
	fn grid_math_and_footnote_cell_edits_match_full_parses(
		n in 1u16..1000, changed in 1u16..1000, ofm in any::<bool>(),
		newline in prop::sample::select(vec!["\n", "\r\n", "\r"]),
	) {
		let render = |n| format!("{}\n[^note]: Value {n}.\n", grid_cell(&format!("Cell[^note] with $x+{n}$.\n\n$$\nx^{n}\n$$"))).replace('\n', newline);
		let dialect = Dialect { ofm, ..Default::default() };
		let previous = parse_with(render(n), dialect);
		let source = render(changed);
		let reparsed = reparse_with(&previous, source.clone().into(), dialect);
		let full = parse_with(source, dialect);
		validate(&reparsed);
		prop_assert_eq!(reparsed.blocks(), full.blocks());
		prop_assert_eq!(reparsed.map.blocks(), full.map.blocks());
	}
}
