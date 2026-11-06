use mvfuzz::{paginate_pipeline, pipeline};

#[test]
fn real_layouts_split_into_ordered_page_fragments() {
	pipeline::warmup();
	let md = b"# Heading\n\nfirst paragraph with enough words to wrap across a line and then some more text.\n\n> quoted words\n> on two lines\n\n| left | right |\n| --- | --- |\n| one | two |\n\n<details>\n<summary>summary</summary>\n\nhidden body text\n\n</details>\n\n- first\n- second\n- third\n\n```rust\nfn main() {}\n```\n";
	for selector in [0u8, 1, 2, 3, 4] {
		let mut input = vec![selector, selector];
		input.extend(md);
		paginate_pipeline::check(&input);
	}
}
