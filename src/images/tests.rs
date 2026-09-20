use super::*;
use super::{
	decode::decode as decode_image, pixels::pixel_bytes, source::fetch,
};
use base64::Engine;
use image::{Rgb, RgbImage, Rgba, RgbaImage};
use mermaid_rs_renderer::TextMetrics as _;
use std::{fs, io::Cursor};

/// Decodes with the system font database, as a standalone image does.
fn decode(bytes: &[u8], target: Option<(u32, u32)>) -> Result<Decoded> {
	decode_image(bytes, target, None)
}

/// The diagram theme a test that never switches stylesheets renders with.
fn diagram_theme() -> diagram::DiagramTheme {
	diagram::resolve(&Stylesheet::default(), None)
}

fn png(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
	let mut bytes = Vec::new();
	image::DynamicImage::ImageRgba8(RgbaImage::from_pixel(
		width,
		height,
		Rgba(color),
	))
	.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
	.unwrap();
	bytes
}
fn rgb_png(width: u32, height: u32, color: [u8; 3]) -> Vec<u8> {
	let mut bytes = Vec::new();
	image::DynamicImage::ImageRgb8(RgbImage::from_pixel(
		width,
		height,
		Rgb(color),
	))
	.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
	.unwrap();
	bytes
}
fn data_uri(mime: &str, bytes: &[u8]) -> String {
	format!(
		"data:{mime};base64,{}",
		base64::engine::general_purpose::STANDARD.encode(bytes)
	)
}

fn images(offline: bool) -> Images {
	// Tests never touch the user's cache directory.
	Images::with_cache(offline, None)
}

/// Renders one Mermaid fence through the image scheduler and returns the
/// scheduler with the fence's image source key.
fn fence_images(code: &str) -> (Images, String) {
	let doc = crate::document::parse(format!("```mermaid\n{code}\n```\n"));
	let mut specs = Vec::new();
	for block in &doc.blocks {
		block.images(&mut specs);
	}
	let src = specs[0].src.clone();
	let mut images = images(true);
	images.prepare(
		&doc,
		Path::new("note.md"),
		1,
		false,
		&Stylesheet::default(),
		&FontConfig::default(),
	);
	images.wait();
	(images, src)
}

#[test]
fn sources_cover_local_network_and_inline_images() {
	let dir = tempfile::tempdir().unwrap();
	let document = dir.path().join("docs/note.md");
	let document_dir = document.parent().unwrap();
	let at = |src: &str| source(src, &document).unwrap();
	assert_eq!(
		at("images/a b.png"),
		Source::File(document_dir.join("images/a b.png"))
	);
	assert_eq!(at("a%20b.png"), Source::File(document_dir.join("a b.png")));
	assert_eq!(
		at("../up.png"),
		Source::File(document_dir.join("../up.png"))
	);
	let absolute = dir.path().join("absolute/x.png");
	// Only relative paths are reachable: absolute paths and `file:` URLs are
	// refused, while `..` still names another relative location.
	let file_url = url::Url::from_file_path(&absolute).unwrap().to_string();
	assert!(source(absolute.to_str().unwrap(), &document).is_err());
	assert!(source(&file_url, &document).is_err());
	assert!(source("/etc/passwd", &document).is_err());
	assert!(!source::rooted(std::path::Path::new("images/a.png")));
	assert!(!source::rooted(std::path::Path::new("../up.png")));
	assert!(source::rooted(std::path::Path::new("/etc/passwd")));
	// A remote source resolves whether or not the run is offline; `--offline`
	// is applied when the body is read, so a cached image can still be served.
	assert_eq!(
		at("https://example.com/a.png"),
		Source::Http("https://example.com/a.png".into())
	);
	assert_eq!(
		at("data:image/png;base64,AA=="),
		Source::Data("data:image/png;base64,AA==".into())
	);
	assert!(source("", &document).is_err());
	assert!(source("ftp://example.com/a.png", &document).is_err());
	assert!(source("a%FF.png", &document).is_err());
}

#[test]
fn data_uris_decode_base64_and_percent_escapes() {
	let bytes = png(4, 2, [1, 2, 3, 255]);
	let encoded = data_uri("image/png", &bytes);
	assert_eq!(
		fetch(&Source::Data(encoded), false, None, &diagram_theme()).unwrap(),
		bytes
	);
	let plain = "data:image/svg+xml,%3Csvg%3E%3C/svg%3E";
	assert_eq!(
		fetch(&Source::Data(plain.into()), false, None, &diagram_theme())
			.unwrap(),
		b"<svg></svg>"
	);
	assert!(
		fetch(
			&Source::Data("data:text/plain,hello".into()),
			false,
			None,
			&diagram_theme(),
		)
		.is_err()
	);
	assert!(
		fetch(
			&Source::Data("data:image/png;base64,!!".into()),
			false,
			None,
			&diagram_theme(),
		)
		.is_err()
	);
}

