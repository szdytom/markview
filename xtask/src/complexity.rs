use std::{
	collections::BTreeSet,
	fs,
	path::{Path, PathBuf},
	process::Command,
};

use anyhow::{Context, Result, bail, ensure};
use syn::{
	Attribute, Expr, Meta, Token,
	punctuated::Punctuated,
	spanned::Spanned,
	visit::{self, Visit},
};

#[derive(Debug)]
struct Function {
	line: usize,
	name: String,
	score: usize,
	exemption: Option<String>,
}

impl Function {
	fn exceeds(&self, limit: usize) -> bool {
		self.score > limit && self.exemption.is_none()
	}
}

type Module = (PathBuf, PathBuf);

/// A production workspace source file and its parsed syntax tree.
pub(crate) struct Source {
	pub path: PathBuf,
	pub syntax: syn::File,
}

/// The counts printed by one complexity pass.
#[derive(Clone, Copy)]
pub(crate) struct Summary {
	pub reports: usize,
	pub functions: usize,
	pub exemptions: usize,
}

pub fn run() -> Result<()> {
	let summary = audit(true)?;
	ensure!(summary.reports == 0, "cyclomatic complexity check failed");
	Ok(())
}

/// Audits the workspace without printing the individual findings.
pub(crate) fn summary() -> Result<Summary> {
	audit(false)
}

fn audit(verbose: bool) -> Result<Summary> {
	let output = Command::new("cargo")
		.args(["metadata", "--no-deps", "--format-version=1", "--locked"])
		.current_dir(workspace_root())
		.output()?;
	ensure!(
		output.status.success(),
		"cargo metadata: {}",
		String::from_utf8_lossy(&output.stderr)
	);
	let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)?;
	let limit = metadata["metadata"]["lint"]["cyclomatic-complexity-threshold"]
		.as_u64()
		.unwrap() as usize;
	let (root, sources) = workspace_sources()?;
	let mut reports = Vec::new();
	let mut exemptions = Vec::new();
	let mut functions = 0;
	for source in &sources {
		let measured = measure(&source.path, &source.syntax, &mut Vec::new())?;
		functions += measured.len();
		for function in measured {
			if function.exceeds(limit) {
				reports.push((
					source.path.strip_prefix(&root)?.to_owned(),
					function.line,
					function.name,
					function.score,
				));
			} else if let Some(reason) = function.exemption {
				exemptions.push((
					source.path.strip_prefix(&root)?.to_owned(),
					function.line,
					function.name,
					function.score,
					reason,
				));
			}
		}
	}
	reports.sort();
	exemptions.sort();
	if verbose {
		for (path, line, name, score) in &reports {
			println!(
				"{}:{line}: {name}: cyclomatic complexity {score} > {limit}",
				path.display()
			);
		}
		for (path, line, name, score, reason) in &exemptions {
			println!(
				"{}:{line}: {name}: cyclomatic complexity {score}; exempt: {reason}",
				path.display()
			);
		}
		println!(
			"{} reports / {functions} functions and closures; {} exemptions; maximum allowed: {limit}",
			reports.len(),
			exemptions.len()
		);
	}
	Ok(Summary {
		reports: reports.len(),
		functions,
		exemptions: exemptions.len(),
	})
}

fn workspace_root() -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.parent()
		.unwrap()
		.to_owned()
}

/// Parses every production workspace target and its reachable modules.
///
/// Test-only targets and modules are skipped, as are test-only items at the
/// root of a file. Inline test modules remain in the returned syntax tree;
/// consumers visit items through [`test_only`] to exclude them.
pub(crate) fn workspace_sources() -> Result<(PathBuf, Vec<Source>)> {
	let root = workspace_root();
	let output = Command::new("cargo")
		.args(["metadata", "--no-deps", "--format-version=1", "--locked"])
		.current_dir(&root)
		.output()?;
	ensure!(
		output.status.success(),
		"cargo metadata: {}",
		String::from_utf8_lossy(&output.stderr)
	);
	let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)?;
	let mut pending = targets(&metadata);
	let mut seen = BTreeSet::new();
	let mut sources = Vec::new();
	while let Some((path, directory)) = pending.pop() {
		if !seen.insert(path.clone()) {
			continue;
		}
		let source = fs::read_to_string(&path)
			.with_context(|| path.display().to_string())?;
		let syntax = syn::parse_file(&source)
			.with_context(|| path.display().to_string())?;
		discover(&syntax, directory, &mut pending);
		if !test_only(&syntax.attrs) {
			sources.push(Source { path, syntax });
		}
	}
	Ok((root, sources))
}

