//! Per-document state and transient read-only interaction state.
use crate::{
	document,
	layout::{LayoutOptions, LayoutSnapshot},
};
use markview_core::text::{TextCounts, TextSelection};
use std::{
	collections::HashMap,
	path::PathBuf,
	sync::Arc,
	time::{Duration, Instant},
};
use winit::keyboard::ModifiersState;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
	Open,
	Smaller,
	Larger,
	Narrower,
	Wider,
	Align,
	Hyphens,
	/// First-line paragraph indent in whole em units.
	Indent(u8),
	CjkType(markview_core::style::CjkType),
	Settings,
	Reset,
	OpenConfig,
	SystemTheme,
	Styles,
	StyleToggle(usize),
	StyleUp(usize),
	StyleDown(usize),
	StylePrev,
	StyleNext,
	StylesFolder,
	SelectTab(usize),
	CloseTab(usize),
	/// Dismiss the local-file confirmation without opening anything.
	ModalDismiss,
	/// Open the directory containing the file the confirmation names.
	ModalOpenFolder,
	/// Hand the confirmed local file to the operating system.
	ModalConfirm,
	/// Hide the remote-image banner, keeping the current fetch limit.
	RemoteDismiss,
	/// Lift the remote-image limit for the current document revision.
	RemoteLoadAll,
}

/// A blocking question awaiting the reader's answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Modal {
	/// A local file whose type is not on the inert allowlist.
	OpenLocal {
		path: PathBuf,
		/// The file's directory, for the "open folder" action.
		dir: PathBuf,
		/// The open document's directory, for a shorter relative display.
		document_dir: Option<PathBuf>,
	},
}

#[derive(Default, Clone)]
pub(crate) struct ReaderSession {
	pub(crate) counts: TextCounts,
	pub(crate) path: Option<PathBuf>,
	pub(crate) snapshot: LayoutSnapshot,
	pub(crate) accepted_revision: u64,
	pub(crate) accepted_content_id: u64,
	pub(crate) version: u64,
	pub(crate) content_version: u64,
	pub(crate) document: Option<Arc<document::Document>>,
	pub(crate) requested_options: Option<LayoutOptions>,
	pub(crate) scroll: f32,
	pub(crate) horizontal: HashMap<(usize, usize), f32>,
	pub(crate) follow_update: bool,
	pub(crate) layout_pending: bool,
	pub(crate) pending_scroll: Option<f32>,
	/// A heading anchor waiting for its heading to be laid out.
	pub(crate) pending_anchor: Option<String>,
	pub(crate) select_all_pending: bool,
	pub(crate) displayed_version: u64,
	pub(crate) snapshot_complete: bool,
	/// Remote image sources the loader left unrequested past the cap.
	pub(crate) remote_deferred: usize,
	/// Whether this tab's reader lifted the remote-image cap for this content.
	/// It lives with the session, so it is per tab and per revision.
	pub(crate) load_all_images: bool,
	/// Whether this tab's reader hid the remote-image notice for this content.
	/// It must live with the session too: every freshly opened tab starts at
	/// revision 1, so a shared flag would suppress the notice in new documents.
	pub(crate) remote_notice_dismissed: bool,
}

impl ReaderSession {
	/// The deferred count while the notice strip is worth showing.
	pub(crate) fn remote_notice(&self) -> Option<usize> {
		(self.remote_deferred > 0 && !self.remote_notice_dismissed)
			.then_some(self.remote_deferred)
	}
}

pub(crate) struct ReaderTab {
	pub(crate) path: PathBuf,
	pub(crate) session: ReaderSession,
	pub(crate) last_active: Instant,
}

impl ReaderTab {
	pub(crate) fn new(path: PathBuf) -> Self {
		Self {
			path,
			session: ReaderSession::default(),
			last_active: Instant::now(),
		}
	}
}

