use markview_document::{
	Block, BlockKind, Document, Inline, InlineKind, Link, TextStyle,
	content_identity, fingerprint, semantic_key,
};
use markview_layout::{
	fonts::FontConfig,
	layout::{LayoutEngine, LayoutOptions},
};
use std::sync::Arc;

#[test]
fn semantic_documents_layout_without_source_parsing() {
	let source: Arc<str> = Arc::from("Read café 👋 after");
	let kind = BlockKind::Paragraph(vec![Inline {
		kind: InlineKind::Text(source.to_string()),
		style: TextStyle {
			link: Some(Link::new("https://example.com/article", 0)),
			..Default::default()
		},
		source: 0..source.len(),
	}]);
	let blocks = vec![Block {
		anchors: vec!["article".into()],
		id: fingerprint(&source),
		content_key: semantic_key(&kind),
		source: 0..source.len(),
		kind,
	}];
	let document = Document {
		source,
		content_id: content_identity(&blocks),
		blocks,
	};
	let options = LayoutOptions {
		width: 320.0,
		fonts: FontConfig::from_faces(
			0x646f63756d656e74,
			vec![parley::fontique::Blob::new(Arc::new(
				include_bytes!("fonts/NotoSerif-Regular-subset.otf").as_slice(),
			))],
		),
		..Default::default()
	};
	let mut engine = LayoutEngine::new();
	let first = engine.layout(&document, &options);
	assert_eq!(first.full_reading_text(), document.source.as_ref());
	assert!(first.anchor_y("article").is_some());
	let text = &first.blocks[0].layout.text[0];
	assert_eq!(text.clusters.first().unwrap().range.start, 0);
	assert_eq!(
		text.clusters.last().unwrap().range.end,
		document.source.len()
	);
	assert!(
		first.blocks[0]
			.layout
			.links
			.iter()
			.any(|link| { link.url.as_ref() == "https://example.com/article" })
	);

	let narrower = engine.layout(
		&document,
		&LayoutOptions {
			width: 160.0,
			..options
		},
	);
	assert!(first.same_reading_text(&narrower));
}
