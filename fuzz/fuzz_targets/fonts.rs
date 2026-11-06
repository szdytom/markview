//! G5/D, Tier 1 + Tier 2: font validation and face parsing. The unit carries
//! the bytes; valid committed faces are mutated so the scan and shaper see a
//! realistic face as well as arbitrary garbage. Oracle (O1, O2): a malformed
//! font degrades to "not a font", never panics; a valid face shapes finite
//! advances.
#![no_main]

use libfuzzer_sys::{fuzz_mutator, fuzz_target};
use markview_layout::{
	fonts::{self, FontConfig},
	scene::Paint,
	shaping::TextShaper,
};
use mvfuzz::{budget, coverage, mutators, oracle};
use std::path::{Path, PathBuf};

fn faces() -> Vec<PathBuf> {
	let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("../crates/markview-layout/tests/fonts");
	let mut faces: Vec<_> = std::fs::read_dir(directory)
		.expect("committed faces")
		.filter_map(|entry| {
			let path = entry.ok()?.path();
			let extension = path.extension()?.to_str()?;
			matches!(extension, "otf" | "ttf").then_some(path)
		})
		.collect();
	faces.sort();
	faces
}

/// `FNT1`, a face index, mutation count, then mutation bytes. An empty
/// mutation tail keeps the face untouched, so face parsing and shaping run on
/// a valid font.
fn check_committed(data: &[u8]) -> bool {
	if !data.starts_with(b"FNT1") || data.len() < 3 {
		return false;
	}
	let faces = faces();
	if faces.is_empty() {
		return true;
	}
	let face = std::fs::read(&faces[usize::from(data[1]) % faces.len()])
		.expect("a committed face");
	let mut bytes = face;
	let mutations = usize::from(data[2]).min(64);
	for (index, byte) in data[3..].iter().take(mutations).enumerate() {
		if bytes.is_empty() {
			break;
		}
		let at = (usize::from(*byte) * 31 + index * 97) % bytes.len();
		bytes[at] ^= byte.wrapping_add(index as u8).max(1);
	}
	let dir = tempfile::tempdir().expect("tempdir");
	let path = dir.path().join("face.otf");
	std::fs::write(&path, &bytes).expect("write face");
	let config = FontConfig {
		ignore_system_fonts: true,
		directories: vec![dir.path().to_owned()],
		..Default::default()
	};
	let families = fonts::families(&config, false);
	for family in families.iter() {
		assert!(!family.is_empty());
	}
	if mutations == 0 {
		assert!(!families.is_empty(), "a committed face lost its family");
	}
	let mut shaper = TextShaper::with_fonts(config);
	let text = String::from_utf8_lossy(&data[3..]).into_owned();
	let (draws, width) =
		shaper.label_measured(&text, 16., 0., 10., Paint::Text);
	assert!(
		width.is_finite(),
		"face shaping produced a non-finite width"
	);
	for draw in &draws {
		if let markview_layout::scene::Draw::Glyph(glyph) = draw {
			assert!(glyph.x.is_finite() && glyph.y.is_finite());
		}
	}
	true
}

fuzz_mutator! { |data: &mut [u8], size: usize, max_size: usize, seed: u32| {
	let keep = if data.starts_with(b"FNT1") { 3 } else { 0 };
	mutators::bytes_after(data, size, max_size, seed, keep)
}}

fuzz_target!(|data: &[u8]| {
	let budget = budget::Budget::parse().from_env();
	if data.len() > 1_000_000 {
		return;
	}
	let guard = budget::InputGuard::new();
	if check_committed(data) {
		coverage::hit(coverage::Hit::FontFaceValid);
		guard.finish(&budget, data.len());
		coverage::tick();
		return;
	}
	let _ = fonts::is_font(data);
	let _ = fonts::is_postscript_outline(data);
	// The directory scan: derived file names and contents, so a hostile
	// header and a hostile name meet in the same scan.
	let dir = tempfile::tempdir().expect("tempdir");
	let seed = oracle::derive(data);
	let n = 1 + (seed as usize % 4);
	for i in 0..n {
		let name = sanitize_name(data, i, n);
		let start = (i * data.len()) / n;
		let end = if i + 1 == n {
			data.len()
		} else {
			(i + 1) * data.len() / n
		};
		std::fs::write(dir.path().join(name), &data[start..end]).ok();
	}
	// `families` drives the shaper's own scan-and-register path over the
	// directory, which is where a malformed face is dropped; the names it
	// reports must never be empty.
	let config = FontConfig {
		ignore_system_fonts: true,
		directories: vec![dir.path().to_owned()],
		..Default::default()
	};
	for family in fonts::families(&config, false).iter() {
		assert!(!family.is_empty());
	}
	guard.finish(&budget, data.len());
	coverage::tick();
});

/// A file name the scan will accept, derived from the input at slot `i`.
fn sanitize_name(data: &[u8], i: usize, n: usize) -> String {
	let base = match data.get(i % data.len().max(1)) {
		Some(b'0') => "a.ttf",
		Some(b'1') => "b.otf",
		Some(b'2') => "c.ttf",
		Some(b'3') => "d.woff2",
		Some(_) => "e.ttf",
		None => "f.ttf",
	};
	if n > 1 {
		format!("{i}-{base}")
	} else {
		base.to_string()
	}
}