#[test]
fn bitmap_and_animation_formats_use_their_first_frame() {
	let (w, h) = (12, 8);
	let mut formats =
		vec![png(w, h, [10, 20, 30, 255]), rgb_png(w, h, [10, 20, 30])];
	for format in [image::ImageFormat::Bmp, image::ImageFormat::Jpeg] {
		let mut bytes = Vec::new();
		image::DynamicImage::ImageRgb8(RgbImage::from_pixel(
			w,
			h,
			Rgb([10, 20, 30]),
		))
		.write_to(&mut Cursor::new(&mut bytes), format)
		.unwrap();
		formats.push(bytes);
	}
	// An ICO whose entry is a PNG that is not 32-bit RGBA.
	let payload = rgb_png(w, h, [10, 20, 30]);
	let mut ico = vec![0, 0, 1, 0, 1, 0];
	ico.extend([w as u8, h as u8, 0, 0, 1, 0, 32, 0]);
	ico.extend((payload.len() as u32).to_le_bytes());
	ico.extend(22u32.to_le_bytes());
	ico.extend(&payload);
	formats.push(ico);
	for bytes in formats {
		let decoded = decode(&bytes, None).unwrap();
		assert_eq!(decoded.intrinsic, (w, h));
		assert_eq!(decoded.pixels.width, w);
		assert_eq!(&decoded.pixels.rgba[..4], &[10, 20, 30, 255]);
		assert!(!decoded.svg);
	}
	let mut gif = Vec::new();
	{
		let mut encoder = image::codecs::gif::GifEncoder::new(&mut gif);
		for color in [[1u8, 0, 0, 255], [0, 2, 0, 255]] {
			encoder
				.encode_frame(image::Frame::new(RgbaImage::from_pixel(
					4,
					4,
					Rgba(color),
				)))
				.unwrap();
		}
	}
	let decoded = decode(&gif, None).unwrap();
	assert_eq!(decoded.intrinsic, (4, 4));
	assert_eq!(&decoded.pixels.rgba[..4], &[1, 0, 0, 255]);
}

#[test]
fn mermaid_fences_render_through_the_image_scheduler() {
	// `--offline` still renders diagrams: they are local computation.
	let source = "```mermaid\ngraph TD\n A[Start] --> B[End]\n```\n";
	let doc = crate::document::parse(source);
	let mut specs = Vec::new();
	for block in &doc.blocks {
		block.images(&mut specs);
	}
	let src = specs[0].src.clone();
	assert!(src.starts_with(markview_core::image::MERMAID_SCHEME));
	let mut images = images(true);
	images.prepare(
		&doc,
		Path::new("note.md"),
		1,
		false,
		&Stylesheet::default(),
		&FontConfig::default(),
	);
	images.wait();
	let entry = &images.snapshot.entries[&src];
	assert!(entry.error.is_none());
	let (width, height) = entry.size.expect("diagram size");
	assert!(width > 0 && height > 0);
	let pixels = images.snapshot.pixels.decoded.lock().unwrap();
	let pixels = &pixels[&src];
	assert!(pixels.rgba.chunks(4).any(|p| p[3] > 0), "blank diagram");
}

#[test]
fn the_readers_own_faces_measure_a_diagram() {
	// The provider resolves through the shaper's collection, so a diagram is
	// measured with the faces the stylesheet selected, Han text included.
	let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("crates/markview-core/tests/fonts");
	let config = FontConfig {
		ignore_system_fonts: true,
		directories: vec![dir],
		revision: 0,
	};
	let fonts = DiagramFonts::get(&config, &[]);
	let families: Vec<String> =
		fonts.faces().into_iter().map(|face| face.family).collect();
	let family = families
		.iter()
		.find(|family| family.contains("Sans"))
		.expect("a sans face in the test collection");
	let latin = fonts
		.measure_text_width("Hello", 16.0, family)
		.expect("the collection measures Latin");
	assert!(latin > 16.0, "{family}: {latin}");
	// A family that draws no Han falls back to the stylesheet's Han faces, and
	// an unserved family is declined rather than guessed.
	let han = families
		.iter()
		.find(|family| family.contains("CJK"))
		.expect("a CJK face in the test collection");
	assert!(fonts.cover(han, '\u{6c49}').is_some());
	assert!(
		fonts
			.measure_text_width("Hello", 16.0, "No Such Family")
			.is_none()
	);
	// A generic resolves to a face rather than being taken as a literal
	// family, so a theme that names `monospace` still measures.
	let mono = fonts
		.generics()
		.into_iter()
		.find(|(generic, _)| *generic == "monospace")
		.map(|(_, family)| family)
		.expect("a mono face in the test collection");
	assert_eq!(
		fonts.measure_text_width("Hello", 16.0, "monospace"),
		fonts.measure_text_width("Hello", 16.0, &mono)
	);
}

