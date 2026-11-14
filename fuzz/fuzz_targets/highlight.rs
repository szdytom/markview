//! G4, Tier 1 + Tier 2: syntax highlighting through syntect, reached the way
//! the reader reaches it — a fenced code block in a layout pass. The unit is
//! `language \0 code`: the first NUL separates the two. Oracle (O1, O2, I7):
//! no panic within budgets; the uncolored fallback is a legal outcome.
#![no_main]

use libfuzzer_sys::fuzz_target;
use mvfuzz::{budget, highlight, oracle, pipeline};

/// Languages the shipped syntax set recognizes. The raw byte only selects;
/// it does not become an unknown fence language.
const LANGUAGES: &[&str] = &[
	"rust",
	"python",
	"javascript",
	"json",
	"yaml",
	"toml",
	"c",
	"cpp",
	"css",
	"html",
	"shell",
	"markdown",
];

fuzz_target!(|data: &[u8]| {
	let budget = budget::Budget::layout().from_env();
	let (lang, code) = oracle::split_nul(data);
	if lang.len() + code.len() > 128 * 1024 {
		return;
	}
	pipeline::warmup();
	let guard = budget::InputGuard::new();
	let raw = lang.starts_with("raw");
	let language = if raw {
		&lang[3..]
	} else {
		LANGUAGES[lang.bytes().fold(0usize, |n, byte| {
			n.wrapping_mul(31).wrapping_add(usize::from(byte))
		}) % LANGUAGES.len()]
	};
	let fence = if raw {
		"```".to_owned()
	} else {
		highlight::fence(&code)
	};
	let md = format!("{fence}{language}\n{code}\n{fence}\n");
	let (doc, _options, snapshot) = pipeline::parse_layout(&md);
	if !raw {
		highlight::assert_code(&doc, &code);
	}
	oracle::assert_snapshot_finite(&snapshot);
	guard.finish(&budget, md.len());
});
