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

mod outline;
pub(crate) use outline::OutlineTree;

pub(crate) use markview_core::layout::scroll_limit;
pub(crate) use markview_selection::{
	Drag, Grain, Host, Modifiers, Point, Selection,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
	RetrySettingsLoad,
	SearchCase,
	SearchWord,
	SearchPrevious,
	SearchNext,
	SearchClose,
	FocusInput(TextField),
	Open,
	Smaller,
	Larger,
	Narrower,
	Wider,
	Align,
	Hyphens,
	CodeWrap,
	SingleInstance,
	/// Step the reader's scroll-speed multiplier by whole steps.
	ScrollSpeed(i8),
	/// First-line paragraph indent in whole em units.
	Indent(u8),
	CjkType(markview_core::style::CjkType),
	/// The interface language; `None` follows the system again.
	Language(Option<crate::lang::Lang>),
	/// Pick the family a font role shapes with; `None` restores the
	/// stylesheet's own candidate chain. Selections address the catalogue
	/// generation that supplied the chooser's shared name.
	FontFamily(
		crate::settings::FontRole,
		Option<crate::app::font_panel::Selection>,
	),
	/// Open a control's option list on the option in force, or close it again.
	ToggleDropdown(DropdownId, usize),
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
	/// Write the document with the current export settings.
	ExportRun,
	Settings,
	SettingsPreview,
	CopyDiagnostics,
	OpenProject,
	Reset,
	OpenConfig,
	SystemTheme,
	Styles,
	StyleToggle(usize),
	StyleUp(usize),
	StyleDown(usize),
	StylesFolder,
	/// Show one page of the settings panel.
	SettingsTab(PanelTab),
	Fonts(crate::app::font_panel::Command),
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
	OutlineToggle(usize),
	OutlineExpandAll,
	OutlineCollapseAll,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextField {
	Search,
	ExportTitle,
}

/// A panel has exactly one page; the outline and confirmation remain independent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PanelPage {
	#[default]
	Closed,
	Settings(PanelTab),
	Export,
	ExportStyles,
}

impl InteractionState {
	pub(crate) fn panel_open(&self) -> bool {
		self.panel != PanelPage::Closed
	}

	pub(crate) fn styles_open(&self) -> bool {
		self.panel == PanelPage::Settings(PanelTab::Styles)
	}

	pub(crate) fn fonts_open(&self) -> bool {
		self.panel == PanelPage::Settings(PanelTab::Fonts)
	}

	pub(crate) fn export_open(&self) -> bool {
		matches!(self.panel, PanelPage::Export | PanelPage::ExportStyles)
	}

	pub(crate) fn export_styles_open(&self) -> bool {
		self.panel == PanelPage::ExportStyles
	}

	/// Changes pages without losing the parent form's scroll position.
	pub(crate) fn show_panel(&mut self, page: PanelPage) {
		self.panel = page;
		self.focus = None;
		self.pressed = None;
		self.pointer_down = None;
		self.pressed_image = None;
		self.drag_at = None;
		self.scrollbar = None;
		self.panel_grab = None;
		// An option list belongs to the page that opened it.
		self.dropdown = None;
	}

	pub(crate) fn toggle_settings(&mut self) {
		let open = !self.panel_open();
		self.show_panel(if open {
			PanelPage::Settings(PanelTab::Generic)
		} else {
			PanelPage::Closed
		});
		if open {
			self.settings_scroll = 0.0;
			self.settings_preview = false;
			self.focus = Some(Command::SettingsTab(PanelTab::Generic));
		}
	}

	pub(crate) fn toggle_export(&mut self) {
		let open = !self.export_open();
		self.show_panel(if open {
			PanelPage::Export
		} else {
			PanelPage::Closed
		});
		if open {
			self.export_scroll = 0.0;
			self.focus = Some(Command::ExportRun);
		}
	}

	pub(crate) fn show_styles(&mut self, export: bool) {
		let page = if export {
			if self.export_styles_open() {
				PanelPage::Export
			} else {
				PanelPage::ExportStyles
			}
		} else {
			PanelPage::Settings(PanelTab::Styles)
		};
		self.show_panel(page);
		self.styles_scroll = 0.0;
	}
}

