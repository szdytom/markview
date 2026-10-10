//! Image semantics and immutable resource metadata. No I/O or GPU dependencies.
use std::{
	collections::HashMap,
	sync::{Arc, Mutex},
};

/// Prefix that marks an image source as a Mermaid diagram rather than a path
/// or URL. The scheduler strips it and renders the remainder as diagram source.
pub const MERMAID_SCHEME: &str = "mermaid:";

/// One Markdown or HTML image as the document sees it, before any I/O.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct ImageSpec {
	pub src: String,
	pub alt: String,
	pub title: String,
	/// Explicit `width` / `height` attributes; `None` keeps the aspect ratio.
	pub width: Option<u32>,
	pub height: Option<u32>,
}

/// The image source for a Mermaid fence's code, so a diagram travels through
/// the image scheduler like any other source.
pub fn mermaid_source(code: &str) -> String {
	format!("{MERMAID_SCHEME}{code}")
}

/// Decoded pixels in the one layout the renderer consumes.
#[derive(Clone, Debug)]
pub struct Pixels {
	pub width: u32,
	pub height: u32,
	/// Straight-alpha sRGB RGBA8.
	pub rgba: Arc<[u8]>,
}

/// What layout knows about a source right now. `version` changes on every
/// decode, so a layout cache key can detect new pixels without comparing them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImageInfo {
	pub version: u64,
	pub size: Option<(u32, u32)>,
	pub error: Option<String>,
}

/// Versioned pixels and the latest complete frame, shared with rendering.
#[derive(Default)]
pub struct ImagePixels {
	decoded: Mutex<HashMap<(String, u64), Arc<Pixels>>>,
	demand: Mutex<Option<FrameDemand>>,
	wake: Mutex<Option<crate::background::Wake>>,
}
#[derive(Debug)]
struct FrameDemand {
	generation: u64,
	entries: HashMap<String, ImageDemand>,
}
impl std::fmt::Debug for ImagePixels {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ImagePixels").finish_non_exhaustive()
	}
}
impl ImagePixels {
	pub fn set_wake(&self, wake: Option<crate::background::Wake>) {
		*self.wake.lock().unwrap() = wake;
	}
	pub fn get(&self, alias: &str, version: u64) -> Option<Arc<Pixels>> {
		let key = (alias.to_owned(), version);
		crate::sync::cache(&self.decoded, "Image pixels")
			.get(&key)
			.cloned()
	}
	/// Moves the old map out of the lock before releasing its allocations.
	pub fn replace(&self, pixels: HashMap<(String, u64), Arc<Pixels>>) {
		let retired = std::mem::replace(
			&mut *crate::sync::cache(&self.decoded, "Image pixels"),
			pixels,
		);
		drop(retired);
	}
	pub fn insert(&self, alias: String, version: u64, pixels: Arc<Pixels>) {
		let retired = crate::sync::cache(&self.decoded, "Image pixels")
			.insert((alias, version), pixels);
		drop(retired);
	}
	pub fn publish_demand(
		&self,
		generation: u64,
		entries: HashMap<String, ImageDemand>,
	) {
		let retired = crate::sync::cache(&self.demand, "Image demand").replace(
			FrameDemand {
				generation,
				entries,
			},
		);
		drop(retired);
		let wake = self.wake.lock().unwrap().clone();
		if let Some(wake) = wake {
			wake();
		}
	}
	pub fn take_demand(
		&self,
		generation: u64,
	) -> Option<HashMap<String, ImageDemand>> {
		let frame = crate::sync::cache(&self.demand, "Image demand").take()?;
		(frame.generation == generation).then_some(frame.entries)
	}
	/// A diagnostic copy; the scheduler consumes frames with `take_demand`.
	pub fn demand(&self, generation: u64) -> HashMap<String, ImageDemand> {
		crate::sync::cache(&self.demand, "Image demand")
			.as_ref()
			.filter(|frame| frame.generation == generation)
			.map(|frame| frame.entries.clone())
			.unwrap_or_default()
	}
}

/// One complete frame's requirements. A texture already on the GPU does not
/// need its CPU pixels fetched again after cache eviction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageDemand {
	pub size: (u32, u32),
	pub needs_pixels: bool,
}
impl ImageDemand {
	pub fn merge(&mut self, other: Self) {
		self.size.0 = self.size.0.max(other.size.0);
		self.size.1 = self.size.1.max(other.size.1);
		self.needs_pixels |= other.needs_pixels;
	}
}

/// The largest raster one dimension may be asked at. `MAX_RASTER_PIXELS`
/// bounds the pair, so a request is capped in both axes at once.
const MAX_RASTER_DIMENSION: u32 = 4000;

/// The most pixels one raster may hold.
const MAX_RASTER_PIXELS: u64 = 16_000_000;

/// The raster size to ask for a picture displayed `w` by `h` pixels large.
///
/// One uniform factor caps the request at the tightest of the per-dimension
/// limit and the pixel budget, so a demand past the cap comes back smaller in
/// both axes rather than honoured in one and refused in the other. Capping each
/// axis on its own squares a wide picture off, and a viewer that then fits that
/// raster shows the squash — and feeds the squared rect back as the next
/// demand, which is what locks the distortion in.
///
/// The capped axes land on the nearest pixel, not the one below: the raster
/// that arrives feeds the viewer's fit and through it the next demand, so
/// shaving a sub-pixel per round trip re-rasterizes forever and walks the
/// proportions away from the picture's.
pub fn raster_size(w: f32, h: f32) -> (u32, u32) {
	let (w, h) = (w.max(1.), h.max(1.));
	let factor = (MAX_RASTER_DIMENSION as f32 / w)
		.min(MAX_RASTER_DIMENSION as f32 / h)
		.min((MAX_RASTER_PIXELS as f32 / (w * h)).sqrt())
		.min(1.);
	(
		(w * factor).round().max(1.) as u32,
		(h * factor).round().max(1.) as u32,
	)
}

