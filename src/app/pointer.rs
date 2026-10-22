use crate::state::{ChromeCommand, DocumentCommand};
use crate::state::{
	Command, Drag, Grain, HoveredLink, ScrollbarDrag, Selection, WheelGesture,
	WheelStep,
};
use crate::{link, state};
use markview_document::footnote;
use std::{path::PathBuf, time::Instant};
use winit::{
	event::{MouseScrollDelta, TouchPhase},
	keyboard::ModifiersState,
	window::CursorIcon,
};

use super::modal::{Message, Modal, PermissionAction};
use super::viewer::Message as ViewerMessage;
use super::{App, LINE_STEP, TOP, anchor, chrome};
use markview_platform::{WheelAmount, WheelNotch};

/// Everything the pointer owns between events: the press in flight, what it
/// hovers, the scrollbar grab and the wheel gesture.
///
/// The input hot paths call these methods directly; no message queue stands
/// between the event and the state. The fields stay private so every direct
/// write has exactly one owner, and every other module reads and writes
/// through the methods.
#[derive(Default)]
pub(crate) struct State {
	/// The press the selection machine is extending, if any.
	pointer_down: Option<Drag>,
	/// An image press, independent of whether the page has selectable text.
	pressed_image: Option<(String, (f32, f32))>,
	dragged: bool,
	drag_at: Option<Instant>,
	modifiers: ModifiersState,
	cursor: (f32, f32),
	hover: Option<HoveredLink>,
	hover_image: Option<String>,
	/// The wide block whose horizontal scrollbar the pointer is over.
	hover_overflow: Option<(usize, usize)>,
	scrollbar: Option<ScrollbarDrag>,
	last_click: Option<(Instant, (f32, f32), u8)>,
	/// The axis of the wheel gesture in flight.
	wheel: WheelGesture,
}

impl State {
	/// A pointer state that only carries a position, for the throwaway
	/// interaction values the chrome draws with.
	pub(crate) fn at(cursor: (f32, f32)) -> Self {
		Self {
			cursor,
			..Default::default()
		}
	}

	pub(crate) fn cursor(&self) -> (f32, f32) {
		self.cursor
	}

	pub(crate) fn move_to(&mut self, point: (f32, f32)) {
		self.cursor = point;
	}

	/// The pointer left the window or the surface that reported it.
	pub(crate) fn leave(&mut self) {
		self.cursor = (f32::NEG_INFINITY, f32::NEG_INFINITY);
	}

	pub(crate) fn modifiers(&self) -> ModifiersState {
		self.modifiers
	}

	pub(crate) fn set_modifiers(&mut self, modifiers: ModifiersState) {
		self.modifiers = modifiers;
	}

	pub(crate) fn drag(&self) -> Option<&Drag> {
		self.pointer_down.as_ref()
	}

	pub(crate) fn press(&mut self, drag: Drag) {
		self.pointer_down = Some(drag);
	}

	pub(crate) fn release(&mut self) -> Option<Drag> {
		self.pointer_down.take()
	}

	pub(crate) fn dragged(&self) -> bool {
		self.dragged
	}

	pub(crate) fn set_dragged(&mut self, dragged: bool) {
		self.dragged = dragged;
	}

	pub(crate) fn auto_scroll_at(&self) -> Option<Instant> {
		self.drag_at
	}

	pub(crate) fn set_auto_scroll_at(&mut self, at: Option<Instant>) {
		self.drag_at = at;
	}

	pub(crate) fn pressed_image(&self) -> Option<&(String, (f32, f32))> {
		self.pressed_image.as_ref()
	}

	pub(crate) fn set_pressed_image(
		&mut self,
		image: Option<(String, (f32, f32))>,
	) {
		self.pressed_image = image;
	}

	pub(crate) fn take_pressed_image(
		&mut self,
	) -> Option<(String, (f32, f32))> {
		self.pressed_image.take()
	}

	pub(crate) fn clear_pressed_image(&mut self) {
		self.pressed_image = None;
	}

	pub(crate) fn hover(&self) -> Option<&HoveredLink> {
		self.hover.as_ref()
	}

	pub(crate) fn hover_image(&self) -> Option<&str> {
		self.hover_image.as_deref()
	}

	pub(crate) fn hover_overflow(&self) -> Option<(usize, usize)> {
		self.hover_overflow
	}

