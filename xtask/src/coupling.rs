//! Application coupling audit.
//!
//! The audit counts the fields, write sites, and command surface that the
//! application-layer refactor ratchets down. Struct fields, function touches
//! and write sites come from `syn` syntax trees; command references are a
//! production-only trend metric because same-named enums in other modules are
//! included by design.
use std::{
	collections::{BTreeMap, BTreeSet},
	fs,
	path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use syn::{
	Expr, ExprMethodCall, Fields, ItemEnum, ItemImpl, ItemStruct, ItemTrait,
	Member, Type,
	visit::{self, Visit},
};

use crate::complexity;

/// Struct field counts that the baseline ratchets.
const STRUCTS: &[(&str, &str)] = &[
	("App", "src/app.rs"),
	("InteractionState", "src/state.rs"),
	("ReaderSession", "src/state.rs"),
	("Chrome", "src/app/chrome.rs"),
	("Config", "src/settings/store.rs"),
	("ReaderSettings", "src/settings.rs"),
	("Markview", "crates/markview-web/src/api.rs"),
];

/// Fields whose direct production writes the baseline ratchets.
///
/// A name may be a path (for example `status.message`); a write site is an
/// assignment whose accessed field chain contains that path. `Status` owns its
/// three fields, so the status observation names them instead of the whole
/// `App.status`: the owner's `set`/`clear` calls from other modules are the
/// sanctioned path, while a direct `self.status.<field>` write still counts.
/// `export::Session` owns the eight export fields, so those observations name
/// `export.<field>`: the orchestrating `impl App` is the only direct writer,
/// and any other file increases the writer count. The owner's own
/// `self.<field>` writes stay out of the ratchet.
const WRITERS: &[&str] = &[
	"status.message",
	"status.error",
	"status.until",
	"export.running",
	"export.thread",
	"export.source",
	"export.png",
	"export.watch",
	"export.watch_at",
	"export.rebuild",
	"export.watch_request",
	// `modal::Model` owns the confirmation's pending question; the observation
	// names the path through `InteractionState.overlays` so a direct write
	// from another module still counts, while the model's own
	// `self.current` writes stay out.
	"overlays.modal.current",
	// `dropdown::Model` owns the open list; the observation names the path
	// through `InteractionState.overlays` so a direct write from another
	// module still counts, while the model's own `self.open` writes stay out.
	"overlays.dropdown.open",
	// `viewer::Model` owns the picture on show and its zoom, pan and press;
	// the observation names the path through `InteractionState.overlays` so
	// a direct write from another module still counts, while the model's own
	// `self.current` writes stay out.
	"overlays.viewer.current",
	// `pointer::State` owns the twelve pointer fields; the observation names
	// the path through `InteractionState` so a direct write in any other
	// module still counts, while writes inside the owner stay out.
	"pointer.pointer_down",
	"pointer.pressed_image",
	"pointer.dragged",
	"pointer.drag_at",
	"pointer.modifiers",
	"pointer.cursor",
	"pointer.hover",
	"pointer.hover_image",
	"pointer.hover_overflow",
	"pointer.scrollbar",
	"pointer.last_click",
	"pointer.wheel",
	// `panels::Model` owns the shell's page offsets and the scrollbar drag;
	// the observation names the path through `InteractionState`, so a direct
	// write from another module still counts while the owner's stays out.
	"panels.export_scroll",
	"panels.styles_scroll",
	"panels.grab",
	// `sidebar::State` owns the resize drag it holds.
	"sidebar.drag",
	// The settings page model owns its offset and preview; a write reached
	// through `App.preferences` still counts, the owner's stays out.
	"settings_scroll",
	"settings_preview",
	// A placed control layout is a draw product, so the host owns it.
	"controls",
	// `outline::Model` owns the drawer's four fields; the observation names
	// the path through `InteractionState.overlays` so a direct write from the
	// interaction state or the host still counts, while the model's own
	// `self.<field>` writes stay out.
	"overlays.outline.open",
	"overlays.outline.scroll",
	"overlays.outline.selection",
	"overlays.outline.animation",
	// `TabStrip` and `Browser` own their drawer animations; the observation
	// names the path through `App` so only a direct host write counts.
	"tab_strip.animation",
	"folder.animation",
];

const TOP_FUNCTIONS: usize = 20;

pub struct Options {
	pub check: bool,
	pub update: bool,
	pub json: bool,
}

pub fn run(options: Options) -> Result<()> {
	let (root, sources) = complexity::workspace_sources()?;
	let command = command_surface(&root, &sources)?;
	let fields = field_surface(&root, &sources);
	let mut audit = Audit::default();
	for source in &sources {
		let relative = source.path.strip_prefix(&root).unwrap_or(&source.path);
		analyze(
			relative,
			&source.syntax,
			&command.variants,
			&fields,
			WRITERS,
			&mut audit,
		);
	}
	audit.command_variants = Some(command.variants.len());
	audit.finish()?;
	let report = Report {
		audit,
		complexity: complexity::summary()?,
	};
	if options.update {
		let path = write_baseline(&root, &report)?;
		let note = format!(
			"baseline updated: {}\nnote: writer observation paths change as fields move into sub-models (for example `status.message`); the baseline records the new path.",
			path.strip_prefix(&root).unwrap_or(&path).display()
		);
		if options.json {
			eprintln!("{note}");
		} else {
			println!("{note}");
		}
	}
	if options.check {
		check_baseline(&root, &report)?;
	}
	if options.json {
		println!("{}", serde_json::to_string_pretty(&report.json())?);
	} else {
		report.print();
	}
	Ok(())
}

#[derive(Default)]
struct Audit {
	structs: BTreeMap<String, usize>,
	functions: Vec<Function>,
	writers: BTreeMap<String, Writer>,
	command_variants: Option<usize>,
	command_references: usize,
	command_files: BTreeSet<String>,
}

#[derive(Default)]
struct Writer {
	sites: usize,
	files: BTreeSet<String>,
}

struct Function {
	path: String,
	line: usize,
	name: String,
	fields: usize,
	touches: usize,
}

struct FunctionAcc {
	name: String,
	line: usize,
	fields: BTreeSet<String>,
	touches: BTreeSet<String>,
}

/// The named fields declared by each production struct.
#[derive(Default)]
struct FieldSurface {
	by_file: BTreeMap<String, BTreeMap<String, BTreeSet<String>>>,
	by_name: BTreeMap<String, BTreeSet<String>>,
	ambiguous: BTreeSet<String>,
}

impl FieldSurface {
	fn add(&mut self, relative: &str, name: String, fields: BTreeSet<String>) {
		match self.by_name.get(&name) {
			Some(existing) if existing != &fields => {
				self.ambiguous.insert(name.clone());
			}
			None => {
				self.by_name.insert(name.clone(), fields.clone());
			}
			_ => {}
		}
		self.by_file
			.entry(relative.to_owned())
			.or_default()
			.insert(name, fields);
	}

	fn owner(&self, relative: &str, name: &str) -> Option<&BTreeSet<String>> {
		self.by_file
			.get(relative)
			.and_then(|types| types.get(name))
			.or_else(|| {
				(!self.ambiguous.contains(name))
					.then(|| self.by_name.get(name))
					.flatten()
			})
	}
}

impl Audit {
	fn finish(&mut self) -> Result<()> {
		for (name, _) in STRUCTS {
			ensure!(
				self.structs.contains_key(*name),
				"coupling audit could not find struct {name}"
			);
		}
		ensure!(
			self.command_variants.is_some(),
			"coupling audit could not find state::Command"
		);
		self.functions.sort_by(|a, b| {
			b.touches
				.cmp(&a.touches)
				.then_with(|| b.fields.cmp(&a.fields))
				.then_with(|| a.path.cmp(&b.path))
				.then_with(|| a.name.cmp(&b.name))
				.then_with(|| a.line.cmp(&b.line))
		});
		self.functions.truncate(TOP_FUNCTIONS);
		Ok(())
	}

	fn writer(&self, field: &str) -> (usize, usize) {
		self.writers
			.get(field)
			.map_or((0, 0), |writer| (writer.files.len(), writer.sites))
	}

	fn json(&self, complexity: &complexity::Summary) -> Value {
		let writers = WRITERS
			.iter()
			.map(|field| {
				let (files, sites) = self.writer(field);
				(
					(*field).to_owned(),
					json!({ "files": files, "sites": sites }),
				)
			})
			.collect::<BTreeMap<_, _>>();
		json!({
			"structs": self.structs,
			"functions": self.functions.iter().map(|function| json!({
				"path": function.path,
				"line": function.line,
				"name": function.name,
				"fields": function.fields,
				"touches": function.touches,
			})).collect::<Vec<_>>(),
			"writers": writers,
			"commands": {
				"variants": self.command_variants.unwrap_or(0),
				"references": self.command_references,
				"files": self.command_files.len(),
			},
			"complexity": {
				"reports": complexity.reports,
				"functions": complexity.functions,
				"exemptions": complexity.exemptions,
			},
		})
	}

	fn print(&self, complexity: &complexity::Summary) {
		println!("structs");
		for (name, _) in STRUCTS {
			println!("  {name} {}", self.structs[*name]);
		}
		println!(
			"functions (top {TOP_FUNCTIONS} by distinct self.<name> touches; fields are declared struct fields)"
		);
		for function in &self.functions {
			println!(
				"  {}:{} {}: {} fields / {} touches",
				function.path,
				function.line,
				function.name,
				function.fields,
				function.touches
			);
		}
		println!("writers (production direct writes, test modules excluded)");
		for field in WRITERS {
			let (files, sites) = self.writer(field);
			println!("  {field} {files} files / {sites} sites");
		}
		println!("commands");
		println!(
			"  state::Command {} variants",
			self.command_variants.unwrap_or(0)
		);
		println!(
			"  Command:: {} references / {} files (trend only)",
			self.command_references,
			self.command_files.len()
		);
		println!("complexity");
		println!(
			"  {} reports / {} functions / {} exemptions",
			complexity.reports, complexity.functions, complexity.exemptions
		);
	}
}

struct Report {
	audit: Audit,
	complexity: complexity::Summary,
}

impl Report {
	fn json(&self) -> Value {
		self.audit.json(&self.complexity)
	}

	fn print(&self) {
		self.audit.print(&self.complexity);
	}

	fn baseline(&self) -> Value {
		let writers = WRITERS
			.iter()
			.map(|field| {
				let (files, sites) = self.audit.writer(field);
				(
					(*field).to_owned(),
					json!({ "files": files, "sites": sites }),
				)
			})
			.collect::<BTreeMap<_, _>>();
		json!({
			"structs": self.audit.structs,
			"writers": writers,
			"commands": { "variants": self.audit.command_variants.unwrap_or(0) },
		})
	}
}

fn analyze(
	relative: &Path,
	syntax: &syn::File,
	command_variants: &BTreeSet<String>,
	field_surface: &FieldSurface,
	writers: &[&'static str],
	audit: &mut Audit,
) {
	let mut analyzer = Analyzer {
		audit,
		relative: relative.display().to_string(),
		impl_type: None,
		command_variants,
		field_surface,
		writers,
		functions: Vec::new(),
	};
	analyzer.visit_file(syntax);
	debug_assert!(analyzer.functions.is_empty());
}

struct CommandSurface {
	variants: BTreeSet<String>,
}

/// Finds the variants of `state::Command` before any reference is counted.
fn command_surface(
	root: &Path,
	sources: &[complexity::Source],
) -> Result<CommandSurface> {
	let expected = root.join("src/state/command.rs");
	let source = sources
		.iter()
		.find(|source| source.path == expected)
		.with_context(|| {
			format!("coupling audit could not find {}", expected.display())
		})?;
	let mut finder = CommandFinder { variants: None };
	finder.visit_file(&source.syntax);
	finder
		.variants
		.map(|variants| CommandSurface { variants })
		.context("coupling audit could not find state::Command")
}

struct CommandFinder {
	variants: Option<BTreeSet<String>>,
}

impl<'ast> Visit<'ast> for CommandFinder {
	fn visit_item_enum(&mut self, item: &'ast ItemEnum) {
		if complexity::test_only(&item.attrs) {
			return;
		}
		if item.ident == "Command" {
			self.variants = Some(
				item.variants
					.iter()
					.map(|variant| variant.ident.to_string())
					.collect(),
			);
		}
		visit::visit_item_enum(self, item);
	}
}

/// Collects the production struct fields used to resolve `self.<field>`.
fn field_surface(root: &Path, sources: &[complexity::Source]) -> FieldSurface {
	let mut surface = FieldSurface::default();
	for source in sources {
		let relative = source
			.path
			.strip_prefix(root)
			.unwrap_or(&source.path)
			.display()
			.to_string();
		let mut finder = FieldFinder {
			relative: &relative,
			surface: &mut surface,
		};
		finder.visit_file(&source.syntax);
	}
	surface
}

struct FieldFinder<'a> {
	relative: &'a str,
	surface: &'a mut FieldSurface,
}

impl<'ast> Visit<'ast> for FieldFinder<'_> {
	fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
		if complexity::test_only(&module.attrs) {
			return;
		}
		visit::visit_item_mod(self, module);
	}

	fn visit_item_struct(&mut self, item: &'ast ItemStruct) {
		if complexity::test_only(&item.attrs) {
			return;
		}
		if let Fields::Named(named) = &item.fields {
			let fields = named
				.named
				.iter()
				.filter_map(|field| field.ident.as_ref())
				.map(ToString::to_string)
				.collect();
			self.surface
				.add(self.relative, item.ident.to_string(), fields);
		}
	}
}

