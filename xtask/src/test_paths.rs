//! Keep real-file fixtures on canonical storage throughout the repository.
use std::{
	collections::BTreeSet,
	fs,
	path::{Path, PathBuf},
	process::Command,
};

use anyhow::{Context, Result, ensure};
use proc_macro2::{Span, TokenStream, TokenTree};
use syn::{
	Attribute, Meta, Token, UseTree,
	punctuated::Punctuated,
	spanned::Spanned,
	visit::{self, Visit},
};

pub fn run() -> Result<()> {
	let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
	let sources = source_paths(root)?;
	let mut reports = 0;
	for source in &sources {
		let text = fs::read_to_string(root.join(source))?;
		let lines =
			audit(&text).with_context(|| source.display().to_string())?;
		for line in lines {
			eprintln!(
				"{}:{line}: use markview_test_support::TestDir for real files; raw-path contracts require #[cfg_attr(any(), markview::allow_raw_paths(reason = \"...\"))]",
				source.display()
			);
			reports += 1;
		}
	}
	ensure!(reports == 0, "test path check failed: {reports} reports");
	println!(
		"test paths: {} repository Rust source files checked",
		sources.len()
	);
	Ok(())
}

fn source_paths(root: &Path) -> Result<Vec<PathBuf>> {
	let output = Command::new("git")
		.args([
			"ls-files",
			"--cached",
			"--others",
			"--exclude-standard",
			"-z",
			"--",
			"*.rs",
		])
		.current_dir(root)
		.output()?;
	ensure!(
		output.status.success(),
		"git ls-files: {}",
		String::from_utf8_lossy(&output.stderr)
	);
	let mut paths = BTreeSet::new();
	for path in std::str::from_utf8(&output.stdout)?
		.split('\0')
		.filter(|path| !path.is_empty())
	{
		if root.join(path).is_file() {
			paths.insert(PathBuf::from(path));
		}
	}
	Ok(paths.into_iter().collect())
}

fn audit(source: &str) -> Result<BTreeSet<usize>> {
	if !["tempfile", "tempdir", "allow_raw_paths"]
		.iter()
		.any(|name| source.contains(name))
	{
		return Ok(BTreeSet::new());
	}
	let syntax = syn::parse_file(source)?;
	let mut check = Check::default();
	check.visit_file(&syntax);
	if let Some(error) = check.error {
		return Err(error);
	}
	Ok(check.lines)
}

#[derive(Default)]
struct Check {
	lines: BTreeSet<usize>,
	exempt: bool,
	error: Option<anyhow::Error>,
}

impl Check {
	fn report(&mut self, span: Span) {
		if !self.exempt {
			self.lines.insert(span.start().line);
		}
	}

	fn function(&mut self, attrs: &[Attribute], visit: impl FnOnce(&mut Self)) {
		let exempt = match exemption(attrs) {
			Ok(exempt) => exempt,
			Err(error) => {
				self.error.get_or_insert(error);
				false
			}
		};
		let previous = std::mem::replace(&mut self.exempt, exempt);
		visit(self);
		self.exempt = previous;
	}

	fn imports(&mut self, tree: &UseTree) {
		match tree {
			UseTree::Path(path) if path.ident == "tempfile" => {
				self.directory_import(&path.tree)
			}
			UseTree::Name(name) if name.ident == "tempfile" => {
				self.report(name.span())
			}
			UseTree::Rename(rename) if rename.ident == "tempfile" => {
				self.report(rename.span())
			}
			UseTree::Group(group) => {
				for item in &group.items {
					self.imports(item);
				}
			}
			_ => {}
		}
	}

