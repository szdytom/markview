//! Single-instance policy for native launch documents.
use super::single_instance::Remote;
use super::{Event, SendEvent};
use anyhow::Result;
use winit::event_loop::{EventLoop, EventLoopProxy};

pub(super) fn install(proxy: EventLoopProxy<Event>) {
	markview_platform::install_events(move |event| {
		proxy.send(super::native_event(event))
	});
}
pub(super) fn forward_pending(remote: Remote) -> Result<()> {
	let Some(event_loop) = markview_platform::pending_documents_loop(
		EventLoop::<Event>::with_user_event(),
	)?
	else {
		return Ok(());
	};
	install(event_loop.create_proxy());
	let mut pending = ForwardPending {
		remote,
		result: Ok(()),
	};
	event_loop.run_app(&mut pending)?;
	markview_platform::shutdown_events();
	pending.result
}
struct ForwardPending {
	remote: Remote,
	result: Result<()>,
}

impl ForwardPending {
	fn receive(&mut self, event: Event) {
		if self.result.is_ok()
			&& let Event::Open(Some(path)) = event
		{
			self.result = self.remote.forward(Some(&path));
		}
	}
}

impl winit::application::ApplicationHandler<Event> for ForwardPending {
	fn resumed(&mut self, _: &winit::event_loop::ActiveEventLoop) {}

	fn window_event(
		&mut self,
		_: &winit::event_loop::ActiveEventLoop,
		_: winit::window::WindowId,
		_: winit::event::WindowEvent,
	) {
	}

	fn user_event(
		&mut self,
		_: &winit::event_loop::ActiveEventLoop,
		event: Event,
	) {
		self.receive(event);
	}

	fn about_to_wait(
		&mut self,
		event_loop: &winit::event_loop::ActiveEventLoop,
	) {
		// AppKit has finished launching; `winit` has drained the pending documents.
		event_loop.exit();
	}
}

#[cfg(test)]
mod forwarding_tests {
	use super::*;
	use crate::app::single_instance::{Start, start};
	use std::{path::PathBuf, sync::mpsc, time::Duration};

	#[test]
	fn secondary_forwards_all_desktop_documents_and_reports_delivery_failure() {
		let dir = tempfile::tempdir().unwrap();
		let lock = dir.path().join("instance.lock");
		let Start::Primary(primary) = start(&lock, true, None, false).unwrap()
		else {
			panic!()
		};
		let (tx, rx) = mpsc::channel();
		let listener = primary.listen(move |event| {
			let Event::Activate(path) = event else {
				panic!()
			};
			tx.send(path).is_ok()
		});
		let Start::Forwarded(remote) = start(&lock, true, None, false).unwrap()
		else {
			panic!()
		};
		let timeout = Duration::from_secs(2);
		assert_eq!(rx.recv_timeout(timeout).unwrap(), None);
		let mut pending = ForwardPending {
			remote,
			result: Ok(()),
		};
		for name in ["first.md", "second.md"] {
			let path = dir.path().join(name);
			pending.receive(Event::Open(Some(path.clone())));
			assert!(pending.result.is_ok());
			assert_eq!(rx.recv_timeout(timeout).unwrap(), Some(path));
		}
		drop(rx);
		pending.receive(Event::Open(Some(PathBuf::from("undelivered.md"))));
		assert!(pending.result.is_err());
		drop(listener);
	}
}