struct Analyzer<'a> {
	audit: &'a mut Audit,
	relative: String,
	impl_type: Option<String>,
	command_variants: &'a BTreeSet<String>,
	field_surface: &'a FieldSurface,
	writers: &'a [&'static str],
	functions: Vec<FunctionAcc>,
}

impl Analyzer<'_> {
	fn measure(&mut self, name: &syn::Ident, block: &syn::Block) {
		self.functions.push(FunctionAcc {
			name: name.to_string(),
			line: name.span().start().line,
			fields: BTreeSet::new(),
			touches: BTreeSet::new(),
		});
		self.visit_block(block);
		let function =
			self.functions.pop().expect("function scope is balanced");
		if !function.touches.is_empty() {
			self.audit.functions.push(Function {
				path: self.relative.clone(),
				line: function.line,
				name: function.name,
				fields: function.fields.len(),
				touches: function.touches.len(),
			});
		}
	}

	fn write(&mut self, expression: &Expr) {
		let Some(chain) = field_chain(expression) else {
			return;
		};
		let Some((field, start)) = self.writers.iter().find_map(|field| {
			let observed = field.split('.').collect::<Vec<_>>();
			observed_at(&chain, &observed).map(|start| (*field, start))
		}) else {
			return;
		};
		// The enclosing impl must own the observed path, or reach it through
		// the documented bridge (`App.interaction` for interaction fields).
		let owner = writer_owner(field);
		// The owner writes `self.<path>` directly; another impl may reach the
		// path through its documented bridge (`App.interaction`, say), where
		// the observed window starts after the bridge's first segment.
		let allowed = (self.impl_type.as_deref() == Some(owner) && start == 0)
			|| writer_bridge(field).is_some_and(|(container, bridge)| {
				self.impl_type.as_deref() == Some(container)
					&& start >= 1 && chain
					.first()
					.is_some_and(|first| first == bridge)
			});
		if !allowed {
			return;
		}
		let writer = self.audit.writers.entry(field.to_owned()).or_default();
		writer.sites += 1;
		writer.files.insert(self.relative.clone());
	}
}

