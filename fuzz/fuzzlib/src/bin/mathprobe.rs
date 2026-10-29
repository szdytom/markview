//! Standalone measurement bench for the ratex math path.
//!
//! Where the `math` target judges one input against a budget, this prints the
//! numbers a record needs: wall time, peak live allocation (the counting
//! allocator's window high-water mark), node count, layout geometry, and
//! layout invariants. It reads formulas from files or from built-in
//! generators, so an amplification can be measured across its growth curve
//! without hand-writing each rung.
//!
//! ```sh
//! cargo +nightly build --release -p mvfuzz --bin mathprobe
//! mathprobe file <formula.tex>...        # one file per line of output
//! mathprobe gen <generator> [args...]    # a growth series
//! mathprobe list                         # available generators
//! ```
//!
//! **Bound it from outside.** Nothing in ratex 0.1.14 bounds an expansion, so
//! a generator can allocate without limit; run it as
//! `bash -c 'ulimit -v <kB> && exec mathprobe gen ...'` (a plain Rust binary,
//! so `ulimit -v` works — an ASAN build reserves terabytes of shadow space
//! and dies at startup instead).

use std::panic::{AssertUnwindSafe, catch_unwind};

use mvfuzz::probe::{Measured, measure};

type Formula = (String, String);

fn main() {
	let args: Vec<String> = std::env::args().skip(1).collect();
	if args.is_empty() {
		usage();
		return;
	}
	let formulas: Vec<Formula> = match args[0].as_str() {
		"file" => args[1..]
			.iter()
			.map(|p| {
				let text = std::fs::read_to_string(p)
					.unwrap_or_else(|e| panic!("{p}: {e}"));
				(p.clone(), text)
			})
			.collect(),
		"gen" => generate(&args[1..]),
		"list" => {
			println!("generators: {}", GENERATORS.join(", "));
			return;
		}
		other => {
			eprintln!("unknown mode {other:?}");
			usage();
			return;
		}
	};
	if formulas.is_empty() {
		eprintln!("no formulas");
		return;
	}
	for (label, latex) in formulas {
		run_one(&label, &latex);
	}
}

fn usage() {
	eprintln!("usage:");
	eprintln!("  mathprobe file <formula.tex>...");
	eprintln!("  mathprobe gen <generator> [args...]");
	eprintln!("  mathprobe list");
}

const GENERATORS: &[&str] = &[
	"edef-double",
	"def-self",
	"substack",
	"leftright",
	"mathchoice",
	"unicode-supsub",
	"char",
	"verb",
	"array",
	"sqrt",
	"braces",
	"katex-tests",
];

