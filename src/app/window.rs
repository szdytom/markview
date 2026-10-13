use crate::cli::Mode;
use crate::state::{
	Command, Grain, Selection, WheelAxis, WheelGesture, WheelStep,
};
use log::{error, info};
use std::time::{Duration, Instant};
use winit::{
	dpi::PhysicalSize,
	event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
	event_loop::ActiveEventLoop,
	keyboard::{Key, NamedKey},
	window::{CursorIcon, Fullscreen, Window, WindowId},
};

use super::{App, TOP, frame::Caption};

/// What the app asks of the loop it runs under.
///
/// `ActiveEventLoop` is the only real implementation; a test drives the app
/// without a window server by handing it a stub instead, which is why the
/// handlers below name this rather than the concrete type.
pub(super) trait Loop {
	fn exit(&self);
	fn window_drag(
		&self,
		window: Option<&Window>,
		direction: Option<winit::window::ResizeDirection>,
	) {
		if let Some(window) = window {
			let result = direction.map_or_else(
				|| window.drag_window(),
				|direction| window.drag_resize_window(direction),
			);
			if let Err(error) = result {
				log::debug!("Cannot start window drag: {error}");
			}
		}
	}
	fn window_action(&self, window: Option<&Window>, caption: Caption) {
		if caption == Caption::Close {
			self.exit();
		} else if let Some(window) = window {
			match caption {
				Caption::Minimize => window.set_minimized(true),
				Caption::Expand if markview_platform::native_buttons() => {
					window.set_fullscreen(
						window
							.fullscreen()
							.is_none()
							.then_some(Fullscreen::Borderless(None)),
					)
				}
				Caption::Expand => window.set_maximized(!window.is_maximized()),
				Caption::Close => unreachable!(),
			}
		}
	}
}
impl Loop for ActiveEventLoop {
	fn exit(&self) {
		ActiveEventLoop::exit(self);
	}
}

impl<P: super::SendEvent> App<P> {
	pub(super) fn android_back(&mut self) {
		if self.readers.session.search.open {
			self.close_search();
		} else if self.interaction.panel_open()
			|| self.interaction.viewer.is_some()
			|| self.interaction.outline_open
			|| self.interaction.modal.is_some()
		{
			self.key_pressed(&Key::Named(NamedKey::Escape));
		} else if let Err(error) =
			markview_platform::android::call_string("backgroundTask", "")
		{
			log::warn!("Android back: {error}");
		}
	}

	pub(super) fn handle_window_event(
		&mut self,
		event_loop: &impl Loop,
		_: WindowId,
		event: WindowEvent,
	) {
		if self.before_window_input(event_loop, &event)
			|| self.window_state_event(event_loop, &event)
		{
			return;
		}
		match event {
			WindowEvent::Touch(touch) => {
				self.dispatch_touch(event_loop, touch);
			}
			WindowEvent::PinchGesture { .. } => {
				// TODO: implement viewport zoom without changing the document layout.
			}
			WindowEvent::DroppedFile(path) => self.open(path),
			WindowEvent::ModifiersChanged(m) => {
				self.interaction.modifiers = m.state()
			}
			WindowEvent::CursorMoved { position, .. } => {
				self.pointer_moved(position);
			}
			WindowEvent::CursorLeft { .. } => {
				self.pointer_left();
			}
			WindowEvent::MouseInput {
				button: MouseButton::Middle,
				state: ElementState::Pressed,
				..
			} => {
				self.middle_pressed();
			}
			WindowEvent::MouseInput {
				button: MouseButton::Left,
				state: ElementState::Pressed,
				..
			} => {
				self.pointer_pressed();
			}
			WindowEvent::MouseInput {
				button: MouseButton::Left,
				state: ElementState::Released,
				..
			} => {
				self.pointer_released();
			}
			WindowEvent::MouseWheel { delta, phase, .. } => {
				self.scroll_event(delta, phase);
			}
			WindowEvent::KeyboardInput { event, .. }
				if event.state == ElementState::Pressed =>
			{
				self.dispatch_key(event_loop, &event);
			}
			_ => {}
		}
	}
	fn before_window_input(
		&mut self,
		event_loop: &impl Loop,
		event: &WindowEvent,
	) -> bool {
		if markview_platform::touch_layout()
			&& let WindowEvent::KeyboardInput { event, .. } = event
			&& event.state == ElementState::Pressed
			&& event.logical_key == Key::Named(NamedKey::BrowserBack)
		{
			self.android_back();
			return true;
		}
		if self.frame_event(event_loop, event) || self.viewer_event(event) {
			return true;
		}
		if self.interaction.modal.is_none()
			&& let WindowEvent::KeyboardInput { event, .. } = event
			&& event.state == ElementState::Pressed
			&& self.search_window_key(event)
		{
			return true;
		}
		self.input_event(event)
	}

