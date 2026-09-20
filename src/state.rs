//! Per-document state and transient read-only interaction state.
use crate::{
	document,
	layout::{LayoutOptions, LayoutSnapshot},
};
use markview_core::text::{TextCounts, TextSelection};
use std::{
	collections::{BTreeMap, HashMap},
	path::PathBuf,
	sync::Arc,
	time::{Duration, Instant},
};
use winit::{event::TouchPhase, keyboard::ModifiersState};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
	Open,
	Smaller,
	Larger,
	Narrower,
	Wider,
	Align,
	Hyphens,
	CodeWrap,
	/// Ease discrete scroll requests over time.
	SmoothScroll,
	/// Step the reader's scroll-speed multiplier by whole steps.
	ScrollSpeed(i8),
	/// First-line paragraph indent in whole em units.
	Indent(u8),
	CjkType(markview_core::style::CjkType),
	/// Open or close the export panel.
	Export,
	ExportFormat(crate::settings::ExportFormat),
	/// Step the export's text size by whole pixels.
	ExportSize(i8),
	/// First-line indent preset, in em units.
	ExportIndent(u8),
	/// Paper preset index.
	ExportPaper(u8),
	/// Paper orientation; `true` is landscape.
	ExportOrientation(bool),
	/// Margin preset index.
	ExportMargin(u8),
	/// PNG scale preset index.
	ExportScale(u8),
	/// Export once, then keep re-exporting whenever the document changes.
	ExportAndWatch,
	/// Show the export's stylesheet chooser in place of the export panel.
	ExportStyles,
	ExportStyleToggle(usize),
	ExportStyleUp(usize),
	ExportStyleDown(usize),
	ExportStylePrev,
	ExportStyleNext,
	/// Write the document with the current export settings.
	ExportRun,
	Settings,
	SettingsPreview,
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
	/// Fetch the font files the shown stylesheets declare.
	FontsDownload,
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
	/// Open or close the table-of-contents drawer.
	Outline,
	/// Scroll the document to the heading of one outline entry.
	OutlineGoto(usize),
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
	/// The outline built for `accepted_content_id`, cached so a frame or an
	/// event never walks the document again. It is built on first demand.
	pub(crate) outline: Option<(u64, Arc<[document::OutlineEntry]>)>,
	pub(crate) requested_options: Option<LayoutOptions>,
	/// Reader-chosen `<details>` collapse state, keyed by block id, overriding
	/// what the source declared. It is layout input, and a reload drops it.
	pub(crate) details_open: Arc<BTreeMap<u64, bool>>,
	pub(crate) scroll: f32,
	pub(crate) horizontal: HashMap<(usize, usize), f32>,
	pub(crate) follow_update: bool,
	pub(crate) layout_pending: bool,
	pub(crate) pending_scroll: Option<f32>,
	/// An eased scroll in flight. Its destination is `pending_scroll`, so the
	/// worker still prioritizes what the animation is heading for.
	pub(crate) scroll_animation: Option<ScrollAnimation>,
	/// A heading anchor waiting for its heading to be laid out.
	pub(crate) pending_anchor: Option<String>,
	/// The internal fragment the reader last jumped to, with the scroll offset
	/// it left, so a footnote's number can return to its reference.
	pub(crate) jump_origin: Option<(String, f32)>,
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

	/// The scroll offset a footnote's number returns to, when the reader
	/// jumped there from one of its references.
	pub(crate) fn footnote_return(&self, label: &str) -> Option<f32> {
		let (fragment, scroll) = self.jump_origin.as_ref()?;
		(document::footnote::label(fragment) == Some(label)).then_some(*scroll)
	}

	/// The cached outline. Empty until the drawer first asks for it.
	pub(crate) fn outline_entries(&self) -> &[document::OutlineEntry] {
		self.outline
			.as_ref()
			.map_or(&[], |(_, entries)| entries.as_ref())
	}

	/// Builds the outline once per accepted document, on first demand.
	pub(crate) fn ensure_outline(&mut self) {
		if self
			.outline
			.as_ref()
			.is_some_and(|(id, _)| *id == self.accepted_content_id)
		{
			return;
		}
		let mut entries = Vec::new();
		if let Some(document) = &self.document {
			entries = document.outline();
		}
		self.outline = Some((self.accepted_content_id, entries.into()));
	}

	/// The anchor an outline entry addresses, as a fragment link would name it.
	pub(crate) fn outline_anchor(&self, index: usize) -> Option<&str> {
		self.outline_entries()
			.get(index)
			.map(|entry| entry.anchor.as_str())
	}

	/// The outline entry whose heading contains the reading position: the last
	/// heading at or above the top of the viewport, or the first heading while
	/// the reader is still above every one of them.
	///
	/// Both the outline and the laid-out heading anchors are in reading order,
	/// so one walk over the snapshot's anchors is enough. A heading this prefix
	/// has not laid out (or one inside a collapsed `<details>`) cannot match a
	/// later anchor, so the pointer may pass it.
	pub(crate) fn current_outline(&self) -> Option<usize> {
		let outline = self.outline_entries();
		if outline.is_empty() {
			return None;
		}
		let mut index = 0;
		let mut current = None;
		'blocks: for block in &self.snapshot.blocks {
			for anchor in &block.layout.anchors {
				// A footnote definition and a reference both register layout
				// anchors. Neither is in the outline, so treating one as a
				// heading would advance the scan past every later entry.
				if document::footnote::is_anchor(&anchor.anchor) {
					continue;
				}
				while index < outline.len()
					&& outline[index].anchor != anchor.anchor
				{
					index += 1;
				}
				if index >= outline.len()
					|| block.y + anchor.y > self.scroll + 0.5
				{
					break 'blocks;
				}
				current = Some(index);
				index += 1;
			}
		}
		current.or(Some(0))
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
	pub(crate) settings_scroll: f32,
	pub(crate) settings_preview: bool,
	pub(crate) export_scroll: f32,
	/// Offset from the centre of the panel scrollbar thumb while dragging.
	pub(crate) panel_grab: Option<f32>,
	pub(crate) styles_open: bool,
	/// The export page of the panel. It implies `panel_open`.
	pub(crate) export_open: bool,
	/// The export's stylesheet chooser, drawn in place of the export panel.
	pub(crate) export_styles_open: bool,
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
	/// Keyboard navigation shows a focus outline; pointer activation does not.
	pub(crate) focus_visible: bool,
	pub(crate) pressed: Option<Command>,
	pub(crate) scrollbar: Option<ScrollbarDrag>,
	pub(crate) last_click: Option<(Instant, (f32, f32), u8)>,
	/// A pending local-file confirmation; while it is set it owns input.
	pub(crate) modal: Option<Modal>,
	/// The axis of the wheel gesture in flight.
	pub(crate) wheel: WheelGesture,
	/// The outline drawer is open. It is an overlay, not a modal panel: the
	/// document keeps scrolling and selecting behind it.
	pub(crate) outline_open: bool,
	/// The drawer's own list offset.
	pub(crate) outline_scroll: f32,
	/// The entry the drawer's keyboard selection is on.
	pub(crate) outline_selection: Option<usize>,
}

