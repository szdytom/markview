use super::*;
use crate::{
	app::{Event, window::Loop},
	cli::{LaunchOptions, Mode},
	state::PanelPage,
};
use winit::{event::DeviceId, window::WindowId};

#[derive(Clone)]
struct Proxy;
impl SendEvent for Proxy {
	fn try_send(&self, _: Event) -> bool {
		true
	}
}
struct TestLoop;
impl Loop for TestLoop {
	fn exit(&self) {
		panic!("input must not quit reader")
	}
}
fn app() -> App<Proxy> {
	let mut app = App::new(
		LaunchOptions {
			mode: Mode::Smoke,
			options: crate::test_support::options(),
			..Default::default()
		},
		Proxy,
	);
	app.ui = crate::test_support::shaper();
	app.interaction.show_panel(PanelPage::Export);
	app.interaction.widgets.focus =
		Some(Command::FocusInput(TextField::ExportTitle).widget_id());
	app.reveal_panel_focus();
	app.sync_input();
	app
}
fn event(app: &mut App<Proxy>, event: WindowEvent) {
	app.handle_window_event(&TestLoop, WindowId::dummy(), event);
}

#[test]
fn slash_is_literal_text_in_export_and_search_inputs() {
	let mut app = app();
	let slash = Key::Character("/".into());
	assert_eq!(app.input_key(&slash, Some("/")), Outcome::Consumed);
	assert_eq!(app.readers.session.export_title.text(), "/");
	assert!(!app.readers.session.search.open);
	app.readers.session.path = Some("search.md".into());
	app.open_search();
	assert_eq!(app.input_key(&slash, Some("/")), Outcome::Consumed);
	assert_eq!(app.readers.session.search.input.text(), "/");
	assert!(app.readers.session.search.open);
}

#[test]
fn slash_followed_by_typing_starts_a_new_query() {
	let mut app = app();
	app.readers.session.path = Some("search.md".into());
	app.interaction.show_panel(PanelPage::Closed);
	app.readers
		.session
		.search
		.input
		.set_text(&mut app.ui, "old");
	assert!(app.key_pressed(&Key::Character("/".into())));
	for c in "foobar".chars() {
		let text = c.to_string();
		assert_eq!(
			app.input_key(&Key::Character(text.clone().into()), Some(&text)),
			Outcome::Consumed
		);
	}
	assert!(app.readers.session.search.open);
	assert_eq!(app.readers.session.search.input.text(), "foobar");
}

#[test]
fn form_focus_routes_typing_ime_and_tab_without_document_shortcuts() {
	let mut app = app();
	assert_eq!(
		app.input_key(&Key::Character("q".into()), Some("q")),
		Outcome::Consumed
	);
	assert_eq!(app.readers.session.export_title.text(), "q");
	app.readers.session.export_title.select_all(&mut app.ui);
	event(
		&mut app,
		WindowEvent::Ime(Ime::Preedit("ni".into(), Some((2, 2)))),
	);
	assert_eq!(app.readers.session.export_title.text(), "q");
	event(&mut app, WindowEvent::Ime(Ime::Preedit("".into(), None)));
	event(&mut app, WindowEvent::Ime(Ime::Commit("你好".into())));
	assert_eq!(app.readers.session.export_title.text(), "你好");
	let actions = app.focus_buttons();
	let at = actions
		.iter()
		.position(|b| b.action == Command::FocusInput(TextField::ExportTitle))
		.unwrap();
	assert_eq!(
		app.input_key(&Key::Named(NamedKey::Tab), None),
		Outcome::Traverse
	);
	app.key_pressed(&Key::Named(NamedKey::Tab));
	app.sync_input();
	assert_eq!(
		app.interaction.widgets.focus,
		Some(actions[(at + 1) % actions.len()].action.widget_id())
	);
	assert_eq!(app.text_input.focused, None);
	app.interaction.pointer.set_modifiers(ModifiersState::SHIFT);
	app.key_pressed(&Key::Named(NamedKey::Tab));
	app.sync_input();
	assert_eq!(app.text_input.focused, Some(TextField::ExportTitle));
	assert_eq!(
		app.input_key(&Key::Named(NamedKey::Enter), None),
		Outcome::Submit
	);
	assert!(!app.export.is_running());
}