fn generate(args: &[String]) -> Vec<Formula> {
	let name = args.first().map(String::as_str).unwrap_or("");
	let nums = |i: usize, default: usize| -> usize {
		args.get(i).and_then(|s| s.parse().ok()).unwrap_or(default)
	};
	let series = |labels: Vec<String>, items: Vec<String>| -> Vec<Formula> {
		labels.into_iter().zip(items).collect()
	};
	match name {
		// A chain of body doublings: linear input, exponential tokens.
		"edef-double" => {
			let body = nums(1, 8);
			let max_levels = nums(2, 18);
			series(
				(0..=max_levels)
					.map(|l| format!("edef L={l} body={body}"))
					.collect(),
				(0..=max_levels)
					.map(|levels| edef_double(body, levels))
					.collect(),
			)
		}
		// One macro whose body ends by calling itself.
		"def-self" => {
			let sizes: Vec<usize> = match args.get(1).map(String::as_str) {
				Some("ramp") | None => vec![8, 32, 128, 512, 2048, 8192, 32768],
				Some(_) => {
					args[1..].iter().filter_map(|s| s.parse().ok()).collect()
				}
			};
			series(
				sizes.iter().map(|n| format!("def-self n={n}")).collect(),
				sizes.iter().map(|n| def_self(*n)).collect(),
			)
		}
		"substack" => {
			let sizes = ramp(args, 1, &[1, 2, 4, 8, 16, 24, 32, 40, 64]);
			series(
				sizes.iter().map(|n| format!("substack n={n}")).collect(),
				sizes.iter().map(|n| substack(*n)).collect(),
			)
		}
		"leftright" => {
			let sizes = ramp(args, 1, &[1, 2, 4, 8, 16, 32, 64, 128, 256]);
			series(
				sizes.iter().map(|n| format!("leftright n={n}")).collect(),
				sizes.iter().map(|n| leftright(*n)).collect(),
			)
		}
		"mathchoice" => {
			let sizes = ramp(args, 1, &[1, 2, 4, 8, 12, 16, 20, 24, 32]);
			series(
				sizes.iter().map(|n| format!("mathchoice n={n}")).collect(),
				sizes.iter().map(|n| mathchoice(*n)).collect(),
			)
		}
		"unicode-supsub" => {
			let sizes = ramp(args, 1, &[1, 2, 4, 8, 16, 32, 64, 128]);
			series(
				sizes
					.iter()
					.map(|n| format!("unicode-supsub n={n}"))
					.collect(),
				sizes.iter().map(|n| unicode_supsub(*n)).collect(),
			)
		}
		"char" => vec![
			("char dec ok".into(), "\\char65".into()),
			("char dec max".into(), format!("\\char{}", i64::MAX)),
			(
				"char dec over".into(),
				format!("\\char{}", i64::MAX as u128 + 1),
			),
			("char hex".into(), "\\char\"41".into()),
			("char hex over".into(), "\\char\"FFFFFFFFFFFFFFFF".into()),
			("char octal".into(), "\\char'101".into()),
			(
				"char octal over".into(),
				format!("\\char'{}", "7".repeat(40)),
			),
			("char cyrillic".into(), "\\char`\\я".into()),
			("char at".into(), "\\@char{65}".into()),
			(
				"char at over".into(),
				"\\@char{99999999999999999999}".into(),
			),
		],
		"verb" => vec![
			("verb ok".into(), "\\verb|x|".into()),
			("verb multibyte".into(), "\\verbéxé".into()),
			("verb astral".into(), "\\verb😀x😀".into()),
			("verb eof".into(), "\\verb|".into()),
		],
		"array" => {
			let sizes = ramp(args, 1, &[1, 2, 4, 8, 16, 32, 64]);
			series(
				sizes.iter().map(|n| format!("array n={n}")).collect(),
				sizes.iter().map(|n| array(*n)).collect(),
			)
		}
		"sqrt" => {
			let sizes = ramp(args, 1, &[1, 2, 4, 8, 16, 32, 64, 128, 256]);
			series(
				sizes.iter().map(|n| format!("sqrt n={n}")).collect(),
				sizes.iter().map(|n| sqrt(*n)).collect(),
			)
		}
		"braces" => {
			let sizes = ramp(args, 1, &[1, 2, 4, 8, 16, 32, 64, 128, 256]);
			series(
				sizes.iter().map(|n| format!("braces n={n}")).collect(),
				sizes.iter().map(|n| braces(*n)).collect(),
			)
		}
		"katex-tests" => katex_tests(),
		other => {
			eprintln!("unknown generator {other:?}; try `mathprobe list`");
			Vec::new()
		}
	}
}

/// Explicit list when given, otherwise the built-in ramp.
fn ramp(args: &[String], from: usize, default: &[usize]) -> Vec<usize> {
	match args.get(from) {
		None => default.to_vec(),
		// The parsed arguments keep the order they were written in; the
		// default ramp is the only ordered preset.
		Some(_) => args[from..].iter().filter_map(|s| s.parse().ok()).collect(),
	}
}

fn edef_double(body: usize, levels: usize) -> String {
	let mut lines = vec![format!("\\edef\\m0{{{}}}", "x".repeat(body))];
	for k in 1..=levels {
		lines.push(format!("\\edef\\m{k}{{\\m{}\\m{}}}", k - 1, k - 1));
	}
	lines.push(format!("\\m{levels}"));
	lines.join("\n")
}

fn def_self(n: usize) -> String {
	format!("\\def\\x{{{} \\x}}\\x", "a".repeat(n))
}

fn substack(n: usize) -> String {
	let rows = vec!["a"; n].join("\\\\");
	format!("\\substack{{{rows}}}")
}

fn leftright(n: usize) -> String {
	format!("{}{}", "\\left(".repeat(n), "\\right)".repeat(n))
}

fn mathchoice(n: usize) -> String {
	let mut s = String::new();
	for _ in 0..n {
		s.push_str("\\mathchoice{a}{b}{c}{d}");
	}
	s
}

fn unicode_supsub(n: usize) -> String {
	format!("x{}", "\u{00B2}".repeat(n))
}

