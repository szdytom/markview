//! Native lifecycle events delivered without application types.
use std::{
	path::PathBuf,
	sync::{Arc, Mutex},
};

pub enum NativeEvent {
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
			NativeEvent::Open(_)
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
	BRIDGE.lock().unwrap().install(Arc::new(callback));
	#[cfg(target_os = "macos")]
	crate::open_document::install();
}
pub fn shutdown_events() {
	BRIDGE.lock().unwrap().clear();
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn startup_preserves_order_and_shutdown_releases_callbacks_and_pending_events()
	 {
		let mut bridge = Bridge::default();
		let received = Arc::new(Mutex::new(Vec::new()));
		bridge.send(NativeEvent::Open("first.md".into()));
		bridge.send(NativeEvent::Back);
		bridge.send(NativeEvent::Shared("second.md".into()));
		let sink = received.clone();
		bridge.install(Arc::new(move |event| {
			let path = match event {
				NativeEvent::Open(path) | NativeEvent::Shared(path) => path,
				_ => panic!(),
			};
			sink.lock().unwrap().push(path);
		}));
		bridge.send(NativeEvent::Open("third.md".into()));
		assert_eq!(
			*received.lock().unwrap(),
			[
				PathBuf::from("first.md"),
				PathBuf::from("second.md"),
				PathBuf::from("third.md")
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
