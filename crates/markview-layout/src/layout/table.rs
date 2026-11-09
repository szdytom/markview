//! Complex table geometry: spans, row groups, widths and shared borders.
#[cfg(test)]
mod proptests;

use super::{BlockContext, LayoutOptions};
use crate::{
	document::{BlockKind, CellAlign, CellKind, Table, TableCell},
	scene::{
		BlockLayout, BoxDecoration, Draw, HeadingAnchor, Overflow,
		PageConstraint, Rect,
	},
	style::{BorderCollapse, Condition, Rule, TextAppearance},
};

/// One cell placed in the grid, with the span it actually occupies.
struct PlacedCell {
	group: usize,
	row: usize,
	cell: usize,
	rowspan: usize,
	colspan: usize,
	/// Global row of the cell's first row.
	row_start: usize,
	col: usize,
	draw: usize,
	min_width: f32,
	preferred_width: f32,
	gap_before: bool,
	gap_after: bool,
	decoration: Option<std::sync::Arc<BoxDecoration>>,
}

impl BlockContext<'_> {
	/// Lays out a caption for a table whose grid has no laid-out rows.
	fn table_caption(
		&mut self,
		owner: markview_document::BlockId,
		table: &Table,
		position: [f32; 2],
		width: f32,
		opts: &LayoutOptions,
		out: &mut BlockLayout,
	) -> f32 {
		let Some(caption) = &table.caption else {
			return 0.;
		};
		let table_appearance = self.shaper.appearance.clone();
		self.shaper.appearance =
			opts.stylesheet.text(&table_appearance, Condition::Caption);
		let size = opts.font_size * self.shaper.appearance.size;
		let gap = size * 0.6;
		let [x, y] = position;
		let height = self.rich(
			Some(owner),
			caption,
			x,
			y + gap,
			width,
			size,
			false,
			CellAlign::Center,
			false,
			false,
			opts,
			out,
		);
		self.shaper.appearance = table_appearance.clone();
		gap + height
	}

	#[cfg_attr(
		any(),
		markview::allow_complexity(
			reason = "Keep grid construction, span constraints, two-dimensional layout and shared borders in one ordered table pass; each phase consumes the grid the previous one produced."
		)
	)]
	pub(super) fn table(
		&mut self,
		owner: markview_document::BlockId,
		table: &Table,
		position: [f32; 2],
		width: f32,
		opts: &LayoutOptions,
		out: &mut BlockLayout,
	) -> f32 {
		let [x, y] = position;
		let table_appearance = self.shaper.appearance.clone();
		let collapse = opts
			.stylesheet
			.element_rule(table_appearance.chain, Condition::Table)
			.border_collapse
			== Some(BorderCollapse::Collapse);
		let columns = table.columns.len().min(opts.limits.table_columns);
		if columns == 0 {
			return self.table_caption(owner, table, [x, y], width, opts, out);
		}
		let (mut placed, rows) =
			self.build_grid(table, columns, opts, &table_appearance, collapse);
		if placed.is_empty() {
			return self.table_caption(owner, table, [x, y], width, opts, out);
		}
		let mut minima = vec![0_f32; columns];
		let mut preferred = vec![0_f32; columns];
		for cell in &mut placed {
			let (min, pref) =
				self.cell_intrinsic(table, cell, opts, &table_appearance);
			cell.min_width = min;
			cell.preferred_width = pref;
			if cell.colspan == 1 {
				minima[cell.col] = minima[cell.col].max(min);
				preferred[cell.col] = preferred[cell.col].max(pref);
			}
		}
		for i in 0..columns {
			preferred[i] = preferred[i].max(minima[i]);
		}
		// Multi-column cells constrain the sum of their columns. Process the
		// narrowest spans first so a later wide span sees settled columns.
		let mut order: Vec<usize> = (0..placed.len()).collect();
		order.sort_by_key(|&i| placed[i].colspan);
		for &index in &order {
			let cell = &placed[index];
			if cell.colspan > 1 {
				let range = cell.col..cell.col + cell.colspan;
				grow(&mut minima, range.clone(), cell.min_width);
				for i in range.clone() {
					preferred[i] = preferred[i].max(minima[i]);
				}
				grow(&mut preferred, range, cell.preferred_width);
			}
		}
		for i in 0..columns {
			preferred[i] = preferred[i].max(minima[i]);
		}
		let min: f32 = minima.iter().sum();
		let preferred_total: f32 = preferred.iter().sum();
		let total = width.max(min);
		let widths: Vec<f32> = (0..columns)
			.map(|i| {
				minima[i]
					+ if preferred_total > min {
						(total - min) * (preferred[i] - minima[i]).max(0.)
							/ (preferred_total - min)
					} else {
						(total - min) / columns as f32
					}
			})
			.collect();
		let starts: Vec<f32> = (0..columns)
			.map(|i| x + widths[..i].iter().sum::<f32>())
			.collect();
		let mut row_heights = vec![0_f32; rows];
		let mut row_min = vec![0_f32; rows];
		let mut row_tops = vec![0_f32; rows];
		let mut row_cells = vec![Vec::new(); rows];
		for (index, cell) in placed.iter().enumerate() {
			row_cells[cell.row_start].push(index);
		}
		let mut grid_height = 0.;
		let start_draws = out.draws.len();
		let overflow_start = out.overflow.len();
		for row in 0..rows {
			let top = y + grid_height;
			self.shaper.appearance = table_appearance.clone();
			let mut base = 0_f32;
			let mut spans: Vec<(usize, f32)> = Vec::new();
			let mut emitted = false;
			for &index in &row_cells[row] {
				let cell = &placed[index];
				self.shaper.appearance =
					cell_appearance(&table_appearance, table, cell, opts);
				let rule =
					cell_rule(&self.shaper.appearance.chain, opts, cell, table);
				let pad = super::blocks::block_padding(&rule, opts.font_size);
				let before = rule.space_before.unwrap_or(0.) * opts.font_size;
				let after = rule.space_after.unwrap_or(0.) * opts.font_size;
				let size = opts.font_size * self.shaper.appearance.size;
				let cell_x = starts[cell.col];
				let cell_w: f32 =
					widths[cell.col..cell.col + cell.colspan].iter().sum();
				let content_x = cell_x + pad[3];
				let content_y = top + before + pad[0];
				let inner = (cell_w - pad[1] - pad[3]).max(1.);
				let first_text = out.text.len();
				let align = cell_data(table, cell).align.unwrap_or_else(|| {
					table.columns[cell.col].align.unwrap_or(CellAlign::Left)
				});
				let draw = out.draws.len();
				out.draws.push(Draw::Box {
					rect: Rect::default(),
					chain: self.shaper.appearance.chain,
					condition: if cell_data(table, cell).kind
						== CellKind::Header
					{
						Condition::Header
					} else {
						Condition::Cell
					},
					radius: rule.radius.unwrap_or(0.),
					border: rule.border_width.unwrap_or(0.),
					left_only: false,
					decoration: None,
				});
				let framed =
					cell_data(table, cell).blocks.iter().any(|block| {
						!matches!(block.kind(), BlockKind::Paragraph(_))
					});
				if framed {
					out.begin_parallel_scene();
				}
				let h = self.cell_children(
					cell_data(table, cell),
					content_x,
					content_y,
					inner,
					size,
					align,
					opts,
					out,
				);
				if framed {
					out.end_scene(h, f32::NEG_INFINITY);
				}
				if let Some(node) = out.text.get_mut(first_text) {
					node.separator = if emitted {
						"\t"
					} else if row > 0 {
						"\n"
					} else {
						"\n\n"
					};
				}
				emitted = true;
				let required = h + pad[0] + pad[2] + before + after;
				let entry = &mut placed[index];
				entry.draw = draw;
				if entry.rowspan == 1 {
					base = base.max(required);
				} else {
					spans.push((entry.rowspan, required));
				}
			}
			// Distribute each spanning cell's height over the rows it covers.
			spans.sort_by(|a, b| b.1.total_cmp(&a.1));
			for (span, needed) in spans {
				let future: f32 =
					row_min[row + 1..(row + span).min(rows)].iter().sum();
				let deficit = (needed - base - future).max(0.);
				if deficit > 0. {
					let each = (deficit / span as f32).ceil();
					base += each;
					for slot in row_min
						.iter_mut()
						.take((row + span).min(rows))
						.skip(row + 1)
					{
						*slot += each;
					}
				}
			}
			let height = base.max(row_min[row]);
			row_heights[row] = height;
			row_tops[row] = top;
			grid_height += height;
		}
		// Fill the cell boxes now that every row height is final.
		for cell in &placed {
			let cell_top = row_tops[cell.row_start];
			let cell_height: f32 = row_heights
				[cell.row_start..cell.row_start + cell.rowspan]
				.iter()
				.sum();
			let cell_x = starts[cell.col];
			let cell_w = widths[cell.col..cell.col + cell.colspan].iter().sum();
			let condition = if cell_data(table, cell).kind == CellKind::Header {
				Condition::Header
			} else {
				Condition::Cell
			};
			let appearance =
				cell_appearance(&table_appearance, table, cell, opts);
			let rule = cell_rule(&appearance.chain, opts, cell, table);
			let before = rule.space_before.unwrap_or(0.) * opts.font_size;
			let after = rule.space_after.unwrap_or(0.) * opts.font_size;
			*out.draws.get_mut(cell.draw).unwrap() = Draw::Box {
				rect: Rect {
					x: cell_x,
					y: cell_top + before,
					w: cell_w,
					h: (cell_height - before - after).max(0.),
				},
				chain: appearance.chain,
				condition,
				radius: rule.radius.unwrap_or(0.),
				border: rule.border_width.unwrap_or(0.),
				left_only: false,
				decoration: cell.decoration.clone(),
			};
			if cell.rowspan > 1 {
				out.page_constraints.push(PageConstraint {
					top: cell_top,
					bottom: cell_top + cell_height,
					orphans: None,
					widows: None,
					keep_together: true,
				});
			}
		}
		let mut height = grid_height;
		let overflow_end = out.draws.len();
		if let Some(caption) = &table.caption {
			self.shaper.appearance =
				opts.stylesheet.text(&table_appearance, Condition::Caption);
			let size = opts.font_size * self.shaper.appearance.size;
			let gap = size * 0.6;
			let h = self.rich(
				Some(owner),
				caption,
				x,
				y + grid_height + gap,
				width,
				size,
				false,
				CellAlign::Center,
				false,
				false,
				opts,
				out,
			);
			height = grid_height + gap + h;
		}
		if total > width + 0.5 {
			let gutter = opts.stylesheet.scrollbar_gutter();
			out.truncate_overflow(overflow_start);
			out.overflow.push(Overflow {
				rect: Rect {
					x,
					y,
					w: width,
					h: grid_height,
				},
				content_width: total,
				commands: start_draws..overflow_end,
				gutter,
			});
			height += gutter;
		}
		self.shaper.appearance = table_appearance;
		height
	}

	#[expect(
		clippy::too_many_arguments,
		reason = "Cell content needs its box origin, measure, appearance and output streams"
	)]
	fn cell_children(
		&mut self,
		cell: &TableCell,
		x: f32,
		y: f32,
		width: f32,
		size: f32,
		align: CellAlign,
		opts: &LayoutOptions,
		out: &mut BlockLayout,
	) -> f32 {
		let mut height = 0.;
		for block in &cell.blocks {
			if (self.cancelled)() {
				break;
			}
			let cursor = y + height;
			for anchor in block.anchors() {
				out.anchors.push(HeadingAnchor {
					anchor: crate::document::AnchorId::Fragment(anchor.clone()),
					y: cursor,
				});
			}
			if let BlockKind::Heading { anchor, .. } = block.kind() {
				out.anchors.push(HeadingAnchor {
					anchor: crate::document::AnchorId::Fragment(anchor.clone()),
					y: cursor,
				});
			}
			let h = match block.kind() {
				BlockKind::Paragraph(text) => self.rich(
					Some(block.id()),
					text,
					x,
					cursor,
					width,
					size,
					false,
					align,
					false,
					false,
					opts,
					out,
				),
				_ => self.cached_child(block, [x, cursor], width, opts, out),
			};
			height += h;
		}
		height
	}

	fn cell_intrinsic(
		&mut self,
		table: &Table,
		cell: &PlacedCell,
		opts: &LayoutOptions,
		table_appearance: &TextAppearance,
	) -> (f32, f32) {
		self.shaper.appearance =
			cell_appearance(table_appearance, table, cell, opts);
		let rule = cell_rule(&self.shaper.appearance.chain, opts, cell, table);
		let pad = super::blocks::block_padding(&rule, opts.font_size);
		let inset = pad[1] + pad[3];
		let size = opts.font_size * self.shaper.appearance.size;
		let mut min = 0_f32;
		let mut preferred = 0_f32;
		let mut scratch = BlockLayout::default();
		for block in &cell_data(table, cell).blocks {
			match block.kind() {
				BlockKind::Paragraph(text)
				| BlockKind::Heading { text, .. } => {
					let prepared = self.prepare(text, size, &mut scratch);
					let units = self.units(
						&prepared,
						size,
						false,
						false,
						f32::MAX,
						opts.typography(),
					);
					let mut segment = 0_f32;
					let mut block_min = 0_f32;
					for unit in &units {
						segment += unit.width;
						if unit.after.is_some() {
							block_min = block_min.max(segment);
							segment = 0.;
						}
					}
					block_min = block_min.max(segment);
					min = min.max(block_min + inset);
					preferred = preferred.max(
						units.iter().map(|unit| unit.width).sum::<f32>()
							+ inset,
					);
				}
				BlockKind::Code { text, .. } => {
					let widest = text
						.lines()
						.map(|line| line.chars().count() as f32)
						.fold(0_f32, f32::max);
					min = min.max((widest * size * 0.5).min(48.) + inset);
					preferred = preferred.max((widest * size * 0.6) + inset);
				}
				_ => {
					min = min.max(48. + inset);
					preferred = preferred.max(160. + inset);
				}
			}
		}
		(min, preferred)
	}

	#[cfg_attr(
		any(),
		markview::allow_complexity(
			reason = "Keep grid construction, span clipping and shared-border resolution in one ordered pass; each decision consumes the occupancy grid."
		)
	)]
	fn build_grid(
		&self,
		table: &Table,
		columns: usize,
		opts: &LayoutOptions,
		table_appearance: &TextAppearance,
		collapse: bool,
	) -> (Vec<PlacedCell>, usize) {
		let capacity = table
			.groups
			.iter()
			.map(|group| group.rows.len())
			.sum::<usize>()
			.min(opts.limits.table_rows);
		let cell_limit = opts.limits.table_cells.max(1);
		let mut grid = vec![vec![None::<usize>; columns]; capacity];
		let mut cells = Vec::new();
		let mut total_rows = 0usize;
		for (group_index, group) in table.groups.iter().enumerate() {
			if total_rows >= capacity || cells.len() >= cell_limit {
				break;
			}
			let rows = group.rows.len().min(capacity - total_rows);
			if rows == 0 {
				continue;
			}
			for (row, entry) in group.rows.iter().take(rows).enumerate() {
				let global_row = total_rows + row;
				let mut col = 0;
				for (index, cell) in entry.cells.iter().enumerate() {
					while col < columns && grid[global_row][col].is_some() {
						col += 1;
					}
					if col >= columns {
						break;
					}
					let colspan =
						(cell.colspan.max(1) as usize).min(columns - col);
					let rowspan =
						(cell.rowspan.max(1) as usize).min(rows - row);
					let free = (global_row..global_row + rowspan).all(|r| {
						(col..col + colspan).all(|c| grid[r][c].is_none())
					});
					if !free {
						col += 1;
						continue;
					}
					if cells.len() >= cell_limit {
						break;
					}
					let id = cells.len();
					cells.push(PlacedCell {
						group: group_index,
						row,
						cell: index,
						rowspan,
						colspan,
						row_start: global_row,
						col,
						draw: 0,
						min_width: 0.,
						preferred_width: 0.,
						gap_before: false,
						gap_after: false,
						decoration: None,
					});
					for row in grid
						.iter_mut()
						.take(global_row + rowspan)
						.skip(global_row)
					{
						for slot in row.iter_mut().take(col + colspan).skip(col)
						{
							*slot = Some(id);
						}
					}
					col += colspan;
				}
			}
			total_rows += rows;
		}
		let mut base_edges = vec![[0_f32; 4]; cells.len()];
		let mut gap_before = vec![false; cells.len()];
		let mut gap_after = vec![false; cells.len()];
		for (id, cell) in cells.iter().enumerate() {
			let appearance =
				cell_appearance(table_appearance, table, cell, opts);
			let rule = cell_rule(&appearance.chain, opts, cell, table);
			base_edges[id] = rule
				.border_edges
				.unwrap_or([rule.border_width.unwrap_or(0.); 4]);
			gap_before[id] = rule.space_before.unwrap_or(0.) > 0.;
			gap_after[id] = rule.space_after.unwrap_or(0.) > 0.;
		}
		for (id, cell) in cells.iter_mut().enumerate() {
			let appearance =
				cell_appearance(table_appearance, table, cell, opts);
			let rule = cell_rule(&appearance.chain, opts, cell, table);
			cell.gap_before = gap_before[id];
			cell.gap_after = gap_after[id];
			let own = base_edges[id];
			let mut edges = own;
			if collapse {
				let neighbor = |r: usize, c: usize| -> Option<usize> {
					grid.get(r).and_then(|row| row.get(c)).copied().flatten()
				};
				let side = |range: std::ops::Range<usize>,
				            f: &dyn Fn(usize) -> Option<usize>| {
					range.map(f).collect::<Option<Vec<_>>>()
				};
				if cell.row_start > 0
					&& let Some(ids) =
						side(cell.col..cell.col + cell.colspan, &|c| {
							neighbor(cell.row_start - 1, c)
						}) && ids.iter().all(|id| *id == ids[0])
					&& !cell.gap_before
					&& !gap_after[ids[0]]
					&& base_edges[ids[0]][2] >= own[0]
				{
					edges[0] = 0.;
				}
				if cell.row_start + cell.rowspan < total_rows
					&& let Some(ids) =
						side(cell.col..cell.col + cell.colspan, &|c| {
							neighbor(cell.row_start + cell.rowspan, c)
						}) && ids.iter().all(|id| *id == ids[0])
					&& !cell.gap_after
					&& !gap_before[ids[0]]
					&& base_edges[ids[0]][0] > own[2]
				{
					edges[2] = 0.;
				}
				if cell.col > 0
					&& let Some(ids) = side(
						cell.row_start..cell.row_start + cell.rowspan,
						&|r| neighbor(r, cell.col - 1),
					) && ids.iter().all(|id| *id == ids[0])
					&& base_edges[ids[0]][1] >= own[3]
				{
					edges[3] = 0.;
				}
				if cell.col + cell.colspan < columns
					&& let Some(ids) = side(
						cell.row_start..cell.row_start + cell.rowspan,
						&|r| neighbor(r, cell.col + cell.colspan),
					) && ids.iter().all(|id| *id == ids[0])
					&& base_edges[ids[0]][3] > own[1]
				{
					edges[1] = 0.;
				}
			}
			cell.decoration = if collapse {
				Some(std::sync::Arc::new(BoxDecoration {
					edges,
					corners: [0.0; 4],
				}))
			} else {
				BoxDecoration::from_rule(&rule, false)
			};
		}
		(cells, total_rows)
	}
}