/// Immutable view of every image in one document revision.
#[derive(Clone, Debug, Default)]
pub struct ImageSnapshot {
	pub generation: u64,
	/// Metadata by alias, including the error for an unusable source.
	pub entries: HashMap<String, ImageInfo>,
	pub pixels: Arc<ImagePixels>,
}

impl ImageSnapshot {
	/// A diagnostic view containing only pixels belonging to this snapshot.
	pub fn decoded(&self) -> HashMap<String, Arc<Pixels>> {
		self.entries
			.iter()
			.filter_map(|(alias, info)| {
				self.pixels
					.get(alias, info.version)
					.map(|pixels| (alias.clone(), pixels))
			})
			.collect()
	}
}

impl ImageSpec {
	/// The box this image occupies in a paragraph `available` wide: the explicit
	/// attribute, the intrinsic size, or a 160x96 placeholder while unknown.
	/// Images never grow past the available measure.
	pub fn size(&self, info: Option<&ImageInfo>, available: f32) -> (f32, f32) {
		let (iw, ih) = info.and_then(|i| i.size).unwrap_or((160, 96));
		let (w, h) = match (self.width, self.height) {
			(Some(w), Some(h)) => (w as f32, h as f32),
			(Some(w), None) => {
				(w as f32, w as f32 * ih as f32 / iw.max(1) as f32)
			}
			(None, Some(h)) => {
				(h as f32 * iw as f32 / ih.max(1) as f32, h as f32)
			}
			_ => (iw as f32, ih as f32),
		};
		let scale = (available.max(1.) / w.max(1.)).min(1.);
		((w * scale).min(available.max(1.)), h * scale)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn snapshots_and_frames_cannot_cross_resource_generations() {
		let pixels = Arc::new(ImagePixels::default());
		let first = Arc::new(Pixels {
			width: 1,
			height: 1,
			rgba: Arc::from([0; 4]),
		});
		let retired = Arc::downgrade(&first);
		pixels.insert("same".into(), 1, first);
		let old = ImageSnapshot {
			generation: 1,
			entries: HashMap::from([(
				"same".into(),
				ImageInfo {
					version: 1,
					..Default::default()
				},
			)]),
			pixels: pixels.clone(),
		};
		assert_eq!(old.decoded().len(), 1);
		pixels.replace(HashMap::from([(
			("same".into(), 2),
			Arc::new(Pixels {
				width: 2,
				height: 1,
				rgba: Arc::from([0; 8]),
			}),
		)]));
		assert!(old.decoded().is_empty());
		assert!(retired.upgrade().is_none());
		assert_eq!(pixels.get("same", 2).unwrap().width, 2);
		let frame = |size| {
			HashMap::from([(
				"same".into(),
				ImageDemand {
					size,
					needs_pixels: true,
				},
			)])
		};
		pixels.publish_demand(1, frame((1, 1)));
		assert!(pixels.take_demand(2).is_none());
		pixels.publish_demand(2, frame((2, 1)));
		pixels.publish_demand(2, frame((4, 2)));
		assert_eq!(pixels.take_demand(2).unwrap()["same"].size, (4, 2));
		assert!(pixels.take_demand(2).is_none());
	}

	#[test]
	fn a_request_under_the_cap_is_untouched() {
		assert_eq!(raster_size(100., 60.), (100, 60));
		assert_eq!(raster_size(4000., 3000.), (4000, 3000));
	}

	#[test]
	fn a_request_past_the_cap_keeps_its_proportions() {
		assert_eq!(raster_size(6000., 4500.), (4000, 3000));
		assert_eq!(raster_size(12_000., 9000.), (4000, 3000));
	}

	#[test]
	fn the_capped_axes_land_on_the_nearest_pixel() {
		// `4501` scaled by two thirds is `3000.67`: the nearest pixel is
		// `3001`, and the one below would shave the raster's proportions a
		// little further every time the viewer feeds it back.
		assert_eq!(raster_size(6000., 4501.), (4000, 3001));
		assert_eq!(raster_size(6000., 4498.), (4000, 2999));
	}

	#[test]
	fn the_cap_bounds_both_axes_and_the_total_at_once() {
		for (w, h) in [
			(10_000., 10_000.),
			(8192., 1024.),
			(20_000., 100.),
			(1., 1.),
			(0., 0.),
		] {
			let (raster_w, raster_h) = raster_size(w, h);
			assert!(
				raster_w <= MAX_RASTER_DIMENSION
					&& raster_h <= MAX_RASTER_DIMENSION,
				"{raster_w}x{raster_h} from {w}x{h}"
			);
			assert!(
				u64::from(raster_w) * u64::from(raster_h) <= MAX_RASTER_PIXELS,
				"{raster_w}x{raster_h} from {w}x{h}"
			);
			let asked = w.max(1.) / h.max(1.);
			let capped = raster_w as f32 / raster_h as f32;
			assert!((asked - capped).abs() < 0.01, "{capped} from {asked}");
		}
	}
}