#[derive(Default)]
pub(crate) struct InteractionState {
	pub(crate) selection_counts: Option<(TextSelection, TextCounts)>,
	pub(crate) panel_open: bool,
	pub(crate) styles_open: bool,
	pub(crate) selection: Option<TextSelection>,
	pub(crate) pointer_down: Option<Drag>,
	pub(crate) dragged: bool,
	pub(crate) drag_at: Option<Instant>,
	pub(crate) modifiers: ModifiersState,
	pub(crate) cursor: (f32, f32),
	pub(crate) hover: Option<String>,
	pub(crate) hover_image: Option<String>,
	/// The wide block whose horizontal scrollbar the pointer is over.
	pub(crate) hover_overflow: Option<(usize, usize)>,
	pub(crate) focus: Option<Command>,
	pub(crate) pressed: Option<Command>,
	pub(crate) scrollbar: Option<ScrollbarDrag>,
	pub(crate) last_click: Option<(Instant, (f32, f32), u8)>,
	/// A pending local-file confirmation; while it is set it owns input.
	pub(crate) modal: Option<Modal>,
}

/// Which scrollbar a press grabbed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScrollbarAxis {
	/// The document's vertical scrollbar.
	Document,
	/// The horizontal scrollbar of one overflowing block.
	Overflow { block: usize, overflow: usize },
}

/// An in-flight scrollbar drag: the grabbed bar and how far inside its thumb
/// the pointer grabbed it, so the thumb never jumps under the pointer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScrollbarDrag {
	pub(crate) target: ScrollbarAxis,
	pub(crate) grab: f32,
}

/// Selection unit of an in-flight press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Grain {
	Char,
	Word,
	Block,
}

/// An in-flight press: where it started and the link it would activate.
#[derive(Clone, Debug)]
pub(crate) struct Drag {
	pub(crate) start: (f32, f32),
	pub(crate) link: Option<String>,
	pub(crate) grain: Grain,
	/// The word or block a multi-click press selected, kept as the drag base.
	pub(crate) base: Option<TextSelection>,
}

/// Extends a multi-click base selection to the word or block under the pointer,
/// keeping the base as the fixed edge.
fn extend(
	base: TextSelection,
	unit: TextSelection,
	position: markview_core::text::TextPosition,
) -> TextSelection {
	let (first, last) = base.ordered();
	if position.cmp_reading(first).is_lt() {
		TextSelection {
			anchor: last,
			focus: unit.anchor,
		}
	} else if position.cmp_reading(last).is_gt() {
		TextSelection {
			anchor: first,
			focus: unit.focus,
		}
	} else {
		base
	}
}

impl InteractionState {
	pub(crate) fn reset_clicks(&mut self) {
		self.last_click = None;
	}

	pub(crate) fn click_count(&mut self, now: Instant) -> u8 {
		const CLICK_INTERVAL: Duration = Duration::from_millis(500);
		const CLICK_DISTANCE: f32 = 6.0;
		let count = match self.last_click {
			Some((at, point, count))
				if now.duration_since(at) <= CLICK_INTERVAL
					&& (self.cursor.0 - point.0)
						.hypot(self.cursor.1 - point.1)
						<= CLICK_DISTANCE =>
			{
				count % 3 + 1
			}
			_ => 1,
		};
		self.last_click = Some((now, self.cursor, count));
		count
	}

