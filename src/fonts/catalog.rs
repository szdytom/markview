//! The user font directory: what each file declares, the catalogue a
//! stylesheet builds from those families, and how a download's files are
//! named, staged and installed.
use anyhow::{Context, Result, bail};
use markview_layout::{
	fonts::{FontConfig, is_font_file},
	style::FontFamily,
};
use std::{
	fs,
	path::{Path, PathBuf},
};

use super::{
	MAX_FILE_BYTES,
	archive::{sha256_hex, temp_path},
};

/// What one font file on disk says about itself.
///
/// A downloaded family is only ever known through its own files: the reader
/// reads their names and attributes instead of trusting a manifest, so a file
/// copied in by hand is described exactly like one this application wrote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaceInfo {
	/// The file name inside its directory, which is what a reader shows.
	pub file: String,
	/// Family names the file declares, the typographic family first. Every
	/// localized spelling is kept, because one family may be named differently
	/// per language.
	pub families: Vec<String>,
	/// The attributes of the first face. A collection's faces differ from one
	/// another, and only the family names are needed to recognize a family.
	pub weight: u16,
	pub style: FaceStyle,
	/// How many faces the file holds; a collection holds several.
	pub faces: usize,
	pub bytes: u64,
}

/// The slant a face declares for itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceStyle {
	Normal,
	Italic,
	Oblique,
}

/// Reads every font file directly inside `directory` and reports what each one
/// says about itself, in file-name order. A file that cannot be read is
/// skipped, exactly as the shaper's own scan does.
pub fn describe(directory: &Path) -> Vec<FaceInfo> {
	let Ok(entries) = std::fs::read_dir(directory) else {
		return Vec::new();
	};
	let mut paths: Vec<PathBuf> =
		entries.flatten().map(|entry| entry.path()).collect();
	// Sorting keeps the report stable on every filesystem.
	paths.sort();
	paths.iter().filter_map(|path| describe_one(path)).collect()
}

pub(crate) fn describe_one(path: &Path) -> Option<FaceInfo> {
	if !is_font_file(path) {
		return None;
	}
	let file = path.file_name()?.to_str()?.to_owned();
	let bytes = std::fs::read(path).ok()?;
	let bytes_len = bytes.len() as u64;
	let data = swash::FontDataRef::new(&bytes)?;
	// The typographic family groups every weight of one family; the plain
	// family is what an older face declares instead. Every face contributes,
	// because a collection may hold several families.
	let mut typographic: Vec<String> = Vec::new();
	let mut plain: Vec<String> = Vec::new();
	let mut first: Option<(u16, FaceStyle)> = None;
	for font in data.fonts() {
		if first.is_none() {
			let attributes = font.attributes();
			let style = match attributes.style() {
				swash::Style::Italic => FaceStyle::Italic,
				swash::Style::Oblique(_) => FaceStyle::Oblique,
				swash::Style::Normal => FaceStyle::Normal,
			};
			first = Some((attributes.weight().0, style));
		}
		collect_family_names(font, &mut typographic, &mut plain);
	}
	typographic.extend(plain);
	let (weight, style) = first?;
	Some(FaceInfo {
		file,
		families: typographic,
		// A collection's faces differ; these describe its first face and are
		// shown for it alone.
		weight,
		style,
		faces: data.len(),
		bytes: bytes_len,
	})
}

fn collect_family_names(
	font: swash::FontRef<'_>,
	typographic: &mut Vec<String>,
	plain: &mut Vec<String>,
) {
	for string in font.localized_strings() {
		if !string.is_decodable() {
			continue;
		}
		let name = string.chars().collect::<String>();
		let name = name.trim();
		if name.is_empty() {
			continue;
		}
		let name = name.to_owned();
		match string.id() {
			swash::StringId::TypographicFamily => push_name(typographic, name),
			swash::StringId::Family => push_name(plain, name),
			_ => {}
		}
	}
}

fn push_name(names: &mut Vec<String>, name: String) {
	if !names.contains(&name) {
		names.push(name);
	}
}

