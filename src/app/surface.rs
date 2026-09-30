//! The desktop window as a render target.
//!
//! The renderer is handed a [`SurfaceSource`] rather than a window, so it
//! never names a windowing toolkit. This is the implementation the desktop
//! build supplies; a front end that draws into a canvas implements the same
//! trait over its canvas instead.
use crate::render::SurfaceSource;
use anyhow::Result;
use std::sync::Arc;
use winit::window::Window;

/// A window named as the target its surface presents through.
///
/// The trait is defined in another crate and `Arc<Window>` is not local
/// either, so the implementation needs a name of its own to hang on.
pub(super) struct WindowTarget(Arc<Window>);

/// The render target a desktop window presents through.
pub(super) fn target(window: Arc<Window>) -> Box<dyn SurfaceSource> {
	Box::new(WindowTarget(window))
}

impl SurfaceSource for WindowTarget {
	fn instance_descriptor(&self) -> wgpu::InstanceDescriptor {
		wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(
			self.0.clone(),
		))
	}
	fn create_surface(
		&self,
		instance: &wgpu::Instance,
	) -> Result<wgpu::Surface<'static>> {
		Ok(instance.create_surface(self.0.clone())?)
	}
	fn size(&self) -> (u32, u32) {
		let size = self.0.inner_size();
		(size.width, size.height)
	}
}
