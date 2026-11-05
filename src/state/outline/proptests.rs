use super::*;
use proptest::prelude::*;

proptest! {
	#[test]
	fn collapse_sequences_hide_exactly_the_declared_descendant_intervals(
		levels in prop::collection::vec(1u8..7, 0..32),
		toggles in prop::collection::vec(0usize..34, 0..40),
	) {
		let entries: Vec<_> = levels.iter().enumerate().map(|(i, level)| OutlineEntry {
			block: crate::document::BlockId::new(i as u32 + 1).unwrap(), level: *level, text: format!("heading {i}"), anchor: format!("h{i}"),
		}).collect();
		let descendants: Vec<Vec<_>> = (0..levels.len()).map(|i| {
			let end = (i + 1..levels.len()).find(|&j| levels[j] <= levels[i]).unwrap_or(levels.len());
			(i + 1..end).collect()
		}).collect();
		for collapsed in [false, true] {
			let mut tree = if collapsed { OutlineTree::all_collapsed(&entries) } else { OutlineTree::default() };
			let mut flags: Vec<_> = (0..34).map(|i| collapsed && descendants.get(i).is_some_and(|children| !children.is_empty())).collect();
			for toggle in [None].into_iter().chain(toggles.iter().copied().map(Some)) {
				if let Some(index) = toggle { tree.toggle(index); flags[index] = !flags[index]; }
				let hidden: BTreeSet<_> = descendants.iter().enumerate().filter(|(i, _)| flags[*i]).flat_map(|(_, children)| children.iter().copied()).collect();
				let expected: Vec<_> = (0..levels.len()).filter(|i| !hidden.contains(i)).collect();
				prop_assert_eq!(tree.rows(&entries), expected);
				for (i, &flag) in flags.iter().enumerate() {
					prop_assert_eq!(tree.is_collapsed(i), flag);
					prop_assert_eq!(OutlineTree::has_children(&entries, i), descendants.get(i).is_some_and(|children| !children.is_empty()));
				}
			}
		}
	}
}
