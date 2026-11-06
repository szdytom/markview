use markview_image::image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use mvfuzz::image_decode;
use std::io::Cursor;

fn controls(width: u32, height: u32, initial: u16, pdf: bool) -> Vec<u8> {
	let mut out = b"IDC1".to_vec();
	out.extend(width.to_le_bytes());
	out.extend(height.to_le_bytes());
	out.extend(initial.to_le_bytes());
	out.push(u8::from(pdf));
	out.push(u8::from(width == 0 && height == 0));
	out
}

fn png(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
	let mut bytes = Vec::new();
	DynamicImage::ImageRgba8(RgbaImage::from_pixel(width, height, Rgba(color)))
		.write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
		.unwrap();
	bytes
}

#[test]
fn valid_bitmaps_svgs_and_malformed_bytes_reach_the_contract_checks() {
	let png = png(4, 3, [10, 20, 30, 40]);
	for (target, initial, pdf) in [
		((4, 3), 0, false),
		((2, 1), 0, true),
		((0, 0), 20, false),
		((9999, 9999), 0, false),
	] {
		let mut input = controls(target.0, target.1, initial, pdf);
		input.extend(&png);
		image_decode::check(&input);
	}
	let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><text x="1" y="15" font-family="sans-serif" font-size="14">Ab</text><rect width="40" height="20" fill="none"/></svg>"#;
	for target in [(0, 0), (80, 40), (0, 10)] {
		let mut input = controls(target.0, target.1, 0, false);
		input.extend(svg);
		image_decode::check(&input);
	}
	for payload in [
		&b""[..],
		&b"not an image"[..],
		&png[..png.len() / 2],
		&b"\x89PNG\r\n\x1a\n"[..],
		&b"<svg"[..],
	] {
		let mut input = controls(0, 0, 0, false);
		input.extend(payload);
		image_decode::check(&input);
	}
	// A zero SVG target is an error, while a bitmap target is clamped to 1.
	let mut input = controls(0, 0, 0, false);
	input.extend(svg);
	image_decode::check(&input);
}
