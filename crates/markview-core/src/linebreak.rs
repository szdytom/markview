//! Bounded Knuth–Plass: boxes, stretchable glue, penalties and fitness classes.
//! Widths are shaped advances, never estimates from Unicode character counts.
use crate::limits::Limits;
use std::ops::Range;

#[derive(Clone, Copy, Debug)]
pub struct Break {
	pub penalty: f64,
	pub hyphen_width: f32,
	pub forced: bool,
	/// Whether the line ending here is still set flush. Only a break the
	/// author asked for does that.
	pub justify: bool,
}
impl Break {
	pub const NORMAL: Self = Self {
		penalty: 0.0,
		hyphen_width: 0.0,
		forced: false,
		justify: false,
	};
	pub const FORCED: Self = Self {
		forced: true,
		..Self::NORMAL
	};
}

#[derive(Clone, Debug)]
pub struct Unit {
	pub source: Range<usize>,
	pub width: f32,
	pub stretch: f32,
	pub shrink: f32,
	/// Whether the line's leftover slack may be shared out over this unit.
	pub justifiable: bool,
	pub discard: bool,
	pub after: Option<Break>,
}

#[derive(Clone, Debug)]
pub struct Line {
	pub units: Range<usize>,
	pub width: f32,
	pub ratio: f32,
	pub hyphen: bool,
	pub last: bool,
}

#[derive(Default, Debug)]
pub struct Solution {
	pub lines: Vec<Line>,
	pub demerits: f64,
	pub degraded: bool,
	pub evaluations: usize,
}

const INF: f64 = f64::INFINITY;

/// Demerits for a last line holding a single unbreakable chunk, which strands
/// one word on a line of its own. Weighed against the hyphen penalty, which is
/// the other thing the break search trades against even spacing.
const RUNT_DEMERITS: f64 = 1800.0;

#[derive(Clone, Copy)]
struct State {
	cost: f64,
	previous: (usize, usize),
}
const EMPTY: State = State {
	cost: INF,
	previous: (0, 0),
};

struct Prefix {
	width: Vec<f32>,
	stretch: Vec<f32>,
	shrink: Vec<f32>,
	justifiables: Vec<usize>,
}
impl Prefix {
	fn new(units: &[Unit]) -> Self {
		let mut p = Self {
			width: vec![0.0],
			stretch: vec![0.0],
			shrink: vec![0.0],
			justifiables: vec![0],
		};
		for u in units {
			p.width.push(p.width.last().unwrap() + u.width);
			p.stretch.push(p.stretch.last().unwrap() + u.stretch);
			p.shrink.push(p.shrink.last().unwrap() + u.shrink);
			p.justifiables
				.push(p.justifiables.last().unwrap() + u.justifiable as usize);
		}
		p
	}
	fn metrics(
		&self,
		units: &[Unit],
		mut start: usize,
		mut end: usize,
	) -> (Range<usize>, f32, f32, f32, usize) {
		while start < end && units[start].discard {
			start += 1;
		}
		while end > start && units[end - 1].discard {
			end -= 1;
		}
		let mut stretch = self.stretch[end] - self.stretch[start];
		let mut shrink = self.shrink[end] - self.shrink[start];
		let mut justifiables =
			self.justifiables[end] - self.justifiables[start];
		// Inter-character glue belongs between characters, not after the line.
		if end > start && !units[end - 1].discard {
			stretch -= units[end - 1].stretch;
			shrink -= units[end - 1].shrink;
			justifiables -= units[end - 1].justifiable as usize;
		}
		(
			start..end,
			self.width[end] - self.width[start],
			stretch.max(0.0),
			shrink.max(0.0),
			justifiables,
		)
	}
}

#[expect(
	clippy::too_many_arguments,
	reason = "Line metrics and break context are independent inputs"
)]
fn line_score(
	width: f32,
	stretch: f32,
	shrink: f32,
	justifiables: usize,
	target: f32,
	size: f32,
	last: bool,
	justified: bool,
	emergency: bool,
) -> Option<(f32, usize, f64)> {
	let delta = target - width;
	if last && delta >= -0.01 {
		return Some((0.0, 1, 0.0));
	}
	if !justified {
		if delta < -0.01 {
			return None;
		}
		let r = delta / target.max(1.0);
		return Some((0.0, 1, (10.0 + 100.0 * (r as f64).powi(2)).powi(2)));
	}
	if delta < -0.01 && shrink <= 0.0 {
		return None;
	}
	let stretch = stretch + if emergency { target * 0.15 } else { 0.0 };
	let solve =
		crate::microtype::solve(width, target, stretch, shrink, justifiables);
	if solve.ratio < -1.0 {
		return None;
	}
	// Past its natural stretchability an underfull line hands the leftover
	// slack to the justifiable clusters. Normalizing that per-cluster share by
	// half an em keeps its cost on the same scale as an ordinary ratio.
	let ratio = if solve.extra > 0.0 {
		1.0 + solve.extra / (size * 0.5)
	} else {
		solve.ratio
	};
	if ratio > 16.0 {
		return None;
	}
	let fitness = if ratio < -0.5 {
		0
	} else if ratio <= 0.5 {
		1
	} else if ratio <= 1.0 {
		2
	} else {
		3
	};
	let badness = 100.0 * (ratio.abs() as f64).powi(3);
	Some((ratio, fitness, (10.0 + badness).powi(2)))
}