#[test]
fn pointer_drag_and_focus_loss_cancel_composition_without_losing_committed_text()
 {
	let mut app = app();
	app.readers
		.session
		.export_title
		.set_text(&mut app.ui, "original");
	app.readers.session.export_title.select_all(&mut app.ui);
	event(
		&mut app,
		WindowEvent::Ime(Ime::Preedit("候选".into(), None)),
	);
	event(&mut app, WindowEvent::Focused(false));
	assert_eq!(app.readers.session.export_title.text(), "original");
	assert!(!app.readers.session.export_title.is_composing());
	assert!(app.text_input.deadline.is_none());
	event(&mut app, WindowEvent::Focused(true));
	let (rect, viewport) = app.input_geometry(TextField::ExportTitle).unwrap();
	assert!(rect.intersect(viewport).is_some());
	app.interaction.pointer.move_to((rect.x + 9., rect.y + 16.));
	event(
		&mut app,
		WindowEvent::MouseInput {
			device_id: DeviceId::dummy(),
			state: ElementState::Pressed,
			button: MouseButton::Left,
		},
	);
	assert!(app.text_input.dragging);
	app.interaction
		.pointer
		.move_to((rect.x + rect.w + 30., rect.y + 16.));
	app.drag_input();
	event(
		&mut app,
		WindowEvent::MouseInput {
			device_id: DeviceId::dummy(),
			state: ElementState::Released,
			button: MouseButton::Left,
		},
	);
	assert!(app.readers.session.export_title.selected_text().is_some());
	assert!(!app.text_input.dragging);
	app.action(Command::ExportFormat(crate::settings::ExportFormat::Png));
	app.sync_input();
	assert!(app.text_input.focused.is_none());
	assert_eq!(app.readers.session.export_title.text(), "original");
	assert!(app.input_geometry(TextField::ExportTitle).is_none());
}

#[test]
fn title_and_undo_history_belong_to_document_sessions() {
	let mut app = app();
	let now = Instant::now();
	app.readers.open("a.md".into(), now);
	app.readers
		.session
		.export_title
		.insert(&mut app.ui, "A", EditKind::Typing);
	app.readers.open("b.md".into(), now);
	assert_eq!(app.readers.session.export_title.text(), "");
	app.readers
		.session
		.export_title
		.insert(&mut app.ui, "B", EditKind::Typing);
	assert!(app.readers.select(0, now));
	app.readers.session.release_heavy();
	assert_eq!(app.readers.session.export_title.text(), "A");
	app.readers.session.export_title.undo(&mut app.ui, false);
	assert_eq!(app.readers.session.export_title.text(), "");
	assert!(app.readers.select(1, now));
	assert_eq!(app.readers.session.export_title.text(), "B");
}

#[test]
fn clipboard_shortcuts_replace_selection_and_undo_without_a_desktop_clipboard()
{
	#[derive(Default)]
	struct Clipboard(String);
	impl InputClipboard for Clipboard {
		fn read(&mut self) -> anyhow::Result<String> {
			Ok(self.0.clone())
		}
		fn write(&mut self, text: String) -> anyhow::Result<()> {
			self.0 = text;
			Ok(())
		}
	}
	let mut ui = crate::test_support::shaper();
	let mut input = TextInput::default();
	let mut clipboard = Clipboard::default();
	let primary = if markview_platform::command_key() {
		ModifiersState::SUPER
	} else {
		ModifiersState::CONTROL
	};
	input.set_text(&mut ui, "标题");
	input.select_all(&mut ui);
	key(
		&mut input,
		&mut ui,
		&mut clipboard,
		&Key::Character("x".into()),
		None,
		primary,
	);
	assert_eq!(clipboard.0, "标题");
	assert_eq!(input.text(), "");
	clipboard.0 = "two\r\nlines".into();
	key(
		&mut input,
		&mut ui,
		&mut clipboard,
		&Key::Character("v".into()),
		None,
		primary,
	);
	assert_eq!(input.text(), "two lines");
	key(
		&mut input,
		&mut ui,
		&mut clipboard,
		&Key::Character("z".into()),
		None,
		primary,
	);
	assert_eq!(input.text(), "");
	key(
		&mut input,
		&mut ui,
		&mut clipboard,
		&Key::Character("z".into()),
		None,
		primary,
	);
	assert_eq!(input.text(), "标题");
}

