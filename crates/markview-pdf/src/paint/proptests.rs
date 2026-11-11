use super::*;
use markview_layout::{
	document::BlockId,
	fonts::FontConfig,
	image::ImageSnapshot,
	scene::{Glyph, Paint},
	text::{TextCluster, TextNode},
};
use proptest::prelude::*;

fn metadata_text(indices: Vec<usize>) -> String {
	let alphabet = ["A", "é", "中", "😀", "\\", "(", ")", "<", "&", "|"];
	format!(
		"data:{}",
		indices
			.into_iter()
			.map(|index| alphabet[index])
			.collect::<String>()
	)
}

fn face() -> &'static FontData {
	static FACE: std::sync::LazyLock<FontData> =
		std::sync::LazyLock::new(|| FontData {
			data: parley::fontique::Blob::new(Arc::new(
				include_bytes!(concat!(
					env!("CARGO_MANIFEST_DIR"),
					"/../markview-layout/tests/fonts/NotoSerif-Regular-subset.otf"
				))
				.to_vec(),
			)),
			index: 0,
		});
	&FACE
}

proptest! {
	#[test]
	fn page_frames_preserve_affine_geometry_and_formula_item_origins(
		origin in prop::array::uniform2(-64i16..257), top in -64i16..257, scale in 1u8..5,
		position in prop::array::uniform2(-64i16..257), extent in prop::array::uniform2(1u8..65), vertex in prop::array::uniform2(-32i16..33), em in 1u8..33,
	) {
		let frame = Frame { origin_x: f32::from(origin[0]), origin_y: f32::from(origin[1]), top: f32::from(top), scale: f32::from(scale) / 4.0 };
		let factor = f64::from(scale) * 3.0 / 16.0;
		let rect = LocalRect { x: f32::from(position[0]), y: f32::from(position[1]), w: f32::from(extent[0]), h: f32::from(extent[1]) };
		let actual = frame.rect(rect).unwrap();
		let expected = [f64::from(origin[0]) + f64::from(position[0]) * factor, f64::from(origin[1]) + f64::from(position[1] - top) * factor, f64::from(extent[0]) * factor, f64::from(extent[1]) * factor];
		for (actual, expected) in [actual.left(), actual.top(), actual.width(), actual.height()].into_iter().zip(expected) { prop_assert!((f64::from(actual) - expected).abs() <= 0.001); }
		let vertex = (f64::from(vertex[0]) / 8.0, f64::from(vertex[1]) / 8.0);
		let point = frame.math_point((rect.x, rect.y), f32::from(em), vertex);
		prop_assert!((f64::from(point.0) - (expected[0] + vertex.0 * f64::from(em) * factor)).abs() <= 0.001);
		prop_assert!((f64::from(point.1) - (expected[1] + vertex.1 * f64::from(em) * factor)).abs() <= 0.001);
		prop_assert_eq!(frame.y(frame.top), frame.origin_y);
	}

	#[test]
	fn real_pdf_exports_preserve_metadata_and_copyable_text_across_paint_runs_and_reuse(
		body in "[A-Za-z0-9]{1,12}", size in 8u8..33, italic in any::<bool>(), bold in any::<bool>(), split_paint in any::<bool>(),
		title in prop::option::of(prop::collection::vec(0usize..10, 0..16)), subject in prop::option::of(prop::collection::vec(0usize..10, 0..16)), creator in prop::option::of(prop::collection::vec(0usize..10, 0..16)),
		authors in prop::collection::vec(prop::collection::vec(0usize..10, 0..8), 0..4), keywords in prop::collection::vec(prop::collection::vec(0usize..10, 0..8), 0..4), language in prop::option::of(0usize..4),
	) {
		let metadata = crate::Metadata { title: title.map(metadata_text), subject: subject.map(metadata_text), creator: creator.map(metadata_text), authors: authors.into_iter().enumerate().map(|(index, text)| format!("author{index}-{}", metadata_text(text))).collect(), keywords: keywords.into_iter().enumerate().map(|(index, text)| format!("keyword{index}-{}", metadata_text(text))).collect(), language: language.map(|index| ["en", "zh-CN", "ja", "ar"][index].to_owned()) };
		let font = face().clone();
		let reference = FontRef::from_index(font.data.data(), 0).unwrap();
		let mut node = TextNode::new(body.clone(), "");
		let mut layout = BlockLayout { height: 80.0, ..Default::default() };
		for (index, ch) in body.chars().enumerate() {
			let x = 10.0 + index as f32 * f32::from(size);
			let command = layout.draws.len();
			layout.draws.push(Draw::Glyph(Glyph { style: std::sync::Arc::new(markview_layout::scene::GlyphStyle { font: font.clone(), coords: Arc::from([]), size: f32::from(size), synthetic_italic: italic, synthetic_bold: bold, paint: Paint::Color(markview_layout::style::Color(if split_paint && index % 2 == 0 { 0x112233ff } else { 0x445566ff })) }), id: reference.charmap().map(ch), x, y: 40.0, }));
			node.push(TextCluster { mixed_spacing: (0.0, 0.0), range: index..index + 1, rect: LocalRect { x, y: 8.0, w: f32::from(size), h: 40.0 }, rtl: false, atomic: false, command });
		}
		layout.text.push(node);
		let snapshot = LayoutSnapshot { blocks: vec![PlacedBlock { id: BlockId::new(1).unwrap(), content: Default::default(), y: 0.0, layout: Arc::new(layout), flow: Arc::default() }], height: 80.0, ..Default::default() };
		let pagination = Pagination { pages: vec![vec![PageItem { block: 0, bands: 0..1, scale: 1.0, y: 0.0, top: 0.0, bottom: 80.0, first: true, last: true }]], ..Default::default() };
		let geometry = PageGeometry { width_pt: 400.0, height_pt: 300.0, margin_pt: [20.0; 4] };
		let mut sheet = Stylesheet::default();
		sheet.page.footer_center = Some(String::new());
		let images = ImageSnapshot::default();
		let input = Export { outline: &[], snapshot: &snapshot, images: &images, prepared_images: None, stylesheet: &sheet, geometry: &geometry, pagination: &pagination, metadata: metadata.clone(), path: String::new(), body_size_px: f32::from(size), links: false, fonts: FontConfig::from_faces(0x7062_7470_6466_0001, vec![font.data.clone()]) };
		let mut renderer = Renderer::default();
		let bytes = renderer.export(&input).unwrap();
		prop_assert_eq!(renderer.export(&input).unwrap(), bytes.clone());
		let pdf = lopdf::Document::load_mem(&bytes).unwrap();
		let copied = pdf.extract_text(&[1]).unwrap();
		prop_assert_eq!(copied.chars().filter(|ch| !ch.is_ascii_whitespace()).collect::<String>(), body);
		let info = pdf.get_object(pdf.trailer.get(b"Info").unwrap().as_reference().unwrap()).unwrap().as_dict().unwrap();
		for (key, expected) in [(b"Title".as_slice(), metadata.title.as_ref()), (b"Subject", metadata.subject.as_ref()), (b"Creator", metadata.creator.as_ref())] {
			prop_assert_eq!(info.get(key).ok().map(|object| lopdf::decode_text_string(object).unwrap()), expected.cloned());
		}
		for (key, expected) in [(b"Author".as_slice(), &metadata.authors), (b"Keywords", &metadata.keywords)] {
			prop_assert_eq!(info.has(key), !expected.is_empty());
			if !expected.is_empty() { let actual = lopdf::decode_text_string(info.get(key).unwrap()).unwrap(); let positions: Vec<_> = expected.iter().map(|value| actual.find(value).unwrap()).collect(); prop_assert!(positions.windows(2).all(|pair| pair[0] < pair[1])); }
		}
		prop_assert_eq!(lopdf::decode_text_string(info.get(b"Producer").unwrap()).unwrap(), format!("Markview {}", env!("CARGO_PKG_VERSION")));
		prop_assert!(!info.has(b"CreationDate") && !info.has(b"ModDate"));
		prop_assert_eq!(pdf.catalog().unwrap().get(b"Lang").ok().map(|object| lopdf::decode_text_string(object).unwrap()), metadata.language);
	}
}
