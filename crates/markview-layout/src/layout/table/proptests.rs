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

proptest! {
	#[test]
	fn segmented_corner_rectangles_cover_the_border_union_exactly_once(
		width in 2usize..21, height in 2usize..21,
		top_start in any::<u8>(), bottom_end in any::<u8>(),
		border_widths in prop::array::uniform4(1u8..31),
	) {
		let rect = Rect { x: 0., y: 0., w: width as f32, h: height as f32 };
		let top_start = usize::from(top_start) % width;
		let bottom_end = 1 + usize::from(bottom_end) % width;
		let raw = border_widths.map(f32::from);
		let widths = crate::scene::fit_edges(rect, raw);
		let runs = vec![
			BorderRun { side: 0, start: top_start, end: width, width: raw[0], cut: None },
			BorderRun { side: 1, start: 0, end: height, width: raw[1], cut: None },
			BorderRun { side: 2, start: 0, end: bottom_end, width: raw[2], cut: None },
			BorderRun { side: 3, start: 0, end: height, width: raw[3], cut: None },
		];
		let xs: Vec<_> = (0..=width).map(|x| x as f32).collect();
		let ys: Vec<_> = (0..=height).map(|y| y as f32).collect();
		let painted = borders::rectangles(&runs, rect, &xs, &ys);
		for y in 0..height {
			for x in 0..width {
				let (x, y) = (x as f32 + 0.5, y as f32 + 0.5);
				let expected = x >= top_start as f32 && y < widths[0]
					|| x < bottom_end as f32 && y >= rect.h - widths[2]
					|| x < widths[3] || x >= rect.w - widths[1];
				let count = painted.iter().filter(|r| r.x <= x && x < r.x + r.w && r.y <= y && y < r.y + r.h).count();
				prop_assert_eq!(count, usize::from(expected), "point ({}, {})", x, y);
			}
		}
	}
}