/// What the reader knows about one catalogued family.
#[derive(Clone, Debug)]
pub struct Family {
	pub family: FontFamily,
	/// The catalogued sheets that declare this id, lowest layer first.
	pub owners: Vec<String>,
	pub state: State,
	/// The local files that serve the family, in file-name order.
	pub files: Vec<String>,
	/// What those files occupy.
	pub bytes: u64,
}

/// How a family is already satisfied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
	/// Its own files are in the download directory.
	Downloaded,
	/// A name it lists is already installable, so there is nothing to fetch.
	Provided,
	/// Nothing provides it yet.
	Missing,
}

/// Builds the catalogue from the sheets that declare families, lowest layer
/// first, together with what the download directory holds.
///
/// A higher layer replaces a whole entry of the same id, exactly as font
/// definitions cascade. Ownership is kept per declaring sheet, because several
/// sheets may offer the same family.
pub fn catalog<'a>(
	sheets: impl IntoIterator<Item = (&'a str, &'a [FontFamily])>,
	dir: Option<&Path>,
	config: &FontConfig,
) -> Vec<Family> {
	let faces = dir.map(describe).unwrap_or_default();
	catalog_faces(sheets, &faces, config)
}

pub(crate) fn catalog_faces<'a>(
	sheets: impl IntoIterator<Item = (&'a str, &'a [FontFamily])>,
	faces: &[FaceInfo],
	config: &FontConfig,
) -> Vec<Family> {
	let mut out: Vec<Family> = Vec::new();
	for (owner, families) in sheets {
		for family in families {
			let served = |family: &FontFamily| -> Vec<String> {
				faces
					.iter()
					.filter(|face| {
						face.families
							.iter()
							.any(|name| family.lookfor.contains(name))
					})
					.map(|face| face.file.clone())
					.collect()
			};
			if let Some(existing) =
				out.iter_mut().find(|entry| entry.family.id == family.id)
			{
				// A higher layer replaces the whole entry, so the files it is
				// served by are read again from its new name list.
				existing.family = family.clone();
				existing.files = served(family);
				existing.bytes = faces
					.iter()
					.filter(|face| existing.files.contains(&face.file))
					.map(|face| face.bytes)
					.sum();
				existing.state = state_of(family, &existing.files, config);
				if !existing.owners.iter().any(|name| name == owner) {
					existing.owners.push(owner.to_owned());
				}
				continue;
			}
			let files = served(family);
			let bytes = faces
				.iter()
				.filter(|face| files.contains(&face.file))
				.map(|face| face.bytes)
				.sum();
			out.push(Family {
				state: state_of(family, &files, config),
				family: family.clone(),
				owners: vec![owner.to_owned()],
				files,
				bytes,
			});
		}
	}
	out
}

fn state_of(
	family: &FontFamily,
	files: &[String],
	config: &FontConfig,
) -> State {
	if !files.is_empty() {
		return State::Downloaded;
	}
	if markview_layout::fonts::provided_family(config, &family.lookfor)
		.is_some()
	{
		return State::Provided;
	}
	State::Missing
}

/// How much of a selection to take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
	/// Only families nothing provides yet. This is what the bulk action means:
	/// a family an installed face already covers is not "missing".
	Missing,
	/// Also families an installed face covers: a named family downloads a
	/// copy, and the bulk **Download All** takes every family not on disk.
	Named,
	/// Everything named, even what is already on disk.
	All,
}

/// The families a selection picked out of the catalogue, in catalogue order.
pub fn select<'a>(
	families: &'a [Family],
	ids: &[String],
	scope: Scope,
) -> Vec<&'a FontFamily> {
	families
		.iter()
		.filter(|entry| ids.iter().any(|id| id == &entry.family.id))
		.filter(|entry| match scope {
			Scope::Missing => entry.state == State::Missing,
			Scope::Named => entry.state != State::Downloaded,
			Scope::All => true,
		})
		.map(|entry| &entry.family)
		.collect()
}

