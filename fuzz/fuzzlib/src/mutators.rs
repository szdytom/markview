//! Structure-aware mutation (F3).
//!
//! Pure `Arbitrary`/byte mutation grows coverage slowly on line-oriented
//! formats. These mutators edit whole lines and markers of a document: they
//! change heading levels, list markers, fence languages, table columns and
//! math delimiters, and insert, duplicate, move or delete whole lines drawn
//! from a structural alphabet. A quarter of the calls defer to libFuzzer's
//! byte mutator, which still catches encoding-level bugs.

use libfuzzer_sys::fuzzer_mutate;

/// SplitMix64: small, fast, deterministic per `seed`.
pub struct Rng(u64);

impl Rng {
	pub fn new(seed: u32) -> Self {
		// A zero state would stay zero; the offset guarantees forward progress.
		Self(u64::from(seed).wrapping_add(0x9E3779B97F4A7C15))
	}
	pub fn u32(&mut self) -> u32 {
		self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
		let mut z = self.0;
		z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
		z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
		(z ^ (z >> 31)) as u32
	}
	pub fn range(&mut self, n: usize) -> usize {
		if n == 0 {
			return 0;
		}
		self.u32() as usize % n
	}
	pub fn chance(&mut self, numerator: usize, denominator: usize) -> bool {
		self.range(denominator) < numerator
	}
}

/// TeX macro and brace fragments the math mutator splices in.
///
/// The `math` corpus reaches the macro expander only incidentally: a KaTeX
/// test-suite snippet that happens to contain `\def` is rare, and the byte
/// mutator has to discover `\`, `e`, `d`, `e`, `f` in order and then a
/// matching `{`/`}` pair. Every entry here is a token the expander, the depth
/// budget, or the argument reader treats specially, so a splice lands inside
/// a macro body far more often than random bytes do. The list is deliberately
/// *fragments* rather than whole formulas: a fragment composes with whatever
/// the corpus unit already is, which is what keeps the search from collapsing
/// onto one shape.
const MATH_MACRO_FRAGMENTS: &[&str] = &[
	// Definitions and expansion. `\edef` with a self-doubling body is the
	// known exponential path; the fragments below are the parts that make it
	// reachable from an arbitrary seed rather than a hand-written one.
	"\\def",
	"\\edef",
	"\\xdef",
	"\\gdef",
	"\\let",
	"\\newcommand",
	"\\renewcommand",
	"\\providecommand",
	"\\global",
	"\\globaldefs",
	"\\noexpand",
	"\\relax",
	"\\futurelet",
	"\\expandafter",
	"\\csname",
	"\\endcsname",
	"\\begingroup",
	"\\endgroup",
	"\\bgroup",
	"\\egroup",
	"\\edef\\m{\\m\\m}",
	"\\def\\m{\\m}",
	"\\edef\\a{\\b}\\edef\\b{\\a}",
	// Character codes: the known `i64` accumulator, and its siblings.
	"\\char",
	"\\char\"",
	"\\char'",
	"\\char`",
	"\\@char",
	"\\char\"FFFFFFFFFFFFFFFF",
	"\\char9999999999999999999",
	"\\unicode",
	// Argument and grouping shapes.
	"{",
	"}",
	"{{",
	"}}",
	"#1",
	"#2",
	"{#1}",
	"[1]",
	// Depth and stacking: the constructs the depth budget names.
	"\\substack",
	"\\substack{",
	"\\mathchoice",
	"\\mathchoice{a}{b}{c}{d}",
	"\\mathstrut",
	"\\left",
	"\\right",
	"\\middle",
	"\\big",
	"\\Big",
	"\\bigg",
	"\\Bigg",
	"\\sqrt",
	"\\frac",
	"\\dfrac",
	"\\tfrac",
	"\\cfrac",
	"\\genfrac",
	"\\overline",
	"\\underline",
	"\\overbrace",
	"\\underbrace",
	"\\widehat",
	"\\widetilde",
	"\\overleftarrow",
	"\\overrightarrow",
	"\\xrightarrow",
	"\\xleftarrow",
	// Boxes, phantoms and vertical movement.
	"\\hbox",
	"\\text",
	"\\mbox",
	"\\phantom",
	"\\vphantom",
	"\\hphantom",
	"\\smash",
	"\\vcenter",
	"\\raisebox",
	"\\rlap",
	"\\llap",
	"\\rule",
	"\\kern",
	"\\hskip",
	"\\hspace",
	"\\mkern",
	"\\mskip",
	// Class, styling and colour wrappers.
	"\\mathrel",
	"\\mathord",
	"\\mathbin",
	"\\mathop",
	"\\mathpunct",
	"\\mathinner",
	"\\displaystyle",
	"\\textstyle",
	"\\scriptstyle",
	"\\scriptscriptstyle",
	"\\color",
	"\\textcolor",
	"\\colorbox",
	"\\fcolorbox",
	"\\Huge",
	"\\tiny",
	// Environments: the array reader has its own depth accounting.
	"\\begin{array}{c}",
	"\\begin{matrix}",
	"\\begin{cases}",
	"\\begin{aligned}",
	"\\begin{CD}",
	"\\begin{prooftree}",
	"\\end{array}",
	"\\end{matrix}",
	"\\end{cases}",
	"\\end{aligned}",
	"\\\\",
	"&",
	"\\hline",
	"\\hdashline",
	"\\cr",
	// Text-mode and unicode entries.
	"\\verb|",
	"\\verb",
	"\\text{",
	"\\ce{",
	"\\pu{",
	"\\unicode{x}",
	"\u{00b2}",
	"\u{2081}",
	"\u{1D62}",
	"\u{2C7C}",
	// Delimiters and symbols wide enough to matter to layout.
	"\\langle",
	"\\rangle",
	"\\vert",
	"\\Vert",
	"\\|",
	"\\,",
	"\\;",
	"\\!",
	"\\quad",
	"\\qquad",
	"\\limits",
	"\\nolimits",
	"\\tag{1}",
	"\\notag",
	"\\nonumber",
];