	pub(crate) fn begin_selection(
		&mut self,
		position: markview_core::text::TextPosition,
		link: Option<String>,
	) {
		let anchor = if self.modifiers.shift_key() {
			self.selection.map_or(position, |s| s.anchor)
		} else {
			position
		};
		self.selection = Some(TextSelection {
			anchor,
			focus: position,
		});
		self.pointer_down = Some(Drag {
			start: self.cursor,
			link,
			grain: Grain::Char,
			base: None,
		});
		self.dragged = self.modifiers.shift_key();
		self.focus = None;
	}
	/// Starts a press that already selected a word or block, so dragging
	/// extends the selection by that unit instead of by grapheme. Returns
	/// false when there is no selection to start from.
	pub(crate) fn begin_grain_selection(
		&mut self,
		selection: Option<TextSelection>,
		grain: Grain,
	) -> bool {
		let Some(selection) = selection else {
			return false;
		};
		self.selection = Some(selection);
		self.pointer_down = Some(Drag {
			start: self.cursor,
			link: None,
			grain,
			base: Some(selection),
		});
		self.dragged = false;
		self.focus = None;
		true
	}
	pub(crate) fn move_selection(
		&mut self,
		position: Option<markview_core::text::TextPosition>,
		snapshot: &LayoutSnapshot,
	) {
		let Some(drag) = &self.pointer_down else {
			return;
		};
		let (start, grain, base) = (drag.start, drag.grain, drag.base);
		self.dragged |=
			(self.cursor.0 - start.0).hypot(self.cursor.1 - start.1) >= 4.0;
		if !self.dragged {
			return;
		}
		let Some(position) = position else {
			return;
		};
		let Some(selection) = &mut self.selection else {
			return;
		};
		match (grain, base) {
			(Grain::Char, _) | (_, None) => selection.focus = position,
			(Grain::Word, Some(base)) => {
				if let Some(word) = snapshot.select_word_at(position) {
					*selection = extend(base, word, position);
				}
			}
			(Grain::Block, Some(base)) => {
				if let Some(block) = snapshot.select_block_at(position) {
					*selection = extend(base, block, position);
				}
			}
		}
	}
	pub(crate) fn finish_selection(
		&mut self,
		release_link: Option<&str>,
	) -> Option<String> {
		self.drag_at = None;
		let drag = self.pointer_down.take()?;
		drag.link
			.filter(|link| !self.dragged && release_link == Some(link.as_str()))
	}
	pub(crate) fn clear_selection(&mut self) {
		self.selection = None;
		self.pointer_down = None;
		self.drag_at = None;
		self.scrollbar = None;
	}
}

/// The furthest a document of `height` scrolls in `viewport`: its last line
/// can be lifted to one third of a page below the top, leaving the other two
/// thirds blank, and a document that already ends higher does not scroll.
pub(crate) fn scroll_limit(height: f32, viewport: f32) -> f32 {
	(height - viewport / 3.0).max(0.0)
}

impl ReaderSession {
	pub(crate) fn extends_prefix(
		&self,
		reader: &crate::worker::ReaderSnapshot,
	) -> bool {
		!self.snapshot_complete
			&& self.accepted_content_id == reader.document.content_id
			&& self.snapshot.blocks.len() <= reader.layout.blocks.len()
			&& self
				.snapshot
				.blocks
				.iter()
				.zip(&reader.layout.blocks)
				.all(|(a, b)| a.y == b.y && Arc::ptr_eq(&a.layout, &b.layout))
	}
	pub(crate) fn can_display(
		&self,
		reader: &crate::worker::ReaderSnapshot,
		viewport: f32,
	) -> bool {
		if reader.complete {
			return true;
		}
		if self.snapshot.blocks.is_empty() {
			return reader.layout.height >= self.scroll + viewport;
		}
		let index = self
			.snapshot
			.blocks
			.partition_point(|b| b.y <= self.scroll)
			.saturating_sub(1);
		let anchor = &self.snapshot.blocks[index];
		let occurrence = self.snapshot.blocks[..index]
			.iter()
			.filter(|b| b.id == anchor.id)
			.count();
		reader
			.layout
			.blocks
			.iter()
			.filter(|b| b.id == anchor.id)
			.nth(occurrence)
			.is_some_and(|b| {
				b.y + (self.scroll - anchor.y).min(b.layout.height) + viewport
					<= reader.layout.height
			})
	}
	pub(crate) fn scroll_by(&mut self, dy: f32, viewport: f32) {
		if dy == 0. {
			self.pending_scroll.get_or_insert(self.scroll);
			self.resolve_scroll(viewport);
			return;
		}
		// A deliberate scroll abandons an anchor that was still waiting for
		// its heading to be laid out.
		self.pending_anchor = None;
		self.follow_update = false;
		let base = self
			.pending_scroll
			.filter(|v| v.is_finite())
			.unwrap_or(self.scroll);
		let target = (base + dy).max(0.0);
		self.pending_scroll = Some(target);
		self.resolve_scroll(viewport);
	}
	pub(crate) fn resolve_scroll(&mut self, viewport: f32) {
		if let Some(target) = self.pending_scroll {
			let max = (self.snapshot.height - viewport).max(0.0);
			if !self.layout_pending || target <= max {
				self.scroll =
					target.min(scroll_limit(self.snapshot.height, viewport));
				self.pending_scroll = None;
			}
		}
	}
	pub(crate) fn coverage(&self, viewport: f32) -> f32 {
		self.pending_scroll.unwrap_or(self.scroll) + viewport * 1.5
	}
	pub(crate) fn release_heavy(&mut self) {
		self.counts = TextCounts::default();
		self.snapshot = LayoutSnapshot::default();
		self.snapshot_complete = false;
		self.document = None;
		self.requested_options = None;
		self.pending_anchor = None;
	}