fn array(n: usize) -> String {
	let cells = vec!["c"; n].join(" & ");
	format!("\\begin{{array}}{{c}}\\hline {cells} \\\\ \\hline\\end{{array}}")
}

fn sqrt(n: usize) -> String {
	format!(
		"{}{}",
		"\\sqrt{".repeat(n),
		"x".to_string() + &"}".repeat(n)
	)
}

fn braces(n: usize) -> String {
	format!("{}{}", "{".repeat(n), "}".repeat(n))
}

/// Inputs lifted from the KaTeX test suite's security-relevant cases: the
/// expansion-budget bypasses, the `\char` family, and the depth probes.
fn katex_tests() -> Vec<Formula> {
	let cases: &[(&str, &str)] = &[
		(
			"katex edef double",
			"\\edef\\m0{xx}\\edef\\m1{\\m0\\m0}\\edef\\m2{\\m1\\m1}\\edef\\m3{\\m2\\m2}\\edef\\m4{\\m3\\m3}\\edef\\m5{\\m4\\m4}\\m6",
		),
		("katex def self rec", "\\def\\x{x\\x}\\x"),
		("katex sup unicode recur", "x^2^2^2^2^2"),
		("katex sub unicode recur", "x_2_2_2_2_2"),
		(
			"katex expand after",
			"\\def\\a{\\b}\\def\\b{c}\\expandafter\\a\\a",
		),
		(
			"katex noexpand",
			"\\def\\a{b}\\edef\\b{\\noexpand\\a\\a}\\b",
		),
		("katex csname", "\\csname a\\endcsname"),
		(
			"katex csname nested",
			"\\csname\\csname a\\endcsname\\endcsname",
		),
		(
			"katex begingroup",
			"\\begingroup\\def\\a{b}\\a\\endgroup\\a",
		),
		("katex futurelet", "\\def\\a{b}\\futurelet\\x\\a\\x"),
		("katex bgroup", "\\bgroup\\def\\a{b}\\a\\egroup"),
		("katex text nesting", "\\text{\\text{\\text{\\text{a}}}}"),
		("katex hbox nesting", "\\hbox{\\hbox{\\hbox{\\hbox{a}}}}"),
		("katex rule", "\\rule{1em}{1em}"),
		("katex kern", "\\kern1em"),
		("katex vphantom", "\\vphantom{\\frac{a}{b}}"),
		("katex smash", "\\smash{\\frac{a}{b}}"),
		("katex genfrac", "\\genfrac{(}{)}{0pt}{}{a}{b}"),
		(
			"katex overline nest",
			"\\overline{\\overline{\\overline{a}}}",
		),
		("katex enclose", "\\enclose{updiagonalstrike}{a}"),
		("katex color nest", "\\color{red}{\\color{blue}{a}}"),
		("katex sizing nest", "\\Huge\\Huge\\Huge a"),
		("katex raisebox", "\\raisebox{1em}{a}"),
		("katex lap", "\\rlap{\\frac{a}{b}}"),
		("katex pmod", "\\pmod{n}"),
		("katex bmod", "a \\bmod b"),
		("katex op name nest", "\\operatorname{\\operatorname{a}}"),
		("katex cd", "\\begin{CD} A @>a>> B \\end{CD}"),
		(
			"katex bussproofs",
			"\\begin{prooftree}\\AxiomC{$A$}\\UnaryInfC{$B$}\\end{prooftree}",
		),
		("katex mhchem", "\\ce{H2O}"),
		("katex mhchem arrow", "\\ce{A -> B}"),
		("katex href", "\\href{https://x}{a}"),
		("katex html", "\\htmlClass{c}{a}"),
		(
			"katex includegraphics",
			"\\includegraphics[width=1em]{a.png}",
		),
		("katex verb", "\\verb|x|"),
		("katex verb multibyte", "\\verbéxé"),
		("katex char", "\\char65"),
		("katex char hex", "\\char\"41"),
		("katex char cyrillic", "\\char`\\я"),
		("katex wide hat", "\\widehat{abc}"),
		("katex overbrace", "\\overbrace{a}^{b}"),
		("katex xarrow", "a \\xrightarrow{b} c"),
		("katex tag", "\\tag{1} a"),
		("katex phantom nest", "\\phantom{\\phantom{\\phantom{a}}}"),
		("katex vcenter", "\\vcenter{a}"),
		("katex mclass", "\\mathrel{a}"),
		(
			"katex styling nest",
			"\\displaystyle\\textstyle\\scriptstyle a",
		),
		("katex delimiter sizing", "\\big\\Big\\bigg\\Bigg("),
		("katex matrix", "\\begin{matrix}a&b\\\\c&d\\end{matrix}"),
		("katex cases", "\\begin{cases}a&b\\\\c&d\\end{cases}"),
		(
			"katex aligned",
			"\\begin{aligned}a&=b\\\\c&=d\\end{aligned}",
		),
		("katex substack", "\\substack{a\\\\b\\\\c}"),
		("katex mathchoice", "\\mathchoice{a}{b}{c}{d}"),
		("katex unicode sup sub mix", "x²₃"),
		("katex nbsp", "a\u{a0}b"),
		("katex cr", "a\\cr b"),
		("katex hline", "\\hline"),
		("katex newline", "a\\newline b"),
		("katex allowbreak", "a\\allowbreak b"),
		("katex relax", "\\relax a"),
		("katex globaldef", "\\gdef\\a{b}\\a"),
		("katex let", "\\let\\a=\\b\\a"),
		("katex newcommand", "\\newcommand{\\a}{b}\\a"),
		(
			"katex newcommand nested",
			"\\newcommand{\\a}{\\newcommand{\\b}{c}}\\a\\b",
		),
		("katex expandafter edef", "\\edef\\a{\\b}\\def\\b{c}\\a"),
	];
	cases
		.iter()
		.map(|(label, latex)| ((*label).to_string(), (*latex).to_string()))
		.collect()
}

