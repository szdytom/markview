//! Measure a formula twice in the same process, so a one-time lazy table is
//! visible as a large first window and a small second one. An amplification
//! scales with the input on *both* runs; an initializer does not.
//!
//! Run: `cargo +nightly build --release -p mvfuzz --bin warmcheck -- '\ce{H2O}'`

use mvfuzz::probe::measure;

fn main() {
	let args: Vec<String> = std::env::args().skip(1).collect();
	let latex = args.join(" ");
	for round in 1..=3 {
		let mut engine = markview_layout::math::MathEngine::default();
		let (result, m) =
			measure(latex.len(), || engine.layout(
				&latex,
				true,
				18.0,
				&markview_layout::style::TextAppearance::default(),
			));
		println!(
			"round {round}: peak={:>10} B  {:>9.3} s  {}",
			m.peak_bytes,
			m.elapsed.as_secs_f64(),
			match result {
				Ok(b) => format!("Ok items={}", b.display.items.len()),
				Err(e) => format!("Err({e})"),
			}
		);
	}
}
