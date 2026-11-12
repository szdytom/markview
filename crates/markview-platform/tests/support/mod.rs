/// Creates a fixture window while keeping its event loop available to the test.
#[cfg(windows)]
pub fn create_window(
	event_loop: &mut winit::event_loop::EventLoop,
	attributes: winit::window::WindowAttributes,
) -> Box<dyn winit::window::Window> {
	use winit::{
		application::ApplicationHandler,
		event_loop::{ActiveEventLoop, run_on_demand::EventLoopExtRunOnDemand},
		window::{Window, WindowId},
	};
	struct Create {
		attributes: Option<winit::window::WindowAttributes>,
		window: Option<Box<dyn Window>>,
	}
	impl ApplicationHandler for Create {
		fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
			self.window = Some(
				event_loop
					.create_window(self.attributes.take().unwrap())
					.unwrap(),
			);
			event_loop.exit();
		}
		fn window_event(
			&mut self,
			_: &dyn ActiveEventLoop,
			_: WindowId,
			_: winit::event::WindowEvent,
		) {
		}
	}
	let mut create = Create {
		attributes: Some(attributes),
		window: None,
	};
	let wake = event_loop.create_proxy();
	let (done, finished) = std::sync::mpsc::channel();
	let watchdog = std::thread::spawn(move || {
		if finished
			.recv_timeout(std::time::Duration::from_secs(10))
			.is_err()
		{
			wake.wake_up();
			false
		} else {
			true
		}
	});
	event_loop.run_app_on_demand(&mut create).unwrap();
	done.send(()).unwrap();
	assert!(
		watchdog.join().unwrap(),
		"exit must not wait for another native message"
	);
	create.window.unwrap()
}
