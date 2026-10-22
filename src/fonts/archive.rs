//! Archive recognition and extraction: zip, tar, gzip and zstd.
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::{
	fs,
	io::{self, Read},
	path::{Path, PathBuf},
};

use super::{MAX_FILE_BYTES, MAX_MEMBERS, MAX_UNPACKED_BYTES};

/// The container an archive body is stored in, read from its leading bytes.
///
/// Nothing is declared in a stylesheet: the same family may be published as a
/// zip on one mirror and a tarball on another, and both are recognized here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Container {
	Zip,
	Gzip,
	Zstd,
	Tar,
}

/// Recognizes a container from its first bytes.
pub fn sniff(head: &[u8]) -> Option<Container> {
	if head.starts_with(b"PK\x03\x04")
		|| head.starts_with(b"PK\x05\x06")
		|| head.starts_with(b"PK\x07\x08")
	{
		return Some(Container::Zip);
	}
	if head.starts_with(&[0x1f, 0x8b]) {
		return Some(Container::Gzip);
	}
	if head.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
		return Some(Container::Zstd);
	}
	// A plain tar names itself at offset 257.
	if head.len() >= 262 && &head[257..262] == b"ustar" {
		return Some(Container::Tar);
	}
	None
}

pub(super) fn sniff_file(path: &Path) -> Result<Container> {
	let mut file = fs::File::open(path)?;
	let mut head = [0u8; 512];
	let read = file.read(&mut head)?;
	sniff(&head[..read])
		.with_context(|| format!("{} is not an archive", path.display()))
}

/// A fresh temporary name inside the download directory.
///
/// The `.tmp` suffix is what keeps the shaper's directory scan from ever
/// registering a half-written file, and staying in the same directory is what
/// makes the final rename atomic. The random part comes from the operating
/// system rather than the process id, so no one who can write beside the
/// directory can predict the name and plant a symlink there. The name alone is
/// returned; every caller creates the file exclusively, and [`install`] only
/// uses it as a rename destination.
pub(super) fn temp_path(dir: &Path) -> Result<PathBuf> {
	let mut random = [0u8; 8];
	getrandom::fill(&mut random)
		.context("Cannot name a temporary font file")?;
	Ok(dir.join(format!(".tmp-{:016x}.tmp", u64::from_le_bytes(random))))
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
	let mut out = String::with_capacity(64);
	for byte in Sha256::digest(bytes) {
		use std::fmt::Write as _;
		let _ = write!(out, "{byte:02x}");
	}
	out
}

pub(super) fn sha256_file(path: &Path) -> Result<String> {
	let mut file = fs::File::open(path)?;
	let mut hasher = Sha256::new();
	let mut buffer = vec![0u8; 64 * 1024];
	loop {
		let read = file.read(&mut buffer)?;
		if read == 0 {
			break;
		}
		hasher.update(&buffer[..read]);
	}
	let mut out = String::with_capacity(64);
	for byte in hasher.finalize() {
		use std::fmt::Write as _;
		let _ = write!(out, "{byte:02x}");
	}
	Ok(out)
}

/// Unpacks the members matching `patterns` into temporary files.
///
/// A pattern matches `/`-separated member paths and never crosses a separator,
/// so `*` stays inside one segment, `?` matches one character, and `**` is not
/// part of the grammar. Directories, symlinks and hard links are never taken,
/// so an archive can only ever contribute regular files, and nothing is written
/// outside `dir`.
pub(super) fn unpack(
	container: Container,
	path: &Path,
	patterns: &[String],
	dir: &Path,
) -> Result<Extracted> {
	let mut out = Extracted::default();
	match container {
		Container::Zip => unpack_zip(path, patterns, dir, &mut out)?,
		Container::Tar => {
			let file = fs::File::open(path)?;
			unpack_tar(file, patterns, dir, &mut out)?;
		}
		Container::Gzip => {
			let file = fs::File::open(path)?;
			unpack_tar(
				flate2::read::GzDecoder::new(file),
				patterns,
				dir,
				&mut out,
			)?;
		}
		Container::Zstd => {
			let file = fs::File::open(path)?;
			let decoder = zstd::stream::read::Decoder::new(file)
				.context("Cannot read the zstd stream")?;
			unpack_tar(decoder, patterns, dir, &mut out)?;
		}
	}
	if out.files.is_empty() {
		bail!("No archive member matched");
	}
	Ok(out)
}

