use crate::cli::Mode;
use crate::state::{Command, Grain, WheelAxis, WheelStep};
use log::{error, info};
use std::time::{Duration, Instant};
use winit::{
	dpi::PhysicalSize,
	event::{ElementState, MouseButton, WindowEvent},
	event_loop::ActiveEventLoop,
	keyboard::{Key, NamedKey},
	window::{CursorIcon, WindowId},
};

use super::{App, BOTTOM, TOP};
impl App {
	pub(super) fn handle_window_event(
		&mut self,
		event_loop: &ActiveEventLoop,
		_: WindowId,
		event: WindowEvent,
	) {
		match event {
			WindowEvent::CloseRequested => event_loop.exit(),
			WindowEvent::Resized(PhysicalSize { width, height }) => {
				self.tab_strip.reveal_active = true;
				if let Some(r) = &mut self.renderer {
					r.resize(width, height);
				}
				if width > 0 && height > 0 {
					self.reveal_panel_focus();
					self.worker.prioritize(
						self.readers.session.coverage(self.viewport()),
					);
					self.reflow_at =
						Some(Instant::now() + Duration::from_millis(40));
					self.redraw();
				}
			}
			WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
				self.tab_strip.reveal_active = true;
				info!("Display scale (DPR) changed: {scale_factor:.3}");
				if let Some(r) = &mut self.renderer {
					r.clear_raster_cache();
				}
				self.reflow_at = Some(Instant::now());
				self.redraw();
			}
			WindowEvent::Occluded(false) => self.redraw(),
			WindowEvent::ThemeChanged(_) if self.args.mode == Mode::Window => {
				self.apply_saved_settings();
			}
			WindowEvent::DroppedFile(path) => self.open(path),
			WindowEvent::ModifiersChanged(m) => {
				self.interaction.modifiers = m.state()
			}
			WindowEvent::CursorMoved { position, .. } => {
				let scale = self.dimensions().2;
				let old = self.interaction.cursor;
				let was_button = self.button_at_cursor();
				self.interaction.cursor =
					(position.x as f32 / scale, position.y as f32 / scale);
				self.move_tab_drag();
				self.drag_scrollbar();
				self.drag_panel();
				self.update_drag();
				self.refresh_hover();
				if self.interaction.panel_open
					|| was_button || self.button_at_cursor()
					|| old.1 < TOP || self.interaction.cursor.1 < TOP
					|| old.0 >= self.dimensions().0 - 16.
					|| self.interaction.cursor.0 >= self.dimensions().0 - 16.
				{
					self.redraw();
				}
			}
			WindowEvent::CursorLeft { .. } => {
				if self.tab_strip.drag.is_none()
					&& self.interaction.scrollbar.is_none()
					&& self.interaction.panel_grab.is_none()
				{
					self.interaction.cursor =
						(f32::NEG_INFINITY, f32::NEG_INFINITY);
				}
				// A scrollbar drag survives leaving the window: the implicit
				// pointer grab still reports motion and the release, so the
				// thumb keeps following the pointer past the edges. The text
				// selection gesture still ends here.
				self.interaction.pointer_down = None;
				self.interaction.drag_at = None;
				self.interaction.hover = None;
				self.interaction.hover_overflow = None;
				if let Some(w) = &self.window {
					w.set_cursor(CursorIcon::Default);
				}
				self.redraw();
			}
			WindowEvent::MouseInput {
				button: MouseButton::Middle,
				state: ElementState::Pressed,
				..
			} if !self.interaction.panel_open
				&& self.interaction.modal.is_none()
				&& !self.pointer_in_outline() =>
			{
				if let Some(index) = self.tab_at_cursor() {
					self.action(Command::CloseTab(index));
				} else if let Some(link) = self.link_at(
					self.interaction.cursor.0,
					self.interaction.cursor.1,
				) {
					self.open_link(&link, true);
				}
			}
			WindowEvent::MouseInput {
				button: MouseButton::Left,
				state: ElementState::Pressed,
				..
			} => {
				self.tab_strip.cancel_drag();
				self.interaction.focus_visible = false;
				self.interaction.pressed = None;
				self.readers.session.select_all_pending = false;
				// A new press always ends a drag left over from a release the
				// platform swallowed outside the window.
				self.interaction.scrollbar = None;
				self.interaction.panel_grab = None;
				// A confirmation owns input: only its buttons answer.
				if self.interaction.modal.is_some() {
					self.interaction.reset_clicks();
					let (x, y) = self.interaction.cursor;
					if let Some(button) = self
						.buttons()
						.into_iter()
						.find(|b| b.rect.contains(x, y))
					{
						self.interaction.focus = Some(button.action);
						self.interaction.pressed = Some(button.action);
					}
					self.redraw();
					return;
				}
				if let Some(index) = (!self.interaction.panel_open)
					.then(|| self.tab_close_at_cursor())
					.flatten()
				{
					self.interaction.reset_clicks();
					self.interaction.focus = None;
					self.action(Command::CloseTab(index));
				} else if let Some(index) = (!self.interaction.panel_open)
					.then(|| self.tab_at_cursor())
					.flatten()
				{
					self.interaction.reset_clicks();
					self.interaction.focus = None;
					self.begin_tab_drag(index);
				} else if let Some(button) =
					self.buttons().into_iter().find(|b| {
						b.rect.contains(
							self.interaction.cursor.0,
							self.interaction.cursor.1,
						)
					}) {
					self.interaction.reset_clicks();
					self.interaction.focus = Some(button.action);
					self.interaction.pressed = Some(button.action);
					self.redraw();
				} else if self.interaction.panel_open {
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
					self.interaction.focus = None;
					if self.begin_scrollbar_drag() {
						self.redraw();
					} else if self.interaction.cursor.1
						>= self.content_top() + 10.0
						&& self.interaction.cursor.1
							< self.dimensions().1 - BOTTOM - 10.0
					{
						let link = self.link_at(
							self.interaction.cursor.0,
							self.interaction.cursor.1,
						);
						if let Some(position) = self.text_at_cursor() {
							let click_count =
								if self.interaction.modifiers.shift_key() {
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
									if !self.interaction.begin_grain_selection(
										selection,
										Grain::Word,
									) {
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
									if !self.interaction.begin_grain_selection(
										selection,
										Grain::Block,
									) {
										self.interaction
											.begin_selection(position, link);
									}
								}
								_ => self
									.interaction
									.begin_selection(position, link),
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
			WindowEvent::MouseInput {
				button: MouseButton::Left,
				state: ElementState::Released,
				..
			} => {
				self.tab_strip.cancel_drag();
				let was_pressed = self.interaction.pressed.is_some();
				let hovered = self
					.buttons()
					.into_iter()
					.find(|b| {
						b.rect.contains(
							self.interaction.cursor.0,
							self.interaction.cursor.1,
						)
					})
					.map(|b| b.action);
				let action = self.interaction.release_button(hovered);
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
				let link = self.link_at(
					self.interaction.cursor.0,
					self.interaction.cursor.1,
				);
				if let Some(link) =
					self.interaction.finish_selection(link.as_deref())
				{
					self.open_link(&link, false);
				}
				self.refresh_hover();
				self.redraw();
			}
			WindowEvent::Focused(false) => {
				self.interaction.focus_visible = false;
				self.tab_strip.cancel_drag();
				self.interaction.pressed = None;
				self.interaction.pointer_down = None;
				self.interaction.drag_at = None;
				self.interaction.scrollbar = None;
				self.interaction.panel_grab = None;
				self.interaction.modifiers = Default::default();
				self.refresh_hover();
				self.redraw();
			}
			WindowEvent::MouseWheel { delta, phase, .. } => {
				if self.interaction.modal.is_some() {
					return;
				}
				let (dx, dy) = super::pointer::wheel_pixels(
					delta,
					self.wheel_notch,
					self.scroll_speed(),
					self.dimensions().2,
					self.viewport_size(),
				);
				if self.interaction.panel_open {
					if self.pointer_in_panel() {
						self.scroll_panel(-dy);
					}
					return;
				}
				if self.pointer_in_outline() {
					self.scroll_outline(-dy);
					return;
				}
				if self.scroll_tabs(if dx.abs() > dy.abs() { -dx } else { -dy })
				{
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
					match self.interaction.wheel.feed(dx, dy, now, shift, phase)
					{
						// Nothing to move: the direction is not decided yet,
						// or the event carried no motion.
						WheelStep::Pending => {}
						WheelStep::Travel(WheelAxis::Vertical, _, dy) => {
							self.scroll_wheel(-dy);
						}
						WheelStep::Travel(WheelAxis::Horizontal, dx, dy) => {
							// A sideways gesture pans the block under the
							// pointer; Shift+wheel asks for sideways motion
							// with a mostly vertical wheel. With no block to
							// pan, the vertical motion the gesture carries
							// still scrolls the page rather than being
							// dropped; a purely sideways gesture has neither a
							// target nor vertical motion to apply.
							let pan =
								if dx.abs() >= dy.abs() { -dx } else { -dy };
							if !self.horizontal_by(pan) {
								self.scroll_wheel(-dy);
							}
						}
					}
				}
			}
			WindowEvent::KeyboardInput { event, .. }
				if event.state == ElementState::Pressed =>
			{
				self.interaction.focus_visible = true;
				let command = self.interaction.modifiers.control_key()
					|| self.interaction.modifiers.super_key();
				// A confirmation answers to Tab, Enter and Escape only.
				if self.interaction.modal.is_some()
					&& (command
						|| !matches!(
							event.logical_key,
							Key::Named(
								NamedKey::Tab
									| NamedKey::Enter | NamedKey::Escape
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
									self.readers.session.select_all_pending =
										true;
									self.redraw();
									return;
								}
								self.interaction.selection =
									self.readers.session.snapshot.select_all(
										self.readers.session.accepted_revision,
									);
								self.redraw();
							}
							"c" if !self.panel_has_focus() => {
								self.copy_selection()
							}
							"v" if !self.interaction.panel_open => {
								self.paste_markdown()
							}
							"w" if !self.panel_has_focus() => self.action(
								Command::CloseTab(self.readers.active()),
							),
							"," => self.action(Command::Settings),
							"o" if self.interaction.modifiers.shift_key()
								&& !self.panel_has_focus() =>
							{
								self.action(Command::Outline)
							}
							"o" if !self.panel_has_focus() => {
								self.action(Command::Open)
							}
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
					if self.panel_has_focus()
						&& !matches!(
							event.logical_key,
							Key::Named(
								NamedKey::Tab
									| NamedKey::Enter | NamedKey::Escape
							)
						) {
						return;
					}
					match event.logical_key {
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
						Key::Named(NamedKey::Tab) => {
							let actions: Vec<Command> = self
								.focus_buttons()
								.into_iter()
								.map(|button| button.action)
								.collect();
							let backward =
								self.interaction.modifiers.shift_key();
							if let Some(action) =
								self.interaction.tab_focus(&actions, backward)
							{
								if let Command::OutlineGoto(index) = action {
									self.reveal_outline(index);
								}
								self.reveal_panel_focus();
							}
							self.redraw();
						}
						Key::Named(NamedKey::Enter) => {
							let buttons = self.buttons();
							if let Some(action) = self
								.interaction
								.enter_action(buttons.iter().map(|b| b.action))
							{
								self.action(action);
							}
						}
						Key::Named(NamedKey::Escape) => {
							self.tab_strip.cancel_drag();
							self.interaction.pressed = None;
							self.interaction.focus = None;
							self.interaction.modal = None;
							self.interaction.panel_open = false;
							self.interaction.styles_open = false;
							self.interaction.export_open = false;
							self.interaction.export_styles_open = false;
							self.interaction.selection = None;
							self.interaction.pointer_down = None;
							self.interaction.drag_at = None;
							self.interaction.scrollbar = None;
							self.interaction.panel_grab = None;
							self.interaction.close_outline();
							self.refresh_hover();
							self.redraw();
						}
						_ => {}
					}
				}
			}
			WindowEvent::RedrawRequested => {
				if let Err(e) = self.render(event_loop) {
					self.error = true;
					self.status = format!("Rendering failed: {e:#}");
					error!("{}", self.status);
					if self.args.mode == Mode::Smoke {
						self.fatal = Some(self.status.clone());
						event_loop.exit();
					}
				}
			}
			_ => {}
		}
	}
}