proptest! {
	#[test]
	fn collapsed_segments_have_exactly_one_winner_and_preserve_exposed_edges(
		columns in 1usize..9,
		rows in 1usize..9,
		seeds in prop::collection::vec((any::<u8>(), any::<u8>(), any::<u8>(), prop::array::uniform4(0u8..5), 0u8..8, 0u8..8), 64),
	) {
		let mut grid = vec![vec![None; columns]; rows];
		let mut placed = Vec::new();
		let mut widths = Vec::new();
		let mut before = Vec::new();
		let mut after = Vec::new();
		for row in 0..rows {
			for col in 0..columns {
				let (w, h, hole, edges, gap_before, gap_after) = seeds[row * columns + col];
				if grid[row][col].is_some() || hole % 4 == 0 { continue; }
				let free = grid[row][col..].iter().take_while(|slot| slot.is_none()).count();
				let colspan = 1 + usize::from(w) % free;
				let mut rowspan = 1 + usize::from(h) % (rows - row);
				while grid[row..row + rowspan].iter().any(|r| r[col..col + colspan].iter().any(Option::is_some)) {
					rowspan -= 1;
				}
				let id = placed.len();
				for r in &mut grid[row..row + rowspan] { r[col..col + colspan].fill(Some(id)); }
				placed.push(PlacedCell {
					group: 0, row, cell: id, row_start: row, col, rowspan, colspan,
					draw: 0, min_width: 0., preferred_width: 0., decoration: None, border_runs: Vec::new(),
				});
				widths.push(edges.map(f32::from));
				before.push(f32::from(gap_before));
				after.push(f32::from(gap_after));
			}
		}
		let resolved: Vec<_> = placed.iter().enumerate().map(|(id, _)| {
			collapse_borders(&placed, &grid, &widths, &before, &after, id)
		}).collect();
		let present = |id: usize, y: f32| {
			let cell = &placed[id];
			y >= cell.row_start as f32 * 27.5 + before[id]
				&& y < (cell.row_start + cell.rowspan) as f32 * 27.5 - after[id]
		};
		let at = |id: usize, side: usize, index: usize, y: Option<f32>| {
			let (edges, runs) = &resolved[id];
			if y.is_some_and(|y| !present(id, y)) { return 0.; }
			edges[side] + runs.iter().filter(|run| run.side == side && (run.start..run.end).contains(&index))
				.filter(|run| !run.cut.as_ref().is_some_and(|cut| y.is_some_and(|y| y >= cut.top as f32 * 27.5 + cut.before && y < cut.bottom as f32 * 27.5 - cut.after)))
				.map(|run| run.width).sum::<f32>()
		};
		// The oracle examines actual overlap at each boundary, including spacing gaps.
		for horizontal in [true, false] {
			let (across, along) = if horizontal { (rows, columns) } else { (columns, rows) };
			let (first_side, second_side) = if horizontal { (2, 0) } else { (1, 3) };
			let get = |a: usize, b: usize| if horizontal { grid[a][b] } else { grid[b][a] };
			for boundary in 0..=across {
				for index in 0..along {
					let first = boundary.checked_sub(1).and_then(|a| get(a, index));
					let second = (boundary < across).then(|| get(boundary, index)).flatten();
					if first == second { continue; }
					let samples = if horizontal { vec![None] } else { [0.5, 13.75, 27.].map(|dy| Some(index as f32 * 27.5 + dy)).to_vec() };
					for y in samples {
						let width_at = |id: Option<usize>, side| id.filter(|&id| y.is_none_or(|y| present(id, y))).map_or(0., |id| widths[id][side]);
						let a = width_at(first, first_side);
						let b = width_at(second, second_side);
						let separated = horizontal && (first.is_some_and(|id| after[id] > 0.) || second.is_some_and(|id| before[id] > 0.));
						let expected = if separated { (a, b) } else if a >= b { (a, 0.) } else { (0., b) };
						let actual = (first.map_or(0., |id| at(id, first_side, index, y)), second.map_or(0., |id| at(id, second_side, index, y)));
						prop_assert_eq!(actual, expected, "horizontal={}, boundary={}, index={}, y={:?}", horizontal, boundary, index, y);
					}
				}
			}
		}
		// Every fallback run must also survive conversion into disjoint paint rectangles.
		let xs: Vec<_> = (0..=columns).map(|i| 7. + i as f32 * 31.25).collect();
		let ys: Vec<_> = (0..=rows).map(|i| 11. + i as f32 * 27.5).collect();
		for (id, cell) in placed.iter().enumerate() {
			let runs = &resolved[id].1;
			let rect = Rect { x: xs[cell.col], y: ys[cell.row_start] + before[id], w: xs[cell.col + cell.colspan] - xs[cell.col], h: ys[cell.row_start + cell.rowspan] - ys[cell.row_start] - before[id] - after[id] };
			let painted = borders::rectangles(runs, rect, &xs, &ys);
			for (i, a) in painted.iter().enumerate() {
				prop_assert!(a.x >= rect.x && a.y >= rect.y && a.x + a.w <= rect.x + rect.w && a.y + a.h <= rect.y + rect.h);
				for b in &painted[i + 1..] {
					prop_assert!((a.x + a.w).min(b.x + b.w) <= a.x.max(b.x) || (a.y + a.h).min(b.y + b.h) <= a.y.max(b.y));
				}
			}
			for run in runs {
				for index in run.start..run.end {
					let horizontal = run.side % 2 == 0;
					let offsets = if horizontal { vec![13.75] } else { vec![0.5, 13.75, 27.] };
					for offset in offsets {
						let coordinate = if horizontal { (xs[index] + xs[index + 1]) / 2. } else { ys[index] + offset };
						let (x, y) = match run.side {
							0 => (coordinate, rect.y + run.width / 2.),
							1 => (rect.x + rect.w - run.width / 2., coordinate),
							2 => (coordinate, rect.y + rect.h - run.width / 2.),
							3 => (rect.x + run.width / 2., coordinate),
							_ => unreachable!(),
						};
						let column = ((x - 7.) / 31.25).floor() as usize;
						let in_box = rect.x <= x && x < rect.x + rect.w && rect.y <= y && y < rect.y + rect.h;
						let horizontal_edge = in_box && (y < rect.y + at(id, 0, column, None) || y >= rect.y + rect.h - at(id, 2, column, None));
						let expected = usize::from(horizontal || horizontal_edge || at(id, run.side, index, Some(coordinate - 11.)) > 0.);
						prop_assert_eq!(painted.iter().filter(|r| r.x <= x && x < r.x + r.w && r.y <= y && y < r.y + r.h).count(), expected);
					}
				}
			}
		}
	}
}
