//! User stylesheet discovery and atomic installation. Bundled IDs cannot be shadowed.
use anyhow::{Context, Result, bail};
use markview_core::style::{CjkType, StyleTarget, Stylesheet};
use std::{
	fs,
	io::Write,
	path::{Path, PathBuf},
	sync::Arc,
};
pub const SUFFIX: &str = ".mvss.toml";
pub fn apply_font_overrides(
	sheet: Arc<Stylesheet>,
	overrides: &[crate::settings::FontDefOverride],
) -> Result<Arc<Stylesheet>> {
	if overrides.is_empty() {
		return Ok(sheet);
	}
	let mut sheet = (*sheet).clone();
	sheet.apply_font_overrides(
		&overrides
			.iter()
			.map(|item| (item.id.clone(), item.replacement.clone()))
			.collect::<Vec<_>>(),
	)?;
	Ok(Arc::new(sheet))
}
pub fn directory() -> Option<PathBuf> {
	crate::settings::config_path()
		.and_then(|p| p.parent().map(|p| p.join("styles")))
}
fn reserved(id: &str) -> bool {
	Stylesheet::READER_THEMES
		.iter()
		.chain(Stylesheet::PDF_THEMES)
		.chain([&"builtin"])
		.any(|name| id.eq_ignore_ascii_case(name))
}
pub fn validate_id(id: &str) -> Result<()> {
	if id.is_empty()
		|| id == "."
		|| id == ".."
		|| id.contains(['/', '\\', ':'])
		|| id.chars().any(char::is_control)
		|| id.ends_with(['.', ' '])
		|| (reserved(id)
			&& !Stylesheet::READER_THEMES.contains(&id)
			&& !Stylesheet::PDF_THEMES.contains(&id))
	{
		bail!("Invalid stylesheet ID {id:?}");
	}
	Ok(())
}
/// The stylesheet a run starts from: the bundled one when no styles are named,
/// otherwise the named ones, with the reader's CJK variant applied either way.
///
/// Loading a stylesheet without a variant leaves it unselected, and an
/// unselected variant has no `[cjk]` font definition at all, so every CJK
/// cluster loses its configured face to a system fallback. The window path
/// applies the variant through the settings store; this is the same step for
/// the paths that lay out straight from the command line.
pub fn load_for_run(
	ids: Option<&[String]>,
	dir: Option<&Path>,
	cjk_type: CjkType,
) -> Result<Arc<Stylesheet>> {
	match ids {
		Some(ids) => load_with_cjk_type(ids, dir, cjk_type),
		None => {
			let mut sheet = (*Stylesheet::bundled(false)).clone();
			sheet.set_cjk_type(cjk_type);
			Ok(Arc::new(sheet))
		}
	}
}
pub fn load_with_cjk_type(
	ids: &[String],
	dir: Option<&Path>,
	cjk_type: CjkType,
) -> Result<Arc<Stylesheet>> {
	load_over(
		ids,
		dir,
		cjk_type,
		Stylesheet::builtin(),
		Some(StyleTarget::Ui),
	)
}

/// The stylesheet a PDF export starts from: the bundled print sheet, with any
/// named styles layered on top. The reader's theme never applies to paper.
pub fn load_for_pdf(
	ids: Option<&[String]>,
	dir: Option<&Path>,
	cjk_type: CjkType,
) -> Result<Arc<Stylesheet>> {
	match ids {
		Some(ids) => load_over(
			ids,
			dir,
			cjk_type,
			Stylesheet::bundled_print(),
			Some(StyleTarget::Pdf),
		),
		None => {
			let mut sheet = (*Stylesheet::bundled_print()).clone();
			sheet.set_cjk_type(cjk_type);
			Ok(Arc::new(sheet))
		}
	}
}

/// Offscreen diagnostics can preview either kind without selecting it in a UI.
pub fn load_for_preview(
	ids: Option<&[String]>,
	dir: Option<&Path>,
	cjk_type: CjkType,
) -> Result<Arc<Stylesheet>> {
	match ids {
		Some(ids) => load_over(ids, dir, cjk_type, Stylesheet::builtin(), None),
		None => load_for_run(None, dir, cjk_type),
	}
}

