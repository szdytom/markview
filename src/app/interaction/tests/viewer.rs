use super::*;
use crate::app::frame::Caption;
use crate::layout::{Draw, Rect};
use markview_core::image::{ImageInfo, ImageSnapshot, Pixels};
use winit::event::{Ime, MouseScrollDelta, TouchPhase};
use winit::keyboard::{Key, NamedKey};

fn event(app: &mut App<StubProxy>, event: WindowEvent) {
	app.handle_window_event(&StubLoop, WindowId::dummy(), event);
}

fn button(app: &mut App<StubProxy>, button: MouseButton, state: ElementState) {
	event(
		app,
		WindowEvent::MouseInput {
			device_id: DeviceId::dummy(),
			button,
			state,
		},
	);
}

fn move_to(app: &mut App<StubProxy>, at: (f32, f32)) {
	event(
		app,
		WindowEvent::CursorMoved {
			device_id: DeviceId::dummy(),
			position: PhysicalPosition::new(f64::from(at.0), f64::from(at.1)),
		},
	);
}

fn image_reader(source: &str, src: &str, loaded: bool) -> App<StubProxy> {
	let (mut app, document) = reader(source, 760.);
	if loaded {
		let mut images = ImageSnapshot::default();
		images.entries.insert(
			src.into(),
			ImageInfo {
				version: 1,
				size: Some((400, 400)),
				error: None,
			},
		);
		images.pixels.insert(
			src.into(),
			1,
			Arc::new(Pixels {
				width: 400,
				height: 400,
				rgba: vec![255; 400 * 400 * 4].into(),
			}),
		);
		app.readers.session.snapshot = LayoutEngine::new().layout_with_images(
			&document,
			&app.options(),
			&images,
		);
	}
	let geometry = app.view_geometry();
	let at = app
		.readers
		.session
		.snapshot
		.blocks
		.iter()
		.find_map(|block| {
			block.layout.draws.iter().find_map(|draw| match draw {
				Draw::Image { rect, .. } => Some((
					geometry.left + rect.x + rect.w / 2.,
					geometry.top + block.y + rect.y + rect.h / 2.,
				)),
				_ => None,
			})
		})
		.expect("an image is laid out");
	move_to(&mut app, at);
	app
}

fn open(app: &mut App<StubProxy>) {
	button(app, MouseButton::Left, ElementState::Pressed);
	button(app, MouseButton::Left, ElementState::Released);
	assert!(app.interaction.viewer.is_some(), "a click opens the viewer");
}

fn viewer_image(app: &mut App<StubProxy>) -> (Rect, u64) {
	let source = app.interaction.viewer.as_ref().unwrap().src.clone();
	app.overlay()
		.into_iter()
		.find_map(|draw| match draw {
			Draw::Image {
				src, rect, version, ..
			} if src == source => Some((rect, version)),
			_ => None,
		})
		.expect("the viewer image is drawn")
}

#[derive(Default)]
struct CaptionLoop(std::cell::Cell<usize>, std::cell::RefCell<Vec<Caption>>);
impl Loop for CaptionLoop {
	fn exit(&self) {
		self.0.set(self.0.get() + 1);
	}
	fn window_action(
		&self,
		_: Option<&winit::window::Window>,
		caption: Caption,
	) {
		self.1.borrow_mut().push(caption);
		if caption == Caption::Close {
			self.exit();
		}
	}
}

