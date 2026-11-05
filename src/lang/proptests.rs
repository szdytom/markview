use super::*;
use proptest::prelude::*;

proptest! {
	#[test]
	fn locale_spelling_preserves_the_language_and_generated_arguments(
		upper in any::<bool>(), underscores in any::<bool>(),
		label in "[a-z你😀 ]{0,16}", value in ".{0,16}",
	) {
		for (tag, expected) in [("", Lang::En), ("fr-FR", Lang::En), ("en-US", Lang::En), ("ja", Lang::Ja), ("ja-JP", Lang::Ja), ("zh", Lang::ZhHans), ("zh-CN", Lang::ZhHans), ("zh-Hans-TW", Lang::ZhHans), ("zh-Hant", Lang::ZhHant), ("zh-Hant-CN", Lang::ZhHant), ("zh-TW", Lang::ZhHant), ("zh-HK", Lang::ZhHant), ("zh-MO", Lang::ZhHant)] {
			let tag = if upper { tag.to_ascii_uppercase() } else { tag.to_owned() };
			let tag = if underscores { tag.replace('-', "_") } else { tag };
			prop_assert_eq!(Lang::from_locale(&tag), expected);
		}
		prop_assert_eq!(Lang::En.diagnostics_row(&label, &value), format!("{label}: {value}"));
		prop_assert_eq!(Lang::ZhHans.diagnostics_row(&label, &value), format!("{label}：{value}"));
	}
}