fn load_over(
	ids: &[String],
	dir: Option<&Path>,
	cjk_type: CjkType,
	base: Arc<Stylesheet>,
	target: Option<StyleTarget>,
) -> Result<Arc<Stylesheet>> {
	let mut sheet = (*base).clone();
	for id in ids.iter().rev() {
		validate_id(id)?;
		let next = read_rules(id, dir)?;
		if let Some(target) = target
			&& !next.targets.contains(&target)
		{
			bail!("{id}: targets do not include {}", target.as_str());
		}
		sheet.merge(&next);
	}
	sheet.set_cjk_type(cjk_type);
	if let Some(target) = target {
		sheet.targets = vec![target];
	}
	Ok(Arc::new(sheet))
}
#[derive(Clone, Debug)]
pub struct Entry {
	pub id: String,
	pub name: String,
	pub source: String,
	pub error: Option<String>,
}
fn read_rules(id: &str, dir: Option<&Path>) -> Result<Arc<Stylesheet>> {
	validate_id(id)?;
	if let Some(sheet) = Stylesheet::named_rules(id) {
		return Ok(sheet);
	}
	let path = dir
		.context("No user stylesheet directory")?
		.join(format!("{id}{SUFFIX}"));
	let source = fs::read_to_string(&path)
		.with_context(|| format!("Cannot read {}", path.display()))?;
	Ok(Arc::new(
		Stylesheet::parse(&source)
			.with_context(|| path.display().to_string())?,
	))
}

pub fn catalog(dir: Option<&Path>, selected: Option<&[String]>) -> Vec<Entry> {
	catalog_for(dir, selected, StyleTarget::Ui)
}

pub fn catalog_for(
	dir: Option<&Path>,
	selected: Option<&[String]>,
	target: StyleTarget,
) -> Vec<Entry> {
	let mut ids: Vec<String> = Stylesheet::READER_THEMES
		.iter()
		.chain(Stylesheet::PDF_THEMES)
		.map(|id| (*id).into())
		.collect();
	let bundled_count = ids.len();
	if let Some(dir) = dir
		&& let Ok(files) = fs::read_dir(dir)
	{
		for file in files.flatten() {
			let path = file.path();
			if !path.is_file() {
				continue;
			}
			if let Some(id) = path
				.file_name()
				.and_then(|n| n.to_str())
				.and_then(|n| n.strip_suffix(SUFFIX))
				&& !reserved(id)
			{
				ids.push(id.into());
			}
		}
	}
	ids[bundled_count..].sort();
	for id in selected.into_iter().flatten() {
		if !id.eq_ignore_ascii_case("builtin") && !ids.contains(id) {
			ids.push(id.clone());
		}
	}
	ids.into_iter()
		.filter_map(|id| {
			let result = read_rules(&id, dir);
			let selected = selected.is_some_and(|ids| ids.contains(&id));
			let incompatible = result
				.as_ref()
				.is_ok_and(|sheet| !sheet.targets.contains(&target));
			if incompatible && !selected {
				return None;
			}
			let name = result
				.as_ref()
				.ok()
				.and_then(|s| s.meta.name.clone())
				.unwrap_or_else(|| id.clone());
			let error = if incompatible {
				Some(format!(
					"Stylesheet does not support {} use",
					target.as_str()
				))
			} else {
				result.err().map(|e| format!("{e:#}"))
			};
			let source = if Stylesheet::named_rules(&id).is_some() {
				"Bundled".into()
			} else {
				dir.map(|p| {
					p.join(format!("{id}{SUFFIX}")).display().to_string()
				})
				.unwrap_or_default()
			};
			Some(Entry {
				id,
				name,
				source,
				error,
			})
		})
		.collect()
}

/// Parses a stylesheet file without installing it, so a caller can check a
/// draft in place and report the sheet's own metadata on success.
pub fn validate(source: &Path) -> Result<Stylesheet> {
	let text = fs::read_to_string(source)
		.with_context(|| format!("Cannot read {}", source.display()))?;
	Stylesheet::parse(&text).with_context(|| source.display().to_string())
}