	pub(crate) fn set_hover(
		&mut self,
		hover: Option<HoveredLink>,
		image: Option<String>,
		overflow: Option<(usize, usize)>,
	) {
		self.hover = hover;
		self.hover_image = image;
		self.hover_overflow = overflow;
	}

	pub(crate) fn clear_hover(&mut self) {
		self.set_hover(None, None, None);
	}

	/// The document hover ends while the image hint stays, which is what a
	/// pointer leaving the window has always left behind.
	pub(crate) fn clear_document_hover(&mut self) {
		self.hover = None;
		self.hover_overflow = None;
	}

	pub(crate) fn scrollbar(&self) -> Option<ScrollbarDrag> {
		self.scrollbar
	}

	pub(crate) fn begin_scrollbar(&mut self, drag: ScrollbarDrag) {
		self.scrollbar = Some(drag);
	}

	pub(crate) fn end_scrollbar(&mut self) {
		self.scrollbar = None;
	}

	pub(crate) fn last_click(&self) -> Option<(Instant, (f32, f32), u8)> {
		self.last_click
	}

	pub(crate) fn set_last_click(
		&mut self,
		click: Option<(Instant, (f32, f32), u8)>,
	) {
		self.last_click = click;
	}

	pub(crate) fn feed_wheel(
		&mut self,
		dx: f32,
		dy: f32,
		now: Instant,
		horizontal_only: bool,
		phase: TouchPhase,
	) -> WheelStep {
		self.wheel.feed(dx, dy, now, horizontal_only, phase)
	}

	pub(crate) fn reset_wheel(&mut self) {
		self.wheel = WheelGesture::default();
	}

	/// Ends an unfinished press: the drag, the image target, the auto-scroll
	/// deadline and the scrollbar grab. Hover and click history are not a
	/// press and stay.
	pub(crate) fn cancel(&mut self) {
		self.pointer_down = None;
		self.pressed_image = None;
		self.drag_at = None;
		self.scrollbar = None;
	}

	/// A new press ends what a swallowed release left behind: the previous
	/// image target and scrollbar grab go before the new owner runs.
	pub(crate) fn reset_press(&mut self) {
		self.pressed_image = None;
		self.scrollbar = None;
	}
}

/// The logical pixels one notch of `amount` travels along an axis `extent` long.
///
/// A character borrows the line step: Windows names lines and characters in
/// separate settings, but the two only differ in the count they carry.
fn notch_pixels(amount: WheelAmount, extent: f32) -> f32 {
	match amount {
		WheelAmount::Lines(lines) => LINE_STEP * lines,
		WheelAmount::Page => extent * 0.9,
	}
}

/// The logical pixels one wheel event asks the reader to move.
///
/// A line delta is a notch on X11 and Wayland, where the desktop's own speed
/// setting never reaches the event, and an already-scaled line on macOS, where
/// it does, so the notch only multiplies where the platform left it raw. Each
/// axis carries the desktop's own value, because Windows configures the two
/// separately. A pixel delta is physical, so the display scale turns it back
/// into logical pixels. The reader's speed multiplier scales both.
pub(super) fn wheel_pixels(
	delta: MouseScrollDelta,
	notch: WheelNotch,
	speed: f32,
	scale: f32,
	viewport: (f32, f32),
) -> (f32, f32) {
	match delta {
		MouseScrollDelta::LineDelta(x, y) => (
			x * notch_pixels(notch.horizontal, viewport.0) * speed,
			y * notch_pixels(notch.vertical, viewport.1) * speed,
		),
		MouseScrollDelta::PixelDelta(p) => {
			let step = speed / scale;
			(p.x as f32 * step, p.y as f32 * step)
		}
	}
}

/// Whether a wheel event comes from a high-resolution device.
///
/// A mouse wheel names whole detents. A touchpad emulating one through
/// Windows' high-resolution wheel support names the fractional counts it
/// accumulated instead, and the same device hands its inertia over in a few
/// large packets rather than as the stream a desktop's own smooth scrolling
/// would give.
pub(super) fn high_resolution(delta: MouseScrollDelta) -> bool {
	match delta {
		MouseScrollDelta::LineDelta(x, y) => {
			x.fract() != 0.0 || y.fract() != 0.0
		}
		MouseScrollDelta::PixelDelta(_) => false,
	}
}

/// Whether a `dy` of input has a finite distance to ease over.
fn eases(dy: f32) -> bool {
	dy.is_finite() && dy != 0.0
}