/// Structure-aware *LaTeX* mutation for the `math` target.
///
/// The math target consumes a bare formula, not a Markdown document, so the
/// line-oriented mutators below are the wrong shape: they think in headings
/// and fences that LaTeX reads as literal text. This one edits at TeX's own
/// granularity instead — it splices macro fragments, wraps spans in braced
/// groups, duplicates a span (which is what turns one `\edef` level into
/// two), and falls back to the byte mutator.
///
/// The fallback share is higher than the Markdown mutator's (1/2 rather than
/// 1/4): a formula is short, and libFuzzer's own mutator is good at finding
/// the byte-level `\`/`{`/`}` combinations that make a fragment parse.
pub fn math(data: &mut [u8], size: usize, max_size: usize, seed: u32) -> usize {
	let size = size.min(max_size);
	let mut rng = Rng::new(seed);
	if size == 0 || rng.chance(1, 2) {
		return fuzzer_mutate(data, size, max_size);
	}
	let mut text: Vec<u8> = data[..size].to_vec();
	// Only splice at a character boundary; a LaTeX source is UTF-8 and a
	// mid-scalar insert would be rejected before it reached the expander.
	// The buffer may not be valid UTF-8 at all (the target lossily converts),
	// so boundaries come from the bytes rather than from `str`.
	let mut boundaries: Vec<usize> = (0..=text.len())
		.filter(|i| *i == text.len() || text[*i] & 0xC0 != 0x80)
		.collect();
	if boundaries.len() < 2 {
		return fuzzer_mutate(data, size, max_size);
	}
	boundaries.pop();

	match rng.range(8) {
		// Splice a macro fragment at a random boundary.
		0 | 1 => {
			let frag =
				MATH_MACRO_FRAGMENTS[rng.range(MATH_MACRO_FRAGMENTS.len())];
			let at = boundaries[rng.range(boundaries.len())];
			text.splice(at..at, frag.bytes());
		}
		// Delete a random span: shortens a macro name, strips a brace.
		2 => {
			let (lo, hi) = span(&mut rng, &boundaries);
			text.drain(lo..hi);
		}
		// Duplicate a span in place. On `\edef\m1{\m0\m0}` this is exactly
		// the doubling step, so the exponential path is one mutation away
		// from any seed that already holds one level.
		3 => {
			let (lo, hi) = span(&mut rng, &boundaries);
			let copied = text[lo..hi].to_vec();
			text.splice(hi..hi, copied);
		}
		// Wrap a span in braces.
		4 => {
			let (lo, hi) = span(&mut rng, &boundaries);
			text.splice(lo..lo, *b"{");
			text.splice(hi + 1..hi + 1, *b"}");
		}
		// Drop the first brace, if there is one.
		5 => {
			if let Some(pos) =
				text.iter().position(|b| *b == b'{' || *b == b'}')
			{
				text.remove(pos);
			}
		}
		// Move a span: relocates a macro body into another macro's argument,
		// which is how a benign definition becomes a self-referential one.
		6 => {
			let (lo, hi) = span(&mut rng, &boundaries);
			let moved: Vec<u8> = text.drain(lo..hi).collect();
			if !moved.is_empty() {
				let at =
					boundaries[rng.range(boundaries.len())].min(text.len());
				text.splice(at..at, moved);
			}
		}
		// Swap two adjacent bytes, so a `\def` can become `\edef` and back.
		_ => {
			if text.len() >= 2 {
				let i = rng.range(text.len() - 1);
				text.swap(i, i + 1);
			}
		}
	}
	text.truncate(max_size);
	let len = text.len().min(data.len());
	data[..len].copy_from_slice(&text[..len]);
	len
}