/// Verifies a downloaded body and gives it the name it will be installed under.
///
/// The name is built from the family and the file's own source name, so the
/// same logical file lands on one name whichever mirror produced it. Nothing
/// in the download directory is touched here.
pub(super) fn stage(
	family: &FontFamily,
	temp: &Path,
	source_name: &str,
	sha256: Option<&str>,
) -> Result<Staged> {
	let bytes = fs::read(temp)
		.with_context(|| format!("Cannot read {}", temp.display()))?;
	if bytes.len() as u64 > MAX_FILE_BYTES {
		bail!("Font file exceeds {} MiB", MAX_FILE_BYTES / (1024 * 1024));
	}
	if let Some(expected) = sha256 {
		let actual = sha256_hex(&bytes);
		if !actual.eq_ignore_ascii_case(expected) {
			bail!("sha256 mismatch for {source_name}");
		}
	}
	if !markview_layout::fonts::is_font(&bytes) {
		bail!("{source_name} is not a font file");
	}
	Ok(Staged {
		temp: temp.to_owned(),
		name: local_name(&family.id, source_name, &bytes),
		bytes: bytes.len() as u64,
		installed: false,
	})
}

/// The local name one file is stored under.
///
/// `source_name` is the name the file has at its origin — a URL's last path
/// segment, or an archive member's whole path. Its last segment becomes the
/// readable prefix, while the whole name is hashed together with the family
/// id: two mirrors of one family agree on where their copy of a file lands,
/// and two members of one archive that share a basename but not a path stay
/// apart.
pub fn local_name(family: &str, source_name: &str, bytes: &[u8]) -> String {
	let basename = source_name.rsplit('/').next().unwrap_or(source_name);
	let decoded = percent_encoding::percent_decode_str(basename)
		.decode_utf8_lossy()
		.into_owned();
	let cleaned: String = decoded
		.chars()
		.map(|c| {
			if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
				c
			} else {
				'_'
			}
		})
		.collect();
	let (mut stem, declared) = match cleaned.rsplit_once('.') {
		Some((stem, extension)) if !stem.is_empty() => {
			(stem.to_owned(), Some(extension.to_ascii_lowercase()))
		}
		_ => (cleaned, None),
	};
	let extension = match declared.as_deref() {
		Some(extension @ ("ttf" | "otf" | "ttc" | "otc")) => {
			extension.to_owned()
		}
		// A member with no usable extension takes the one its outlines imply.
		_ => outline_extension(bytes).to_owned(),
	};
	if stem.is_empty() || stem == "." || stem == ".." {
		stem = "font".into();
	}
	// The hash is fixed length, so bounding the stem keeps the whole name well
	// inside a file name's limit while the readable prefix stays.
	stem.truncate(96);
	format!("{stem}-{:016x}.{extension}", key_hash(family, source_name))
}

fn outline_extension(bytes: &[u8]) -> &'static str {
	if markview_layout::fonts::is_postscript_outline(bytes) {
		"otf"
	} else {
		"ttf"
	}
}

/// A stable hash of one logical file's identity, used only to keep names
/// apart. FNV-1a never changes between builds, unlike `DefaultHasher`.
fn key_hash(family: &str, name: &str) -> u64 {
	const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
	const PRIME: u64 = 0x0000_0100_0000_01b3;
	family
		.bytes()
		.chain([0])
		.chain(name.bytes())
		.fold(OFFSET, |hash, byte| {
			(hash ^ u64::from(byte)).wrapping_mul(PRIME)
		})
}

/// The last path segment of a URL, percent-decoded.
pub(super) fn url_basename(url: &str) -> String {
	url::Url::parse(url)
		.ok()
		.and_then(|url| {
			url.path_segments()
				.and_then(|mut path| path.next_back())
				.map(str::to_owned)
		})
		.filter(|name| !name.is_empty())
		.unwrap_or_else(|| "font".into())
}