/// Which way a wheel gesture travels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WheelAxis {
	/// Pan the wide block under the pointer sideways.
	Horizontal,
	/// Scroll the document.
	Vertical,
}

/// What to do with one wheel event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum WheelStep {
	/// Nothing to apply yet: either the gesture has not travelled far enough
	/// to have a direction, in which case the motion is held and arrives with
	/// the deciding event, or the event carried no motion at all.
	Pending,
	/// Travel this far on this axis. Both components are reported so that a
	/// horizontal gesture with no block under the pointer can still scroll.
	Travel(WheelAxis, f32, f32),
}

/// How far a gesture travels before its direction is decided.
const WHEEL_DECISION: f32 = 6.0;
/// A pause this long starts a new gesture on platforms that never report one.
const WHEEL_GAP: Duration = Duration::from_millis(150);

/// Decides a wheel gesture's axis once, from its first few moments, and holds
/// it until the gesture ends, and inherits nothing from the one before it.
///
/// Deciding per event instead makes a diagonal gesture stutter: the events that
/// lean sideways pan the block under the pointer, and when no block is there
/// they do nothing at all, so the page stops following the hand. A reported
/// boundary separates gestures where the platform reports one, and the pause
/// between events is the fallback for the platforms that never do.
#[derive(Debug, Default)]
pub(crate) struct WheelGesture {
	axis: Option<WheelAxis>,
	/// Motion held back until the direction is unambiguous.
	held: (f32, f32),
	last: Option<Instant>,
	/// True while the platform's own gesture is in flight. A pause inside one
	/// is a slow moment, not a boundary; where no gesture is reported, the
	/// pause is the only boundary there is.
	reported: bool,
}

