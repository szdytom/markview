//! The icon a running window advertises to the desktop environment.
//!
//! Every platform embeds the same committed byte assets, so the reader never
//! depends on a system theme to find its own icon.
use std::sync::OnceLock;
use winit::window::Icon;

#[cfg(target_os = "windows")]
const WINDOWS_ICON: &[u8] = include_bytes!("../../assets/icons/markview.ico");

#[cfg(not(target_os = "windows"))]
const LINUX_ICON: &[u8] = include_bytes!("../../assets/icons/markview-128.png");

/// Returns the window icon, decoding the embedded bytes at most once.
///
/// winit wants an owned [`Icon`], so the decoded RGBA buffer is cached and a
/// fresh icon is built from it.
pub(super) fn window_icon() -> Option<Icon> {
	type Pixels = (u32, u32, Vec<u8>);
	static PIXELS: OnceLock<Option<Pixels>> = OnceLock::new();
	let (width, height, data) = PIXELS.get_or_init(decode).as_ref()?;
	Icon::from_rgba(data.clone(), *width, *height).ok()
}

#[cfg(target_os = "windows")]
fn decode() -> Option<(u32, u32, Vec<u8>)> {
	let dir = ico::IconDir::read(std::io::Cursor::new(WINDOWS_ICON)).ok()?;
	let entry = dir.entries().iter().max_by_key(|e| e.width())?;
	let image = entry.decode().ok()?;
	Some((image.width(), image.height(), image.rgba_data().to_vec()))
}

#[cfg(not(target_os = "windows"))]
fn decode() -> Option<(u32, u32, Vec<u8>)> {
	let image = image::load_from_memory(LINUX_ICON).ok()?.into_rgba8();
	let (width, height) = image.dimensions();
	Some((width, height, image.into_raw()))
}

#[cfg(test)]
mod tests {
	use super::*;

	/// The embedded asset must decode without a window server, because every
	/// archive ships it.
	#[test]
	fn embedded_icon_decodes_to_a_square_rgba_image() {
		let (width, height, data) = decode().expect("embedded icon decodes");
		assert_eq!(width, height);
		assert!(width >= 32);
		assert_eq!(data.len(), (width * height * 4) as usize);
		assert!(window_icon().is_some());
	}
}
