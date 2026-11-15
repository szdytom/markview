//! Passive hints use the same clipped geometry and identities as input.
use super::{App, Button, Label, widgets::UiLayout};
use crate::layout::Rect;
use markview_platform::event::{ElementState, WindowEvent};
use std::time::{Duration, Instant};
use winit::keyboard::{Key, NamedKey};

#[derive(Default)]
pub(crate) struct Model {
	pub(super) hint: markview_ui_core::Tooltip<Target>,
	keyboard: bool,
}

impl Model {
	pub(super) fn pointer(&mut self) {
		self.keyboard = false;
	}
	pub(super) fn keyboard(&mut self) {
		self.keyboard = true;
		// A fresh focus traversal can revisit the dismissed hover target.
		self.hint = Default::default();
	}
}

const SHOW_DELAY: Duration = Duration::from_millis(600);

#[derive(PartialEq)]
pub(super) struct Target {
	id: markview_ui_core::WidgetId,
	pub rect: Rect,
	pub text: Label,
}

impl Button {
	fn tooltip_label(&self) -> Option<&Label> {
		self.tooltip
			.as_ref()
			.or_else(|| self.icon.is_some().then_some(&self.label))
			.filter(|label| !label.trim().is_empty())
	}

	fn tooltip_target(&self, bounds: Rect) -> Option<Target> {
		if !self.enabled {
			return None;
		}
		let text = self.tooltip_label()?;
		let rect = self.widget().hit_rect()?.intersect(bounds)?;
		(rect.w > 0. && rect.h > 0.).then(|| Target {
			id: self.id,
			rect,
			text: text.clone(),
		})
	}
}

impl<P: super::SendEvent> App<P> {
	/// Escape dismisses the hint before reaching the control beneath it.
	pub(super) fn tooltip_event(&mut self, event: &WindowEvent) -> bool {
		match event {
			WindowEvent::KeyboardInput { event, .. } => {
				self.tooltip_key(&event.logical_key, event.state)
			}
			WindowEvent::MouseInput {
				state: ElementState::Pressed,
				..
			}
			| WindowEvent::MouseWheel { .. }
			| WindowEvent::Touch(_) => {
				self.dismiss_tooltip();
				false
			}
			_ => false,
		}
	}

	fn tooltip_key(&mut self, key: &Key, state: ElementState) -> bool {
		if state != ElementState::Pressed {
			return false;
		}
		let consumed = *key == Key::Named(NamedKey::Escape)
			&& self.interaction.overlays.tooltip.hint.visible().is_some();
		self.dismiss_tooltip();
		consumed
	}

	pub(super) fn dismiss_tooltip(&mut self) {
		if self.interaction.overlays.tooltip.hint.dismiss() {
			self.redraw();
		}
	}

	pub(super) fn update_tooltip(&mut self, layout: &UiLayout, now: Instant) {
		if self.holding_pointer()
			|| self.interaction.widgets.captured.is_some()
			|| self.interaction.panels.grab().is_some()
			|| self.folder.grab.is_some()
		{
			self.dismiss_tooltip();
			return;
		}
		let target = self.tooltip_target(layout);
		let delay = if self.interaction.overlays.tooltip.keyboard
			&& self.interaction.widgets.focus_visible
		{
			Duration::ZERO
		} else {
			SHOW_DELAY
		};
		if self
			.interaction
			.overlays
			.tooltip
			.hint
			.update(target, now, delay)
		{
			self.redraw();
		}
	}

	fn tooltip_target(&self, layout: &UiLayout) -> Option<Target> {
		if !self.frame.focused
			|| markview_platform::touch_layout()
			|| self.gestures.suppress_mouse()
			|| self.interaction.overlays.viewer_open()
			|| self.interaction.overlays.dropdown_open()
			|| self.preview.open().is_some()
		{
			return None;
		}
		let keyboard = self.interaction.overlays.tooltip.keyboard
			&& self.interaction.widgets.focus_visible;
		let widget = if keyboard {
			layout.hit.get(self.interaction.widgets.focus?)
		} else {
			layout.hit.hit(self.interaction.pointer.cursor())
		}?;
		let button = layout.buttons.iter().find(|b| b.id == widget.id)?;
		let (width, height, _) = self.dimensions();
		button.tooltip_target(Rect {
			x: 0.,
			y: 0.,
			w: width,
			h: height,
		})
	}

	pub(super) fn tooltip_tick(&mut self, now: Instant) {
		if self
			.interaction
			.overlays
			.tooltip
			.hint
			.deadline()
			.is_some_and(|at| at <= now)
		{
			let layout = self.ui_layout();
			self.update_tooltip(&layout, now);
			if self.interaction.overlays.tooltip.hint.tick(now) {
				self.redraw();
			}
		}
	}
}

#[cfg(test)]
mod tests;