#[test]
fn only_a_diagram_uses_the_readers_faces() {
	// A standalone SVG's missing glyphs must fall back through the system
	// resolver, not through an unrelated `[mermaid] font_family`.
	let theme = diagram_theme();
	let fonts = DiagramFonts::get(&FontConfig::default(), &[]);
	for source in [
		Source::File("a.svg".into()),
		Source::Http("https://example.com/a.svg".into()),
		Source::Data("data:image/svg+xml,%3Csvg%3E%3C/svg%3E".into()),
	] {
		assert!(
			rasterizer_fonts(&source, Some(&fonts), &theme).is_none(),
			"{source:?}"
		);
	}
	let diagram = Source::Diagram("graph TD\n A-->B\n".into());
	assert!(
		rasterizer_fonts(&diagram, Some(&fonts), &theme).is_some(),
		"a diagram kept the system resolver"
	);
}

#[test]
fn diagram_fonts_are_built_only_for_a_diagram() {
	// A document without a diagram never scans the reader's font collection.
	let mut plain = images(true);
	plain.prepare(
		&crate::document::parse("plain **text** and ![a](a.svg)"),
		Path::new("note.md"),
		1,
		false,
		&Stylesheet::default(),
		&FontConfig::default(),
	);
	assert!(plain.diagram.is_none(), "an SVG loaded the diagram faces");
	// A document with a diagram builds them.
	let mut diagram = images(true);
	diagram.prepare(
		&crate::document::parse("```mermaid\ngraph TD\n A-->B\n```\n"),
		Path::new("note.md"),
		1,
		false,
		&Stylesheet::default(),
		&FontConfig::default(),
	);
	assert!(
		diagram.diagram.is_some(),
		"the diagram faces were not built"
	);
}

#[test]
fn a_private_font_directory_draws_a_diagram() {
	// The face comes from `--fonts` alone: with the system set off, a diagram
	// still measures and draws, which is what the reader's own collection is
	// shared with the renderer and the rasterizer for.
	let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("crates/markview-core/tests/fonts");
	let config = FontConfig {
		ignore_system_fonts: true,
		directories: vec![dir],
		revision: 0,
	};
	let sheet = Stylesheet::parse(
		"format_version=2\nversion=1\n\
		 [mermaid]\ntheme='default'\nfont_family=['Noto Serif']",
	)
	.unwrap();
	let doc = crate::document::parse(
		"```mermaid\ngraph TD\n A[Wide label] --> B[Serif test]\n```\n",
	);
	let mut specs = Vec::new();
	for block in &doc.blocks {
		block.images(&mut specs);
	}
	let src = specs[0].src.clone();
	let mut images = Images::with_cache_and_fonts(true, None, config.clone());
	images.prepare(&doc, Path::new("note.md"), 1, false, &sheet, &config);
	images.wait();
	let entry = &images.snapshot.entries[&src];
	assert!(entry.error.is_none(), "{entry:?}");
	assert!(entry.size.is_some());
	let pixels = images.snapshot.pixels.decoded.lock().unwrap();
	assert!(
		pixels[&src].rgba.chunks(4).any(|pixel| pixel[3] > 0),
		"blank diagram"
	);
}

#[test]
fn a_generic_family_still_draws_in_a_diagram() {
	// A database built face by face has no generic mappings, so text that
	// names `sans-serif` would be skipped before character fallback runs.
	const SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="60" height="24"><text x="2" y="18" font-family="sans-serif" font-size="16" fill="#000000">Ab</text></svg>"##;
	for config in [
		FontConfig::default(),
		// A pinned directory has no system generics to borrow either.
		FontConfig {
			ignore_system_fonts: true,
			directories: vec![
				Path::new(env!("CARGO_MANIFEST_DIR"))
					.join("crates/markview-core/tests/fonts"),
			],
			revision: 0,
		},
	] {
		let fonts = DiagramFonts::get(&config, &[]);
		let decoded =
			decode_image(SVG, None, Some((&fonts, "sans-serif"))).unwrap();
		assert!(
			decoded.pixels.rgba.chunks(4).any(|pixel| pixel[3] > 0),
			"no ink for {config:?}"
		);
	}
}

#[test]
fn a_new_font_configuration_reaches_the_diagrams() {
	let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("crates/markview-core/tests/fonts");
	let system = FontConfig::default();
	// A download adds the directory and bumps the revision; the request that
	// follows carries the configuration.
	let downloaded = FontConfig {
		ignore_system_fonts: true,
		directories: vec![dir],
		revision: 1,
	};
	let doc = crate::document::parse(
		"```mermaid\ngraph TD\n A[Start] --> B[End]\n```\n",
	);
	let mut specs = Vec::new();
	for block in &doc.blocks {
		block.images(&mut specs);
	}
	let src = specs[0].src.clone();
	let path = Path::new("note.md");
	let sheet = Stylesheet::default();
	let mut images = Images::with_cache_and_fonts(true, None, system.clone());
	images.prepare(&doc, path, 1, false, &sheet, &system);
	images.wait();
	let painted = |images: &Images| {
		let pixels = images.snapshot.pixels.decoded.lock().unwrap();
		crate::document::fingerprint(&pixels[&src].rgba.to_vec())
	};
	let before = painted(&images);
	let key = source(&specs[0].src, path).unwrap();
	let faces = images
		.diagram
		.as_ref()
		.expect("a diagram built the faces")
		.key();
	images.prepare(&doc, path, 2, false, &sheet, &downloaded);
	assert_eq!(images.fonts, downloaded);
	assert_ne!(
		images
			.diagram
			.as_ref()
			.expect("a diagram built the faces")
			.key(),
		faces
	);
	assert!(
		images.entries[&key].busy,
		"the diagram was not scheduled again"
	);
	images.wait();
	assert!(
		images.entries[&key].info.error.is_none(),
		"{:?}",
		images.entries[&key].info.error
	);
	assert_ne!(painted(&images), before, "diagrams kept the old faces");
}