	fn directory_import(&mut self, tree: &UseTree) {
		match tree {
			UseTree::Group(group) => {
				for item in &group.items {
					self.directory_import(item);
				}
			}
			UseTree::Path(path)
				if raw_directory_name(&path.ident.to_string()) =>
			{
				self.report(path.span())
			}
			UseTree::Name(name)
				if raw_directory_name(&name.ident.to_string())
					|| name.ident == "self" =>
			{
				self.report(name.span())
			}
			UseTree::Rename(rename)
				if raw_directory_name(&rename.ident.to_string())
					|| rename.ident == "self" =>
			{
				self.report(rename.span())
			}
			UseTree::Glob(glob) => self.report(glob.span()),
			_ => {}
		}
	}

	fn tokens(&mut self, tokens: TokenStream) {
		let tokens: Vec<_> = tokens.into_iter().collect();
		for (index, token) in tokens.iter().enumerate() {
			if let TokenTree::Group(group) = token {
				self.tokens(group.stream());
			}
			if let Some(span) = raw_reference(&tokens[index..]) {
				self.report(span);
			}
		}
	}
}

fn raw_directory_name(name: &str) -> bool {
	matches!(name, "tempdir" | "tempdir_in" | "TempDir")
}

fn directory_method(name: &str) -> bool {
	matches!(name, "tempdir" | "tempdir_in")
}

fn raw_reference(tokens: &[TokenTree]) -> Option<Span> {
	match tokens {
		[
			TokenTree::Ident(root),
			TokenTree::Punct(first),
			TokenTree::Punct(second),
			TokenTree::Ident(name),
			tail @ ..,
		] if root == "tempfile"
			&& first.as_char() == ':'
			&& second.as_char() == ':'
			&& raw_directory_name(&name.to_string())
			&& constructor_reference(name, tail) =>
		{
			Some(root.span())
		}
		[
			TokenTree::Punct(dot),
			TokenTree::Ident(name),
			TokenTree::Group(arguments),
			..,
		] if dot.as_char() == '.'
			&& directory_method(&name.to_string())
			&& arguments.delimiter() == proc_macro2::Delimiter::Parenthesis =>
		{
			Some(name.span())
		}
		_ => None,
	}
}

fn constructor_reference(
	name: &proc_macro2::Ident,
	tail: &[TokenTree],
) -> bool {
	name != "TempDir"
		|| matches!(tail, [TokenTree::Punct(first), TokenTree::Punct(second), ..] if first.as_char() == ':' && second.as_char() == ':')
}

impl<'ast> Visit<'ast> for Check {
	fn visit_expr_path(&mut self, expression: &'ast syn::ExprPath) {
		let path = &expression.path;
		if path
			.segments
			.first()
			.is_some_and(|segment| segment.ident == "tempfile")
			&& path.segments.get(1).is_some_and(|segment| {
				raw_directory_name(&segment.ident.to_string())
			}) {
			self.report(path.span());
		}
		visit::visit_expr_path(self, expression);
	}

	fn visit_expr_method_call(
		&mut self,
		expression: &'ast syn::ExprMethodCall,
	) {
		if directory_method(&expression.method.to_string()) {
			self.report(expression.method.span());
		}
		visit::visit_expr_method_call(self, expression);
	}

	fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
		self.imports(&item.tree);
	}

	fn visit_item_extern_crate(&mut self, item: &'ast syn::ItemExternCrate) {
		if item.ident == "tempfile" {
			self.report(item.span());
		}
	}

	fn visit_macro(&mut self, mac: &'ast syn::Macro) {
		self.tokens(mac.tokens.clone());
	}

	fn visit_attribute(&mut self, _: &'ast Attribute) {}

	fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
		self.function(&item.attrs, |check| visit::visit_item_fn(check, item));
	}

	fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
		self.function(&item.attrs, |check| {
			visit::visit_impl_item_fn(check, item)
		});
	}

	fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
		self.function(&item.attrs, |check| {
			visit::visit_trait_item_fn(check, item)
		});
	}
}

fn exemption(attrs: &[Attribute]) -> Result<bool> {
	let mut exempt = false;
	for attr in attrs.iter().filter(|attr| attr.path().is_ident("cfg_attr")) {
		exempt |= raw_marker(attr).with_context(|| {
			format!(
				"line {}: invalid allow_raw_paths marker",
				attr.span().start().line
			)
		})?;
	}
	Ok(exempt)
}

