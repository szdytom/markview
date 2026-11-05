use super::*;
use proptest::prelude::*;

proptest! {
	#[test]
	fn template_expansion_preserves_literals_and_number_runs(
		parts in prop::collection::vec((0u8..5, "[a-z你😀 }]{0,12}"), 0..24),
		page in any::<usize>(), pages in any::<usize>(), title in ".{0,24}", path in ".{0,24}",
	) {
		let title = format!("{{page}}{title}");
		let path = format!("{path}{{title}}");
		prop_assert!(template_is_valid(""));
		prop_assert!(expand_template("", page, pages, &title, &path).is_empty());
		let mut template = String::new();
		let mut expected = String::new();
		let mut numbers = Vec::new();
		let directed = [(0, "literal".to_owned()), (1, String::new()), (2, String::new()), (3, String::new()), (4, String::new())];
		for (kind, literal) in directed.into_iter().chain(parts) {
			let (syntax, value) = match kind {
				0 => (literal.clone(), literal),
				1 => ("{page}".to_owned(), page.to_string()),
				2 => ("{pages}".to_owned(), pages.to_string()),
				3 => ("{title}".to_owned(), title.clone()),
				_ => ("{path}".to_owned(), path.clone()),
			};
			if matches!(kind, 1 | 2) { numbers.push(value.clone()); }
			template.push_str(&syntax);
			expected.push_str(&value);
		}
		prop_assert!(template_is_valid(&template));
		let segments = expand_template(&template, page, pages, &title, &path);
		let actual: String = segments.iter().map(|segment| match segment { SlotSegment::Text(t) | SlotSegment::Number(t) => t.as_str() }).collect();
		let actual_numbers: Vec<_> = segments.iter().filter_map(|segment| match segment { SlotSegment::Number(t) => Some(t.clone()), _ => None }).collect();
		prop_assert_eq!(actual, expected);
		prop_assert_eq!(actual_numbers, numbers);
		prop_assert!(segments.iter().all(|segment| !matches!(segment, SlotSegment::Text(t) if t.is_empty())));
		prop_assert!(segments.windows(2).all(|pair| !matches!(pair, [SlotSegment::Text(_), SlotSegment::Text(_)])));
	}

	#[test]
	fn unknown_and_unclosed_placeholders_are_rejected(literal in "[a-z你😀 ]{0,16}", name in "[a-z]{0,12}") {
		for malformed in [format!("{literal}{{{name}wrong}}"), format!("{literal}{{page"), format!("{{{{page}}{literal}")] {
			prop_assert!(!template_is_valid(&malformed), "accepted {malformed:?}");
		}
	}
}