#[test]
fn a_new_diagram_theme_redraws_the_diagram() {
	let doc = crate::document::parse(
		"```mermaid\ngraph TD\n A[Start] --> B[End]\n```\n",
	);
	let mut specs = Vec::new();
	for block in &doc.blocks {
		block.images(&mut specs);
	}
	let src = specs[0].src.clone();
	let mut images = images(true);
	images.prepare(
		&doc,
		Path::new("note.md"),
		1,
		false,
		&Stylesheet::default(),
		&FontConfig::default(),
	);
	images.wait();
	let painted = |images: &Images| {
		let pixels = images.snapshot.pixels.decoded.lock().unwrap();
		pixels[&src].rgba.to_vec()
	};
	let light = painted(&images);
	// The same revision and the same source, only the stylesheet differs.
	let dark = Stylesheet::parse(
		"format_version=2\nversion=1\n[mermaid]\ntheme='dark'\nbackground='#101820'",
	)
	.unwrap();
	images.prepare(
		&doc,
		Path::new("note.md"),
		1,
		false,
		&dark,
		&FontConfig::default(),
	);
	images.wait();
	let dark = painted(&images);
	assert_ne!(light, dark);
	// The diagram paints its own background over the whole canvas.
	assert_eq!(&dark[..4], &[0x10, 0x18, 0x20, 255]);
}

#[test]
fn a_diagram_failure_does_not_outlive_its_theme() {
	// A text size far past the pixel limit makes the drawing fail.
	let doc = crate::document::parse(
		"```mermaid\ngraph TD\n A[Start] --> B[End]\n```\n",
	);
	let mut specs = Vec::new();
	for block in &doc.blocks {
		block.images(&mut specs);
	}
	let src = specs[0].src.clone();
	let mut images = images(true);
	let huge = Stylesheet::parse(
		"format_version=2\nversion=1\n[mermaid]\nfont_size=4000",
	)
	.unwrap();
	images.prepare(
		&doc,
		Path::new("note.md"),
		1,
		false,
		&huge,
		&FontConfig::default(),
	);
	images.wait();
	assert!(images.snapshot.entries[&src].error.is_some());
	// The next theme draws a size that fits, and must get its own chance.
	let dark = Stylesheet::parse(
		"format_version=2\nversion=1\n[mermaid]\ntheme='dark'",
	)
	.unwrap();
	images.prepare(
		&doc,
		Path::new("note.md"),
		1,
		false,
		&dark,
		&FontConfig::default(),
	);
	images.wait();
	let entry = &images.snapshot.entries[&src];
	assert!(entry.error.is_none(), "{entry:?}");
	assert!(entry.size.is_some());
}

#[test]
fn a_failure_from_the_previous_theme_is_retried() {
	let mut images = images(true);
	// The test delivers completions itself, so no real job can race it.
	let (send, recv) = mpsc::channel();
	images.recv = recv;
	let doc = crate::document::parse("```mermaid\ngraph TD\n A-->B\n```\n");
	let mut specs = Vec::new();
	for block in &doc.blocks {
		block.images(&mut specs);
	}
	let path = Path::new("note.md");
	let alias = specs[0].src.clone();
	let src = source(&alias, path).unwrap();
	images.prepare(
		&doc,
		path,
		1,
		false,
		&Stylesheet::default(),
		&FontConfig::default(),
	);
	// A job under the current theme is still in flight when the reader
	// switches, and it is about to fail.
	let old_ticket = {
		let e = images.entries.get_mut(&src).unwrap();
		e.busy = true;
		e.ticket = VERSION.fetch_add(1, Ordering::Relaxed);
		e.ticket
	};
	let dark = Stylesheet::parse(
		"format_version=2\nversion=1\n[mermaid]\ntheme='dark'",
	)
	.unwrap();
	images.prepare(&doc, path, 1, false, &dark, &FontConfig::default());
	assert!(images.entries[&src].busy);
	send.send(Finished {
		source: src.clone(),
		generation: images.generation,
		ticket: old_ticket,
		result: Err(anyhow::Error::msg(
			"Image exceeds 16 million pixels or has invalid dimensions",
		)),
	})
	.unwrap();
	images.poll();
	// The failure belonged to the old theme: the new one was scheduled.
	let entry = &images.entries[&src];
	assert!(entry.info.error.is_none(), "{:?}", entry.info.error);
	assert!(entry.busy, "the diagram was not scheduled again");
	assert_ne!(entry.ticket, old_ticket);
}

