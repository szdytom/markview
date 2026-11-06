//! G4, Tier 1 + Tier 2: syntax highlighting through syntect, reached the way
//! the reader reaches it — a fenced code block in a layout pass. The unit is
//! `language \0 code`: the first NUL separates the two. Oracle (O1, O2, I7):
//! no panic within budgets; the uncolored fallback is a legal outcome.
#![no_main]

use libfuzzer_sys::fuzz_target;
use mvfuzz::{budget, oracle, pipeline};

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

/// A fence longer than any run in `code`, so the block cannot close early.
fn fence(code: &str) -> String {
	let mut longest = 0;
	let mut run = 0;
	for byte in code.bytes() {
		if byte == b'`' {
			run += 1;
			longest = longest.max(run);
		} else {
			run = 0;
		}
	}
	"`".repeat((longest + 2).max(3))
}

fn is_code(block: &markview_document::Block) -> Option<(&str, &str)> {
	match block.kind() {
		markview_document::BlockKind::Code { language, text } => {
			Some((language, text))
		}
		_ => None,
	}
}

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
	let fence = if raw { "```".to_owned() } else { fence(&code) };
	let md = format!("{fence}{language}\n{code}\n{fence}\n");
	let (doc, _options, snapshot) = pipeline::parse_layout(&md);
	if !raw {
		let expected = code.trim_end_matches('\n');
		let found = doc
			.document
			.blocks()
			.iter()
			.find_map(is_code)
			.expect("the fenced input did not become a code block");
		assert_eq!(found.1.trim_end_matches('\n'), expected);
	}
	oracle::assert_snapshot_finite(&snapshot);
	guard.finish(&budget, md.len());
});