/// An ordered `(lo, hi)` byte range drawn from two random boundaries.
fn span(rng: &mut Rng, boundaries: &[usize]) -> (usize, usize) {
	let a = boundaries[rng.range(boundaries.len())];
	let b = boundaries[rng.range(boundaries.len())];
	if a <= b { (a, b) } else { (b, a) }
}

/// Structural one-liners the Markdown mutator can insert at any position.
const MD_LINES: &[&str] = &[
	"---",
	"### Heading three",
	"#### Deeper",
	"- item",
	"* other item",
	"1. numbered",
	"   - nested",
	"1. ordered again",
	"```",
	"```rust",
	"~~~yaml",
	"let x = 1 + 1; // code",
	"| a | b | c |",
	"|---|:-:|---:|",
	"| 1 | two | **three** |",
	"$e = mc^2$",
	"$$\\int_0^1 x^2\\,dx$$",
	"![alt](https://example.com/a.png){width=120}",
	"[link](https://example.com/doc#sec)",
	"[^1]: a footnote body",
	"see [^1] and the text",
	"<details open>",
	"<summary>the summary line</summary>",
	"</details>",
	"<br>",
	"**bold** __ital__ ~~gone~~ `code`",
	"中文段落 with CJK text mixed in",
	"\ttab-indented line",
	"a long unbroken word wordwordword wordwordword wordwordword",
	"  ",
	"",
];

/// Structural one-liners the MVSS mutator can insert.
const MVSS_LINES: &[&str] = &[
	"format_version = 2",
	"version = 3",
	"targets = [\"ui\"]",
	"[fontdef]",
	"id = \"F\"",
	"lookfor = [\"F\", \"Fallback\"]",
	"type = \"cjk\"",
	"[[font-family]]",
	"name = \"DL\"",
	"[meta]",
	"name = \"Theme\"",
	"description = \"d\"",
	"[page]",
	"paper = \"a4\"",
	"margin = [18.0]",
	"[page.header]",
	"rule_width = 1.0",
	"rule_color = \"#FF0000\"",
	"[[rule]]",
	"conditions = [\"body\"]",
	"font_size = 1.1",
	"color = \"#010203\"",
	"padding = [1.0, 2.0, 3.0, 4.0]",
	"border_width = 0.5",
	"border_edges = [0.0, 1.0, 2.0, 3.0]",
	"corner_radii = [1.0, 2.0, 3.0, 4.0]",
	"[[rule]]",
	"conditions = [\"heading\"]",
	"space_before = 0.5",
	"shape = [{ symbol = \"•\" }]",
	"[svg]",
	"generic_families = [{ family = \"serif\", candidates = [\"F\"] }]",
	"[mermaid]",
	"families = [\"F\"]",
];