pub fn install(
	source: &Path,
	dir: &Path,
	force: bool,
) -> Result<(String, PathBuf)> {
	let id = source
		.file_name()
		.and_then(|n| n.to_str())
		.and_then(|n| n.strip_suffix(SUFFIX))
		.context("Stylesheet filename must end in .mvss.toml")?;
	validate_id(id)?;
	if reserved(id) {
		bail!("{id}: reserved bundled stylesheet ID");
	}
	let bytes = fs::read(source)
		.with_context(|| format!("Cannot read {}", source.display()))?;
	let incoming = Stylesheet::parse(std::str::from_utf8(&bytes)?)
		.with_context(|| source.display().to_string())?;
	fs::create_dir_all(dir)?;
	let destination = dir.join(format!("{id}{SUFFIX}"));
	let replace = force || destination.exists();
	if destination.exists() && !force {
		let installed = Stylesheet::parse(
			&fs::read_to_string(&destination).with_context(|| {
				format!(
					"Cannot read installed stylesheet {}",
					destination.display()
				)
			})?,
		)
		.with_context(|| destination.display().to_string())?;
		if incoming.version <= installed.version {
			bail!(
				"{} version {} is not newer than installed version {}; use --force to replace it",
				source.display(),
				incoming.version,
				installed.version
			);
		}
	}
	let mut file = tempfile::NamedTempFile::new_in(dir)?;
	file.write_all(&bytes)?;
	file.as_file().sync_all()?;
	if replace {
		file.persist(&destination)?;
	} else {
		file.persist_noclobber(&destination).with_context(|| {
			format!(
				"Cannot install {}; use --force to replace an existing stylesheet",
				destination.display()
			)
		})?;
	}
	if let Ok(d) = fs::File::open(dir) {
		let _ = d.sync_all();
	}
	Ok((id.into(), destination))
}
#[cfg(test)]
mod tests {
	use super::*;
	/// Load named styles without caring about the CJK variant.
	fn load(ids: &[String], dir: Option<&Path>) -> Result<Arc<Stylesheet>> {
		load_with_cjk_type(ids, dir, CjkType::Sc)
	}
	#[test]
	fn validate_reads_a_sheet_without_installing_it() {
		let tmp = tempfile::tempdir().unwrap();
		let source = tmp.path().join("a.mvss.toml");
		fs::write(
			&source,
			"format_version=2\nversion=1\n[[rule]]\nwhen=['body']\ncolor='#abcdef'",
		)
		.unwrap();
		let sheet = validate(&source).unwrap();
		assert_eq!(sheet.version, 1);
		assert_eq!(sheet.rules.len(), 1);
		// Nothing is copied, and a file that cannot parse names itself.
		assert!(!tmp.path().join("styles").exists());
		fs::write(&source, "version=1\n").unwrap();
		let error = validate(&source).unwrap_err().to_string();
		assert!(error.contains("a.mvss.toml"), "{error}");
		assert!(validate(&tmp.path().join("missing.mvss.toml")).is_err());
	}
	#[test]
	fn install_is_validated_atomic_and_not_enabled() {
		let tmp = tempfile::tempdir().unwrap();
		let dir = tmp.path().join("styles");
		let source = tmp.path().join("a.mvss.toml");
		fs::write(
			&source,
			"format_version=2\nversion=1\n[[rule]]\nwhen=['body']\ncolor='#abcdef'",
		)
		.unwrap();
		install(&source, &dir, false).unwrap();
		assert!(install(&source, &dir, false).is_err());
		fs::write(
			&source,
			"format_version=2\nversion=2\n[[rule]]\nwhen=['body']\ncolor='#123456'",
		)
		.unwrap();
		install(&source, &dir, false).unwrap();
		fs::write(&source, "bad").unwrap();
		assert!(install(&source, &dir, true).is_err());
		assert!(load(&["a".into()], Some(&dir)).is_ok());
		assert!(load(&["missing".into()], Some(&dir)).is_err());
		assert_eq!(
			catalog(Some(&dir), None).len(),
			Stylesheet::READER_THEMES.len() + 1
		);
		fs::write(
			&source,
			"format_version=2\nversion=3\n[[rule]]\nwhen=['body']\ncolor='#123456'",
		)
		.unwrap();
		install(&source, &dir, true).unwrap();
		assert_eq!(
			fs::read(&source).unwrap(),
			fs::read(dir.join("a.mvss.toml")).unwrap()
		);
	}
	#[test]
	fn leftmost_wins() {
		let tmp = tempfile::tempdir().unwrap();
		fs::write(
			tmp.path().join("a.mvss.toml"),
			"format_version=2\nversion=1\n[[rule]]\nwhen=['body']\ncolor='#123456'",
		)
		.unwrap();
		let s = load(&["a".into(), "dark".into()], Some(tmp.path())).unwrap();
		assert_eq!(
			s.rule(markview_core::style::Condition::Body).color,
			Some(markview_core::style::Color(0x123456ff))
		);
	}
}

