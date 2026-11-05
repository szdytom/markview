use super::*;
use proptest::prelude::*;

proptest! {
	#[test]
	fn clipboard_validation_and_heading_titles_follow_scalar_and_control_boundaries(
		characters in prop::collection::vec(any::<char>(), 0..128),
		words in prop::collection::vec(prop::sample::select(vec!["a", "β", "中", "😀", "é"]), 1..90), level in 1usize..7, closing in 0usize..8,
	) {
		let text: String = characters.into_iter().collect();
		let trimmed = text.trim();
		let controls = trimmed.chars().any(|ch| ch.is_control() && ch != '\t' && ch != '\r' && ch != '\n');
		let replacements = trimmed.chars().fold(0usize, |count, ch| count + usize::from(ch == '\u{fffd}'));
		prop_assert_eq!(looks_like_markdown(&text), !trimmed.is_empty() && !controls && replacements < 2);
		let title = words.concat();
		let source = format!("First sentence.\n\n{} **{title}** {}\n\n# Later\n", "#".repeat(level), "#".repeat(closing));
		let expected: String = title.chars().take(72).collect();
		for language in [crate::lang::Lang::En, crate::lang::Lang::ZhHans, crate::lang::Lang::ZhHant, crate::lang::Lang::Ja] { prop_assert_eq!(title_for(&source, language), expected.clone()); }
	}
}
