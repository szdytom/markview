use super::*;
use proptest::prelude::*;

fn occurrence(value: u32) -> LinkOccurrenceId {
	LinkOccurrenceId::new(value).unwrap()
}

proptest! {
	#[test]
	fn addresses_and_occurrences_are_separate(url in ".{1,64}", ids in prop::collection::vec(1u32..17, 0..32)) {
		let first = Link::new(url.clone(), occurrence(1));
		let independent = Link::new(url.clone(), occurrence(1));
		prop_assert!(!Arc::ptr_eq(first.shared(), independent.shared()));
		prop_assert!(first.same_address(&independent));
		prop_assert!(first.same_occurrence(&independent));
		prop_assert_eq!(first.hash_key(), independent.hash_key());
		let mut current = first.clone();
		for id in ids {
			current = current.with_occurrence(occurrence(id));
			prop_assert_eq!(current.as_str(), url.as_str());
			prop_assert_eq!(current.occurrence(), occurrence(id));
			prop_assert!(Arc::ptr_eq(current.shared(), first.shared()));
			prop_assert_eq!(current.same_occurrence(&first), id == 1);
			prop_assert!(current.same_occurrence(&current.clone()));
			prop_assert!(current.same_address(&independent));
			prop_assert_eq!(current.hash_key(), first.hash_key());
		}
	}
	#[test]
	fn identity_uses_address_and_explicit_occurrence(a in ".{0,32}", b in ".{0,32}", c in ".{0,32}", ids in prop::array::uniform3(1u32..4)) {
		let texts = [a, b, c];
		let links: Vec<_> = texts
			.iter()
			.zip(ids)
			.map(|(text, id)| Link::new(text.clone(), occurrence(id)))
			.collect();
		for i in 0..3 {
			for j in 0..3 {
				prop_assert_eq!(
					links[i].same_address(&links[j]),
					texts[i] == texts[j]
				);
				prop_assert_eq!(
					links[i].same_occurrence(&links[j]),
					texts[i] == texts[j] && ids[i] == ids[j]
				);
				if texts[i] == texts[j] {
					prop_assert_eq!(links[i].hash_key(), links[j].hash_key());
				}
			}
		}
	}
}
