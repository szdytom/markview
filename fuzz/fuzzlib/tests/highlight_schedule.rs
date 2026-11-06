use mvfuzz::{highlight_schedule, pipeline};

#[test]
fn controlled_queues_converge_after_document_and_theme_switches() {
	pipeline::warmup();
	for controls in [
		[0, 0, 0, 0, 0, 0, 0, 0],
		[1, 3, 0, 4, 5, 2, 6, 7],
		[0, 1, 2, 3, 4, 5, 6, 7],
		[0, 0, 1, 4, 5, 7, 3, 2],
	] {
		let mut input = b"HLP1".to_vec();
		input.extend(controls);
		input.extend(b"fn main() { let value = 1; }\n");
		highlight_schedule::check(&input);
	}
}