impl WheelGesture {
	/// Feeds one wheel delta in logical pixels. `horizontal_only` is the
	/// explicit sideways request of Shift+wheel, which skips the wait.
	pub(crate) fn feed(
		&mut self,
		dx: f32,
		dy: f32,
		now: Instant,
		horizontal_only: bool,
		phase: TouchPhase,
	) -> WheelStep {
		let starts = matches!(phase, TouchPhase::Started);
		let ends = matches!(phase, TouchPhase::Ended | TouchPhase::Cancelled);
		// A reported start, or a pause outside a reported gesture, begins a new
		// gesture. Two gestures can follow each other faster than the pause,
		// and then only the reported boundary tells them apart.
		if starts
			|| (!self.reported
				&& self
					.last
					.is_some_and(|last| now.duration_since(last) > WHEEL_GAP))
		{
			self.axis = None;
			// Motion the finished gesture never travelled to is its own; the
			// next gesture decides what to do from its own first moments.
			self.held = (0.0, 0.0);
		}
		// A start without its end, as when a gesture is cut off by the window
		// losing focus, must not turn every later pause into a slow moment.
		self.reported = (self.reported || starts) && !ends;
		self.last = Some(now);
		if horizontal_only {
			self.axis = Some(WheelAxis::Horizontal);
			self.held = (0.0, 0.0);
		}
		self.held.0 += dx;
		self.held.1 += dy;
		let axis = match self.axis {
			Some(axis) => axis,
			None => {
				if self.held.0.abs().max(self.held.1.abs()) < WHEEL_DECISION {
					if ends {
						self.held = (0.0, 0.0);
					}
					return WheelStep::Pending;
				}
				if self.held.0.abs() > self.held.1.abs() {
					WheelAxis::Horizontal
				} else {
					WheelAxis::Vertical
				}
			}
		};
		let (held_x, held_y) = std::mem::take(&mut self.held);
		// The event that ends a gesture still belongs to it.
		self.axis = (!ends).then_some(axis);
		if held_x == 0.0 && held_y == 0.0 {
			return WheelStep::Pending;
		}
		WheelStep::Travel(axis, held_x, held_y)
	}
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
	/// Activate only when the release still targets the pressed control.
	pub(crate) fn release_button(
		&mut self,
		hovered: Option<Command>,
	) -> Option<Command> {
		self.pressed
			.take()
			.filter(|command| Some(*command) == hovered)
	}

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
	/// Starts a press that has no text under it, so only a link-like target
	/// can activate on release. A `<details>` marker is such a target.
	pub(crate) fn begin_link_press(&mut self, link: String) {
		self.pointer_down = Some(Drag {
			start: self.cursor,
			link: Some(link),
			grain: Grain::Char,
			base: None,
		});
		self.dragged = false;
		self.focus = None;
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

	/// Toggles the outline drawer. Opening puts the keyboard selection on the
	/// heading at the reading position, so Up/Down and Enter work at once.
	/// Returns whether the drawer is now open.
	pub(crate) fn toggle_outline(
		&mut self,
		entries: usize,
		current: Option<usize>,
	) -> bool {
		self.outline_open = !self.outline_open;
		if self.outline_open {
			self.focus = None;
			self.outline_scroll = 0.0;
			self.outline_selection =
				(entries > 0).then_some(current.unwrap_or(0));
		} else {
			self.outline_selection = None;
		}
		self.outline_open
	}

	/// Closes the drawer, as Escape and the toolbar toggle do.
	pub(crate) fn close_outline(&mut self) {
		self.outline_open = false;
		self.outline_selection = None;
	}

	/// Whether the drawer answers input.
	///
	/// The drawer is an overlay, not a panel, so it stands down while a panel
	/// or a confirmation owns input: both draw over it, and the panel's
	/// scrollbar drag and outside-click dismissal must keep working where they
	/// overlap it.
	pub(crate) fn outline_owns_input(&self) -> bool {
		self.outline_open && !self.panel_open && self.modal.is_none()
	}

	/// Moves the drawer's selection by `delta` entries, clamped to the
	/// outline. Returns whether there is an entry to move to.
	pub(crate) fn move_outline(
		&mut self,
		delta: isize,
		entries: usize,
	) -> bool {
		if entries == 0 {
			self.outline_selection = None;
			self.focus = None;
			return false;
		}
		let base = self.outline_selection.unwrap_or(0) as isize;
		let next = (base + delta).clamp(0, entries as isize - 1) as usize;
		self.outline_selection = Some(next);
		// The visible selection is what Enter activates, so button focus has
		// to follow it; a row clicked before the move must not outrank it.
		self.focus = Some(Command::OutlineGoto(next));
		true
	}

	/// The command Enter activates: the focused button while it is still on
	/// screen, otherwise the drawer's selected entry. Moving the selection
	/// keeps focus on it, so the focused button and the selection agree. The
	/// drawer only answers while no panel or confirmation owns input, so Enter
	/// never reaches the document behind one.
	pub(crate) fn enter_action(
		&self,
		mut visible: impl Iterator<Item = Command>,
	) -> Option<Command> {
		if let Some(focus) = self.focus
			&& visible.any(|action| action == focus)
		{
			return Some(focus);
		}
		self.outline_owns_input()
			.then_some(self.outline_selection)
			.flatten()
			.map(Command::OutlineGoto)
	}

	/// Scrolls the drawer's own list by `delta`, clamped to `max`.
	pub(crate) fn scroll_outline(&mut self, delta: f32, max: f32) {
		self.outline_scroll =
			(self.outline_scroll + delta).clamp(0.0, max.max(0.0));
	}

	/// Advances keyboard focus to the next (or previous) button, wrapping at
	/// the ends. Returns the focused action, or `None` when there is nothing
	/// to focus.
	///
	/// A row the drawer marks as selected is what Enter activates when no
	/// button has focus, so focusing an entry row moves the visible selection
	/// with it; otherwise Tab would leave the marker on another heading.
	pub(crate) fn tab_focus(
		&mut self,
		buttons: &[Command],
		backward: bool,
	) -> Option<Command> {
		let current = buttons.iter().position(|b| Some(*b) == self.focus);
		let index = match current {
			Some(i) => {
				(i + if backward { buttons.len() - 1 } else { 1 })
					% buttons.len()
			}
			None if backward => buttons.len().checked_sub(1)?,
			None => 0,
		};
		let action = *buttons.get(index)?;
		self.focus = Some(action);
		if let Command::OutlineGoto(row) = action {
			self.outline_selection = Some(row);
		}
		Some(action)
	}
}

/// The furthest a document of `height` scrolls in `viewport`: its last line
/// can be lifted to one third of a page below the top, leaving the other two
/// thirds blank, and a document that already ends higher does not scroll.
pub(crate) fn scroll_limit(height: f32, viewport: f32) -> f32 {
	(height - viewport / 3.0).max(0.0)
}

/// The shortest and longest a discrete scroll may take, and the distance at
/// which it reaches the longest.
const SCROLL_MIN: Duration = Duration::from_millis(120);
const SCROLL_MAX: Duration = Duration::from_millis(400);
const SCROLL_FULL: f32 = 2400.0;
/// How often a running animation asks the event loop for a frame.
const SCROLL_FRAME: Duration = Duration::from_millis(8);

/// Ease-out cubic: fast away from the start and settling into the target.
/// Both ends are exact and the curve is strictly increasing between them.
pub(crate) fn ease_out_cubic(t: f32) -> f32 {
	let remaining = 1.0 - t.clamp(0.0, 1.0);
	1.0 - remaining * remaining * remaining
}

/// A time-driven scroll from one offset to another.
///
/// The offset depends only on elapsed time, so the motion is identical at any
/// frame rate; the duration grows with the distance between a floor and a
/// ceiling, which keeps a one-line step responsive and a whole-page jump calm.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScrollAnimation {
	from: f32,
	to: f32,
	started: Instant,
	duration: Duration,
}

