//! A clipboard another application owns, for tests that compete over it.

/// A desktop clipboard written by someone other than the reader, so a test can
/// watch the reader yield an older deferred copy to a newer one.
///
/// Android has no desktop clipboard, so every access there fails instead of
/// keeping a native backend compiled for tests that cannot run.
pub struct ExternalClipboard(
	#[cfg(not(target_os = "android"))] arboard::Clipboard,
);

impl ExternalClipboard {
	pub fn new() -> anyhow::Result<Self> {
		#[cfg(not(target_os = "android"))]
		{
			Ok(Self(arboard::Clipboard::new()?))
		}
		#[cfg(target_os = "android")]
		{
			unsupported()
		}
	}

	/// Writes a one-pixel opaque image whose red channel is `red`.
	pub fn set_pixel(&mut self, red: u8) -> anyhow::Result<()> {
		#[cfg(not(target_os = "android"))]
		{
			self.0.set_image(arboard::ImageData {
				width: 1,
				height: 1,
				bytes: vec![red, 0, 0, u8::MAX].into(),
			})?;
			Ok(())
		}
		#[cfg(target_os = "android")]
		{
			let _ = red;
			unsupported()
		}
	}

	/// The raw RGBA bytes of the image the clipboard holds.
	pub fn pixel(&mut self) -> anyhow::Result<Vec<u8>> {
		#[cfg(not(target_os = "android"))]
		{
			Ok(self.0.get_image()?.bytes.to_vec())
		}
		#[cfg(target_os = "android")]
		{
			unsupported()
		}
	}

	pub fn set_html(
		&mut self,
		html: &str,
		alternative: Option<&str>,
	) -> anyhow::Result<()> {
		#[cfg(not(target_os = "android"))]
		{
			Ok(self.0.set_html(html, alternative)?)
		}
		#[cfg(target_os = "android")]
		{
			let _ = (html, alternative);
			unsupported()
		}
	}

	/// The HTML flavour the clipboard holds.
	pub fn html(&mut self) -> anyhow::Result<String> {
		#[cfg(not(target_os = "android"))]
		{
			Ok(self.0.get().html()?)
		}
		#[cfg(target_os = "android")]
		{
			unsupported()
		}
	}

	/// The plain-text flavour the clipboard holds.
	pub fn text(&mut self) -> anyhow::Result<String> {
		#[cfg(not(target_os = "android"))]
		{
			Ok(self.0.get_text()?)
		}
		#[cfg(target_os = "android")]
		{
			unsupported()
		}
	}
}

#[cfg(target_os = "android")]
fn unsupported<T>() -> anyhow::Result<T> {
	anyhow::bail!("Android has no desktop clipboard")
}
