use crate::cli::Mode;
use crate::state::{Command, Grain};
use log::{error, info};
use std::time::{Duration, Instant};
use winit::{
	dpi::PhysicalSize,
	event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
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
				self.interaction.cursor =
					(position.x as f32 / scale, position.y as f32 / scale);
				self.move_tab_drag();
				self.drag_scrollbar();
				self.update_drag();
				self.refresh_hover();
				if self.interaction.panel_open
					|| old.1 < TOP || self.interaction.cursor.1 < TOP
					|| old.0 >= self.dimensions().0 - 16.
					|| self.interaction.cursor.0 >= self.dimensions().0 - 16.
				{
					self.redraw();
				}
			}
			WindowEvent::CursorLeft { .. } => {
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
				&& self.interaction.modal.is_none() =>
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
				self.readers.session.select_all_pending = false;
				// A new press always ends a drag left over from a release the
				// platform swallowed outside the window.
				self.interaction.scrollbar = None;
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
						self.action(button.action);
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
					self.action(button.action);
				} else if self.interaction.panel_open {
					self.interaction.reset_clicks();
					if !self.pointer_in_panel() {
						self.action(Command::Settings);
					}
				} else if !self.pointer_in_panel() {
					self.interaction.focus = None;
					if self.begin_scrollbar_drag() {
						self.redraw();
					} else if self.interaction.cursor.1
						>= self.content_top() + 10.0
						&& self.interaction.cursor.1
							< self.dimensions().1 - BOTTOM - 10.0
					{
						if let Some(position) = self.text_at_cursor() {
							let click_count =
								if self.interaction.modifiers.shift_key() {
									self.interaction.reset_clicks();
									1
								} else {
									self.interaction.click_count(Instant::now())
								};
							let link = self.link_at(
								self.interaction.cursor.0,
								self.interaction.cursor.1,
							);
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
				self.interaction.pressed = None;
				self.interaction.scrollbar = None;
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
				self.tab_strip.cancel_drag();
				self.interaction.pressed = None;
				self.interaction.pointer_down = None;
				self.interaction.drag_at = None;
				self.interaction.scrollbar = None;
				self.interaction.modifiers = Default::default();
				self.refresh_hover();
				self.redraw();
			}
			WindowEvent::MouseWheel { delta, .. } => {
				if self.interaction.panel_open
					|| self.interaction.modal.is_some()
				{
					return;
				}
				let (dx, dy) = match delta {
					MouseScrollDelta::LineDelta(x, y) => (x * 42.0, y * 42.0),
					MouseScrollDelta::PixelDelta(p) => (
						p.x as f32 / self.dimensions().2,
						p.y as f32 / self.dimensions().2,
					),
				};
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
				} else if self.interaction.modifiers.shift_key()
					|| dx.abs() > dy.abs()
				{
					self.horizontal_by(if dx.abs() > dy.abs() {
						-dx
					} else {
						-dy
					});
				} else {
					self.scroll_by(-dy);
				}
			}
			WindowEvent::KeyboardInput { event, .. }
				if event.state == ElementState::Pressed =>
			{
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
							"o" if !self.panel_has_focus() => {
								self.action(Command::Open)
							}
							"t" => self.action(Command::Styles),
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
						Key::Named(NamedKey::ArrowDown) => self.scroll_by(42.0),
						Key::Named(NamedKey::ArrowUp) => self.scroll_by(-42.0),
						Key::Named(NamedKey::PageDown | NamedKey::Space) => {
							self.scroll_by(self.viewport() * 0.9)
						}
						Key::Named(NamedKey::PageUp) => {
							self.scroll_by(-self.viewport() * 0.9)
						}
						Key::Named(NamedKey::Home) => {
							self.scroll_by(f32::NEG_INFINITY)
						}
						Key::Named(NamedKey::End) => {
							self.scroll_by(f32::INFINITY)
						}
						Key::Named(NamedKey::ArrowLeft) => {
							self.horizontal_by(-42.0)
						}
						Key::Named(NamedKey::ArrowRight) => {
							self.horizontal_by(42.0)
						}
						Key::Named(NamedKey::Tab) => {
							let buttons = self.buttons();
							let current = buttons.iter().position(|b| {
								Some(b.action) == self.interaction.focus
							});
							let index = match current {
								None => {
									if self.interaction.modifiers.shift_key() {
										buttons.len() - 1
									} else {
										0
									}
								}
								Some(i) => {
									(i + if self
										.interaction
										.modifiers
										.shift_key()
									{
										buttons.len() - 1
									} else {
										1
									}) % buttons.len()
								}
							};
							self.interaction.focus =
								Some(buttons[index].action);
							self.redraw();
						}
						Key::Named(NamedKey::Enter) => {
							if let Some(action) = self.interaction.focus
								&& self
									.buttons()
									.iter()
									.any(|b| b.action == action)
							{
								self.action(action);
							}
						}
						Key::Named(NamedKey::Escape) => {
							self.tab_strip.cancel_drag();
							self.interaction.focus = None;
							self.interaction.modal = None;
							self.interaction.panel_open = false;
							self.interaction.styles_open = false;
							self.interaction.selection = None;
							self.interaction.pointer_down = None;
							self.interaction.drag_at = None;
							self.interaction.scrollbar = None;
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