	fn search_window_key(&mut self, event: &winit::event::KeyEvent) -> bool {
		let primary = if markview_platform::command_key() {
			self.interaction.modifiers.super_key()
		} else {
			self.interaction.modifiers.control_key()
				&& !self.interaction.modifiers.alt_key()
		};
		if primary
			&& matches!(&event.logical_key, Key::Character(c) if c.eq_ignore_ascii_case("f"))
		{
			self.open_search();
			return true;
		}
		if self.readers.session.search.open
			&& !self.readers.session.search.input.is_composing()
		{
			if event.logical_key == Key::Named(NamedKey::F3) {
				self.navigate_search(self.interaction.modifiers.shift_key());
				return true;
			}
			if event.logical_key == Key::Named(NamedKey::Escape) {
				self.close_search();
				return true;
			}
		}
		false
	}

	fn dispatch_touch(
		&mut self,
		event_loop: &impl Loop,
		touch: winit::event::Touch,
	) {
		let action = self.handle_touch(touch);
		if markview_platform::client_resize() {
			let drag = match action {
				Some(super::gestures::TouchAction::Drag(direction)) => {
					Some(direction)
				}
				_ => None,
			};
			if let Some(frame) = &mut self.touch_frame {
				frame.touch(touch, drag);
			} else if let Some(direction) = drag {
				event_loop.window_drag(self.window.as_deref(), direction);
			}
		}
		if let Some(super::gestures::TouchAction::Caption(caption)) = action {
			event_loop.window_action(self.window.as_deref(), caption);
		}
	}

	fn window_resized(&mut self, width: u32, height: u32) {
		self.cancel_gestures();
		self.tab_strip.reveal_active = true;
		let scale = self.dimensions().2;
		if let Some(dm) = self.dm.as_mut() {
			// The layout changes under the gesture, so it is
			// abandoned and the offset clamps with the reflow.
			dm.abandon();
			dm.resize(width, height, scale);
		}
		if let Some(r) = &mut self.renderer {
			r.resize(width, height);
		}
		if width > 0 && height > 0 {
			self.update_media();
			self.reveal_panel_focus();
			self.worker
				.prioritize(self.readers.session.coverage(self.viewport()));
			self.reflow_at = Some(Instant::now() + Duration::from_millis(40));
			self.redraw();
		}
	}

	fn scale_changed(&mut self, scale_factor: f64) {
		self.cancel_gestures();
		self.tab_strip.reveal_active = true;
		info!("Display scale (DPR) changed: {scale_factor:.3}");
		if let (Some(dm), Some(window)) =
			(self.dm.as_mut(), self.window.as_ref())
		{
			dm.abandon();
			let size = window.inner_size();
			dm.resize(size.width, size.height, scale_factor as f32);
		}
		if let Some(r) = &mut self.renderer {
			r.clear_raster_cache();
		}
		self.reflow_at = Some(Instant::now());
		self.redraw();
	}

