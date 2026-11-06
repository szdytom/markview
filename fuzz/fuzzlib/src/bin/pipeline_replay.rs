//! Replay integration corpora without libFuzzer, reporting the failing file.
use mvfuzz::{
	coverage, font_archive, highlight_schedule, history, image_decode,
	interaction_sequence, paginate_pipeline, pipeline, reading,
	reparse_sequence, web_layout,
};
fn main() {
	let mut args = std::env::args().skip(1);
	let target = args.next().expect("usage: pipeline_replay TARGET FILE...");
	pipeline::warmup();
	let started = std::time::Instant::now();
	let mut count = 0;
	for path in args {
		let input = std::fs::read(&path).unwrap();
		eprintln!("replay {target}: {path}");
		match target.as_str() {
			"reading_pipeline" => reading::check(&input),
			"webpage_layout" => web_layout::check(&input),
			"layout_diff" => history::check(&input),
			"highlight_schedule" => highlight_schedule::check(&input),
			"image_decode" => image_decode::check(&input),
			"reparse" => {
				reparse_sequence::check(&input);
			}
			"paginate_pipeline" => paginate_pipeline::check(&input),
			"interaction_sequence" => interaction_sequence::check(&input),
			"font_archive" => font_archive::check(&input),
			_ => panic!("unknown pipeline {target}"),
		}
		count += 1;
	}
	eprintln!("{count} inputs completed in {:?}", started.elapsed());
	eprintln!("[mvfuzz coverage] {}", coverage::report());
}