#[test]
fn window_controls_remain_above_the_image_viewer() {
	use crate::settings::WindowLayout;
	for style in [
		WindowLayout::Macos,
		WindowLayout::Windows,
		WindowLayout::Linux,
	] {
		let mut app = image_reader("![](a.png)", "a.png", false);
		app.frame.layout = style;
		open(&mut app);
		let layout = app.frame_layout();
		let actions = CaptionLoop::default();
		let release = |app: &mut App<StubProxy>| {
			app.handle_window_event(
				&actions,
				WindowId::dummy(),
				WindowEvent::MouseInput {
					device_id: DeviceId::dummy(),
					button: MouseButton::Left,
					state: ElementState::Released,
				},
			);
		};
		let content = (400., 300.);
		for (caption, rect) in layout.captions() {
			let at = (rect.x + rect.w / 2., rect.y + rect.h / 2.);
			if layout.caption_at(at.0, at.1).is_none() {
				continue;
			}
			move_to(&mut app, at);
			assert_eq!(app.frame.hover, Some(caption));
			assert_eq!(
				app.frame_cursor(),
				Some(winit::window::CursorIcon::Pointer)
			);
			button(&mut app, MouseButton::Left, ElementState::Pressed);
			assert_eq!(app.frame.pressed, Some(caption));
			assert!(app.interaction.viewer.as_ref().unwrap().grab.is_none());
			move_to(&mut app, content);
			let before = actions.1.borrow().len();
			release(&mut app);
			assert_eq!(actions.1.borrow().len(), before);
			assert!(app.frame.pressed.is_none());
			assert!(app.interaction.viewer.is_some());
			move_to(&mut app, at);
			button(&mut app, MouseButton::Left, ElementState::Pressed);
			release(&mut app);
			assert_eq!(actions.1.borrow().len(), before + 1);
			assert_eq!(actions.1.borrow().last(), Some(&caption));
			assert!(app.interaction.viewer.is_some());
		}
		let at = (layout.drag.x + layout.drag.w / 2., 20.);
		move_to(&mut app, at);
		button(&mut app, MouseButton::Left, ElementState::Pressed);
		assert!(
			app.interaction
				.viewer
				.as_ref()
				.unwrap()
				.pressed_at
				.is_none()
		);
		button(&mut app, MouseButton::Left, ElementState::Released);
		assert!(app.interaction.viewer.is_some());
		assert_eq!(actions.0.get(), usize::from(!layout.native_buttons));
	}
}

#[test]
fn caption_taps_do_not_reach_covered_controls_or_activate_after_a_drag() {
	use crate::{app::frame::Caption, settings::WindowLayout};
	use winit::event::Touch;
	for style in [
		WindowLayout::Macos,
		WindowLayout::Windows,
		WindowLayout::Linux,
	] {
		for viewer in [false, true] {
			let mut app = image_reader("![](a.png)", "a.png", false);
			app.frame.layout = style;
			if viewer {
				open(&mut app);
			}
			let exit = CaptionLoop::default();
			let send = |app: &mut App<StubProxy>, phase, at: (f32, f32)| {
				app.handle_window_event(
					&exit,
					WindowId::dummy(),
					WindowEvent::Touch(Touch {
						device_id: DeviceId::dummy(),
						id: 1,
						phase,
						force: None,
						location: PhysicalPosition::new(
							f64::from(at.0),
							f64::from(at.1),
						),
					}),
				);
			};
			let layout = app.frame_layout();
			if layout.native_buttons {
				continue;
			}
			for (caption, rect) in layout.captions() {
				let at = (rect.x + rect.w / 2., rect.y + rect.h / 2.);
				let before = exit.0.get();
				let before_actions = exit.1.borrow().len();
				send(&mut app, TouchPhase::Started, at);
				assert_eq!(app.frame.pressed, Some(caption));
				send(&mut app, TouchPhase::Cancelled, at);
				assert!(app.frame.pressed.is_none());
				assert_eq!(exit.0.get(), before);
				assert_eq!(exit.1.borrow().len(), before_actions);
				send(&mut app, TouchPhase::Started, at);
				send(&mut app, TouchPhase::Moved, (at.0, at.1 + 40.));
				send(&mut app, TouchPhase::Ended, at);
				assert_eq!(exit.0.get(), before);
				assert_eq!(exit.1.borrow().len(), before_actions);
				send(&mut app, TouchPhase::Started, at);
				send(&mut app, TouchPhase::Ended, at);
				assert_eq!(exit.1.borrow().len(), before_actions + 1);
				assert_eq!(exit.1.borrow().last(), Some(&caption));
				assert_eq!(
					exit.0.get(),
					before + usize::from(caption == Caption::Close)
				);
				assert!(!app.interaction.panel_open());
				assert_eq!(app.interaction.viewer.is_some(), viewer);
				assert!(app.frame.pressed.is_none());
			}
		}
	}
}

