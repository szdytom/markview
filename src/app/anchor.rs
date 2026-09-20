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
use markview_core::document::footnote;
use std::time::{Duration, Instant};

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

impl App {
	/// Queues an anchor and applies it as soon as it is laid out.
	pub(super) fn goto_anchor(&mut self, anchor: String) {
		// A jump is direct input, so any easing for the previous destination
		// ends here.
		self.readers.session.cancel_scroll_animation();
		// Remember where the reader was, so a footnote's number can return.
		self.readers.session.jump_origin =
			Some((anchor.clone(), self.readers.session.scroll));
		self.readers.session.pending_anchor = Some(anchor);
		self.apply_anchor();
	}

	/// Returns a footnote's number to the reference that opened it, or to the
	/// first reference when the note was reached by scrolling.
	pub(super) fn return_from_footnote(&mut self, label: &str) {
		if let Some(scroll) = self.readers.session.footnote_return(label) {
			self.readers.session.pending_anchor = None;
			self.readers.session.pending_scroll = None;
			self.readers.session.follow_update = false;
			let to = scroll.clamp(
				0.0,
				crate::state::scroll_limit(
					self.readers.session.snapshot.height,
					self.viewport(),
				),
			);
			if (to - self.readers.session.scroll).abs() > 0.5 {
				self.readers.session.animate_scroll_to(to, Instant::now());
			} else {
				self.readers.session.scroll = to;
			}
			self.error = false;
			self.status.clear();
			self.status_until = None;
			self.worker
				.prioritize(self.readers.session.coverage(self.viewport()));
			self.refresh_hover();
			self.redraw();
			return;
		}
		// No reference to return to: fall back to the first one.
		self.goto_anchor(footnote::reference(label));
	}

	/// Applies a queued anchor, reporting a heading the finished layout lacks.
	pub(super) fn apply_anchor(&mut self) {
		// A heading or footnote inside a collapsed `<details>` is never laid
		// out, so the jump first expands the disclosures framing it and lets
		// the next layout resolve the anchor, exactly as clicking each summary
		// would. Opening them changes the layout options, so the reflow is
		// requested here and the anchor stays queued until it arrives.
		if let Some(anchor) = self.readers.session.pending_anchor.clone()
			&& self.readers.session.open_enclosing_details(&anchor)
		{
			self.request(false);
			return;
		}
		let before = self.readers.session.scroll;
		let Some(result) = self.readers.session.resolve_anchor(self.viewport())
		else {
			return;
		};
		match result {
			Ok(()) => {
				// A resolved anchor lands where it was queued; ease there from
				// where the reader was rather than snapping.
				let to = self.readers.session.scroll;
				if (to - before).abs() > 0.5 {
					self.readers.session.scroll = before;
					self.readers.session.animate_scroll_to(to, Instant::now());
				}
				self.error = false;
				self.status.clear();
				self.status_until = None;
				self.worker
					.prioritize(self.readers.session.coverage(self.viewport()));
				self.refresh_hover();
			}
			Err(anchor) => {
				self.error = true;
				self.status = format!("Heading not found: #{anchor}");
				self.status_until =
					Some(Instant::now() + Duration::from_secs(4));
			}
		}
		self.redraw();
	}
}

#[cfg(test)]
mod tests;