/// Runs one formula through `MathEngine::layout` and then through the raw
/// ratex entry points, printing every number a record might need.
fn run_one(label: &str, latex: &str) {
	let bytes = latex.len();
	println!("── {label}  ({} B)", bytes);
	println!("   input: {:?}", truncate(latex, 96));

	// The reader's own entry point: what Markview actually calls.
	let mut engine = markview_layout::math::MathEngine::default();
	let (result, m) = measure(bytes, || engine.layout(
		latex,
		true,
		18.0,
		&markview_layout::style::TextAppearance::default(),
	));
	println!("   MathEngine::layout   {}", summary(&m));
	match &result {
		Ok(b) => {
			println!(
				"     box w={} ascent={} descent={} items={}",
				b.width,
				b.ascent,
				b.descent,
				b.display.items.len()
			);
			report_invariants(&b.display);
		}
		Err(e) => println!("     Err({})", truncate(e, 120)),
	}

	// The raw parser: isolates ratex from the reader's byte budgets, which is
	// what an upstream record needs.
	let (parsed, pm) = measure(bytes, || {
		catch_unwind(AssertUnwindSafe(|| ratex_parser::parse(latex)))
	});
	match parsed {
		Ok(Ok(ast)) => {
			println!(
				"   ratex_parser::parse  {}  nodes={}",
				summary(&pm),
				ast.len()
			);
			let opts = ratex_layout::LayoutOptions::default();
			let (laid, lm) = measure(bytes, || {
				let layout = ratex_layout::layout(&ast, &opts);
				ratex_layout::to_display_list(&layout)
			});
			println!("   ratex_layout         {}", summary(&lm));
			report_invariants(&laid);
		}
		Ok(Err(e)) => println!(
			"   ratex_parser::parse  {}  Err({})",
			summary(&pm),
			truncate(&e.to_string(), 120)
		),
		Err(payload) => println!(
			"   ratex_parser::parse  {}  PANIC({})",
			summary(&pm),
			truncate(&panic_message(&payload), 120)
		),
	}
}

fn summary(m: &Measured) -> String {
	format!(
		"peak={:>12} B ({:>8.1} B/byte)  {:>8.3} s",
		m.peak_bytes,
		m.bytes_per_input_byte(),
		m.elapsed.as_secs_f64()
	)
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
	if let Some(s) = payload.downcast_ref::<&str>() {
		return (*s).to_string();
	}
	if let Some(s) = payload.downcast_ref::<String>() {
		return s.clone();
	}
	"<non-string panic>".to_string()
}

