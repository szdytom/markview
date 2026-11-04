//! Resident-memory probe for the retired pipeline on the current model.
#[path = "webpage_bench/replay.rs"]
mod old_replay;
#[path = "webpage_bench/rss.rs"]
mod rss;
fn main() {
	rss::measure(
		|html| {
			let old = old_replay::extract(html, "https://example.org/article")
				.unwrap();
			(markview_commonmark::parse(old.markdown), old.title)
		},
		|(parse, _)| &parse.document,
	);
}
