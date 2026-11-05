use super::*;
use proptest::prelude::*;

#[derive(Clone, Debug)]
enum Predicate {
	Leaf(u8),
	Branch(bool, Vec<Predicate>),
}

impl Predicate {
	fn source(&self) -> String {
		match self {
			Self::Leaf(kind) => [
				"test",
				"feature = \"test-support\"",
				"unix",
				"feature = \"ordinary\"",
			][usize::from(*kind)]
			.to_owned(),
			Self::Branch(all, children) => format!(
				"{}({})",
				if *all { "all" } else { "any" },
				children
					.iter()
					.map(Self::source)
					.collect::<Vec<_>>()
					.join(",")
			),
		}
	}
	fn production(&self) -> bool {
		match self {
			Self::Leaf(kind) => *kind >= 2,
			Self::Branch(true, children) => {
				children.iter().all(Self::production)
			}
			Self::Branch(false, children) => {
				children.iter().any(Self::production)
			}
		}
	}
}

fn predicate() -> impl Strategy<Value = Predicate> {
	(0u8..4)
		.prop_map(Predicate::Leaf)
		.prop_recursive(3, 24, 3, |child| {
			(any::<bool>(), prop::collection::vec(child, 1..4))
				.prop_map(|(all, children)| Predicate::Branch(all, children))
		})
}

fn fragment(kind: u8, count: u8) -> (String, usize) {
	match kind {
		0 => ("if p {}".to_owned(), 1),
		1 => ("while p {}".to_owned(), 1),
		2 => ("for x in xs {}".to_owned(), 1),
		3 => ("value?;".to_owned(), 1),
		4 => (
			format!(
				"let _ = {};",
				vec!["p"; usize::from(count) + 1].join(" && ")
			),
			usize::from(count),
		),
		5 => (
			format!(
				"let _ = {};",
				vec!["p"; usize::from(count) + 1].join(" || ")
			),
			usize::from(count),
		),
		_ => {
			let arms = (0..count)
				.map(|index| format!("{index} => (),"))
				.collect::<String>();
			(
				format!("match x {{ {arms} _ if p => (), _ => () }};"),
				usize::from(count) + 2,
			)
		}
	}
}

proptest! {
	#[test]
	fn positive_cfg_predicates_filter_only_items_that_require_test_configuration(
		predicates in prop::collection::vec(predicate(), 1..5),
	) {
		let attributes = predicates.iter().map(|predicate| format!("#[cfg({})]", predicate.source())).collect::<String>();
		let expected = !predicates.iter().all(Predicate::production);
		let source = format!("{attributes} fn fixture() {{}} {attributes} mod external;");
		let syntax = syn::parse_file(&source).unwrap();
		let syn::Item::Fn(function) = &syntax.items[0] else { unreachable!() };
		prop_assert_eq!(test_only(&function.attrs), expected);
		let mut pending = Vec::new();
		let functions = measure(Path::new("src/lib.rs"), &syntax, &mut pending).unwrap();
		prop_assert_eq!(functions.len(), usize::from(!expected));
		prop_assert_eq!(pending.len(), usize::from(!expected));
		if !expected { prop_assert_eq!(&functions[0].name, "fixture"); }
	}

	#[test]
	fn complexity_scores_compose_fragments_without_counting_nested_function_bodies_twice(
		plans in prop::collection::vec(prop::collection::vec((0u8..7, 0u8..5), 0..10), 1..6),
	) {
		let mut outer = String::new();
		let mut expected = Vec::new();
		let mut outer_score = 1;
		for &(kind, count) in &plans[0] { let (text, score) = fragment(kind, count); outer.push_str(&text); outer_score += score; }
		for (index, plan) in plans.iter().enumerate().skip(1) {
			let mut body = String::new();
			let mut score = 1;
			for &(kind, count) in plan { let (text, decisions) = fragment(kind, count); body.push_str(&text); score += decisions; }
			if index % 2 == 0 { outer.push_str(&format!("let closure_{index} = || {{ {body} }};")); expected.push(("<closure>".to_owned(), score)); }
			else { let name = format!("child_{index}"); outer.push_str(&format!("fn {name}() {{ {body} }}")); expected.push((name, score)); }
		}
		expected.push(("outer".to_owned(), outer_score));
		let source = format!("fn outer() {{ {outer} }}");
		let actual = measure(Path::new("src/lib.rs"), &syn::parse_file(&source).unwrap(), &mut Vec::new()).unwrap();
		prop_assert_eq!(actual.iter().map(|function| (function.name.clone(), function.score)).collect::<Vec<_>>(), expected);
	}

	#[test]
	fn exemption_reasons_keep_exact_unicode_and_malformed_markers_never_disable_the_limit(
		characters in prop::collection::vec(any::<char>(), 0..32),
	) {
		let reason = format!("why: {}", characters.into_iter().collect::<String>());
		for marker in [format!("markview::allow_complexity(reason = {reason:?})"), "markview::allow_complexity(reason = \"\")".to_owned(), format!("markview::allow_complexity(reason = {reason:?}, reason = \"duplicate\")")] {
			let source = format!("#[cfg_attr(any(), {marker})] fn fixture() {{ if p {{}} }}");
			let result = measure(Path::new("src/lib.rs"), &syn::parse_file(&source).unwrap(), &mut Vec::new());
			if marker == format!("markview::allow_complexity(reason = {reason:?})") {
				let functions = result.unwrap();
				prop_assert_eq!(functions[0].exemption.as_deref(), Some(reason.as_str()));
				prop_assert!(!functions[0].exceeds(1));
			} else { prop_assert!(result.is_err()); }
		}
	}
}