impl<P: super::SendEvent> App<P> {
	pub(super) fn pointer_moved(
		&mut self,
		position: winit::dpi::PhysicalPosition<f64>,
	) {
		let scale = self.dimensions().2;
		let insets = self.insets();
		let point = (
			position.x as f32 / scale - insets[0],
			position.y as f32 / scale - insets[1],
		);
		// The macOS backend reports the pointer's position again
		// before every wheel event, hand still or not. A report that
		// moves nothing is not motion, and every handler below
		// answers motion — the open list's hover, most urgently,
		// must not claim the highlight back from the wheel the
		// report precedes.
		if point != self.interaction.pointer.cursor() {
			let old = self.interaction.pointer.cursor();
			let was_button = self.button_at_cursor();
			self.interaction.pointer.move_to(point);
			if let Some((_, (px, py))) =
				self.interaction.pointer.pressed_image()
				&& (point.0 - px).hypot(point.1 - py) > 3.
			{
				self.interaction.pointer.clear_pressed_image();
			}
			self.hover_dropdown();
			self.drag_sidebar();
			self.move_tab_drag();
			self.drag_scrollbar();
			self.drag_panel();
			self.drag_folder();
			self.update_drag();
			self.refresh_hover();
			let now = Instant::now();
			if self.hover_preview(now) {
				self.redraw();
			}
			if self.interaction.panel_open()
				|| was_button
				|| self.button_at_cursor()
				|| old.1 < TOP
				|| point.1 < TOP
				|| old.0 >= self.dimensions().0 - 16.
				|| point.0 >= self.dimensions().0 - 16.
			{
				self.redraw();
			}
		}
	}

	pub(super) fn pointer_left(&mut self) {
		if self.tab_strip.drag.is_none()
			&& self.interaction.pointer.scrollbar().is_none()
			&& self.interaction.panels.grab().is_none()
			&& self.folder.grab.is_none()
			&& self.interaction.panels.sidebar().drag().is_none()
		{
			self.interaction.pointer.leave();
		}
		// A scrollbar drag survives leaving the window: the implicit
		// pointer grab still reports motion and the release, so the
		// thumb keeps following the pointer past the edges. The text
		// selection gesture still ends here.
		self.interaction.pointer.release();
		self.interaction.pointer.clear_pressed_image();
		self.interaction.pointer.set_auto_scroll_at(None);
		self.interaction.pointer.clear_document_hover();
		self.dismiss_preview();
		if let Some(w) = &self.window {
			w.set_cursor(CursorIcon::Default);
		}
		self.redraw();
	}

	pub(super) fn pointer_pressed(&mut self) {
		self.prepare_press();
		if self.press_modal()
			|| self.press_dropdown()
			|| self.begin_sidebar_resize()
			|| self.press_drawer()
			|| self.press_tab()
			|| self.press_button()
			|| self.press_panel()
		{
			return;
		}
		self.press_page();
	}

	/// Readies the pointer for the press about to land: the previous gesture
	/// ends and the capture a swallowed release may have left starts clean.
	fn prepare_press(&mut self) {
		self.dismiss_preview();
		self.cancel_gestures();
		self.tab_strip.cancel_drag();
		self.interaction.widgets.focus_visible = false;
		self.interaction.widgets.captured = None;
		self.interaction.pointer.reset_press();
		self.readers.session.select_all_pending = false;
		self.interaction.panels.set_grab(None);
		self.folder.grab = None;
		self.interaction.panels.sidebar_mut().clear();
	}

	/// A confirmation owns input: only its buttons answer.
	fn press_modal(&mut self) -> bool {
		if !self.interaction.overlays.modal_open() {
			return false;
		}
		self.interaction.reset_clicks();
		let buttons = self.buttons();
		self.press_widget(&buttons);
		self.redraw();
		true
	}

	/// An open option list owns input the way a confirmation does: a press on
	/// one of its own options picks it, and a press anywhere else closes it
	/// without reaching the page it covered. Nothing behind it answers the
	/// same click, so the press is always the last word and the release that
	/// follows has nothing left to dispatch.
	fn press_dropdown(&mut self) -> bool {
		if !self.interaction.overlays.dropdown_open() {
			return false;
		}
		self.interaction.reset_clicks();
		let buttons = self.dropdown_buttons();
		self.press_widget(&buttons);
		if self.interaction.widgets.captured.is_none() {
			self.close_dropdown();
		}
		self.redraw();
		true
	}