fn discover(syntax: &syn::File, directory: PathBuf, pending: &mut Vec<Module>) {
	let mut discover = Discover { directory, pending };
	discover.visit_file(syntax);
}

/// Collects external modules without scoring anything.
struct Discover<'a> {
	directory: PathBuf,
	pending: &'a mut Vec<Module>,
}

impl<'ast> Visit<'ast> for Discover<'_> {
	fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
		if test_only(&module.attrs) {
			return;
		}
		if let Some((_, items)) = &module.content {
			let directory = self.directory.join(module.ident.to_string());
			let previous = std::mem::replace(&mut self.directory, directory);
			for item in items {
				self.visit_item(item);
			}
			self.directory = previous;
		} else {
			self.pending.push(external_module(&self.directory, module));
		}
	}
}

fn external_module(directory: &Path, module: &syn::ItemMod) -> Module {
	let module_directory = directory.join(module.ident.to_string());
	let explicit = module.attrs.iter().find_map(|attr| {
		if let Meta::NameValue(value) = &attr.meta
			&& value.path.is_ident("path")
			&& let Expr::Lit(literal) = &value.value
			&& let syn::Lit::Str(path) = &literal.lit
		{
			return Some(directory.join(path.value()));
		}
		None
	});
	let flat = module_directory.with_extension("rs");
	let path = explicit.clone().unwrap_or_else(|| {
		if flat.exists() {
			flat
		} else {
			module_directory.join("mod.rs")
		}
	});
	let children = if explicit.is_some() {
		path.parent().unwrap().to_owned()
	} else {
		module_directory
	};
	(path, children)
}

fn measure(
	path: &Path,
	syntax: &syn::File,
	pending: &mut Vec<Module>,
) -> Result<Vec<Function>> {
	let mut check = Check {
		directory: path.parent().unwrap_or(Path::new("")).to_owned(),
		pending,
		scores: Vec::new(),
		functions: Vec::new(),
		error: None,
	};
	if !test_only(&syntax.attrs) {
		check.visit_file(syntax);
	}
	if let Some(error) = check.error {
		bail!("{}:{}: {error}", path.display(), error.span().start().line);
	}
	Ok(check.functions)
}

fn exemption(attrs: &[Attribute]) -> syn::Result<Option<String>> {
	let mut reason = None;
	for attr in attrs.iter().filter(|attr| attr.path().is_ident("cfg_attr")) {
		let args = attr
			.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
		for meta in args.iter().skip(1) {
			if meta.path().segments.len() == 2
				&& meta.path().segments[0].ident == "markview"
				&& meta.path().segments[1].ident == "allow_complexity"
			{
				if !matches!(args.first(), Some(Meta::List(predicate)) if predicate.path.is_ident("any") && predicate.tokens.is_empty())
				{
					return Err(syn::Error::new_spanned(
						attr,
						"complexity exemptions require `cfg_attr(any(), ...)`",
					));
				}
				let Meta::List(marker) = meta else {
					return Err(syn::Error::new_spanned(
						meta,
						"allow_complexity requires a nonempty `reason`",
					));
				};
				if reason.is_some() {
					return Err(syn::Error::new_spanned(
						marker,
						"only one complexity exemption is allowed per function",
					));
				}
				marker.parse_nested_meta(|arg| {
					if !arg.path.is_ident("reason") || reason.is_some() {
						return Err(arg.error("expected one `reason` string"));
					}
					let value: syn::LitStr = arg.value()?.parse()?;
					if value.value().trim().is_empty() {
						return Err(syn::Error::new_spanned(
							value,
							"complexity exemption reason must not be empty",
						));
					}
					reason = Some(value.value());
					Ok(())
				})?;
				if reason.is_none() {
					return Err(syn::Error::new_spanned(
						marker,
						"allow_complexity requires a nonempty `reason`",
					));
				}
			}
		}
	}
	Ok(reason)
}

