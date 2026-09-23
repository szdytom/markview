//! Compile-time interface strings.
//!
//! [`locales!`] reads one flat TOML file per language from a directory next to
//! the calling crate's manifest, checks that every file declares the same keys
//! with the same `{placeholders}`, and expands to one method per key on the
//! language enum the invocation names. The reader keeps its interface text as
//! editable TOML without parsing, allocating or looking anything up at runtime:
//! each method hands back a `&'static str` the compiler already knows.
//!
//! The emitted code also embeds every file through `include_bytes!`, so editing
//! a locale rebuilds the crate that uses it.
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Expands to an `impl` of the calling crate's language enum.
///
/// The first argument is the directory holding the locale files, relative to
/// the manifest directory of the crate that invokes the macro. Every further
/// argument names one enum variant and the file that fills it:
///
/// ```ignore
/// markview_i18n::locales!("assets/locales", En = "en.toml", ZhHans = "zh-Hans.toml");
/// ```
///
/// Every file declares the same keys. A key is a lowercase `snake_case`
/// identifier and becomes the method name; its value is the text, where
/// `{name}` marks a parameter the method takes. The first declaration is the
/// base: it is the one a missing key is reported against.
#[proc_macro]
pub fn locales(input: TokenStream) -> TokenStream {
	let args = syn::parse_macro_input!(input as Args);
	match expand(&args) {
		Ok(expansion) => expansion.into(),
		Err(error) => error.to_compile_error().into(),
	}
}

/// The macro's arguments: a directory and one file per language.
struct Args {
	directory: syn::LitStr,
	locales: Vec<(syn::Ident, syn::LitStr)>,
}
impl syn::parse::Parse for Args {
	fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
		let directory: syn::LitStr = input.parse()?;
		let mut locales = Vec::new();
		while !input.is_empty() {
			input.parse::<syn::Token![,]>()?;
			if input.is_empty() {
				break;
			}
			let variant = input.parse()?;
			input.parse::<syn::Token![=]>()?;
			let file = input.parse()?;
			locales.push((variant, file));
		}
		if locales.is_empty() {
			return Err(syn::Error::new(
				directory.span(),
				"declare at least one language",
			));
		}
		Ok(Self { directory, locales })
	}
}

/// One language, read and parsed.
struct Locale {
	variant: syn::Ident,
	file: syn::LitStr,
	/// The file's path relative to the manifest directory, for messages.
	relative: String,
	entries: BTreeMap<String, String>,
}

fn expand(args: &Args) -> syn::Result<proc_macro2::TokenStream> {
	let root = std::env::var("CARGO_MANIFEST_DIR")
		.expect("cargo sets CARGO_MANIFEST_DIR for the crate being compiled");
	let directory = args.directory.value();
	let mut locales = Vec::new();
	for (variant, file) in &args.locales {
		let relative = format!("{directory}/{}", file.value());
		let source =
			std::fs::read_to_string(PathBuf::from(&root).join(&relative))
				.map_err(|error| {
					syn::Error::new(
						file.span(),
						format!("cannot read {relative}: {error}"),
					)
				})?;
		let entries = parse_entries(&source).map_err(|message| {
			syn::Error::new(file.span(), format!("{relative}: {message}"))
		})?;
		locales.push(Locale {
			variant: variant.clone(),
			file: file.clone(),
			relative,
			entries,
		});
	}
	let base = &locales[0];
	for locale in &locales[1..] {
		for key in base.entries.keys() {
			if !locale.entries.contains_key(key) {
				return Err(syn::Error::new(
					locale.file.span(),
					format!("{} has no `{key}`", locale.relative),
				));
			}
		}
		for key in locale.entries.keys() {
			if !base.entries.contains_key(key) {
				return Err(syn::Error::new(
					locale.file.span(),
					format!(
						"{} declares `{key}`, which {} does not",
						locale.relative, base.relative
					),
				));
			}
		}
	}
	let methods = base
		.entries
		.keys()
		.map(|key| method(key, &locales))
		.collect::<syn::Result<Vec<_>>>()?;
	let relatives = locales.iter().map(|locale| &locale.relative);
	let variants = locales.iter().map(|locale| &locale.variant);
	// Two languages share a spelling wherever the text is a proper noun, a size
	// or a unit, which the reader intends rather than a mistake.
	Ok(quote! {
		#[allow(clippy::match_same_arms)]
		impl Lang {
			/// Every language this build carries, in the order the invocation
			/// declares them: the order a chooser offers them in.
			pub const ALL: &'static [Self] = &[#(Self::#variants),*];

			#(#methods)*
		}
		#(
			const _: &[u8] = include_bytes!(concat!(
				env!("CARGO_MANIFEST_DIR"),
				"/",
				#relatives
			));
		)*
	})
}