#[test]
fn broken_mermaid_diagram_becomes_an_error_placeholder() {
	// An unclosed subgraph is invalid; it must not panic or blank the reader.
	let source = "```mermaid\nflowchart LR\n subgraph S\n  A-->B\n```\n";
	let doc = crate::document::parse(source);
	let mut specs = Vec::new();
	for block in &doc.blocks {
		block.images(&mut specs);
	}
	let src = specs[0].src.clone();
	let mut images = images(false);
	images.prepare(
		&doc,
		Path::new("note.md"),
		1,
		false,
		&Stylesheet::default(),
		&FontConfig::default(),
	);
	images.wait();
	let entry = &images.snapshot.entries[&src];
	assert!(entry.error.is_some());
	assert_eq!(entry.size, None);
	assert!(
		!images
			.snapshot
			.pixels
			.decoded
			.lock()
			.unwrap()
			.contains_key(&src)
	);
}

#[test]
fn diagram_graph_budget_accepts_the_limit_and_rejects_one_more() {
	assert!(diagram::within_graph_budget(diagram::MAX_GRAPH_ELEMENTS));
	assert!(!diagram::within_graph_budget(
		diagram::MAX_GRAPH_ELEMENTS + 1
	));
}

#[test]
fn diagram_nesting_budget_accepts_the_limit_and_rejects_one_more() {
	let nested = |groups: usize| {
		format!("A[\"$${}x{}$$\"]", "^{".repeat(groups), "}".repeat(groups))
	};
	assert!(diagram::within_nesting_budget(&nested(
		diagram::MAX_LABEL_NESTING
	)));
	assert!(!diagram::within_nesting_budget(&nested(
		diagram::MAX_LABEL_NESTING + 1
	)));
}

#[test]
fn pathological_mermaid_label_nesting_becomes_an_error_placeholder() {
	// The reproduction from the review: one node whose quoted label nests
	// 2,600 `^{` groups inside `$$` math. At 7,823 bytes it passes the source
	// and graph budgets, but the text normalizer recurses once per group and
	// overflows a worker's default stack in a debug build. A stack overflow
	// aborts the process, so the nesting bound must reject it before layout.
	// Completing this test at all is the no-abort assertion.
	let code = format!(
		"flowchart TD\nA[\"$${}x{}$$\"]",
		"^{".repeat(2600),
		"}".repeat(2600)
	);
	assert!(
		code.len() < diagram::MAX_SOURCE_BYTES,
		"the reproduction must pass the source cap: {} bytes",
		code.len()
	);
	let (images, src) = fence_images(&code);
	let entry = &images.snapshot.entries[&src];
	assert!(
		entry.error.as_deref().is_some_and(|e| e.contains("nest")),
		"{entry:?}"
	);
	assert_eq!(entry.size, None);
	assert!(
		!images
			.snapshot
			.pixels
			.decoded
			.lock()
			.unwrap()
			.contains_key(&src)
	);
}

#[test]
fn pathological_mermaid_chain_becomes_an_error_placeholder() {
	// The reproduction from the review: a 20,000-edge chain, about 298 KiB.
	// The layout's recursive traversal overflows a default worker stack on
	// this, and a stack overflow aborts the process, so the bound must reject
	// the source before the renderer is called.
	let mut code = String::from("flowchart TD\n");
	for i in 0..20_000 {
		code.push_str(&format!("N{i}-->N{}\n", i + 1));
	}
	let (images, src) = fence_images(&code);
	let entry = &images.snapshot.entries[&src];
	assert!(
		entry
			.error
			.as_deref()
			.is_some_and(|e| e.contains("exceeds")),
		"{entry:?}"
	);
	assert_eq!(entry.size, None);
	assert!(
		!images
			.snapshot
			.pixels
			.decoded
			.lock()
			.unwrap()
			.contains_key(&src)
	);
}

#[test]
fn mermaid_chain_just_under_the_graph_budget_renders() {
	// Every edge adds one node and one edge, so a path of `n` edges spends
	// `2n + 1` of the budget. This one stays just inside it.
	let edges = diagram::MAX_GRAPH_ELEMENTS / 2 - 1;
	let mut code = String::from("flowchart TD\n");
	for i in 0..edges {
		code.push_str(&format!("N{i}-->N{}\n", i + 1));
	}
	let (images, src) = fence_images(&code);
	let entry = &images.snapshot.entries[&src];
	assert!(entry.error.is_none(), "{entry:?}");
	let (width, height) = entry.size.expect("diagram size");
	assert!(width > 0 && height > 0);
	let pixels = images.snapshot.pixels.decoded.lock().unwrap();
	assert!(
		pixels[&src].rgba.chunks(4).any(|p| p[3] > 0),
		"blank diagram"
	);
}