impl<'ast> Visit<'ast> for Analyzer<'_> {
	fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
		if complexity::test_only(&module.attrs) {
			return;
		}
		visit::visit_item_mod(self, module);
	}

	fn visit_item_struct(&mut self, item: &'ast ItemStruct) {
		if complexity::test_only(&item.attrs) {
			return;
		}
		if let Some((name, expected)) =
			STRUCTS.iter().find(|(name, _)| item.ident == *name)
			&& self.relative.as_str() == *expected
		{
			let fields = match &item.fields {
				Fields::Named(fields) => fields.named.len(),
				_ => 0,
			};
			self.audit.structs.insert((*name).to_owned(), fields);
		}
	}

	fn visit_item_impl(&mut self, item: &'ast ItemImpl) {
		if complexity::test_only(&item.attrs) {
			return;
		}
		let previous = self.impl_type.replace(type_name(&item.self_ty));
		visit::visit_item_impl(self, item);
		self.impl_type = previous;
	}

	fn visit_item_trait(&mut self, item: &'ast ItemTrait) {
		if complexity::test_only(&item.attrs) {
			return;
		}
		let previous = self.impl_type.replace(item.ident.to_string());
		visit::visit_item_trait(self, item);
		self.impl_type = previous;
	}

	fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
		if complexity::test_only(&item.attrs) {
			return;
		}
		self.measure(&item.sig.ident, &item.block);
	}

	fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
		if complexity::test_only(&item.attrs) {
			return;
		}
		self.measure(&item.sig.ident, &item.block);
	}

	fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
		if complexity::test_only(&item.attrs) {
			return;
		}
		if let Some(block) = &item.default {
			self.measure(&item.sig.ident, block);
		}
	}

	fn visit_expr_assign(&mut self, expression: &'ast syn::ExprAssign) {
		self.write(&expression.left);
		visit::visit_expr_assign(self, expression);
	}

	fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
		if call.method == "clear" {
			self.write(&call.receiver);
		}
		// `touches` keeps the regex-comparable `self.<name>` surface, which
		// includes methods called directly on `self`; `fields` does not.
		if is_self(&call.receiver)
			&& let Some(function) = self.functions.last_mut()
		{
			function.touches.insert(call.method.to_string());
		}
		visit::visit_expr_method_call(self, call);
	}

	fn visit_expr_field(&mut self, field: &'ast syn::ExprField) {
		if is_self(&field.base)
			&& let Member::Named(name) = &field.member
		{
			let name = name.to_string();
			let is_field = self
				.impl_type
				.as_deref()
				.and_then(|owner| {
					self.field_surface.owner(&self.relative, owner)
				})
				.is_some_and(|fields| fields.contains(&name));
			if let Some(function) = self.functions.last_mut() {
				function.touches.insert(name.clone());
				if is_field {
					function.fields.insert(name);
				}
			}
		}
		visit::visit_expr_field(self, field);
	}

	fn visit_path(&mut self, path: &'ast syn::Path) {
		let segments = &path.segments;
		if (0..segments.len().saturating_sub(1)).any(|index| {
			segments[index].ident == "Command"
				&& self
					.command_variants
					.contains(&segments[index + 1].ident.to_string())
		}) {
			self.audit.command_references += 1;
			self.audit.command_files.insert(self.relative.clone());
		}
		visit::visit_path(self, path);
	}
}