/// Structure-aware Markdown mutation: line and marker edits over the input,
/// with a byte-mutation fallback.
pub fn markdown(
	data: &mut [u8],
	size: usize,
	max_size: usize,
	seed: u32,
) -> usize {
	let size = size.min(max_size);
	let mut rng = Rng::new(seed);
	if size == 0 || rng.chance(1, 4) {
		return fuzzer_mutate(data, size, max_size);
	}
	let buf = &mut data[..size];
	let mut lines: Vec<Vec<u8>> =
		buf.split(|b| *b == b'\n').map(Vec::from).collect();
	if lines.is_empty() {
		lines.push(Vec::new());
	}
	match rng.range(6) {
		// Rewrite one line's structural marker.
		0 => {
			let i = rng.range(lines.len());
			alter_md_line(&mut rng, &mut lines[i]);
		}
		// Insert a structural line.
		1 => {
			let new = if rng.chance(1, 2) {
				MD_LINES[rng.range(MD_LINES.len())].as_bytes().to_vec()
			} else {
				lines[rng.range(lines.len())].clone()
			};
			lines.insert(rng.range(lines.len() + 1), new);
		}
		// Delete a line.
		2 => {
			lines.remove(rng.range(lines.len()));
		}
		// Move a line.
		3 => {
			let from = rng.range(lines.len());
			let line = lines.remove(from);
			lines.insert(rng.range(lines.len() + 1), line);
		}
		// Byte-perturb one line's content.
		4 => {
			let i = rng.range(lines.len());
			let line = &mut lines[i];
			perturb(&mut rng, line);
		}
		// Change one line's indentation.
		_ => {
			let i = rng.range(lines.len());
			let line = &mut lines[i];
			let stripped = line
				.iter()
				.position(|b| !b.is_ascii_whitespace())
				.unwrap_or(line.len());
			line.drain(..stripped);
			line.splice(
				0..0,
				"  ".repeat(rng.range(4)).chars().map(|c| c as u8),
			);
		}
	}
	let mut out: Vec<u8> = Vec::with_capacity(size);
	for (i, line) in lines.iter().enumerate() {
		if i > 0 {
			out.push(b'\n');
		}
		out.extend_from_slice(line);
		if out.len() >= max_size {
			break;
		}
	}
	out.truncate(max_size);
	data[..out.len()].copy_from_slice(&out);
	out.len()
}

