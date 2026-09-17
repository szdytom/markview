//! User stylesheet discovery and atomic installation. Bundled IDs cannot be shadowed.
use anyhow::{Context, Result, bail};
use markview_core::style::{CjkType, Stylesheet};
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
pub fn validate_id(id: &str) -> Result<()> {
	if id.is_empty()
		|| id == "."
		|| id == ".."
		|| id.contains(['/', '\\', ':'])
		|| id.chars().any(char::is_control)
		|| id.ends_with(['.', ' '])
		|| (["light", "dark", "print"]
			.iter()
			.any(|reserved| id.eq_ignore_ascii_case(reserved))
			&& !matches!(id, "light" | "dark" | "print"))
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
	load_over(ids, dir, cjk_type, Stylesheet::bundled(false))
}

/// The stylesheet a PDF export starts from: the bundled print sheet, with any
/// named styles layered on top. The reader's theme never applies to paper.
pub fn load_for_pdf(
	ids: Option<&[String]>,
	dir: Option<&Path>,
	cjk_type: CjkType,
) -> Result<Arc<Stylesheet>> {
	match ids {
		Some(ids) => load_over(ids, dir, cjk_type, Stylesheet::bundled_print()),
		None => {
			let mut sheet = (*Stylesheet::bundled_print()).clone();
			sheet.set_cjk_type(cjk_type);
			Ok(Arc::new(sheet))
		}
	}
}

fn load_over(
	ids: &[String],
	dir: Option<&Path>,
	cjk_type: CjkType,
	base: Arc<Stylesheet>,
) -> Result<Arc<Stylesheet>> {
	let mut sheet = (*base).clone();
	for id in ids.iter().rev() {
		validate_id(id)?;
		match id.as_str() {
			"light" => sheet.merge(&Stylesheet::bundled(false)),
			"dark" => sheet.merge(&Stylesheet::bundled_rules(true)),
			"print" => sheet.merge(&Stylesheet::bundled_print()),
			_ => {
				let path = dir
					.context("No user stylesheet directory")?
					.join(format!("{id}{SUFFIX}"));
				let source = fs::read_to_string(&path).with_context(|| {
					format!("Cannot read {}", path.display())
				})?;
				let next = Stylesheet::parse(&source)
					.with_context(|| path.display().to_string())?;
				sheet.merge(&next);
			}
		}
	}
	sheet.set_cjk_type(cjk_type);
	Ok(Arc::new(sheet))
}
#[derive(Clone, Debug)]
pub struct Entry {
	pub id: String,
	pub name: String,
	pub source: String,
	pub error: Option<String>,
}
pub fn scan(dir: Option<&Path>) -> Vec<Entry> {
	let mut entries = vec![];
	for id in ["light", "dark"] {
		entries.push(Entry {
			id: id.into(),
			name: if id == "light" { "Light" } else { "Dark" }.into(),
			source: "Bundled".into(),
			error: None,
		});
	}
	if let Some(dir) = dir
		&& let Ok(files) = fs::read_dir(dir)
	{
		for file in files.flatten() {
			let path = file.path();
			if !path.is_file() {
				continue;
			}
			let Some(id) = path
				.file_name()
				.and_then(|n| n.to_str())
				.and_then(|n| n.strip_suffix(SUFFIX))
			else {
				continue;
			};
			if matches!(id, "light" | "dark") {
				continue;
			}
			let result = validate_id(id)
				.and_then(|()| Stylesheet::parse(&fs::read_to_string(&path)?));
			entries.push(Entry {
				id: id.into(),
				name: result
					.as_ref()
					.ok()
					.and_then(|s| s.meta.name.clone())
					.unwrap_or_else(|| id.into()),
				source: path.display().to_string(),
				error: result.err().map(|e| format!("{e:#}")),
			});
		}
	}
	entries[2..].sort_by(|a, b| a.id.cmp(&b.id));
	entries
}
pub fn catalog(dir: Option<&Path>, selected: Option<&[String]>) -> Vec<Entry> {
	let mut entries = scan(dir);
	for id in selected.into_iter().flatten() {
		if !entries.iter().any(|entry| &entry.id == id) {
			entries.push(Entry {
				id: id.clone(),
				name: id.clone(),
				source: dir
					.map(|p| {
						p.join(format!("{id}{SUFFIX}")).display().to_string()
					})
					.unwrap_or_default(),
				error: Some("Selected stylesheet is missing".into()),
			});
		}
	}
	entries
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
	if matches!(id.to_ascii_lowercase().as_str(), "light" | "dark") {
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
		assert_eq!(scan(Some(&dir)).len(), 3);
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
