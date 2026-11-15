use super::*;
use crate::{
	app::{Event, SendEvent, window::Loop},
	cli::{LaunchOptions, Mode},
	state::{ChromeCommand, Command, DocumentCommand},
};
use markview_platform::event::{
	DeviceId, ElementState, MouseButton, WindowEvent,
};
use winit::{dpi::PhysicalPosition, window::WindowId};

#[derive(Clone)]
struct Proxy;
impl SendEvent for Proxy {
	fn try_send(&self, _: Event) -> bool {
		true
	}
}
struct StubLoop;
impl Loop for StubLoop {
	fn exit(&self) {
		panic!("tooltip input must keep the window open");
	}
}

fn app() -> App<Proxy> {
	let mut app = App::new(
		LaunchOptions {
			mode: Mode::Smoke,
			offline: true,
			width: 800,
			height: 600,
			options: crate::test_support::options(),
			..Default::default()
		},
		Proxy,
	);
	app.preferences.values = Default::default();
	app.frame.focused = true;
	app
}

fn hover(app: &mut App<Proxy>, action: Command, now: Instant) -> Button {
	let layout = app.ui_layout();
	let button = layout
		.buttons
		.iter()
		.find(|b| b.action == action)
		.unwrap()
		.clone();
	app.interaction.pointer.move_to((
		button.rect.x + button.rect.w / 2.,
		button.rect.y + button.rect.h / 2.,
	));
	app.update_tooltip(&layout, now);
	button
}

#[test]
fn native_controls_open_on_the_idle_timer_and_revalidate_before_showing() {
	let mut app = app();
	let now = Instant::now();
	let button =
		hover(&mut app, Command::Document(DocumentCommand::NewPage), now);
	assert_eq!(
		app.interaction.overlays.tooltip.hint.deadline(),
		Some(now + SHOW_DELAY)
	);
	app.tooltip_tick(now + SHOW_DELAY - Duration::from_millis(1));
	assert!(app.interaction.overlays.tooltip.hint.visible().is_none());
	app.tick(&StubLoop, now + SHOW_DELAY);
	let target = app.interaction.overlays.tooltip.hint.visible().unwrap();
	assert_eq!(target.id, button.id);
	assert_eq!(target.text, button.label);
	assert!(app.interaction.overlays.tooltip.hint.deadline().is_none());
	// Opening settings removes the toolbar controls beneath the panel.
	app.interaction.toggle_settings();
	app.overlay();
	assert!(app.interaction.overlays.tooltip.hint.visible().is_none());
	assert!(app.interaction.overlays.tooltip.hint.deadline().is_none());
}

#[test]
fn keyboard_hints_are_immediate_and_pointer_motion_takes_over() {
	let mut app = app();
	let now = Instant::now();
	let button =
		hover(&mut app, Command::Document(DocumentCommand::NewPage), now);
	app.interaction.widgets.focus = Some(button.id);
	app.interaction.widgets.focus_visible = true;
	app.interaction.overlays.tooltip.keyboard();
	app.overlay();
	assert!(
		app.interaction
			.overlays
			.tooltip
			.hint
			.visible()
			.is_some_and(|t| t.id == button.id)
	);
	assert!(app.interaction.overlays.tooltip.keyboard);
	let next = app
		.buttons()
		.into_iter()
		.find(|b| b.action == Command::Chrome(ChromeCommand::Settings))
		.unwrap();
	app.pointer_moved(PhysicalPosition::new(
		f64::from(next.rect.x + next.rect.w / 2.),
		f64::from(next.rect.y + next.rect.h / 2.),
	));
	assert!(app.interaction.overlays.tooltip.hint.visible().is_none());
	assert!(app.interaction.overlays.tooltip.hint.deadline().is_some());
	app.tooltip_tick(Instant::now() + SHOW_DELAY);
	assert!(
		app.interaction
			.overlays
			.tooltip
			.hint
			.visible()
			.is_some_and(|t| t.id == next.id)
	);
	assert!(!app.interaction.overlays.tooltip.keyboard);
}

