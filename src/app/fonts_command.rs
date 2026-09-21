//! The `markview fonts` commands.
//!
//! A download directory is another `--fonts` directory, so these commands only
//! ever report what the declarations offer and what the directory holds; the
//! families themselves are named by the files they contain.
use crate::{cli::FontsCommand, logging::report};
use anyhow::{Context, Result, bail};
use markview_core::{
	fonts::FontConfig,
	style::{FontFamily, Stylesheet},
};
use std::{
	collections::BTreeSet,
	io::IsTerminal,
	path::{Path, PathBuf},
};

pub(super) fn run(command: &FontsCommand, offline: bool) -> Result<()> {
	match command {
		FontsCommand::Path => {
			let dir = directory()?;
			report(format_args!("{}\n", dir.display()));
			Ok(())
		}
		FontsCommand::List { style, file, all } => {
			list(style.as_deref(), file.as_deref(), *all)
		}
		FontsCommand::Verify { style, file } => {
			verify(style.as_deref(), file.as_deref())
		}
		FontsCommand::Download {
			style,
			file,
			families,
			force,
			dry_run,
			jobs,
		} => download(
			style.as_deref(),
			file.as_deref(),
			families,
			*force,
			*dry_run,
			*jobs,
			offline,
		),
	}
}

fn directory() -> Result<PathBuf> {
	crate::fonts::directory()
		.context("No user configuration directory to hold downloaded fonts")
}

/// Every family the declarations offer, lowest layer first.
fn declared(
	style: Option<&str>,
	file: Option<&Path>,
) -> Result<Vec<(String, Vec<FontFamily>)>> {
	if let Some(path) = file {
		let sheet = crate::stylesheet::validate(path)?;
		return Ok(vec![("@file".into(), sheet.font_families.clone())]);
	}
	let builtin = Stylesheet::builtin();
	let directory = crate::stylesheet::directory();
	let mut entries = crate::stylesheet::catalog(directory.as_deref(), None);
	// The builtin recommendations are the catalogue's own layer, so they join
	// only a whole-catalogue request. Naming one stylesheet asks for the
	// families that sheet brings, which may be none at all.
	let mut sheets = Vec::new();
	if let Some(id) = style {
		entries.retain(|entry| entry.id == id);
		if entries.is_empty() {
			bail!("No stylesheet named {id:?}");
		}
	} else {
		sheets.push(("builtin".to_string(), builtin.font_families.clone()));
	}
	for entry in entries {
		sheets.push((entry.id, entry.font_families));
	}
	Ok(sheets)
}

/// The catalogue these commands work from.
fn families(
	style: Option<&str>,
	file: Option<&Path>,
) -> Result<Vec<crate::fonts::Family>> {
	let sheets = declared(style, file)?;
	let refs: Vec<(&str, &[FontFamily])> = sheets
		.iter()
		.map(|(owner, families)| (owner.as_str(), families.as_slice()))
		.collect();
	let dir = crate::fonts::directory();
	// A command line has no `--fonts` of its own; what is installed is the
	// machine's own set plus whatever the download directory holds.
	Ok(crate::fonts::catalog(
		refs,
		dir.as_deref(),
		&FontConfig::default(),
	))
}

fn state_label(state: crate::fonts::State) -> &'static str {
	match state {
		crate::fonts::State::Downloaded => "on disk",
		crate::fonts::State::Provided => "installed",
		crate::fonts::State::Missing => "missing",
	}
}

fn bytes_label(bytes: u64) -> String {
	if bytes >= 1024 * 1024 {
		format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
	} else if bytes >= 1024 {
		format!("{} KiB", bytes / 1024)
	} else {
		format!("{bytes} B")
	}
}