/// Which page of the settings panel is showing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PanelTab {
	#[default]
	Generic,
	Styles,
	Fonts,
	About,
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

/// The full-size image viewer an opened image floats in.
///
/// Like a modal it owns input while it is set: a press pans, a wheel zooms,
/// and a click or Escape closes. The picture is the page's own image texture
/// named the way the document names it, so a diagram shows the same drawing
/// the page does, at whatever resolution the demand has produced.
pub(crate) struct Viewer {
	/// The image's alias, as the page's `Draw::Image` spells it.
	pub(crate) src: String,
	/// The raster's pixel size, which caps how far fitting may upscale.
	pub(crate) pixels: (f32, f32),
	/// The window scale, which puts the pixels in logical units.
	pub(crate) scale: f32,
	/// Zoom over the fitted size, `1.0` showing the whole image.
	pub(crate) zoom: f32,
	/// The picture centre's offset from the window centre.
	pub(crate) pan: (f32, f32),
	/// The last pointer position while panning.
	pub(crate) grab: Option<(f32, f32)>,
	/// Where the press began, to tell a closing click from a pan.
	pub(crate) pressed_at: Option<(f32, f32)>,
	/// Whether this press has ever crossed the drag threshold.
	pub(crate) dragged: bool,
}

/// The margin the fitted picture keeps to the window's edges.
const VIEWER_MARGIN: f32 = 48.0;

impl Viewer {
	/// The picture's displayed size at the current zoom.
	fn displayed(&self, window: (f32, f32)) -> (f32, f32) {
		// Fitting shows everything without upscaling past the pixels the
		// rasterizer produced, so the fitted size is never blurry.
		let fit = ((window.0 - VIEWER_MARGIN) / self.pixels.0.max(1.))
			.min((window.1 - VIEWER_MARGIN) / self.pixels.1.max(1.))
			.min(1. / self.scale.max(1.));
		(
			(self.pixels.0 * fit).max(1.) * self.zoom,
			(self.pixels.1 * fit).max(1.) * self.zoom,
		)
	}

	/// The picture's rect at the current zoom and pan.
	pub(crate) fn rect(
		&self,
		window: (f32, f32),
	) -> markview_core::scene::Rect {
		let (w, h) = self.displayed(window);
		markview_core::scene::Rect {
			x: (window.0 - w) / 2. + self.pan.0,
			y: (window.1 - h) / 2. + self.pan.1,
			w,
			h,
		}
	}

	/// Zooms by `factor` about `pointer`, which stays over the same spot.
	pub(crate) fn zoom_at(
		&mut self,
		factor: f32,
		pointer: (f32, f32),
		window: (f32, f32),
	) {
		let zoom = (self.zoom * factor).clamp(1., 8.);
		let ratio = zoom / self.zoom;
		if (ratio - 1.).abs() <= f32::EPSILON {
			return;
		}
		self.zoom = zoom;
		// The point under the pointer keeps its distance from the picture's
		// centre, which scales with the picture.
		let centre = (window.0 / 2., window.1 / 2.);
		self.pan = (
			pointer.0 - centre.0 - (pointer.0 - centre.0 - self.pan.0) * ratio,
			pointer.1 - centre.1 - (pointer.1 - centre.1 - self.pan.1) * ratio,
		);
		self.clamp_pan(window);
	}

	/// Pans by `delta`, keeping some of the picture inside the window.
	pub(crate) fn pan_by(&mut self, delta: (f32, f32), window: (f32, f32)) {
		self.pan.0 += delta.0;
		self.pan.1 += delta.1;
		self.clamp_pan(window);
	}

	/// Keeps the picture's centre within half a picture of the window's.
	fn clamp_pan(&mut self, window: (f32, f32)) {
		let (w, h) = self.displayed(window);
		self.pan.0 = if w >= window.0 {
			self.pan.0.clamp(-(w - window.0) / 2., (w - window.0) / 2.)
		} else {
			0.
		};
		self.pan.1 = if h >= window.1 {
			self.pan.1.clamp(-(h - window.1) / 2., (h - window.1) / 2.)
		} else {
			0.
		};
	}