/// One accessor, taking a parameter per `{placeholder}` it spells.
fn method(
	key: &str,
	locales: &[Locale],
) -> syn::Result<proc_macro2::TokenStream> {
	let name = format_ident!("{key}");
	let values = locales
		.iter()
		.map(|locale| locale.entries[key].as_str())
		.collect::<Vec<_>>();
	let parameters = placeholders(values[0]).map_err(|message| {
		syn::Error::new(
			locales[0].file.span(),
			format!("{}: `{key}` {message}", locales[0].relative),
		)
	})?;
	for (locale, value) in locales.iter().zip(&values).skip(1) {
		let found = placeholders(value).map_err(|message| {
			syn::Error::new(
				locale.file.span(),
				format!("{}: `{key}` {message}", locale.relative),
			)
		})?;
		if found != parameters {
			return Err(syn::Error::new(
				locale.file.span(),
				format!(
					"{}: `{key}` spells {parameters:?}, not {found:?}",
					locale.relative
				),
			));
		}
	}
	let arguments = parameters.iter().map(|parameter| {
		let parameter = format_ident!("{parameter}");
		quote!(#parameter: impl ::core::fmt::Display)
	});
	let arms = locales.iter().zip(&values).map(|(locale, value)| {
		let variant = &locale.variant;
		let text = syn::LitStr::new(value, locale.file.span());
		if parameters.is_empty() {
			quote!(Self::#variant => #text)
		} else {
			quote!(Self::#variant => ::std::format!(#text))
		}
	});
	let output = if parameters.is_empty() {
		quote!(&'static str)
	} else {
		quote!(::std::string::String)
	};
	Ok(quote! {
		pub fn #name(self, #(#arguments),*) -> #output {
			match self {
				#(#arms,)*
			}
		}
	})
}

/// Reads a flat `key = "value"` table, rejecting anything else.
fn parse_entries(source: &str) -> Result<BTreeMap<String, String>, String> {
	let document = source
		.parse::<toml_edit::DocumentMut>()
		.map_err(|error: toml_edit::TomlError| error.to_string())?;
	let mut entries = BTreeMap::new();
	for (key, item) in document.iter() {
		if !is_identifier(key) {
			return Err(format!("`{key}` is not a snake_case key"));
		}
		let Some(value) = item.as_str() else {
			return Err(format!("`{key}` is not a string"));
		};
		entries.insert(key.to_owned(), value.to_owned());
	}
	Ok(entries)
}

/// The `{name}` parameters a value spells, in the order they first appear.
fn placeholders(value: &str) -> Result<Vec<String>, String> {
	let mut names: Vec<String> = Vec::new();
	let mut rest = value;
	while let Some(open) = rest.find('{') {
		let after = &rest[open + 1..];
		let Some(close) = after.find('}') else {
			return Err("has an unclosed `{`".into());
		};
		let name = &after[..close];
		if !is_identifier(name) {
			return Err(format!("spells `{{{name}}}`, which is not a name"));
		}
		if !names.iter().any(|seen| seen == name) {
			names.push(name.to_owned());
		}
		rest = &after[close + 1..];
	}
	if rest.contains('}') {
		return Err("has an unmatched `}`".into());
	}
	Ok(names)
}

/// Whether `name` is a key or a placeholder: a method name the compiler takes.
fn is_identifier(name: &str) -> bool {
	let mut characters = name.chars();
	matches!(characters.next(), Some('a'..='z'))
		&& characters
			.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

#[cfg(test)]
mod tests {
	use super::*;

	/// A key becomes a method, so anything the compiler would not take as a
	/// name has to be refused while the locale is read.
	#[test]
	fn keys_are_snake_case_names() {
		assert!(is_identifier("status_exported_png"));
		assert!(is_identifier("a1"));
		assert!(!is_identifier(""));
		assert!(!is_identifier("Status"));
		assert!(!is_identifier("_status"));
		assert!(!is_identifier("status-exported"));
		assert!(!is_identifier("状态"));
	}

	/// A placeholder becomes a parameter, and its order is the order the
	/// method takes them in, so a repeat must be named once.
	#[test]
	fn placeholders_read_in_first_appearance_order() {
		assert_eq!(
			placeholders("no parameters").unwrap(),
			Vec::<String>::new()
		);
		assert_eq!(
			placeholders("{width}×{height} mm").unwrap(),
			vec!["width".to_owned(), "height".to_owned()]
		);
		assert_eq!(
			placeholders("{a} then {b} then {a}").unwrap(),
			vec!["a".to_owned(), "b".to_owned()]
		);
	}

	/// A file the reader cannot turn into text is a mistake in the file, and
	/// the message has to say which one.
	#[test]
	fn a_broken_placeholder_is_rejected() {
		assert!(placeholders("{width").is_err());
		assert!(placeholders("trailing }").is_err());
		assert!(placeholders("{}").is_err());
		assert!(placeholders("{two words}").is_err());
	}

	/// Only a flat table of strings is a locale; a nested table, a number or
	/// a badly named key is not.
	#[test]
	fn a_locale_is_a_flat_table_of_strings() {
		let entries = parse_entries("a = \"one\"\nb = \"two\"").unwrap();
		assert_eq!(entries.get("a").map(String::as_str), Some("one"));
		assert_eq!(entries.get("b").map(String::as_str), Some("two"));

		assert!(parse_entries("a = 1").is_err());
		assert!(parse_entries("a = [\"x\"]").is_err());
		assert!(parse_entries("[section]\na = \"one\"").is_err());
		assert!(parse_entries("Bad = \"one\"").is_err());
		assert!(parse_entries("not toml at all").is_err());
	}
}
