//! The open option list, driven through the reader's own input paths.
//!
//! The list is an overlay that owns input, so these tests feed the real
//! handlers and watch the settings they answer with: a release over an option
//! commits it, an arrow moves its highlight, a press that misses the list
//! closes it without reaching the control it covered, and a touch agrees with
//! the list's own geometry.
use crate::app::window::Loop;
use crate::app::{App, Event, SendEvent};
use crate::cli::{LaunchOptions, Mode};
use crate::lang::Lang;
use crate::layout::Rect;
use crate::state::{Command, DropdownId, PanelPage, PanelTab};
use std::cell::Cell;
use winit::dpi::PhysicalPosition;
use winit::event::{
	DeviceId, ElementState, MouseButton, MouseScrollDelta, Touch, TouchPhase,
	WindowEvent,
};
use winit::keyboard::{Key, NamedKey};
use winit::window::WindowId;

/// Stands in for the reader's loop, which a test cannot open: winit builds one
/// event loop per process. Nothing here reads the events it is handed, so the
/// stub only has to exist for `App::new` to store it.
#[derive(Clone)]
struct StubProxy;
impl SendEvent for StubProxy {
	fn send(&self, _event: Event) {}
}

/// What the app asks of its loop, answered without a window server.
struct StubLoop;
impl Loop for StubLoop {
	fn exit(&self) {}
}

/// A loop that remembers whether anything asked it to stop, so a test can see
/// whether a key reached the quit shortcut or an owner took it first.
struct QuitLoop(Cell<bool>);
impl Loop for QuitLoop {
	fn exit(&self) {
		self.0.set(true);
	}
}

/// A reader whose settings panel is open on its first page, so the list has a
/// real page behind it to leak to.
fn app_with_panel() -> App<StubProxy> {
	let mut app = App::new(
		LaunchOptions {
			mode: Mode::Smoke,
			..Default::default()
		},
		StubProxy,
	);
	app.interaction
		.show_panel(PanelPage::Settings(PanelTab::Generic));
	app
}

/// One physical click: move the pointer there, press, then release in place.
fn click(app: &mut App<StubProxy>, x: f32, y: f32) {
	// A tap suppresses mouse input for a moment; these tests drive both paths,
	// so each click starts from a pointer the platform is not holding back.
	app.gestures.allow_mouse();
	app.handle_window_event(
		&StubLoop,
		WindowId::dummy(),
		WindowEvent::CursorMoved {
			device_id: DeviceId::dummy(),
			position: PhysicalPosition::new(f64::from(x), f64::from(y)),
		},
	);
	for state in [ElementState::Pressed, ElementState::Released] {
		app.handle_window_event(
			&StubLoop,
			WindowId::dummy(),
			WindowEvent::MouseInput {
				device_id: DeviceId::dummy(),
				button: MouseButton::Left,
				state,
			},
		);
	}
}

/// The window point at the centre of one option in the open list.
fn option_centre(app: &mut App<StubProxy>, option: Command) -> (f32, f32) {
	app.buttons()
		.into_iter()
		.find(|button| button.action == option)
		.map(|button| {
			(
				button.rect.x + button.rect.w / 2.0,
				button.rect.y + button.rect.h / 2.0,
			)
		})
		.unwrap()
}

/// The option the list's keyboard is on, as the reader measures it.
fn highlighted(app: &mut App<StubProxy>) -> Command {
	app.dropdown_menu()
		.expect("the list measures")
		.chosen()
		.expect("an option is under the keyboard")
}

/// A tap at a window point: down, then up in place.
fn touch(app: &mut App<StubProxy>, x: f32, y: f32) {
	let event = |phase: TouchPhase| Touch {
		device_id: DeviceId::dummy(),
		id: 1,
		phase,
		location: PhysicalPosition::new(f64::from(x), f64::from(y)),
		force: None,
	};
	app.handle_touch(event(TouchPhase::Started));
	app.handle_touch(event(TouchPhase::Ended));
}

/// An unmodified key press, as the window's own loop delivers one.
fn press(app: &mut App<StubProxy>, quit: &QuitLoop, key: Key) {
	app.press_unmodified(quit, &key);
}

