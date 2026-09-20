//! Raster and vector decoding with pixel limits.
use anyhow::{Context, Result, bail};
use image::{AnimationDecoder, ImageDecoder};
use markview_core::image::Pixels;
use std::{io::Cursor, sync::Arc};
const MAX_PIXELS: u64 = 16_000_000;

fn dimensions(w: u32, h: u32) -> Result<()> {
	if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
		bail!("Image exceeds 16 million pixels or has invalid dimensions");
	}
	Ok(())
}

pub(super) struct Decoded {
	pub(super) pixels: Arc<Pixels>,
	pub(super) intrinsic: (u32, u32),
	pub(super) svg: bool,
}

/// The largest PNG-compressed entry of an ICO. Windows renders entries that
/// are not 32-bit RGBA, while `image`'s ICO decoder rejects them.
fn ico_png(bytes: &[u8]) -> Option<&[u8]> {
	const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
	let count = u16::from_le_bytes([*bytes.get(4)?, *bytes.get(5)?]) as usize;
	let mut best: Option<(u32, &[u8])> = None;
	for i in 0..count {
		let entry = bytes.get(6 + i * 16..6 + i * 16 + 16)?;
		let size = u32::from_le_bytes(entry[8..12].try_into().ok()?) as usize;
		let offset =
			u32::from_le_bytes(entry[12..16].try_into().ok()?) as usize;
		let data = bytes.get(offset..offset.checked_add(size)?)?;
		if !data.starts_with(&SIGNATURE) {
			continue;
		}
		let side = |v: u8| if v == 0 { 256 } else { u32::from(v) };
		let area = side(entry[0]) * side(entry[1]);
		if best.is_none_or(|(best, _)| area > best) {
			best = Some((area, data));
		}
	}
	best.map(|(_, data)| data)
}

/// System fonts are shared: loading them is expensive and SVGs without text
/// do not need them at all.
fn svg_fonts() -> Arc<resvg::usvg::fontdb::Database> {
	static FONTS: std::sync::OnceLock<Arc<resvg::usvg::fontdb::Database>> =
		std::sync::OnceLock::new();
	FONTS
		.get_or_init(|| {
			let mut db = resvg::usvg::fontdb::Database::new();
			db.load_system_fonts();
			Arc::new(db)
		})
		.clone()
}

fn has_svg_text(bytes: &[u8]) -> bool {
	[b"<text".as_slice(), b"<tspan", b"<textPath"]
		.iter()
		.any(|tag| {
			bytes
				.windows(tag.len())
				.any(|w| w.eq_ignore_ascii_case(tag))
		})
}

/// Decodes one image. `fonts` is the reader's own face set and the theme's
/// family list, for an SVG that carries text; `None` resolves fonts the way
/// an SVG file outside the reader does.
pub(super) fn decode(
	bytes: &[u8],
	target: Option<(u32, u32)>,
	fonts: Option<(&std::sync::Arc<super::fonts::DiagramFonts>, &str)>,
) -> Result<Decoded> {
	let format = image::guess_format(bytes).ok();
	if format.is_none() {
		let mut options = resvg::usvg::Options::default();
		options.image_href_resolver.resolve_string = Box::new(|_, _| None);
		if has_svg_text(bytes) {
			match fonts {
				Some((fonts, families)) => fonts.apply(&mut options, families),
				None => options.fontdb = svg_fonts(),
			}
		}
		let tree = resvg::usvg::Tree::from_data(bytes, &options)
			.context("Unsupported or invalid image/SVG")?;
		let intrinsic = tree.size().to_int_size();
		dimensions(intrinsic.width(), intrinsic.height())?;
		let (w, h) = target.unwrap_or((intrinsic.width(), intrinsic.height()));
		dimensions(w, h)?;
		let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)
			.context("Cannot allocate SVG")?;
		resvg::render(
			&tree,
			resvg::tiny_skia::Transform::from_scale(
				w as f32 / tree.size().width(),
				h as f32 / tree.size().height(),
			),
			&mut pixmap.as_mut(),
		);
		// tiny-skia stores premultiplied alpha; the image pipeline uses straight alpha.
		let mut rgba = pixmap.take();
		for p in rgba.as_chunks_mut::<4>().0 {
			if p[3] > 0 {
				for i in 0..3 {
					p[i] = ((u32::from(p[i]) * 255 + u32::from(p[3]) / 2)
						/ u32::from(p[3]))
					.min(255) as u8;
				}
			}
		}
		return Ok(Decoded {
			pixels: Arc::new(Pixels {
				width: w,
				height: h,
				rgba: rgba.into(),
			}),
			intrinsic: (intrinsic.width(), intrinsic.height()),
			svg: true,
		});
	}
	let format = format.unwrap();
	let mut reader =
		image::ImageReader::with_format(Cursor::new(bytes), format);
	let mut limits = image::Limits::default();
	limits.max_alloc = Some(128 * 1024 * 1024);
	reader.limits(limits.clone());
	let mut decoder = reader.into_decoder()?;
	let (w, h) = decoder.dimensions();
	dimensions(w, h)?;
	let orientation = decoder.orientation()?;
	let mut bitmap = match format {
		image::ImageFormat::Gif => {
			let mut d =
				image::codecs::gif::GifDecoder::new(Cursor::new(bytes))?;
			d.set_limits(limits)?;
			image::DynamicImage::ImageRgba8(
				d.into_frames().next().context("Empty GIF")??.into_buffer(),
			)
		}
		image::ImageFormat::Png => {
			let d = image::codecs::png::PngDecoder::with_limits(
				Cursor::new(bytes),
				limits,
			)?;
			if d.is_apng()? {
				image::DynamicImage::ImageRgba8(
					d.apng()?
						.into_frames()
						.next()
						.context("Empty APNG")??
						.into_buffer(),
				)
			} else {
				image::DynamicImage::from_decoder(decoder)?
			}
		}
		image::ImageFormat::WebP => {
			let mut d =
				image::codecs::webp::WebPDecoder::new(Cursor::new(bytes))?;
			d.set_limits(limits)?;
			if d.has_animation() {
				image::DynamicImage::ImageRgba8(
					d.into_frames()
						.next()
						.context("Empty WebP")??
						.into_buffer(),
				)
			} else {
				image::DynamicImage::from_decoder(decoder)?
			}
		}
		image::ImageFormat::Ico => match ico_png(bytes) {
			Some(png) => image::DynamicImage::from_decoder(
				image::codecs::png::PngDecoder::with_limits(
					Cursor::new(png),
					limits,
				)?,
			)?,
			None => image::DynamicImage::from_decoder(decoder)?,
		},
		_ => image::DynamicImage::from_decoder(decoder)?,
	};
	bitmap.apply_orientation(orientation);
	let intrinsic = (bitmap.width(), bitmap.height());
	// Stay within the baseline WebGPU 8192-pixel texture dimension.
	if bitmap.width() > 8192 || bitmap.height() > 8192 {
		bitmap =
			bitmap.resize(8192, 8192, image::imageops::FilterType::Lanczos3);
	}
	let rgba = bitmap.into_rgba8();
	Ok(Decoded {
		intrinsic,
		svg: false,
		pixels: Arc::new(Pixels {
			width: rgba.width(),
			height: rgba.height(),
			rgba: rgba.into_raw().into(),
		}),
	})
}

#[cfg(test)]
mod tests {
	#[test]
	fn rejects_invalid_pixel_dimensions() {
		assert!(super::dimensions(0, 10).is_err());
		assert!(super::dimensions(5000, 4000).is_err());
		assert!(super::dimensions(4000, 4000).is_ok());
	}
}
