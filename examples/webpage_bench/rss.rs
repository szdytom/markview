//! One pipeline, one input and one live result, without report code.
#[path = "fonts.rs"]
mod fonts;
use markview_document::Document;
use markview_layout::layout::LayoutEngine;
use std::{
	sync::atomic::{AtomicUsize, Ordering},
	time::Duration,
};
static LIVE: AtomicUsize = AtomicUsize::new(0);
struct Iteration;
impl Drop for Iteration {
	fn drop(&mut self) {
		assert_eq!(LIVE.swap(0, Ordering::Relaxed), 1);
	}
}
pub fn measure<T>(
	produce: impl Fn(&str) -> T,
	document: impl Fn(&T) -> &Document,
) {
	let input = std::env::args_os().nth(1).unwrap();
	let html = std::fs::read_to_string(input).unwrap();
	let options = fonts::layout_options();
	for _ in 0..4 {
		assert_eq!(LIVE.swap(1, Ordering::Relaxed), 0);
		let _iteration = Iteration;
		let produced = produce(&html);
		let mut engine = LayoutEngine::new();
		let mut layout = engine.begin_layout(
			document(&produced),
			&options,
			&Default::default(),
		);
		while !layout.is_complete() {
			engine.advance(
				&mut layout,
				document(&produced),
				Duration::from_secs(60),
			);
		}
		std::hint::black_box((&produced, layout.snapshot()));
	}
	let status = std::fs::read_to_string("/proc/self/status").unwrap();
	println!(
		"{}",
		status
			.lines()
			.find_map(|l| l.strip_prefix("VmHWM:"))
			.unwrap()
			.split_whitespace()
			.next()
			.unwrap()
	);
}