/// The members one archive has unpacked so far.
///
/// It owns every temporary file it holds, so a failure anywhere after the
/// first member was written still cleans up after itself: whatever was not
/// taken into place is removed when this is dropped.
#[derive(Default)]
pub(super) struct Extracted {
	files: Vec<(PathBuf, String)>,
	bytes: u64,
}

impl Drop for Extracted {
	fn drop(&mut self) {
		for (path, _) in &self.files {
			let _ = fs::remove_file(path);
		}
	}
}

impl Extracted {
	/// Takes one extracted member, leaving the rest to [`Drop`].
	pub(super) fn next_file(&mut self) -> Option<(PathBuf, String)> {
		self.files.pop()
	}

	pub(super) fn is_empty(&self) -> bool {
		self.files.is_empty()
	}

	fn take(
		&mut self,
		reader: &mut dyn Read,
		dir: &Path,
		name: String,
	) -> Result<()> {
		if self.files.len() >= MAX_MEMBERS {
			bail!("Archive has more than {MAX_MEMBERS} matching members");
		}
		let temp = temp_path(dir)?;
		// Registered before the first write, so a failed copy or sync is
		// cleaned up by [`Drop`] exactly like an abandoned extraction.
		self.files.push((temp.clone(), name));
		// Exclusive: the name is random, and a file already sitting there is
		// not something this extraction may write through.
		let mut file = fs::OpenOptions::new()
			.write(true)
			.create_new(true)
			.open(&temp)?;
		let mut limited = reader.take(MAX_FILE_BYTES + 1);
		let written = io::copy(&mut limited, &mut file)?;
		file.sync_all()?;
		if written > MAX_FILE_BYTES {
			bail!("Font file exceeds {} MiB", MAX_FILE_BYTES / (1024 * 1024));
		}
		self.bytes = self.bytes.saturating_add(written);
		if self.bytes > MAX_UNPACKED_BYTES {
			bail!(
				"Archive unpacks to more than {} GiB",
				MAX_UNPACKED_BYTES >> 30
			);
		}
		Ok(())
	}
}

fn unpack_zip(
	path: &Path,
	patterns: &[String],
	dir: &Path,
	out: &mut Extracted,
) -> Result<()> {
	let file = fs::File::open(path)?;
	let mut archive = zip::ZipArchive::new(io::BufReader::new(file))
		.context("Not a zip archive")?;
	for index in 0..archive.len() {
		let mut entry = archive.by_index(index)?;
		if !entry.is_file() {
			continue;
		}
		// A symlink is an entry too; only a regular file may be taken.
		if let Some(mode) = entry.unix_mode()
			&& mode & 0o170000 != 0o100000
		{
			continue;
		}
		let name = entry.name().replace('\\', "/");
		if !patterns.iter().any(|pattern| glob_match(pattern, &name)) {
			continue;
		}
		out.take(&mut entry, dir, name)?;
	}
	Ok(())
}

fn unpack_tar(
	reader: impl Read,
	patterns: &[String],
	dir: &Path,
	out: &mut Extracted,
) -> Result<()> {
	let mut archive = tar::Archive::new(reader);
	for entry in archive.entries().context("Not a tar archive")? {
		let mut entry = entry?;
		if !entry.header().entry_type().is_file() {
			continue;
		}
		let name = entry.path()?.to_string_lossy().replace('\\', "/");
		if !patterns.iter().any(|pattern| glob_match(pattern, &name)) {
			continue;
		}
		out.take(&mut entry, dir, name)?;
	}
	Ok(())
}

/// Whether `pattern` matches the `/`-separated member `name`.
///
/// A pattern never crosses a separator, so both sides must have the same
/// number of segments; `*` matches any run of characters inside one, `?`
/// matches exactly one character, and every other character is literal.
pub fn glob_match(pattern: &str, name: &str) -> bool {
	let mut segments = name.split('/');
	pattern
		.split('/')
		.all(|part| segments.next().is_some_and(|s| segment_match(part, s)))
		&& segments.next().is_none()
}