/// One verified file of a source, waiting for the rest of it to succeed.
///
/// It is still a temporary file with a `.tmp` name, so the shaper's directory
/// scan cannot see it and an installed file is never replaced before the whole
/// source is known to be sound.
pub(super) struct Staged {
	pub(super) temp: PathBuf,
	/// The name this file takes once it is installed.
	pub(super) name: String,
	pub(super) bytes: u64,
	/// Set once the file has been moved into place, because a temp path that
	/// no longer exists must not be removed again on the way out.
	pub(super) installed: bool,
}

impl Drop for Staged {
	fn drop(&mut self) {
		if !self.installed {
			let _ = fs::remove_file(&self.temp);
		}
	}
}

/// Moves staged files into place, keeping what they replace until all of them
/// have landed.
///
/// An existing file is moved aside rather than removed, so a failure part way
/// through puts every replaced copy back and removes everything already
/// installed. Two staged files may not claim one destination: that would
/// silently drop one of them.
pub(super) fn install(dir: &Path, staged: &mut [Staged]) -> Result<()> {
	for (index, entry) in staged.iter().enumerate() {
		if staged[..index].iter().any(|other| other.name == entry.name) {
			bail!(
				"Two files of one source map to {:?}; give them distinct names",
				entry.name
			);
		}
	}
	let mut placed: Vec<PathBuf> = Vec::new();
	let mut replaced: Vec<(PathBuf, PathBuf)> = Vec::new();
	for entry in staged.iter_mut() {
		let target = dir.join(&entry.name);
		let saved = if target.exists() {
			let keep = temp_path(dir)?;
			if let Err(error) = fs::rename(&target, &keep) {
				remove_placed(&placed);
				restore(&replaced);
				return Err(error).with_context(|| {
					format!("Cannot replace {}", target.display())
				});
			}
			Some(keep)
		} else {
			None
		};
		if let Err(error) = fs::rename(&entry.temp, &target) {
			if let Some(keep) = &saved {
				let _ = fs::rename(keep, &target);
			}
			remove_placed(&placed);
			restore(&replaced);
			return Err(error)
				.with_context(|| format!("Cannot store {}", target.display()));
		}
		entry.installed = true;
		placed.push(target.clone());
		if let Some(keep) = saved {
			replaced.push((target, keep));
		}
	}
	// Every file landed, so the copies they replaced are no longer wanted.
	for (_, keep) in replaced {
		let _ = fs::remove_file(keep);
	}
	if let Ok(parent) = fs::File::open(dir) {
		let _ = parent.sync_all();
	}
	Ok(())
}

/// Removes the family's own earlier copies of the faces this run installed.
///
/// A mirror may name the same face differently — a release archive uses
/// `NotoSerif/hinted/ttf/NotoSerif-Regular.ttf` while a CDN serves
/// `NotoSerif-Regular.ttf` — so switching mirrors would otherwise leave both.
/// Only a file that this application named (a readable stem, a dash and a
/// sixteen-digit digest), that shares a stem with a file just installed, and
/// whose every declared name belongs to this family is removed. A face that
/// also declares another family, or a file a reader placed by hand, is left
/// alone.
pub(super) fn retire_replaced(
	dir: &Path,
	family: &FontFamily,
	installed: &[String],
) -> Result<()> {
	let stems: Vec<&str> = installed
		.iter()
		.filter_map(|name| generated_stem(name))
		.collect();
	if stems.is_empty() {
		return Ok(());
	}
	for face in describe(dir) {
		if installed.contains(&face.file) {
			continue;
		}
		let Some(stem) = generated_stem(&face.file) else {
			continue;
		};
		if !stems.contains(&stem) {
			continue;
		}
		if face.families.is_empty()
			|| !face
				.families
				.iter()
				.all(|name| family.lookfor.contains(name))
		{
			continue;
		}
		fs::remove_file(dir.join(&face.file)).with_context(|| {
			format!("Cannot remove the replaced {}", face.file)
		})?;
	}
	Ok(())
}

/// The readable stem of a name this application generated, if it is one.
fn generated_stem(name: &str) -> Option<&str> {
	let (stem, extension) = name.rsplit_once('.')?;
	if !matches!(
		extension.to_ascii_lowercase().as_str(),
		"ttf" | "otf" | "ttc" | "otc"
	) {
		return None;
	}
	let (stem, digest) = stem.rsplit_once('-')?;
	if stem.is_empty()
		|| digest.len() != 16
		|| !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
	{
		return None;
	}
	Some(stem)
}

