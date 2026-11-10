//! Windows shows the reader window only once it has a frame to paint.
//!
//! A window made visible during creation is composed and painted by the shell
//! before the reader can present anything, so startup shows a blank window for
//! the whole of it. The platform therefore creates the window hidden and
//! reveals it after the first frame.
//!
//! `winit` allows one event loop per process, so this lives in an integration
//! test rather than beside the crate's unit tests.
#![cfg(windows)]
#![allow(deprecated)]

use winit::{
	event_loop::EventLoop, platform::windows::EventLoopBuilderExtWindows,
	window::Window,
};

#[test]
fn the_window_is_created_hidden_and_revealed_on_request() {
	assert!(
		markview_platform::starts_hidden(),
		"the platform should hide a window until it has a frame"
	);
	let attributes = markview_platform::window_attributes(
		Window::default_attributes(),
		false,
	);
	assert!(
		!attributes.visible,
		"the window attributes must keep the window hidden"
	);

	let event_loop =
		EventLoop::builder().with_any_thread(true).build().unwrap();
	let window = event_loop.create_window(attributes).unwrap();
	assert_eq!(
		window.is_visible(),
		Some(false),
		"the created window must stay hidden"
	);

	markview_platform::reveal(&window);
	assert_eq!(
		window.is_visible(),
		Some(true),
		"reveal must show the window"
	);
}
