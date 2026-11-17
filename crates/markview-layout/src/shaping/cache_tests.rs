use super::*;
use proptest::prelude::*;

#[test]
fn font_sets_share_borrowed_stacks_but_keep_weights_and_revisions_distinct() {
	let mut shaper = TextShaper::new();
	let base = shaper.appearance.clone();
	let index = shaper.resolve_fonts(&base);
	for _ in 0..32 {
		let mut painted = base.clone();
		painted.paint = Paint::Text;
		painted.size = 3.;
		painted.background = Some(Paint::Text);
		assert_eq!(shaper.resolve_fonts(&painted), index);
	}
	let mut bold = base.clone();
	bold.weight = 700;
	assert_ne!(shaper.resolve_fonts(&bold), index);
	assert_eq!(shaper.faces.len(), 1);
	assert_eq!(shaper.faces[&base.font].len(), 2);
	shaper.set_stylesheet(shaper.stylesheet.clone());
	assert!(shaper.faces.is_empty());
	assert!(shaper.font_sets.is_empty());
	shaper.resolve_fonts(&base);
	let mut config = shaper.font_config().clone();
	config.revision = config.revision.wrapping_add(1);
	shaper.set_fonts(&config);
	assert!(shaper.faces.is_empty());
	assert!(shaper.font_sets.is_empty());
	assert!(shaper.fallbacks.is_empty());
}

#[test]
fn collection_fallback_keys_remain_bounded_across_style_groups() {
	let mut shaper = TextShaper::new();
	for weight in [100, 400, 700] {
		let mut appearance = shaper.appearance.clone();
		appearance.weight = weight;
		let set = shaper.resolve_fonts(&appearance);
		shaper.collection_fallback(set, "\u{10ffff}");
		let entries =
			shaper.fallbacks.values().map(HashMap::len).sum::<usize>();
		for _ in 0..10 {
			shaper.collection_fallback(set, "\u{10ffff}");
		}
		assert_eq!(
			shaper.fallbacks.values().map(HashMap::len).sum::<usize>(),
			entries
		);
	}
	assert_eq!(shaper.fallbacks.len(), 3);
	let choices = shaper.fallbacks.get_mut(&(0, 100)).unwrap();
	for i in 0..4093 {
		choices.insert(format!("cached-{i}"), None);
	}
	assert_eq!(
		shaper.fallbacks.values().map(HashMap::len).sum::<usize>(),
		4096
	);
	let mut appearance = shaper.appearance.clone();
	appearance.weight = 900;
	let set = shaper.resolve_fonts(&appearance);
	assert!(shaper.collection_fallback(set, "\u{10ffff}").is_none());
	assert!(!shaper.fallbacks.contains_key(&(0, 900)));
	assert_eq!(
		shaper.fallbacks.values().map(HashMap::len).sum::<usize>(),
		4096
	);
}

proptest! {
	#![proptest_config(ProptestConfig::with_cases(48))]
	#[test]
	fn borrowed_font_keys_preserve_the_original_stack_weight_identity(
		requests in prop::collection::vec((any::<bool>(), prop::sample::select(vec![100u16, 400, 500, 700, 900])), 1..32),
	) {
		let mut shaper = TextShaper::new();
		let mut expected = HashMap::new();
		for (reverse, weight) in requests {
			let mut appearance = shaper.appearance.clone();
			if reverse { appearance.font.reverse(); }
			appearance.weight = weight;
			let index = shaper.resolve_fonts(&appearance);
			let key = (appearance.font.clone(), weight);
			prop_assert_eq!(index, *expected.entry(key).or_insert(index));
			prop_assert_eq!(shaper.font_sets.len(), expected.len());
		}
	}
}