fn remove_placed(placed: &[PathBuf]) {
	for path in placed {
		let _ = fs::remove_file(path);
	}
}

fn restore(replaced: &[(PathBuf, PathBuf)]) {
	for (target, keep) in replaced {
		let _ = fs::rename(keep, target);
	}
}

#[cfg(test)]
mod tests {
	use super::super::{
		join_download_directory, run,
		test_support::{Fake, family, fan, font_bytes},
	};
	use super::*;
	use markview_layout::style::FontFile;
	use std::sync::Arc;

	/// Packs single-face fonts into one collection, so a test can describe a
	/// file that holds more than one family.
	fn collection_of(faces: &[Vec<u8>]) -> Vec<u8> {
		let mut out = Vec::new();
		out.extend(b"ttcf");
		out.extend(1u32.to_be_bytes());
		out.extend((faces.len() as u32).to_be_bytes());
		out.extend(std::iter::repeat_n(0u8, 4 * faces.len()));
		for (index, face) in faces.iter().enumerate() {
			let start = out.len();
			let slot = 12 + index * 4;
			out[slot..slot + 4].copy_from_slice(&(start as u32).to_be_bytes());
			let count = u16::from_be_bytes([face[4], face[5]]) as usize;
			out.extend(&face[..12]);
			let mut tables = Vec::new();
			for table in 0..count {
				let record = 12 + table * 16;
				let at = u32::from_be_bytes(
					face[record + 8..record + 12].try_into().unwrap(),
				) as usize;
				let length = u32::from_be_bytes(
					face[record + 12..record + 16].try_into().unwrap(),
				) as usize;
				tables.push(face[at..at + length].to_vec());
				out.extend(&face[record..record + 8]);
				out.extend(0u32.to_be_bytes());
				out.extend((length as u32).to_be_bytes());
			}
			for (table, data) in tables.iter().enumerate() {
				while out.len() % 4 != 0 {
					out.push(0);
				}
				let at = out.len();
				let slot = start + 12 + table * 16 + 8;
				out[slot..slot + 4].copy_from_slice(&(at as u32).to_be_bytes());
				out.extend(data);
			}
		}
		out
	}

	/// A collection holds several families; describing only its first face
	/// would hide the rest from the download catalogue.
	#[test]
	fn a_collection_reports_every_family() {
		let dir = tempfile::tempdir().unwrap();
		let read = |name: &str| {
			std::fs::read(
				Path::new(env!("CARGO_MANIFEST_DIR"))
					.join("crates/markview-layout/tests/fonts")
					.join(name),
			)
			.unwrap()
		};
		let faces = vec![
			read("NotoSerif-Regular-subset.otf"),
			read("NotoSans-Regular-subset.otf"),
		];
		let bytes = collection_of(&faces);
		std::fs::write(dir.path().join("both.ttc"), &bytes).unwrap();
		let described = describe(dir.path());
		assert_eq!(described.len(), 1);
		assert_eq!(described[0].file, "both.ttc");
		assert_eq!(described[0].faces, 2);
		assert_eq!(described[0].bytes, bytes.len() as u64);
		let names = &described[0].families;
		assert!(
			names.iter().any(|name| name.contains("Noto Serif")),
			"{names:?}"
		);
		assert!(
			names.iter().any(|name| name.contains("Noto Sans")),
			"{names:?}"
		);
	}