#[test]
fn svg_renders_at_the_intrinsic_and_requested_size() {
	let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="#ff0000"/></svg>"##;
	let decoded = decode(svg, None).unwrap();
	assert!(decoded.svg);
	assert_eq!(decoded.intrinsic, (40, 20));
	assert_eq!((decoded.pixels.width, decoded.pixels.height), (40, 20));
	assert_eq!(&decoded.pixels.rgba[..4], &[255, 0, 0, 255]);
	let scaled = decode(svg, Some((80, 40))).unwrap();
	assert_eq!(scaled.intrinsic, (40, 20));
	assert_eq!((scaled.pixels.width, scaled.pixels.height), (80, 40));
	assert!(decode(b"not an image", None).is_err());
}

#[test]
#[ignore = "requires a GPU; writes artifacts/images.png"]
fn gpu_frame_draws_decoded_images() -> Result<()> {
	use crate::{
		layout::{LayoutEngine, LayoutOptions},
		render::{Renderer, View},
	};
	let dir = tempfile::tempdir()?;
	let path = dir.path().join("note.md");
	let source =
		"![png](a.png)\n\n<img src=\"b.svg\" width=\"80\">\n\n![svg](b.svg)\n";
	fs::write(&path, source)?;
	fs::write(dir.path().join("a.png"), png(40, 30, [255, 0, 255, 255]))?;
	fs::write(
		dir.path().join("b.svg"),
		br##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="30"><rect width="40" height="30" fill="#00ffff"/></svg>"##,
	)?;
	let doc = crate::document::parse(source.to_string());
	let mut images = images(true);
	images.prepare(
		&doc,
		&path,
		1,
		false,
		&Stylesheet::default(),
		&FontConfig::default(),
	);
	images.wait();
	let mut snapshot = LayoutEngine::new().layout_with_images(
		&doc,
		&LayoutOptions {
			width: 400.,
			fonts: crate::test_support::fonts(),
			..Default::default()
		},
		&images.snapshot,
	);
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	let target = renderer.offscreen(400, 300);
	let horizontal = HashMap::new();
	let view = View {
		selection: None,
		revision: 1,
		width: 400,
		height: 300,
		scale: 1.,
		scroll: 0.,
		left: 0.,
		top: 0.,
		bottom: 0.,
		theme: crate::render::Theme::Light,
		horizontal: &horizontal,
		hovered_link: None,
		hovered_overflow: None,
		held_overflow: None,
	};
	let submission = renderer.render(
		&snapshot,
		&view,
		&[],
		&target.create_view(&Default::default()),
	)?;
	renderer.wait(Some(submission))?;
	assert_eq!(
		images.snapshot.pixels.demand.lock().unwrap()["b.svg"].size,
		(80, 60)
	);
	images.wait();
	assert_eq!(
		images.snapshot.pixels.decoded.lock().unwrap()["b.svg"].width,
		80
	);
	// Updating the resource metadata through layout also updates Draw versions.
	snapshot = LayoutEngine::new().layout_with_images(
		&doc,
		&LayoutOptions {
			width: 400.,
			fonts: crate::test_support::fonts(),
			..Default::default()
		},
		&images.snapshot,
	);
	let submission = renderer.render(
		&snapshot,
		&view,
		&[],
		&target.create_view(&Default::default()),
	)?;
	renderer.wait(Some(submission))?;
	let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("artifacts/images.png");
	fs::create_dir_all(output.parent().unwrap())?;
	renderer.save_png(&target, &output)?;
	let frame = image::open(&output)?.to_rgb8();
	let count = |want: [u8; 3]| {
		frame
			.pixels()
			.filter(|p| {
				let p = p.0;
				(0..3).all(|i| p[i].abs_diff(want[i]) <= 6)
			})
			.count()
	};
	assert!(count([255, 0, 255]) > 800, "PNG pixels missing");
	assert!(count([0, 255, 255]) > 800, "SVG pixels missing");
	Ok(())
}

#[test]
fn loader_publishes_pixels_and_reports_failures() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("note.md");
	let source = "![a](a.png) ![b](missing.png)";
	fs::write(&path, source).unwrap();
	fs::write(dir.path().join("a.png"), png(6, 4, [9, 8, 7, 255])).unwrap();
	let doc = crate::document::parse(source.to_string());
	let mut images = images(true);
	images.prepare(
		&doc,
		&path,
		1,
		false,
		&Stylesheet::default(),
		&FontConfig::default(),
	);
	images.wait();
	assert_eq!(images.snapshot.entries["a.png"].size, Some((6, 4)));
	assert!(images.snapshot.entries["a.png"].error.is_none());
	assert!(images.snapshot.entries["missing.png"].error.is_some());
	let pixels = images.snapshot.pixels.decoded.lock().unwrap();
	assert_eq!(pixels["a.png"].width, 6);
	assert!(!pixels.contains_key("missing.png"));
}