	pub(crate) fn begin_press(&mut self, pointer: (f32, f32)) {
		self.grab = Some(pointer);
		self.pressed_at = Some(pointer);
		self.dragged = false;
	}

	pub(crate) fn move_pointer(
		&mut self,
		pointer: (f32, f32),
		window: (f32, f32),
	) {
		if let Some((gx, gy)) = self.grab {
			let (px, py) = self.pressed_at.unwrap();
			self.dragged |= (pointer.0 - px).hypot(pointer.1 - py) > 3.;
			self.pan_by((pointer.0 - gx, pointer.1 - gy), window);
			self.grab = Some(pointer);
		}
	}

	pub(crate) fn cancel_press(&mut self) {
		self.grab = None;
		self.pressed_at = None;
		self.dragged = false;
	}

	/// Returns whether a completed press was a closing click.
	pub(crate) fn finish_press(&mut self) -> bool {
		let clicked = self.pressed_at.is_some() && !self.dragged;
		self.cancel_press();
		clicked
	}
}

/// A control whose options open in a list rather than in place.
///
/// The list is anchored to the control that opens it and floats over the page
/// behind, so the page never reflows for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DropdownId {
	Language,
	/// One font role's family chooser.
	Font(crate::settings::FontRole),
}

/// The option list a control has open.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Dropdown {
	pub(crate) id: DropdownId,
	/// The highlighted option, which `Enter` commits.
	pub(crate) highlight: usize,
	/// The first option drawn. It follows the highlight, so a list too long for
	/// the window always shows the part the reader is on.
	pub(crate) offset: usize,
	/// Wheel travel held back until it adds up to one option, so a trackpad's
	/// small deltas and a mouse's whole notches move the list alike.
	pub(crate) wheel: f32,
}
impl Dropdown {
	pub(crate) fn new(id: DropdownId, highlight: usize) -> Self {
		Self {
			id,
			highlight,
			offset: 0,
			wheel: 0.0,
		}
	}
	/// Moves the highlight by `steps`, wrapping round `count` options.
	pub(crate) fn step(&mut self, steps: i8, count: usize) {
		if count == 0 {
			return;
		}
		if steps < 0 {
			self.highlight = self.highlight.checked_sub(1).unwrap_or(count - 1);
		} else if self.highlight + 1 == count {
			self.highlight = 0;
		} else {
			self.highlight += 1;
		}
	}
	/// Keeps the highlight inside a window of `capacity` drawn options.
	pub(crate) fn follow(&mut self, capacity: usize) {
		if capacity == 0 {
			return;
		}
		let last = self.highlight.saturating_sub(capacity - 1);
		self.offset = self.offset.min(self.highlight).max(last);
	}
}

#[derive(Default, Clone)]
pub(crate) struct ReaderSession {
	pub(crate) search: crate::app::search::SearchState,
	pub(crate) parse_complete: bool,
	pub(crate) export_title: markview_core::text_input::TextInput,
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
	pub(crate) outline_tree: OutlineTree,
	pub(crate) requested_options: Option<LayoutOptions>,
	/// Reader-chosen `<details>` collapse state, keyed by block id, overriding
	/// what the source declared. It is layout input, and a reload drops it.
	pub(crate) details_open: Arc<BTreeMap<u64, bool>>,
	pub(crate) scrolling: markview_selection::ScrollState,
	pub(crate) horizontal: HashMap<(usize, usize), f32>,
	pub(crate) follow_update: bool,
	pub(crate) layout_pending: bool,
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
		self.outline_tree = OutlineTree::default();
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
					|| block.y + anchor.y > self.scrolling.offset + 0.5
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
	pub(crate) panel: PanelPage,
	pub(crate) settings_scroll: f32,
	pub(crate) settings_preview: bool,
	pub(crate) export_scroll: f32,
	/// Offset from the centre of the panel scrollbar thumb while dragging.
	pub(crate) panel_grab: Option<f32>,
	/// The Styles page's list offset. The export's stylesheet chooser shares
	/// it: no two pages of the panel are ever open at once.
	pub(crate) styles_scroll: f32,
	pub(crate) selection: Option<TextSelection>,
	pub(crate) pointer_down: Option<Drag>,
	/// An image press, independent of whether the page has selectable text.
	pub(crate) pressed_image: Option<(String, (f32, f32))>,
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
	/// The full-size image viewer; while it is set it owns input too.
	pub(crate) viewer: Option<Viewer>,
	/// An open option list. Like a confirmation it owns input while it is set:
	/// a press outside it closes it without reaching the page behind.
	pub(crate) dropdown: Option<Dropdown>,
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

