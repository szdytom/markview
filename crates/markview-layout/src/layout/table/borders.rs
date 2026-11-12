use super::PlacedCell;
use crate::scene::{Rect, fit_edges};

/// A surviving border interval in grid coordinates.
#[derive(Clone, Debug)]
pub(super) struct BorderRun {
	pub side: usize,
	pub start: usize,
	pub end: usize,
	pub width: f32,
	pub cut: Option<Box<BorderGap>>,
}

/// The actual vertical extent of a neighboring cell that owns the overlap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct BorderGap {
	pub top: usize,
	pub bottom: usize,
	pub before: f32,
	pub after: f32,
}

fn exposed_gap(
	side: usize,
	own: &PlacedCell,
	other: &PlacedCell,
	before: [f32; 2],
	after: [f32; 2],
) -> Option<BorderGap> {
	let top = before[1] > 0.
		&& (own.row_start != other.row_start || before[0] < before[1]);
	let bottom = after[1] > 0.
		&& (own.row_start + own.rowspan != other.row_start + other.rowspan
			|| after[0] < after[1]);
	(matches!(side, 1 | 3) && (top || bottom)).then_some(BorderGap {
		top: other.row_start,
		bottom: other.row_start + other.rowspan,
		before: before[1],
		after: after[1],
	})
}

fn neighbor(
	cell: &PlacedCell,
	side: usize,
	index: usize,
) -> Option<(usize, usize)> {
	match side {
		0 => cell.row_start.checked_sub(1).map(|r| (r, index)),
		1 => Some((index, cell.col + cell.colspan)),
		2 => Some((cell.row_start + cell.rowspan, index)),
		3 => cell.col.checked_sub(1).map(|c| (index, c)),
		_ => unreachable!(),
	}
}

pub(super) fn collapse_borders(
	cells: &[PlacedCell],
	grid: &[Vec<Option<usize>>],
	widths: &[[f32; 4]],
	before: &[f32],
	after: &[f32],
	id: usize,
) -> ([f32; 4], Vec<BorderRun>) {
	let cell = &cells[id];
	let mut runs: Vec<BorderRun> = Vec::new();
	let row_end = cell.row_start + cell.rowspan;
	let col_end = cell.col + cell.colspan;
	let mut edges = [0.; 4];
	let mut segmented = false;
	for side in 0..4 {
		let range = if side % 2 == 0 {
			cell.col..col_end
		} else {
			cell.row_start..row_end
		};
		let first_run = runs.len();
		for index in range.clone() {
			let neighbor = neighbor(cell, side, index)
				.and_then(|(r, c)| grid.get(r)?.get(c).copied().flatten());
			let hidden = neighbor.is_some_and(|other| {
				let touching = match side {
					0 => before[id] <= 0. && after[other] <= 0.,
					2 => after[id] <= 0. && before[other] <= 0.,
					_ => true,
				};
				let opposite = widths[other][(side + 2) % 4];
				// Equal widths belong to the upper or left cell.
				touching
					&& (opposite > widths[id][side]
						|| opposite == widths[id][side]
							&& matches!(side, 0 | 3))
			});
			let cut = neighbor.filter(|_| hidden).and_then(|other| {
				exposed_gap(
					side,
					cell,
					&cells[other],
					[before[id], before[other]],
					[after[id], after[other]],
				)
			});
			if (hidden && cut.is_none()) || widths[id][side] == 0. {
				continue;
			}
			push_run(&mut runs, side, index, widths[id][side], cut);
		}
		let side_runs = &runs[first_run..];
		if let [run] = side_runs
			&& run.start == range.start
			&& run.end == range.end
			&& run.cut.is_none()
		{
			edges[side] = run.width;
		} else if !side_runs.is_empty() {
			segmented = true;
		}
	}
	if segmented {
		([0.; 4], runs)
	} else {
		(edges, Vec::new())
	}
}

