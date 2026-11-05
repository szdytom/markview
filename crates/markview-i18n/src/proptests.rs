use super::*;
use proc_macro2::Span;
use proptest::prelude::*;

fn text() -> impl Strategy<Value = String> {
	prop::collection::vec(
		prop::sample::select(vec![
			"a", "你", "😀", " ", "\n", "\r", "\t", "\"", "\\",
		]),
		0..20,
	)
	.prop_map(|parts| parts.concat())
}

fn quoted(text: &str) -> String {
	let mut out = String::from("\"");
	for c in text.chars() {
		let escaped = match c {
			'"' => "\\\"",
			'\\' => "\\\\",
			'\n' => "\\n",
			'\r' => "\\r",
			'\t' => "\\t",
			_ => {
				out.push(c);
				continue;
			}
		};
		out.push_str(escaped);
	}
	out.push('"');
	out
}

fn locale(variant: &str, value: String) -> Locale {
	Locale {
		variant: format_ident!("{variant}"),
		file: syn::LitStr::new("locale.toml", Span::call_site()),
		relative: "locale.toml".to_owned(),
		entries: BTreeMap::from([("message".to_owned(), value)]),
	}
}

proptest! {
	#[test]
	fn placeholders_preserve_first_appearance_order_and_ignore_repeats(
		parameters in prop::collection::vec(0u8..8, 0..24), literal in "[a-z你😀 ]{0,16}",
	) {
		prop_assert_eq!(placeholders(&literal).unwrap(), Vec::<String>::new());
		let mut expected = Vec::new();
		let mut template = literal;
		for parameter in [0, 1, 0].into_iter().chain(parameters) {
			let name = format!("p{parameter}");
			if !expected.contains(&name) { expected.push(name.clone()); }
			template.push_str(&format!(" {{{name}}} "));
		}
		prop_assert_eq!(placeholders(&template).unwrap(), expected);
		for malformed in [format!("{template}{{"), format!("{template}}}"), format!("{template}{{bad name}}"), format!("{template}{{}}"), format!("{template}{{{{p0}}")] {
			prop_assert!(placeholders(&malformed).is_err(), "accepted {malformed:?}");
		}
	}

	#[test]
	fn flat_locale_tables_preserve_unicode_strings_and_escaped_controls(
		entries in prop::collection::btree_map("key_[a-z0-9_]{0,12}", text(), 0..12),
	) {
		let source: String = entries.iter().rev().map(|(key, value)| format!("{key} = {}\n", quoted(value))).collect();
		prop_assert_eq!(parse_entries(&source).unwrap(), entries);
	}

	#[test]
	fn locale_tables_reject_non_strings_nested_tables_and_invalid_names(value in text()) {
		for invalid in ["1", "true", "[]", "[\"text\"]", "{ child = \"text\" }"] {
			prop_assert!(parse_entries(&format!("key = {invalid}")).is_err(), "accepted non-string value");
		}
		for name in ["", "Bad", "_key", "a-b", "你", "1key"] {
			prop_assert!(parse_entries(&format!("{} = {}", quoted(name), quoted(&value))).is_err(), "accepted invalid name");
		}
		prop_assert!(parse_entries(&format!("[nested]\nkey = {}", quoted(&value))).is_err(), "accepted nested table");
	}

	#[test]
	fn locale_methods_have_one_display_argument_per_declared_parameter(
		parameters in prop::collection::vec(0u8..8, 0..20), prefix in "[a-z你😀 ]{0,16}",
	) {
		let mut expected = Vec::new();
		let mut template = String::new();
		for parameter in parameters {
			let name = format!("p{parameter}");
			if !expected.contains(&name) { expected.push(name.clone()); }
			template.push_str(&format!("{{{name}}}"));
		}
		let locales = [locale("En", format!("{prefix}{template}")), locale("Zh", format!("译 {template}"))];
		let tokens = method("message", &locales).unwrap();
		let syntax: syn::ImplItemFn = syn::parse2(tokens).unwrap();
		prop_assert_eq!(syntax.sig.ident.to_string(), "message");
		let mut arguments = Vec::new();
		for argument in syntax.sig.inputs.iter().skip(1) {
			let syn::FnArg::Typed(argument) = argument else { unreachable!() };
			let syn::Pat::Ident(name) = &*argument.pat else { unreachable!() };
			let syn::Type::ImplTrait(ty) = &*argument.ty else { unreachable!() };
			prop_assert_eq!(ty.bounds.len(), 1);
			let syn::TypeParamBound::Trait(bound) = &ty.bounds[0] else { unreachable!() };
			let path: Vec<_> = bound.path.segments.iter().map(|segment| segment.ident.to_string()).collect();
			prop_assert_eq!(path, ["core", "fmt", "Display"]);
			arguments.push(name.ident.to_string());
		}
		prop_assert_eq!(arguments, expected);
		let wrong = [locale("En", template.clone()), locale("Zh", format!("{template}{{extra}}"))];
		prop_assert!(method("message", &wrong).is_err());
	}

	#[test]
	#[ignore = "Known bug PBT-006: locale keys that are Rust keywords pass identifier validation"]
	fn rust_keywords_are_rejected_before_generating_locale_methods(
		keyword in prop::sample::select(vec!["type", "match", "fn", "self"]),
	) {
		let source = format!("{keyword} = \"text\"");
		let syntax = format!("pub fn {keyword}(self) -> &'static str {{ \"text\" }}");
		prop_assert!(syn::parse_str::<syn::ImplItemFn>(&syntax).is_err());
		prop_assert!(parse_entries(&source).is_err(), "accepted reserved key: {keyword}");
	}

	#[test]
	#[ignore = "Known bug PBT-005: an unmatched closing brace before a placeholder is skipped"]
	fn unmatched_closing_braces_before_placeholders_are_rejected(prefix in "[a-z]{0,8}", parameter in "p[a-z]{0,8}") {
		let template = format!("{prefix}}}{{{parameter}}}");
		prop_assert!(placeholders(&template).is_err(), "accepted {template:?}");
	}
}
