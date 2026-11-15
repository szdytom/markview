use markview_render::{Renderer, SurfaceSource};
use std::{cell::Cell, rc::Rc};

struct UnavailableSurface(Rc<Cell<usize>>);

impl SurfaceSource for UnavailableSurface {
	fn instance_descriptor(&self) -> wgpu::InstanceDescriptor {
		wgpu::InstanceDescriptor::new_without_display_handle()
	}

	fn create_surface(
		&self,
		_instance: &wgpu::Instance,
	) -> anyhow::Result<wgpu::Surface<'static>> {
		self.0.set(self.0.get() + 1);
		anyhow::bail!("Surface unavailable");
	}

	fn size(&self) -> (u32, u32) {
		(640, 480)
	}
}

#[test]
fn renderer_retries_surface_creation_for_each_enabled_backend() {
	let attempts = Rc::new(Cell::new(0));
	let result = pollster::block_on(Renderer::new(Some(Box::new(
		UnavailableSurface(attempts.clone()),
	))));
	let error = result
		.err()
		.expect("Every surface attempt fails")
		.to_string();
	let enabled = wgpu::Instance::enabled_backend_features();
	assert_eq!(attempts.get(), enabled.iter().count());
	for backend in enabled.iter() {
		assert!(
			error.contains(&format!("{backend:?}: Surface unavailable")),
			"Missing backend failure: {error}"
		);
	}
}