	fn pointer_moved(&mut self, position: winit::dpi::PhysicalPosition<f64>) {
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
		if point != self.interaction.cursor {
			let old = self.interaction.cursor;
			let was_button = self.button_at_cursor();
			self.interaction.cursor = point;
			if let Some((_, (px, py))) = self.interaction.pressed_image.as_ref()
				&& (point.0 - px).hypot(point.1 - py) > 3.
			{
				self.interaction.pressed_image = None;
			}
			self.hover_dropdown();
			self.move_tab_drag();
			self.drag_scrollbar();
			self.drag_panel();
			self.update_drag();
			self.refresh_hover();
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

	fn pointer_left(&mut self) {
		if self.tab_strip.drag.is_none()
			&& self.interaction.scrollbar.is_none()
			&& self.interaction.panel_grab.is_none()
		{
			self.interaction.cursor = (f32::NEG_INFINITY, f32::NEG_INFINITY);
		}
		// A scrollbar drag survives leaving the window: the implicit
		// pointer grab still reports motion and the release, so the
		// thumb keeps following the pointer past the edges. The text
		// selection gesture still ends here.
		self.interaction.pointer_down = None;
		self.interaction.pressed_image = None;
		self.interaction.drag_at = None;
		self.interaction.hover = None;
		self.interaction.hover_overflow = None;
		if let Some(w) = &self.window {
			w.set_cursor(CursorIcon::Default);
		}
		self.redraw();
	}

	fn middle_pressed(&mut self) {
		if !self.interaction.panel_open()
			&& self.interaction.modal.is_none()
			&& !self.pointer_in_outline()
		{
			if let Some(index) = self.tab_at_cursor() {
				self.action(Command::CloseTab(index));
			} else if let Some(link) = self
				.link_at(self.interaction.cursor.0, self.interaction.cursor.1)
			{
				self.open_link(&link, true);
			}
		}
	}

	#[cfg_attr(
		any(),
		markview::allow_complexity(
			reason = "Keep press ownership and capture transitions ordered across modal, dropdown, tab, scrollbar, outline and selection targets; the first owner consumes the press."
		)
	)]
	fn pointer_pressed(&mut self) {
		self.cancel_gestures();
		self.tab_strip.cancel_drag();
		self.interaction.widgets.focus_visible = false;
		self.interaction.widgets.captured = None;
		self.interaction.pressed_image = None;
		self.readers.session.select_all_pending = false;
		// A new press always ends a drag left over from a release the
		// platform swallowed outside the window.
		self.interaction.scrollbar = None;
		self.interaction.panel_grab = None;
		// A confirmation owns input: only its buttons answer.
		if self.interaction.modal.is_some() {
			self.interaction.reset_clicks();
			let buttons = self.buttons();
			self.press_widget(&buttons);
			self.redraw();
			return;
		}
		// An open option list owns input the way a confirmation does: a
		// press on one of its own options picks it, and a press anywhere
		// else closes it without reaching the page it covered. Nothing
		// behind it answers the same click, so the press is always the
		// last word and the release that follows has nothing left to
		// dispatch.
		if self.interaction.dropdown.is_some() {
			self.interaction.reset_clicks();
			let buttons = self.dropdown_buttons();
			self.press_widget(&buttons);
			if self.interaction.widgets.captured.is_none() {
				self.close_dropdown();
			}
			self.redraw();
			return;
		}
		let (x, y) = self.interaction.cursor;
		let on_outline_toggle = self.buttons().into_iter().any(|button| {
			button.action == Command::Outline && button.rect.contains(x, y)
		});
		if self.interaction.close_outline_if_outside(
			self.pointer_in_outline(),
			on_outline_toggle,
		) {
			self.redraw();
		}
		if let Some(index) = (!self.interaction.panel_open())
			.then(|| self.tab_close_at_cursor())
			.flatten()
		{
			self.interaction.reset_clicks();
			self.interaction.widgets.focus = None;
			self.action(Command::CloseTab(index));
		} else if let Some(index) = (!self.interaction.panel_open())
			.then(|| self.tab_at_cursor())
			.flatten()
		{
			self.interaction.reset_clicks();
			self.interaction.widgets.focus = None;
			self.begin_tab_drag(index);
		} else if self.button_at_cursor() {
			self.interaction.reset_clicks();
			let buttons = self.buttons();
			self.press_widget(&buttons);
			self.redraw();
		} else if self.interaction.panel_open() {
			// A panel draws over the drawer, so it answers first: its
			// scrollbar drag and its outside-click dismissal must work
			// where the two overlap.
			if self.begin_panel_drag() {
				self.redraw();
				return;
			}
			self.interaction.reset_clicks();
			if !self.pointer_in_panel() {
				self.action(Command::Settings);
			}
		} else if self.pointer_in_outline() {
			// The drawer owns presses inside it; one between its rows
			// must not start a document selection underneath.
			self.interaction.reset_clicks();
		} else if !self.pointer_in_panel() {
			self.interaction.widgets.focus = None;
			if self.begin_scrollbar_drag() {
				self.redraw();
			} else if self.interaction.cursor.1 >= self.content_top() + 10.0
				&& self.interaction.cursor.1
					< self.dimensions().1 - self.bottom() - 10.0
			{
				let link = self.link_at(
					self.interaction.cursor.0,
					self.interaction.cursor.1,
				);
				self.interaction.pressed_image = self
					.image_at_cursor()
					.map(|(src, _)| (src, self.interaction.cursor));
				self.interaction.dragged = false;
				if let Some(position) = self.text_at_cursor() {
					let click_count = if self.interaction.modifiers.shift_key()
					{
						self.interaction.reset_clicks();
						1
					} else {
						self.interaction.click_count(Instant::now())
					};
					match click_count {
						2 => {
							let selection = self
								.readers
								.session
								.snapshot
								.select_word_at(position);
							if !self
								.interaction
								.begin_grain_selection(selection, Grain::Word)
							{
								self.interaction
									.begin_selection(position, link);
							}
						}
						3 => {
							let selection = self
								.readers
								.session
								.snapshot
								.select_block_at(position);
							if !self
								.interaction
								.begin_grain_selection(selection, Grain::Block)
							{
								self.interaction
									.begin_selection(position, link);
							}
						}
						_ => self.interaction.begin_selection(position, link),
					}
				} else if let Some(link) = link {
					// A summary line's marker carries no text, but the
					// whole line is still its control.
					self.interaction.begin_link_press(link);
				}
				self.redraw();
			}
		}
	}