#[test]
fn renamed_alias_reuses_pixels_and_removed_aliases_are_released() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("note.md");
	fs::write(dir.path().join("a.png"), png(6, 4, [1, 2, 3, 255])).unwrap();
	let mut images = images(true);
	images.prepare(
		&crate::document::parse("![a](a.png)"),
		&path,
		1,
		false,
		&Stylesheet::default(),
		&FontConfig::default(),
	);
	images.wait();
	let first = images.snapshot.pixels.decoded.lock().unwrap()["a.png"].clone();
	let version = images.snapshot.entries["a.png"].version;
	images.prepare(
		&crate::document::parse("![a](./a.png)"),
		&path,
		2,
		false,
		&Stylesheet::default(),
		&FontConfig::default(),
	);
	assert_eq!(images.snapshot.entries["./a.png"].version, version);
	let pixels = images.snapshot.pixels.decoded.lock().unwrap();
	assert!(!pixels.contains_key("a.png"));
	assert!(Arc::ptr_eq(&first, &pixels["./a.png"]));
}

#[test]
fn obsolete_completion_cannot_replace_a_readded_resource() {
	let mut images = images(true);
	let (send, recv) = mpsc::channel();
	images.recv = recv;
	let path = Path::new("/unused/note.md");
	let doc = crate::document::parse("![a](a.png)");
	images.prepare(
		&doc,
		path,
		1,
		false,
		&Stylesheet::default(),
		&FontConfig::default(),
	);
	let src = source("a.png", path).unwrap();
	let old_ticket = images.entries[&src].ticket;
	images.prepare(
		&crate::document::parse("no image"),
		path,
		2,
		false,
		&Stylesheet::default(),
		&FontConfig::default(),
	);
	images.prepare(
		&doc,
		path,
		3,
		false,
		&Stylesheet::default(),
		&FontConfig::default(),
	);
	assert_ne!(images.entries[&src].ticket, old_ticket);
	send.send(Finished {
		source: src.clone(),
		generation: images.generation,
		ticket: old_ticket,
		result: decode(&png(2, 2, [0, 0, 0, 255]), None),
	})
	.unwrap();
	images.poll();
	assert!(images.entries[&src].busy);
	assert_eq!(images.snapshot.entries["a.png"].size, None);
}

#[test]
fn pixel_budget_counts_allocations_and_evicts_even_when_all_are_visible() {
	use markview_core::image::ImageDemand;
	let a = decode(&png(2, 2, [1, 0, 0, 255]), None).unwrap().pixels;
	let b = decode(&png(2, 2, [2, 0, 0, 255]), None).unwrap().pixels;
	let mut pixels =
		HashMap::from([("a".into(), a.clone()), ("alias".into(), a)]);
	assert_eq!(pixel_bytes(&pixels), 16);
	let demand = HashMap::from([
		(
			"a".into(),
			ImageDemand {
				size: (2, 2),
				needs_pixels: false,
			},
		),
		(
			"alias".into(),
			ImageDemand {
				size: (2, 2),
				needs_pixels: false,
			},
		),
	]);
	cache_pixels(&mut pixels, &["b".into()], b, &demand, 16);
	assert_eq!(pixel_bytes(&pixels), 16);
	assert_eq!(pixels.len(), 1);
	assert!(pixels.contains_key("b"));
}

#[test]
fn vector_demand_merges_alias_sizes_and_gpu_residency_avoids_refetch() {
	use markview_core::image::ImageDemand;
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("note.md");
	fs::write(
		dir.path().join("a.svg"),
		br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"/>"#,
	)
	.unwrap();
	let mut images = images(true);
	images.prepare(
		&crate::document::parse("![a](a.svg) ![b](./a.svg)"),
		&path,
		1,
		false,
		&Stylesheet::default(),
		&FontConfig::default(),
	);
	images.wait();
	*images.snapshot.pixels.demand.lock().unwrap() = HashMap::from([
		(
			"a.svg".into(),
			ImageDemand {
				size: (160, 80),
				needs_pixels: false,
			},
		),
		(
			"./a.svg".into(),
			ImageDemand {
				size: (80, 40),
				needs_pixels: false,
			},
		),
	]);
	images.wait();
	let version = images.snapshot.entries["a.svg"].version;
	assert_eq!(
		images.entries.values().next().unwrap().raster,
		Some((160, 80))
	);
	images.snapshot.pixels.decoded.lock().unwrap().clear();
	images.poll();
	assert!(!images.entries.values().next().unwrap().busy);
	assert_eq!(images.snapshot.entries["a.svg"].version, version);
}

