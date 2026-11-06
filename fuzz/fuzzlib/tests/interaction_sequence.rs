use mvfuzz::{interaction_sequence, pipeline};

#[test]
fn real_snapshots_survive_selection_scroll_pan_and_overflow() {
	pipeline::warmup();
	let md = b"# Heading\n\nalpha beta gamma with several words to select and drag across the line.\n\n[link](https://example.org)\n\n| a | b | c | d | e | f | g | h |\n| --- | --- | --- | --- | --- | --- | --- | --- |\n| one | two | three | four | five | six | seven | eight |\n\n<details>\n<summary>summary</summary>\n\nhidden alpha\n\n</details>\n";
	for controls in [
		[0u8; 12],
		[60, 7, 1, 9, 0, 2, 3, 4, 5, 6, 1, 0],
		[30, 3, 2, 20, 1, 1, 255, 128, 64, 32, 2, 1],
	] {
		let mut input = b"INT1".to_vec();
		input.extend(controls);
		input.extend(md);
		interaction_sequence::check(&input);
	}
	interaction_sequence::check(md);
}
