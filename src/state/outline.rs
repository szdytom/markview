//! Per-document collapse state; row indices always address the original outline.
use crate::document::OutlineEntry;
use std::collections::BTreeSet;

#[derive(Default, Clone)]
pub(crate) struct OutlineTree {
	collapsed: BTreeSet<usize>,
}

impl OutlineTree {
	pub(crate) fn has_children(entries: &[OutlineEntry], index: usize) -> bool {
		entries
			.get(index)
			.zip(entries.get(index + 1))
			.is_some_and(|(entry, next)| next.level > entry.level)
	}

	pub(crate) fn is_collapsed(&self, index: usize) -> bool {
		self.collapsed.contains(&index)
	}

	pub(crate) fn toggle(&mut self, index: usize) {
		if !self.collapsed.remove(&index) {
			self.collapsed.insert(index);
		}
	}

	pub(crate) fn rows(&self, entries: &[OutlineEntry]) -> Vec<usize> {
		let mut hidden_below = None;
		entries
			.iter()
			.enumerate()
			.filter_map(|(index, entry)| {
				if hidden_below.is_some_and(|level| entry.level > level) {
					return None;
				}
				hidden_below = self.is_collapsed(index).then_some(entry.level);
				Some(index)
			})
			.collect()
	}
}