	/// Closes the drawer when a press lands outside it, except on its toggle.
	pub(crate) fn close_outline_if_outside(
		&mut self,
		inside: bool,
		on_toggle: bool,
	) -> bool {
		if self.outline_open && !inside && !on_toggle {
			self.close_outline();
			true
		} else {
			false
		}
	}

	/// Whether the drawer answers input.
	///
	/// The drawer is an overlay, not a panel, so it stands down while a panel
	/// or a confirmation owns input: both draw over it, and the panel's
	/// scrollbar drag and outside-click dismissal must keep working where they
	/// overlap it.
	pub(crate) fn outline_owns_input(&self) -> bool {
		self.outline_open && !self.panel_open() && self.modal.is_none()
	}

	/// Moves the drawer's selection by `delta` entries, clamped to the
	/// outline. Returns whether there is an entry to move to.
	pub(crate) fn move_outline(
		&mut self,
		delta: isize,
		rows: &[usize],
	) -> bool {
		if rows.is_empty() {
			self.outline_selection = None;
			self.focus = None;
			return false;
		}
		let base = rows
			.iter()
			.position(|index| Some(*index) == self.outline_selection)
			.unwrap_or(0) as isize;
		let next =
			rows[(base + delta).clamp(0, rows.len() as isize - 1) as usize];
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
		if let Command::OutlineGoto(row) | Command::OutlineToggle(row) = action
		{
			self.outline_selection = Some(row);
		}
		Some(action)
	}
}