fn type_name(ty: &Type) -> String {
	match ty {
		Type::Path(path) => path
			.path
			.segments
			.last()
			.map_or_else(String::new, |segment| segment.ident.to_string()),
		Type::Reference(reference) => type_name(&reference.elem),
		Type::Paren(paren) => type_name(&paren.elem),
		Type::Group(group) => type_name(&group.elem),
		_ => String::new(),
	}
}

fn writer_owner(field: &str) -> &'static str {
	match field.split('.').next().unwrap_or(field) {
		"status" | "export" | "controls" | "settings_scroll"
		| "settings_preview" | "tab_strip" | "folder" => "App",
		_ => "InteractionState",
	}
}

/// The container type and field through which another impl may write the
/// observed path.
fn writer_bridge(field: &str) -> Option<(&'static str, &'static str)> {
	match field.split('.').next().unwrap_or(field) {
		"settings_scroll" | "settings_preview" => Some(("App", "preferences")),
		_ if writer_owner(field) == "InteractionState" => {
			Some(("App", "interaction"))
		}
		_ => None,
	}
}

/// Returns the named fields accessed from `self`, outermost first.
fn field_chain(expression: &Expr) -> Option<Vec<String>> {
	match expression {
		Expr::Field(field) => {
			let mut chain = field_chain(&field.base)?;
			let Member::Named(name) = &field.member else {
				return None;
			};
			chain.push(name.to_string());
			Some(chain)
		}
		Expr::Path(path)
			if path.qself.is_none() && path.path.is_ident("self") =>
		{
			Some(Vec::new())
		}
		Expr::Paren(paren) => field_chain(&paren.expr),
		Expr::Group(group) => field_chain(&group.expr),
		_ => None,
	}
}

