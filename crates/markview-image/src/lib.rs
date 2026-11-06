//! Raster and vector decoding with pixel limits, shared by the reader and the
//! fuzz harness.
mod decode;

pub use decode::{Decoded, SvgFonts, decode, decode_with_metadata};
pub use image;
pub use resvg;
