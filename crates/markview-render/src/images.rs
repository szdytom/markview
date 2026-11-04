//! Versioned image textures, GPU budget and per-frame demand publication.
use crate::gpu::Gpu;
use markview_layout::{
	image::{ImageDemand, ImageInfo, ImageSnapshot, raster_size},
	scene::{Draw, LayoutSnapshot, Rect},
};
use std::{
	collections::{HashMap, HashSet},
	ops::Range,
};
/// One image texture: the snapshot generation, source alias and version.
///
/// The generation keeps two documents' identically named sources apart while
/// a hover preview shows another tab's images in the same frame.
pub(super) type ImageKey = (u64, String, u64);
/// Texture bytes image sources may occupy on the GPU at once.
const IMAGE_BUDGET: u64 = 256 * 1024 * 1024;
/// Whether `key` still matches the metadata of the snapshot that draws it.
fn current(entries: &HashMap<String, ImageInfo>, key: &ImageKey) -> bool {
	entries
		.get(&key.1)
		.is_some_and(|info| info.version == key.2 && info.error.is_none())
}
pub(super) struct ImageTextures {
	pub(super) pipeline: wgpu::RenderPipeline,
	cache: HashMap<ImageKey, (wgpu::BindGroup, u64)>,
	// None selects the shared color glyph atlas, in document paint order.
	runs: Vec<(Range<u32>, Option<ImageKey>)>,
	demand: HashMap<String, ImageDemand>,
	images: ImageSnapshot,
	/// The snapshot the main frame draws, whose textures stay cached.
	frame: u64,
	/// The snapshot a preview layer draws, kept cached while a card is open.
	guest: Option<u64>,
	/// The guest snapshot's image metadata, so textures an earlier preview
	/// pass created can be version-checked while the main snapshot is current.
	guest_entries: HashMap<String, ImageInfo>,
	/// Textures the main pass used, which a preview upload must not evict.
	frame_keys: HashSet<ImageKey>,
}
impl ImageTextures {
	pub(super) fn new(pipeline: wgpu::RenderPipeline) -> Self {
		Self {
			pipeline,
			cache: HashMap::new(),
			runs: Vec::new(),
			demand: HashMap::new(),
			images: Default::default(),
			frame: 0,
			guest: None,
			guest_entries: HashMap::new(),
			frame_keys: HashSet::new(),
		}
	}
	pub(super) fn begin(&mut self, snapshot: &ImageSnapshot) {
		self.runs.clear();
		self.images = snapshot.clone();
		self.frame = snapshot.generation;
		self.frame_keys.clear();
		let guest = self.guest;
		let guest_entries = &self.guest_entries;
		self.cache.retain(|key, _| {
			if key.0 == snapshot.generation {
				current(&snapshot.entries, key)
			} else if Some(key.0) == guest {
				current(guest_entries, key)
			} else {
				false
			}
		});
		self.demand.clear();
	}
	/// Switches the pixel source to a preview's snapshot while both
	/// documents' textures stay cached across frames.
	pub(super) fn begin_guest(&mut self, snapshot: &ImageSnapshot) {
		// Record what the main pass just used before its runs are cleared: a
		// preview upload must not evict the frame the reader is looking at.
		self.frame_keys = self
			.runs
			.iter()
			.filter_map(|(_, key)| key.clone())
			.collect();
		self.runs.clear();
		self.images = snapshot.clone();
		self.guest = Some(snapshot.generation);
		self.guest_entries = snapshot.entries.clone();
		let frame = self.frame;
		self.cache.retain(|key, _| {
			if key.0 == snapshot.generation {
				current(&snapshot.entries, key)
			} else {
				key.0 == frame
			}
		});
	}
	/// Forgets the preview snapshot once no card shows it.
	pub(super) fn release_guest(&mut self) {
		self.guest = None;
		self.guest_entries.clear();
	}
	pub(super) fn publish(&mut self) {
		self.images.pixels.publish_demand(
			self.images.generation,
			std::mem::take(&mut self.demand),
		);
	}
	#[cfg(test)]
	pub(super) fn contains(&self, key: &ImageKey) -> bool {
		self.cache.contains_key(key)
	}

