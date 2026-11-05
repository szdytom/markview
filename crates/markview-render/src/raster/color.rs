//! Lazily allocated color glyph atlas; ordinary text keeps its R8 mask atlas.
#[cfg(test)]
mod proptests;

use super::Entry;

pub(super) const SIZE: u32 = 512;

// Swash composites COLR outline layers into premultiplied RGBA, whereas its
// decoded color PNG bitmaps already have straight alpha. The image pipeline
// expects straight alpha for both, including on a dark background.
pub(super) fn unpremultiply(rgba: &mut [u8]) {
	for pixel in rgba.as_chunks_mut::<4>().0 {
		let a = u32::from(pixel[3]);
		for c in &mut pixel[..3] {
			*c = (u32::from(*c) * 255 + a / 2)
				.checked_div(a)
				.unwrap_or(0)
				.min(255) as u8;
		}
	}
}
pub(super) struct ColorAtlas {
	texture: wgpu::Texture,
	pub(super) bind_group: wgpu::BindGroup,
	size: u32,
	shelf: (u32, u32, u32),
}
impl ColorAtlas {
	pub(super) fn new(
		device: &wgpu::Device,
		pipeline: &wgpu::RenderPipeline,
		size: u32,
	) -> Self {
		let texture = device.create_texture(&wgpu::TextureDescriptor {
			label: Some("color glyph atlas"),
			size: wgpu::Extent3d {
				width: size,
				height: size,
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: wgpu::TextureFormat::Rgba8UnormSrgb,
			usage: wgpu::TextureUsages::TEXTURE_BINDING
				| wgpu::TextureUsages::COPY_DST,
			view_formats: &[],
		});
		let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
			mag_filter: wgpu::FilterMode::Linear,
			min_filter: wgpu::FilterMode::Linear,
			..Default::default()
		});
		let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("color glyphs"),
			layout: &pipeline.get_bind_group_layout(0),
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: wgpu::BindingResource::TextureView(
						&texture.create_view(&Default::default()),
					),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::Sampler(&sampler),
				},
			],
		});
		Self {
			texture,
			bind_group,
			size,
			shelf: (0, 0, 0),
		}
	}
	pub(super) fn reset(&mut self) {
		self.shelf = (0, 0, 0);
	}
	pub(super) fn insert(
		&mut self,
		queue: &wgpu::Queue,
		mut entry: Entry,
		rgba: &[u8],
	) -> Option<Entry> {
		let (w, h) = (entry.w + 2, entry.h + 2);
		if self.shelf.0 + w > self.size {
			self.shelf.0 = 0;
			self.shelf.1 += self.shelf.2;
			self.shelf.2 = 0;
		}
		if w > self.size || self.shelf.1 + h > self.size {
			return None;
		}
		let mut padded = vec![0; (w * h * 4) as usize];
		for row in 0..entry.h as usize {
			let start = ((row + 1) * w as usize + 1) * 4;
			let stride = entry.w as usize * 4;
			padded[start..start + stride]
				.copy_from_slice(&rgba[row * stride..(row + 1) * stride]);
		}
		queue.write_texture(
			wgpu::TexelCopyTextureInfo {
				texture: &self.texture,
				mip_level: 0,
				origin: wgpu::Origin3d {
					x: self.shelf.0,
					y: self.shelf.1,
					z: 0,
				},
				aspect: wgpu::TextureAspect::All,
			},
			&padded,
			wgpu::TexelCopyBufferLayout {
				offset: 0,
				bytes_per_row: Some(w * 4),
				rows_per_image: Some(h),
			},
			wgpu::Extent3d {
				width: w,
				height: h,
				depth_or_array_layers: 1,
			},
		);
		entry.x = self.shelf.0 + 1;
		entry.y = self.shelf.1 + 1;
		entry.color = true;
		self.shelf.0 += w;
		self.shelf.2 = self.shelf.2.max(h);
		Some(entry)
	}
}