	/// The drawer answers a press inside it; a press outside closes the
	/// outline unless it landed on its toggle. Returns whether a folder drag
	/// consumed the press.
	fn press_drawer(&mut self) -> bool {
		let (x, y) = self.interaction.pointer.cursor();
		let on_outline_toggle = self.buttons().into_iter().any(|button| {
			button.action == Command::Document(DocumentCommand::Outline)
				&& button.rect.contains(x, y)
		});
		if self.interaction.close_outline_if_outside(
			self.pointer_in_outline(),
			on_outline_toggle,
		) {
			self.redraw();
		}
		self.pointer_in_folder() && self.begin_folder_drag()
	}

	/// A press on a tab answers before the toolbar buttons under it.
	fn press_tab(&mut self) -> bool {
		if self.interaction.panel_open() {
			return false;
		}
		if let Some(index) = self.tab_close_at_cursor() {
			self.interaction.reset_clicks();
			self.interaction.widgets.focus = None;
			self.action(Command::Chrome(ChromeCommand::CloseTab(index)));
			return true;
		}
		let Some(index) = self.tab_at_cursor() else {
			return false;
		};
		self.interaction.reset_clicks();
		self.interaction.widgets.focus = None;
		self.begin_tab_drag(index);
		true
	}

	fn press_button(&mut self) -> bool {
		if !self.button_at_cursor() {
			return false;
		}
		self.interaction.reset_clicks();
		let buttons = self.buttons();
		self.press_widget(&buttons);
		self.redraw();
		true
	}

	/// A panel draws over the drawer, so it answers first: its scrollbar drag
	/// and its outside-click dismissal must work where the two overlap.
	fn press_panel(&mut self) -> bool {
		if !self.interaction.panel_open() {
			return false;
		}
		if self.begin_panel_drag() {
			self.redraw();
			return true;
		}
		self.interaction.reset_clicks();
		if !self.pointer_in_panel() {
			self.action(Command::Chrome(ChromeCommand::Settings));
		}
		true
	}

	/// The page behind every overlay: the drawer takes a press inside it, the
	/// scrollbar next, and only then the text selection.
	fn press_page(&mut self) {
		if self.pointer_in_folder() || self.pointer_in_outline() {
			// The drawer owns presses inside it; one between its rows must not
			// start a document selection underneath.
			self.interaction.reset_clicks();
			return;
		}
		if self.pointer_in_panel() {
			return;
		}
		self.interaction.widgets.focus = None;
		if self.press_scrollbar() {
			return;
		}
		self.press_selection();
	}

	fn press_scrollbar(&mut self) -> bool {
		if !self.begin_scrollbar_drag() {
			return false;
		}
		self.redraw();
		true
	}

	fn press_selection(&mut self) {
		let (x, y) = self.interaction.pointer.cursor();
		if y < self.content_top() + 10.0
			|| y >= self.dimensions().1 - self.bottom() - 10.0
		{
			return;
		}
		let link = self.link_at(x, y);
		self.interaction.pointer.set_pressed_image(
			self.image_at_cursor().map(|(src, _)| (src, (x, y))),
		);
		self.interaction.pointer.set_dragged(false);
		if let Some(position) = self.text_at_cursor() {
			let click_count =
				if self.interaction.pointer.modifiers().shift_key() {
					self.interaction.reset_clicks();
					1
				} else {
					self.interaction.click_count(Instant::now())
				};
			match click_count {
				2 => {
					let selection =
						self.readers.session.snapshot.select_word_at(position);
					if !self
						.interaction
						.begin_grain_selection(selection, Grain::Word)
					{
						self.interaction.begin_selection(position, link);
					}
				}
				3 => {
					let selection =
						self.readers.session.snapshot.select_block_at(position);
					if !self
						.interaction
						.begin_grain_selection(selection, Grain::Block)
					{
						self.interaction.begin_selection(position, link);
					}
				}
				_ => self.interaction.begin_selection(position, link),
			}
		} else if let Some(link) = link {
			// A summary line's marker carries no text, but the whole line is
			// still its control.
			self.interaction.begin_link_press(link);
		}
		self.redraw();
	}