fn push_run(
	runs: &mut Vec<BorderRun>,
	side: usize,
	index: usize,
	width: f32,
	cut: Option<BorderGap>,
) {
	if let Some(last) = runs.last_mut()
		&& last.side == side
		&& last.end == index
		&& last.cut.as_deref().copied() == cut
	{
		last.end += 1;
	} else {
		runs.push(BorderRun {
			side,
			start: index,
			end: index + 1,
			width,
			cut: cut.map(Box::new),
		});
	}
}

/// Disjoint, inset border rectangles, shared by screen and PDF painting.
pub(super) fn rectangles(
	runs: &[BorderRun],
	rect: Rect,
	columns: &[f32],
	rows: &[f32],
) -> Vec<Rect> {
	if rect.w <= 0. || rect.h <= 0. {
		return Vec::new();
	}
	let mut widths = [0.; 4];
	for run in runs {
		widths[run.side] = run.width;
	}
	let widths = fit_edges(rect, widths);
	let mut result = Vec::new();
	let mut vertical = Vec::new();
	let mut parts = Vec::new();
	let mut next = Vec::new();
	for run in runs {
		let horizontal = run.side % 2 == 0;
		let coordinates = if horizontal { columns } else { rows };
		let start = coordinates[run.start];
		let end = coordinates[run.end];
		let width = widths[run.side];
		let edge = if horizontal {
			Rect {
				x: start,
				y: if run.side == 0 {
					rect.y
				} else {
					rect.y + rect.h - width
				},
				w: end - start,
				h: width,
			}
		} else {
			Rect {
				x: if run.side == 3 {
					rect.x
				} else {
					rect.x + rect.w - width
				},
				y: start,
				w: width,
				h: end - start,
			}
		};
		if let Some(edge) = edge.intersect(rect) {
			if horizontal {
				cut_parts(edge, result.iter().copied(), &mut parts, &mut next);
				result.extend(parts.iter().copied());
			} else {
				if let Some(cut) = &run.cut {
					let top = rows[cut.top] + cut.before;
					let bottom = rows[cut.bottom] - cut.after;
					subtract(
						edge,
						Rect {
							y: top,
							h: (bottom - top).max(0.),
							..edge
						},
						&mut vertical,
					);
				} else {
					vertical.push(edge);
				}
			}
		}
	}
	let horizontal_count = result.len();
	vertical.sort_by(|a, b| a.y.total_cmp(&b.y));
	let mut active: Vec<Rect> = Vec::new();
	for edge in vertical {
		active.retain(|r| r.y + r.h > edge.y);
		let cuts = result[..horizontal_count].iter().chain(&active).copied();
		cut_parts(edge, cuts, &mut parts, &mut next);
		result.extend(parts.iter().copied());
		active.push(edge);
	}
	result
}

fn cut_parts(
	rect: Rect,
	cuts: impl Iterator<Item = Rect>,
	parts: &mut Vec<Rect>,
	next: &mut Vec<Rect>,
) {
	parts.clear();
	parts.push(rect);
	for cut in cuts {
		for part in parts.drain(..) {
			subtract(part, cut, next);
		}
		std::mem::swap(parts, next);
		next.clear();
	}
}

/// Preserve every part of a vertical strip outside a horizontal border.
fn subtract(rect: Rect, cut: Rect, out: &mut Vec<Rect>) {
	let Some(overlap) = rect.intersect(cut) else {
		out.push(rect);
		return;
	};
	out.extend(
		[
			Rect {
				h: overlap.y - rect.y,
				..rect
			},
			Rect {
				y: overlap.y + overlap.h,
				h: rect.y + rect.h - overlap.y - overlap.h,
				..rect
			},
			Rect {
				y: overlap.y,
				w: overlap.x - rect.x,
				h: overlap.h,
				..rect
			},
			Rect {
				x: overlap.x + overlap.w,
				y: overlap.y,
				w: rect.x + rect.w - overlap.x - overlap.w,
				h: overlap.h,
			},
		]
		.into_iter()
		.filter(|r| r.w > 0. && r.h > 0.),
	);
}
