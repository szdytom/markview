use super::{BlockContext, LayoutOptions};
use crate::{
	document::{CellAlign, RichText},
	scene::{BlockLayout, BoxDecoration, Draw, Overflow, Paint, Rect},
	style::{BorderCollapse, Condition},
};

/// One table's grid, and whether its first row is a header. A table that is all
/// data declares no header, so `header` applies nowhere in it.
pub(crate) struct Table<'a> {
	pub align: &'a [CellAlign],
	pub headed: bool,
	pub rows: &'a [Vec<RichText>],
}
impl BlockContext<'_> {
	pub(super) fn table(
		&mut self,
		table: &Table<'_>,
		x: f32,
		y: f32,
		width: f32,
		opts: &LayoutOptions,
		out: &mut BlockLayout,
	) -> f32 {
		let align = table.align;
		let n = align.len().min(opts.limits.table_columns);
		if n == 0 {
			return 0.;
		}
		// Both loops walk the same truncated grid: columns are capped first,
		// then rows are capped so the cell total stays bounded too.
		let rows = &table.rows[..table
			.rows
			.len()
			.min(opts.limits.table_rows)
			.min((opts.limits.table_cells / n).max(1))];
		let table_appearance = self.shaper.appearance.clone();
		let collapse = opts
			.stylesheet
			.element_rule(table_appearance.chain, Condition::Table)
			.border_collapse
			== Some(BorderCollapse::Collapse);
		let mut minima = vec![48_f32; n];
		let mut preferred = vec![48_f32; n];
		let headed = table.headed;
		// Asked for both the text appearance and the box, so the two can never
		// disagree about which row is a header.
		let header_at = |index: usize| headed && index == 0;
		let cell_appearance = |index: usize| {
			let parent =
				opts.stylesheet.child(&table_appearance, index, rows.len());
			let base = opts.stylesheet.text(&parent, Condition::Cell);
			if header_at(index) {
				opts.stylesheet.text(&base, Condition::Header)
			} else {
				base
			}
		};
		let rules: Vec<_> = (0..rows.len())
			.map(|index| {
				let chain = cell_appearance(index).chain;
				let mut rule =
					opts.stylesheet.element_rule(chain, Condition::Cell);
				if header_at(index) {
					rule.overlay(
						&opts.stylesheet.element_rule(chain, Condition::Header),
					);
				}
				rule
			})
			.collect();
		let edges = |index: usize| {
			rules[index]
				.border_edges
				.unwrap_or([rules[index].border_width.unwrap_or(0.0); 4])
		};
		let adjoining = |upper: usize, lower: usize| {
			rules[upper].space_after.unwrap_or(0.0) == 0.0
				&& rules[lower].space_before.unwrap_or(0.0) == 0.0
		};
		let cell_decoration = |row: usize, col: usize| {
			if !collapse {
				return BoxDecoration::from_rule(&rules[row], false);
			}
			let [t, r, b, l] = edges(row);
			let mut shared = [t, r, b, l];
			// Keep the wider edge in its original cell, preserving its color.
			// Ties belong to the upper or left cell.
			if row > 0 && adjoining(row - 1, row) && edges(row - 1)[2] >= t {
				shared[0] = 0.0;
			}
			if col + 1 < n && l > r {
				shared[1] = 0.0;
			}
			if row + 1 < rows.len()
				&& adjoining(row, row + 1)
				&& edges(row + 1)[0] > b
			{
				shared[2] = 0.0;
			}
			if col > 0 && r >= l {
				shared[3] = 0.0;
			}
			Some(BoxDecoration {
				edges: shared,
				corners: [0.0; 4],
			})
		};
		for (row_index, row) in rows.iter().enumerate() {
			self.shaper.appearance = cell_appearance(row_index);
			let rule = &rules[row_index];
			let pad = super::blocks::block_padding(rule, opts.font_size);
			let size = opts.font_size * self.shaper.appearance.size;
			for (col, cell) in row.iter().enumerate().take(n) {
				let p = self.prepare(cell, size, out);
				let units = self.units(
					&p,
					size,
					false,
					false,
					width,
					opts.typography(),
				);
				let inset = pad[1] + pad[3];
				preferred[col] = preferred[col]
					.max(units.iter().map(|u| u.width).sum::<f32>() + inset);
				let mut segment = 0_f32;
				for u in &units {
					segment += u.width;
					if u.after.is_some() {
						minima[col] = minima[col].max(segment + inset);
						segment = 0.;
					}
				}
				minima[col] = minima[col].max(segment + inset);
			}
		}
		let min: f32 = minima.iter().sum();
		let preferred_total: f32 = preferred.iter().sum();
		let total = width.max(min);
		let widths: Vec<f32> = (0..n)
			.map(|i| {
				minima[i]
					+ if preferred_total > min {
						(total - min) * (preferred[i] - minima[i]).max(0.)
							/ (preferred_total - min)
					} else {
						(total - min) / n as f32
					}
			})
			.collect();
		let start = out.draws.len();
		let overflow_start = out.overflow.len();
		let mut top = y;
		for (row_index, row) in rows.iter().enumerate() {
			let role = if header_at(row_index) {
				Condition::Header
			} else {
				Condition::Cell
			};
			let rule = &rules[row_index];
			let pad = super::blocks::block_padding(rule, opts.font_size);
			let before = rule.space_before.unwrap_or(0.) * opts.font_size;
			let after = rule.space_after.unwrap_or(0.) * opts.font_size;
			self.shaper.appearance = cell_appearance(row_index);
			let cell_chain = self.shaper.appearance.chain;
			let size = opts.font_size * self.shaper.appearance.size;
			let mut left = x;
			let mut row_height = size * self.shaper.appearance.line_height
				+ pad[0] + pad[2]
				+ before + after;
			let mut boxes = Vec::new();
			for col in 0..n {
				let index = out.draws.len();
				out.draws.push(Draw::Rect(Rect::default(), Paint::Text));
				boxes.push((index, left, widths[col]));
				if let Some(cell) = row.get(col) {
					let first_node = out.text.len();
					let h = self.rich(
						cell,
						left + pad[3],
						top + before + pad[0],
						(widths[col] - pad[1] - pad[3]).max(1.),
						size,
						false,
						align[col],
						false,
						false,
						opts,
						out,
					);
					if let Some(node) = out.text.get_mut(first_node) {
						node.separator = if col > 0 {
							"\t"
						} else if row_index > 0 {
							"\n"
						} else {
							"\n\n"
						};
					}
					row_height =
						row_height.max(h + pad[0] + pad[2] + before + after);
				}
				left += widths[col];
			}
			for (col, (index, left, w)) in boxes.into_iter().enumerate() {
				out.draws[index] = Draw::Box {
					rect: Rect {
						x: left,
						y: top + before,
						w,
						h: (row_height - before - after).max(0.),
					},
					chain: cell_chain,
					condition: role,
					radius: rule.radius.unwrap_or(0.),
					border: rule.border_width.unwrap_or(0.),
					left_only: false,
					decoration: cell_decoration(row_index, col),
				};
			}
			top += row_height;
		}
		let mut height = top - y;
		if total > width + 0.5 {
			let gutter = opts.stylesheet.scrollbar_gutter();
			out.overflow.truncate(overflow_start);
			out.overflow.push(Overflow {
				rect: Rect {
					x,
					y,
					w: width,
					h: top - y,
				},
				content_width: total,
				commands: start..out.draws.len(),
				gutter,
			});
			height += gutter;
		}
		self.shaper.appearance = table_appearance;
		height
	}
}