#[test]
fn text_input_frames() -> anyhow::Result<()> {
	use crate::render::{Renderer, Theme, View};
	use markview_layout::style::Stylesheet;
	let mut app = app();
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	let directory =
		std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("artifacts");
	std::fs::create_dir_all(&directory)?;
	for (name, dark, scale, text, compose) in [
		("empty", false, 1.0, "", false),
		("selection", false, 1.0, "Markdown 中文", false),
		("ime", true, 1.5, "Document ", true),
		(
			"scroll",
			false,
			1.5,
			"A long title that extends far beyond the right edge of this input field",
			false,
		),
	] {
		let sheet = Stylesheet::bundled(dark);
		app.ui.set_stylesheet(sheet.clone());
		renderer.set_stylesheet(sheet);
		app.readers.session.export_title.set_text(&mut app.ui, text);
		if name == "selection" {
			app.readers.session.export_title.select_all(&mut app.ui);
		}
		if compose {
			app.readers.session.export_title.preedit(
				&mut app.ui,
				"中文",
				Some((0, 6)),
			);
		}
		let overlay = app.overlay();
		let view = View {
			width: (1200.0 * scale) as u32,
			height: (800.0 * scale) as u32,
			scale,
			scroll: 0.0,
			left: 20.0,
			top: 50.0,
			bottom: 38.0,
			theme: if dark { Theme::Dark } else { Theme::Light },
			horizontal: &std::collections::HashMap::new(),
			selection: None,
			revision: 0,
			hovered_link: None,
			hovered_overflow: None,
			held_overflow: None,
		};
		let target = renderer.offscreen(view.width, view.height);
		let submission = renderer.render(
			&app.readers.session.snapshot,
			&view,
			&overlay,
			&target.create_view(&Default::default()),
		)?;
		renderer.wait(Some(submission))?;
		renderer.save_png(
			&target,
			&directory.join(format!("text-input-{name}.png")),
		)?;
	}
	Ok(())
}

#[test]
fn ime_cursor_requests_settle_after_callbacks_and_coalesce_preedit_batches() {
	let mut app = app();
	app.readers
		.session
		.export_title
		.set_text(&mut app.ui, "Existing title ");
	assert!(app.take_ime_area().is_none());
	event(&mut app, WindowEvent::Ime(Ime::Enabled));
	let initial = app
		.take_ime_area()
		.expect("enabled IME receives its initial rectangle");
	// A Wayland `done` with no pending text is delivered as an empty preedit.
	for _ in 0..10 {
		event(
			&mut app,
			WindowEvent::Ime(Ime::Preedit(String::new(), None)),
		);
		app.draw_inputs();
		app.input_tick(Instant::now() + BLINK);
		assert!(
			app.take_ime_area().is_none(),
			"callbacks and blinking must not trigger more requests"
		);
	}
	event(
		&mut app,
		WindowEvent::Ime(Ime::Preedit("nihao".repeat(20), Some((100, 100)))),
	);
	let composing = app
		.take_ime_area()
		.expect("composition moves the candidate window");
	assert_ne!(composing, initial);
	event(
		&mut app,
		WindowEvent::Ime(Ime::Preedit(String::new(), None)),
	);
	app.sync_input();
	assert_eq!(
		app.text_input.last_ime_area,
		Some(composing),
		"transient clear must not send the old cursor position"
	);
	event(&mut app, WindowEvent::Ime(Ime::Commit("你好".into())));
	let committed = app
		.take_ime_area()
		.expect("publish final committed position once");
	assert_ne!(committed, composing);
	assert!(app.take_ime_area().is_none());
	event(&mut app, WindowEvent::Ime(Ime::Disabled));
	assert!(app.take_ime_area().is_none());
	event(&mut app, WindowEvent::Ime(Ime::Enabled));
	assert_eq!(
		app.take_ime_area(),
		Some(committed),
		"new IME session needs the same rectangle again"
	);
	event(&mut app, WindowEvent::Focused(false));
	assert!(app.take_ime_area().is_none());
}