	pub(super) fn bytes(&self) -> u64 {
		self.cache.values().map(|(_, bytes)| bytes).sum()
	}
	pub(super) fn runs(&self) -> &[(Range<u32>, Option<ImageKey>)] {
		&self.runs
	}
	pub(super) fn bind_group(&self, key: &ImageKey) -> &wgpu::BindGroup {
		&self.cache[key].0
	}
	pub(super) fn record(&mut self, range: Range<u32>, key: ImageKey) {
		self.runs.push((range, Some(key)));
	}
	pub(super) fn record_color_glyph(&mut self, range: Range<u32>) {
		if let Some((previous, None)) = self.runs.last_mut()
			&& previous.end == range.start
		{
			previous.end = range.end;
			return;
		}
		self.runs.push((range, None));
	}
	/// Makes the texture for one image, without recording paint demand.
	fn ensure(
		&mut self,
		src: &str,
		version: u64,
		gpu: &Gpu,
		allow_evict: bool,
	) -> Option<ImageKey> {
		let key = (self.images.generation, src.to_owned(), version);
		if self.cache.contains_key(&key) {
			return Some(key);
		}
		let pixels = self.images.pixels.get(src, version)?;
		if pixels.width > gpu.device.limits().max_texture_dimension_2d
			|| pixels.height > gpu.device.limits().max_texture_dimension_2d
		{
			return None;
		}
		let bytes = pixels.rgba.len() as u64;
		if self.bytes() + bytes > IMAGE_BUDGET {
			if !allow_evict {
				return None;
			}
			// Keep the current frame and this pass's already drawn images;
			// a prefetch must never displace the frame on screen.
			let mut pinned = self.frame_keys.clone();
			pinned
				.extend(self.runs.iter().filter_map(|(_, used)| used.clone()));
			self.cache.retain(|key, _| pinned.contains(key));
		}
		if self.bytes() + bytes > IMAGE_BUDGET {
			return None;
		}
		let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
			label: Some("document image"),
			size: wgpu::Extent3d {
				width: pixels.width,
				height: pixels.height,
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
		gpu.queue.write_texture(
			texture.as_image_copy(),
			&pixels.rgba,
			wgpu::TexelCopyBufferLayout {
				offset: 0,
				bytes_per_row: Some(pixels.width * 4),
				rows_per_image: Some(pixels.height),
			},
			texture.size(),
		);
		let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
			mag_filter: wgpu::FilterMode::Linear,
			min_filter: wgpu::FilterMode::Linear,
			..Default::default()
		});
		let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("document image"),
			layout: &self.pipeline.get_bind_group_layout(0),
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
		self.cache.insert(key.clone(), (group, bytes));
		Some(key)
	}

	pub(super) fn prepare(
		&mut self,
		src: &String,
		version: u64,
		rect: Rect,
		scale: f32,
		gpu: &Gpu,
	) -> Option<ImageKey> {
		let key = (self.images.generation, src.clone(), version);
		let demand = ImageDemand {
			size: raster_size(rect.w * scale, rect.h * scale),
			needs_pixels: !self.cache.contains_key(&key),
		};
		self.demand
			.entry(src.clone())
			.and_modify(|d| d.merge(demand))
			.or_insert(demand);
		let key = self.ensure(src, version, gpu, true)?;
		self.demand.get_mut(src).unwrap().needs_pixels = false;
		Some(key)
	}

	/// Uploads one image in the current, next, or previous screenful that is
	/// not cached yet. It records no paint demand, so the visible frame's
	/// published demand is untouched.
	pub(super) fn prewarm(
		&mut self,
		snapshot: &LayoutSnapshot,
		view: &crate::View<'_>,
		gpu: &Gpu,
	) -> bool {
		if self.images.generation != snapshot.images.generation {
			return false;
		}
		let height = view.viewport().clip().h.max(1.0);
		let regions = [
			(view.scroll, view.scroll + height),
			(view.scroll + height, view.scroll + 2.0 * height),
			((view.scroll - height).max(0.0), view.scroll),
		];
		for (start, end) in regions {
			let first = snapshot
				.blocks
				.partition_point(|b| b.y + b.height() < start);
			for block in snapshot.blocks.iter().skip(first) {
				if block.y > end {
					break;
				}
				for (_, draw, position) in block.draws() {
					let Draw::Image {
						src, version, rect, ..
					} = draw
					else {
						continue;
					};
					let top = block.y + position[1] + rect.y;
					if top > end || top + rect.h < start {
						continue;
					}
					let key = (self.images.generation, src.clone(), *version);
					if self.cache.contains_key(&key) {
						continue;
					}
					if self.ensure(src, *version, gpu, false).is_some() {
						return true;
					}
				}
			}
		}
		false
	}
}