/// The shortest and longest a discrete scroll may take, and the distance at
/// which it reaches the longest.
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
			return reader.layout.height >= self.scrolling.offset + viewport;
		}
		let index = self
			.snapshot
			.blocks
			.partition_point(|b| b.y <= self.scrolling.offset)
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
				b.y + (self.scrolling.offset - anchor.y).min(b.layout.height)
					+ viewport <= reader.layout.height
			})
	}
	fn scroll_bounds(&self, viewport: f32) -> markview_selection::ScrollBounds {
		markview_selection::ScrollBounds {
			max: if self.layout_pending {
				(self.snapshot.height - viewport).max(0.0)
			} else {
				scroll_limit(self.snapshot.height, viewport)
			},
			complete: !self.layout_pending,
		}
	}
	pub(crate) fn scroll_by(&mut self, dy: f32, viewport: f32) {
		if dy != 0.0 {
			self.pending_anchor = None;
			self.follow_update = false;
		}
		self.scrolling.by(dy, self.scroll_bounds(viewport));
	}
	pub(crate) fn animate_scroll_by(&mut self, dy: f32, now: Instant) {
		if dy != 0.0 {
			self.pending_anchor = None;
			self.follow_update = false;
		}
		self.scrolling.animate_by(dy, now);
	}
	pub(crate) fn animate_wheel_by(&mut self, dy: f32, now: Instant) {
		if dy != 0.0 {
			self.pending_anchor = None;
			self.follow_update = false;
		}
		self.scrolling.wheel_by(dy, now);
	}
	pub(crate) fn coast_wheel_by(&mut self, dy: f32, now: Instant) {
		if dy != 0.0 {
			self.pending_anchor = None;
			self.follow_update = false;
		}
		self.scrolling.coast_wheel_by(dy, now);
	}
	pub(crate) fn animate_scroll_to(&mut self, target: f32, now: Instant) {
		self.pending_anchor = None;
		self.follow_update = false;
		self.scrolling.animate_to(target, now);
	}
	pub(crate) fn advance_scroll(
		&mut self,
		now: Instant,
		viewport: f32,
	) -> bool {
		self.scrolling.advance(now, self.scroll_bounds(viewport))
	}
	pub(crate) fn cancel_scroll_animation(&mut self) {
		self.scrolling.cancel();
	}
	pub(crate) fn scroll_animating(&self) -> bool {
		self.scrolling.animating()
	}
	pub(crate) fn scroll_animation_deadline(
		&self,
		now: Instant,
	) -> Option<Instant> {
		self.scrolling.deadline(now)
	}
	pub(crate) fn resolve_scroll(&mut self, viewport: f32) {
		self.scrolling.resolve(self.scroll_bounds(viewport));
	}
	pub(crate) fn coverage(&self, viewport: f32) -> f32 {
		self.scrolling
			.offset
			.max(self.scrolling.target.unwrap_or(self.scrolling.offset))
			+ viewport * 1.5
	}
	pub(crate) fn release_heavy(&mut self) {
		self.counts = TextCounts::default();
		self.snapshot = LayoutSnapshot::default();
		self.snapshot_complete = false;
		self.document = None;
		self.search.document = None;
		self.parse_complete = false;
		self.outline = None;
		self.outline_tree = OutlineTree::default();
		self.requested_options = None;
		self.pending_anchor = None;
		self.jump_origin = None;
		self.scrolling.cancel();
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
			self.scrolling.target = None;
			self.follow_update = false;
			self.scrolling.offset =
				y.clamp(0.0, scroll_limit(self.snapshot.height, viewport));
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
		self.scrolling.offset = if extending {
			self.scrolling.offset
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
			self.scrolling.offset.clamp(0.0, limit)
		} else {
			crate::layout::anchored_scroll(
				&self.snapshot,
				&reader.layout,
				self.scrolling.offset,
				viewport,
				self.follow_update,
			)
		};
		self.accepted_content_id = reader.document.content_id;
		self.document = Some(reader.document);
		self.snapshot = reader.layout;
		self.layout_pending = !reader.complete;
		self.snapshot_complete = reader.complete;
		self.parse_complete = reader.parse_complete;
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

/// The state the selection machine keeps, reached through the accessors this
/// crate's own types already name. `cursor` and `modifiers` are the reader's
/// rather than the machine's: the tab strip, the chrome and the option lists
/// all read the same pointer, and a chord is a chord wherever it lands.
impl Host for InteractionState {
	fn cursor(&self) -> Point {
		Point::new(self.cursor.0, self.cursor.1)
	}
	fn modifiers(&self) -> Modifiers {
		Modifiers {
			shift: self.modifiers.shift_key(),
			control: self.modifiers.control_key(),
			alt: self.modifiers.alt_key(),
			meta: self.modifiers.super_key(),
		}
	}
	fn selection(&self) -> Option<TextSelection> {
		self.selection
	}
	fn set_selection(&mut self, selection: Option<TextSelection>) {
		self.selection = selection;
	}
	fn drag(&self) -> Option<&Drag> {
		self.pointer_down.as_ref()
	}
	fn set_drag(&mut self, drag: Option<Drag>) {
		self.pointer_down = drag;
	}
	fn take_drag(&mut self) -> Option<Drag> {
		self.pointer_down.take()
	}
	fn dragged(&self) -> bool {
		self.dragged
	}
	fn set_dragged(&mut self, dragged: bool) {
		self.dragged = dragged;
	}
	fn auto_scroll_at(&self) -> Option<Instant> {
		self.drag_at
	}
	fn set_auto_scroll_at(&mut self, at: Option<Instant>) {
		self.drag_at = at;
	}
	fn last_click(&self) -> Option<(Instant, Point, u8)> {
		self.last_click
			.map(|(at, (x, y), count)| (at, Point::new(x, y), count))
	}
	fn set_last_click(&mut self, click: Option<(Instant, Point, u8)>) {
		self.last_click =
			click.map(|(at, point, count)| (at, (point.x, point.y), count));
	}
	fn blur(&mut self) {
		self.focus = None;
	}
	fn release_pointer(&mut self) {
		self.pressed_image = None;
		self.scrollbar = None;
	}
}

#[cfg(test)]
mod tests;