#[cfg(test)]
mod cascade_tests {
	use super::*;
	/// Load named styles without caring about the CJK variant.
	fn load(ids: &[String], dir: Option<&Path>) -> Result<Arc<Stylesheet>> {
		load_with_cjk_type(ids, dir, CjkType::Sc)
	}
	#[test]
	fn bundled_dark_only_overrides_its_explicit_fields() {
		let tmp = tempfile::tempdir().unwrap();
		fs::write(
			tmp.path().join("fonts.mvss.toml"),
			"format_version=2\nversion=1\n[[rule]]\nwhen=['em']\nfont=[{family='Custom'}]",
		)
		.unwrap();
		let sheet =
			load(&["dark".into(), "fonts".into()], Some(tmp.path())).unwrap();
		assert_eq!(
			sheet
				.rule(markview_core::style::Condition::Em)
				.font
				.as_ref()
				.unwrap()[0]
				.family,
			"Custom"
		);
	}
	#[test]
	fn missing_selected_styles_remain_removable_and_ids_are_portable() {
		let tmp = tempfile::tempdir().unwrap();
		let entries = catalog(Some(tmp.path()), Some(&["missing".into()]));
		assert!(
			entries
				.iter()
				.any(|e| e.id == "missing" && e.error.is_some())
		);
		for id in ["../a", "a/b", "a\\b", "DARK", "a:", "a\n"] {
			assert!(validate_id(id).is_err());
		}
		assert!(validate_id("纸 与 墨").is_ok());
	}
}

#[cfg(test)]
mod theme_tests {
	use super::*;
	use markview_core::style::{Color, Condition};

	#[test]
	fn fallback_is_hidden_reserved_and_beneath_explicit_rules() {
		let dir = tempfile::tempdir().unwrap();
		let source = dir.path().join("builtin.mvss.toml");
		fs::write(&source, "format_version=2\nversion=1\n").unwrap();
		assert!(install(&source, dir.path(), true).is_err());
		assert!(
			load_with_cjk_type(&["builtin".into()], None, CjkType::Sc).is_err()
		);
		assert!(
			!catalog(Some(dir.path()), Some(&["builtin".into()]))
				.iter()
				.any(|e| e.id == "builtin")
		);
		let empty = load_with_cjk_type(&[], None, CjkType::Sc).unwrap();
		assert_eq!(
			empty.rule(Condition::Body).background,
			Stylesheet::builtin().rule(Condition::Body).background
		);
		fs::write(
			dir.path().join("custom.mvss.toml"),
			"format_version=2\nversion=1\n[[rule]]\nwhen=['p']\nspace_after=1.23\n",
		)
		.unwrap();
		for id in ["light", "dark"] {
			let sheet = load_with_cjk_type(
				&[id.into(), "custom".into()],
				Some(dir.path()),
				CjkType::Sc,
			)
			.unwrap();
			assert_eq!(sheet.rule(Condition::P).space_after, Some(1.23));
		}
	}

	#[test]
	fn every_reader_theme_loads_with_shared_fonts_and_owns_its_palette() {
		let entries = catalog(None, None);
		for &id in Stylesheet::READER_THEMES {
			assert!(entries.iter().any(|e| e.id == id && e.error.is_none()));
			let sheet =
				load_with_cjk_type(&[id.into()], None, CjkType::Sc).unwrap();
			assert!(sheet.fontdefs.contains_key("serif[cjk]"));
			assert!(sheet.fontdefs["emoji"].emoji);
			assert!(sheet.rule(Condition::CodeBlock).font.is_some());
			assert_ne!(sheet.rule(Condition::Body).background, Some(Color(0)));
			assert!(validate_id(&id.to_uppercase()).is_err());
		}
		let print = load_for_pdf(None, None, CjkType::Sc).unwrap();
		assert_eq!(
			print.rule(Condition::Body).background,
			Some(Color(0xffffffff))
		);
		assert!(print.fontdefs.contains_key("serif[cjk]"));
	}
}

#[cfg(test)]
mod target_tests {
	use super::*;
	use markview_core::style::{Color, Condition};

	fn write_style(dir: &Path, id: &str, targets: &str) {
		fs::write(
			dir.join(format!("{id}{SUFFIX}")),
			format!("format_version=2\nversion=1\n{targets}\n[[rule]]\nwhen=['body']\ncolor='#123456'"),
		).unwrap();
	}

