//! Desktop no-ops for activity-owned operations.
use std::path::PathBuf;
pub type AndroidApp = ();
pub fn call_string(_: &str, _: &str) -> anyhow::Result<()> {
	Ok(())
}
pub fn phone_layout() -> bool {
	false
}
pub fn smallest_width_dp() -> i32 {
	0
}
pub fn insets(_: u32, _: u32, _: f32) -> [f32; 4] {
	[0.0; 4]
}
pub fn data_path() -> Option<PathBuf> {
	None
}
pub fn event_loop(
	_: AndroidApp,
) -> anyhow::Result<winit::event_loop::EventLoop> {
	Ok(winit::event_loop::EventLoop::builder().build()?)
}
#[macro_export]
macro_rules! android_entry {
	($start:path, $run:path, $url:path) => {
		const _: fn($crate::android::AndroidApp) -> anyhow::Result<()> = $run;
		const _: fn() = $start;
		const _: fn(&str) -> Option<String> = $url;
	};
}
