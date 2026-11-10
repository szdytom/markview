//! Real image bytes through the reader's decoder and pixel consumers.
use crate::coverage;
use markview_image::{Decoded, SvgFonts, decode, resvg};
use std::sync::{Arc, OnceLock};

/// The committed layout faces, injected into the SVG rasterizer so a vector
/// input with text does not depend on the host's font set.
pub struct FixedFonts(Arc<resvg::usvg::fontdb::Database>);

impl FixedFonts {
	pub fn pinned() -> &'static Self {
		static FONTS: OnceLock<FixedFonts> = OnceLock::new();
		FONTS.get_or_init(|| {
			let mut database = resvg::usvg::fontdb::Database::new();
			database.load_fonts_dir(
				std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
					.join("../../crates/markview-layout/tests/fonts"),
			);
			let family = database
				.faces()
				.next()
				.and_then(|face| face.families.first())
				.map(|(family, _)| family.clone());
			if let Some(family) = family {
				database.set_sans_serif_family(family.clone());
				database.set_serif_family(family.clone());
				database.set_monospace_family(family);
			}
			Self(database.into())
		})
	}
}

impl SvgFonts for FixedFonts {
	fn apply(&self, options: &mut resvg::usvg::Options<'_>, _: &str) {
		options.fontdb = self.0.clone();
	}
}

fn dims(bytes: &[u8]) -> (Option<(u32, u32)>, Option<f32>, bool) {
	let word =
		|at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
	let width = word(0);
	let height = word(4);
	let target = (!bytes[11].is_multiple_of(2)).then_some((width, height));
	let initial = u16::from_le_bytes(bytes[8..10].try_into().unwrap());
	let initial = (initial != 0).then(|| f32::from(initial) / 4.);
	(target, initial, !bytes[10].is_multiple_of(2))
}

fn assert_decoded(decoded: &Decoded) {
	assert!(
		decoded.pixels.width > 0 && decoded.pixels.height > 0,
		"decoded image has a zero side"
	);
	let pixels =
		u64::from(decoded.pixels.width) * u64::from(decoded.pixels.height);
	assert!(
		pixels <= 16_000_000,
		"decoded image exceeds the pixel budget"
	);
	assert_eq!(
		decoded.pixels.rgba.len() as u64,
		pixels * 4,
		"RGBA length does not match the decoded size"
	);
	assert!(
		decoded.intrinsic.0 > 0 && decoded.intrinsic.1 > 0,
		"intrinsic size is zero"
	);
	assert!(
		u64::from(decoded.intrinsic.0) * u64::from(decoded.intrinsic.1)
			<= 16_000_000,
		"intrinsic size exceeds the pixel budget"
	);
	if !decoded.svg {
		assert!(
			decoded.pixels.width <= 8192 && decoded.pixels.height <= 8192,
			"bitmap output exceeds the texture side"
		);
	}
}

/// `IDC1`, twelve control bytes, then the image payload.
pub fn check(data: &[u8]) {
	if !data.starts_with(b"IDC1") || data.len() < 16 {
		return;
	}
	let controls = &data[4..16];
	let payload = &data[16..];
	let (target, initial, pdf) = dims(controls);
	let fonts = FixedFonts::pinned();
	let decoded = decode(payload, target, initial, Some((fonts, "")), &[]);
	match decoded {
		Ok(decoded) => {
			assert_decoded(&decoded);
			if pdf {
				markview_pdf::PreparedImage::new(&decoded.pixels)
					.expect("a decoded image becomes a PDF image");
				coverage::hit(coverage::Hit::ImagePdf);
			}
			coverage::hit(coverage::Hit::ImageDecoded);
			if decoded.svg {
				coverage::hit(coverage::Hit::ImageSvg);
			}
		}
		Err(_) => coverage::hit(coverage::Hit::ImageRejected),
	}
}