fn list(style: Option<&str>, file: Option<&Path>, all: bool) -> Result<()> {
	let catalog = families(style, file)?;
	let dir = crate::fonts::directory();
	for family in &catalog {
		// Without `--all` the list is what still needs downloading.
		if !all && family.state != crate::fonts::State::Missing {
			continue;
		}
		let license = family.family.license.as_deref().unwrap_or("-");
		report(format_args!(
			"{:<22} {:<10} {:>9}  {:<28} {:<10} {}\n",
			family.family.id,
			state_label(family.state),
			bytes_label(family.bytes),
			family.family.display_name(),
			license,
			family.owners.join(", ")
		));
	}
	let Some(dir) = dir else {
		return Ok(());
	};
	let (total, unclaimed) = occupancy(&catalog, &dir);
	report(format_args!(
		"{}: {} families, {} on disk\n",
		dir.display(),
		catalog.len(),
		bytes_label(total)
	));
	for name in &unclaimed {
		report(format_args!("  {name} (not declared by any stylesheet)\n"));
	}
	if total > crate::fonts::SOFT_TOTAL_BYTES {
		report(format_args!(
			"  the directory is larger than {} MiB\n",
			crate::fonts::SOFT_TOTAL_BYTES / (1024 * 1024)
		));
	}
	Ok(())
}

/// The directory's total size, and the files no declaration claims.
fn occupancy(
	catalog: &[crate::fonts::Family],
	dir: &Path,
) -> (u64, Vec<String>) {
	let claimed: BTreeSet<&str> = catalog
		.iter()
		.flat_map(|family| family.files.iter().map(String::as_str))
		.collect();
	let Ok(entries) = std::fs::read_dir(dir) else {
		return (0, Vec::new());
	};
	let mut total = 0;
	let mut unclaimed = Vec::new();
	let mut names: Vec<String> = entries
		.flatten()
		.filter_map(|entry| {
			let meta = entry.metadata().ok()?;
			meta.is_file()
				.then(|| entry.file_name().to_string_lossy().into_owned())
		})
		.collect();
	names.sort();
	for name in names {
		let size = std::fs::metadata(dir.join(&name))
			.map(|meta| meta.len())
			.unwrap_or(0);
		total += size;
		if !name.starts_with('.') && !claimed.contains(name.as_str()) {
			unclaimed.push(name);
		}
	}
	(total, unclaimed)
}

fn verify(style: Option<&str>, file: Option<&Path>) -> Result<()> {
	let catalog = families(style, file)?;
	let mut problems = 0usize;
	for family in &catalog {
		let detail = match family.state {
			crate::fonts::State::Downloaded => {
				format!("{} on disk", bytes_label(family.bytes))
			}
			crate::fonts::State::Provided => "installed".to_string(),
			crate::fonts::State::Missing => {
				problems += 1;
				"missing".to_string()
			}
		};
		report(format_args!("{:<22} {}\n", family.family.id, detail));
	}
	// A file that cannot be read as a font is invisible to the catalogue, so
	// only a direct look at the directory can name it.
	let Some(dir) = crate::fonts::directory() else {
		bail!("No user configuration directory to hold downloaded fonts");
	};
	let described: BTreeSet<String> = markview_core::fonts::describe(&dir)
		.into_iter()
		.map(|face| face.file)
		.collect();
	if let Ok(entries) = std::fs::read_dir(&dir) {
		let mut names: Vec<String> = entries
			.flatten()
			.filter(|entry| entry.path().is_file())
			.map(|entry| entry.file_name().to_string_lossy().into_owned())
			.collect();
		names.sort();
		for name in names {
			if name.ends_with(".tmp") || described.contains(&name) {
				continue;
			}
			problems += 1;
			report(format_args!(
				"{}: not a readable font\n",
				dir.join(&name).display()
			));
		}
	}
	if problems > 0 {
		bail!("{problems} font problem(s)");
	}
	Ok(())
}

