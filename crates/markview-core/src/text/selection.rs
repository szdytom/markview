//! Reading identity and logical selection ranges.
use crate::layout::LayoutSnapshot;
use unicode_segmentation::UnicodeSegmentation;

use super::{Affinity, TextPosition, TextSelection, changed_span, word_range};
impl LayoutSnapshot {
	pub fn same_reading_text(&self, other: &Self) -> bool {
		self.blocks.len() == other.blocks.len()
			&& self.blocks.iter().zip(&other.blocks).all(|(a, b)| {
				std::sync::Arc::ptr_eq(&a.layout, &b.layout)
					|| (a.layout.text.len() == b.layout.text.len()
						&& a.layout.text.iter().zip(&b.layout.text).all(
							|(a, b)| {
								a.text == b.text && a.separator == b.separator
							},
						))
			})
	}

	/// Preserve a selection across presentation text changes only when the
	/// selected text itself survives. Stable node slots keep nested cells aligned.
	pub fn rebase_selection(
		&self,
		next: &Self,
		selection: TextSelection,
		old_revision: u64,
		revision: u64,
	) -> Option<TextSelection> {
		if selection.anchor.revision != old_revision
			|| selection.focus.revision != old_revision
		{
			return None;
		}
		let (a, b) = selection.ordered();
		for bi in a.block..=b.block {
			let old = self.blocks.get(bi)?;
			let new = next.blocks.get(bi)?;
			if old.id != new.id
				|| old.layout.text.len() != new.layout.text.len()
			{
				return None;
			}
			for (ni, (old, new)) in
				old.layout.text.iter().zip(&new.layout.text).enumerate()
			{
				if (bi, ni) < (a.block, a.node)
					|| (bi, ni) > (b.block, b.node)
					|| old.text == new.text
				{
					continue;
				}
				let (prefix, end, _) = changed_span(&old.text, &new.text);
				let start = if (bi, ni) == (a.block, a.node) {
					a.offset
				} else {
					0
				};
				let stop = if (bi, ni) == (b.block, b.node) {
					b.offset
				} else {
					old.text.len()
				};
				if start < end && stop > prefix
					|| (prefix == end && start < prefix && stop > prefix)
				{
					return None;
				}
			}
		}
		let position = |mut p: TextPosition| -> Option<TextPosition> {
			let old = &self.blocks.get(p.block)?.layout.text.get(p.node)?.text;
			let new = &next.blocks.get(p.block)?.layout.text.get(p.node)?.text;
			if old != new {
				let (prefix, end, new_end) = changed_span(old, new);
				if prefix == end && p.offset == prefix {
					if p.key() == a.key() {
						p.offset = new_end;
					}
				} else if p.offset >= end {
					p.offset = p.offset - end + new_end;
				} else if p.offset > prefix {
					return None;
				}
			}
			if !new.is_char_boundary(p.offset) {
				return None;
			}
			p.revision = revision;
			Some(p)
		};
		Some(TextSelection {
			anchor: position(selection.anchor)?,
			focus: position(selection.focus)?,
		})
	}
	/// Select the word nearest a text hit-test position, or the whole atomic
	/// box the position falls in. Uses dictionary segmentation, so
	/// double-click selects a Chinese or Japanese word rather than a single
	/// character.
	pub fn select_word_at(
		&self,
		position: TextPosition,
	) -> Option<TextSelection> {
		let node = self
			.blocks
			.get(position.block)?
			.layout
			.text
			.get(position.node)?;
		// A formula or a drawn image is one box whose source is not what is
		// displayed, so a double click takes all of it rather than a word of
		// the LaTeX or the `alt` text it copies. Its two edges only belong to
		// it from the inside: `After` at its start means the text before it,
		// and `Before` at its end means the text after it.
		if let Some(cluster) = node.clusters.iter().find(|c| {
			c.atomic
				&& ((position.offset > c.range.start
					&& position.offset < c.range.end)
					|| (position.offset == c.range.start
						&& position.affinity == Affinity::Before)
					|| (position.offset == c.range.end
						&& position.affinity == Affinity::After))
		}) {
			return Some(TextSelection {
				anchor: TextPosition {
					offset: cluster.range.start,
					affinity: Affinity::Before,
					..position
				},
				focus: TextPosition {
					offset: cluster.range.end,
					affinity: Affinity::After,
					..position
				},
			});
		}
		let text = node.text.as_str();
		let offset = position.offset.min(text.len());
		// A hit reports the boundary before or after the grapheme under the
		// pointer; recover that grapheme so both halves of a character agree.
		let clicked = match position.affinity {
			Affinity::Before => {
				let tail = text.get(offset..)?;
				let end = tail
					.graphemes(true)
					.next()
					.map_or(offset, |g| offset + g.len());
				offset..end
			}
			Affinity::After => {
				let head = text.get(..offset)?;
				let start = head
					.graphemes(true)
					.next_back()
					.map_or(offset, |g| offset - g.len());
				start..offset
			}
		};
		let range = word_range(text, clicked)?;
		let start = node.grapheme_floor(range.start);
		let end = node.grapheme_ceil(range.end);
		if start >= end {
			return None;
		}
		Some(TextSelection {
			anchor: TextPosition {
				offset: start,
				affinity: Affinity::Before,
				..position
			},
			focus: TextPosition {
				offset: end,
				affinity: Affinity::After,
				..position
			},
		})
	}

	/// Select all text belonging to the laid-out block under a text hit.
	pub fn select_block_at(
		&self,
		position: TextPosition,
	) -> Option<TextSelection> {
		let block = self.blocks.get(position.block)?;
		let first = block
			.layout
			.text
			.iter()
			.enumerate()
			.find(|(_, node)| !node.text.is_empty())?;
		let last = block
			.layout
			.text
			.iter()
			.enumerate()
			.rev()
			.find(|(_, node)| !node.text.is_empty())?;
		Some(TextSelection {
			anchor: TextPosition {
				block: position.block,
				node: first.0,
				offset: 0,
				affinity: Affinity::Before,
				..position
			},
			focus: TextPosition {
				block: position.block,
				node: last.0,
				offset: last.1.text.len(),
				affinity: Affinity::After,
				..position
			},
		})
	}

	pub fn select_all(&self, revision: u64) -> Option<TextSelection> {
		let positions: Vec<_> = self
			.blocks
			.iter()
			.enumerate()
			.flat_map(|(bi, b)| {
				b.layout
					.text
					.iter()
					.enumerate()
					.filter(|(_, n)| !n.text.is_empty())
					.map(move |(ni, n)| (bi, ni, n.text.len()))
			})
			.collect();
		let &(b, n, _) = positions.first()?;
		let &(eb, en, len) = positions.last()?;
		Some(TextSelection {
			anchor: TextPosition {
				revision,
				block: b,
				node: n,
				offset: 0,
				affinity: Affinity::Before,
			},
			focus: TextPosition {
				revision,
				block: eb,
				node: en,
				offset: len,
				affinity: Affinity::After,
			},
		})
	}
}
