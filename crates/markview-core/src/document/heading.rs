//! Anchors for headings, matching the slugs GitHub gives a rendered heading.
//!
//! A link such as `#getting-started` or `other.md#getting-started` names a
//! heading by its anchor: the heading text lowercased, punctuation and symbols
//! dropped, ASCII spaces replaced by hyphens, and a repeated heading suffixed
//! `-1`, `-2`, and so on in document order.

use std::collections::{HashMap, HashSet};

/// Per-document anchor uniqueness, assigned in reading order.
#[derive(Default)]
pub(crate) struct Anchors {
	used: HashSet<String>,
	/// The next suffix to try for a base slug. A document may repeat one
	/// heading many times, and without this the suffix scan would restart at
	/// one every time, making anchor assignment quadratic.
	next: HashMap<String, u32>,
}

impl Anchors {
	/// The anchor for one heading.
	pub(crate) fn unique(&mut self, text: &str) -> String {
		let base = heading_slug(text);
		if self.used.insert(base.clone()) {
			return base;
		}
		let next = self.next.entry(base.clone()).or_insert(1);
		loop {
			let anchor = format!("{base}-{next}");
			*next += 1;
			if self.used.insert(anchor.clone()) {
				return anchor;
			}
		}
	}
}

/// The slug of one heading. Letters and digits survive in any script; `-` and
/// `_` are kept; an ASCII space becomes a hyphen; everything else is dropped.
pub fn heading_slug(text: &str) -> String {
	let mut slug = String::with_capacity(text.len());
	for c in text.to_lowercase().chars() {
		match c {
			' ' => slug.push('-'),
			'-' | '_' => slug.push(c),
			_ if c.is_alphanumeric() => slug.push(c),
			_ => {}
		}
	}
	slug
}