/// A point inside both rectangles, when they overlap.
fn overlap(a: Rect, b: Rect) -> Option<(f32, f32)> {
	let (x0, y0) = (a.x.max(b.x), a.y.max(b.y));
	let (x1, y1) = ((a.x + a.w).min(b.x + b.w), (a.y + a.h).min(b.y + b.h));
	(x1 > x0 && y1 > y0).then(|| ((x0 + x1) / 2.0, (y0 + y1) / 2.0))
}

/// A page control the open list covers, and a point on it that none of the
/// list's options own. The options are inset by the list's padding, so the
/// covered control's own edge is such a point.
fn exposed_covered_point(
	app: &mut App<StubProxy>,
) -> Option<(Command, f32, f32)> {
	let menu = app.dropdown_menu()?;
	let options: Vec<Rect> = menu.buttons.iter().map(|b| b.rect).collect();
	app.buttons()
		.into_iter()
		.filter(|button| {
			!matches!(
				button.action,
				Command::Language(_) | Command::ToggleDropdown(..)
			)
		})
		.find_map(|button| {
			let point =
				(button.rect.x + 1.0, button.rect.y + button.rect.h / 2.0);
			(menu.rect.contains(point.0, point.1)
				&& options
					.iter()
					.all(|option| !option.contains(point.0, point.1)))
			.then_some((button.action, point.0, point.1))
		})
}

#[test]
fn a_release_over_an_open_option_commits_it() {
	let mut app = app_with_panel();
	app.action(Command::ToggleDropdown(DropdownId::Language, 0));
	assert!(app.interaction.dropdown.is_some());

	let target = Command::Language(Some(Lang::ZhHans));
	let (x, y) = option_centre(&mut app, target);
	click(&mut app, x, y);

	// The release finds the option in the button list and commits it.
	assert_eq!(app.preferences.values.lang, Some(Lang::ZhHans));
	assert!(app.interaction.dropdown.is_none());
}

/// The language reaches the document as well as the chrome: the front matter
/// draws the interface's own label, so committing a language has to ask for a
/// relayout. Without the request the open document keeps the old label until
/// some unrelated edit happens to lay the block out again.
#[test]
fn committing_a_language_relabels_the_front_matter() {
	let mut app = app_with_panel();
	// A request is only about a document, so the session needs one.
	app.readers.session.path = Some(std::path::PathBuf::from("/tmp/a.md"));
	app.action(Command::Language(Some(Lang::ZhHans)));

	let options = app
		.readers
		.session
		.requested_options
		.expect("a language change asks for a relayout");
	assert_eq!(options.front_matter_label, Lang::ZhHans.front_matter());
}

/// The release path and the drawing path have to agree on where an option is,
/// which they only do because the open list answers through `App::buttons`.
#[test]
fn an_open_option_is_a_button_the_pointer_can_hit() {
	let mut app = app_with_panel();
	app.action(Command::ToggleDropdown(DropdownId::Language, 0));
	let target = Command::Language(Some(Lang::ZhHans));
	assert!(
		app.buttons().iter().any(|button| button.action == target),
		"the open list's options are reachable as buttons"
	);
}

#[test]
fn arrows_move_the_highlight_and_enter_commits_it() {
	let mut app = app_with_panel();
	app.action(Command::ToggleDropdown(DropdownId::Language, 0));

	let options = [
		Command::Language(None),
		Command::Language(Some(Lang::En)),
		Command::Language(Some(Lang::ZhHans)),
	];

	// Down steps to the next option and up steps back. The panel owns the
	// page, so before the fix the guard dropped the key and the highlight
	// never moved.
	app.key_pressed(&Key::Named(NamedKey::ArrowDown));
	assert_eq!(highlighted(&mut app), options[1]);
	app.key_pressed(&Key::Named(NamedKey::ArrowDown));
	assert_eq!(highlighted(&mut app), options[2]);
	app.key_pressed(&Key::Named(NamedKey::ArrowUp));
	assert_eq!(highlighted(&mut app), options[1]);

	// Enter closes the list and commits the option under the keyboard.
	app.key_pressed(&Key::Named(NamedKey::Enter));
	assert!(app.interaction.dropdown.is_none());
	assert_eq!(app.preferences.values.lang, Some(Lang::En));
}