#[test]
fn loaded_images_without_selectable_text_open_on_click() {
	let code = "graph TD\nA-->B\n";
	for (source, src) in [
		("![](a.png)".to_owned(), "a.png".to_owned()),
		(
			format!("```mermaid\n{code}```"),
			markview_core::image::mermaid_source(code),
		),
	] {
		let mut app = image_reader(&source, &src, true);
		assert!(app.text_at_cursor().is_none());
		open(&mut app);
		assert_eq!(app.interaction.viewer.as_ref().unwrap().src, src);
	}
	let mut app =
		image_reader("[![](a.png)](#target)\n\n# Target", "a.png", true);
	let at = app.interaction.cursor;
	physical_click(&mut app, at);
	assert!(
		app.interaction.viewer.is_none(),
		"an image link keeps its action"
	);
}

#[test]
fn an_image_press_that_drags_or_loses_focus_does_not_open() {
	let mut app = image_reader("![](a.png)", "a.png", true);
	let at = app.interaction.cursor;
	button(&mut app, MouseButton::Left, ElementState::Pressed);
	move_to(&mut app, (at.0 + 20., at.1));
	move_to(&mut app, at);
	button(&mut app, MouseButton::Left, ElementState::Released);
	assert!(app.interaction.viewer.is_none());
	button(&mut app, MouseButton::Left, ElementState::Pressed);
	event(&mut app, WindowEvent::Focused(false));
	button(&mut app, MouseButton::Left, ElementState::Released);
	assert!(app.interaction.viewer.is_none());
	button(&mut app, MouseButton::Left, ElementState::Pressed);
	app.interaction
		.show_panel(crate::state::PanelPage::Settings(
			crate::state::PanelTab::Generic,
		));
	button(&mut app, MouseButton::Left, ElementState::Released);
	assert!(app.interaction.viewer.is_none());
	app.interaction.show_panel(crate::state::PanelPage::Closed);
	open(&mut app);
}

#[test]
fn viewer_captures_search_input_and_middle_clicks() {
	let mut app = image_reader("![](a.png)", "a.png", false);
	let snapshot = app.readers.session.snapshot.clone();
	app.readers
		.open(PathBuf::from("/tmp/viewer.md"), Instant::now());
	app.readers.session.snapshot = snapshot;
	app.readers.session.search.open = true;
	app.readers
		.session
		.search
		.input
		.set_text(&mut app.ui, "query");
	open(&mut app);
	let rect = app.search_input_rect();
	move_to(&mut app, (rect.x + 10., rect.y + rect.h / 2.));
	button(&mut app, MouseButton::Left, ElementState::Pressed);
	assert!(app.interaction.viewer.as_ref().unwrap().grab.is_some());
	assert_ne!(
		app.interaction.focus,
		Some(crate::state::Command::FocusInput(
			crate::state::TextField::Search
		))
	);
	event(&mut app, WindowEvent::Ime(Ime::Commit("changed".into())));
	assert_eq!(app.readers.session.search.input.text(), "query");
	let tab = app.tab_layout().rects[0];
	move_to(&mut app, (tab.x + tab.w / 2., tab.y + tab.h / 2.));
	assert!(app.tab_at_cursor().is_some());
	button(&mut app, MouseButton::Middle, ElementState::Pressed);
	assert_eq!(app.readers.entries().len(), 1);
	assert!(app.interaction.viewer.is_some());
	let scroll = app.readers.session.scrolling.offset;
	app.key_pressed(&Key::Named(NamedKey::PageDown));
	assert_eq!(app.readers.session.scrolling.offset, scroll);
	assert!(!app.readers.session.scroll_animating());
	app.interaction.modal = Some(crate::state::Modal::OpenLocal {
		path: PathBuf::from("/tmp/viewer.bin"),
		dir: PathBuf::from("/tmp"),
		document_dir: None,
	});
	app.key_pressed(&Key::Named(NamedKey::Escape));
	assert!(app.interaction.viewer.is_none());
	assert!(app.readers.session.search.open);
	assert!(
		app.interaction.modal.is_some(),
		"Escape only closes the viewer"
	);
}