/// Marker-level rewrites for one Markdown line.
fn alter_md_line(rng: &mut Rng, line: &mut Vec<u8>) {
	let stripped = line
		.iter()
		.position(|b| !b.is_ascii_whitespace())
		.unwrap_or(line.len());
	let indent: Vec<u8> = line[..stripped].to_vec();
	let mut body: Vec<u8> = line.split_off(stripped);
	let head = |n: usize| body.get(..n).unwrap_or(&[]);
	let is_list = head(1)
		.first()
		.is_some_and(|c| *c == b'-' || *c == b'*' || *c == b'+')
		|| (head(2).last().is_some_and(|c| *c == b'.')
			&& head(2).first().is_some_and(|c| c.is_ascii_digit()));
	match rng.range(8) {
		// Heading: set a new level.
		0 if head(1) == b"#" => {
			let level = 1 + rng.range(6);
			while body.len() > level {
				body.pop();
			}
			while body.len() < level {
				body.push(b'#');
			}
			if body.get(level) != Some(&b' ') {
				body.insert(level, b' ');
			}
			while body.len() < level + 1 {
				body.push(b'x');
			}
		}
		// Turn a plain line into a heading.
		1 if head(1) != b"#" && !body.is_empty() => {
			let level = 1 + rng.range(3);
			body.splice(0..0, vec![b'#'; level]);
			if body.get(level) != Some(&b' ') {
				body.insert(level, b' ');
			}
		}
		// List marker: cycle between -, *, + and ordered.
		2 if is_list => {
			let is_ordered = head(2).last() == Some(&b'.')
				&& head(2).first().is_some_and(|c| c.is_ascii_digit());
			let marker: &[u8] = if is_ordered {
				if rng.chance(1, 2) { b"* " } else { b"2. " }
			} else {
				match rng.range(3) {
					0 => b"- ",
					1 => b"* ",
					_ => b"1. ",
				}
			};
			// Drop the current marker up to its space, keep the item text.
			let rest_start = body
				.iter()
				.position(|b| *b == b' ')
				.map(|i| i + 1)
				.unwrap_or(0);
			let rest: Vec<u8> = body[rest_start..].to_vec();
			body.clear();
			body.extend_from_slice(marker);
			body.extend_from_slice(&rest);
		}
		// Code fence: change the fence kind or strip the language.
		3 if head(3) == b"```" || head(3) == b"~~~" => {
			// The language is alphanumeric; a pure ```lang line drops the
			// whole remainder.
			let rest_start = match body
				.iter()
				.skip(3)
				.position(|b| !b.is_ascii_alphanumeric())
			{
				Some(i) => 3 + i,
				None => body.len(),
			};
			let rest: Vec<u8> = body[rest_start..].to_vec();
			body.clear();
			let fence = if rng.chance(1, 2) { "~~~" } else { "```" };
			let lang = if rng.chance(3, 4) {
				["", "rust", "python", "js", "go", "yaml", "c"][rng.range(7)]
			} else {
				""
			};
			body.extend_from_slice(fence.as_bytes());
			body.extend_from_slice(lang.as_bytes());
			body.extend_from_slice(&rest);
		}
		// Table row: add or drop a cell.
		4 => {
			if body.first() == Some(&b'|') {
				if rng.chance(1, 2) {
					body.extend_from_slice(b" | cell");
				} else if let Some(pos) = body.iter().rposition(|b| *b == b'|')
					&& body.len() > 2
				{
					let from = pos.saturating_sub(3).min(pos);
					body.drain(from..=pos);
				}
			} else {
				body.insert(0, b'|');
				body.push(b'|');
			}
		}
		// Math: toggle the $ delimiters.
		5 if head(1) == b"$" || rng.chance(1, 3) => {
			if body.first() == Some(&b'$') {
				while body.first() == Some(&b'$') {
					body.remove(0);
				}
				while body.last() == Some(&b'$') {
					body.pop();
				}
			} else if !body.is_empty() {
				body.push(b'$');
				body.insert(0, b'$');
			}
		}
		// Front matter: toggle the leading `---`.
		6 if head(3) == b"---" => {
			body.drain(..3.min(body.len()));
		}
		_ => perturb(rng, &mut body),
	}
	line.clear();
	line.extend_from_slice(&indent);
	line.extend_from_slice(&body);
}

/// MVSS is line-oriented TOML, so the mutator edits lines and values.
pub fn mvss(data: &mut [u8], size: usize, max_size: usize, seed: u32) -> usize {
	let size = size.min(max_size);
	let mut rng = Rng::new(seed);
	if size == 0 || rng.chance(1, 4) {
		return fuzzer_mutate(data, size, max_size);
	}
	let buf = &mut data[..size];
	let mut lines: Vec<Vec<u8>> =
		buf.split(|b| *b == b'\n').map(Vec::from).collect();
	if lines.is_empty() {
		lines.push(Vec::new());
	}
	match rng.range(5) {
		// Rewrite one line's value.
		0 => {
			let i = rng.range(lines.len());
			let line = &mut lines[i];
			rewrite_mvss_line(&mut rng, line);
		}
		// Insert a structural line.
		1 => {
			let new = if rng.chance(1, 2) {
				MVSS_LINES[rng.range(MVSS_LINES.len())].as_bytes().to_vec()
			} else {
				lines[rng.range(lines.len())].clone()
			};
			lines.insert(rng.range(lines.len() + 1), new);
		}
		2 => {
			lines.remove(rng.range(lines.len()));
		}
		3 => {
			let from = rng.range(lines.len());
			let line = lines.remove(from);
			lines.insert(rng.range(lines.len() + 1), line);
		}
		_ => {
			let i = rng.range(lines.len());
			let line = &mut lines[i];
			perturb(&mut rng, line);
		}
	}
	let mut out: Vec<u8> = Vec::with_capacity(size);
	for (i, line) in lines.iter().enumerate() {
		if i > 0 {
			out.push(b'\n');
		}
		out.extend_from_slice(line);
		if out.len() >= max_size {
			break;
		}
	}
	out.truncate(max_size);
	data[..out.len()].copy_from_slice(&out);
	out.len()
}