	fn pointer_released(&mut self) {
		self.tab_strip.cancel_drag();
		let pressed_image = self.interaction.pressed_image.take();
		let was_pressed = self.interaction.widgets.captured.is_some();
		let action = self.release_widget();
		self.interaction.scrollbar = None;
		self.interaction.panel_grab = None;
		if was_pressed {
			if let Some(action) = action {
				self.action(action);
			}
			self.refresh_hover();
			self.redraw();
			return;
		}
		if self.interaction.modal.is_some() {
			self.redraw();
			return;
		}
		let link =
			self.link_at(self.interaction.cursor.0, self.interaction.cursor.1);
		if let Some(link) = self.interaction.finish_selection(link.as_deref()) {
			self.open_link(&link, false);
		} else if !self.interaction.dragged
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

	fn window_unfocused(&mut self) {
		if let Some(native) = &self.native_frame {
			native.cancel();
		}
		self.frame.focused = false;
		self.frame.hover = None;
		self.frame.pressed = None;
		self.frame.last_click = None;
		self.cancel_gestures();
		// The gesture dies with the focus: the OS cancels it, and
		// the page is still when the window is revisited.
		self.abandon_dm();
		self.interaction.widgets.focus_visible = false;
		self.tab_strip.cancel_drag();
		self.interaction.widgets.captured = None;
		self.interaction.pointer_down = None;
		self.interaction.pressed_image = None;
		self.interaction.drag_at = None;
		self.interaction.scrollbar = None;
		self.interaction.panel_grab = None;
		self.interaction.modifiers = Default::default();
		self.refresh_hover();
		self.redraw();
	}

	#[cfg_attr(
		any(),
		markview::allow_complexity(
			reason = "Keep wheel ownership, axis locking and native or coasting input together; interrupted streams must be abandoned before a new target receives deltas."
		)
	)]
	fn scroll_event(
		&mut self,
		delta: MouseScrollDelta,
		phase: winit::event::TouchPhase,
	) {
		if self.readers.session.search.open
			&& self.interaction.cursor.1 >= self.dimensions().1 - self.bottom()
		{
			return;
		}
		if self.interaction.modal.is_some() {
			return;
		}
		// An open option list owns the wheel wherever the pointer
		// rests, so the page behind it never scrolls out from under the
		// row the list hangs from. One notch of travel moves the list
		// one option, whatever the reader's own scroll speed is. A
		// chord with a modifier belongs to the reader, as it does for
		// the list's own keys.
		//
		// Owning the wheel also means the page's gesture is fed
		// nothing while the list is open, so it is put away here: the
		// wheel event that follows the list's close starts a fresh
		// page gesture instead of waking a stale one.
		if self.interaction.dropdown.is_some()
			&& !self.interaction.modifiers.control_key()
			&& !self.interaction.modifiers.super_key()
		{
			self.interaction.wheel = WheelGesture::default();
			// One detent is one option, so a line delta counts
			// options directly: the desktop's lines-per-notch value
			// sizes the page's scroll, not the list's highlight, and
			// letting it in would move three options per detent on
			// the platforms that ship a three-line notch. A
			// trackpad's pixel travel still accumulates to a notch.
			let travel = match delta {
				MouseScrollDelta::LineDelta(_, y) => y * super::dropdown::NOTCH,
				MouseScrollDelta::PixelDelta(_) => {
					super::pointer::wheel_pixels(
						delta,
						self.wheel_notch,
						1.0,
						self.dimensions().2,
						self.viewport_size(),
					)
					.1
				}
			};
			self.wheel_dropdown(-travel);
			return;
		}
		let (dx, dy) = super::pointer::wheel_pixels(
			delta,
			self.wheel_notch,
			self.scroll_speed(),
			self.dimensions().2,
			self.viewport_size(),
		);
		if matches!(delta, MouseScrollDelta::PixelDelta(_)) {
			if self.interaction.modifiers.control_key()
				|| self.interaction.modifiers.super_key()
			{
				// TODO: implement viewport zoom without changing the document layout.
			} else {
				// macOS delivers its momentum inside the pixel deltas;
				// every other desktop's pixel stream asks the reader
				// to coast on its own when the stream ends.
				let inertia = if markview_platform::native_scroll_inertia() {
					super::gestures::Inertia::Native
				} else {
					super::gestures::Inertia::Synthesized
				};
				self.trackpad_scroll(dx, dy, phase, inertia);
			}
			return;
		}
		self.cancel_gestures();
		if self.interaction.panel_open() {
			if self.pointer_in_panel() {
				self.scroll_panel(-dy);
			}
			return;
		}
		if self.pointer_in_outline() {
			self.scroll_outline(-dy);
			return;
		}
		if self.scroll_tabs(if dx.abs() > dy.abs() { -dx } else { -dy }) {
			return;
		}
		if self.interaction.modifiers.control_key()
			|| self.interaction.modifiers.super_key()
		{
			self.action(if dy > 0.0 {
				Command::Larger
			} else {
				Command::Smaller
			});
		} else {
			let now = Instant::now();
			let shift = self.interaction.modifiers.shift_key();
			// Windows hands a touchpad's inertia over in packets the
			// reader has to give momentum of its own; a whole-detent
			// wheel, and every other desktop's stream, is one eased
			// step per event. A whole detent inside a live stream is
			// the OS rounding a packet, not a wheel: the stream
			// adopts it, or easing it would kill the momentum.
			let packets = markview_platform::wheel_packets()
				&& (super::pointer::high_resolution(delta)
					|| self.readers.session.wheel_stream_alive(now));
			let travel = if packets {
				Self::scroll_wheel_packet
			} else {
				Self::scroll_wheel
			};
			match self.interaction.wheel.feed(dx, dy, now, shift, phase) {
				// Nothing to move: the direction is not decided yet,
				// or the event carried no motion.
				WheelStep::Pending => {}
				WheelStep::Travel(WheelAxis::Vertical, _, dy) => {
					travel(self, -dy);
				}
				WheelStep::Travel(WheelAxis::Horizontal, dx, dy) => {
					// A sideways gesture pans the block under the
					// pointer; Shift+wheel asks for sideways motion
					// with a mostly vertical wheel. With no block to
					// pan, the vertical motion the gesture carries
					// still scrolls the page rather than being
					// dropped; a purely sideways gesture has neither a
					// target nor vertical motion to apply.
					let pan = if dx.abs() >= dy.abs() { -dx } else { -dy };
					if !self.horizontal_by(pan) {
						travel(self, -dy);
					}
				}
			}
		}
	}

	#[cfg_attr(
		any(),
		markview::allow_complexity(
			reason = "Keep the flat application shortcut map beside per-command focus guards; pending select-all and modal ownership must precede command effects."
		)
	)]
	fn dispatch_key(
		&mut self,
		event_loop: &impl Loop,
		event: &winit::event::KeyEvent,
	) {
		self.cancel_gestures();
		self.interaction.widgets.focus_visible = true;
		let command = self.interaction.modifiers.control_key()
			|| self.interaction.modifiers.super_key();
		// A confirmation answers to Tab, Enter and Escape only.
		if self.interaction.modal.is_some()
			&& (command
				|| !matches!(
					event.logical_key,
					Key::Named(
						NamedKey::Tab | NamedKey::Enter | NamedKey::Escape
					)
				)) {
			return;
		}
		if command {
			if let Key::Character(c) = &event.logical_key {
				match c.to_lowercase().as_str() {
					"a" if !self.panel_has_focus() => {
						if self.readers.session.layout_pending {
							self.interaction.clear_selection();
							self.readers.session.select_all_pending = true;
							self.redraw();
							return;
						}
						self.interaction.selection =
							self.readers.session.snapshot.select_all(
								self.readers.session.accepted_revision,
							);
						self.redraw();
					}
					"c" if !self.panel_has_focus() => self.copy_selection(),
					"v" if !self.interaction.panel_open() => {
						self.paste_markdown()
					}
					"w" if !self.panel_has_focus() => {
						self.action(Command::CloseTab(self.readers.active()))
					}
					"," => self.action(Command::Settings),
					"b" if !self.panel_has_focus() => {
						self.action(Command::Outline)
					}
					"o" if !self.panel_has_focus() => {
						self.action(Command::Open)
					}
					"r" if markview_platform::folder_reveal()
						&& self.interaction.modifiers.alt_key()
						&& !self.interaction.modifiers.shift_key()
						&& !self.panel_has_focus() =>
					{
						self.action(Command::RevealFolder)
					}
					"n" => self.action(Command::NewPage),
					"t" => self.action(Command::Styles),
					"e" => self.action(Command::Export),
					"-" => self.action(Command::Smaller),
					"+" | "=" => self.action(Command::Larger),
					"[" => self.action(Command::Narrower),
					"]" => self.action(Command::Wider),
					"l" => self.action(Command::Align),
					"h" => self.action(Command::Hyphens),
					"q" => event_loop.exit(),
					_ => {}
				}
			}
		} else {
			self.press_unmodified(event_loop, &event.logical_key);
		}
	}

	fn redraw_event(&mut self, event_loop: &impl Loop) {
		if let Err(e) = self.render(event_loop) {
			self.error = true;
			self.status = self
				.preferences
				.values
				.lang()
				.status_rendering_failed(format!("{e:#}"));
			error!("{}", self.status);
			if self.args.mode == Mode::Smoke {
				self.fatal = Some(self.status.clone());
				event_loop.exit();
			}
		}
	}

	fn window_state_event(
		&mut self,
		event_loop: &impl Loop,
		event: &WindowEvent,
	) -> bool {
		match event {
			WindowEvent::CloseRequested => event_loop.exit(),
			WindowEvent::Moved(_) => self.frame.last_click = None,
			WindowEvent::Resized(PhysicalSize { width, height }) => {
				self.window_resized(*width, *height);
			}
			WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
				self.scale_changed(*scale_factor);
			}
			WindowEvent::Occluded(false) => self.redraw(),
			WindowEvent::ThemeChanged(_) if self.args.mode == Mode::Window => {
				self.apply_saved_settings();
			}
			WindowEvent::Focused(true) => {
				self.frame.focused = true;
				self.redraw();
			}
			WindowEvent::Focused(false) => {
				self.window_unfocused();
			}
			WindowEvent::RedrawRequested => {
				self.redraw_event(event_loop);
			}
			_ => return false,
		}
		true
	}

	/// Routes one unmodified key press through the owners of input, then
	/// answers the quit shortcut itself.
	///
	/// A chord with a modifier never reaches here; the caller answers those
	/// first. Inside, an open option list and the settings panel come before
	/// the shortcut, so a key they own never quits the reader.
	pub(super) fn press_unmodified(
		&mut self,
		event_loop: &impl Loop,
		key: &Key,
	) {
		if !self.key_pressed(key) && *key == Key::Character("q".into()) {
			event_loop.exit();
		}
	}
	/// The viewer captures input after window controls.
	fn viewer_event(&mut self, event: &WindowEvent) -> bool {
		if self.interaction.viewer.is_none() {
			return false;
		}
		if matches!(event, WindowEvent::Touch(_)) {
			return false;
		}
		self.refresh_viewer();
		let (width, height, scale) = self.dimensions();
		let insets = self.insets();
		let viewer = self.interaction.viewer.as_mut().unwrap();
		match event {
			WindowEvent::CursorMoved { position, .. } => {
				self.interaction.cursor = (
					position.x as f32 / scale - insets[0],
					position.y as f32 / scale - insets[1],
				);
				viewer.move_pointer(self.interaction.cursor, (width, height));
			}
			WindowEvent::CursorLeft { .. } => {
				if viewer.grab.is_none() {
					self.interaction.cursor =
						(f32::NEG_INFINITY, f32::NEG_INFINITY);
				}
			}
			WindowEvent::MouseInput {
				button: MouseButton::Left,
				state,
				..
			} => match state {
				ElementState::Pressed => {
					viewer.begin_press(self.interaction.cursor)
				}
				ElementState::Released => {
					if viewer.finish_press() {
						self.interaction.viewer = None;
					}
				}
			},
			WindowEvent::MouseWheel { delta, .. } => {
				let factor = match delta {
					MouseScrollDelta::LineDelta(_, y) => (0.2 * y).exp(),
					MouseScrollDelta::PixelDelta(p) => {
						(p.y as f32 / 240.).exp()
					}
				};
				viewer.zoom_at(
					factor,
					self.interaction.cursor,
					(width, height),
				);
			}
			WindowEvent::KeyboardInput { event, .. } => {
				if event.state == ElementState::Pressed {
					self.key_pressed(&event.logical_key);
				}
			}
			WindowEvent::MouseInput { .. }
			| WindowEvent::Ime(_)
			| WindowEvent::PinchGesture { .. }
			| WindowEvent::PanGesture { .. }
			| WindowEvent::RotationGesture { .. }
			| WindowEvent::DoubleTapGesture { .. } => return true,
			WindowEvent::Focused(false) => {
				viewer.cancel_press();
				return false;
			}
			_ => return false,
		}
		self.refresh_hover();
		self.redraw();
		true
	}

	/// Routes one unmodified key press, with no window server involved,
	/// reporting whether an owner took the key.
	///
	/// A chord with a modifier is the reader's and never reaches here; the
	/// caller answers those first. An open option list comes before the
	/// settings panel's own guard, which would otherwise swallow the keys
	/// that move it. A key no owner takes comes back as `false`, which is
	/// what lets the caller answer it last.
	pub(super) fn key_pressed(&mut self, key: &Key) -> bool {
		if self.overlay_key(key) {
			return true;
		}
		match key {
			Key::Character(c)
				if c == "/"
					&& !self.interaction.modifiers.control_key()
					&& !self.interaction.modifiers.super_key()
					&& !self.interaction.modifiers.alt_key() =>
			{
				self.open_new_search();
			}
			Key::Named(NamedKey::ArrowDown)
				if self.interaction.outline_owns_input() =>
			{
				self.move_outline(1)
			}
			Key::Named(NamedKey::ArrowUp)
				if self.interaction.outline_owns_input() =>
			{
				self.move_outline(-1)
			}
			Key::Named(NamedKey::Tab) => {
				self.navigate_focus();
			}
			Key::Named(NamedKey::Enter) => {
				let buttons = self.buttons();
				if let Some(action) = self.interaction.enter_action(
					buttons
						.iter()
						.filter(|b| b.enabled)
						.map(|b| (b.id, b.action)),
				) {
					self.action(action);
				}
			}
			Key::Named(NamedKey::Escape) => {
				self.escape_key();
			}
			_ => return self.document_key(key),
		}
		true
	}
	fn overlay_key(&mut self, key: &Key) -> bool {
		if self.interaction.viewer.is_some() {
			if key == &Key::Named(NamedKey::Escape) {
				self.interaction.viewer = None;
				self.refresh_hover();
				self.redraw();
			}
			return true;
		}
		// An open option list owns the keys while it is up. A key it hands
		// back still reaches the page behind it.
		if self.interaction.dropdown.is_some() && self.dropdown_key(key) {
			self.redraw();
			return true;
		}
		if self.panel_has_focus()
			&& !matches!(
				key,
				Key::Named(NamedKey::Tab | NamedKey::Enter | NamedKey::Escape)
			) {
			return true;
		}
		false
	}

	fn document_key(&mut self, key: &Key) -> bool {
		match key {
			Key::Named(NamedKey::ArrowDown) => {
				self.scroll_step(self.line_step())
			}
			Key::Named(NamedKey::ArrowUp) => {
				self.scroll_step(-self.line_step())
			}
			Key::Named(NamedKey::PageDown | NamedKey::Space) => {
				self.scroll_step(self.viewport() * 0.9)
			}
			Key::Named(NamedKey::PageUp) => {
				self.scroll_step(-self.viewport() * 0.9)
			}
			Key::Named(NamedKey::Home) => self.scroll_bound(false),
			Key::Named(NamedKey::End) => self.scroll_bound(true),
			Key::Named(NamedKey::ArrowLeft) => {
				self.horizontal_by(-self.line_step());
			}
			Key::Named(NamedKey::ArrowRight) => {
				self.horizontal_by(self.line_step());
			}
			_ => return false,
		}
		true
	}

	fn navigate_focus(&mut self) {
		let backward = self.interaction.modifiers.shift_key();
		if let Some(action) = self.navigate_widgets(backward) {
			if let Command::OutlineGoto(index) | Command::OutlineToggle(index) =
				action
			{
				self.reveal_outline(index);
			}
			self.reveal_panel_focus();
		}
		self.redraw();
	}

	fn escape_key(&mut self) {
		if let Some(native) = &self.native_frame {
			native.cancel();
		}
		if !self.interaction.panel_open()
			&& self.interaction.modal.is_none()
			&& let Some(window) = &self.window
			&& window.fullscreen().is_some()
		{
			window.set_fullscreen(None);
		}
		self.frame.pressed = None;
		self.frame.last_click = None;
		self.tab_strip.cancel_drag();
		self.interaction.widgets.captured = None;
		self.interaction.widgets.focus = None;
		self.interaction.modal = None;
		self.interaction.show_panel(crate::state::PanelPage::Closed);
		self.interaction.selection = None;
		self.interaction.pointer_down = None;
		self.interaction.drag_at = None;
		self.interaction.scrollbar = None;
		self.interaction.panel_grab = None;
		self.interaction.close_outline();
		self.refresh_hover();
		self.redraw();
	}
}
