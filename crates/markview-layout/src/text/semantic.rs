//! Public reading identities resolved against the current geometry snapshot.
use super::{
	Affinity, GeometryPosition, GeometrySelection, SemanticPosition,
	text_position,
};
use crate::{
	layout::{LayoutSnapshot, Rect},
	search::SearchMatch,
};
use std::{cmp::Ordering, collections::HashMap, ops::Range};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextSelection {
	pub anchor: SemanticPosition,
	pub focus: SemanticPosition,
}
impl TextSelection {
	pub fn is_empty(self) -> bool {
		self.anchor.revision == self.focus.revision
			&& self.anchor.block == self.focus.block
			&& self.anchor.field == self.focus.field
			&& self.anchor.offset == self.focus.offset
			&& self.anchor.projection == self.focus.projection
	}
	pub fn ordered(
		self,
		snapshot: &LayoutSnapshot,
	) -> Option<(SemanticPosition, SemanticPosition)> {
		Some(
			if snapshot.compare_positions(self.anchor, self.focus)?.is_gt() {
				(self.focus, self.anchor)
			} else {
				(self.anchor, self.focus)
			},
		)
	}
	fn resolve(self, snapshot: &LayoutSnapshot) -> Option<GeometrySelection> {
		Some(GeometrySelection {
			anchor: text_position(snapshot, self.anchor)?,
			focus: text_position(snapshot, self.focus)?,
		})
	}
}
impl GeometrySelection {
	fn semantic(self, snapshot: &LayoutSnapshot) -> Option<TextSelection> {
		Some(TextSelection {
			anchor: self.anchor.semantic(snapshot)?,
			focus: self.focus.semantic(snapshot)?,
		})
	}
}

/// A transient query result borrowed from one snapshot. Its indices are valid
/// only in `snapshot`; retain [`SemanticPosition`] across events or reflow.
pub struct ResolvedPosition<'a> {
	pub block: usize,
	pub node: usize,
	pub offset: usize,
	pub snapshot: &'a LayoutSnapshot,
}
impl LayoutSnapshot {
	/// Adapts a reading-text query inside this snapshot to a stable identity.
	pub fn semantic_at(
		&self,
		block: usize,
		node: usize,
		offset: usize,
		affinity: Affinity,
	) -> Option<SemanticPosition> {
		GeometryPosition {
			revision: self.revision.get(),
			block,
			node,
			offset,
			affinity,
		}
		.semantic(self)
	}
	pub fn resolve_position(
		&self,
		position: SemanticPosition,
	) -> Option<ResolvedPosition<'_>> {
		let p = text_position(self, position)?;
		Some(ResolvedPosition {
			block: p.block,
			node: p.node,
			offset: p.offset,
			snapshot: self,
		})
	}
	/// Reading order follows placed document fields, never allocated `BlockId`s.
	pub fn compare_positions(
		&self,
		a: SemanticPosition,
		b: SemanticPosition,
	) -> Option<Ordering> {
		Some(text_position(self, a)?.cmp_reading(text_position(self, b)?))
	}
	pub fn contains_selection(&self, selection: TextSelection) -> bool {
		selection.resolve(self).is_some()
	}
	pub fn hit_test_text(
		&self,
		x: f32,
		y: f32,
		horizontal: &HashMap<(usize, usize), f32>,
		_presentation_revision: u64,
	) -> Option<SemanticPosition> {
		self.geometry_hit_test_text(x, y, horizontal, self.revision.get())?
			.semantic(self)
	}
	pub fn select_word_at(
		&self,
		position: SemanticPosition,
	) -> Option<TextSelection> {
		self.geometry_select_word_at(text_position(self, position)?)?
			.semantic(self)
	}
	pub fn select_block_at(
		&self,
		position: SemanticPosition,
	) -> Option<TextSelection> {
		self.geometry_select_block_at(text_position(self, position)?)?
			.semantic(self)
	}
	pub fn select_all(
		&self,
		_presentation_revision: u64,
	) -> Option<TextSelection> {
		self.geometry_select_all(self.revision.get())?
			.semantic(self)
	}
	pub fn extract_text(
		&self,
		selection: TextSelection,
		_presentation_revision: u64,
	) -> String {
		selection.resolve(self).map_or_else(String::new, |s| {
			self.geometry_extract_text(s, self.revision.get())
		})
	}
	pub fn selection_rects(
		&self,
		selection: TextSelection,
		horizontal: &HashMap<(usize, usize), f32>,
		presentation_revision: u64,
	) -> Vec<Rect> {
		self.selection_rects_in(
			selection,
			horizontal,
			presentation_revision,
			f32::NEG_INFINITY..f32::INFINITY,
		)
	}
	pub fn selection_rects_in(
		&self,
		selection: TextSelection,
		horizontal: &HashMap<(usize, usize), f32>,
		_presentation_revision: u64,
		visible: Range<f32>,
	) -> Vec<Rect> {
		selection.resolve(self).map_or_else(Vec::new, |s| {
			self.geometry_selection_rects_in(
				s,
				horizontal,
				self.revision.get(),
				visible,
			)
		})
	}
	pub fn search_selection(
		&self,
		hit: &SearchMatch,
		_presentation_revision: u64,
	) -> Option<TextSelection> {
		self.geometry_search_selection(hit, self.revision.get())?
			.semantic(self)
	}
	/// Explicitly maps surviving reading text between semantic revisions. Pure
	/// reflow resolves the same semantic endpoints against the new snapshot.
	pub fn rebase_selection(
		&self,
		next: &Self,
		selection: TextSelection,
		_old_presentation_revision: u64,
		_presentation_revision: u64,
	) -> Option<TextSelection> {
		if self.revision == next.revision && self.same_reading_text(next) {
			return next.contains_selection(selection).then_some(selection);
		}
		self.geometry_rebase_selection(
			next,
			selection.resolve(self)?,
			self.revision.get(),
			next.revision.get(),
		)?
		.semantic(next)
	}
}
