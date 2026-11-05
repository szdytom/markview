use super::*;
use crate::{app::SendEvent, cli::LaunchOptions};
use std::cell::Cell;
use winit::keyboard::ModifiersState;

#[derive(Clone)]
struct Proxy;
impl SendEvent for Proxy {
	fn try_send(&self, _: crate::app::Event) -> bool {
		true
	}
}

#[derive(Default)]
struct WindowLoop(Cell<usize>);
impl Loop for WindowLoop {
	fn exit(&self) {
		panic!("the current window must stay open");
	}
	fn new_window(&self) -> anyhow::Result<()> {
		self.0.set(self.0.get() + 1);
		Ok(())
	}
}

#[test]
fn new_window_shortcut_keeps_the_current_document_and_ignores_repeats() {
	if !markview_platform::desktop_cli() {
		return;
	}
	let mut app = App::new(
		LaunchOptions {
			mode: Mode::Smoke,
			options: crate::test_support::options(),
			..Default::default()
		},
		Proxy,
	);
	let path = std::path::PathBuf::from("notes.md");
	app.readers.open(path.clone(), Instant::now());
	let event_loop = WindowLoop::default();
	let key = Key::Character("n".into());
	for modifiers in [ModifiersState::CONTROL, ModifiersState::SUPER] {
		app.interaction.pointer.set_modifiers(modifiers);
		let count = event_loop.0.get();
		app.dispatch_key(&event_loop, &key, false);
		assert_eq!(event_loop.0.get(), count + 1);
		app.dispatch_key(&event_loop, &key, true);
		assert_eq!(event_loop.0.get(), count + 1);
	}
	for modifiers in [
		ModifiersState::empty(),
		ModifiersState::ALT,
		ModifiersState::CONTROL | ModifiersState::ALT,
		ModifiersState::CONTROL | ModifiersState::SHIFT,
	] {
		app.interaction.pointer.set_modifiers(modifiers);
		app.dispatch_key(&event_loop, &key, false);
	}
	assert_eq!(event_loop.0.get(), 2);
	assert_eq!(app.readers.entries().len(), 1);
	assert_eq!(app.readers.session.path.as_ref(), Some(&path));
	app.interaction.overlays.modal.send(Message::Open(
		crate::app::modal::Modal::OpenLocal {
			path,
			dir: Default::default(),
			document_dir: None,
		},
	));
	app.interaction
		.pointer
		.set_modifiers(ModifiersState::CONTROL);
	app.dispatch_key(&event_loop, &key, false);
	assert_eq!(event_loop.0.get(), 2);
}