	#[test]
	fn a_face_is_described_from_its_own_tables() {
		let dir = tempfile::tempdir().unwrap();
		let bytes = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(
			"crates/markview-layout/tests/fonts/NotoSerif-Regular-subset.otf",
		))
		.unwrap();
		std::fs::write(dir.path().join("face.ttf"), &bytes).unwrap();
		// A file that is not a font is skipped rather than reported.
		std::fs::write(dir.path().join("notes.txt"), b"x").unwrap();
		let faces = describe(dir.path());
		assert_eq!(faces.len(), 1);
		assert_eq!(faces[0].file, "face.ttf");
		assert_eq!(faces[0].bytes, bytes.len() as u64);
		assert_eq!(faces[0].style, FaceStyle::Normal);
		assert!((1..=1000).contains(&faces[0].weight), "{}", faces[0].weight);
		assert!(
			faces[0]
				.families
				.iter()
				.any(|name| name.contains("Noto Serif")),
			"{:?}",
			faces[0].families
		);
		// An absent directory describes nothing instead of failing.
		assert!(describe(&dir.path().join("missing")).is_empty());
	}

	#[test]
	fn a_download_directory_joins_the_font_set_once() {
		let dir = tempfile::tempdir().unwrap();
		// A drawing run gains the directory, like another `--fonts` one.
		let mut fonts = FontConfig::default();
		join_download_directory(&mut fonts, Some(dir.path().to_path_buf()));
		assert_eq!(fonts.directories, vec![dir.path().to_path_buf()]);
		// A second listing of the same directory is dropped.
		join_download_directory(&mut fonts, Some(dir.path().to_path_buf()));
		assert_eq!(fonts.directories, vec![dir.path().to_path_buf()]);
		// A directory already named on the command line is not repeated.
		let mut named = FontConfig {
			directories: vec![dir.path().to_path_buf()],
			..Default::default()
		};
		join_download_directory(&mut named, Some(dir.path().to_path_buf()));
		assert_eq!(named.directories, vec![dir.path().to_path_buf()]);
		// A pinned run keeps exactly its command-line set.
		let mut pinned = FontConfig {
			ignore_system_fonts: true,
			..Default::default()
		};
		join_download_directory(&mut pinned, Some(dir.path().to_path_buf()));
		assert!(pinned.directories.is_empty());
	}

	/// A refresh that succeeds replaces the old copy and keeps no leftover.
	#[test]
	fn a_successful_refresh_replaces_the_installed_copy() {
		let dir = tempfile::tempdir().unwrap();
		let font = font_bytes();
		let family = fan("noto", &["https://x.example/a.otf"]);
		let installed = local_name("noto", "a.otf", &font);
		fs::write(dir.path().join(&installed), b"old copy").unwrap();
		let mut fake = Fake::default();
		fake.bodies
			.insert("https://x.example/a.otf".into(), font.clone());
		let summary = run(
			&[family],
			dir.path(),
			&fake,
			1,
			Arc::new(|_: &str| false),
			&mut |_| {},
		);
		assert_eq!(summary.stored, 1);
		assert_eq!(fs::read(dir.path().join(&installed)).unwrap(), font);
		// The copy that was moved aside is gone once the new one is in place.
		let names: Vec<String> = fs::read_dir(dir.path())
			.unwrap()
			.flatten()
			.map(|entry| entry.file_name().to_string_lossy().into_owned())
			.collect();
		assert_eq!(names, [installed], "{names:?}");
	}

	/// A refresh that fails half way must leave the copies it would have
	/// replaced exactly where they were.
	#[test]
	fn a_failed_refresh_keeps_the_files_it_would_have_replaced() {
		let dir = tempfile::tempdir().unwrap();
		let font = font_bytes();
		let mut family = fan(
			"noto",
			&["https://x.example/a.otf", "https://x.example/b.otf"],
		);
		family.source[0].files[1] = FontFile::Full {
			url: "https://x.example/b.otf".into(),
			sha256: Some("0".repeat(64)),
		};
		// The first file is already installed, and this run replaces it.
		let installed = local_name("noto", "a.otf", &font);
		fs::write(dir.path().join(&installed), b"old copy").unwrap();
		let mut fake = Fake::default();
		fake.bodies
			.insert("https://x.example/a.otf".into(), font.clone());
		fake.bodies
			.insert("https://x.example/b.otf".into(), font.clone());
		let summary = run(
			&[family],
			dir.path(),
			&fake,
			2,
			Arc::new(|_: &str| false),
			&mut |_| {},
		);
		assert_eq!(summary.stored, 0);
		assert_eq!(summary.failed.len(), 1);
		// The old copy is untouched and no new file appeared beside it.
		let names: Vec<String> = fs::read_dir(dir.path())
			.unwrap()
			.flatten()
			.map(|entry| entry.file_name().to_string_lossy().into_owned())
			.collect();
		assert_eq!(names, std::slice::from_ref(&installed), "{names:?}");
		assert_eq!(fs::read(dir.path().join(&installed)).unwrap(), b"old copy");
	}

	#[test]
	fn the_catalogue_follows_the_layers_and_the_disk() {
		let dir = tempfile::tempdir().unwrap();
		let font = font_bytes();
		let low = family("noto", &["https://x.example/a.otf"]);
		let high = FontFamily {
			description: Some("higher".into()),
			..low.clone()
		};
		// Nothing on disk and no system font: the family is missing.
		let config = crate::test_support::fonts();
		let listed = catalog(
			[("builtin", std::slice::from_ref(&low))],
			Some(dir.path()),
			&config,
		);
		assert_eq!(listed.len(), 1);
		assert_eq!(listed[0].owners, ["builtin"]);
		assert_eq!(listed[0].state, State::Missing);
		// A higher layer replaces the entry, and both owners are kept.
		let listed = catalog(
			[
				("builtin", std::slice::from_ref(&low)),
				("mine", std::slice::from_ref(&high)),
			],
			Some(dir.path()),
			&config,
		);
		assert_eq!(listed.len(), 1);
		assert_eq!(listed[0].family.description.as_deref(), Some("higher"));
		assert_eq!(listed[0].owners, ["builtin", "mine"]);
		// A stored file whose own family name matches makes it downloaded.
		let name = local_name("noto", "a.otf", &font);
		fs::write(dir.path().join(&name), &font).unwrap();
		let mut named = low.clone();
		named.lookfor = vec!["Noto Serif".into()];
		let listed = catalog(
			[("builtin", std::slice::from_ref(&named))],
			Some(dir.path()),
			&config,
		);
		assert_eq!(listed[0].state, State::Downloaded);
		assert_eq!(listed[0].files, [name]);
		assert_eq!(listed[0].bytes, font.len() as u64);
	}

	/// The bulk action says "missing"; an explicit request may still name a
	/// family an installed face already covers.
	#[test]
	fn a_selection_narrows_by_state() {
		let entry = |id: &str, state: State| Family {
			family: fan(id, &["https://x.example/a.otf"]),
			owners: vec!["builtin".into()],
			state,
			files: Vec::new(),
			bytes: 0,
		};
		let catalog = vec![
			entry("have", State::Downloaded),
			entry("installed", State::Provided),
			entry("want", State::Missing),
		];
		let ids = vec![
			"have".to_string(),
			"installed".to_string(),
			"want".to_string(),
		];
		let names = |scope| {
			select(&catalog, &ids, scope)
				.into_iter()
				.map(|family| family.id.clone())
				.collect::<Vec<_>>()
		};
		assert_eq!(names(Scope::Missing), ["want"]);
		assert_eq!(names(Scope::Named), ["installed", "want"]);
		assert_eq!(names(Scope::All), ["have", "installed", "want"]);
	}

	#[test]
	fn a_selection_leaves_out_what_is_already_downloaded() {
		let dir = tempfile::tempdir().unwrap();
		let font = font_bytes();
		let family = family("noto", &["https://x.example/a.otf"]);
		let mut named = family.clone();
		named.lookfor = vec!["Noto Serif".into()];
		let name = local_name("noto", "a.otf", &font);
		fs::write(dir.path().join(&name), &font).unwrap();
		let config = crate::test_support::fonts();
		let listed = catalog(
			[("builtin", std::slice::from_ref(&named))],
			Some(dir.path()),
			&config,
		);
		let ids = vec!["noto".to_string()];
		assert!(select(&listed, &ids, Scope::Named).is_empty());
		assert_eq!(select(&listed, &ids, Scope::All).len(), 1);
	}
}