fn raw_marker(attr: &Attribute) -> Result<bool> {
	let args =
		attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
	let args: Vec<_> = args.iter().collect();
	let [condition, Meta::List(marker)] = args.as_slice() else {
		return Ok(false);
	};
	if !marker
		.path
		.segments
		.iter()
		.map(|segment| segment.ident.to_string())
		.eq(["markview", "allow_raw_paths"])
	{
		return Ok(false);
	}
	ensure!(
		matches!(condition, Meta::List(list) if list.path.is_ident("any") && list.tokens.is_empty()),
		"allow_raw_paths must use cfg_attr(any(), ...)"
	);
	let reason: syn::MetaNameValue = marker.parse_args()?;
	ensure!(
		reason.path.is_ident("reason"),
		"allow_raw_paths requires reason = \"...\""
	);
	let syn::Expr::Lit(syn::ExprLit {
		lit: syn::Lit::Str(reason),
		..
	}) = reason.value
	else {
		anyhow::bail!("allow_raw_paths reason must be a nonempty string");
	};
	ensure!(
		!reason.value().trim().is_empty(),
		"allow_raw_paths reason must be a nonempty string"
	);
	Ok(true)
}

#[cfg(test)]
mod tests {
	use super::*;
	use proptest::prelude::*;

	const MARKER: &str = r#"#[cfg_attr(any(), markview::allow_raw_paths(reason = "Exercise a raw input"))]"#;

	#[test]
	fn rejects_raw_constructors_imports_and_macro_arguments() {
		for code in [
			"let dir = tempfile::tempdir().unwrap();",
			"let dir = ::tempfile::tempdir_in(root).unwrap();",
			"let dir = tempfile::TempDir::new().unwrap();",
			"let dir = tempfile::Builder::new().tempdir().unwrap();",
			"use tempfile::tempdir; let dir = tempdir().unwrap();",
			"use tempfile::tempdir as make; let dir = make().unwrap();",
			"use tempfile::Builder as Scratch; let dir = Scratch::new().tempdir();",
			"use tempfile as tmp; let dir = tmp::tempdir().unwrap();",
			"use {tempfile::{TempDir as Directory}};",
			"extern crate tempfile as tmp;",
			"assert!(wrap!(tempfile::tempdir()).is_ok());",
			"assert!(tempfile::TempDir::new().is_ok());",
			"assert!(tempfile::Builder::new().tempdir().is_ok());",
		] {
			let source = format!(
				"#[cfg(test)] mod tests {{\n#[test] fn regression() {{\n{code}\n}}\n}}"
			);
			assert_eq!(audit(&source).unwrap(), BTreeSet::from([3]), "{code}");
		}
	}

	#[test]
	fn discovers_new_staged_and_platform_specific_sources_without_a_file_list()
	{
		let directory = markview_test_support::TestDir::new();
		let root = directory.path();
		assert!(
			Command::new("git")
				.args(["init", "--quiet"])
				.current_dir(root)
				.status()
				.unwrap()
				.success()
		);
		fs::write(root.join(".gitignore"), "target/\n").unwrap();
		fs::write(root.join("tracked.rs"), "fn production() {}\n").unwrap();
		assert!(
			Command::new("git")
				.args(["add", "tracked.rs"])
				.current_dir(root)
				.status()
				.unwrap()
				.success()
		);
		for name in [
			"crates/new/src/windows.rs",
			"tests/a new test.rs",
			"fuzz/fuzz_targets/new.rs",
			"target/generated.rs",
		] {
			let path = root.join(name);
			fs::create_dir_all(path.parent().unwrap()).unwrap();
			fs::write(
				path,
				"#[cfg(windows)] fn test() {\n let dir = tempfile::tempdir();\n}\n",
			)
			.unwrap();
		}
		let paths = source_paths(root).unwrap();
		assert_eq!(
			paths,
			[
				"crates/new/src/windows.rs",
				"fuzz/fuzz_targets/new.rs",
				"tests/a new test.rs",
				"tracked.rs"
			]
			.map(PathBuf::from)
		);
		for path in &paths[..3] {
			assert_eq!(
				audit(&fs::read_to_string(root.join(path)).unwrap()).unwrap(),
				BTreeSet::from([2])
			);
		}
		fs::remove_file(root.join("tracked.rs")).unwrap();
		assert_eq!(source_paths(root).unwrap().len(), 3);
	}

