use super::geometry::Vertex;
pub(super) fn create(
	device: &wgpu::Device,
	format: wgpu::TextureFormat,
) -> (wgpu::RenderPipeline, wgpu::RenderPipeline) {
	let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
		label: Some("text and rectangles"),
		source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
	});
	let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("Markview flat pipeline"), layout: None,
			vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[Some(wgpu::VertexBufferLayout {
				array_stride: std::mem::size_of::<Vertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex,
				attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4],
			})] },
			fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState {
				format, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL,
			})] }),
			primitive: Default::default(), depth_stencil: None, multisample: Default::default(), multiview_mask: None, cache: None,
		});
	let image_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("sRGB images"), layout: None,
			vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[Some(wgpu::VertexBufferLayout {
				array_stride: std::mem::size_of::<Vertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex,
				attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4],
			})] },
			fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("image_fs"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState {
				format, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL,
			})] }),
			primitive: Default::default(), depth_stencil: None, multisample: Default::default(), multiview_mask: None, cache: None,
		});
	(pipeline, image_pipeline)
}