#[expect(
	clippy::too_many_arguments,
	reason = "Line metrics and break context are independent inputs"
)]
fn optimize(
	units: &[Unit],
	target: f32,
	first_target: f32,
	size: f32,
	justified: bool,
	emergency: bool,
	budget: &mut usize,
	limit: usize,
) -> Option<Solution> {
	let p = Prefix::new(units);
	let mut points = vec![0];
	points.extend(
		units
			.iter()
			.enumerate()
			.filter_map(|(i, u)| u.after.map(|_| i + 1)),
	);
	if *points.last()? != units.len() {
		points.push(units.len());
	}
	let mut states = vec![[EMPTY; 4]; points.len()];
	states[0][1].cost = 0.0;
	let mut earliest = 0;
	for j in 1..points.len() {
		let end = points[j];
		let br = units[end - 1].after.unwrap_or(Break::FORCED);
		let last = end == units.len() || (br.forced && !br.justify);
		for i in (earliest..j).rev() {
			*budget += 1;
			if *budget > limit {
				return None;
			}
			let (range, natural, stretch, shrink, justifiables) =
				p.metrics(units, points[i], end);
			// Only the line opening the paragraph sees the first-line indent.
			let line_target =
				if points[i] == 0 { first_target } else { target };
			let width = natural + if last { 0.0 } else { br.hyphen_width };
			if width - shrink > line_target + 0.01 && !range.is_empty() {
				break;
			}
			if range.is_empty() && !last {
				continue;
			}
			let Some((_, fitness, cost)) = line_score(
				width,
				stretch,
				shrink,
				justifiables,
				line_target,
				size,
				last,
				justified,
				emergency,
			) else {
				continue;
			};
			let mut penalty = if br.penalty >= 0.0 {
				br.penalty.powi(2)
			} else {
				-br.penalty.powi(2)
			};
			// A last line holding one unbreakable chunk strands a lone word on
			// a line of its own, so reflow the lines above to avoid it.
			if last && j == i + 1 {
				penalty += RUNT_DEMERITS;
			}
			let flagged = br.hyphen_width > 0.0;
			let previous_flagged = points[i] > 0
				&& units[points[i] - 1]
					.after
					.is_some_and(|b| b.hyphen_width > 0.0);
			for f in 0..4 {
				let adjacent =
					if fitness.abs_diff(f) > 1 { 3000.0 } else { 0.0 };
				let consecutive = if flagged && previous_flagged {
					5000.0
				} else {
					0.0
				};
				let total =
					states[i][f].cost + cost + penalty + adjacent + consecutive;
				if total < states[j][fitness].cost {
					states[j][fitness] = State {
						cost: total,
						previous: (i, f),
					};
				}
			}
		}
		if br.forced {
			earliest = j;
		}
	}
	let end = points.len() - 1;
	let fitness = (0..4)
		.min_by(|&a, &b| states[end][a].cost.total_cmp(&states[end][b].cost))?;
	if !states[end][fitness].cost.is_finite() {
		return None;
	}
	let mut result = Solution {
		demerits: states[end][fitness].cost,
		evaluations: *budget,
		..Default::default()
	};
	let (mut j, mut f) = (end, fitness);
	while j > 0 {
		let (i, previous_f) = states[j][f].previous;
		let br = units[points[j] - 1].after.unwrap_or(Break::FORCED);
		let last = points[j] == units.len() || (br.forced && !br.justify);
		let (range, width, stretch, shrink, justifiables) =
			p.metrics(units, points[i], points[j]);
		let hyphen = !last && br.hyphen_width > 0.0;
		let width = width + if hyphen { br.hyphen_width } else { 0.0 };
		let line_target = if points[i] == 0 { first_target } else { target };
		let (ratio, _, _) = line_score(
			width,
			stretch,
			shrink,
			justifiables,
			line_target,
			size,
			last,
			justified,
			emergency,
		)?;
		result.lines.push(Line {
			units: range,
			width,
			ratio,
			hyphen,
			last,
		});
		(j, f) = (i, previous_f);
	}
	result.lines.reverse();
	Some(result)
}

