use super::*;
use proptest::prelude::*;

fn check(units: &[Unit], solution: &Solution) {
	let mut kept = Vec::new();
	let mut previous = 0;
	for line in &solution.lines {
		assert!(
			line.units.start >= previous && line.units.end <= units.len(),
			"{solution:?}"
		);
		assert!(units[previous..line.units.start].iter().all(|u| u.discard));
		kept.extend(line.units.clone().filter(|&i| !units[i].discard));
		let width: f32 =
			units[line.units.clone()].iter().map(|u| u.width).sum();
		if line.hyphen {
			let mut candidates = Vec::new();
			for (i, u) in units
				.iter()
				.enumerate()
				.skip(line.units.end.saturating_sub(1))
			{
				if let Some(br) = u.after {
					candidates.push(width + br.hyphen_width);
				}
				if i >= line.units.end && !u.discard {
					break;
				}
			}
			assert!(candidates.contains(&line.width), "{solution:?}");
		} else {
			assert_eq!(line.width, width, "{solution:?}");
		}
		previous = line.units.end;
	}
	assert!(units[previous..].iter().all(|u| u.discard));
	assert_eq!(
		kept,
		(0..units.len())
			.filter(|&i| !units[i].discard)
			.collect::<Vec<_>>()
	);
}
proptest! {
	#[test]
	fn forced_line_endings_preserve_natural_width(
		widths in prop::collection::vec(1u8..64, 1..24),
		target in 64u16..2000,
	) {
		let units: Vec<_> = widths.iter().enumerate().map(|(i, &width)| Unit {
			source: i..i + 1,
			width: width as f32,
			stretch: 2.0,
			shrink: 1.0,
			justifiable: true,
			discard: false,
			after: Some(Break::FORCED),
		}).collect();
		let limits = Limits::default();
		for solution in [
			greedy(&units, target as f32, &limits),
			break_lines(&units, target as f32, 12.0, true, &limits),
		] {
			check(&units, &solution);
			prop_assert_eq!(solution.lines.len(), widths.len());
			for line in &solution.lines {
				prop_assert!(line.last);
				prop_assert_eq!(line.ratio, 0.0);
			}
		}
	}
}

proptest! {
	#[test]
	fn all_entries_conserve_content(data in prop::collection::vec((0u8..32,any::<bool>(),0u8..4),0..24), target in 1u16..100, first in 1u16..100, budget in prop_oneof![Just(0usize),Just(1),Just(10000)], justified in any::<bool>()) {
		let units: Vec<_> = data
			.into_iter()
			.enumerate()
			.map(|(i, (w, discard, kind))| Unit {
				source: i..i + 1,
				width: w as f32,
				stretch: 2.,
				shrink: 1.,
				justifiable: true,
				discard,
				after: match kind {
					0 => None,
					1 => Some(Break::NORMAL),
					2 => Some(Break::FORCED),
					_ => Some(Break {
						hyphen_width: 2.,
						..Break::NORMAL
					}),
				},
			})
			.collect();
		let limits = Limits {
			linebreak_evaluations: budget,
			..Limits::default()
		};
		let (target, first) = (target as f32, first as f32);
		let g = greedy(&units, target, &limits);
		check(&units, &g);
		prop_assert!(g.evaluations <= budget);
		let gf = greedy_with_first(&units, target, first, budget);
		check(&units, &gf);
		prop_assert!(gf.evaluations <= budget);
		let b = break_lines(&units, target, 12., justified, &limits);
		check(&units, &b);
		let bf =
			break_lines_with_first(&units, target, first, 12., justified, &limits);
		check(&units, &bf);
		let same =
			break_lines_with_first(&units, target, target, 12., justified, &limits);
		prop_assert_eq!(format!("{b:?}"), format!("{same:?}"));
		prop_assert_eq!(
			format!("{g:?}"),
			format!("{:?}", greedy_with_first(&units, target, target, budget))
		);
	}
}

proptest! {
	#[test]
	fn short_normal_solutions_belong_to_enumerated_partitions(widths in prop::collection::vec(1u8..12,1..7),target in 12u8..64,forced in 0usize..6) {
		let units: Vec<_> = widths
			.iter()
			.enumerate()
			.map(|(i, &w)| Unit {
				source: i..i + 1,
				width: w as f32,
				stretch: 0.,
				shrink: 0.,
				justifiable: false,
				discard: false,
				after: Some(if i == forced {
					Break::FORCED
				} else {
					Break::NORMAL
				}),
			})
			.collect();
		let solution =
			break_lines(&units, target as f32, 12., false, &Limits::default());
		check(&units, &solution);
		let mut allowed = Vec::new();
		for mask in 0..(1usize << (units.len() - 1)) {
			let ends: Vec<_> = (1..=units.len())
				.filter(|&end| end == units.len() || mask & (1 << (end - 1)) != 0)
				.collect();
			if forced + 1 < units.len() && !ends.contains(&(forced + 1)) {
				continue;
			}
			let mut start = 0;
			if ends.iter().all(|&end| {
				let width: u16 = widths[start..end].iter().map(|&w| w as u16).sum();
				start = end;
				width <= target as u16
			}) {
				allowed.push(ends);
			}
		}
		prop_assert!(!allowed.is_empty());
		prop_assert!(
			!solution.degraded,
			"feasible short input fell back: {:?}",
			solution
		);
		let ends: Vec<_> = solution.lines.iter().map(|l| l.units.end).collect();
		prop_assert!(
			allowed.contains(&ends),
			"allowed={:?}, actual={:?}",
			allowed,
			solution
		);
	}
}
