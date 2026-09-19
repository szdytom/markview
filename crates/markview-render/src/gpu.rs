//! Device, surface recovery and offscreen readback.
use crate::FrameStatus;
use anyhow::{Context, Result, bail};
use std::{
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
	time::Duration,
};
use winit::window::Window;
pub(super) struct Gpu {
	instance: wgpu::Instance,
	surface: Option<wgpu::Surface<'static>>,
	config: Option<wgpu::SurfaceConfiguration>,
	pub(super) device: wgpu::Device,
	pub(super) queue: wgpu::Queue,
	pub(super) format: wgpu::TextureFormat,
	pub adapter_name: String,
	lost: Arc<AtomicBool>,
}
impl Gpu {
	pub fn acquire(&mut self, window: Arc<Window>) -> Result<FrameStatus> {
		let size = window.inner_size();
		let surface = self.surface.as_ref().context("No window surface")?;
		Ok(match surface.get_current_texture() {
			wgpu::CurrentSurfaceTexture::Success(frame) => {
				FrameStatus::Ready(frame, false)
			}
			wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
				FrameStatus::Ready(frame, true)
			}
			wgpu::CurrentSurfaceTexture::Lost => {
				self.surface = Some(self.instance.create_surface(window)?);
				self.resize(size.width, size.height);
				FrameStatus::Retry(Duration::from_millis(16))
			}
			wgpu::CurrentSurfaceTexture::Outdated => {
				self.resize(size.width, size.height);
				FrameStatus::Retry(Duration::from_millis(16))
			}
			wgpu::CurrentSurfaceTexture::Timeout => {
				FrameStatus::Retry(Duration::from_millis(30))
			}
			wgpu::CurrentSurfaceTexture::Occluded => FrameStatus::Occluded,
			wgpu::CurrentSurfaceTexture::Validation => {
				bail!("GPU surface validation failed")
			}
		})
	}
	pub fn on_device_lost(&self, callback: impl Fn() + Send + 'static) {
		let flag = self.lost.clone();
		self.device.set_device_lost_callback(move |reason, _| {
			if reason != wgpu::DeviceLostReason::Destroyed {
				flag.store(true, Ordering::Relaxed);
				callback();
			}
		});
	}

	pub async fn new(window: Option<Arc<Window>>) -> Result<Self> {
		let descriptor = match &window {
			Some(w) => {
				wgpu::InstanceDescriptor::new_with_display_handle_from_env(
					Box::new(w.clone()),
				)
			}
			None => {
				wgpu::InstanceDescriptor::new_without_display_handle_from_env()
			}
		};
		let instance = wgpu::Instance::new(descriptor);
		let surface = window
			.as_ref()
			.map(|w| instance.create_surface(w.clone()))
			.transpose()?;
		let adapter = instance
			.request_adapter(&wgpu::RequestAdapterOptions {
				power_preference: wgpu::PowerPreference::LowPower,
				compatible_surface: surface.as_ref(),
				force_fallback_adapter: false,
			})
			.await
			.context("No compatible GPU adapter (try WGPU_BACKEND=gl)")?;
		let info = adapter.get_info();
		let adapter_name = format!(
			"{} ({:?}, {:?})",
			info.name, info.backend, info.device_type
		);
		let (device, queue) = adapter
			.request_device(&wgpu::DeviceDescriptor {
				label: Some("Markview"),
				memory_hints: wgpu::MemoryHints::MemoryUsage,
				..Default::default()
			})
			.await?;
		let lost = Arc::new(AtomicBool::new(false));
		let flag = lost.clone();
		device.set_device_lost_callback(move |_, _| {
			flag.store(true, Ordering::Relaxed);
		});
		let config = surface.as_ref().map(|s| {
			let size = window.as_ref().unwrap().inner_size();
			let caps = s.get_capabilities(&adapter);
			let format = caps
				.formats
				.iter()
				.find(|f| f.is_srgb())
				.copied()
				.unwrap_or(caps.formats[0]);
			wgpu::SurfaceConfiguration {
				usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
				format,
				width: size.width.max(1),
				height: size.height.max(1),
				present_mode: wgpu::PresentMode::AutoVsync,
				desired_maximum_frame_latency: 1,
				alpha_mode: caps.alpha_modes[0],
				view_formats: vec![],
			}
		});
		if let (Some(surface), Some(config)) = (&surface, &config) {
			surface.configure(&device, config);
		}
		let format = config
			.as_ref()
			.map_or(wgpu::TextureFormat::Rgba8UnormSrgb, |c| c.format);
		Ok(Self {
			instance,
			surface,
			config,
			device,
			queue,
			format,
			adapter_name,
			lost,
		})
	}
	pub fn resize(&mut self, width: u32, height: u32) {
		if width == 0 || height == 0 {
			return;
		}
		if let (Some(surface), Some(config)) = (&self.surface, &mut self.config)
		{
			config.width = width;
			config.height = height;
			surface.configure(&self.device, config);
		}
	}

	pub fn wait(&self, index: Option<wgpu::SubmissionIndex>) -> Result<()> {
		self.device.poll(wgpu::PollType::Wait {
			submission_index: index,
			timeout: Some(Duration::from_secs(10)),
		})?;
		Ok(())
	}
	pub fn offscreen(&self, width: u32, height: u32) -> wgpu::Texture {
		self.device.create_texture(&wgpu::TextureDescriptor {
			label: Some("headless validation"),
			size: wgpu::Extent3d {
				width,
				height,
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: self.format,
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT
				| wgpu::TextureUsages::COPY_SRC,
			view_formats: &[],
		})
	}
	/// Reads a texture back as tightly packed, non-premultiplied sRGB RGBA8.
	pub(super) fn read_pixels(
		&self,
		texture: &wgpu::Texture,
	) -> Result<(u32, u32, Vec<u8>)> {
		let w = texture.width();
		let h = texture.height();
		let pitch = (w * 4).div_ceil(256) * 256;
		let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("screenshot readback"),
			size: (pitch * h) as u64,
			usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
			mapped_at_creation: false,
		});
		let mut encoder =
			self.device.create_command_encoder(&Default::default());
		encoder.copy_texture_to_buffer(
			texture.as_image_copy(),
			wgpu::TexelCopyBufferInfo {
				buffer: &buffer,
				layout: wgpu::TexelCopyBufferLayout {
					offset: 0,
					bytes_per_row: Some(pitch),
					rows_per_image: Some(h),
				},
			},
			texture.size(),
		);
		self.queue.submit([encoder.finish()]);
		let (tx, rx) = std::sync::mpsc::channel();
		buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
			let _ = tx.send(r);
		});
		self.wait(None)?;
		rx.recv()??;
		let mapped = buffer.slice(..).get_mapped_range();
		let mut rgba = vec![0u8; (w as usize) * (h as usize) * 4];
		for (src, dst) in mapped
			.chunks_exact(pitch as usize)
			.zip(rgba.chunks_exact_mut(w as usize * 4))
		{
			dst.copy_from_slice(&src[..w as usize * 4]);
		}
		if matches!(
			self.format,
			wgpu::TextureFormat::Bgra8UnormSrgb
				| wgpu::TextureFormat::Bgra8Unorm
		) {
			for p in rgba.as_chunks_mut::<4>().0 {
				p.swap(0, 2);
			}
		}
		drop(mapped);
		buffer.unmap();
		Ok((w, h, rgba))
	}
}