/// Greedy fallback observes legal boundaries; an unbreakable box may overflow.
pub fn greedy(units: &[Unit], target: f32, limits: &Limits) -> Solution {
	greedy_with_first(units, target, target, limits.linebreak_evaluations)
}

/// Greedy fallback whose first line may have a shorter target, for indents.
///
/// `max_evaluations` bounds the candidate scan. Once it is spent, the scan
/// stops at the best candidate found so far, or advances one unit when there
/// was none, so the loop always terminates and never loses content.
pub fn greedy_with_first(
	units: &[Unit],
	target: f32,
	first_target: f32,
	max_evaluations: usize,
) -> Solution {
	let p = Prefix::new(units);
	let mut result = Solution {
		degraded: true,
		..Default::default()
	};
	let mut start = 0;
	while start < units.len() {
		let line_target =
			if start == 0 { first_target } else { target }.max(1.0);
		let mut best = None;
		let mut end = start + 1;
		while end <= units.len() {
			// The budget stops the scan even when no legal break was found, so
			// a paragraph with few break opportunities cannot cost O(n²).
			if result.evaluations >= max_evaluations {
				break;
			}
			result.evaluations += 1;
			let br = units[end - 1]
				.after
				.or_else(|| (end == units.len()).then_some(Break::FORCED));
			let Some(br) = br else {
				end += 1;
				continue;
			};
			let (range, width, ..) = p.metrics(units, start, end);
			let last = br.forced || end == units.len();
			let width = width + if last { 0.0 } else { br.hyphen_width };
			if width > line_target && best.is_some() {
				break;
			}
			best = Some((
				end,
				Line {
					units: range,
					width,
					ratio: 0.0,
					hyphen: !last && br.hyphen_width > 0.0,
					last,
				},
			));
			if last || width > line_target {
				break;
			}
			end += 1;
		}
		let (end, line) = best.unwrap_or_else(|| {
			// The budget ran out before a legal break; take one unit so the
			// outer loop always advances.
			let (range, width, ..) = p.metrics(units, start, start + 1);
			(
				start + 1,
				Line {
					units: range,
					width,
					ratio: 0.0,
					hyphen: false,
					last: start + 1 == units.len(),
				},
			)
		});
		result.lines.push(line);
		start = end;
	}
	result
}

pub fn break_lines(
	units: &[Unit],
	target: f32,
	size: f32,
	justified: bool,
	limits: &Limits,
) -> Solution {
	break_lines_with_first(units, target, target, size, justified, limits)
}

/// Bounded Knuth–Plass where the line opening the paragraph may be indented,
/// so it is measured and justified against a shorter target than the rest.
pub fn break_lines_with_first(
	units: &[Unit],
	target: f32,
	first_target: f32,
	size: f32,
	justified: bool,
	limits: &Limits,
) -> Solution {
	if units.is_empty() {
		return Solution::default();
	}
	let target = target.max(1.0);
	let first_target = first_target.max(1.0);
	let limit = limits.linebreak_evaluations;
	let mut budget = 0;
	if let Some(s) = optimize(
		units,
		target,
		first_target,
		size,
		justified,
		false,
		&mut budget,
		limit,
	) {
		return s;
	}
	if budget <= limit
		&& let Some(s) = optimize(
			units,
			target,
			first_target,
			size,
			justified,
			true,
			&mut budget,
			limit,
		) {
		return s;
	}
	let spent = budget;
	let mut s = greedy_with_first(units, target, first_target, limit);
	s.evaluations += spent;
	s
}

