//! Publication bookkeeping and the cached selection length: pure state that
//! any front end shares, kept out of the `wasm32` gate so native tests cover
//! it directly.

use crate::selection::Pointer;
use markview_core::layout::LayoutSnapshot;
use markview_selection::Host;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::Arc;

/// The snapshot the canvas last drew, and the revision it was accepted at.
#[derive(Default)]
pub(crate) struct Published {
	pub(crate) snapshot: LayoutSnapshot,
	pub(crate) revision: u64,
	/// The source of the document `snapshot` describes, and the pass that laid
	/// it out. Together they say whether a prefix merely adds blocks to what is
	/// on screen: the same document laid out for a new column width does not.
	pub(crate) source: Option<Arc<str>>,
	pub(crate) pass: Option<u64>,
}
impl Published {
	/// Reflow retained geometry and invalidate published coordinate caches.
	pub(crate) fn present_disclosures(
		&mut self,
		open: &BTreeMap<u64, bool>,
		force_open: bool,
		pointer: &mut Pointer,
	) {
		self.snapshot.set_disclosures(open, force_open);
		let revision = self.revision.wrapping_add(1);
		pointer.retag(self.revision, revision);
		self.revision = revision;
		self.pass = None;
	}

	/// A same-document prefix must cover the selection and held drag base
	/// before replacement can rebase them onto it.
	pub(crate) fn covers_interaction(
		&self,
		source: &Arc<str>,
		prefix: &LayoutSnapshot,
		pointer: &Pointer,
	) -> bool {
		if !self
			.source
			.as_ref()
			.is_some_and(|old| Arc::ptr_eq(old, source))
		{
			return true;
		}
		[
			pointer.selection(),
			pointer.drag().and_then(|drag| drag.base),
		]
		.into_iter()
		.flatten()
		.all(|selection| {
			selection.anchor.block.max(selection.focus.block)
				< prefix.blocks.len()
		})
	}

	/// Accepts `next` as a whole new snapshot, moving the selection onto it
	/// while it still reads the same text and clearing it otherwise. `pass` is
	/// the resumable pass that produced it, or `None` for a snapshot no pass
	/// can extend.
	pub(crate) fn accept(
		&mut self,
		next: LayoutSnapshot,
		source: Arc<str>,
		pass: Option<u64>,
		pointer: &mut Pointer,
	) {
		let revision = self.revision.wrapping_add(1);
		let previous = std::mem::replace(&mut self.snapshot, next);
		let selection = pointer.selection().and_then(|selection| {
			previous.rebase_selection(
				&self.snapshot,
				selection,
				self.revision,
				revision,
			)
		});
		pointer.set_selection(selection);
		pointer.rebase_drag(&previous, &self.snapshot, self.revision, revision);
		self.revision = revision;
		self.source = Some(source);
		self.pass = pass;
	}

	/// Appends the blocks `prefix` added since the last publication, so a
	/// budgeted step pays for the blocks it reached rather than for the whole
	/// prefix again. Returns false when `prefix` does not continue what is
	/// published, which leaves the caller to accept it whole.
	pub(crate) fn extend(
		&mut self,
		prefix: &LayoutSnapshot,
		pointer: &mut Pointer,
	) -> bool {
		let published = self.snapshot.blocks.len();
		if prefix.blocks.len() < published {
			return false;
		}
		self.snapshot
			.blocks
			.extend_from_slice(&prefix.blocks[published..]);
		self.snapshot.height = prefix.height;
		self.snapshot.presentation_key = prefix.presentation_key;
		self.snapshot.document_box = prefix.document_box.clone();
		self.snapshot.width = prefix.width;
		self.snapshot.reused = prefix.reused;
		self.snapshot.degraded = prefix.degraded;
		self.snapshot.math_errors = prefix.math_errors;
		self.snapshot.images = prefix.images.clone();
		let revision = self.revision.wrapping_add(1);
		// Only blocks were added, so every reading position still means what it
		// meant and the gesture in flight keeps extending from the same base.
		pointer.retag(self.revision, revision);
		self.revision = revision;
		true
	}

	/// Whether `pass` over `source` merely adds blocks to what is published.
	pub(crate) fn continues(&self, source: &Arc<str>, pass: u64) -> bool {
		self.pass == Some(pass)
			&& self
				.source
				.as_ref()
				.is_some_and(|published| Arc::ptr_eq(published, source))
	}
}

/// The UTF-16 length of the selected text, or `None` after a change.
/// Reading the stats runs at least once per frame, while extracting the
/// selection scans every block and allocates the whole text, so the length is
/// extracted at most once between changes.
#[derive(Default)]
pub(crate) struct SelectionLength {
	chars: Cell<Option<usize>>,
	/// How many times the text has been extracted. Tests read this to assert
	/// the cache answers repeated reads without extracting again.
	extractions: Cell<u64>,
}
impl SelectionLength {
	/// The cached length, extracting it once when it is missing. An empty
	/// selection is not cached: clearing must invalidate, and a fresh
	/// selection has to wait for its first read.
	pub(crate) fn get(&self, extract: impl FnOnce() -> String) -> usize {
		if let Some(length) = self.chars.get() {
			return length;
		}
		let text = extract();
		self.extractions.set(self.extractions.get() + 1);
		let length = text.encode_utf16().count();
		self.chars.set(Some(length));
		length
	}

	/// Drops the cached count after the selection or the snapshot it was
	/// extracted from changes.
	pub(crate) fn forget(&self) {
		self.chars.set(None);
	}

	/// How many times the text has been extracted so far.
	#[cfg(test)]
	pub(crate) fn extractions(&self) -> u64 {
		self.extractions.get()
	}
}

#[cfg(test)]
mod tests;