	/// Scrolls to a queued heading anchor against the current snapshot.
	///
	/// `None` means the heading has not been laid out yet and the anchor stays
	/// queued; `Some(Ok(()))` means the scroll offset moved; `Some(Err(anchor))`
	/// means the complete layout has no such heading.
	pub(crate) fn resolve_anchor(
		&mut self,
		viewport: f32,
	) -> Option<Result<(), String>> {
		let anchor = self.pending_anchor.clone()?;
		if let Some(y) = self.snapshot.anchor_y(&anchor) {
			self.pending_anchor = None;
			self.pending_scroll = None;
			self.follow_update = false;
			self.scroll =
				y.clamp(0.0, (self.snapshot.height - viewport).max(0.0));
			return Some(Ok(()));
		}
		if self.snapshot_complete {
			self.pending_anchor = None;
			return Some(Err(anchor));
		}
		None
	}

	pub(crate) fn accept(
		&mut self,
		reader: crate::worker::ReaderSnapshot,
		viewport: f32,
	) -> bool {
		// A metadata-only change re-reads identical bytes; only a real content
		// change may invalidate reading positions.
		let changed = reader.document.content_id != self.accepted_content_id;
		if reader.complete
			&& (changed
				|| self.layout_pending
				|| !self.snapshot.same_reading_text(&reader.layout))
		{
			self.counts = reader
				.layout
				.select_all(reader.content_version)
				.map(|selection| {
					TextCounts::of(
						&reader
							.layout
							.extract_text(selection, reader.content_version),
					)
				})
				.unwrap_or_default();
		}
		let extending = self.extends_prefix(&reader);
		self.scroll = if extending {
			self.scroll
		} else if self.snapshot.blocks.is_empty() {
			// A released tab has no old layout to anchor against, but its
			// scroll position is still user state and should survive reloading.
			// A prefix keeps the content limit, because a position inside the
			// blank would exceed what `can_display` accepts.
			let limit = if reader.complete {
				scroll_limit(reader.layout.height, viewport)
			} else {
				(reader.layout.height - viewport).max(0.0)
			};
			self.scroll.clamp(0.0, limit)
		} else {
			crate::layout::anchored_scroll(
				&self.snapshot,
				&reader.layout,
				self.scroll,
				viewport,
				self.follow_update,
			)
		};
		self.accepted_content_id = reader.document.content_id;
		self.document = Some(reader.document);
		self.snapshot = reader.layout;
		self.layout_pending = !reader.complete;
		self.snapshot_complete = reader.complete;
		self.remote_deferred = reader.remote_deferred;
		self.resolve_scroll(viewport);
		self.accepted_revision = reader.content_version;
		self.follow_update = false;
		self.horizontal.retain(|(bi, oi), offset| {
			if changed {
				return false;
			}
			if let Some(o) = self
				.snapshot
				.blocks
				.get(*bi)
				.and_then(|b| b.layout.overflow.get(*oi))
			{
				*offset =
					offset.clamp(0., (o.content_width - o.rect.w).max(0.));
				true
			} else {
				false
			}
		});
		changed
	}
}

#[cfg(test)]
mod tests;