#[test]
fn search_ime_defers_queries_enter_and_escape_until_composition_ends() {
	let mut app = app();
	app.readers.session.path = Some("search.md".into());
	app.open_search();
	app.readers.session.search.dirty = false;
	let sequence = app.readers.session.search.sequence;
	event(&mut app, WindowEvent::Ime(Ime::Enabled));
	event(
		&mut app,
		WindowEvent::Ime(Ime::Preedit("ni".into(), Some((2, 2)))),
	);
	app.input_key(&Key::Named(NamedKey::Enter), None);
	assert_eq!(app.readers.session.search.current, None);
	assert_eq!(app.readers.session.search.sequence, sequence);
	assert!(!app.readers.session.search.dirty);
	assert!(app.readers.session.search.input.is_composing());
	app.input_key(&Key::Named(NamedKey::Escape), None);
	assert!(!app.readers.session.search.input.is_composing());
	assert!(app.readers.session.search.open);
	event(
		&mut app,
		WindowEvent::Ime(Ime::Preedit("ni".into(), Some((2, 2)))),
	);
	event(&mut app, WindowEvent::Ime(Ime::Commit("你".into())));
	assert_eq!(app.readers.session.search.input.text(), "你");
	assert!(app.readers.session.search.dirty);
	assert!(app.readers.session.search.sequence > sequence);
	assert!(app.take_ime_area().is_some());
	for _ in 0..20 {
		event(&mut app, WindowEvent::Ime(Ime::Preedit("".into(), None)));
		assert!(app.take_ime_area().is_none());
	}
	app.input_key(&Key::Named(NamedKey::Escape), None);
	assert!(!app.readers.session.search.open);
	assert_eq!(app.readers.session.search.input.text(), "你");
}

#[test]
fn empty_page_url_input_accepts_text_and_submits_with_enter() {
	let mut app = app();
	app.interaction.show_panel(PanelPage::Closed);
	app.args.offline = true;
	let id = TextField::Url;
	let rect = app.input_geometry(id).unwrap().0;
	app.interaction
		.pointer
		.move_to((rect.x + 8.0, rect.y + 16.0));
	assert_eq!(app.input_at_cursor(), Some(id));
	app.action(Command::FocusInput(id));
	assert_eq!(app.text_input.focused, Some(id));
	assert_eq!(
		app.input_key(
			&Key::Character("h".into()),
			Some(" https://example.com/article ")
		),
		Outcome::Consumed
	);
	assert_eq!(
		app.input_key(&Key::Named(NamedKey::Enter), None),
		Outcome::Consumed
	);
	assert!(app.web_pages.contains_key("https://example.com/article"));
	assert!(app.readers.session.load_error.is_some());
	assert!(app.url_input.text().is_empty());
	assert!(app.input_geometry(id).is_none());
	assert!(app.text_input.focused.is_none());
}

#[test]
fn empty_page_link_button_and_focus_follow_page_visibility() {
	let mut app = app();
	app.interaction.show_panel(PanelPage::Closed);
	app.args.offline = true;
	assert!(
		app.focus_buttons()
			.iter()
			.any(|b| b.action == Command::FocusInput(TextField::Url))
	);
	app.action(Command::OpenUrl);
	assert!(app.readers.session.path.is_none());
	app.interaction.show_panel(PanelPage::Tabs);
	assert!(app.input_geometry(TextField::Url).is_none());
	app.interaction.show_panel(PanelPage::Closed);
	app.url_input
		.set_text(&mut app.ui, "https://example.com/button");
	app.action(Command::OpenUrl);
	assert!(app.web_pages.contains_key("https://example.com/button"));
	assert!(
		!app.buttons()
			.iter()
			.any(|b| b.action == Command::FocusInput(TextField::Url))
	);
}

#[test]
fn search_navigation_preserves_the_input_method_session() {
	let mut app = app();
	app.readers.session.path = Some("search.md".into());
	app.open_search();
	event(&mut app, WindowEvent::Ime(Ime::Enabled));
	for action in [Command::SearchNext, Command::SearchPrevious] {
		app.action(action);
		assert_eq!(app.text_input.focused, Some(TextField::Search));
		assert!(app.text_input.ime_enabled);
	}
	app.action(Command::SearchClose);
	assert!(!app.text_input.ime_enabled);
}
