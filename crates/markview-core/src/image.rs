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
	/// Semantic reading text in place of the placeholder or `alt` when the
	/// image reads as something other than its caption. A Mermaid diagram
	/// carries its source here so selection and copying keep it while `alt`
	/// and `title` stay empty and draw no caption.
	pub reading: Option<String>,
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

/// Pixels and per-frame demand, shared by the loader, the layout snapshots and
/// the renderer. Residency is independent of retained layout snapshots.
#[derive(Debug, Default)]
pub struct ImagePixels {
	/// Decoded pixels by every alias of a source.
	pub decoded: Mutex<HashMap<String, Arc<Pixels>>>,
	/// Display size in physical pixels requested by the last painted frame,
	/// by alias. Vector images are rasterized at this size.
	pub demand: Mutex<HashMap<String, ImageDemand>>,
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

/// Immutable view of every image in one document revision.
#[derive(Clone, Debug, Default)]
pub struct ImageSnapshot {
	/// Metadata by alias, including the error for an unusable source.
	pub entries: HashMap<String, ImageInfo>,
	pub pixels: Arc<ImagePixels>,
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
