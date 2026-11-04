//! Legacy URL spelling for presentation and external fragment compatibility.
//! Internal navigation uses typed anchors and targets.

/// The legacy fragment for a footnote definition.
pub fn anchor(label: &str) -> String {
	format!("fn:{label}")
}

/// The legacy fragment for a footnote definition's return link.
pub fn back(label: &str) -> String {
	format!("fnback:{label}")
}

/// The legacy fragment for a footnote's first reference.
pub fn reference(label: &str) -> String {
	format!("fnref:{label}")
}

/// The label an anchor names, if it is a footnote anchor.
pub fn label(fragment: &str) -> Option<&str> {
	fragment.strip_prefix("fn:")
}

/// The label a reference anchor names, if it is a footnote reference.
pub fn reference_label(fragment: &str) -> Option<&str> {
	fragment.strip_prefix("fnref:")
}

/// The label a back fragment names, if it is a footnote back fragment.
pub fn back_label(fragment: &str) -> Option<&str> {
	fragment.strip_prefix("fnback:")
}

/// Whether a legacy fragment spells a definition or reference target.
pub fn is_anchor(fragment: &str) -> bool {
	label(fragment).is_some() || reference_label(fragment).is_some()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn each_direction_has_its_own_unambiguous_prefix() {
		assert_eq!(anchor("1"), "fn:1");
		assert_eq!(reference("1"), "fnref:1");
		assert_eq!(label("fn:1"), Some("1"));
		assert_eq!(label("fnref:1"), None);
		assert_eq!(label("fnback:1"), None);
		assert_eq!(reference_label("fnref:1"), Some("1"));
		assert_eq!(reference_label("fn:1"), None);
		assert_eq!(back_label("fnback:1"), Some("1"));
		assert_eq!(back_label("fn:1"), None);
		assert_eq!(label("section"), None);
		assert!(is_anchor("fn:1"));
		assert!(is_anchor("fnref:1"));
		assert!(!is_anchor("fnback:1"));
		assert!(!is_anchor("section"));
	}
}