#[test]
fn input_and_focus_loss_dismiss_visible_and_pending_hints() {
	for event in [
		WindowEvent::MouseInput {
			device_id: DeviceId::from_raw(0),
			state: ElementState::Pressed,
			button: MouseButton::Right,
		},
		WindowEvent::MouseWheel {
			device_id: DeviceId::from_raw(0),
			delta: markview_platform::event::MouseScrollDelta::LineDelta(
				0., 1.,
			),
			phase: markview_platform::event::TouchPhase::Moved,
		},
		WindowEvent::CursorLeft {
			device_id: DeviceId::from_raw(0),
		},
		WindowEvent::Focused(false),
	] {
		for visible in [false, true] {
			let mut app = app();
			let now = Instant::now();
			hover(&mut app, Command::Document(DocumentCommand::NewPage), now);
			if visible {
				app.tooltip_tick(now + SHOW_DELAY);
			}
			app.handle_window_event(
				&StubLoop,
				WindowId::from_raw(0),
				event.clone(),
			);
			assert!(
				app.interaction.overlays.tooltip.hint.visible().is_none(),
				"{event:?}"
			);
			assert!(
				app.interaction.overlays.tooltip.hint.deadline().is_none(),
				"{event:?}"
			);
		}
	}
}

fn button(label: &'static str, action: Command, rect: Rect) -> Button {
	Button {
		id: action.widget_id(),
		rect,
		clip: None,
		label: label.into(),
		tooltip: None,
		icon: None,
		marker: None,
		active: false,
		kind: Default::default(),
		enabled: true,
		action,
	}
}

fn placed(buttons: Vec<Button>) -> UiLayout {
	UiLayout {
		hit: crate::app::widgets::layout(&buttons),
		buttons,
		focus: Vec::new(),
		form: None,
	}
}

#[test]
fn hints_respect_clips_disabled_blockers_and_explicit_text() {
	let mut app = app();
	let now = Instant::now();
	let mut control = button(
		"visible label",
		Command::Chrome(ChromeCommand::Settings),
		Rect {
			x: 0.,
			y: 0.,
			w: 40.,
			h: 40.,
		},
	);
	control.tooltip = Some("Custom hint".into());
	control.clip = Some(Rect {
		x: 20.,
		y: 0.,
		w: 20.,
		h: 40.,
	});
	app.interaction.pointer.move_to((10., 10.));
	assert!(app.tooltip_target(&placed(vec![control.clone()])).is_none());
	app.interaction.pointer.move_to((30., 10.));
	let layout = placed(vec![control.clone()]);
	let target = app.tooltip_target(&layout).unwrap();
	assert_eq!(target.text, "Custom hint");
	assert_eq!(target.rect, control.clip.unwrap());
	app.update_tooltip(&layout, now);
	app.interaction.overlays.tooltip.hint.tick(now + SHOW_DELAY);
	assert!(app.interaction.overlays.tooltip.hint.visible().is_some());
	let mut blocker =
		button("", Command::Chrome(ChromeCommand::Tabs), control.rect);
	blocker.enabled = false;
	let blocked = placed(vec![blocker, control.clone()]);
	assert!(app.tooltip_target(&blocked).is_none());
	app.update_tooltip(&blocked, now);
	assert!(app.interaction.overlays.tooltip.hint.visible().is_none());
	// A changed translation replaces the text and restarts the hover delay.
	app.update_tooltip(&layout, now);
	app.interaction.overlays.tooltip.hint.tick(now + SHOW_DELAY);
	control.tooltip = Some("新的提示".into());
	app.update_tooltip(&placed(vec![control]), now + SHOW_DELAY);
	assert!(app.interaction.overlays.tooltip.hint.visible().is_none());
	assert_eq!(
		app.interaction.overlays.tooltip.hint.deadline(),
		Some(now + SHOW_DELAY * 2)
	);
}

#[test]
fn a_held_button_does_not_rearm_its_dismissed_hint() {
	let mut app = app();
	let now = Instant::now();
	let button =
		hover(&mut app, Command::Document(DocumentCommand::NewPage), now);
	let layout = placed(vec![button.clone()]);
	app.interaction.overlays.tooltip.hint.tick(now + SHOW_DELAY);
	app.interaction.widgets.captured = Some(button.id);
	app.update_tooltip(&layout, now);
	app.interaction.widgets.captured = None;
	app.update_tooltip(&layout, now + SHOW_DELAY);
	assert!(app.interaction.overlays.tooltip.hint.visible().is_none());
	assert!(app.interaction.overlays.tooltip.hint.deadline().is_none());
}

