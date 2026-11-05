use super::*;
use proptest::prelude::*;

fn matches(pattern: &[char], name: &[char]) -> bool {
	let mut prefixes = vec![false; name.len() + 1];
	prefixes[0] = true;
	for &token in pattern {
		let mut next = vec![false; name.len() + 1];
		for start in 0..=name.len() {
			if !prefixes[start] {
				continue;
			}
			if token == '*' {
				for (end, reachable) in next.iter_mut().enumerate().skip(start)
				{
					*reachable = true;
					if name.get(end) == Some(&'/') {
						break;
					}
				}
			} else if start < name.len()
				&& (token == '?' && name[start] != '/' || token == name[start])
			{
				next[start + 1] = true;
			}
		}
		prefixes = next;
	}
	prefixes[name.len()]
}

proptest! {
	#[test]
	fn archive_patterns_match_an_exhaustive_unicode_prefix_model_without_crossing_separators(
		pattern in prop::collection::vec(prop::sample::select(vec!['a', 'b', '中', 'é', '/', '*', '?']), 0..16),
		name in prop::collection::vec(prop::sample::select(vec!['a', 'b', '中', 'é', '/', '*', '?']), 0..16),
	) {
		let expected = matches(&pattern, &name);
		let pattern: String = pattern.into_iter().collect();
		let name: String = name.into_iter().collect();
		prop_assert_eq!(glob_match(&pattern, &name), expected);
	}

	#[test]
	fn archive_wildcards_match_literal_asterisks_in_member_names(prefix in "[ab中é]{0,4}", tail in "[ab中é]{2,5}") {
		let pattern = format!("{prefix}*?");
		let name = format!("{prefix}*{tail}");
		prop_assert!(matches(&pattern.chars().collect::<Vec<_>>(), &name.chars().collect::<Vec<_>>()));
		prop_assert!(glob_match(&pattern, &name));
	}
}