fn targets(metadata: &serde_json::Value) -> Vec<Module> {
	let mut pending = Vec::new();
	for package in metadata["packages"].as_array().unwrap() {
		for target in package["targets"].as_array().unwrap() {
			if target["kind"].as_array().unwrap().iter().any(|kind| {
				matches!(kind.as_str(), Some("test" | "bench" | "example"))
			}) {
				continue;
			}
			let path = PathBuf::from(target["src_path"].as_str().unwrap());
			pending.push((path.clone(), path.parent().unwrap().to_owned()));
		}
	}
	pending
}

pub(crate) fn test_only(attrs: &[Attribute]) -> bool {
	attrs.iter().any(|attr| {
		attr.path()
			.segments
			.last()
			.is_some_and(|segment| segment.ident == "test")
			|| (attr.path().is_ident("cfg")
				&& attr
					.parse_args::<Meta>()
					.is_ok_and(|meta| requires_test(&meta)))
	})
}

fn requires_test(meta: &Meta) -> bool {
	match meta {
		Meta::Path(path) => path.is_ident("test"),
		Meta::NameValue(value) if value.path.is_ident("feature") => {
			matches!(&value.value, Expr::Lit(literal) if matches!(&literal.lit, syn::Lit::Str(feature) if feature.value() == "test-support"))
		}
		Meta::List(list)
			if list.path.is_ident("all") || list.path.is_ident("any") =>
		{
			let args = list
				.parse_args_with(
					Punctuated::<Meta, Token![,]>::parse_terminated,
				)
				.unwrap();
			if list.path.is_ident("all") {
				args.iter().any(requires_test)
			} else {
				!args.is_empty() && args.iter().all(requires_test)
			}
		}
		_ => false,
	}
}

struct Check<'a> {
	directory: PathBuf,
	pending: &'a mut Vec<Module>,
	scores: Vec<usize>,
	functions: Vec<Function>,
	error: Option<syn::Error>,
}

impl Check<'_> {
	fn function(
		&mut self,
		attrs: &[Attribute],
		name: String,
		span: proc_macro2::Span,
		visit: impl FnOnce(&mut Self),
	) {
		let exemption = match exemption(attrs) {
			Ok(reason) => reason,
			Err(error) => {
				self.error.get_or_insert(error);
				None
			}
		};
		self.scores.push(1);
		visit(self);
		self.functions.push(Function {
			line: span.start().line,
			name,
			score: self.scores.pop().unwrap(),
			exemption,
		});
	}
}

