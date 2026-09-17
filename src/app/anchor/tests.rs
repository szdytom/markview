use super::{footnote_link, link_fragment, link_target};

#[test]
fn a_link_splits_into_its_document_and_decoded_fragment() {
	assert_eq!(link_target("other.md#getting-started"), "other.md");
	assert_eq!(
		link_target("https://example.com/a#b"),
		"https://example.com/a"
	);
	assert_eq!(link_target("#section"), "");
	assert_eq!(link_target("other.md"), "other.md");
	assert_eq!(link_fragment("other.md"), None);
	assert_eq!(link_fragment("#"), None);
	assert_eq!(link_fragment("#section"), Some("section".into()));
	assert_eq!(link_fragment("#中文"), Some("中文".into()));
	assert_eq!(link_fragment("other.md#a%20b"), Some("a b".into()));
	assert_eq!(
		link_fragment("file:///tmp/a.md#getting-started"),
		Some("getting-started".into())
	);
}

#[test]
fn only_footnote_jumps_are_hidden_from_the_footer() {
	assert!(footnote_link("#fn:1"));
	assert!(footnote_link("#fnback:1"));
	assert!(!footnote_link("#section"));
	assert!(!footnote_link("https://example.com#fn:1"));
	assert!(!footnote_link("other.md#fn:1"));
}
