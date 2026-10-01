//! Text faces supplied by the browser host; only KaTeX remains embedded.

use markview_core::fonts::{FontConfig, is_font};
use parley::fontique::Blob;
use std::{
	cell::RefCell,
	sync::{
		Arc,
		atomic::{AtomicU32, Ordering},
	},
};

const TAG_BASE: u64 = 0x6d76_7765_0000_0000;

thread_local! {
	static CONFIG: RefCell<FontConfig> = RefCell::new(FontConfig::from_faces(TAG_BASE, Vec::new()));
}

/// Validates the entire set before replacing it, so failed startup can retry.
pub(crate) fn build(mut faces: Vec<Vec<u8>>) -> Result<FontConfig, String> {
	for (index, data) in faces.iter_mut().enumerate() {
		if matches!(data.get(..4), Some(b"wOFF" | b"wOF2")) {
			*data = decode(data).map_err(|error| {
				format!("invalid host font at index {index}: {error}")
			})?;
		}
		if !is_font(data) {
			return Err(format!(
				"invalid host font at index {index}: expected an OpenType or TrueType font"
			));
		}
	}
	// The collection cache compares tags alone; each replacement needs its
	// own identity, while readers clone the installed configuration.
	static NEXT_TAG: AtomicU32 = AtomicU32::new(1);
	let tag = TAG_BASE | u64::from(NEXT_TAG.fetch_add(1, Ordering::Relaxed));
	Ok(FontConfig::from_faces(
		tag,
		faces
			.into_iter()
			.map(|data| Blob::new(Arc::new(data)))
			.collect(),
	))
}

#[cfg(feature = "woff")]
fn decode(data: &[u8]) -> Result<Vec<u8>, String> {
	let result = if data.starts_with(b"wOFF") {
		wuff::decompress_woff1(data)
	} else {
		wuff::decompress_woff2(data)
	};
	result.map_err(|error| format!("WOFF decoding failed: {error:?}"))
}

#[cfg(not(feature = "woff"))]
fn decode(_: &[u8]) -> Result<Vec<u8>, String> {
	Err("WOFF/WOFF2 requires a WASM build with the `woff` feature".into())
}

pub(crate) fn install(faces: Vec<Vec<u8>>) -> Result<(), String> {
	let config = build(faces)?;
	CONFIG.with(|current| *current.borrow_mut() = config);
	Ok(())
}

/// A shared snapshot of the faces installed during `init()`.
pub(crate) fn config() -> FontConfig {
	CONFIG.with(|config| config.borrow().clone())
}

#[cfg(test)]
mod tests {
	use super::*;
	use markview_core::fonts::families;

	#[test]
	fn font_formats_and_independent_sets() {
		let read = |name: &str| {
			std::fs::read(
				std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
					.join("tests/fonts")
					.join(name),
			)
			.unwrap()
		};
		let collection = build(vec![read("Noto-subset.ttc")]).unwrap();
		assert_eq!(families(&collection, false).len(), 2);
		for name in ["NotoSerif-Regular-subset", "NotoColorEmoji-subset"] {
			for format in ["woff", "woff2"] {
				let result = build(vec![read(&format!("{name}.{format}"))]);
				#[cfg(feature = "woff")]
				{
					let set = result.unwrap();
					assert_ne!(set, collection);
					assert_eq!(families(&set, false).len(), 1);
				}
				#[cfg(not(feature = "woff"))]
				assert!(result.unwrap_err().contains("`woff` feature"));
			}
		}
		for signature in [b"wOFF", b"wOF2"] {
			assert!(build(vec![signature.to_vec()]).is_err());
		}
	}

	#[test]
	fn host_faces_replace_atomically_and_keep_existing_snapshots() {
		let read = |name: &str| {
			std::fs::read(
				std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
					.join("../markview-core/tests/fonts")
					.join(name),
			)
			.unwrap()
		};
		install(vec![read("NotoSerif-Regular-subset.otf")]).unwrap();
		let serif = config();
		assert!(serif.ignore_system_fonts);
		assert_eq!(families(&serif, false).as_ref(), [Arc::from("Noto Serif")]);
		assert_eq!(config(), serif);

		assert!(
			install(vec![read("NotoSans-Regular-subset.otf"), b"404".to_vec()])
				.is_err()
		);
		assert_eq!(config(), serif);
		install(vec![read("NotoSans-Regular-subset.otf")]).unwrap();
		assert_ne!(config(), serif);
		assert_eq!(
			families(&config(), false).as_ref(),
			[Arc::from("Noto Sans")]
		);
		assert_eq!(families(&serif, false).as_ref(), [Arc::from("Noto Serif")]);

		install(Vec::new()).unwrap();
		assert!(families(&config(), false).is_empty());
	}
}
