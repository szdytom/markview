//! Queries over ordered, non-overlapping half-open byte ranges.
use std::ops::Range;

/// The item containing `offset`; empty ranges and gaps have no owner.
pub(crate) fn containing<T>(
	items: &[T],
	offset: usize,
	range: impl Fn(&T) -> &Range<usize>,
) -> Option<usize> {
	let index = items.partition_point(|item| range(item).end <= offset);
	items
		.get(index)
		.filter(|item| range(item).contains(&offset))
		.map(|_| index)
}

#[cfg(test)]
mod tests {
	use super::*;
	use proptest::prelude::*;
	use std::cell::Cell;

	proptest! {
		#[test]
		fn arbitrary_query_order_matches_linear_lookup(
			segments in prop::collection::vec((0usize..8, 0usize..4), 0..32),
			queries in prop::collection::vec(0usize..384, 1..64),
		) {
			let mut end = 0;
			let ranges: Vec<_> = segments.into_iter().map(|(length, gap)| {
				let start = end + gap;
				end = start + length;
				(start..end, ())
			}).collect();
			for offset in queries {
				prop_assert_eq!(
					containing(&ranges, offset, |(range, _)| range),
					ranges.iter().position(|(range, _)| range.contains(&offset)),
				);
			}
		}
	}

	#[test]
	fn reverse_queries_skip_empty_ranges_and_gaps() {
		let ranges = [0..0, 0..2, 2..2, 4..7, 7..7, 9..10];
		for (offset, expected) in [
			(9, Some(5)),
			(6, Some(3)),
			(4, Some(3)),
			(1, Some(1)),
			(0, Some(1)),
			(2, None),
			(7, None),
			(3, None),
			(10, None),
		] {
			assert_eq!(containing(&ranges, offset, |range| range), expected);
		}
	}

	#[test]
	fn queries_use_logarithmically_many_range_comparisons() {
		for length in [1usize, 3, 32, 1024] {
			let ranges: Vec<_> = (0..length).map(|i| i..i + 1).collect();
			let calls = Cell::new(0);
			for offset in (0..length).rev() {
				calls.set(0);
				assert_eq!(
					containing(&ranges, offset, |range| {
						calls.set(calls.get() + 1);
						range
					}),
					Some(offset)
				);
				assert!(
					calls.get()
						<= length.next_power_of_two().ilog2() as usize + 2
				);
			}
		}
	}
}
