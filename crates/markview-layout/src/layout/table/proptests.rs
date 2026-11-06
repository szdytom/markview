use super::*;
use crate::{
	document::{TableColumn, TableGroup, TableGroupKind, TableRow},
	layout::LayoutEngine,
};
use proptest::prelude::*;

proptest! {
	#[test]
	fn mixed_grid_cells_occupy_their_declared_rectangles_and_stop_at_groups(
		width in 1usize..7,
		groups in prop::collection::vec(prop::collection::vec((1usize..4, prop::collection::vec(any::<u8>(), 0..6)), 1..3), 1..4),
		normalize in any::<bool>(),
	) {
		// Each stripe partitions columns independently, producing staggered rectangles.
		let mut table = Table { caption: None, columns: vec![TableColumn { align: None }; width], groups: Vec::new() };
		let mut expected = Vec::new();
		let mut global_row = 0;
		for (group, stripes) in groups.iter().enumerate() {
			let height: usize = stripes.iter().map(|stripe| stripe.0).sum();
			let mut rows: Vec<_> = (0..height).map(|_| TableRow { cells: Vec::new() }).collect();
			let mut row = 0;
			for (h, cuts) in stripes {
				let mut edges = vec![0, width];
				edges.extend(cuts.iter().map(|&cut| usize::from(cut) % (width + 1)));
				edges.sort_unstable(); edges.dedup();
				for (cell, pair) in edges.windows(2).enumerate() {
					let w = pair[1] - pair[0];
					let raw_h = if normalize && row + h == height { h + 10 } else { *h };
					rows[row].cells.push(TableCell {
						kind: CellKind::Data, rowspan: raw_h as u32,
						colspan: if normalize && w == 1 { 0 } else { w as u32 },
						align: None, blocks: Vec::new(),
					});
					expected.push((group, row, cell, global_row + row, pair[0], *h, w));
				}
				row += h;
			}
			table.groups.push(TableGroup { kind: TableGroupKind::Body, rows });
			global_row += height;
		}
		let mut engine = LayoutEngine::new();
		let images = Default::default();
		let context = BlockContext {
			nested: None, cancelled: &|| false, fields: Default::default(),
			shaper: &mut engine.shaper, math: &mut engine.math, images: &images,
			highlight_cache: engine.highlights.results(), marker_depth: 0, enum_depth: 0, footnote_column: 1,
		};
		let opts = LayoutOptions::default();
		let (placed, rows) = context.build_grid(&table, width, &opts, &TextAppearance::default(), false);
		prop_assert_eq!(rows, global_row);
		let actual: Vec<_> = placed.iter().map(|cell| (cell.group, cell.row, cell.cell, cell.row_start, cell.col, cell.rowspan, cell.colspan)).collect();
		prop_assert_eq!(&actual, &expected);
		let mut occupancy = vec![vec![0; width]; rows];
		for cell in &placed {
			for row in &mut occupancy[cell.row_start..cell.row_start + cell.rowspan] {
				for count in &mut row[cell.col..cell.col + cell.colspan] { *count += 1; }
			}
		}
		prop_assert!(occupancy.iter().flatten().all(|&count| count == 1));
		// Consumer limits truncate input, whereas HTML lowering rejects the table.
		let mut limited = opts;
		limited.limits.table_cells = 1;
		let (placed, _) = context.build_grid(&table, width, &limited, &TextAppearance::default(), false);
		prop_assert_eq!(placed.len(), 1);
		prop_assert_eq!((placed[0].group, placed[0].row, placed[0].cell), (0, 0, 0));
		table.groups[0].rows[0].cells[0].rowspan = 0;
		table.groups[0].rows[0].cells[0].colspan = u32::MAX;
		limited.limits.table_rows = 1;
		let (placed, rows) = context.build_grid(&table, width, &limited, &TextAppearance::default(), false);
		prop_assert_eq!(rows, 1);
		prop_assert_eq!((placed[0].rowspan, placed[0].colspan), (1, width));
	}
}
