//! Raster and vector decoding with pixel limits.
use anyhow::{Context, Result, bail};
use image::{AnimationDecoder, ImageDecoder};
use markview_core::image::Pixels;
use std::{collections::VecDeque, io::Cursor, sync::Arc};
const MAX_PIXELS: u64 = 16_000_000;
const SVG_FONT_CACHE_CAP: usize = 4;

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
fn svg_fonts(
	generic_families: &[(String, Vec<String>)],
) -> Arc<resvg::usvg::fontdb::Database> {
	type Cache = std::sync::Mutex<
		VecDeque<(
			Vec<(String, Vec<String>)>,
			Arc<resvg::usvg::fontdb::Database>,
		)>,
	>;
	static SYSTEM: std::sync::OnceLock<Arc<resvg::usvg::fontdb::Database>> =
		std::sync::OnceLock::new();
	static CACHE: std::sync::OnceLock<Cache> = std::sync::OnceLock::new();
	let fonts = CACHE.get_or_init(|| std::sync::Mutex::new(VecDeque::new()));
	let mut fonts = fonts.lock().unwrap();
	if let Some((_, database)) =
		fonts.iter().find(|(key, _)| key == generic_families)
	{
		return database.clone();
	}
	let system = SYSTEM.get_or_init(|| {
		let mut db = resvg::usvg::fontdb::Database::new();
		#[cfg(not(test))]
		db.load_system_fonts();
		#[cfg(test)]
		for directory in crate::test_support::fonts().directories {
			db.load_fonts_dir(directory);
		}
		Arc::new(db)
	});
	let mut database = (**system).clone();
	for (generic, candidates) in generic_families {
		if let Some(family) = candidates
			.iter()
			.find_map(|candidate| resolve_svg_family(&database, candidate))
		{
			super::fonts::set_generic(&mut database, generic, family.clone());
		}
	}
	let database = Arc::new(database);
	fonts.push_back((generic_families.to_vec(), database.clone()));
	if fonts.len() > SVG_FONT_CACHE_CAP {
		fonts.pop_front();
	}
	database
}

/// Resolves a configured SVG candidate to the database's canonical family
/// name, including a candidate that is itself a generic family.
fn resolve_svg_family(
	database: &resvg::usvg::fontdb::Database,
	candidate: &str,
) -> Option<String> {
	use resvg::usvg::fontdb::{FaceInfo, Family};
	let candidate = if candidate.eq_ignore_ascii_case("serif") {
		database.family_name(&Family::Serif)
	} else if candidate.eq_ignore_ascii_case("sans-serif") {
		database.family_name(&Family::SansSerif)
	} else if candidate.eq_ignore_ascii_case("monospace") {
		database.family_name(&Family::Monospace)
	} else if candidate.eq_ignore_ascii_case("cursive") {
		database.family_name(&Family::Cursive)
	} else if candidate.eq_ignore_ascii_case("fantasy") {
		database.family_name(&Family::Fantasy)
	} else {
		candidate
	};
	database.faces().find_map(|face: &FaceInfo| {
		face.families
			.iter()
			.find(|(name, _)| name.eq_ignore_ascii_case(candidate))
			.map(|(name, _)| name.clone())
	})
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
	generic_families: &[(String, Vec<String>)],
) -> Result<Decoded> {
	let format = image::guess_format(bytes).ok();
	if format.is_none() {
		let mut options = resvg::usvg::Options::default();
		options.image_href_resolver.resolve_string = Box::new(|_, _| None);
		if has_svg_text(bytes) {
			match fonts {
				Some((fonts, families)) => fonts.apply(&mut options, families),
				None => options.fontdb = svg_fonts(generic_families),
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
	fn standalone_svg_text_uses_only_pinned_faces() {
		let mappings = [("sans-serif".into(), vec!["Noto Sans".into()])];
		let database = super::svg_fonts(&mappings);
		let directories = crate::test_support::fonts().directories;
		assert!(!database.is_empty());
		for face in database.faces() {
			let resvg::usvg::fontdb::Source::File(path) = &face.source else {
				panic!("an SVG face did not come from a pinned file");
			};
			assert!(
				directories.contains(&path.parent().unwrap().to_path_buf())
			);
		}
		const SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="60" height="24"><text x="2" y="18" font-family="sans-serif" font-size="16">Ab</text></svg>"#;
		let decoded = super::decode(SVG, None, None, &mappings).unwrap();
		assert!(decoded.pixels.rgba.chunks(4).any(|pixel| pixel[3] > 0));
	}

	#[test]
	fn rejects_invalid_pixel_dimensions() {
		assert!(super::dimensions(0, 10).is_err());
		assert!(super::dimensions(5000, 4000).is_err());
		assert!(super::dimensions(4000, 4000).is_ok());
	}

	#[test]
	fn generic_svg_candidates_resolve_through_the_database() {
		let mut database = resvg::usvg::fontdb::Database::new();
		database.load_font_data(
			include_bytes!(
				"../../crates/markview-core/tests/fonts/NotoSansMono-Regular-subset.otf"
			)
			.to_vec(),
		);
		database.set_monospace_family("Noto Sans Mono");
		assert!(super::resolve_svg_family(&database, "monospace").is_some());
	}
}
