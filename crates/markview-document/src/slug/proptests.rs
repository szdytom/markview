use super::*;
use proptest::prelude::*;

proptest! {
	#[test]
	fn allocator_uses_first_free_suffix(bases in prop::collection::vec((any::<bool>(), prop::sample::select(vec!["", "a", "a-1", "a-2", "a-1-1", "中", "a_b"])), 0..64)) {
		let mut used = std::collections::BTreeSet::new();
		let mut anchors = Anchors::default();
		for (reserve, base) in bases {
			if reserve {
				anchors.reserve(base.to_owned());
				used.insert(base.to_owned());
			} else {
				let mut expected = base.to_owned();
				for suffix in 1.. {
					if !used.contains(&expected) {
						break;
					}
					expected = format!("{base}-{suffix}");
				}
				prop_assert_eq!(anchors.unique(base), expected.clone());
				used.insert(expected);
			}
			for name in ["", "a", "a-1", "a-2", "a-1-1", "中", "a_b"] {
				prop_assert_eq!(anchors.contains(name), used.contains(name));
			}
		}
		prop_assert_eq!(Anchors::default().unique("a"), "a");
	}
	#[test]
	fn slug_keeps_semantic_tokens(tokens in prop::collection::vec(prop::sample::select(vec![("A","a"),("中","中"),("İ","i"),("É","é"),(" ","-"),("_","_"),("-","-"),("!",""),("😀",""),("\t","")]),0..48)) {
		let input: String = tokens.iter().map(|t| t.0).collect();
		let expected: String = tokens.iter().map(|t| t.1).collect();
		prop_assert_eq!(heading_slug(&input), expected);
	}
}