/// The list takes the keys it moves with and no others: an arrow that wraps
/// round stays in the list and never scrolls the page behind it.
#[test]
fn an_arrow_under_an_open_list_stays_in_the_list() {
	let mut app = app_with_panel();
	app.action(Command::ToggleDropdown(DropdownId::Language, 0));
	let before = app.readers.session.scroll;
	app.key_pressed(&Key::Named(NamedKey::ArrowUp));
	assert_eq!(highlighted(&mut app), Command::Language(Some(Lang::ZhHans)));
	assert!(app.interaction.dropdown.is_some());
	assert_eq!(app.readers.session.scroll, before);
}

#[test]
fn escape_closes_without_committing_and_tab_reaches_the_page() {
	let mut app = app_with_panel();
	app.action(Command::ToggleDropdown(DropdownId::Language, 0));

	// Escape closes without committing anything.
	app.key_pressed(&Key::Named(NamedKey::Escape));
	assert!(app.interaction.dropdown.is_none());
	assert_eq!(app.preferences.values.lang, None);

	// Tab closes the list and hands the key on to the panel's own traversal.
	app.action(Command::ToggleDropdown(DropdownId::Language, 0));
	app.key_pressed(&Key::Named(NamedKey::Tab));
	assert!(app.interaction.dropdown.is_none());
	assert!(app.interaction.focus.is_some());
}

#[test]
fn a_press_outside_the_list_closes_it_without_reaching_the_page() {
	let mut app = app_with_panel();
	app.action(Command::ToggleDropdown(DropdownId::Language, 0));

	// A control the open list covers: a press on it, on no option, must close
	// the list and nothing more.
	let (covered_action, x, y) = exposed_covered_point(&mut app)
		.expect("the list covers a page control");
	// The covered control is the scroll speed row, and its buttons are the
	// ones a click here would otherwise reach.
	let before = app.preferences.values.scroll_speed;
	assert_eq!(covered_action, Command::ScrollSpeed(-1));
	click(&mut app, x, y);
	assert!(app.interaction.dropdown.is_none());
	assert_eq!(
		app.preferences.values.scroll_speed, before,
		"the press must not reach the control the list covered"
	);
	assert_ne!(
		app.interaction.focus,
		Some(covered_action),
		"the covered control never took the press"
	);
	assert_eq!(
		app.interaction.focus,
		Some(Command::ToggleDropdown(DropdownId::Language, 0)),
		"focus returns to the chooser the list belongs to"
	);
}

/// Clicking the control that opened the list closes it, and the click ends
/// there: the release must not reach the page and open the list again.
#[test]
fn clicking_the_control_again_closes_the_list_and_keeps_it_closed() {
	let mut app = app_with_panel();
	app.action(Command::ToggleDropdown(DropdownId::Language, 0));
	let anchor = app
		.buttons()
		.into_iter()
		.find(|button| {
			matches!(
				button.action,
				Command::ToggleDropdown(DropdownId::Language, _)
			)
		})
		.expect("the control that opens the list")
		.rect;
	click(
		&mut app,
		anchor.x + anchor.w / 2.0,
		anchor.y + anchor.h / 2.0,
	);
	assert!(
		app.interaction.dropdown.is_none(),
		"the list stays closed once its own control closes it"
	);
	assert_eq!(app.preferences.values.lang, None);
}

#[test]
fn a_touch_on_an_option_picks_it_like_a_click() {
	let mut app = app_with_panel();
	app.action(Command::ToggleDropdown(DropdownId::Language, 0));
	let target = Command::Language(Some(Lang::ZhHans));
	let (x, y) = option_centre(&mut app, target);
	touch(&mut app, x, y);
	assert_eq!(app.preferences.values.lang, Some(Lang::ZhHans));
	assert!(app.interaction.dropdown.is_none());
}

