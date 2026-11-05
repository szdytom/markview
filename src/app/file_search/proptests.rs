use super::*;
use proptest::prelude::*;

proptest! {
	#[test]
	fn match_snippets_preserve_complete_graphemes_and_mark_exactly_the_visible_matches(
		tokens in prop::collection::vec(prop::sample::select(vec!["a", "β", "中", "😀", "e\u{301}", "👩\u{200d}💻"]), 1..130),
		selected in prop::collection::vec(any::<usize>(), 1..16),
	) {
		let path = tokens.concat();
		prop_assert_eq!(path.graphemes(true).count(), tokens.len());
		let mut indices: Vec<_> = selected.into_iter().map(|index| (index % tokens.len()) as u32).collect();
		indices.sort_unstable(); indices.dedup();
		let snippet = Snippet::new(&path, &indices);
		let start = (indices[0] as usize).saturating_sub(6);
		let end = tokens.len().min(*indices.last().unwrap() as usize + 13).min(start + 96);
		let expected = format!("{}{}{}", if start > 0 { "…" } else { "" }, tokens[start..end].concat(), if end < tokens.len() { "…" } else { "" });
		prop_assert_eq!(&snippet.text, &expected);
		let mut byte = if start > 0 { "…".len() } else { 0 };
		let mut highlighted = Vec::new();
		for (index, token) in tokens.iter().enumerate().take(end).skip(start) {
			let marked = indices.contains(&(index as u32));
			for offset in byte..byte + token.len() { highlighted.push((offset, marked)); }
			byte += token.len();
		}
		for (byte, expected) in highlighted { prop_assert_eq!(snippet.matched.iter().any(|range| range.contains(&byte)), expected); }
		let boundaries: Vec<_> = snippet.text.grapheme_indices(true).map(|(index, _)| index).chain([snippet.text.len()]).collect();
		for range in &snippet.matched { prop_assert!(boundaries.contains(&range.start) && boundaries.contains(&range.end) && range.start < range.end); }
		prop_assert!(snippet.matched.windows(2).all(|pair| pair[0].end < pair[1].start));
	}
}