#[cfg(test)]
mod tests {
	use super::*;
	fn words(widths: &[f32]) -> Vec<Unit> {
		let mut units = Vec::new();
		for &width in widths {
			units.push(Unit {
				source: 0..1,
				width,
				stretch: 0.0,
				shrink: 0.0,
				justifiable: false,
				discard: false,
				after: None,
			});
			units.push(Unit {
				source: 1..2,
				width: 3.0,
				stretch: 2.0,
				shrink: 1.0,
				justifiable: true,
				discard: true,
				after: Some(Break::NORMAL),
			});
		}
		units.last_mut().unwrap().after = Some(Break::FORCED);
		units
	}
	#[test]
	fn forced_breaks_are_not_skipped() {
		let mut u = words(&[12.0; 8]);
		u[3].after = Some(Break::FORCED);
		let s = break_lines(&u, 100.0, 10.0, true, &Limits::default());
		assert_eq!(s.lines[0].units, 0..3);
		assert_eq!(s.lines.len(), 2);
		assert!(s.lines.iter().all(|l| l.last && l.ratio == 0.0));
	}
	#[test]
	fn optimal_raggedness_matches_exhaustive_partitions() {
		let u = words(&[13.0, 9.0, 18.0, 7.0, 14.0, 11.0]);
		let target = 40.0;
		let p = Prefix::new(&u);
		let mut best = INF;
		for mask in 0..(1 << 5) {
			let mut start = 0;
			let mut cost = 0.0;
			for word in 0..6 {
				if word != 5 && mask & (1 << word) == 0 {
					continue;
				}
				let end = (word + 1) * 2;
				let (_, w, stretch, shrink, justifiables) =
					p.metrics(&u, start, end);
				cost += line_score(
					w,
					stretch,
					shrink,
					justifiables,
					target,
					10.0,
					word == 5,
					false,
					false,
				)
				.map_or(INF, |s| s.2);
				start = end;
			}
			best = best.min(cost);
		}
		let actual = break_lines(&u, target, 10.0, false, &Limits::default());
		assert!(!actual.degraded);
		assert!((actual.demerits - best).abs() < 0.001);
	}
	#[test]
	fn unbreakable_box_overflows_without_losing_content() {
		let u = words(&[500.0, 10.0]);
		let s = break_lines(&u, 50.0, 10.0, true, &Limits::default());
		assert!(s.degraded);
		assert_eq!(s.lines[0].units, 0..1);
		assert_eq!(s.lines[1].units, 2..3);
	}
	#[test]
	fn first_line_target_narrows_only_the_opening_line() {
		let u = words(&[12.0; 6]);
		let indented = break_lines_with_first(
			&u,
			40.0,
			20.0,
			10.0,
			false,
			&Limits::default(),
		);
		assert!(!indented.degraded);
		// The full measure fits two words per line; the indented first line
		// fits only one, and the remaining lines recover the full measure.
		assert_eq!(indented.lines[0].units, 0..1);
		assert_eq!(indented.lines.len(), 4);
		let equal = break_lines_with_first(
			&u,
			40.0,
			40.0,
			10.0,
			false,
			&Limits::default(),
		);
		let baseline = break_lines(&u, 40.0, 10.0, false, &Limits::default());
		assert_eq!(equal.lines.len(), baseline.lines.len());
		assert!(
			equal
				.lines
				.iter()
				.zip(&baseline.lines)
				.all(|(a, b)| a.units == b.units)
		);
		let greedy = greedy_with_first(
			&u,
			40.0,
			20.0,
			Limits::default().linebreak_evaluations,
		);
		assert_eq!(greedy.lines[0].units, 0..1);
	}
	#[test]
	fn a_runt_last_line_is_reflowed_away() {
		// Three 30 wide words and two 3 wide spaces fill the measure exactly,
		// so the greedy shape of seven words would strand the last one alone.
		let u = words(&[30.0; 7]);
		let s = break_lines(&u, 100.0, 10.0, false, &Limits::default());
		assert_eq!(s.lines.len(), 3);
		// Trading a slightly underfull line above for a two word last line
		// costs less than the runt penalty, so both words stay on the last line.
		let last = s.lines.last().unwrap();
		assert_eq!(last.units.len(), 3, "{:?}", s.lines);
	}
	#[test]
	fn greedy_budget_terminates_and_covers_every_unit() {
		// One unbreakable run: without a budget the scan is quadratic.
		let mut u = words(&[7.0; 400]);
		for unit in &mut u {
			unit.after = None;
		}
		u.last_mut().unwrap().after = Some(Break::FORCED);
		let budget = 64;
		let s = greedy_with_first(&u, 40.0, 40.0, budget);
		assert_eq!(s.evaluations, budget);
		// The lines are ordered, never go backwards, and reach the end.
		let mut next = 0;
		for line in &s.lines {
			assert!(line.units.start <= line.units.end);
			assert!(line.units.start >= next);
			next = line.units.end;
		}
		assert_eq!(s.lines.last().unwrap().units.end, u.len());
	}
	#[test]
	fn a_zero_budget_still_produces_lines() {
		let u = words(&[9.0; 5]);
		let s = greedy_with_first(&u, 40.0, 40.0, 0);
		assert_eq!(s.evaluations, 0);
		assert_eq!(s.lines.last().unwrap().units.end, u.len());
	}
}