fn grow(values: &mut [f32], range: std::ops::Range<usize>, need: f32) {
	let current: f32 = values[range.clone()].iter().sum();
	if current >= need {
		return;
	}
	let deficit = need - current;
	let count = range.len() as f32;
	for value in &mut values[range] {
		*value += deficit / count;
	}
}

fn cell_data<'a>(table: &'a Table, cell: &PlacedCell) -> &'a TableCell {
	&table.groups[cell.group].rows[cell.row].cells[cell.cell]
}

fn cell_appearance(
	table_appearance: &TextAppearance,
	table: &Table,
	cell: &PlacedCell,
	opts: &LayoutOptions,
) -> TextAppearance {
	let rows: usize = table.groups.iter().map(|group| group.rows.len()).sum();
	let parent = opts
		.stylesheet
		.child(table_appearance, cell.row_start, rows);
	let base = opts.stylesheet.text(&parent, Condition::Cell);
	if cell_data(table, cell).kind == CellKind::Header {
		opts.stylesheet.text(&base, Condition::Header)
	} else {
		base
	}
}

fn cell_rule(
	chain: &u128,
	opts: &LayoutOptions,
	cell: &PlacedCell,
	table: &Table,
) -> Rule {
	let mut rule = opts.stylesheet.element_rule(*chain, Condition::Cell);
	if cell_data(table, cell).kind == CellKind::Header {
		rule.overlay(&opts.stylesheet.element_rule(*chain, Condition::Header));
	}
	rule
}
