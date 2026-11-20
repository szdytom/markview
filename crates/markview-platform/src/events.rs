//! Native lifecycle events delivered without application types.
use std::{
	path::PathBuf,
	sync::{Arc, Mutex},
};

/// Why this reader process was launched. Later open requests do not change it.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum LaunchReason {
	#[default]
	Application,
	Document,
	NewWindow,
}

/// A process's launch evidence, sealed when native startup finishes.
#[cfg(target_os = "macos")]
#[derive(Clone, Default)]
pub struct ReaderLaunch(Arc<Mutex<LaunchState>>);

#[cfg(target_os = "macos")]
#[derive(Default)]
struct LaunchState {
	reason: LaunchReason,
	finished: bool,
}

#[cfg(target_os = "macos")]
impl ReaderLaunch {
	pub fn new(reason: LaunchReason) -> Self {
		Self(Arc::new(Mutex::new(LaunchState {
			reason,
			finished: false,
		})))
	}

	pub(crate) fn opened_document(&self) {
		let mut state = self.0.lock().unwrap();
		if !state.finished && state.reason == LaunchReason::Application {
			state.reason = LaunchReason::Document;
		}
	}

	pub fn finish(&self) {
		self.0.lock().unwrap().finished = true;
	}

	pub fn reason(&self) -> Option<LaunchReason> {
		let state = self.0.lock().unwrap();
		state.finished.then_some(state.reason)
	}
}

/// Platforms that exit on close do not track a launch reason.
#[cfg(not(target_os = "macos"))]
#[derive(Clone, Default)]
pub struct ReaderLaunch {}

#[cfg(not(target_os = "macos"))]
impl ReaderLaunch {
	pub fn new(_: LaunchReason) -> Self {
		Self {}
	}
	pub fn finish(&self) {}
	pub fn reason(&self) -> Option<LaunchReason> {
		None
	}
}

pub enum NativeEvent {
	Reopen,
	Open(PathBuf),
	Shared(PathBuf),
	OpenUrl(String),
	InputText(String),
	InputDelete {
		before: usize,
		after: usize,
		code_points: bool,
	},
	Back,
	Configuration,
	AssetsChanged,
	#[cfg(debug_assertions)]
	Inspect(std::sync::mpsc::Sender<String>),
}
type Callback = Arc<dyn Fn(NativeEvent) + Send + Sync>;
#[derive(Default)]
struct Bridge {
	callback: Option<Callback>,
	pending: Vec<NativeEvent>,
}
impl Bridge {
	#[cfg(any(test, target_os = "android", target_os = "macos"))]
	fn send(&mut self, event: NativeEvent) {
		if let Some(callback) = &self.callback {
			callback(event);
		} else if matches!(
			event,
			NativeEvent::Reopen
				| NativeEvent::Open(_)
				| NativeEvent::Shared(_)
				| NativeEvent::OpenUrl(_)
		) {
			self.pending.push(event);
		}
	}
	fn install(&mut self, callback: Callback) {
		for event in self.pending.drain(..) {
			callback(event);
		}
		self.callback = Some(callback);
	}
	fn clear(&mut self) {
		self.callback = None;
		self.pending.clear();
	}
}
static BRIDGE: Mutex<Bridge> = Mutex::new(Bridge {
	callback: None,
	pending: Vec::new(),
});
// Callbacks only enqueue events; serializing delivery preserves startup order.
#[cfg(any(target_os = "android", target_os = "macos"))]
pub(crate) fn send(event: NativeEvent) {
	BRIDGE.lock().unwrap().send(event);
}
pub fn install_events(callback: impl Fn(NativeEvent) + Send + Sync + 'static) {
	install_reader_events(ReaderLaunch::default(), callback);
}

pub fn install_reader_events(
	_launch: ReaderLaunch,
	callback: impl Fn(NativeEvent) + Send + Sync + 'static,
) {
	BRIDGE.lock().unwrap().install(Arc::new(callback));
	#[cfg(target_os = "macos")]
	crate::open_document::install(_launch);
}
pub fn shutdown_events() {
	BRIDGE.lock().unwrap().clear();
}

#[cfg(test)]
mod tests {
	use super::*;
	#[cfg(target_os = "macos")]
	#[test]
	fn only_documents_received_during_launch_change_its_reason() {
		use LaunchReason::*;
		for (initial, early_files, expected) in [
			(Application, 0, Application),
			(Application, 2, Document),
			(Document, 0, Document),
			(NewWindow, 1, NewWindow),
		] {
			let reader = ReaderLaunch::new(initial);
			let observer = reader.clone();
			for _ in 0..early_files {
				observer.opened_document();
			}
			assert!(reader.reason().is_none());
			observer.finish();
			observer.opened_document();
			assert_eq!(reader.reason(), Some(expected));
		}
	}

	#[cfg(not(target_os = "macos"))]
	#[test]
	fn readers_exit_on_close_without_tracking_launches() {
		assert!(!crate::supports_windowless_reader());
		let launch = ReaderLaunch::new(LaunchReason::Application);
		launch.finish();
		assert_eq!(launch.reason(), None);
		assert_eq!(std::mem::size_of::<ReaderLaunch>(), 0);
	}

	#[test]
	fn startup_preserves_order_and_shutdown_releases_callbacks_and_pending_events()
	 {
		let mut bridge = Bridge::default();
		let received = Arc::new(Mutex::new(Vec::new()));
		bridge.send(NativeEvent::Open("first.md".into()));
		bridge.send(NativeEvent::Reopen);
		bridge.send(NativeEvent::Back);
		bridge.send(NativeEvent::Shared("second.md".into()));
		let sink = received.clone();
		bridge.install(Arc::new(move |event| {
			let path = match event {
				NativeEvent::Open(path) | NativeEvent::Shared(path) => {
					Some(path)
				}
				NativeEvent::Reopen => None,
				_ => panic!(),
			};
			sink.lock().unwrap().push(path);
		}));
		bridge.send(NativeEvent::Open("third.md".into()));
		bridge.send(NativeEvent::Reopen);
		assert_eq!(
			*received.lock().unwrap(),
			[
				Some(PathBuf::from("first.md")),
				None,
				Some(PathBuf::from("second.md")),
				Some(PathBuf::from("third.md")),
				None,
			]
		);
		assert_eq!(Arc::strong_count(&received), 2);
		bridge.clear();
		assert_eq!(Arc::strong_count(&received), 1);
		bridge.send(NativeEvent::Open("discarded.md".into()));
		bridge.clear();
		bridge.install(Arc::new(|_| panic!("pending event survived shutdown")));
	}
}
