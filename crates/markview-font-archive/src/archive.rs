//! Archive recognition and extraction: zip, tar, gzip and zstd.
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::{
	fs,
	io::{self, Read},
	path::{Path, PathBuf},
};

/// The largest single downloaded font accepted.
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// The most one archive may unpack into matched members.
pub const MAX_UNPACKED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// The most members one archive may contribute.
pub const MAX_MEMBERS: usize = 4096;

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

pub fn sniff_file(path: &Path) -> Result<Container> {
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
pub fn temp_path(dir: &Path) -> Result<PathBuf> {
	let mut random = [0u8; 8];
	getrandom::fill(&mut random)
		.context("Cannot name a temporary font file")?;
	Ok(dir.join(format!(".tmp-{:016x}.tmp", u64::from_le_bytes(random))))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
	let mut out = String::with_capacity(64);
	for byte in Sha256::digest(bytes) {
		use std::fmt::Write as _;
		let _ = write!(out, "{byte:02x}");
	}
	out
}

pub fn sha256_file(path: &Path) -> Result<String> {
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
pub fn unpack(
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
pub struct Extracted {
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
	pub fn next_file(&mut self) -> Option<(PathBuf, String)> {
		self.files.pop()
	}

	pub fn is_empty(&self) -> bool {
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
		let name = entry.name()?.replace('\\', "/");
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
		if pattern.get(pi) == Some(&'*') {
			star = Some((pi, si));
			pi += 1;
		} else if pattern.get(pi).is_some_and(|&c| c == '?' || c == name[si]) {
			pi += 1;
			si += 1;
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
	use super::*;
	use std::io::Write as _;

	#[test]
	fn patterns_and_containers_are_recognized() {
		assert!(glob_match("*.otf", "a.otf"));
		assert!(!glob_match("*.otf", "sub/a.otf"));
		assert!(glob_match("sub/*.otf", "sub/a.otf"));
		assert!(!glob_match("*a*a*a*a*a*a*a*b", &"a".repeat(64)));
		assert_eq!(sniff(b"PK\x03\x04rest"), Some(Container::Zip));
		assert_eq!(sniff(&[0x1f, 0x8b, 0x08]), Some(Container::Gzip));
		assert_eq!(sniff(&[0x28, 0xb5, 0x2f, 0xfd, 0]), Some(Container::Zstd));
		assert_eq!(sniff(b"<!doctype html>"), None);
	}

	#[test]
	fn zip_members_are_filtered_and_owned_until_taken() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("faces.zip");
		{
			let mut writer =
				zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
			let options = zip::write::SimpleFileOptions::default();
			for (name, body) in [
				("a.otf", &b"face"[..]),
				("skip.txt", &b"skip"[..]),
				("../escape.otf", &b"no"[..]),
			] {
				writer.start_file(name, options).unwrap();
				writer.write_all(body).unwrap();
			}
			writer.finish().unwrap();
		}
		let out = dir.path().join("out");
		std::fs::create_dir(&out).unwrap();
		let mut members =
			unpack(Container::Zip, &path, &["*.otf".into()], &out).unwrap();
		let (member, name) = members.next_file().unwrap();
		assert_eq!(name, "a.otf");
		assert_eq!(std::fs::read(&member).unwrap(), b"face");
		assert!(members.is_empty());
		assert!(member.exists(), "a taken member belongs to the caller");
		drop(members);
		assert!(member.exists(), "a taken member is not removed");
	}

	#[test]
	fn unmatched_members_fail_instead_of_writing_outside() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("faces.tar");
		{
			let mut builder =
				tar::Builder::new(std::fs::File::create(&path).unwrap());
			let mut header = tar::Header::new_gnu();
			header.set_size(3);
			header.set_mode(0o644);
			header.set_cksum();
			builder
				.append_data(&mut header, "other.bin", &b"abc"[..])
				.unwrap();
			builder.finish().unwrap();
		}
		let out = dir.path().join("out");
		std::fs::create_dir(&out).unwrap();
		assert!(
			unpack(Container::Tar, &path, &["*.otf".into()], &out).is_err()
		);
		assert!(std::fs::read_dir(&out).unwrap().next().is_none());
	}
}