#[test]
fn viewer_drag_stays_a_drag_after_returning_to_the_press_point() {
	let mut app = image_reader("![](a.png)", "a.png", false);
	open(&mut app);
	let at = app.interaction.cursor;
	button(&mut app, MouseButton::Left, ElementState::Pressed);
	move_to(&mut app, (at.0 + 20., at.1));
	move_to(&mut app, (at.0 + 1., at.1));
	button(&mut app, MouseButton::Left, ElementState::Released);
	assert!(app.interaction.viewer.is_some());
	button(&mut app, MouseButton::Left, ElementState::Pressed);
	button(&mut app, MouseButton::Left, ElementState::Released);
	assert!(app.interaction.viewer.is_none(), "the next click closes");
}

#[test]
fn viewer_focus_loss_cancels_the_drag_and_ignores_an_orphan_release() {
	let mut app = image_reader("![](a.png)", "a.png", false);
	open(&mut app);
	event(
		&mut app,
		WindowEvent::MouseWheel {
			device_id: DeviceId::dummy(),
			delta: MouseScrollDelta::LineDelta(0., 20.),
			phase: TouchPhase::Moved,
		},
	);
	button(&mut app, MouseButton::Left, ElementState::Pressed);
	event(&mut app, WindowEvent::Focused(false));
	let viewer = app.interaction.viewer.as_ref().unwrap();
	assert!(viewer.grab.is_none() && viewer.pressed_at.is_none());
	let pan = viewer.pan;
	event(&mut app, WindowEvent::Focused(true));
	move_to(&mut app, (800., 500.));
	assert_eq!(app.interaction.viewer.as_ref().unwrap().pan, pan);
	button(&mut app, MouseButton::Left, ElementState::Released);
	assert!(app.interaction.viewer.is_some());
}

#[test]
fn decoding_refreshes_the_viewer_dimensions_and_texture_version() {
	for layout in [
		crate::settings::WindowLayout::Macos,
		crate::settings::WindowLayout::Windows,
		crate::settings::WindowLayout::Linux,
	] {
		let mut app = image_reader("![](a.png)", "a.png", false);
		app.frame.layout = layout;
		open(&mut app);
		let (placeholder, _) = viewer_image(&mut app);
		assert_ne!(placeholder.w, placeholder.h);
		app.readers.session.snapshot.images.pixels.insert(
			"a.png".into(),
			2,
			Arc::new(Pixels {
				width: 400,
				height: 400,
				rgba: vec![255; 400 * 400 * 4].into(),
			}),
		);
		app.readers.session.snapshot.images.entries.insert(
			"a.png".into(),
			ImageInfo {
				version: 2,
				size: Some((400, 400)),
				error: None,
			},
		);
		let (rect, version) = viewer_image(&mut app);
		assert_eq!((rect.w, rect.h), (400., 400.));
		assert_eq!(version, 2);
	}
}

#[test]
fn a_touchpad_stream_pans_nothing_while_the_viewer_is_open() {
	let mut app = image_reader("![](a.png)", "a.png", true);
	open(&mut app);
	// A Direct Manipulation stream bypasses the wheel the viewer zooms by,
	// so the pixel surfaces owe it nothing: the page behind the viewer
	// holds still.
	app.trackpad_scroll(0.0, -40.0, TouchPhase::Started, Inertia::Native);
	app.trackpad_scroll(0.0, -60.0, TouchPhase::Moved, Inertia::Native);
	assert_eq!(app.readers.session.scrolling.offset, 0.0);
	// With the viewer gone, the page pans again.
	app.key_pressed(&Key::Named(NamedKey::Escape));
	assert!(app.interaction.viewer.is_none());
	app.trackpad_scroll(0.0, -40.0, TouchPhase::Started, Inertia::Native);
	app.trackpad_scroll(0.0, -60.0, TouchPhase::Moved, Inertia::Native);
	assert!(app.readers.session.scrolling.offset > 0.0);
}