	#[test]
	fn permits_canonical_fixtures_virtual_paths_comments_and_literals() {
		let source = r##"
			fn fixture() {
				// tempfile::tempdir() is forbidden.
				let text = "tempfile::tempdir()";
				assert_eq!(text, r#"tempfile::tempdir()"#);
				let dir = crate::test_support::TestDir::new();
				let path = dir.path().join("note.md");
                let file = tempfile::NamedTempFile::new_in(dir.path());
                let backup = tempfile::Builder::new().tempfile_in(dir.path());
                let owned: tempfile::TempDir = production_storage();
                assert_eq!(std::mem::size_of::<tempfile::TempDir>(), 1);
                assert!(metadata.tempdir.is_some());
				let virtual_path = PathBuf::from("/display/note.md");
			}
		"##;
		assert!(audit(source).unwrap().is_empty());
	}

	#[test]
	fn exemption_requires_a_reason_and_stays_on_its_function() {
		let source = format!(
			r#"
			{MARKER}
			fn raw() {{
				let dir = tempfile::tempdir();
				fn nested() {{ let dir = tempfile::tempdir(); }}
			}}
			fn sibling() {{ let dir = tempfile::tempdir(); }}
		"#
		);
		assert_eq!(audit(&source).unwrap(), BTreeSet::from([5, 7]));
		for marker in [
			r#"#[cfg_attr(any(), markview::allow_raw_paths(reason = " "))]"#,
			r#"#[cfg_attr(any(), markview::allow_raw_paths(reason = 1))]"#,
			r#"#[cfg_attr(any(), markview::allow_raw_paths())]"#,
			r#"#[cfg_attr(unix, markview::allow_raw_paths(reason = "raw"))]"#,
		] {
			assert!(
				audit(&format!("{marker} fn raw() {{}} ")).is_err(),
				"{marker}"
			);
		}
	}

	#[test]
	fn checks_methods_and_platform_specific_test_modules() {
		let source = format!(
			r#"
			#[cfg(windows)] mod tests {{
				impl Harness {{
					{MARKER} fn raw() {{ let dir = tempfile::tempdir(); }}
					fn new() {{ let dir = tempfile::tempdir(); }}
				}}
			}}
		"#
		);
		assert_eq!(audit(&source).unwrap(), BTreeSet::from([5]));
	}

	proptest! {
		#[test]
		fn comments_and_literals_preserve_detection_and_line_numbers(
			comments in proptest::collection::vec("[ -~]{0,60}", 0..8),
			literal in "[ -~]{0,80}",
			raw in any::<bool>(),
			exempt in any::<bool>(),
		) {
			let mut source = comments.iter().map(|text| format!("// {text}\n")).collect::<String>();
			if exempt {
				source.push_str(MARKER);
				source.push('\n');
			}
			source.push_str(&format!("fn test() {{ let text = {:?};\n", format!("tempfile::{literal}")));
			let line = source.lines().count() + 1;
			source.push_str(if raw {
				"assert!(tempfile::tempdir().is_ok());\n}"
			} else {
				"let dir = crate::test_support::TestDir::new();\n}"
			});
			let expected = if raw && !exempt { BTreeSet::from([line]) } else { BTreeSet::new() };
			prop_assert_eq!(audit(&source).unwrap(), expected);
		}
	}
}