#[test]
fn clicking_a_keyboard_focused_icon_keeps_its_hint_dismissed() {
	let mut app = app();
	app.interaction.toggle_settings();
	let now = Instant::now();
	let action =
		Command::Settings(crate::state::SettingsCommand::SettingsPreview);
	let button = hover(&mut app, action, now);
	for _ in 0..app.focus_buttons().len() {
		if app.navigate_widgets(false) == Some(action) {
			break;
		}
	}
	assert_eq!(app.interaction.widgets.focus, Some(button.id));
	app.overlay();
	assert!(app.interaction.overlays.tooltip.hint.visible().is_some());
	app.pointer_pressed();
	app.overlay();
	app.pointer_released();
	app.overlay();
	assert!(app.preferences.settings_preview);
	assert!(app.interaction.overlays.tooltip.hint.visible().is_none());
	assert!(app.interaction.overlays.tooltip.hint.deadline().is_none());
	app.tooltip_tick(now + SHOW_DELAY * 2);
	assert!(app.interaction.overlays.tooltip.hint.visible().is_none());
	app.pointer_moved(PhysicalPosition::new(0., 100.));
	app.pointer_moved(PhysicalPosition::new(
		f64::from(button.rect.x + button.rect.w / 2.),
		f64::from(button.rect.y + button.rect.h / 2.),
	));
	let deadline = app.interaction.overlays.tooltip.hint.deadline().unwrap();
	app.tooltip_tick(deadline);
	assert!(app.interaction.overlays.tooltip.hint.visible().is_some());
}

#[test]
fn an_open_dropdown_suppresses_hints_for_the_controls_it_blocks() {
	let mut app = app();
	app.interaction.toggle_settings();
	app.action(Command::Chrome(ChromeCommand::ToggleDropdown(
		crate::state::DropdownId::Language,
		0,
	)));
	assert!(app.interaction.overlays.dropdown_open());
	let now = Instant::now();
	hover(
		&mut app,
		Command::Settings(crate::state::SettingsCommand::SettingsPreview),
		now,
	);
	app.tooltip_tick(now + SHOW_DELAY);
	let hinted = app.interaction.overlays.tooltip.hint.visible().is_some();
	assert!(app.interaction.overlays.dropdown_open());
	app.pointer_pressed();
	app.pointer_released();
	assert!(!app.interaction.overlays.dropdown_open());
	assert!(!app.preferences.settings_preview);
	assert!(!hinted, "the dropdown consumed the icon's click");
	let deadline = app.interaction.overlays.tooltip.hint.deadline().unwrap();
	app.tooltip_tick(deadline);
	assert!(app.interaction.overlays.tooltip.hint.visible().is_some());
}

#[test]
fn releasing_tab_keeps_the_hint_and_escape_dismisses_it_before_the_panel() {
	let mut app = app();
	app.interaction.toggle_settings();
	let now = Instant::now();
	let close = hover(&mut app, Command::Chrome(ChromeCommand::Settings), now);
	app.interaction.widgets.focus = Some(close.id);
	app.interaction.widgets.focus_visible = true;
	app.interaction.overlays.tooltip.keyboard();
	app.overlay();
	assert!(app.interaction.overlays.tooltip.hint.visible().is_some());
	assert!(
		!app.tooltip_key(&Key::Named(NamedKey::Tab), ElementState::Released)
	);
	assert!(app.interaction.overlays.tooltip.hint.visible().is_some());
	assert!(
		app.tooltip_key(&Key::Named(NamedKey::Escape), ElementState::Pressed)
	);
	app.overlay();
	assert!(app.interaction.overlays.tooltip.hint.visible().is_none());
	assert!(app.interaction.panel_open());
	assert!(
		!app.tooltip_key(&Key::Named(NamedKey::Escape), ElementState::Pressed)
	);
}
