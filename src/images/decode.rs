//! The reader's adapter over the shared image decoder.
use anyhow::Result;
use markview_image::SvgFonts;
use std::sync::Arc;

pub(super) use markview_image::Decoded;

/// The reader's diagram faces, which also draw SVG text inside diagrams.
struct DiagramFontSource<'a> {
	fonts: &'a Arc<super::fonts::DiagramFonts>,
}

impl SvgFonts for DiagramFontSource<'_> {
	fn apply(
		&self,
		options: &mut markview_image::resvg::usvg::Options<'_>,
		families: &str,
	) {
		self.fonts.apply(options, families);
	}
}

/// Decodes one image. `fonts` is the reader's own face set and the theme's
/// family list, for an SVG that carries text; `None` resolves fonts the way
/// an SVG file outside the reader does.
#[cfg(test)]
pub(super) fn decode(
	bytes: &[u8],
	target: Option<(u32, u32)>,
	initial_width: Option<f32>,
	fonts: Option<(&Arc<super::fonts::DiagramFonts>, &str)>,
	generic_families: &[(String, Vec<String>)],
) -> Result<Decoded> {
	match fonts {
		Some((fonts, families)) => markview_image::decode(
			bytes,
			target,
			initial_width,
			Some((&DiagramFontSource { fonts }, families)),
			generic_families,
		),
		None => markview_image::decode(
			bytes,
			target,
			initial_width,
			None,
			generic_families,
		),
	}
}

pub(super) fn decode_with_metadata(
	bytes: &[u8],
	target: Option<(u32, u32)>,
	initial_width: Option<f32>,
	fonts: Option<(&Arc<super::fonts::DiagramFonts>, &str)>,
	generic_families: &[(String, Vec<String>)],
	metadata: impl FnMut((u32, u32), bool),
) -> Result<Decoded> {
	match fonts {
		Some((fonts, families)) => markview_image::decode_with_metadata(
			bytes,
			target,
			initial_width,
			Some((&DiagramFontSource { fonts }, families)),
			generic_families,
			metadata,
		),
		None => markview_image::decode_with_metadata(
			bytes,
			target,
			initial_width,
			None,
			generic_families,
			metadata,
		),
	}
}
