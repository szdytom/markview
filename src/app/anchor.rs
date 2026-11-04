//! Fragment links: heading anchors and footnotes inside the reader and across
//! documents.
//!
//! A link may name a heading with a fragment. `#section` moves inside the
//! current document; `other.md#section` opens that document and then moves.
//! Because layout is progressive, the target heading may not exist yet, so the
//! fragment is queued on the session until its heading is laid out. Footnote
//! references and a note's number use the same fragments; the number's
//! `fnback:` fragment is answered here from where the reader jumped from.
use super::App;
use markview_document as document;
use markview_layout::footnote;
use std::time::Instant;

/// A link's document part, without its fragment.
pub(super) fn link_target(link: &str) -> &str {
	link.split_once('#').map_or(link, |(target, _)| target)
}

/// A link's percent-decoded fragment, when it has a non-empty one.
pub(super) fn link_fragment(link: &str) -> Option<String> {
	let (_, fragment) = link.split_once('#')?;
	if fragment.is_empty() {
		return None;
	}
	Some(
		percent_encoding::percent_decode_str(fragment)
			.decode_utf8_lossy()
			.into_owned(),
	)
}

/// Whether a link is a footnote jump, which has no external target to show.
pub(super) fn footnote_link(link: &str) -> bool {
	link_target(link).is_empty()
		&& link_fragment(link).is_some_and(|fragment| {
			footnote::label(&fragment).is_some()
				|| footnote::back_label(&fragment).is_some()
		})
}

impl<P: super::SendEvent> App<P> {
	/// Queues an HTML fragment and applies it as soon as it is laid out.
	pub(super) fn goto_anchor(&mut self, anchor: String) {
		self.goto_target(document::AnchorId::Fragment(anchor));
	}

	/// Queues a typed document target and applies it as soon as it is laid out.
	pub(super) fn goto_target(&mut self, target: document::AnchorId) {
		self.readers.session.saved_reading = None;
		// A jump is direct input, so any easing for the previous destination
		// ends here.
		self.readers.session.cancel_scroll_animation();
		// Remember where the reader was, so a footnote's number can return.
		self.readers.session.jump_origin =
			Some((target.clone(), self.readers.session.scrolling.offset));
		// A jump inside a settled document belongs to this revision. A jump
		// queued while another parse or layout is in flight must wait for
		// whatever revision that work publishes.
		let session = &self.readers.session;
		let settled = session.accepted_revision == session.content_version
			&& session.snapshot_complete
			&& !session.layout_pending;
		let revision = settled.then_some(session.snapshot.revision);
		self.readers.session.pending_anchor =
			Some(crate::state::PendingAnchor { target, revision });
		self.apply_anchor();
	}

	/// Returns a footnote's number to the reference that opened it, or to the
	/// first reference when the note was reached by scrolling.
	pub(super) fn return_from_footnote(&mut self, id: &document::FootnoteId) {
		if let Some(scroll) = self.readers.session.footnote_return(id) {
			self.readers.session.pending_anchor = None;
			self.readers.session.scrolling.target = None;
			self.readers.session.follow_update = false;
			let to = scroll.clamp(
				0.0,
				self.readers.session.snapshot.scroll_limit(self.viewport()),
			);
			if (to - self.readers.session.scrolling.offset).abs() > 0.5 {
				self.readers.session.animate_scroll_to(to, Instant::now());
			} else {
				self.readers.session.scrolling.offset = to;
			}
			self.status.clear();
			self.after_scroll();
			return;
		}
		// No reference to return to: fall back to the first one.
		let first =
			self.readers
				.session
				.snapshot
				.blocks
				.iter()
				.find_map(|block| {
					block.anchor_positions().find_map(
						|(anchor, _)| match anchor {
							document::AnchorId::FootnoteReference {
								definition,
								occurrence,
							} if definition == id => Some(document::AnchorId::FootnoteReference {
								definition: definition.clone(),
								occurrence: *occurrence,
							}),
							_ => None,
						},
					)
				});
		if let Some(first) = first {
			self.goto_target(first);
		} else {
			self.status.set(
				self.preferences
					.values
					.lang()
					.status_heading_not_found(footnote::reference(id.as_str())),
				true,
				4,
			);
			self.redraw();
		}
	}

	/// Applies a queued anchor, reporting a heading the finished layout lacks.
	pub(super) fn apply_anchor(&mut self) {
		// A reload keeps the previous revision visible until new content arrives.
		if self.readers.session.accepted_revision
			!= self.readers.session.content_version
		{
			return;
		}
		if !self.readers.session.pending_anchor_is_current() {
			self.readers.session.pending_anchor = None;
			return;
		}
		// Reveal the retained target before resolving its visible position.
		if let Some(anchor) = self
			.readers
			.session
			.pending_anchor
			.as_ref()
			.map(|pending| pending.target.clone())
			&& self.readers.session.open_enclosing_details(&anchor)
		{
			self.present_disclosures();
		}
		let before = self.readers.session.scrolling.offset;
		let Some(result) = self.readers.session.resolve_anchor(self.viewport())
		else {
			return;
		};
		match result {
			Ok(()) => {
				// A resolved anchor lands where it was queued; ease there from
				// where the reader was rather than snapping.
				let to = self.readers.session.scrolling.offset;
				if (to - before).abs() > 0.5 {
					self.readers.session.scrolling.offset = before;
					self.readers.session.animate_scroll_to(to, Instant::now());
				}
				self.status.clear();
				self.after_scroll();
			}
			Err(anchor) => {
				let name = match anchor {
					document::AnchorId::Fragment(fragment) => fragment,
					document::AnchorId::FootnoteDefinition(id) => {
						footnote::anchor(id.as_str())
					}
					document::AnchorId::FootnoteReference {
						definition,
						..
					} => footnote::reference(definition.as_str()),
				};
				self.status.set(
					self.preferences
						.values
						.lang()
						.status_heading_not_found(name),
					true,
					4,
				);
			}
		}
		self.redraw();
	}
}

#[cfg(test)]
mod tests;