/// Whether `pattern` matches one path segment, in one linear pass.
fn segment_match(pattern: &str, name: &str) -> bool {
	let pattern: Vec<char> = pattern.chars().collect();
	let name: Vec<char> = name.chars().collect();
	let (mut pi, mut si) = (0, 0);
	// Where the last `*` left off, so it can still grow into more characters.
	let mut star: Option<(usize, usize)> = None;
	while si < name.len() {
		if pattern.get(pi).is_some_and(|&c| c == '?' || c == name[si]) {
			pi += 1;
			si += 1;
		} else if pattern.get(pi) == Some(&'*') {
			star = Some((pi, si));
			pi += 1;
		} else if let Some((sp, ss)) = star {
			pi = sp + 1;
			si = ss + 1;
			star = Some((sp, si));
		} else {
			return false;
		}
	}
	pattern[pi..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
	use super::super::{
		describe, local_name, run,
		test_support::{Fake, fan, font_bytes},
	};
	use super::*;
	use markview_layout::style::{FontArchive, FontSource};
	use std::{io::Write as _, sync::Arc};

	#[test]
	fn a_member_pattern_never_crosses_a_separator() {
		assert!(glob_match("*.otf", "a.otf"));
		assert!(!glob_match("*.otf", "sub/a.otf"));
		assert!(glob_match("sub/*.otf", "sub/a.otf"));
		assert!(!glob_match("sub/*.otf", "sub/deep/a.otf"));
		assert!(glob_match("Sans/*/SC/*.otf", "Sans/x/SC/a.otf"));
		assert!(!glob_match("Sans/*/SC/*.otf", "Sans/x/y/SC/a.otf"));
		assert!(glob_match("a?c", "abc"));
		assert!(!glob_match("a?c", "a/c"));
		assert!(!glob_match("*.otf", "a.ttf"));
		// Starred near-misses used to revisit states exponentially; matching
		// stays polynomial in the pattern and name lengths.
		assert!(!glob_match("*a*a*a*a*a*a*a*b", &"a".repeat(64)));
	}

	#[test]
	fn a_container_is_recognized_from_its_leading_bytes() {
		assert_eq!(sniff(b"PK\x03\x04rest"), Some(Container::Zip));
		assert_eq!(sniff(&[0x1f, 0x8b, 0x08]), Some(Container::Gzip));
		assert_eq!(
			sniff(&[0x28, 0xb5, 0x2f, 0xfd, 0x00]),
			Some(Container::Zstd)
		);
		let mut tar = vec![0u8; 512];
		tar[257..262].copy_from_slice(b"ustar");
		assert_eq!(sniff(&tar), Some(Container::Tar));
		assert_eq!(sniff(b"<!doctype html>"), None);
	}

	/// Builds a zip holding the given members, so extraction is tested against
	/// a writer rather than a checked-in binary.
	fn zip_of(members: &[(&str, &[u8])]) -> PathBuf {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.keep().join("a.zip");
		let mut writer = zip::ZipWriter::new(fs::File::create(&path).unwrap());
		let options: zip::write::FileOptions<'_, ()> =
			zip::write::FileOptions::default();
		for (name, bytes) in members {
			writer.start_file(*name, options).unwrap();
			writer.write_all(bytes).unwrap();
		}
		writer.finish().unwrap();
		path
	}

	#[test]
	fn an_archive_contributes_only_its_matching_regular_files() {
		let font = font_bytes();
		let archive = zip_of(&[
			("Sans/SubsetOTF/SC/NotoSansSC-Regular.otf", &font),
			("Sans/SubsetOTF/SC/NotoSansSC-Bold.otf", &font),
			("Sans/OTF/Japanese/NotoSansJP-Regular.otf", &font),
			("Sans/README.md", b"not a font"),
		]);
		let dir = tempfile::tempdir().unwrap();
		let patterns = vec!["Sans/SubsetOTF/SC/*.otf".to_string()];
		let mut members =
			unpack(Container::Zip, &archive, &patterns, dir.path()).unwrap();
		let mut names = Vec::new();
		while let Some((path, name)) = members.next_file() {
			assert!(path.exists());
			names.push(name);
		}
		names.sort();
		assert_eq!(
			names,
			[
				"Sans/SubsetOTF/SC/NotoSansSC-Bold.otf",
				"Sans/SubsetOTF/SC/NotoSansSC-Regular.otf"
			]
		);
		// Nothing matched is an error rather than an empty success.
		assert!(
			unpack(
				Container::Zip,
				&archive,
				&["Sans/*.ttf".into()],
				dir.path()
			)
			.is_err()
		);
	}

	/// Distinct members of one archive may share a basename; they must not
	/// share a destination.
	#[test]
	fn two_members_that_share_a_basename_land_apart() {
		let font = font_bytes();
		let regular = local_name("family", "regular/font.ttf", &font);
		let bold = local_name("family", "bold/font.ttf", &font);
		assert_ne!(regular, bold);
		assert!(regular.starts_with("font-"), "{regular}");
		assert!(bold.starts_with("font-"), "{bold}");
		// One member has one name, whenever it is asked for.
		assert_eq!(regular, local_name("family", "regular/font.ttf", &font));
	}

	/// A member that was unpacked but never taken into place must not outlive
	/// the archive that produced it.
	#[test]
	fn an_abandoned_extraction_removes_its_temporary_files() {
		let font = font_bytes();
		let archive =
			zip_of(&[("a/Regular.otf", &font), ("a/Bold.otf", &font)]);
		let dir = tempfile::tempdir().unwrap();
		let extracted =
			unpack(Container::Zip, &archive, &["a/*.otf".into()], dir.path())
				.unwrap();
		assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
		drop(extracted);
		assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
	}

	#[test]
	fn a_tarball_unpacks_the_same_way() {
		let font = font_bytes();
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("a.tar.gz");
		{
			let file = fs::File::create(&path).unwrap();
			let encoder = flate2::write::GzEncoder::new(
				file,
				flate2::Compression::fast(),
			);
			let mut builder = tar::Builder::new(encoder);
			let mut header = tar::Header::new_gnu();
			header.set_size(font.len() as u64);
			header.set_mode(0o644);
			header.set_cksum();
			builder
				.append_data(
					&mut header,
					"Serif/SubsetOTF/SC/NotoSerifSC-Regular.otf",
					&font[..],
				)
				.unwrap();
			builder.into_inner().unwrap().finish().unwrap();
		}
		let container = sniff_file(&path).unwrap();
		assert_eq!(container, Container::Gzip);
		let mut members = unpack(
			container,
			&path,
			&["Serif/SubsetOTF/SC/*.otf".into()],
			dir.path(),
		)
		.unwrap();
		let (temp, name) = members.next_file().unwrap();
		assert!(temp.exists());
		assert_eq!(name, "Serif/SubsetOTF/SC/NotoSerifSC-Regular.otf");
		assert!(members.next_file().is_none());
	}

	/// Arch ships a font as a zstd tarball, so that container must unpack like
	/// the others.
	#[test]
	fn a_zstd_package_unpacks_the_same_way() {
		let font = font_bytes();
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("ttf-fira-code-6.2-4-any.pkg.tar.zst");
		{
			let file = fs::File::create(&path).unwrap();
			let encoder = zstd::stream::write::Encoder::new(file, 0).unwrap();
			let mut builder = tar::Builder::new(encoder);
			let mut header = tar::Header::new_gnu();
			header.set_size(font.len() as u64);
			header.set_mode(0o644);
			header.set_cksum();
			builder
				.append_data(
					&mut header,
					"usr/share/fonts/TTF/FiraCode-Regular.ttf",
					&font[..],
				)
				.unwrap();
			builder.into_inner().unwrap().finish().unwrap();
		}
		let container = sniff_file(&path).unwrap();
		assert_eq!(container, Container::Zstd);
		let mut members = unpack(
			container,
			&path,
			&["usr/share/fonts/TTF/FiraCode-*.ttf".into()],
			dir.path(),
		)
		.unwrap();
		let (temp, name) = members.next_file().unwrap();
		assert!(temp.exists());
		assert_eq!(name, "usr/share/fonts/TTF/FiraCode-Regular.ttf");
		assert!(members.next_file().is_none());
	}

	/// The temporary name must be unpredictable, keep the two properties the
	/// readers of it rely on, and never be a name the callers may clobber.
	#[test]
	fn a_temporary_font_name_is_random_and_scannable() {
		let dir = tempfile::tempdir().unwrap();
		let first = temp_path(dir.path()).unwrap();
		let second = temp_path(dir.path()).unwrap();
		assert_ne!(first, second, "two names must not collide");
		// `install` moves a copy aside to it, and every creator writes to it
		// exclusively, so naming must not create anything.
		assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
		for path in [&first, &second] {
			assert_eq!(path.parent(), Some(dir.path()));
			let name = path.file_name().unwrap().to_string_lossy().into_owned();
			// The shaper's directory scan must never register a half-written
			// file, and `remove_placed` must clean up after a failed install.
			assert!(
				name.starts_with(".tmp-") && name.ends_with(".tmp"),
				"{name}"
			);
			// The exact shape pins the scheme: a decimal process id and a
			// counter cannot spell it, so the name is no longer predictable
			// to whoever can write beside the directory.
			let random = name
				.strip_prefix(".tmp-")
				.and_then(|rest| rest.strip_suffix(".tmp"));
			assert!(
				random.is_some_and(|random| {
					random.len() == 16
						&& random.bytes().all(|byte| {
							byte.is_ascii_digit()
								|| (b'a'..=b'f').contains(&byte)
						})
				}),
				"{name}"
			);
		}
	}

	/// Two mirrors may name one face differently. A refresh from the other
	/// mirror must replace the copy it already has, not add a second one.
	#[test]
	fn a_refresh_from_another_mirror_replaces_the_installed_copy() {
		let dir = tempfile::tempdir().unwrap();
		let font = font_bytes();
		let mut family =
			fan("noto", &["https://cdn.example/NotoSerif-Regular.ttf"]);
		// The family's names are the ones the fixture itself declares, so the
		// reconciliation can recognize its own copy.
		family.lookfor = declared_names(&font);
		// A release archive names the same face by its member path.
		family.source.push(FontSource {
			name: Some("archive".into()),
			files: Vec::new(),
			archives: vec![FontArchive {
				url: "https://host.example/NotoSerif.zip".into(),
				sha256: None,
				members: vec![
					"NotoSerif/hinted/ttf/NotoSerif-Regular.ttf".into(),
				],
			}],
		});
		let archive = fs::read(zip_of(&[(
			"NotoSerif/hinted/ttf/NotoSerif-Regular.ttf",
			&font,
		)]))
		.unwrap();
		let mut fake = Fake::default();
		fake.bodies.insert(
			"https://cdn.example/NotoSerif-Regular.ttf".into(),
			font.clone(),
		);
		fake.bodies
			.insert("https://host.example/NotoSerif.zip".into(), archive);
		// The direct source is unreachable here, so the archive installs first,
		// under the name its member path gives it.
		fake.fail.push("cdn.example".into());
		let summary = run(
			&[family.clone()],
			dir.path(),
			&fake,
			1,
			Arc::new(|_: &str| false),
			&mut |_| {},
		);
		assert_eq!(summary.stored, 1);
		let first: Vec<String> = names_in(dir.path());
		assert_eq!(first.len(), 1, "{first:?}");
		// Now the direct mirror answers, and the face moves to its other name.
		fake.fail.clear();
		let summary = run(
			&[family],
			dir.path(),
			&fake,
			1,
			Arc::new(|_: &str| false),
			&mut |_| {},
		);
		assert_eq!(summary.stored, 1);
		let second = names_in(dir.path());
		assert_eq!(second.len(), 1, "{second:?}");
		assert_ne!(first, second, "the name should follow the mirror");
		assert_eq!(fs::read(dir.path().join(&second[0])).unwrap(), font);
	}

	/// A reader's own file, and a face another family also declares, are never
	/// retired by a refresh.
	#[test]
	fn a_refresh_leaves_foreign_files_alone() {
		let dir = tempfile::tempdir().unwrap();
		let font = font_bytes();
		let mut family =
			fan("noto", &["https://cdn.example/NotoSerif-Regular.ttf"]);
		family.lookfor = declared_names(&font);
		// A plainly named file that does not follow this application's scheme,
		// and a generated one whose stem nothing installs.
		fs::write(dir.path().join("NotoSerif-Regular.ttf"), &font).unwrap();
		fs::write(dir.path().join("Other-Regular-0123456789abcdef.ttf"), &font)
			.unwrap();
		let mut fake = Fake::default();
		fake.bodies.insert(
			"https://cdn.example/NotoSerif-Regular.ttf".into(),
			font.clone(),
		);
		let summary = run(
			&[family],
			dir.path(),
			&fake,
			1,
			Arc::new(|_: &str| false),
			&mut |_| {},
		);
		assert_eq!(summary.stored, 1);
		let names = names_in(dir.path());
		assert_eq!(names.len(), 3, "{names:?}");
		assert!(names.contains(&"NotoSerif-Regular.ttf".to_string()));
		assert!(
			names.contains(&"Other-Regular-0123456789abcdef.ttf".to_string())
		);
	}

	/// The family names a font file declares, read through the same scanner the
	/// catalogue uses.
	fn declared_names(font: &[u8]) -> Vec<String> {
		let dir = tempfile::tempdir().unwrap();
		fs::write(dir.path().join("probe.ttf"), font).unwrap();
		describe(dir.path())[0].families.clone()
	}

	fn names_in(dir: &Path) -> Vec<String> {
		let mut names: Vec<String> = fs::read_dir(dir)
			.unwrap()
			.flatten()
			.map(|entry| entry.file_name().to_string_lossy().into_owned())
			.collect();
		names.sort();
		names
	}

	/// A member that fails after earlier ones were staged must take those
	/// staged files with it, exactly as a cancellation in the staging loop
	/// does: neither the popped member nor a `Staged` record may leak a
	/// temporary file into the download directory.
	#[test]
	fn a_failed_member_removes_the_members_staged_before_it() {
		let dir = tempfile::tempdir().unwrap();
		let font = font_bytes();
		let archive = fs::read(zip_of(&[
			("a/Good.otf", &font),
			("a/Broken.otf", b"<html>"),
			("a/Never.otf", &font),
		]))
		.unwrap();
		let mut fake = Fake::default();
		fake.bodies
			.insert("https://x.example/a.zip".into(), archive);
		let mut family = fan("noto", &[]);
		family.source[0].files.clear();
		family.source[0].archives.push(FontArchive {
			url: "https://x.example/a.zip".into(),
			sha256: None,
			members: vec!["a/*.otf".into()],
		});
		let summary = run(
			&[family],
			dir.path(),
			&fake,
			1,
			Arc::new(|_: &str| false),
			&mut |_| {},
		);
		assert_eq!(summary.stored, 0);
		assert_eq!(summary.failed.len(), 1);
		assert!(
			summary.failed[0].1.contains("not a font"),
			"{}",
			summary.failed[0].1
		);
		// Neither the staged member nor the ones never reached survive.
		let names: Vec<String> = fs::read_dir(dir.path())
			.unwrap()
			.flatten()
			.map(|entry| entry.file_name().to_string_lossy().into_owned())
			.collect();
		assert!(names.is_empty(), "{names:?}");
	}

	/// Cancelling after the archive has arrived must stop the extraction and
	/// the installation too, not only the transfer.
	#[test]
	fn a_cancelled_extraction_installs_nothing() {
		let dir = tempfile::tempdir().unwrap();
		let font = font_bytes();
		let archive = fs::read(zip_of(&[
			("a/Regular.otf", &font),
			("a/Bold.otf", &font),
		]))
		.unwrap();
		let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
		let mut fake = Fake {
			arm: Some(cancelled.clone()),
			..Default::default()
		};
		fake.bodies
			.insert("https://x.example/a.zip".into(), archive);
		let mut family = fan("noto", &[]);
		family.source[0].files.clear();
		family.source[0].archives.push(FontArchive {
			url: "https://x.example/a.zip".into(),
			sha256: None,
			members: vec!["a/*.otf".into()],
		});
		let cancel = {
			let cancelled = cancelled.clone();
			Arc::new(move |_: &str| {
				cancelled.load(std::sync::atomic::Ordering::Relaxed)
			})
		};
		let summary = run(&[family], dir.path(), &fake, 1, cancel, &mut |_| {});
		assert_eq!(summary.stored, 0);
		assert_eq!(summary.cancelled, ["noto"]);
		// Neither an installed face nor a temporary file survives.
		assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
	}
}
