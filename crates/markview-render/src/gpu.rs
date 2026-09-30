//! Device, surface recovery and offscreen readback.
use crate::{FrameStatus, SurfaceSource};
use anyhow::{Context, Result, bail};
use std::{
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
	time::Duration,
};

/// The surface being drawn into, and the host that can rebuild it.
///
/// A lost surface is only recoverable through the host that made it, so the
/// two are kept together: neither is of any use without the other.
struct Surface {
	source: Box<dyn SurfaceSource>,
	handle: wgpu::Surface<'static>,
	config: wgpu::SurfaceConfiguration,
}
impl Surface {
	fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
		if width == 0 || height == 0 {
			return;
		}
		self.config.width = width;
		self.config.height = height;
		self.handle.configure(device, &self.config);
	}
}

/// The limits the device is created with.
///
/// WebGL2 has no compute or storage buffers at all, so the desktop defaults
/// fail the device request outright: the Wasm build links and then never
/// draws. The renderer uses neither, so the downlevel set costs it nothing —
/// but only the backend that needs it gets it, because the other backends
/// keep the texture-size limit that export tiling reads.
fn device_limits(backend: wgpu::Backend) -> wgpu::Limits {
	if backend == wgpu::Backend::Gl {
		wgpu::Limits::downlevel_webgl2_defaults()
	} else {
		wgpu::Limits::default()
	}
}

pub(super) struct Gpu {
	instance: wgpu::Instance,
	surface: Option<Surface>,
	pub(super) device: wgpu::Device,
	pub(super) queue: wgpu::Queue,
	pub(super) format: wgpu::TextureFormat,
	pub adapter_name: String,
	pub backend: wgpu::Backend,
	lost: Arc<AtomicBool>,
}
impl Gpu {
	pub fn acquire(&mut self) -> Result<FrameStatus> {
		let surface =
			self.surface.as_mut().context("No surface to draw into")?;
		// Read before the match: the recovering arms replace the handle this
		// borrow points at, and all of them want the size the host reports.
		let (width, height) = surface.source.size();
		Ok(match surface.handle.get_current_texture() {
			wgpu::CurrentSurfaceTexture::Success(frame) => {
				FrameStatus::Ready(frame, false)
			}
			wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
				FrameStatus::Ready(frame, true)
			}
			wgpu::CurrentSurfaceTexture::Lost => {
				surface.handle =
					surface.source.create_surface(&self.instance)?;
				surface.resize(&self.device, width, height);
				FrameStatus::Retry(Duration::from_millis(16))
			}
			wgpu::CurrentSurfaceTexture::Outdated => {
				surface.resize(&self.device, width, height);
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

	pub async fn new(
		surface_source: Option<Box<dyn SurfaceSource>>,
	) -> Result<Self> {
		let descriptor = surface_source.as_ref().map_or_else(
			wgpu::InstanceDescriptor::new_without_display_handle_from_env,
			|source| source.instance_descriptor(),
		);
		let instance = wgpu::Instance::new(descriptor);
		let handle = surface_source
			.as_ref()
			.map(|source| source.create_surface(&instance))
			.transpose()?;
		let adapter = instance
			.request_adapter(&wgpu::RequestAdapterOptions {
				power_preference: wgpu::PowerPreference::LowPower,
				compatible_surface: handle.as_ref(),
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
				required_limits: device_limits(info.backend),
				memory_hints: wgpu::MemoryHints::MemoryUsage,
				..Default::default()
			})
			.await?;
		let lost = Arc::new(AtomicBool::new(false));
		let flag = lost.clone();
		device.set_device_lost_callback(move |_, _| {
			flag.store(true, Ordering::Relaxed);
		});
		// A source either yields a handle or has already failed, so the two
		// arrive together and there is no state with one but not the other.
		let surface = surface_source.zip(handle).map(|(source, handle)| {
			let caps = handle.get_capabilities(&adapter);
			let format = caps
				.formats
				.iter()
				.find(|f| f.is_srgb())
				.copied()
				.unwrap_or(caps.formats[0]);
			let (width, height) = source.size();
			let config = wgpu::SurfaceConfiguration {
				usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
				format,
				width: width.max(1),
				height: height.max(1),
				present_mode: wgpu::PresentMode::AutoVsync,
				desired_maximum_frame_latency: 1,
				alpha_mode: caps.alpha_modes[0],
				view_formats: vec![],
			};
			handle.configure(&device, &config);
			Surface {
				source,
				handle,
				config,
			}
		});
		let format = surface
			.as_ref()
			.map_or(wgpu::TextureFormat::Rgba8UnormSrgb, |s| s.config.format);
		Ok(Self {
			instance,
			surface,
			device,
			queue,
			format,
			adapter_name,
			backend: info.backend,
			lost,
		})
	}
	pub fn resize(&mut self, width: u32, height: u32) {
		if let Some(surface) = &mut self.surface {
			surface.resize(&self.device, width, height);
		}
	}

	/// Waits for submitted work to complete.
	#[cfg(feature = "readback")]
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
	#[cfg(feature = "readback")]
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

#[cfg(test)]
mod tests {
	use super::device_limits;

	/// A WebGL2 device has no compute or storage buffers, so the desktop
	/// defaults make `request_device` fail before a browser can draw anything.
	/// The build succeeding says nothing about this: it is a runtime failure,
	/// and only the requested limits decide it.
	#[test]
	fn the_browser_backend_asks_only_for_what_webgl2_has() {
		let limits = device_limits(wgpu::Backend::Gl);
		assert_eq!(limits.max_storage_buffers_per_shader_stage, 0);
		assert_eq!(limits.max_compute_invocations_per_workgroup, 0);
		assert!(limits.max_texture_dimension_2d >= crate::raster::ATLAS_SIZE);
	}

	/// Export tiling reads the device's texture limit, so giving the desktop
	/// backends the WebGL one would cut every page into small tiles.
	#[test]
	fn the_desktop_backends_keep_their_own_texture_limit() {
		assert!(
			device_limits(wgpu::Backend::Vulkan).max_texture_dimension_2d
				> wgpu::Limits::downlevel_webgl2_defaults()
					.max_texture_dimension_2d
		);
	}
}