	pub(super) fn pointer_released(&mut self) {
		if self.interaction.panels.sidebar_mut().take().is_some() {
			self.refresh_hover();
			self.redraw();
			return;
		}
		self.tab_strip.cancel_drag();
		let pressed_image = self.interaction.pointer.take_pressed_image();
		let was_pressed = self.interaction.widgets.captured.is_some();
		let action = self.release_widget();
		self.interaction.pointer.end_scrollbar();
		self.interaction.panels.set_grab(None);
		self.folder.grab = None;
		self.interaction.panels.sidebar_mut().clear();
		if was_pressed {
			if let Some(action) = action {
				self.action(action);
			}
			self.refresh_hover();
			self.redraw();
			return;
		}
		if self.interaction.overlays.modal_open() {
			self.redraw();
			return;
		}
		let (x, y) = self.interaction.pointer.cursor();
		let link = self.link_at(x, y);
		if let Some(link) = self.interaction.finish_selection(link.as_deref()) {
			self.open_link(&link, false);
		} else if !self.interaction.pointer.dragged()
			&& pressed_image.is_some_and(|(src, _)| {
				self.image_at_cursor()
					.is_some_and(|(released, _)| released == src)
			}) {
			// A click that selected nothing and hit no link opens the
			// viewer when it landed on an image.
			self.open_viewer_at_cursor();
		}
		self.refresh_hover();
		self.redraw();
	}

	pub(super) fn pointer_in_panel(&self) -> bool {
		let (width, height, _) = self.dimensions();
		let rect = if self.interaction.panel == crate::state::PanelPage::Tabs {
			chrome::tab_drawer::animated_rect(
				width,
				height,
				self.tabs_reveal(),
				self.preferences.values.tabs_width,
			)
		} else {
			chrome::panel_rect(width, height)
		};
		self.interaction.panel_open()
			&& rect.contains(
				self.interaction.pointer.cursor().0,
				self.interaction.pointer.cursor().1,
			)
	}
	pub(super) fn panel_has_focus(&self) -> bool {
		self.interaction.panel_open()
	}
	/// The reader's scroll-speed multiplier over the desktop's own speed.
	pub(super) fn scroll_speed(&self) -> f32 {
		self.preferences.values.scroll_speed
	}
	/// Logical pixels one line of a discrete scroll travels.
	pub(super) fn line_step(&self) -> f32 {
		LINE_STEP * self.scroll_speed()
	}
	pub(super) fn scroll_by(&mut self, dy: f32) {
		self.readers.session.scroll_by(dy, self.viewport());
		self.after_scroll();
	}
	/// A discrete scroll step, eased to its destination.
	pub(super) fn scroll_step(&mut self, dy: f32) {
		if eases(dy) {
			self.readers.session.animate_scroll_by(dy, Instant::now());
		} else {
			self.readers.session.scroll_by(dy, self.viewport());
		}
		self.after_scroll();
	}
	/// A wheel travel, eased like a discrete step.
	pub(super) fn scroll_wheel(&mut self, dy: f32) {
		if eases(dy) {
			self.readers.session.animate_wheel_by(dy, Instant::now());
		} else {
			self.readers.session.scroll_by(dy, self.viewport());
		}
		self.after_scroll();
	}
	/// A wheel travel from a high-resolution device, which arrives in packets.
	pub(super) fn scroll_wheel_packet(&mut self, dy: f32) {
		if eases(dy) {
			self.readers.session.coast_wheel_by(dy, Instant::now());
		} else {
			self.readers.session.scroll_by(dy, self.viewport());
		}
		self.after_scroll();
	}
	/// Home and End. The top is known before the geometry is, so it eases to
	/// zero; the end is only a number once the snapshot is complete, and until
	/// then the existing infinite target waits for the final height.
	pub(super) fn scroll_bound(&mut self, to_end: bool) {
		let destination = if to_end {
			(self.readers.session.snapshot_complete
				&& !self.readers.session.layout_pending)
				.then(|| {
					crate::state::scroll_limit(
						self.readers.session.snapshot.height,
						self.viewport(),
					)
				})
		} else {
			Some(0.0)
		};
		if let Some(destination) = destination
			&& (destination - self.readers.session.scrolling.offset).abs() > 0.5
		{
			self.readers
				.session
				.animate_scroll_to(destination, Instant::now());
			self.after_scroll();
			return;
		}
		self.scroll_by(if to_end {
			f32::INFINITY
		} else {
			f32::NEG_INFINITY
		});
	}
	/// Steps a running scroll animation and asks for the next frame.
	pub(super) fn advance_scroll(&mut self, now: Instant) {
		if !self.readers.session.scroll_animating() {
			return;
		}
		if self.readers.session.advance_scroll(now, self.viewport()) {
			self.worker
				.prioritize(self.readers.session.coverage(self.viewport()));
		}
		self.follow_scroll();
		self.refresh_hover();
		self.redraw();
	}
	/// Re-prioritizes the worker, follows the pointer and repaints after the
	/// offset moved.
	pub(super) fn after_scroll(&mut self) {
		self.follow_scroll();
		self.worker
			.prioritize(self.readers.session.coverage(self.viewport()));
		self.refresh_hover();
		self.redraw();
	}
	/// The text moves under a pointer that has not, so a press in flight now
	/// covers different reading text. No pointer event arrives to say so: the
	/// pointer never moved, which is why every path that moves the offset has
	/// to say it instead.
	fn follow_scroll(&mut self) {
		if self.interaction.pointer.drag().is_some() {
			self.update_drag();
		}
	}
	/// The link under a window point, using the same origin as the renderer.
	pub(super) fn link_at(&self, px: f32, py: f32) -> Option<String> {
		self.document_interaction()
			.link(markview_selection::Point::new(px, py))
			.map(str::to_owned)
	}
	pub(super) fn button_at_cursor(&mut self) -> bool {
		let buttons = self.buttons();
		super::widgets::layout(&buttons)
			.hit(self.interaction.pointer.cursor())
			.is_some()
	}