/// Change the value side of a `key = value` line, or the name of a
/// `[section]` header, to a structurally valid-looking alternative.
fn rewrite_mvss_line(rng: &mut Rng, line: &mut Vec<u8>) {
	let text = std::str::from_utf8(line).unwrap_or_default();
	if let Some((key, _)) = text.split_once('=')
		&& !key.trim().is_empty()
	{
		let value = match rng.range(5) {
			0 => format!("\"{}\"", key.trim().chars().next().unwrap_or('x')),
			1 => format!("{}", rng.range(100000) as i64),
			2 => format!("{:.1}", rng.range(1000) as f64 / 10.0),
			3 => if rng.chance(1, 2) { "true" } else { "false" }.to_string(),
			_ => format!(
				"[{}, {}, {}]",
				rng.range(9),
				rng.range(9),
				rng.range(9)
			),
		};
		*line = format!("{key} = {value}").into_bytes();
	} else if text.trim_start().starts_with('[') {
		let name = [
			"fontdef",
			"meta",
			"page",
			"svg",
			"mermaid",
			"rule",
			"font-family",
		][rng.range(7)];
		let double = rng.chance(1, 3);
		*line = format!(
			"{}[{}]{}",
			if double { "[" } else { "" },
			name,
			if double { "]" } else { "" }
		)
		.into_bytes();
	} else {
		perturb(rng, line);
	}
}

/// Small in-place byte edits: flip, insert, delete, rotate.
fn perturb(rng: &mut Rng, line: &mut Vec<u8>) {
	if line.is_empty() {
		line.push(b"ab c~"[rng.range(5)]);
		return;
	}
	match rng.range(4) {
		0 => {
			let i = rng.range(line.len());
			line[i] = rng.u32() as u8;
		}
		1 => {
			line.insert(rng.range(line.len() + 1), rng.u32() as u8);
		}
		2 => {
			line.remove(rng.range(line.len()));
		}
		_ => {
			let k = rng.range(line.len());
			line.rotate_left(k);
		}
	}
}

/// Structural HTML the webpage mutator can insert.
///
/// Readability only explores a page whose article candidate scores high
/// enough, so random bytes rarely reach the lowering at all. These lines are
/// the elements the lowering branches on: a title marker, a table with spans,
/// a disclosure, a formula, a checklist and a link.
const HTML_LINES: &[&str] = &[
	"<h1>Web article</h1>",
	"<h2 id=\"section\">A section</h2>",
	"<p>Enough readable words to keep this paragraph in the extracted article body and to give the candidate a score.</p>",
	"<ul><li>first item</li><li>second item</li></ul>",
	"<ol start=\"3\" reversed><li value=\"7\">numbered</li></ol>",
	"<blockquote><p>quoted words that keep their own block</p></blockquote>",
	"<pre><code class=\"language-rust\">fn main() {}</code></pre>",
	"<pre><code class=\"language-mermaid\">graph TD; A-->B</code></pre>",
	"<table><caption>Numbers</caption><thead><tr><th rowspan=\"2\">A</th><th colspan=\"2\">B</th></tr><tr><th>C</th><th>D</th></tr></thead></table>",
	"<tbody><tr><td rowspan=\"0\">all</td><td align=\"center\">1</td></tr><tr><td>2</td><td>3</td></tr></tbody>",
	"<details open><summary>More</summary><p>hidden body text</p></details>",
	"<figure><img src=\"plot.png\" alt=\"Plot\" width=\"320\"><figcaption>A plot</figcaption></figure>",
	"<dl><dt>Term</dt><dd>Definition</dd><dt>Other</dt><dd>Second</dd></dl>",
	"<p><span class=\"math inline\">\\(x^2\\)</span> and <script type=\"math/tex; mode=display\">\\[y\\]</script></p>",
	"<p><a href=\"/relative/post\">a link</a> and <a href=\"#local\">a fragment</a></p>",
	"<p><input type=\"checkbox\" checked><strong>bold</strong> <em>italic</em> <code>code</code></p>",
	"<hr>",
	"<div id=\"wrapper\"><section><p>nested container text</p></section></div>",
	"<svg width=\"20\" height=\"20\"><path d=\"M0 0\"/></svg>",
];

