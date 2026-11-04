//! Heading anchors shared by every document front end.
//!
//! A Markdown heading uses GitHub's slug: lowercased text with punctuation
//! dropped, spaces turned into hyphens, and repeats suffixed. Raw HTML may
//! declare its own `id`; an explicit id is reserved first and later automatic
//! slugs avoid it, while duplicate explicit ids keep the first occurrence.
use std::collections::{HashMap, HashSet};

/// Per-document anchor uniqueness, assigned in reading order.
#[derive(Default)]
pub struct Anchors {
	used: HashSet<String>,
	/// The next suffix to try for a base slug, so repeated headings do not
	/// restart the scan.
	next: HashMap<String, u32>,
}

impl Anchors {
	/// Reserves an explicit id; later slugs avoid it.
	pub fn reserve(&mut self, anchor: String) {
		self.used.insert(anchor);
	}

	/// Whether `anchor` is already taken.
	pub fn contains(&self, anchor: &str) -> bool {
		self.used.contains(anchor)
	}

	/// The anchor for one heading.
	pub fn unique(&mut self, text: &str) -> String {
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