#[cfg(test)]
mod tests {
	use crate::{Renderer, Theme, View};
	use markview_layout::{
		image::{ImageInfo, ImagePixels, ImageSnapshot},
		scene::{BlockLayout, Draw, LayoutSnapshot, PlacedBlock, Rect},
	};
	use std::{collections::HashMap, sync::Arc};

	/// A frame with one image drawn in a box `w` by `h` logical pixels large,
	/// at the device `scale`.
	fn demand_for(w: f32, h: f32, scale: f32) -> (u32, u32) {
		let mut renderer = pollster::block_on(Renderer::new(None)).unwrap();
		let pixels = Arc::new(ImagePixels::default());
		let layout = Arc::new(BlockLayout {
			draws: vec![Draw::Image {
				src: "mermaid:graph".into(),
				version: 1,
				rect: Rect { x: 0., y: 0., w, h },
				title: String::new(),
			}],
			height: h,
			width: w,
			..Default::default()
		});
		let snapshot = LayoutSnapshot {
			images: ImageSnapshot {
				entries: HashMap::from([(
					"mermaid:graph".into(),
					ImageInfo {
						version: 1,
						size: Some((800, 600)),
						error: None,
						svg: false,
					},
				)]),
				pixels: pixels.clone(),
				..Default::default()
			},
			blocks: vec![PlacedBlock {
				id: markview_layout::document::BlockId::new(1).unwrap(),
				content: Default::default(),
				y: 0.,
				flow: Arc::new(layout.resolve_flow(&Default::default(), false)),
				layout,
			}],
			height: h,
			width: w,
			..Default::default()
		};
		let view = View {
			selection: None,
			revision: 0,
			width: (w / 2.) as u32,
			height: (h / 2.) as u32,
			scale,
			scroll: 0.,
			left: 0.,
			top: 0.,
			bottom: 0.,
			theme: Theme::Light,
			horizontal: &HashMap::new(),
			hovered_link: None,
			held_overflow: None,
		};
		let target = renderer.offscreen(view.width, view.height);
		let submission = renderer
			.render(
				&snapshot,
				&view,
				&[],
				&target.create_view(&Default::default()),
			)
			.unwrap();
		renderer.wait(Some(submission)).unwrap();
		pixels.demand(0)["mermaid:graph"].size
	}

	/// A picture zoomed past the cap is re-rastered smaller in both axes, not
	/// clipped in one and honoured in the other.
	#[test]
	#[ignore = "requires a GPU"]
	fn a_demand_past_the_cap_keeps_the_pictures_proportions() {
		assert_eq!(demand_for(1500., 1125., 2.), (3000, 2250));
		assert_eq!(demand_for(3000., 2250., 2.), (4000, 3000));
		assert_eq!(demand_for(6000., 4500., 2.), (4000, 3000));
	}
}
