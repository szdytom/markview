//! G5, Tier 1 + Tier 2: parley/swash shaping over arbitrary text and sizes,
//! plus the stylesheet validation the shaper runs on install. Oracle (O1,
//! O2, I7): no panic within budgets; advances and offsets finite.
#![no_main]

use libfuzzer_sys::{fuzz_mutator, fuzz_target};
use markview_layout::{scene::Paint, shaping::TextShaper, style::Stylesheet};
use mvfuzz::{budget, coverage, mutators, oracle, pipeline};

/// `SHP1`, a size byte, a second control byte, then bytes mapped onto a
/// multilingual alphabet. The result shapes real scripts, combining marks,
/// emoji and bidi runs. Returns the shaped text length: one input byte
/// expands to a whole word, so the shaped text — not the fuzz input — is what
/// the allocation budget has to cover.
fn check_structured(data: &[u8]) -> Option<usize> {
	if !data.starts_with(b"SHP1") || data.len() < 6 {
		return None;
	}
	const WORDS: &[&str] = &[
		"alpha",
		"مرحبا",
		"中文",
		"हिन्दी",
		"e\u{301}",
		"👩‍💻",
		"office",
		"שלום",
		"ไทย",
		"한국어",
		"Zürich",
		"á",
	];
	let body = &data[6..];
	let mut text = String::new();
	for byte in body.iter().take(2048) {
		if !text.is_empty() {
			text.push(' ');
		}
		text.push_str(WORDS[usize::from(*byte) % WORDS.len()]);
	}
	let shaped = text.len();
	let size = 8. + f32::from(data[4]) / 8.;
	let mut shaper = TextShaper::with_fonts(pipeline::pinned_fonts());
	let (draws, width) =
		shaper.label_measured(&text, size, 0., 10., Paint::Text);
	assert!(width.is_finite() && width >= 0.);
	let glyphs = draws
		.iter()
		.filter(|draw| matches!(draw, markview_layout::scene::Draw::Glyph(_)))
		.count();
	assert!(
		text.is_empty() || glyphs > 0,
		"structured text shaped no glyphs"
	);
	for draw in &draws {
		if let markview_layout::scene::Draw::Glyph(glyph) = draw {
			assert!(glyph.x.is_finite() && glyph.y.is_finite());
		}
	}
	// Fixed input and fixed faces must shape identically on a second pass.
	let (again, width_again) =
		shaper.label_measured(&text, size, 0., 10., Paint::Text);
	assert_eq!(
		glyphs,
		again
			.iter()
			.filter(|draw| matches!(
				draw,
				markview_layout::scene::Draw::Glyph(_)
			))
			.count()
	);
	assert_eq!(width, width_again);
	Some(shaped)
}

fuzz_mutator! { |data: &mut [u8], size: usize, max_size: usize, seed: u32| {
	let keep = if data.starts_with(b"SHP1") { 6 } else { 0 };
	mutators::bytes_after(data, size, max_size, seed, keep)
}}

fuzz_target!(|data: &[u8]| {
	let budget = budget::Budget::parse().from_env();
	let text = String::from_utf8_lossy(data);
	if text.len() > 256 * 1024 {
		return;
	}
	let guard = budget::InputGuard::new();
	if let Some(shaped) = check_structured(data) {
		coverage::hit(coverage::Hit::ShapingStructured);
		// The budget scales with the shaped text: a 2 KiB input expands to
		// ~10 KiB of words, and shaping allocates per glyph, so metering the
		// fuzz bytes alone undercounted the work by the expansion factor.
		guard.finish(&budget, shaped.max(data.len()));
		coverage::tick();
		return;
	}
	let seed = oracle::derive(data);
	// Sizes vary in the plausible range; one in eight takes a raw bit
	// pattern (tiny, huge, denormal, or `NaN`) to stress the metric scaling.
	// The shaper owes finiteness of its results to a finite size: a `NaN`
	// in may legally give a `NaN` out. The bounded oracle applies where the
	// size itself is bounded: a label at a 1e30-point size is allowed to be
	// wide, just not infinite.
	let (size, bounded) = if seed & 7 == 0 {
		(f32::from_bits(seed as u32), false)
	} else {
		(oracle::f32_unit(seed as u32) * 16.0, true)
	};
	// GIGO: the shaper owes finiteness to a finite size while the
	// per-character advance sum cannot overflow `f32`. Past that, an
	// infinite result is the honest outcome of `f32` arithmetic, so the
	// oracle relaxes to "not NaN"; a `NaN` size may still give a `NaN`
	// out, so a non-finite size skips the check.
	let size_ok = size.is_finite();
	let no_overflow = size_ok && size.abs() <= 1e38 / text.len().max(1) as f32;
	fn check(
		name: &str,
		v: f32,
		size_ok: bool,
		no_overflow: bool,
		bounded: bool,
	) {
		if !size_ok {
			return;
		}
		if no_overflow {
			assert!(v.is_finite(), "{name} is not finite: {v:?}");
			if bounded {
				oracle::assert_bounded(name, v);
			}
		} else {
			assert!(!v.is_nan(), "{name} is NaN: {v:?}");
		}
	}
	// The validation path runs on install, both for a real sheet and for a
	// sheet whose rules were stripped out from under it.
	let mut shaper = TextShaper::with_fonts(pipeline::pinned_fonts());
	if let Err(e) =
		shaper.validate_stylesheet(Stylesheet::bundled(false).as_ref())
	{
		let _ = e.to_string();
	}
	let mut stripped = (*Stylesheet::bundled(false)).clone();
	stripped.rules.clear();
	if let Err(e) = shaper.validate_stylesheet(&stripped) {
		let _ = e.to_string();
	}
	let (draws, width) =
		shaper.label_measured(&text, size, 0.0, 10.0, Paint::Text);
	check("label.width", width, size_ok, no_overflow, bounded);
	for d in &draws {
		if let markview_layout::scene::Draw::Glyph(g) = d {
			check("glyph.x", g.x, size_ok, no_overflow, bounded);
			check("glyph.y", g.y, size_ok, no_overflow, bounded);
		}
	}
	let fitted = shaper.fit(&text, size, 100.0);
	check(
		"fit.width",
		shaper.text_width(&fitted, size),
		size_ok,
		no_overflow,
		bounded,
	);
	guard.finish(&budget, text.len());
	coverage::tick();
});