	#[test]
	fn destinations_filter_discovery_and_reject_incompatible_layers() {
		let dir = tempfile::tempdir().unwrap();
		for (id, targets) in [
			("screen", "targets=['ui']"),
			("paper", "targets=['pdf']"),
			("shared", "targets=['ui','pdf']"),
			("legacy", ""),
		] {
			write_style(dir.path(), id, targets);
		}
		for target in [StyleTarget::Ui, StyleTarget::Pdf] {
			let entries = catalog_for(Some(dir.path()), None, target);
			for (id, allowed) in [
				("screen", target == StyleTarget::Ui),
				("paper", target == StyleTarget::Pdf),
				("shared", true),
				("legacy", true),
				("print", target == StyleTarget::Pdf),
				("light", target == StyleTarget::Ui),
			] {
				assert_eq!(
					entries.iter().any(|entry| entry.id == id),
					allowed,
					"{target:?}: {id}"
				);
				let ids = [id.into()];
				let result = match target {
					StyleTarget::Ui => {
						load_with_cjk_type(&ids, Some(dir.path()), CjkType::Sc)
					}
					StyleTarget::Pdf => {
						load_for_pdf(Some(&ids), Some(dir.path()), CjkType::Sc)
					}
				};
				assert_eq!(result.is_ok(), allowed, "{target:?}: {id}");
			}
		}
		// Every layer is checked, including one beneath a shared override.
		let ids = ["shared".into(), "screen".into()];
		let error = load_for_pdf(Some(&ids), Some(dir.path()), CjkType::Sc)
			.unwrap_err()
			.to_string();
		assert!(error.contains("screen") && error.contains("pdf"));
		let sheet = load_for_pdf(
			Some(&["shared".into(), "paper".into()]),
			Some(dir.path()),
			CjkType::Sc,
		)
		.unwrap();
		assert_eq!(sheet.rule(Condition::Body).color, Some(Color(0x123456ff)));
	}

	#[test]
	fn changed_targets_leave_selected_styles_removable_and_previewable() {
		let dir = tempfile::tempdir().unwrap();
		write_style(dir.path(), "draft", "targets=['ui']");
		let ids = ["draft".into()];
		assert!(
			catalog(Some(dir.path()), Some(&ids))
				.iter()
				.find(|entry| entry.id == "draft")
				.unwrap()
				.error
				.is_none()
		);
		write_style(dir.path(), "draft", "targets=['pdf']");
		assert!(
			!catalog(Some(dir.path()), None)
				.iter()
				.any(|entry| entry.id == "draft")
		);
		assert!(
			catalog(Some(dir.path()), Some(&ids))
				.iter()
				.find(|entry| entry.id == "draft")
				.unwrap()
				.error
				.is_some()
		);
		assert!(
			load_with_cjk_type(&ids, Some(dir.path()), CjkType::Sc).is_err()
		);
		assert!(
			load_for_preview(Some(&ids), Some(dir.path()), CjkType::Sc).is_ok()
		);
		assert!(
			load_for_preview(Some(&["print".into()]), None, CjkType::Sc)
				.is_ok()
		);
	}
}

#[cfg(test)]
mod paper_theme_tests {
	use super::*;
	use markview_core::style::{Color, Condition};

	#[test]
	fn bundled_paper_themes_are_export_only_and_inherit_readable_furniture() {
		let paper = catalog_for(None, None, StyleTarget::Pdf);
		let reader = catalog(None, None);
		for &id in Stylesheet::PDF_THEMES {
			assert!(
				paper
					.iter()
					.any(|entry| entry.id == id && entry.error.is_none())
			);
			assert!(!reader.iter().any(|entry| entry.id == id));
			assert!(reserved(id));
			assert!(validate_id(&id.to_uppercase()).is_err());
			let sheet =
				load_for_pdf(Some(&[id.into()]), None, CjkType::Sc).unwrap();
			assert_eq!(
				sheet.rule(Condition::Page).background,
				Some(Color(0xffffffff))
			);
			assert!(sheet.fontdefs.contains_key("serif[cjk]"));
			for role in [
				Condition::PageHeader,
				Condition::PageFooter,
				Condition::PageNumber,
			] {
				assert_eq!(sheet.rule(role).size, Some(0.75), "{id}: {role:?}");
			}
		}
		let mono =
			load_for_pdf(Some(&["monochrome".into()]), None, CjkType::Sc)
				.unwrap();
		assert_eq!(
			mono.rule(Condition::CodeBlock).theme.as_deref(),
			Some("none")
		);
	}
}
