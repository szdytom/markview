//! Event-driven wgpu renderer. Only visible glyphs and paths are rasterized.
mod frame;
mod geometry;
mod gpu;
mod images;
mod paint;
mod pipeline;
mod raster;
use anyhow::{Context, Result};
use markview_core::{
	scene::{Paint, Rect},
	shaping::TextShaper,
};
pub use raster::RasterStats;
use std::{collections::HashMap, sync::Arc, time::Duration};
use winit::window::Window;
#[derive(
	Clone,
	Copy,
	Debug,
	Default,
	PartialEq,
	Eq,
	serde::Serialize,
	serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
	#[default]
	Light,
	Dark,
}
impl Theme {
	pub fn color(self, p: Paint) -> [f32; 4] {
		markview_core::style::Stylesheet::bundled(self == Self::Dark).paint(p)
	}
}

#[derive(Clone)]
pub struct View<'a> {
	pub selection: Option<markview_core::text::TextSelection>,
	pub revision: u64,
	pub width: u32,
	pub height: u32,
	pub scale: f32,
	pub scroll: f32,
	pub left: f32,
	pub top: f32,
	pub bottom: f32,
	pub theme: Theme,
	pub horizontal: &'a HashMap<(usize, usize), f32>,
	pub hovered_link: Option<&'a str>,
	/// The wide block whose horizontal bar the pointer is over. Tracked by the
	/// application, which owns the redraw, so the bar thickens on hover even
	/// though pointer motion elsewhere does not repaint.
	pub hovered_overflow: Option<(usize, usize)>,
	/// The wide block whose horizontal bar the reader is currently dragging.
	pub held_overflow: Option<(usize, usize)>,
}

impl View<'_> {
	pub fn viewport(&self) -> markview_core::scene::Viewport {
		markview_core::scene::Viewport {
			width: self.width as f32 / self.scale,
			height: self.height as f32 / self.scale,
			left: self.left,
			top: self.top,
			bottom: self.bottom,
			scroll: self.scroll,
		}
	}
}

pub struct Renderer {
	gpu: gpu::Gpu,
	raster: raster::RasterCache,
	images: images::ImageTextures,
	geometry: geometry::Geometry,
	pipeline: wgpu::RenderPipeline,
	pointer: Option<(f32, f32)>,
	/// While a prewarm pass runs, the frame it builds is thrown away, so it
	/// must leave the published image demand and every cached texture exactly
	/// as the frame on screen left them.
	prewarming: bool,
	stylesheet: Option<Arc<markview_core::style::Stylesheet>>,
	fallback: Option<TextShaper>,
	pub adapter_name: String,
}

/// A rendered texture read back as tightly packed, non-premultiplied sRGB RGBA8.
pub struct Readback {
	pub width: u32,
	pub height: u32,
	pub rgba: Vec<u8>,
}
pub enum FrameStatus {
	Ready(wgpu::SurfaceTexture, bool),
	Retry(Duration),
	Occluded,
}
impl Renderer {
	pub fn set_pointer(&mut self, pointer: Option<(f32, f32)>) {
		self.pointer = pointer;
	}

	pub fn set_stylesheet(
		&mut self,
		style: Arc<markview_core::style::Stylesheet>,
	) {
		self.stylesheet = Some(style);
	}
	fn color(&self, paint: Paint, theme: Theme) -> [f32; 4] {
		self.stylesheet
			.as_ref()
			.map_or_else(|| theme.color(paint), |s| s.paint(paint))
	}
	/// Hovering adds a condition to the chain, so `["link", "hover"]` can
	/// override every link rule while leaving the rest of the chain intact.
	fn hover_paint(paint: Paint) -> Paint {
		use markview_core::style::{Condition, chain_push};
		match paint {
			Paint::Styled(Condition::Link, field) => {
				Paint::Styled(Condition::Hover, field)
			}
			Paint::Cascade(chain, field) => {
				Paint::Cascade(chain_push(chain, Condition::Hover), field)
			}
			paint => paint,
		}
	}

