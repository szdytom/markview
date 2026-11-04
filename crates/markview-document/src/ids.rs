//! Stable identities carried by semantic document nodes.
//!
//! A `BlockId` is allocated from the producer's monotonic counter, so two
//! blocks with equal content still differ, and an incremental parse can keep
//! the ids of blocks it did not touch. A `FieldId` is the field's position in
//! its top-level block's own traversal. Front ends allocate `LinkOccurrenceId`s explicitly; the document model
//! does not interpret source coordinates.
use std::fmt;

macro_rules! nonzero_id {
	($(#[$meta:meta])* $name:ident) => {
		$(#[$meta])*
		#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
		pub struct $name(std::num::NonZeroU32);

		impl $name {
			/// The identity `value` names; `None` for the reserved zero.
			pub fn new(value: u32) -> Option<Self> {
				std::num::NonZeroU32::new(value).map(Self)
			}

			pub fn get(self) -> u32 {
				self.0.get()
			}
		}

		impl fmt::Display for $name {
			fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
				self.0.fmt(f)
			}
		}
	};
}

nonzero_id! {
	/// One node occurrence inside a document.
	///
	/// A block's identity is not its bytes: two equal paragraphs get different
	/// ids, so interactions never leak from one occurrence to another.
	BlockId
}

nonzero_id! {
	/// One link occurrence, shared by every run and open scope that carries it.
	///
	/// Adjacent links keep separate identities even when their addresses match.
	/// The address itself is not part of the identity.
	LinkOccurrenceId
}

/// A semantic text field inside one top-level block's subtree.
///
/// Fields are numbered in the document's own depth-first reading order, the
/// same order search, source mapping and layout use. A field's projection is
/// defined by [`crate::plain_text`] (or the code text itself).
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct FieldId(pub(crate) u32);

impl FieldId {
	pub fn get(self) -> u32 {
		self.0
	}

	pub fn from_index(index: u32) -> Self {
		Self(index)
	}
}

impl fmt::Display for FieldId {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		self.0.fmt(f)
	}
}

/// The semantic version a document currently describes.
///
/// Theme, width and reflow never change it; a new parse of changed input does.
#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct DocumentRevision(pub(crate) u64);

impl DocumentRevision {
	pub fn new(value: u64) -> Self {
		Self(value.max(1))
	}

	/// The next revision no document in this process has used.
	///
	/// Every front end allocates here, so a Markdown parse and an HTML lowering
	/// never share a revision and incremental caches keyed on it stay apart.
	pub fn fresh() -> Self {
		use std::sync::atomic::{AtomicU64, Ordering};
		static NEXT: AtomicU64 = AtomicU64::new(1);
		Self(NEXT.fetch_add(1, Ordering::Relaxed))
	}

	pub fn get(self) -> u64 {
		self.0
	}
}

/// An in-process digest of semantic content, used only to reuse geometry and
/// highlight results for equal reading content.
///
/// It is not a persisted position: a session stores the input fingerprint,
/// front-end version and structural path instead. It is also not the input
/// identity or a permission token, and equal keys do not prove collision-free
/// identity.
#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq)]
pub struct ContentKey(pub(crate) u64);

impl ContentKey {
	pub fn get(self) -> u64 {
		self.0
	}
}

/// Where a zero-width semantic position sits relative to a field offset.
#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq)]
pub enum Affinity {
	/// Before the character at the offset.
	#[default]
	Before,
	/// After the character at the offset.
	After,
}