/// Just past one option's own rectangle is the next option, not a gap. The
/// expanded touch target must not turn that point into a pick of the first.
#[test]
fn a_touch_just_past_an_option_lands_on_the_next_one() {
	let mut app = app_with_panel();
	app.action(Command::ToggleDropdown(DropdownId::Language, 0));
	let first = app
		.buttons()
		.into_iter()
		.find(|b| b.action == Command::Language(None))
		.unwrap()
		.rect;
	let second = app
		.buttons()
		.into_iter()
		.find(|b| b.action == Command::Language(Some(Lang::En)))
		.unwrap()
		.rect;
	// The options are drawn at a fixed pitch, so the next one's own rectangle
	// starts where this one ends and the gap between the two.
	let (x, y) = (second.x + second.w / 2.0, second.y + second.h / 2.0);
	assert!(
		!first.contains(x, y),
		"the point is past the first option's own rectangle"
	);
	assert!(second.contains(x, y), "and inside the next one");
	touch(&mut app, x, y);
	assert_eq!(app.preferences.values.lang, Some(Lang::En));
}

#[test]
fn a_touch_clear_of_every_option_closes_the_list_without_picking() {
	let mut app = app_with_panel();
	app.action(Command::ToggleDropdown(DropdownId::Language, 0));
	let menu = app.dropdown_menu().unwrap();
	// Inside the list's frame but between its options and its edge.
	let (x, y) = (menu.rect.x + menu.rect.w / 2.0, menu.rect.y + 1.0);
	touch(&mut app, x, y);
	assert!(app.interaction.dropdown.is_none());
	assert_eq!(app.preferences.values.lang, None);
}

/// The list is painted over the row it drops across, so a click on an option
/// must reach the option even where a covered control lies under the same
/// point. Reading the page's buttons first used to dismiss the list instead.
#[test]
fn a_click_on_an_option_over_a_covered_control_picks_the_option() {
	let mut app = app_with_panel();
	let page = app.buttons();
	app.action(Command::ToggleDropdown(DropdownId::Language, 0));
	let option = app
		.buttons()
		.into_iter()
		.find(|button| button.action == Command::Language(Some(Lang::En)))
		.expect("the option")
		.rect;
	let (x, y) = page
		.into_iter()
		.filter(|button| {
			!matches!(
				button.action,
				Command::Language(_) | Command::ToggleDropdown(..)
			)
		})
		.find_map(|button| overlap(button.rect, option))
		.expect("the list drops over a page control");

	click(&mut app, x, y);
	assert_eq!(app.preferences.values.lang, Some(Lang::En));
	assert!(app.interaction.dropdown.is_none());
}

/// A tap on a page control the list covers still belongs to the list: it
/// closes the menu without reaching the control underneath, and the 44-pixel
/// touch target never reaches past the list's own options.
#[test]
fn a_touch_on_a_covered_control_dismisses_without_activating_it() {
	let mut app = app_with_panel();
	app.action(Command::ToggleDropdown(DropdownId::Language, 0));
	let (_, x, y) = exposed_covered_point(&mut app)
		.expect("the list covers a page control");
	let before = app.preferences.values.clone();

	touch(&mut app, x, y);
	assert!(app.interaction.dropdown.is_none());
	assert_eq!(
		app.preferences.values, before,
		"the covered control never took the tap"
	);
}

/// The padding between the last option and the list's edge is not a target: a
/// tap there closes the list instead of committing the option it is near.
#[test]
fn a_touch_on_the_list_padding_does_not_pick_the_nearest_option() {
	let mut app = app_with_panel();
	app.action(Command::ToggleDropdown(DropdownId::Language, 0));
	let menu = app.dropdown_menu().unwrap();
	let (x, y) = (
		menu.rect.x + menu.rect.w / 2.0,
		menu.rect.y + menu.rect.h - 1.0,
	);

	touch(&mut app, x, y);
	assert!(app.interaction.dropdown.is_none());
	assert_eq!(
		app.preferences.values.lang, None,
		"the padding is not an option"
	);
}

/// The quit shortcut is the reader's last answer, not its first: an open list
/// takes an unmodified `q` before it can reach the loop.
#[test]
fn an_unmodified_q_with_the_list_open_does_not_quit() {
	let mut app = app_with_panel();
	app.action(Command::ToggleDropdown(DropdownId::Language, 0));
	let quit = QuitLoop(Cell::new(false));

	press(&mut app, &quit, Key::Character("q".into()));
	assert!(!quit.0.get(), "the open list owns the key");
	assert!(app.interaction.dropdown.is_some(), "and stays open");
}