fn download(
	style: Option<&str>,
	file: Option<&Path>,
	requested: &[String],
	force: bool,
	dry_run: bool,
	jobs: usize,
	offline: bool,
) -> Result<()> {
	// The declarations are local, so a dry run still works offline; only the
	// transfers themselves need the network.
	if offline && !dry_run {
		bail!("Offline: font downloads are unavailable; run without --offline");
	}
	let catalog = families(style, file)?;
	for id in requested {
		if !catalog.iter().any(|family| &family.family.id == id) {
			bail!("No font family named {id:?}");
		}
	}
	let ids: Vec<String> = if requested.is_empty() {
		catalog
			.iter()
			.map(|family| family.family.id.clone())
			.collect()
	} else {
		requested.to_vec()
	};
	// Naming a family asks for that family even when an installed face already
	// covers it; naming none asks for what is missing.
	let scope = if force {
		crate::fonts::Scope::All
	} else if requested.is_empty() {
		crate::fonts::Scope::Missing
	} else {
		crate::fonts::Scope::Named
	};
	let selected: Vec<FontFamily> = crate::fonts::select(&catalog, &ids, scope)
		.into_iter()
		.cloned()
		.collect();
	if selected.is_empty() {
		report(format_args!("Everything selected is already downloaded\n"));
		return Ok(());
	}
	if dry_run {
		for family in &selected {
			let sources: Vec<&str> = family
				.source
				.iter()
				.map(|source| source.label().unwrap_or("source"))
				.collect();
			report(format_args!(
				"{:<22} {} ({})\n",
				family.id,
				family.display_name(),
				sources.join(", ")
			));
		}
		return Ok(());
	}
	let dir = directory()?;
	let transport = crate::images::Downloader::new()?;
	let terminal = std::io::stdout().is_terminal();
	let mut last: std::collections::HashMap<String, (usize, String)> =
		std::collections::HashMap::new();
	let summary = crate::fonts::run(
		&selected,
		&dir,
		&transport,
		jobs,
		std::sync::Arc::new(|_: &str| false),
		&mut |progress| {
			let line = describe(&progress);
			if terminal {
				report(format_args!("\r\x1b[K{line}"));
			} else {
				// One line per change of file or phase, so a piped log shows
				// what happened without repeating every chunk.
				let key = (progress.phase as usize, line.clone());
				if last.get(&progress.id) != Some(&key) {
					last.insert(progress.id.clone(), key);
					report(format_args!("{line}\n"));
				}
			}
		},
	);
	if terminal {
		report(format_args!("\r\x1b[K"));
	}
	for (id, reason) in &summary.failed {
		report(format_args!("{id}: {reason}\n"));
	}
	report(format_args!(
		"{} families stored, {} downloaded, {} failed, {} cancelled\n",
		summary.stored,
		bytes_label(summary.bytes),
		summary.failed.len(),
		summary.cancelled.len()
	));
	if !summary.failed.is_empty() {
		bail!("{} font famil(ies) failed", summary.failed.len());
	}
	Ok(())
}

/// One line of progress, for a terminal or a log.
fn describe(progress: &crate::fonts::Progress) -> String {
	let phase = match progress.phase {
		crate::fonts::Phase::Queued => "queued",
		crate::fonts::Phase::Downloading => "downloading",
		crate::fonts::Phase::Extracting => "extracting",
		crate::fonts::Phase::Done => "done",
		crate::fonts::Phase::Failed => "failed",
		crate::fonts::Phase::Cancelled => "cancelled",
	};
	let files = if progress.files_total > 0 {
		format!(" {}/{} files", progress.files_done, progress.files_total)
	} else {
		String::new()
	};
	let current = progress
		.current
		.as_deref()
		.map(|name| format!(" {name}"))
		.unwrap_or_default();
	format!(
		"{}: {phase}{current}{files} {}",
		progress.id,
		bytes_label(progress.bytes_done)
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn download_command(dry_run: bool) -> FontsCommand {
		FontsCommand::Download {
			style: None,
			file: None,
			families: Vec::new(),
			force: false,
			dry_run,
			jobs: 4,
		}
	}

	/// Reading a declaration is local; only a transfer needs the network.
	#[test]
	fn an_offline_download_is_refused() {
		let error = run(&download_command(false), true).unwrap_err();
		let text = format!("{error:#}");
		assert!(text.contains("Offline"), "{text}");
		// A dry run names what it would fetch without touching the network.
		assert!(run(&download_command(true), true).is_ok());
	}

	/// Naming one stylesheet asks for its families, not the builtin
	/// recommendations it happens to layer over.
	#[test]
	fn a_style_selection_does_not_pull_in_the_builtin_families() {
		// `light` declares no families of its own.
		let sheets = declared(Some("light"), None).unwrap();
		assert_eq!(sheets.len(), 1);
		assert_eq!(sheets[0].0, "light");
		assert!(sheets[0].1.is_empty(), "{:?}", sheets[0].1);
		// The whole catalogue still leads with the recommendations.
		let sheets = declared(None, None).unwrap();
		assert_eq!(sheets[0].0, "builtin");
		assert_eq!(sheets[0].1.len(), 7);
	}

	#[test]
	fn a_command_that_only_reads_the_disk_works_offline() {
		assert!(run(&FontsCommand::Path, true).is_ok());
	}
}