/// The webpage target's structure-aware mutator: line edits over an HTML
/// alphabet, plus element-level attribute rewrites.
pub fn html(data: &mut [u8], size: usize, max_size: usize, seed: u32) -> usize {
	let size = size.min(max_size);
	let mut rng = Rng::new(seed);
	if size == 0 || rng.chance(1, 4) {
		return fuzzer_mutate(data, size, max_size);
	}
	let buf = &mut data[..size];
	let mut lines: Vec<Vec<u8>> =
		buf.split(|b| *b == b'\n').map(Vec::from).collect();
	if lines.is_empty() {
		lines.push(Vec::new());
	}
	match rng.range(6) {
		// Insert a structural element.
		0 => {
			let new = if rng.chance(2, 3) {
				HTML_LINES[rng.range(HTML_LINES.len())].as_bytes().to_vec()
			} else {
				lines[rng.range(lines.len())].clone()
			};
			lines.insert(rng.range(lines.len() + 1), new);
		}
		// Delete an element or line.
		1 => {
			lines.remove(rng.range(lines.len()));
		}
		// Duplicate a line, the cheapest way to grow a table body.
		2 => {
			let i = rng.range(lines.len());
			let line = lines[i].clone();
			lines.insert(rng.range(lines.len() + 1), line);
		}
		// Move a line.
		3 => {
			let from = rng.range(lines.len());
			let line = lines.remove(from);
			lines.insert(rng.range(lines.len() + 1), line);
		}
		// Rewrite one tag's attributes.
		4 => {
			let i = rng.range(lines.len());
			alter_html_line(&mut rng, &mut lines[i]);
		}
		// Byte-perturb one line.
		_ => {
			let i = rng.range(lines.len());
			let line = &mut lines[i];
			perturb(&mut rng, line);
		}
	}
	let mut out: Vec<u8> = Vec::with_capacity(size);
	for (i, line) in lines.iter().enumerate() {
		if i > 0 {
			out.push(b'\n');
		}
		out.extend_from_slice(line);
		if out.len() >= max_size {
			break;
		}
	}
	out.truncate(max_size);
	data[..out.len()].copy_from_slice(&out);
	out.len()
}

/// Rewrites the attributes of the first tag on one line.
fn alter_html_line(rng: &mut Rng, line: &mut Vec<u8>) {
	let Ok(text) = std::str::from_utf8(line) else {
		perturb(rng, line);
		return;
	};
	let Some(open) = text.find('<') else {
		perturb(rng, line);
		return;
	};
	let Some(close) = text[open..].find('>').map(|at| at + open) else {
		perturb(rng, line);
		return;
	};
	let name_end = text[open + 1..close]
		.find(|c: char| c.is_ascii_whitespace() || c == '/')
		.map(|at| at + open + 1)
		.unwrap_or(close);
	let name = &text[open + 1..name_end];
	let choices: &[&str] = match name.to_ascii_lowercase().as_str() {
		"td" | "th" => &[
			" align=\"center\"",
			" align=\"right\"",
			" colspan=\"2\"",
			" rowspan=\"2\"",
			" rowspan=\"0\"",
			"",
		],
		"a" => &[
			" href=\"relative/path\"",
			" href=\"#fragment\"",
			" href=\"\"",
			"",
		],
		"img" => &[
			" src=\"pic.png\" alt=\"Plot\"",
			" src=\"data:image/png;base64,AA==\"",
			" width=\"10\" height=\"20\"",
			"",
		],
		"details" => &[" open", ""],
		"ol" => &[" start=\"2\"", " reversed", " start=\"0\" reversed", ""],
		"li" => &[" value=\"5\"", ""],
		"p" | "div" | "section" | "span" => &[
			" id=\"marker\"",
			" class=\"math inline\"",
			" aria-hidden=\"true\"",
			" role=\"presentation\"",
			"",
		],
		"h1" | "h2" | "h3" => &[" id=\"marker\"", " id=\"\"", ""],
		"script" => {
			&[" type=\"math/tex\"", " type=\"math/tex; mode=display\"", ""]
		}
		"base" => {
			&[" href=\"/assets/\"", " href=\"https://other.example/\"", ""]
		}
		_ => &["", ""],
	};
	let attributes = choices[rng.range(choices.len())];
	let mut rewritten = String::with_capacity(text.len() + 24);
	rewritten.push_str(&text[..name_end]);
	rewritten.push_str(attributes);
	if let Some(rest) = text[close..].strip_prefix('>') {
		rewritten.push('>');
		rewritten.push_str(rest);
	} else {
		rewritten.push_str(&text[close..]);
	}
	*line = rewritten.into_bytes();
}

