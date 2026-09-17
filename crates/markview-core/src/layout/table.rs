use super::{BlockContext, LayoutOptions};
use crate::{
	document::{CellAlign, RichText},
	scene::{BlockLayout, Draw, Overflow, Paint, Rect},
	style::Condition,
};
impl BlockContext<'_> {
	#[expect(
		clippy::too_many_arguments,
		reason = "Table geometry and column alignment"
	)]
	pub(super) fn table(
		&mut self,
		align: &[CellAlign],
		rows: &[Vec<RichText>],
		x: f32,
		y: f32,
		width: f32,
		opts: &LayoutOptions,
		out: &mut BlockLayout,
	) -> f32 {
		let n = align.len().min(opts.limits.table_columns);
		if n == 0 {
			return 0.;
		}
		// Both loops walk the same truncated grid: columns are capped first,
		// then rows are capped so the cell total stays bounded too.
		let rows = &rows[..rows
			.len()
			.min(opts.limits.table_rows)
			.min((opts.limits.table_cells / n).max(1))];
		let table_appearance = self.shaper.appearance.clone();
		let mut minima = vec![48_f32; n];
		let mut preferred = vec![48_f32; n];
		let cell_appearance = |header: bool| {
			let base = opts.stylesheet.text(&table_appearance, Condition::Cell);
			if header {
				opts.stylesheet.text(&base, Condition::Header)
			} else {
				base
			}
		};
		let cell_rule = |header: bool| {
			let chain = cell_appearance(header).chain;
			let mut rule = opts.stylesheet.element_rule(chain, Condition::Cell);
			if header {
				rule.overlay(
					&opts.stylesheet.element_rule(chain, Condition::Header),
				);
			}
			rule
		};
		for (row_index, row) in rows.iter().enumerate() {
			self.shaper.appearance = cell_appearance(row_index == 0);
			let rule = cell_rule(row_index == 0);
			let pad = rule
				.padding
				.as_ref()
				.map(|p| p.sides().map(|v| v * opts.font_size))
				.unwrap_or([0.; 4]);
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
			let header = row_index == 0;
			let role = if header {
				Condition::Header
			} else {
				Condition::Cell
			};
			let rule = cell_rule(header);
			let pad = rule
				.padding
				.as_ref()
				.map(|p| p.sides().map(|v| v * opts.font_size))
				.unwrap_or([0.; 4]);
			let before = rule.space_before.unwrap_or(0.) * opts.font_size;
			let after = rule.space_after.unwrap_or(0.) * opts.font_size;
			self.shaper.appearance = cell_appearance(header);
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
			for (index, left, w) in boxes {
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