impl ScrollAnimation {
	/// Starts a move to `to`, scaling the duration from `from`.
	pub(crate) fn new(from: f32, to: f32, now: Instant) -> Self {
		let ratio = ((to - from).abs() / SCROLL_FULL).clamp(0.0, 1.0);
		// Integer nanoseconds keep both bounds exact at the ends.
		let span = (SCROLL_MAX - SCROLL_MIN).as_nanos() as f64;
		let nanos = SCROLL_MIN.as_nanos() as f64 + span * f64::from(ratio);
		Self {
			from,
			to,
			started: now,
			duration: Duration::from_nanos(nanos as u64),
		}
	}

	/// The eased offset at `now`, clamped to the two ends.
	pub(crate) fn offset_at(&self, now: Instant) -> f32 {
		let elapsed = now.saturating_duration_since(self.started).as_secs_f32();
		let progress = (elapsed / self.duration.as_secs_f32()).clamp(0.0, 1.0);
		self.from + (self.to - self.from) * ease_out_cubic(progress)
	}

	/// When the last frame is due.
	pub(crate) fn end(&self) -> Instant {
		self.started + self.duration
	}

	pub(crate) fn finished(&self, now: Instant) -> bool {
		now >= self.end()
	}
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
		// Direct input takes over from the offset on screen, so the running
		// animation and the destination it owns must go before the target is
		// measured. A destination a previous direct request left accumulating
		// is not the animation's, and [`Self::cancel_scroll_animation`] keeps
		// it, so progressive layout still adds those requests up.
		self.cancel_scroll_animation();
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