	pub async fn new(window: Option<Arc<Window>>) -> Result<Self> {
		let gpu = gpu::Gpu::new(window).await?;
		let (pipeline, image_pipeline) =
			pipeline::create(&gpu.device, gpu.format);
		let raster =
			raster::RasterCache::new(&gpu.device, &gpu.queue, &pipeline);
		let geometry = geometry::Geometry::new(&gpu.device);
		let adapter_name = gpu.adapter_name.clone();
		Ok(Self {
			gpu,
			raster,
			geometry,
			pipeline,
			adapter_name,
			images: images::ImageTextures::new(image_pipeline),
			pointer: None,
			prewarming: false,
			stylesheet: None,
			fallback: None,
		})
	}
	pub fn acquire(&mut self, window: Arc<Window>) -> Result<FrameStatus> {
		self.gpu.acquire(window)
	}
	pub fn resize(&mut self, width: u32, height: u32) {
		self.gpu.resize(width, height);
	}
	pub fn on_device_lost(&self, callback: impl Fn() + Send + 'static) {
		self.gpu.on_device_lost(callback);
	}
	pub fn wait(&self, index: Option<wgpu::SubmissionIndex>) -> Result<()> {
		self.gpu.wait(index)
	}
	pub fn offscreen(&self, width: u32, height: u32) -> wgpu::Texture {
		self.gpu.offscreen(width, height)
	}
	/// The largest square offscreen texture the adapter accepts. A document
	/// taller than this is exported in several tiles.
	pub fn max_texture_dimension_2d(&self) -> u32 {
		self.gpu.device.limits().max_texture_dimension_2d
	}
	pub fn read_pixels(&self, texture: &wgpu::Texture) -> Result<Readback> {
		let (width, height, rgba) = self.gpu.read_pixels(texture)?;
		Ok(Readback {
			width,
			height,
			rgba,
		})
	}
	pub fn save_png(
		&self,
		texture: &wgpu::Texture,
		path: &std::path::Path,
	) -> Result<()> {
		let readback = self.read_pixels(texture)?;
		let size = tiny_skia::IntSize::from_wh(readback.width, readback.height)
			.context("Screenshot dimensions too large")?;
		let pixmap = tiny_skia::Pixmap::from_vec(readback.rgba, size)
			.context("Screenshot dimensions too large")?;
		pixmap.save_png(path)?;
		Ok(())
	}
	pub fn gpu_bytes(&self) -> u64 {
		(raster::ATLAS_SIZE * raster::ATLAS_SIZE) as u64
			+ self.raster.color_bytes()
			+ self.geometry.capacity_bytes()
			+ self.images.bytes()
	}
	pub fn clear_raster_cache(&mut self) {
		self.raster.reset_atlas(&self.gpu.queue);
	}
	/// Rasterization counters since this renderer was created.
	pub fn raster_stats(&self) -> RasterStats {
		self.raster.stats()
	}
	/// Rasterizes what the next screenful needs, so scrolling into it does not
	/// pay for the glyphs inside the frame the reader is waiting for. Returns
	/// whether the budget ran out first, which means another pass is worth
	/// scheduling while the reader stays put.
	pub fn prewarm(
		&mut self,
		snapshot: &markview_core::scene::LayoutSnapshot,
		view: &View<'_>,
		budget: Duration,
	) -> bool {
		// An atlas without room would evict the frame the reader is looking
		// at, so a prewarm declines rather than forcing that rebuild.
		if !self.raster.has_room() {
			return false;
		}
		let ahead = View {
			scroll: view.scroll + view.viewport().clip().h,
			selection: None,
			..view.clone()
		};
		self.prewarming = true;
		self.raster.begin_budget(budget);
		self.prepare(snapshot, &ahead, &[], &[]);
		self.prewarming = false;
		self.raster.finish_budget()
	}
}
fn intersect(a: Rect, b: Rect) -> Option<Rect> {
	let x = a.x.max(b.x);
	let y = a.y.max(b.y);
	let w = (a.x + a.w).min(b.x + b.w) - x;
	let h = (a.y + a.h).min(b.y + b.h) - y;
	(w > 0.0 && h > 0.0).then_some(Rect { x, y, w, h })
}
