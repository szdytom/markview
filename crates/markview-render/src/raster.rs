//! Bounded glyph/path atlas and reusable rasterization resources.
use markview_core::{
	document::fingerprint, scene::Glyph, style::SYNTHETIC_ITALIC_ANGLE_DEG,
};
use parley::FontData;
use std::collections::HashMap;
use swash::{
	FontRef,
	scale::{Render, ScaleContext, Source},
	zeno::{Angle, Format, Transform, Vector},
};
pub(super) const ATLAS_SIZE: u32 = 2048;
mod color;
pub(super) const COLOR_ATLAS_SIZE: u32 = color::SIZE;

/// Keep layout advances fractional, but bake horizontal subpixel coverage into
/// the raster itself. Atlas texels must land on physical pixels one-to-one:
/// translating an antialiased bitmap fractionally filters its edges twice.
/// Four phases bound the cache cost and position error to 1/8 physical pixel.
#[derive(Debug)]
pub(super) struct GlyphOrigin {
	pub(super) x: f32,
	pub(super) y: f32,
	pub(super) phase: u8,
}

impl GlyphOrigin {
	pub(super) fn new(x: f32, y: f32, scale: f32) -> Self {
		let phased_x = (x * scale * 4.0).round();
		Self {
			x: (phased_x / 4.0).floor(),
			y: (y * scale).round(),
			phase: phased_x.rem_euclid(4.0) as u8,
		}
	}
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
enum RasterKey {
	Glyph {
		font: u64,
		index: u32,
		id: u16,
		size: u32,
		phase: u8,
		coords: u64,
		synthetic: bool,
	},
	Path {
		path: u64,
		size: u32,
		tx: i32,
		ty: i32,
		w: u32,
		h: u32,
	},
}

#[derive(Clone, Copy, Default)]
pub(super) struct Entry {
	pub(super) color: bool,
	pub(super) x: u32,
	pub(super) y: u32,
	pub(super) w: u32,
	pub(super) h: u32,
	pub(super) left: f32,
	pub(super) top: f32,
}

pub(super) struct RasterCache {
	atlas: wgpu::Texture,
	bind_group: wgpu::BindGroup,
	cache: HashMap<RasterKey, Entry>,
	shelf: (u32, u32, u32),
	scaler: ScaleContext,
	math_fonts: HashMap<String, FontData>,
	atlas_full: bool,
	color_atlas: Option<color::ColorAtlas>,
}
impl RasterCache {
	pub(super) fn new(
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		pipeline: &wgpu::RenderPipeline,
	) -> Self {
		let atlas = device.create_texture(&wgpu::TextureDescriptor {
			label: Some("4 MiB glyph mask atlas"),
			size: wgpu::Extent3d {
				width: ATLAS_SIZE,
				height: ATLAS_SIZE,
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: wgpu::TextureFormat::R8Unorm,
			usage: wgpu::TextureUsages::TEXTURE_BINDING
				| wgpu::TextureUsages::COPY_DST,
			view_formats: &[],
		});
		let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
			label: None,
			mag_filter: wgpu::FilterMode::Linear,
			min_filter: wgpu::FilterMode::Linear,
			..Default::default()
		});
		let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: None,
			layout: &pipeline.get_bind_group_layout(0),
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: wgpu::BindingResource::TextureView(
						&atlas.create_view(&Default::default()),
					),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::Sampler(&sampler),
				},
			],
		});
		let mut cache = Self {
			atlas,
			bind_group,
			cache: HashMap::new(),
			shelf: (2, 0, 2),
			scaler: ScaleContext::new(),
			math_fonts: HashMap::new(),
			atlas_full: false,
			color_atlas: None,
		};
		cache.reset_atlas(queue);
		cache
	}
	pub(super) fn full(&self) -> bool {
		self.atlas_full
	}
	pub(super) fn bind_group(&self) -> &wgpu::BindGroup {
		&self.bind_group
	}
	pub(super) fn color_bind_group(&self) -> &wgpu::BindGroup {
		&self
			.color_atlas
			.as_ref()
			.expect("prepared color glyph atlas")
			.bind_group
	}
	pub(super) fn color_bytes(&self) -> u64 {
		if self.color_atlas.is_some() {
			u64::from(COLOR_ATLAS_SIZE).pow(2) * 4
		} else {
			0
		}
	}
	pub(super) fn reset_atlas(&mut self, queue: &wgpu::Queue) {
		self.cache.clear();
		self.shelf = (2, 0, 2);
		self.atlas_full = false;
		if let Some(atlas) = &mut self.color_atlas {
			atlas.reset();
		}
		self.upload(queue, 0, 0, 1, 1, &[255]);
	}
	fn upload(
		&self,
		queue: &wgpu::Queue,
		x: u32,
		y: u32,
		w: u32,
		h: u32,
		data: &[u8],
	) {
		queue.write_texture(
			wgpu::TexelCopyTextureInfo {
				texture: &self.atlas,
				mip_level: 0,
				origin: wgpu::Origin3d { x, y, z: 0 },
				aspect: wgpu::TextureAspect::All,
			},
			data,
			wgpu::TexelCopyBufferLayout {
				offset: 0,
				bytes_per_row: Some(w),
				rows_per_image: Some(h),
			},
			wgpu::Extent3d {
				width: w,
				height: h,
				depth_or_array_layers: 1,
			},
		);
	}
	fn insert(
		&mut self,
		queue: &wgpu::Queue,
		key: RasterKey,
		mut entry: Entry,
		data: &[u8],
	) -> Option<Entry> {
		if entry.w == 0 || entry.h == 0 {
			self.cache.insert(key, entry);
			return Some(entry);
		}
		let w = entry.w + 2;
		let h = entry.h + 2;
		if self.shelf.0 + w > ATLAS_SIZE {
			self.shelf.0 = 0;
			self.shelf.1 += self.shelf.2;
			self.shelf.2 = 0;
		}
		if w > ATLAS_SIZE || self.shelf.1 + h > ATLAS_SIZE {
			self.atlas_full = true;
			return None;
		}
		entry.x = self.shelf.0 + 1;
		entry.y = self.shelf.1 + 1;
		// Clear the one-pixel border as well: atlas resets may leave old texels.
		let mut padded = vec![0u8; (w * h) as usize];
		for row in 0..entry.h as usize {
			padded[(row + 1) * w as usize + 1
				..(row + 1) * w as usize + 1 + entry.w as usize]
				.copy_from_slice(
					&data[row * entry.w as usize..(row + 1) * entry.w as usize],
				);
		}
		self.upload(queue, self.shelf.0, self.shelf.1, w, h, &padded);
		self.shelf.0 += w;
		self.shelf.2 = self.shelf.2.max(h);
		self.cache.insert(key, entry);
		Some(entry)
	}

	pub(super) fn glyph(
		&mut self,
		device: &wgpu::Device,
		color_pipeline: &wgpu::RenderPipeline,
		queue: &wgpu::Queue,
		g: &Glyph,
		scale: f32,
		phase: u8,
	) -> Option<Entry> {
		let size = (g.size * scale * 4.0).round().max(1.0) as u32;
		let key = RasterKey::Glyph {
			font: g.font.data.id(),
			index: g.font.index,
			id: g.id,
			size,
			phase,
			coords: fingerprint(&g.coords),
			synthetic: g.synthetic_italic,
		};
		if let Some(entry) = self.cache.get(&key) {
			return Some(*entry);
		}
		let font =
			FontRef::from_index(g.font.data.data(), g.font.index as usize)?;
		let mut scaler = self
			.scaler
			.builder_with_id(font, [g.font.data.id(), g.font.index as u64])
			.size(size as f32 / 4.0)
			.hint(true)
			.normalized_coords(g.coords.iter())
			.build();
		let mut render = Render::new(&[
			Source::ColorOutline(0),
			Source::ColorBitmap(swash::scale::StrikeWith::BestFit),
			Source::Outline,
		]);
		render
			.format(Format::Alpha)
			.offset(Vector::new(phase as f32 / 4.0, 0.0));
		if g.synthetic_italic {
			// Shear about the baseline in font units, where y grows upward.
			render.transform(Some(Transform::skew(
				Angle::from_degrees(SYNTHETIC_ITALIC_ANGLE_DEG),
				Angle::from_degrees(0.0),
			)));
		}
		let image = render.render(&mut scaler, g.id);
		let Some(mut image) = image else {
			let e = Entry::default();
			self.cache.insert(key, e);
			return Some(e);
		};
		let entry = Entry {
			w: image.placement.width,
			h: image.placement.height,
			left: image.placement.left as f32,
			top: -image.placement.top as f32,
			..Default::default()
		};
		let data = match image.content {
			swash::scale::image::Content::Mask => image.data,
			swash::scale::image::Content::Color => {
				if matches!(image.source, Source::ColorOutline(_)) {
					color::unpremultiply(&mut image.data);
				}
				let atlas = self.color_atlas.get_or_insert_with(|| {
					color::ColorAtlas::new(device, color_pipeline)
				});
				let Some(entry) = atlas.insert(queue, entry, &image.data)
				else {
					self.atlas_full = true;
					return None;
				};
				self.cache.insert(key, entry);
				return Some(entry);
			}
			swash::scale::image::Content::SubpixelMask => image
				.data
				.as_chunks::<4>()
				.0
				.iter()
				.map(|p| p[0].max(p[1]).max(p[2]))
				.collect(),
		};
		self.insert(queue, key, entry, &data)
	}

	pub(super) fn math_font(&mut self, name: &str) -> Option<FontData> {
		if let Some(f) = self.math_fonts.get(name) {
			return Some(f.clone());
		}
		let bytes = ratex_katex_fonts::ttf_bytes(&format!("KaTeX_{name}.ttf"))?;
		let font = FontData::new(bytes.into_owned().into(), 0);
		self.math_fonts.insert(name.into(), font.clone());
		Some(font)
	}
}
mod path;
#[cfg(test)]
mod tests;