/// Mutates the body after a structured header, so a target keeps its mode and
/// control bytes while the payload follows its own grammar.
pub fn after(
	data: &mut [u8],
	size: usize,
	max_size: usize,
	seed: u32,
	keep: usize,
	mutate: fn(&mut [u8], usize, usize, u32) -> usize,
) -> usize {
	let size = size.min(max_size);
	let keep = keep.min(size).min(max_size);
	if keep == 0 {
		return mutate(data, size, max_size, seed);
	}
	let body = mutate(
		&mut data[keep..],
		size.saturating_sub(keep),
		max_size.saturating_sub(keep),
		seed,
	);
	keep + body
}

/// [`after`] with libFuzzer's byte mutator for the body.
pub fn bytes_after(
	data: &mut [u8],
	size: usize,
	max_size: usize,
	seed: u32,
	keep: usize,
) -> usize {
	let size = size.min(max_size);
	let keep = keep.min(size).min(max_size);
	if keep == 0 {
		return fuzzer_mutate(data, size, max_size);
	}
	let _ = seed;
	let body = fuzzer_mutate(
		&mut data[keep..],
		size.saturating_sub(keep),
		max_size.saturating_sub(keep),
	);
	keep + body
}

/// [`after`] with the Markdown mutator.
pub fn markdown_after(
	data: &mut [u8],
	size: usize,
	max_size: usize,
	seed: u32,
	keep: usize,
) -> usize {
	after(data, size, max_size, seed, keep, markdown)
}

/// [`after`] with the TeX mutator.
pub fn math_after(
	data: &mut [u8],
	size: usize,
	max_size: usize,
	seed: u32,
	keep: usize,
) -> usize {
	after(data, size, max_size, seed, keep, math)
}

/// The minimum buffer length the `RPS1` header, records, pool and body need.
pub fn reparse_required(data: &[u8]) -> Option<usize> {
	if !data.starts_with(b"RPS1") || data.len() < 13 {
		return None;
	}
	let word = |at: usize| {
		u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
			as usize
	};
	Some(13 + usize::from(data[4]).min(16) * 9 + word(9) + word(5))
}

/// Keeps the `RPS1` header, count and length words while mutating the records,
/// pool and body. The body slice is only valid when the buffer still holds
/// `body_at + body_len` bytes, so a shrunken mutation is padded back out.
pub fn reparse(
	data: &mut [u8],
	size: usize,
	max_size: usize,
	seed: u32,
) -> usize {
	let size = size.min(max_size);
	let Some(required) = reparse_required(&data[..size]) else {
		return markdown(data, size, max_size, seed);
	};
	// Keep the raw-edit path in reach: a header-preserving mutation alone
	// would never exercise `apply_deterministic_edit` again.
	if Rng::new(seed).chance(1, 2) {
		return markdown(data, size, max_size, seed);
	}
	let len = bytes_after(data, size, max_size, seed, 13);
	if len < required && required <= max_size {
		data[len..required].fill(b' ');
		required
	} else {
		len
	}
}
