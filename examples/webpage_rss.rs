//! Resident-memory probe for direct webpage lowering.
#[path = "webpage_bench/rss.rs"]
mod rss;
fn main() {
	rss::measure(
		|html| {
			markview_webpage::extract(html, "https://example.org/article")
				.unwrap()
		},
		|article| &article.document,
	);
}
