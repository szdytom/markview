use super::*;
use proptest::prelude::*;
use std::collections::HashSet;

const FACES: [&[u8]; 2] = [
	include_bytes!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/../markview-layout/tests/fonts/NotoSerif-Regular-subset.otf"
	)),
	include_bytes!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/../markview-layout/tests/fonts/NotoSans-Regular-subset.otf"
	)),
];

proptest! {
	#[test]
	fn host_font_installation_is_atomic_and_each_valid_replacement_has_a_new_identity(
		operations in prop::collection::vec((any::<bool>(), prop::collection::vec(0usize..2, 0..4), any::<usize>(), prop::collection::vec(any::<u8>(), 0..24)), 0..8),
	) {
		install(Vec::new()).unwrap();
		let original = config();
		let mut previous = original.clone();
		let mut identities = HashSet::from([original.clone()]);
		let mut expected = HashSet::new();
		for (valid, faces, insertion, tail) in operations {
			let mut bytes: Vec<_> = faces.iter().map(|&slot| FACES[slot].to_vec()).collect();
			let insertion = insertion % (bytes.len() + 1);
			if !valid { let mut bad = b"not-font".to_vec(); bad.extend(tail); bytes.insert(insertion, bad); }
			let result = install(bytes);
			prop_assert_eq!(result.is_ok(), valid);
			let current = config();
			if valid {
				prop_assert!(identities.insert(current.clone()));
				expected = faces.iter().map(|&slot| ["Noto Serif", "Noto Sans"][slot].to_owned()).collect();
			} else {
				prop_assert!(result.unwrap_err().contains(&format!("index {insertion}:")), "the error must identify the injected bad face");
				prop_assert_eq!(&current, &previous);
			}
			prop_assert!(current.ignore_system_fonts);
			prop_assert!(current.directories.is_empty());
			previous = current;
		}
		let families: HashSet<_> = markview_layout::fonts::families(&config(), false).iter().map(ToString::to_string).collect();
		prop_assert_eq!(families, expected);
		prop_assert!(markview_layout::fonts::families(&original, false).is_empty());
	}
}