	fn holding_pointer(&self) -> bool {
		self.interaction.pointer.drag().is_some()
			|| self.interaction.pointer.scrollbar().is_some()
			|| self.tab_strip.drag.is_some()
			|| self.interaction.panels.sidebar().drag().is_some()
	}

	/// Hover state follows scrolling and reflow, not only pointer motion.
	pub(super) fn refresh_hover(&mut self) {
		if self.interaction.overlays.viewer_open() {
			let grabbing = self
				.interaction
				.overlays
				.viewer()
				.is_some_and(|viewer| viewer.grabbing());
			self.interaction.pointer.clear_hover();
			if let Some(w) = &self.window {
				w.set_cursor(if grabbing {
					CursorIcon::Grabbing
				} else {
					self.frame_cursor().unwrap_or(CursorIcon::Default)
				});
			}
			return;
		}
		let controls = self.ui_layout();
		self.interaction.widgets.handle(
			&controls.hit,
			markview_ui_core::Input::Move(self.interaction.pointer.cursor()),
		);
		let over_control = controls
			.hit
			.hit(self.interaction.pointer.cursor())
			.is_some();
		let holding = self.holding_pointer();
		// The drawer covers document content, so it must not inherit the link
		// or image hover underneath it.
		let over_outline = self.pointer_in_outline()
			|| self.pointer_in_folder()
			|| self.sidebar_edge().is_some();
		let idle = !self.interaction.panel_open()
			&& !self.interaction.overlays.modal_open()
			&& !holding
			&& !over_outline;
		let document_hover = if idle {
			self.document_interaction().hover(
				markview_selection::Point::new(
					self.interaction.pointer.cursor().0,
					self.interaction.pointer.cursor().1,
				),
				false,
				true,
				self.preferences
					.values
					.stylesheet
					.overflow_scrollbar_metrics(),
			)
		} else {
			markview_selection::Hover::default()
		};
		let cursor = self.hover_cursor(
			document_hover.cursor,
			document_hover.link.is_some(),
			over_control,
			over_outline,
			idle,
		);
		let hover = document_hover.link;
		let hover_overflow = document_hover.overflow;

		let hover_changed = hover.as_ref() != self.interaction.pointer.hover()
			|| hover_overflow != self.interaction.pointer.hover_overflow();
		let hover_image = document_hover.image_title;
		let hover_changed = hover_changed
			|| hover_image.as_deref() != self.interaction.pointer.hover_image();
		self.interaction
			.pointer
			.set_hover(hover, hover_image, hover_overflow);
		if let Some(w) = &self.window {
			w.set_cursor(cursor);
		}
		if hover_changed {
			self.redraw();
		}
	}
	fn hover_cursor(
		&mut self,
		document_cursor: markview_selection::Cursor,
		over_link: bool,
		over_control: bool,
		over_outline: bool,
		idle: bool,
	) -> CursorIcon {
		if self.interaction.panels.sidebar().drag().is_some()
			|| self.sidebar_edge().is_some()
		{
			CursorIcon::ColResize
		} else if self.input_at_cursor().is_some() {
			CursorIcon::Text
		} else if self.tab_strip.drag.is_some_and(|d| d.moving) {
			CursorIcon::Grabbing
		} else if self.interaction.pointer.scrollbar().is_some() {
			CursorIcon::Default
		} else if self.interaction.pointer.drag().is_some() {
			if self.text_under_cursor() {
				CursorIcon::Text
			} else {
				CursorIcon::Default
			}
		} else if let Some(cursor) = self.frame_cursor() {
			cursor
		} else if over_control || self.tab_at_cursor().is_some() || over_link {
			CursorIcon::Pointer
		} else if !self.interaction.panel_open() && !over_outline {
			self.document_hover_cursor(document_cursor, idle)
		} else {
			CursorIcon::Default
		}
	}

