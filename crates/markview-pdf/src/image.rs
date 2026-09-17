//! Decoded images, embedded once per export.
use krilla::image::Image;
use markview_core::image::ImageSnapshot;
use std::collections::HashMap;

pub struct Images<'a> {
	snapshot: &'a ImageSnapshot,
	cache: HashMap<(String, u64), Option<Image>>,
}

impl<'a> Images<'a> {
	pub fn new(snapshot: &'a ImageSnapshot) -> Self {
		Self {
			snapshot,
			cache: HashMap::new(),
		}
	}

	/// The image for a source at a version, or `None` while its pixels are
	/// missing, which is what an unavailable image already looks like on
	/// screen.
	pub fn get(&mut self, src: &str, version: u64) -> Option<Image> {
		let key = (src.to_owned(), version);
		if let Some(image) = self.cache.get(&key) {
			return image.clone();
		}
		let image = self
			.snapshot
			.pixels
			.decoded
			.lock()
			.ok()
			.and_then(|decoded| decoded.get(src).cloned())
			.map(|pixels| {
				Image::from_rgba8(
					pixels.rgba.to_vec(),
					pixels.width,
					pixels.height,
				)
			});
		self.cache.insert(key, image.clone());
		image
	}
}