#[test]
fn private_and_local_addresses_are_refused() {
	use std::net::IpAddr;
	for ip in [
		"127.0.0.1",
		"10.0.0.1",
		"172.16.0.1",
		"192.168.1.1",
		"169.254.1.1",
		"0.0.0.0",
		"100.64.0.1",
		"240.0.0.1",
		"192.0.2.5",
		"224.0.0.1",
		"::1",
		"fe80::1",
		"fd00::1",
		"::ffff:127.0.0.1",
	] {
		let ip: IpAddr = ip.parse().unwrap();
		assert!(!net::permitted(ip), "{ip}");
	}
	for ip in ["8.8.8.8", "1.1.1.1", "93.184.216.34", "2606:4700::1111"] {
		let ip: IpAddr = ip.parse().unwrap();
		assert!(net::permitted(ip), "{ip}");
	}
}

#[test]
fn bracketed_ipv6_hosts_are_parsed_and_refused_before_connecting() {
	// `Url::host_str` keeps the brackets; a lookup on "[::1]" fails, which
	// used to report a resolution error instead of the address policy.
	for url in [
		"http://[::1]:9/x.png",
		"http://[fe80::1]:9/x.png",
		"http://[fd00::1]:9/x.png",
	] {
		let error =
			fetch(&Source::Http(url.into()), false, None, &diagram_theme())
				.unwrap_err()
				.to_string();
		assert!(error.contains("local or private address"), "{url}: {error}");
	}
}

#[test]
fn remote_images_are_capped_per_document_and_revision() {
	// The documentation range is refused without a connection, so this test
	// exercises the cap and the address policy without touching the network.
	let many = |count: usize| {
		let mut source = String::new();
		for i in 0..count {
			source.push_str(&format!("![a](http://192.0.2.1/{i}.png)\n\n"));
		}
		markview_core::document::parse(source)
	};
	let doc = many(130);
	let path = std::path::Path::new("note.md");
	let mut images = images(false);
	images.prepare(
		&doc,
		path,
		1,
		false,
		&Stylesheet::default(),
		&FontConfig::default(),
	);
	assert_eq!(images.deferred_remote(), 2);
	// Reloading the same revision does not change which images were deferred.
	images.prepare(
		&doc,
		path,
		1,
		false,
		&Stylesheet::default(),
		&FontConfig::default(),
	);
	assert_eq!(images.deferred_remote(), 2);
	// Lifting the cap schedules the remainder for this revision only.
	images.prepare(
		&doc,
		path,
		1,
		true,
		&Stylesheet::default(),
		&FontConfig::default(),
	);
	assert_eq!(images.deferred_remote(), 0);
	// The next revision is capped again.
	images.prepare(
		&doc,
		path,
		2,
		false,
		&Stylesheet::default(),
		&FontConfig::default(),
	);
	assert_eq!(images.deferred_remote(), 2);
	// A different document is never affected by another tab's exemption, even
	// when its own preparation asks for the cap.
	images.prepare(
		&doc,
		std::path::Path::new("other.md"),
		1,
		true,
		&Stylesheet::default(),
		&FontConfig::default(),
	);
	assert_eq!(images.deferred_remote(), 0);
	let other = many(131);
	images.prepare(
		&other,
		std::path::Path::new("other.md"),
		2,
		false,
		&Stylesheet::default(),
		&FontConfig::default(),
	);
	assert_eq!(images.deferred_remote(), 3);
	images.prepare(
		&doc,
		path,
		1,
		false,
		&Stylesheet::default(),
		&FontConfig::default(),
	);
	assert_eq!(images.deferred_remote(), 2);
}

#[test]
fn offline_serves_a_cached_remote_image_and_fails_without_one() {
	let dir = tempfile::tempdir().unwrap();
	let root = dir.path().join("cache");
	let url = "https://example.com/cached.png";
	let bytes = png(5, 3, [4, 5, 6, 255]);
	// A stored entry with no expiry is stale; offline reading still wants it.
	super::cache::Cache::new(root.clone()).put(url, Default::default(), &bytes);
	let path = dir.path().join("note.md");
	let document = format!("![a]({url})");
	fs::write(&path, &document).unwrap();
	let doc = crate::document::parse(document);
	let mut images = Images::with_cache(true, Some(root));
	images.prepare(
		&doc,
		&path,
		1,
		false,
		&Stylesheet::default(),
		&FontConfig::default(),
	);
	images.wait();
	let entry = &images.snapshot.entries[url];
	assert!(entry.error.is_none(), "{entry:?}");
	assert_eq!(entry.size, Some((5, 3)));
	assert_eq!(images.snapshot.pixels.decoded.lock().unwrap()[url].width, 5);
	// With nothing cached, `--offline` fails with the reader's usual message.
	let missing = "https://example.com/missing.png";
	let document = format!("![a]({missing})");
	fs::write(&path, &document).unwrap();
	let doc = crate::document::parse(document);
	let mut images = Images::with_cache(true, None);
	images.prepare(
		&doc,
		&path,
		1,
		false,
		&Stylesheet::default(),
		&FontConfig::default(),
	);
	images.wait();
	assert_eq!(
		images.snapshot.entries[missing].error.as_deref(),
		Some("Network images disabled (--offline)")
	);
}
