//! Valid and damaged font archives through the shared extraction core.
use crate::coverage;
use markview_font_archive::{
	Container, glob_match, sha256_file, sniff, sniff_file, unpack,
};
use std::io::Write;

const MEMBERS: &[&str] = &[
	"face.otf",
	"nested/face.ttf",
	"other.txt",
	"../escape.otf",
	"dir/",
	"nested/deep/face.woff2",
];

fn pattern(data: &[u8]) -> String {
	match data.get(4).copied().unwrap_or(0) % 4 {
		0 => "*.otf".into(),
		1 => "nested/*.ttf".into(),
		2 => "**/*.otf".into(),
		_ => String::from_utf8_lossy(&data[1..data.len().min(9)]).into(),
	}
}

fn member_name(index: usize, data: &[u8]) -> String {
	if index == 0 {
		return "face.otf".into();
	}
	if index == 1 {
		return "nested/face.ttf".into();
	}
	let base = MEMBERS[(usize::from(data.get(index).copied().unwrap_or(0))
		+ index)
		% MEMBERS.len()];
	let base = if base.contains("..") {
		"face.otf"
	} else {
		base
	};
	if index.is_multiple_of(3) {
		base.to_owned()
	} else {
		format!("nested/{base}")
	}
}

fn body(data: &[u8], index: usize) -> &[u8] {
	let len = usize::from(data.get(index).copied().unwrap_or(0)) % 1024;
	let at = (index * 17) % data.len().max(1);
	let end = (at + len).min(data.len());
	&data[at..end]
}

fn write_zip(path: &std::path::Path, data: &[u8]) -> bool {
	let Ok(file) = std::fs::File::create(path) else {
		return false;
	};
	let mut writer = zip::ZipWriter::new(file);
	let options = zip::write::SimpleFileOptions::default();
	let count = 1 + usize::from(data.get(16).copied().unwrap_or(0)) % 4;
	for index in 0..count {
		let name = member_name(index, data);
		if writer.start_file(name, options).is_err() {
			return false;
		}
		let _ = writer.write_all(body(data, index));
	}
	writer.finish().is_ok()
}

fn write_tar(
	path: &std::path::Path,
	data: &[u8],
	gzip: bool,
	zstd: bool,
) -> bool {
	let Ok(file) = std::fs::File::create(path) else {
		return false;
	};
	let count = 1 + usize::from(data.get(16).copied().unwrap_or(0)) % 4;
	let write = |writer: &mut dyn Write| {
		let mut builder = tar::Builder::new(writer);
		for index in 0..count {
			let contents = body(data, index);
			let mut header = tar::Header::new_gnu();
			header.set_size(contents.len() as u64);
			header.set_mode(0o644);
			header.set_cksum();
			let name = member_name(index, data);
			if builder.append_data(&mut header, name, contents).is_err() {
				return false;
			}
		}
		builder.finish().is_ok()
	};
	if gzip {
		let encoder =
			flate2::write::GzEncoder::new(file, flate2::Compression::fast());
		write(&mut flate2::write::GzEncoder::new(
			encoder
				.finish()
				.unwrap_or_else(|_| std::fs::File::create(path).unwrap()),
			flate2::Compression::fast(),
		))
	} else if zstd {
		match zstd::stream::write::Encoder::new(file, 1) {
			Ok(mut encoder) => {
				let ok = write(&mut encoder);
				ok && encoder.finish().is_ok()
			}
			Err(_) => false,
		}
	} else {
		let mut file = file;
		write(&mut file)
	}
}

/// `ARC1`, control bytes, then content. Valid archives are synthesized from
/// the payload; corrupted mode writes the payload itself.
pub fn check(data: &[u8]) {
	if data.len() > 64 * 1024 {
		return;
	}
	let dir = tempfile::tempdir().expect("tempdir");
	let path = dir.path().join("archive.bin");
	let pattern = pattern(data);
	let container = data.get(16).copied().unwrap_or(0) % 4;
	let valid = data.get(17).copied().unwrap_or(0) % 2 == 0;
	let built = if valid {
		match container {
			0 => write_zip(&path, data),
			1 => write_tar(&path, data, false, false),
			2 => write_tar(&path, data, true, false),
			_ => write_tar(&path, data, false, true),
		}
	} else {
		std::fs::write(&path, data).is_ok()
	};
	if !built {
		return;
	}
	let head = std::fs::read(&path).unwrap_or_default();
	let sniffed = sniff(&head[..head.len().min(512)]);
	if valid {
		assert!(sniffed.is_some(), "a synthesized archive lost its magic");
	} else if sniffed.is_none() {
		assert!(sniff_file(&path).is_err());
	}
	if let Some(container) = sniffed {
		for pattern in [pattern, "*.otf".into(), "nested/*.ttf".into()] {
			let out = dir.path().join("out");
			let _ = std::fs::create_dir(&out);
			let Ok(mut members) =
				unpack(container, &path, std::slice::from_ref(&pattern), &out)
			else {
				continue;
			};
			let mut taken = Vec::new();
			while let Some((member, name)) = members.next_file() {
				assert!(glob_match(&pattern, &name) || !pattern.contains('*'));
				assert!(!name.contains(".."), "an escaping member was taken");
				assert!(member.exists());
				taken.push(member);
			}
			drop(members);
			for member in taken {
				assert!(member.exists(), "a taken member was removed on drop");
				let _ = std::fs::remove_file(member);
			}
			coverage::hit(coverage::Hit::ArchiveUnpacked);
		}
	}
	// Hash helpers are part of the caller's verification path.
	let _ = sha256_file(&path);
	coverage::hit(coverage::Hit::ArchiveStep);
}

#[allow(dead_code)]
fn containers() -> [Container; 4] {
	[
		Container::Zip,
		Container::Tar,
		Container::Gzip,
		Container::Zstd,
	]
}