/// Layout invariants the `math` target does not check: a display list whose
/// reported geometry disagrees with its own items is wrong output even when
/// every number is finite.
///
/// Only finiteness and sign are asserted per item. `GlyphPath` carries a font
/// `scale` rather than a glyph advance, and `y` is the baseline from which
/// `height`/`depth` are measured, so "do the items fit the box" would need a
/// per-glyph advance from the font tables. A bogus warning is worse than no
/// warning, so that check is deliberately not attempted.
fn report_invariants(display: &ratex_types::DisplayList) {
	let mut warnings: Vec<String> = Vec::new();
	let (w, h, d) = (display.width, display.height, display.depth);
	if !(w.is_finite() && h.is_finite() && d.is_finite()) {
		warnings.push(format!("non-finite root geometry: {w} {h} {d}"));
	}
	if w < 0.0 || h < 0.0 || d < 0.0 {
		warnings.push(format!("negative root geometry: {w} {h} {d}"));
	}
	for (i, item) in display.items.iter().enumerate() {
		let fields: &[(&str, f64)] = match item {
			ratex_types::DisplayItem::GlyphPath { x, y, scale, .. } => {
				&[("x", *x), ("y", *y), ("scale", *scale)]
			}
			ratex_types::DisplayItem::Line {
				x,
				y,
				width,
				thickness,
				..
			} => &[
				("x", *x),
				("y", *y),
				("width", *width),
				("thickness", *thickness),
			],
			ratex_types::DisplayItem::Rect {
				x,
				y,
				width,
				height,
				..
			} => &[("x", *x), ("y", *y), ("width", *width), ("height", *height)],
			ratex_types::DisplayItem::Path { x, y, commands, .. } => {
				if !x.is_finite() || !y.is_finite() {
					warnings.push(format!(
						"item {i}: non-finite path anchor {x} {y}"
					));
				}
				for (k, command) in commands.iter().enumerate() {
					if !path_command_is_finite(command) {
						warnings.push(format!(
							"item {i}: path command {k} non-finite"
						));
					}
				}
				continue;
			}
		};
		for (name, v) in fields {
			if !v.is_finite() {
				warnings.push(format!("item {i}: non-finite {name}={v}"));
			}
		}
		if let ratex_types::DisplayItem::Rect { width, height, .. } = item
			&& (*width < 0.0 || *height < 0.0)
		{
			warnings.push(format!("item {i}: negative rect extent"));
		}
		if warnings.len() > 8 {
			break;
		}
	}
	for warning in &warnings {
		println!("     ⚠ {warning}");
	}
	if warnings.is_empty() {
		println!("     invariants ok ({} items)", display.items.len());
	}
}

/// Every coordinate a path command carries must be finite.
fn path_command_is_finite(command: &ratex_types::PathCommand) -> bool {
	use ratex_types::PathCommand as P;
	match command {
		P::MoveTo { x, y } | P::LineTo { x, y } => {
			x.is_finite() && y.is_finite()
		}
		P::CubicTo {
			x1,
			y1,
			x2,
			y2,
			x,
			y,
		} => [x1, y1, x2, y2, x, y].iter().all(|v| v.is_finite()),
		P::QuadTo { x1, y1, x, y } => {
			[x1, y1, x, y].iter().all(|v| v.is_finite())
		}
		P::Close => true,
	}
}

fn truncate(s: &str, n: usize) -> String {
	if s.len() <= n {
		return s.to_string();
	}
	let mut end = n;
	while !s.is_char_boundary(end) {
		end -= 1;
	}
	format!("{}…", &s[..end])
}

#[cfg(test)]
mod tests {
	use super::*;

	/// An explicit size list is one rung per argument, in the order given.
	#[test]
	fn explicit_ramp_keeps_argument_order() {
		let args: Vec<String> = ["sqrt", "1", "2", "4"]
			.iter()
			.map(|s| (*s).to_string())
			.collect();
		assert_eq!(ramp(&args, 1, &[9, 8]), vec![1, 2, 4]);
	}

	/// With no explicit arguments the built-in ramp is returned untouched.
	#[test]
	fn default_ramp_is_unchanged() {
		let args: Vec<String> = vec!["sqrt".to_string()];
		assert_eq!(ramp(&args, 1, &[1, 2, 4, 8]), vec![1, 2, 4, 8]);
	}

	/// A malformed rung is dropped, not replaced with a zero or reordered.
	#[test]
	fn unparsable_arguments_are_dropped() {
		let args: Vec<String> = ["sqrt", "2", "x", "8"]
			.iter()
			.map(|s| (*s).to_string())
			.collect();
		assert_eq!(ramp(&args, 1, &[9]), vec![2, 8]);
	}
}