/// The settings panel guards unmodified keys, so `q` is its own while it is
/// open rather than quitting the reader.
#[test]
fn an_unmodified_q_with_the_panel_open_does_not_quit() {
	let mut app = app_with_panel();
	let quit = QuitLoop(Cell::new(false));

	press(&mut app, &quit, Key::Character("q".into()));
	assert!(!quit.0.get(), "the settings panel owns unmodified keys");
}

/// With nothing owning the key, the quit shortcut still works.
#[test]
fn an_unmodified_q_with_no_owner_quits() {
	let mut app = App::new(
		LaunchOptions {
			mode: Mode::Smoke,
			..Default::default()
		},
		StubProxy,
	);
	let quit = QuitLoop(Cell::new(false));

	press(&mut app, &quit, Key::Character("q".into()));
	assert!(
		quit.0.get(),
		"nothing owned the key, so it reached the loop"
	);
}

/// The open list owns the wheel: the page keeps its scroll, so the row its
/// list hangs from cannot move out from under it.
#[test]
fn the_wheel_does_not_scroll_the_page_behind_an_open_list() {
	let mut app = app_with_panel();
	let (_, max) = app.panel_scroll_range().expect("the page scrolls");
	assert!(max > 0.0, "the test needs a page with somewhere to scroll");
	app.action(Command::ToggleDropdown(DropdownId::Language, 0));
	// The pointer is on the page the list covers.
	app.interaction.cursor = (700.0, 400.0);
	let before = app.interaction.settings_scroll;

	app.handle_window_event(
		&StubLoop,
		WindowId::dummy(),
		WindowEvent::MouseWheel {
			device_id: DeviceId::dummy(),
			delta: MouseScrollDelta::LineDelta(0.0, -5.0),
			phase: TouchPhase::Moved,
		},
	);
	assert_eq!(
		app.interaction.settings_scroll, before,
		"the wheel left the page where it was"
	);
	assert!(
		app.dropdown_menu().is_some(),
		"the list still hangs from a visible row"
	);
}

/// Closing the list returns focus to its chooser, so `Enter` opens the list
/// again instead of doing nothing.
#[test]
fn closing_the_list_returns_focus_to_its_chooser() {
	let mut app = app_with_panel();
	app.action(Command::ToggleDropdown(DropdownId::Language, 0));
	// Move the highlight so focus leaves the chooser for an option.
	app.key_pressed(&Key::Named(NamedKey::ArrowDown));

	app.key_pressed(&Key::Named(NamedKey::Escape));
	assert!(app.interaction.dropdown.is_none());
	assert!(
		matches!(
			app.interaction.focus,
			Some(Command::ToggleDropdown(DropdownId::Language, _))
		),
		"focus returns to the chooser"
	);

	// `Enter` reopens the list the focus now names.
	app.key_pressed(&Key::Named(NamedKey::Enter));
	assert!(app.interaction.dropdown.is_some());
}

/// Committing an option with `Enter` also leaves focus on the chooser.
#[test]
fn committing_an_option_returns_focus_to_its_chooser() {
	let mut app = app_with_panel();
	app.action(Command::ToggleDropdown(DropdownId::Language, 0));
	app.key_pressed(&Key::Named(NamedKey::ArrowDown));

	app.key_pressed(&Key::Named(NamedKey::Enter));
	assert_eq!(app.preferences.values.lang, Some(Lang::En));
	assert_eq!(
		app.interaction.focus,
		Some(Command::ToggleDropdown(DropdownId::Language, 1)),
		"focus returns to the chooser"
	);
}

/// `Tab` continues from the chooser the list closed on rather than restarting
/// traversal at the panel's first control.
#[test]
fn tab_after_closing_the_list_advances_from_the_chooser() {
	let mut app = app_with_panel();
	app.action(Command::ToggleDropdown(DropdownId::Language, 0));
	app.key_pressed(&Key::Named(NamedKey::ArrowDown));

	app.key_pressed(&Key::Named(NamedKey::Tab));
	assert!(app.interaction.dropdown.is_none());
	assert_eq!(
		app.interaction.focus,
		Some(Command::ScrollSpeed(-1)),
		"traversal moved past the chooser, not back to the first control"
	);
}