	/// A discrete scroll request, eased from the offset on screen.
	///
	/// It accumulates from the pending destination exactly as [`Self::scroll_by`]
	/// does, so repeated PageDown presses add up even while the geometry they
	/// name is still being laid out.
	pub(crate) fn animate_scroll_by(&mut self, dy: f32, now: Instant) {
		if dy == 0.0 {
			return;
		}
		let base = self
			.pending_scroll
			.filter(|v| v.is_finite())
			.unwrap_or(self.scroll);
		self.animate_scroll_to((base + dy).max(0.0), now);
	}

	/// A wheel travel, eased like a discrete step.
	///
	/// A gesture that continues the motion still pending accumulates on its
	/// destination exactly as [`Self::animate_scroll_by`] does. One that runs
	/// against it instead takes over from the offset on screen, so reversing
	/// the wheel answers the hand at once rather than finishing the old
	/// destination first.
	pub(crate) fn animate_wheel_by(&mut self, dy: f32, now: Instant) {
		if dy == 0.0 {
			return;
		}
		let destination = self
			.pending_scroll
			.filter(|v| v.is_finite())
			.unwrap_or(self.scroll);
		if (destination - self.scroll) * dy < 0.0 {
			self.pending_scroll = None;
		}
		self.animate_scroll_by(dy, now);
	}

	/// Eases the displayed offset to an absolute `target`, retargeting a
	/// running animation from where it currently is rather than snapping.
	pub(crate) fn animate_scroll_to(&mut self, target: f32, now: Instant) {
		self.pending_anchor = None;
		self.follow_update = false;
		self.pending_scroll = Some(target);
		self.scroll_animation =
			Some(ScrollAnimation::new(self.scroll, target, now));
	}