	fn document_hover_cursor(
		&self,
		document_cursor: markview_selection::Cursor,
		idle: bool,
	) -> CursorIcon {
		let cursor = if idle {
			document_cursor
		} else {
			self.document_interaction().cursor(
				markview_selection::Point::new(
					self.interaction.pointer.cursor().0,
					self.interaction.pointer.cursor().1,
				),
				false,
				true,
			)
		};
		match cursor {
			markview_selection::Cursor::Default => CursorIcon::Default,
			markview_selection::Cursor::Text => CursorIcon::Text,
			markview_selection::Cursor::Pointer => CursorIcon::Pointer,
		}
	}

	/// The image under the cursor, named the way the viewer and the renderer
	/// name textures.
	pub(super) fn image_at_cursor(
		&self,
	) -> Option<(String, markview_layout::scene::Rect)> {
		self.document_interaction()
			.image(markview_selection::Point::new(
				self.interaction.pointer.cursor().0,
				self.interaction.pointer.cursor().1,
			))
			.map(|(src, rect, _)| (src.to_owned(), rect))
	}

	/// Opens the full-size viewer over the image under the cursor, if any.
	pub(super) fn open_viewer_at_cursor(&mut self) {
		if self.interaction.overlays.viewer_open() {
			return;
		}
		let Some((src, rect)) = self.image_at_cursor() else {
			return;
		};
		if let Some(resource) =
			self.readers.session.blocked_images.get(&src).cloned()
		{
			self.ask_permission(resource, PermissionAction::Images);
			return;
		}
		// The pixels the renderer already holds cap the fitting, so the
		// viewer never promises a sharper picture than exists. A picture
		// still decoding fits its laid-out rect instead.
		let images = &self.readers.session.snapshot.images;
		let pixels = images
			.entries
			.get(&src)
			.and_then(|info| images.pixels.get(&src, info.version))
			.map(|p| (p.width as f32, p.height as f32))
			.unwrap_or((rect.w.max(1.), rect.h.max(1.)));
		let scale = self.dimensions().2;
		self.interaction.overlays.viewer.send(ViewerMessage::Open {
			src,
			pixels,
			scale,
		});
		self.cancel_gestures();
		// Opening the viewer replaces the view, so the viewport's gesture
		// is abandoned with the reader's own motion.
		self.abandon_dm();
		self.interaction.reset_clicks();
		self.sync_input();
		self.refresh_hover();
		self.redraw();
	}

	/// Decoded pixels can arrive while the viewer is already open.
	pub(super) fn refresh_viewer(&mut self) {
		let (width, height, scale) = self.dimensions();
		if !self.interaction.overlays.viewer_open() {
			return;
		}
		let images = &self.readers.session.snapshot.images;
		let pixels = self.interaction.overlays.viewer().and_then(|viewer| {
			images
				.entries
				.get(viewer.src())
				.and_then(|info| images.pixels.get(viewer.src(), info.version))
				.map(|pixels| (pixels.width as f32, pixels.height as f32))
		});
		self.interaction.overlays.viewer.refresh(
			pixels,
			scale,
			(width, height),
		);
	}