fn is_self(expression: &Expr) -> bool {
	match expression {
		Expr::Path(path) => path.qself.is_none() && path.path.is_ident("self"),
		Expr::Paren(paren) => is_self(&paren.expr),
		Expr::Group(group) => is_self(&group.expr),
		_ => false,
	}
}

fn observed_at(chain: &[String], observed: &[&str]) -> Option<usize> {
	chain
		.windows(observed.len())
		.position(|window| window.iter().zip(observed).all(|(a, b)| a == b))
}

fn write_baseline(root: &Path, report: &Report) -> Result<PathBuf> {
	let path = root.join("xtask/coupling-baseline.json");
	let text = serde_json::to_string_pretty(&report.baseline())? + "\n";
	fs::write(&path, text).with_context(|| path.display().to_string())?;
	Ok(path)
}

fn check_baseline(root: &Path, report: &Report) -> Result<()> {
	let path = root.join("xtask/coupling-baseline.json");
	let text = fs::read_to_string(&path)
		.with_context(|| path.display().to_string())?;
	let baseline: Value = serde_json::from_str(&text)
		.with_context(|| path.display().to_string())?;
	let mut failures = Vec::new();
	for (name, _) in STRUCTS {
		let Some(bound) = baseline["structs"][name].as_u64() else {
			bail!(
				"baseline is missing structs.{name}; run `coupling --update`"
			);
		};
		let live = report.audit.structs[*name];
		if live as u64 > bound {
			failures.push(format!("structs.{name}: {live} > baseline {bound}"));
		}
	}
	for field in WRITERS {
		let Some(bound) = baseline["writers"][field].as_object() else {
			bail!(
				"baseline is missing writers.{field}; run `coupling --update`"
			);
		};
		let (files, sites) = report.audit.writer(field);
		for (metric, live) in [("files", files), ("sites", sites)] {
			let bound = bound[metric].as_u64().unwrap_or(0);
			if live as u64 > bound {
				failures.push(format!(
					"writers.{field}.{metric}: {live} > baseline {bound}"
				));
			}
		}
	}
	let Some(bound) = baseline["commands"]["variants"].as_u64() else {
		bail!("baseline is missing commands.variants; run `coupling --update`");
	};
	let live = report.audit.command_variants.unwrap_or(0);
	if live as u64 > bound {
		failures.push(format!("commands.variants: {live} > baseline {bound}"));
	}
	if !failures.is_empty() {
		bail!("coupling baseline exceeded:\n{}", failures.join("\n"));
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	fn variants(names: &[&str]) -> BTreeSet<String> {
		names.iter().map(|name| (*name).to_owned()).collect()
	}

	fn analyze_source(
		relative: &str,
		source: &str,
		command_variants: &[&str],
	) -> Audit {
		analyze_with(relative, source, command_variants, WRITERS)
	}

	fn analyze_with(
		relative: &str,
		source: &str,
		command_variants: &[&str],
		writers: &[&'static str],
	) -> Audit {
		let syntax = syn::parse_file(source).unwrap();
		let field_surface = source_field_surface(relative, &syntax);
		let mut audit = Audit::default();
		analyze(
			Path::new(relative),
			&syntax,
			&variants(command_variants),
			&field_surface,
			writers,
			&mut audit,
		);
		audit
	}

	fn source_field_surface(
		relative: &str,
		syntax: &syn::File,
	) -> FieldSurface {
		let mut surface = FieldSurface::default();
		let mut finder = FieldFinder {
			relative,
			surface: &mut surface,
		};
		finder.visit_file(syntax);
		surface
	}

	#[test]
	fn counts_only_top_level_named_fields_of_known_structs() {
		let audit = analyze_source(
			"src/app.rs",
			r#"
			struct App { first: u8, second: Nested }
			struct Nested { ignored_a: u8, ignored_b: u8 }
			struct Tuple(u8, u8);
			struct Markview { wrong_file: u8 }
			#[cfg(test)]
			mod tests {
				struct App { test_field: u8 }
			}
		"#,
			&[],
		);
		assert_eq!(audit.structs.get("App"), Some(&2));
		assert!(!audit.structs.contains_key("Nested"));
		assert!(!audit.structs.contains_key("Tuple"));
		assert!(!audit.structs.contains_key("Markview"));
	}

	#[test]
	fn counts_assignments_and_clear_calls_but_not_comparisons_or_other_methods()
	{
		let audit = analyze_with(
			"src/app.rs",
			r#"
			impl App {
				fn update(&mut self) {
					self.status = String::new();
					self.status.clear();
					self.status_until = None;
					self.interaction.modal = None;
					self.interaction.modal.clear();
					if self.status == "ready" {}
					self.status.len();
					self.status.set();
					self.status.push('!');
					self.interaction.dropdown = None;
					other.status = String::new();
				}
			}
			impl Load {
				fn begin(&mut self) {
					self.status = String::new();
				}
			}
		"#,
			&[],
			&["status", "modal", "dropdown", "viewer"],
		);
		assert_eq!(audit.writer("status"), (1, 2));
		assert_eq!(audit.writer("modal"), (1, 2));
		assert_eq!(audit.writer("dropdown"), (1, 1));
		assert_eq!(audit.writer("viewer"), (0, 0));
	}

	#[test]
	fn counts_subfield_writes_as_writes_to_the_observed_path() {
		let audit = analyze_source(
			"src/app.rs",
			r#"
			impl App {
				fn update(&mut self) {
					self.status.error = true;
					self.status.message.clear();
					self.status_until = None;
				}
			}
		"#,
			&[],
		);
		assert_eq!(audit.writer("status.error"), (1, 1));
		assert_eq!(audit.writer("status.message"), (1, 1));
		assert_eq!(audit.writer("status.until"), (0, 0));
	}

	#[test]
	fn the_status_observation_ignores_clears_and_catches_a_field_write() {
		let audit = analyze_source(
			"src/app.rs",
			r#"
			impl App {
				fn update(&mut self) {
					self.status.clear();
					self.status.set("Saved", false, 3);
					self.status.error = true;
				}
			}
		"#,
			&[],
		);
		assert_eq!(audit.writer("status.error"), (1, 1));
		assert_eq!(audit.writer("status.message"), (0, 0));
	}

	#[test]
	fn the_export_observation_counts_app_writes_and_not_session_methods() {
		let audit = analyze_with(
			"src/app/export.rs",
			r#"
			struct Session { running: bool, watch: bool }
			impl Session {
				fn begin(&mut self) {
					self.running = true;
					self.watch = true;
				}
			}
			impl App {
				fn begin(&mut self) {
					self.export.running = true;
				}
			}
		"#,
			&[],
			&["export.running", "export.watch"],
		);
		assert_eq!(audit.writer("export.running"), (1, 1));
		assert_eq!(audit.writer("export.watch"), (0, 0));
	}

	#[test]
	fn excludes_test_modules_test_items_and_nested_closures_are_outer_touches()
	{
		let audit = analyze_with(
			"src/app.rs",
			r#"
			struct App { status: String, modal: Option<()> }
			impl App {
				fn production(&mut self) {
					self.status = String::new();
					let touch = || self.modal;
					touch();
					self.run();
					self.run();
				}

				#[cfg(test)]
				fn helper(&mut self) {
					self.status = String::new();
				}
			}

			#[cfg(test)]
			mod tests {
				fn helper(&mut self) {
					self.status = String::new();
				}

				#[test]
				fn regression() {
					let _ = Command::Open;
				}
			}
		"#,
			&["Open"],
			&["status", "modal"],
		);
		assert_eq!(audit.writer("status"), (1, 1));
		assert_eq!(audit.command_references, 0);
		assert_eq!(audit.functions.len(), 1);
		let function = &audit.functions[0];
		assert_eq!(function.name, "production");
		assert_eq!(function.fields, 2);
		assert_eq!(function.touches, 3);
	}

	#[test]
	fn counts_command_references_by_variant_name() {
		let audit = analyze_source(
			"src/app.rs",
			r#"
			fn production() {
				let _ = Command::Open;
				let _ = state::Command::Close;
				let _ = font_panel::Command::Open;
				let _ = Command::New;
				let _ = OtherCommand::Open;
			}
		"#,
			&["Open", "Close"],
		);
		assert_eq!(audit.command_references, 3);
		assert_eq!(audit.command_files.len(), 1);
	}

	#[test]
	fn method_calls_are_touches_but_not_fields() {
		let audit = analyze_source(
			"src/app.rs",
			r#"
			struct App { value: u8 }
			impl App {
				fn run(&mut self) {
					self.value;
					self.method();
					self.method();
				}
				fn method(&self) {}
			}
		"#,
			&[],
		);
		let run = audit
			.functions
			.iter()
			.find(|function| function.name == "run")
			.unwrap();
		assert_eq!(run.fields, 1);
		assert_eq!(run.touches, 2);
	}

	#[test]
	fn resolves_a_field_through_another_impl_of_the_same_type() {
		let audit = analyze_source(
			"src/app/session.rs",
			r#"
			struct App { interaction: Interaction, status: String }
			struct Interaction { modal: Option<()> }
			impl App {
				fn first(&self) {
					let _ = self.status;
				}
			}
			impl<Event> App {
				fn second(&self) {
					let _ = self.status;
					let _ = self.status;
				}
			}
		"#,
			&[],
		);
		let fields = audit
			.functions
			.iter()
			.map(|function| (function.name.as_str(), function.fields))
			.collect::<Vec<_>>();
		assert_eq!(fields, [("first", 1), ("second", 1)]);
	}

	#[test]
	fn rejects_writes_through_an_unrelated_field() {
		let audit = analyze_source(
			"src/app.rs",
			r#"
			struct App { status: String, other: Other }
			struct Other { status: String }
			impl App {
				fn update(&mut self) {
					self.other.status = String::new();
				}
			}
		"#,
			&[],
		);
		assert_eq!(audit.writer("status"), (0, 0));
	}

	#[test]
	fn counts_nested_observed_paths_inside_the_owning_impl_or_bridge() {
		let audit = analyze_with(
			"src/app.rs",
			r#"
			struct App { status: Status, interaction: Interaction, other: Other }
			struct Status { message: String }
			struct Interaction { overlays: Overlays }
			struct Overlays { modal: Modal }
			struct Modal { current: bool }
			struct Other { status: String }
			impl App {
				fn update(&mut self) {
					self.status.message = String::new();
					self.interaction.overlays.modal.current = true;
					self.other.status = String::new();
				}
			}
		"#,
			&[],
			&["status.message", "overlays.modal.current"],
		);
		assert_eq!(audit.writer("status.message"), (1, 1));
		assert_eq!(audit.writer("overlays.modal.current"), (1, 1));
	}
}
