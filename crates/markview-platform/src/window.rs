//! Uniform window adapters and native event-loop configuration.
use winit::{
	event_loop::{EventLoop, EventLoopBuilder},
	window::{ResizeDirection, Window, WindowAttributes},
};

#[derive(Clone, Copy, Debug)]
pub struct FrameMetrics {
	pub width: f32,
	pub height: f32,
	pub scale: f32,
	pub fullscreen: bool,
	pub maximized: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitTest {
	Client,
	Caption,
	Maximize,
	Resize(ResizeDirection),
}

#[cfg(not(windows))]
pub struct NativeFrame;
#[cfg(not(windows))]
impl NativeFrame {
	pub fn new(
		_: &dyn Window,
		_: impl Fn(FrameMetrics, f32, f32) -> HitTest + 'static,
		_: impl Fn() + 'static,
	) -> anyhow::Result<Option<Self>> {
		Ok(None)
	}
	pub fn set_fullscreen(&self, _: bool) {}
	pub fn feedback(&self) -> (bool, bool) {
		(false, false)
	}
	pub fn cancel(&self) {}
}

pub fn window_attributes(
	attributes: WindowAttributes,
	native_buttons: bool,
) -> WindowAttributes {
	#[cfg(target_os = "macos")]
	let attributes = {
		use winit::platform::macos::WindowAttributesMacOS;
		attributes.with_platform_attributes(Box::new(
			WindowAttributesMacOS::default()
				.with_titlebar_transparent(true)
				.with_title_hidden(true)
				.with_fullsize_content_view(true)
				.with_titlebar_buttons_hidden(!native_buttons)
				.with_movable_by_window_background(false),
		))
	};
	#[cfg(windows)]
	let attributes = {
		use winit::platform::windows::{
			CornerPreference, WindowAttributesWindows,
		};
		// Reveal only after the first frame is ready.
		attributes
			.with_decorations(false)
			.with_visible(false)
			.with_platform_attributes(Box::new(
				WindowAttributesWindows::default()
					.with_undecorated_shadow(true)
					.with_corner_preference(CornerPreference::Round)
					.with_taskbar_icon(crate::icon::window_icon()),
			))
	};
	#[cfg(target_os = "linux")]
	let attributes = {
		use winit::platform::wayland::WindowAttributesWayland;
		attributes
			.with_decorations(false)
			.with_platform_attributes(Box::new(
				WindowAttributesWayland::default()
					.with_name("markview", "markview"),
			))
	};
	let _ = native_buttons;
	crate::icon::with_icon(attributes)
}

/// Whether a new window stays hidden until the reader has a frame for it.
pub fn starts_hidden() -> bool {
	cfg!(windows)
}

/// Shows a window this platform created hidden; a no-op elsewhere.
pub fn reveal(window: &dyn Window) {
	#[cfg(windows)]
	window.set_visible(true);
	#[cfg(not(windows))]
	let _ = window;
}

pub fn pending_documents_loop(
	mut builder: EventLoopBuilder,
) -> Result<Option<EventLoop>, winit::error::EventLoopError> {
	#[cfg(target_os = "macos")]
	{
		use winit::platform::macos::{
			ActivationPolicy, EventLoopBuilderExtMacOS,
		};
		builder
			.with_activation_policy(ActivationPolicy::Accessory)
			.with_activate_ignoring_other_apps(false);
		builder.build().map(Some)
	}
	#[cfg(not(target_os = "macos"))]
	{
		let _ = &mut builder;
		Ok(None)
	}
}

#[cfg(all(test, not(windows)))]
mod tests {
	use super::*;
	#[test]
	fn inactive_frame_and_touchpad_adapters_have_no_feedback_or_work() {
		let frame = NativeFrame;
		frame.set_fullscreen(true);
		frame.cancel();
		assert_eq!(frame.feedback(), (false, false));
		let mut pad = crate::dm::DirectManipulation;
		pad.contact(1);
		pad.resize(800, 500, 2.0);
		pad.abandon();
		assert!(pad.pump().statuses.is_empty());
		assert!(pad.pump().transform.is_none());
		assert!(pad.deadline(std::time::Instant::now()).is_none());
		assert!(
			crate::dm::DirectManipulation::take_offered_pointers().is_empty()
		);
	}
}