	/// Advances a running animation. The displayed offset is clamped to the
	/// geometry at hand, so a destination the layout has not reached yet never
	/// shows blank space; the pending target then resolves as it arrives.
	/// Returns whether another frame is due.
	pub(crate) fn advance_scroll(
		&mut self,
		now: Instant,
		viewport: f32,
	) -> bool {
		let Some(animation) = self.scroll_animation else {
			return false;
		};
		let ceiling = if self.layout_pending {
			(self.snapshot.height - viewport).max(0.0)
		} else {
			scroll_limit(self.snapshot.height, viewport)
		};
		self.scroll = animation.offset_at(now).clamp(0.0, ceiling);
		// A settled document has nowhere further to go once the clamp is
		// reached, so an animation heading past it ends there instead of
		// waiting out its duration.
		let beyond = animation.to >= ceiling - 0.5;
		let at_end =
			!self.layout_pending && beyond && self.scroll >= ceiling - 0.5;
		if !animation.finished(now) && !at_end {
			return true;
		}
		// A destination the geometry could not reach stays pending, so the
		// existing resolve applies it once the layout grows.
		self.scroll_animation = None;
		self.resolve_scroll(viewport);
		false
	}

	/// Ends a running animation where the reader sees it, without moving.
	///
	/// The animation mirrors its destination into `pending_scroll` so the
	/// worker and progressive layout keep chasing it. Dropping the animation
	/// must drop exactly that mirror, or the cancelled movement would resume
	/// on the next input or layout; a target a direct input set on top of it
	/// differs from the destination and is left alone.
	pub(crate) fn cancel_scroll_animation(&mut self) {
		let Some(animation) = self.scroll_animation.take() else {
			return;
		};
		if self.pending_scroll == Some(animation.to) {
			self.pending_scroll = None;
		}
	}

	pub(crate) fn scroll_animating(&self) -> bool {
		self.scroll_animation.is_some()
	}

	/// When the next animation frame is due, or `None` when nothing is
	/// running, so the event loop can go back to waiting.
	pub(crate) fn scroll_animation_deadline(
		&self,
		now: Instant,
	) -> Option<Instant> {
		self.scroll_animation
			.as_ref()
			.map(|animation| (now + SCROLL_FRAME).min(animation.end()))
	}

	pub(crate) fn resolve_scroll(&mut self, viewport: f32) {
		// While an animation is in flight it owns the displayed offset.
		if self.scroll_animation.is_some() {
			return;
		}
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
		self.outline = None;
		self.requested_options = None;
		self.pending_anchor = None;
		self.jump_origin = None;
		self.scroll_animation = None;
	}

	/// Expands the `<details>` elements enclosing `anchor` and reports whether
	/// any changed.
	///
	/// A heading or footnote inside a collapsed body is never laid out, so a
	/// jump to its anchor must open the disclosures framing it, outermost
	/// first, and wait for the reflow before the anchor can resolve.
	pub(crate) fn open_enclosing_details(&mut self, anchor: &str) -> bool {
		let Some(document) = self.document.clone() else {
			return false;
		};
		let closed: Vec<u64> = document
			.details_enclosing(anchor)
			.into_iter()
			.filter(|id| {
				let declared = document.details_declared(*id).unwrap_or(false);
				!self.details_open.get(id).copied().unwrap_or(declared)
			})
			.collect();
		if closed.is_empty() {
			return false;
		}
		let open = Arc::make_mut(&mut self.details_open);
		for id in closed {
			open.insert(id, true);
		}
		true
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
		counts: Option<TextCounts>,
	) -> bool {
		// A metadata-only change re-reads identical bytes; only a real content
		// change may invalidate reading positions. The reading counts arrive
		// with the update, computed off the event loop.
		let changed = reader.document.content_id != self.accepted_content_id;
		if let Some(counts) = counts {
			self.counts = counts;
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