impl<'ast> Visit<'ast> for Check<'_> {
	fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
		if test_only(&module.attrs) {
			return;
		}
		if let Some((_, items)) = &module.content {
			let directory = self.directory.join(module.ident.to_string());
			let previous = std::mem::replace(&mut self.directory, directory);
			for item in items {
				self.visit_item(item);
			}
			self.directory = previous;
		} else {
			self.pending.push(external_module(&self.directory, module));
		}
	}

	fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
		if !test_only(&item.attrs) {
			self.function(
				&item.attrs,
				item.sig.ident.to_string(),
				item.sig.ident.span(),
				|check| check.visit_block(&item.block),
			);
		}
	}

	fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
		if !test_only(&item.attrs) {
			visit::visit_item_impl(self, item);
		}
	}

	fn visit_item_trait(&mut self, item: &'ast syn::ItemTrait) {
		if !test_only(&item.attrs) {
			visit::visit_item_trait(self, item);
		}
	}

	fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
		if !test_only(&item.attrs) {
			self.function(
				&item.attrs,
				item.sig.ident.to_string(),
				item.sig.ident.span(),
				|check| check.visit_block(&item.block),
			);
		}
	}

	fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
		if !test_only(&item.attrs)
			&& let Some(body) = &item.default
		{
			self.function(
				&item.attrs,
				item.sig.ident.to_string(),
				item.sig.ident.span(),
				|check| check.visit_block(body),
			);
		}
	}

	fn visit_expr_closure(&mut self, closure: &'ast syn::ExprClosure) {
		self.function(
			&closure.attrs,
			"<closure>".into(),
			closure.span(),
			|check| check.visit_expr(&closure.body),
		);
	}

	fn visit_expr(&mut self, expr: &'ast Expr) {
		let branches = match expr {
			Expr::If(_) | Expr::While(_) | Expr::ForLoop(_) | Expr::Try(_) => 1,
			Expr::Binary(binary)
				if matches!(
					binary.op,
					syn::BinOp::And(_) | syn::BinOp::Or(_)
				) =>
			{
				1
			}
			Expr::Match(expr) => {
				expr.arms.len().saturating_sub(1)
					+ expr.arms.iter().filter(|arm| arm.guard.is_some()).count()
			}
			_ => 0,
		};
		if let Some(score) = self.scores.last_mut() {
			*score += branches;
		}
		visit::visit_expr(self, expr);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn check(source: &str) -> syn::Result<(Vec<Function>, Vec<Module>)> {
		let mut pending = Vec::new();
		let mut check = Check {
			directory: PathBuf::from("src"),
			pending: &mut pending,
			scores: Vec::new(),
			functions: Vec::new(),
			error: None,
		};
		check.visit_file(&syn::parse_file(source).unwrap());
		if let Some(error) = check.error {
			return Err(error);
		}
		Ok((check.functions, pending))
	}

	#[test]
	fn counts_decisions_and_separates_nested_functions() {
		let (functions, _) = check(
			r#"
			fn outer() {
				if a && b || c {}
				while a {}
				for x in xs {}
				value?;
				match x { 0 => (), 1 if a => (), _ => () }
				let f = || { if a {} };
				fn inner() { if a {} }
			}
			impl T { fn method() { if a {} } }
			trait T { fn default() { if a {} } }
		"#,
		)
		.unwrap();
		assert_eq!(
			functions
				.iter()
				.map(|function| (function.name.as_str(), function.score))
				.collect::<Vec<_>>(),
			[
				("<closure>", 2),
				("inner", 2),
				("outer", 10),
				("method", 2),
				("default", 2)
			]
		);
	}

	#[test]
	fn excludes_test_modules_and_helpers_but_keeps_production_cfg() {
		let (functions, pending) = check(
			r#"
			#[cfg(test)] mod tests { fn helper() { if a {} } }
			#[cfg(all(test, unix))] mod external_tests;
			#[cfg(test)] fn helper() {}
			#[test] fn regression() {}
			#[tokio::test] async fn async_regression() {}
			#[cfg(feature = "test-support")] mod support;
			#[cfg(test)] impl T { fn helper() {} }
			#[cfg(not(test))] fn production() {}
			#[cfg(any(test, unix))] mod platform;
			mod inline { #[path = "custom.rs"] mod child; }
		"#,
		)
		.unwrap();
		assert_eq!(
			functions
				.iter()
				.map(|function| function.name.as_str())
				.collect::<Vec<_>>(),
			["production"]
		);
		assert_eq!(
			pending,
			[
				(
					PathBuf::from("src/platform/mod.rs"),
					PathBuf::from("src/platform")
				),
				(
					PathBuf::from("src/inline/custom.rs"),
					PathBuf::from("src/inline")
				)
			]
		);
	}

	#[test]
	fn accepts_fifteen_and_reports_sixteen() {
		let (functions, _) = check(&format!(
			"fn allowed() {{ {} }} fn exceeded() {{ {} }}",
			"if a {}".repeat(14),
			"if a {}".repeat(15)
		))
		.unwrap();
		assert!(!functions[0].exceeds(15));
		assert!(functions[1].exceeds(15));
	}

	#[test]
	fn marked_rust_compiles_and_exempts_only_the_annotated_body() {
		let directory = tempfile::tempdir().unwrap();
		let source = directory.path().join("fixture.rs");
		let decisions = "if flag {}".repeat(15);
		let marker = r#"#[cfg_attr(any(), markview::allow_complexity(reason = "Keep the mapping together"))]"#;
		fs::write(&source, format!(r#"
			{marker}
			pub fn outer(flag: bool) {{
				fn inner(flag: bool) {{ {decisions} }}
				let nested = || {{ {decisions} }};
				inner(flag);
				nested();
				{decisions}
			}}
			pub fn sibling(flag: bool) {{ {decisions} }}
			pub struct Example;
			impl Example {{ {marker} pub fn method(flag: bool) {{ {decisions} }} }}
			pub trait ExampleTrait {{ {marker} fn default(flag: bool) {{ {decisions} }} }}
			pub fn closure_scope(flag: bool) {{
				let marked = {{ {marker} || {{ {decisions} }} }};
				marked();
			}}
		"#)).unwrap();
		let output = Command::new("rustc")
			.args([
				"--edition=2024",
				"--crate-type=lib",
				"--emit=metadata",
				"-Dwarnings",
				"-o",
			])
			.arg(directory.path().join("fixture.rmeta"))
			.arg(&source)
			.output()
			.unwrap();
		assert!(
			output.status.success(),
			"{}",
			String::from_utf8_lossy(&output.stderr)
		);
		let functions = measure(
			&source,
			&syn::parse_file(&fs::read_to_string(&source).unwrap()).unwrap(),
			&mut Vec::new(),
		)
		.unwrap();
		assert_eq!(
			functions
				.iter()
				.map(|f| (f.name.as_str(), f.score, f.exceeds(15)))
				.collect::<Vec<_>>(),
			[
				("inner", 16, true),
				("<closure>", 16, true),
				("outer", 16, false),
				("sibling", 16, true),
				("method", 16, false),
				("default", 16, false),
				("<closure>", 16, false),
				("closure_scope", 1, false),
			]
		);
		assert_eq!(
			functions
				.iter()
				.filter_map(|f| f.exemption.as_deref())
				.collect::<Vec<_>>(),
			["Keep the mapping together"; 4]
		);
	}

	#[test]
	fn malformed_exemptions_fail_instead_of_disabling_the_check() {
		for marker in [
			"markview::allow_complexity",
			"markview::allow_complexity()",
			r#"markview::allow_complexity(reason = " ")"#,
			"markview::allow_complexity(reason = 15)",
			r#"markview::allow_complexity(reason = "mapping", extra = true)"#,
			r#"markview::allow_complexity(reason = "mapping", reason = "duplicate")"#,
		] {
			let source = format!("#[cfg_attr(any(), {marker})] fn f() {{}}");
			assert!(check(&source).is_err(), "{marker}");
		}
		assert!(check(r#"#[cfg_attr(test, markview::allow_complexity(reason = "mapping"))] fn f() {}"#).is_err());
		assert!(
			check(
				r#"
			#[cfg_attr(any(), markview::allow_complexity(reason = "mapping"))]
			#[cfg_attr(any(), markview::allow_complexity())]
			fn f() {}
		"#
			)
			.is_err()
		);
	}

	#[test]
	fn module_impl_and_unrelated_allow_attributes_do_not_exempt_functions() {
		let (functions, _) = check(
			r#"
			#[cfg_attr(any(), markview::allow_complexity(reason = "module"))]
			mod module { fn child() { if flag {} } }
			#[cfg_attr(any(), markview::allow_complexity(reason = "impl"))]
			impl Example { fn method() { if flag {} } }
			#[allow(clippy::cognitive_complexity, reason = "different metric")]
			fn unrelated() { if flag {} }
		"#,
		)
		.unwrap();
		assert_eq!(functions.len(), 3);
		assert!(
			functions
				.iter()
				.all(|f| f.exemption.is_none() && f.exceeds(1))
		);
	}
}
