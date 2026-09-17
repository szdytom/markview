//! The fragment grammar footnote links use.
//!
//! A reference jumps to `fn:<label>`, the anchor its definition registers. A
//! definition's number jumps to `fnback:<label>`, which the reader resolves to
//! the reference it was opened from, or to the first reference's
//! `fnref:<label>` anchor when the footnote was reached by scrolling. The
//! reader owns the `fnback:` answer, so every prefix lives here.

/// The anchor a footnote definition registers.
pub fn anchor(label: &str) -> String {
	format!("fn:{label}")
}

/// The anchor a footnote's references register; the first one wins.
pub fn reference(label: &str) -> String {
	format!("fnref:{label}")
}

/// The fragment a footnote reference follows.
pub fn url(label: &str) -> String {
	format!("#{}", anchor(label))
}

/// The fragment a definition's number follows.
pub fn back_url(label: &str) -> String {
	format!("#fnback:{label}")
}

/// The label an anchor names, if it is a footnote anchor.
pub fn label(fragment: &str) -> Option<&str> {
	fragment.strip_prefix("fn:")
}

/// The label a back fragment names, if it is a footnote back fragment.
pub fn back_label(fragment: &str) -> Option<&str> {
	fragment.strip_prefix("fnback:")
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn each_direction_has_its_own_unambiguous_prefix() {
		assert_eq!(anchor("1"), "fn:1");
		assert_eq!(reference("1"), "fnref:1");
		assert_eq!(url("1"), "#fn:1");
		assert_eq!(back_url("1"), "#fnback:1");
		assert_eq!(label("fn:1"), Some("1"));
		assert_eq!(label("fnref:1"), None);
		assert_eq!(label("fnback:1"), None);
		assert_eq!(back_label("fnback:1"), Some("1"));
		assert_eq!(back_label("fn:1"), None);
		assert_eq!(label("section"), None);
	}
}