	pub(super) fn open_link(&mut self, url: &str, background: bool) {
		// Activating a link is direct input, so nothing keeps easing behind it.
		self.readers.session.cancel_scroll_animation();
		// A `<details>` summary is hit like a link but toggles its element.
		if let Some(id) = markview_document::details_id(url) {
			self.toggle_details(id);
			return;
		}
		let fragment = anchor::link_fragment(url);
		if anchor::link_target(url).is_empty() {
			// A bare fragment addresses the current document.
			if let Some(fragment) = fragment {
				if let Some(label) = footnote::back_label(&fragment) {
					self.return_from_footnote(label);
				} else {
					self.goto_anchor(fragment);
				}
			}
			return;
		}
		let directory = self
			.readers
			.session
			.path
			.as_deref()
			.and_then(std::path::Path::parent);
		let folder_target =
			self.folder_link(anchor::link_target(url), directory);
		let resolved =
			folder_target.map(|path| link::classify(&path)).or_else(|| {
				crate::link::resolve_for(
					url,
					directory,
					&self.readers.session.security,
				)
			});
		match resolved {
			Some(link::Target::Markdown(path)) => {
				let placement = if background {
					state::TabPlacement::Background
				} else {
					state::TabPlacement::Foreground
				};
				let trust = self.readers.session.security.origin.trust();
				if let Err(error) =
					self.readers.session.security.check_file(&path)
				{
					let required = error
						.downcast_ref::<crate::security::PermissionRequired>()
						.unwrap();
					self.ask_permission(
						required.0.clone(),
						PermissionAction::Markdown {
							path,
							anchor: fragment,
							placement,
						},
					);
				} else {
					self.open_markdown(
						path,
						fragment,
						placement,
						crate::security::Security::local(trust),
					);
				}
			}
			Some(link::Target::Remote(link)) => self.launch(&link),
			Some(link::Target::OsDirect(path)) => {
				self.launch(&path.display().to_string())
			}
			Some(link::Target::Confirm(path)) => {
				let dir = path
					.parent()
					.filter(|p| !p.as_os_str().is_empty())
					.map_or_else(|| PathBuf::from("."), PathBuf::from);
				self.interaction.overlays.modal.send(Message::Open(
					Modal::OpenLocal {
						path,
						dir,
						document_dir: directory
							.map(std::path::Path::to_path_buf),
					},
				));
				// "Open folder" is the default, and Enter activates it.
				self.interaction.widgets.focus = Some(
					crate::state::Command::Chrome(
						crate::state::ChromeCommand::ModalOpenFolder,
					)
					.widget_id(),
				);
				self.refresh_hover();
				self.redraw();
			}
			None => {
				self.status.set(
					self.preferences.values.lang().status_not_opened(url),
					true,
					4,
				);
				self.redraw();
			}
		}
	}

	pub(super) fn present_disclosures(&mut self) {
		self.readers.session.present_disclosures(self.viewport());
		self.interaction.selection_counts = None;
	}

	/// Toggles presentation while keeping the worker's retained geometry.
	pub(super) fn toggle_details(&mut self, id: u64) {
		let declared = self
			.readers
			.session
			.document
			.as_ref()
			.and_then(|document| document.details_declared(id))
			.unwrap_or(false);
		let expanded = self
			.readers
			.session
			.details_open
			.get(&id)
			.copied()
			.unwrap_or(declared);
		std::sync::Arc::make_mut(&mut self.readers.session.details_open)
			.insert(id, !expanded);
		self.present_disclosures();
		self.refresh_hover();
		self.redraw();
	}

	/// Hands an already-approved target to the operating system.
	pub(super) fn launch(&mut self, target: &str) {
		let lang = self.preferences.values.lang();
		let (message, error) = match markview_platform::open_external(target) {
			Ok(()) => (lang.status_opened(target), false),
			Err(error) => (lang.status_cannot_open(target, error), true),
		};
		self.status.set(message, error, 4);
		self.redraw();
	}

	/// Pans the wide block under the pointer. `false` means no block was
	/// there, so the caller can scroll the page instead of dropping the event.
	pub(super) fn horizontal_by(&mut self, dx: f32) -> bool {
		let consumed = markview_selection::horizontal_by(
			&self.readers.session.snapshot,
			self.view_geometry(),
			&mut self.readers.session.horizontal,
			markview_selection::Point::new(
				self.interaction.pointer.cursor().0,
				self.interaction.pointer.cursor().1,
			),
			dx,
		);
		if consumed {
			self.refresh_hover();
			self.redraw();
		}
		consumed
	}
}

#[cfg(test)]
mod tests;
