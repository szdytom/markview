//! Whether a file name or a body of bytes names a face the shaper can use.
//!
//! Every check here is self-contained. Nothing touches the filesystem, so a
//! host that hands over bytes gets the same answer a directory scan would.
use std::path::Path;

/// Tables a renderable face must have.
const REQUIRED_TABLES: [[u8; 4]; 6] =
	[*b"head", *b"maxp", *b"hhea", *b"hmtx", *b"cmap", *b"name"];
/// The outline data itself: a TrueType face has `glyf`, a PostScript one
/// `CFF `, and a variable PostScript one `CFF2`. The shaper renders all three,
/// so any one of them is enough.
const OUTLINE_TABLES: [[u8; 4]; 3] = [*b"glyf", *b"CFF ", *b"CFF2"];

/// Whether the directory names pixels the shaper can draw.
///
/// A color emoji face may carry no outline at all: `Noto Color Emoji` stores
/// its images as `CBDT` strikes located by `CBLC`, and some platforms use an
/// `sbix` table. The rasterizer draws both, so they count beside the outlines.
fn has_drawable_glyphs(tables: &[[u8; 4]]) -> bool {
	OUTLINE_TABLES.iter().any(|tag| tables.contains(tag))
		|| (tables.contains(b"CBDT") && tables.contains(b"CBLC"))
		|| tables.contains(b"sbix")
}
/// Whether `bytes` is a font file the shaper can load.
///
/// A downloaded body is checked before it is stored, so an error page or a
/// truncated transfer never becomes a registered face. Naming two tables is
/// not enough on its own: a body cut short can keep the early `head` and
/// `cmap` records while losing the outlines, metrics and names that sit later
/// in the file. The whole table directory is read instead, every record must
/// lie inside `bytes`, the tables a drawable face needs must be present, and
/// the character map must resolve at least one code point.
pub fn is_font(bytes: &[u8]) -> bool {
	let Some(tables) = table_tags(bytes) else {
		return false;
	};
	if !REQUIRED_TABLES.iter().all(|tag| tables.contains(tag))
		|| !has_drawable_glyphs(&tables)
	{
		return false;
	}
	swash::FontRef::from_index(bytes, 0)
		.is_some_and(|font| maps_a_character(&font))
}

/// Whether the first face draws with PostScript outlines, which is what names
/// an extensionless download `.otf` rather than `.ttf`.
pub fn is_postscript_outline(bytes: &[u8]) -> bool {
	table_tags(bytes)
		.is_some_and(|tags| tags.contains(b"CFF ") || tags.contains(b"CFF2"))
}

/// The table tags of the first face in `bytes`, or `None` when a record does
/// not lie inside the body.
///
/// A collection names the first face's directory at an offset; a single font
/// starts at zero.
fn table_tags(bytes: &[u8]) -> Option<Vec<[u8; 4]>> {
	let base = if bytes.starts_with(b"ttcf") {
		u32_at(bytes, 12)? as usize
	} else {
		0
	};
	let count = u16_at(bytes, base.checked_add(4)?)? as usize;
	let start = base.checked_add(12)?;
	let mut tags = Vec::with_capacity(count);
	for index in 0..count {
		let record = start.checked_add(index.checked_mul(16)?)?;
		let end = record.checked_add(4)?;
		let tag: [u8; 4] = bytes.get(record..end)?.try_into().ok()?;
		let offset = u32_at(bytes, record.checked_add(8)?)? as usize;
		let length = u32_at(bytes, record.checked_add(12)?)? as usize;
		// Every record, not only the named tables, has to fit.
		if offset.checked_add(length)? > bytes.len() {
			return None;
		}
		tags.push(tag);
	}
	Some(tags)
}

fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
	let end = offset.checked_add(2)?;
	Some(u16::from_be_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
	let end = offset.checked_add(4)?;
	Some(u32::from_be_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

/// Whether the character map resolves at least one code point. A directory
/// can parse and still describe no usable character at all.
fn maps_a_character(font: &swash::FontRef<'_>) -> bool {
	let mut mapped = false;
	font.charmap().enumerate(|_, _| mapped = true);
	mapped
}

/// Whether a file name is one the shaper's own scan would look at.
///
/// A downloaded body is validated by [`is_font`] instead, but a directory is
/// filtered by name first: reading every file to find out it is not a face
/// would cost more than the check is worth.
pub fn is_font_file(path: &Path) -> bool {
	path.extension()
		.and_then(|extension| extension.to_str())
		.is_some_and(|extension| {
			matches!(
				extension.to_ascii_lowercase().as_str(),
				"ttf" | "otf" | "ttc" | "otc"
			)
		})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn only_a_parsable_font_is_a_font() {
		let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts");
		let font =
			std::fs::read(dir.join("NotoSerif-Regular-subset.otf")).unwrap();
		assert!(is_font(&font));
		assert!(!is_font(b"<!doctype html><html>404"));
		assert!(!is_font(&font[..64]));
		// A color emoji face keeps its pixels in `CBDT` strikes and has no
		// outline table, and the shaper draws it all the same.
		let emoji =
			std::fs::read(dir.join("NotoColorEmoji-subset.ttf")).unwrap();
		assert!(is_font(&emoji));
		assert!(!is_font(&emoji[..64]));
	}
	/// A body cut after `cmap` keeps the tables the old two-table check named
	/// while losing the outlines, metrics and name that follow them.
	#[test]
	fn a_truncated_font_is_rejected() {
		let font = std::fs::read(
			Path::new(env!("CARGO_MANIFEST_DIR"))
				.join("tests/fonts/NotoSerif-Regular-subset.otf"),
		)
		.unwrap();
		// The `cmap` record ends at 1,852 bytes, so `head` and `cmap` survive
		// the cut while `fpgm`, `glyf` and `name` do not.
		assert!(!is_font(&font[..1852]));
		assert!(is_font(&font));
	}
	/// A variable PostScript face stores its outlines in `CFF2` rather than
	/// `CFF `, and the shaper renders it, so a downloaded one must be accepted.
	/// The pinned test faces are static PostScript and TrueType, so only the
	/// whitelist itself can be asserted here.
	#[test]
	fn the_outline_whitelist_accepts_variable_postscript() {
		assert!(OUTLINE_TABLES.contains(b"CFF2"));
	}
}
