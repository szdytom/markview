use markview_document::{
	BlockKind, DocumentBuilder, DocumentRevision, ImageSource, ImageSpec,
	Inline, InlineKind, Link, ResourceRef, SourceMap, TextStyle,
};
use markview_layout::{
	fonts::FontConfig,
	layout::{LayoutEngine, LayoutOptions},
	search::{SearchIndex, SearchOptions},
};
use std::sync::Arc;

#[test]
fn semantic_documents_layout_without_source_parsing() {
	let reading = "Read café 👋 after";
	let kind = BlockKind::Paragraph(vec![Inline {
		link: Some(Link::new(
			"https://example.com/article",
			markview_document::LinkOccurrenceId::new(1).unwrap(),
		)),
		kind: InlineKind::Text(reading.into()),
		style: TextStyle::default(),
	}]);
	let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
	let block = builder.block(vec!["article".into()], kind);
	let document = builder.finish(vec![block]);
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
	assert_eq!(first.full_reading_text(), reading);
	assert!(first.anchor_y("article").is_some());
	let text = &first.blocks[0].layout.text[0];
	assert_eq!(text.clusters.first().unwrap().range.start, 0);
	assert_eq!(text.clusters.last().unwrap().range.end, reading.len());
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

#[test]
fn a_source_free_document_reads_searches_and_selects_without_mapping() {
	fn rich(text: &str) -> Vec<Inline> {
		vec![Inline {
			link: None,
			kind: InlineKind::Text(text.into()),
			style: TextStyle::default(),
		}]
	}
	let mut builder = DocumentBuilder::new(DocumentRevision::new(7));
	let heading = builder.block(
		vec!["guide".into()],
		BlockKind::Heading {
			level: 1,
			text: rich("Guide"),
			anchor: "guide".into(),
		},
	);
	let linked = Inline {
		link: Some(Link::new(
			"https://example.com/doc",
			markview_document::LinkOccurrenceId::new(1).unwrap(),
		)),
		kind: InlineKind::Text("link".into()),
		style: TextStyle::default(),
	};
	let image = Inline {
		link: None,
		kind: InlineKind::Image(ImageSpec {
			source: ImageSource::Resource(ResourceRef::document("plot.png")),
			alt: "Plot".into(),
			title: String::new(),
			width: None,
			height: None,
		}),
		style: TextStyle::default(),
	};
	let paragraph = builder.block(
		Vec::new(),
		BlockKind::Paragraph(vec![
			Inline {
				link: None,
				kind: InlineKind::Text("Read café 👋 ".into()),
				style: TextStyle::default(),
			},
			image,
			Inline {
				link: None,
				kind: InlineKind::Text(" ".into()),
				style: TextStyle::default(),
			},
			linked,
		]),
	);
	let hidden =
		builder.block(Vec::new(), BlockKind::Paragraph(rich("Hidden body")));
	let details = builder.block(
		Vec::new(),
		BlockKind::Details {
			default_open: false,
			summary: rich("More"),
			blocks: vec![hidden],
		},
	);
	let document = builder.finish(vec![heading, paragraph, details]);
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
	let snapshot = engine.layout(&document, &options);
	let revision = document.revision().get();
	assert_eq!(document.outline()[0].anchor, "guide");
	assert!(snapshot.anchor_y("guide").is_some());
	let mut images = Vec::new();
	document.blocks()[1].images(&mut images);
	assert_eq!(images[0].address(), "plot.png");
	let text =
		snapshot.extract_text(snapshot.select_all(revision).unwrap(), revision);
	assert!(text.contains("Read café 👋 Plot"), "{text}");
	assert!(text.contains("link"), "{text}");
	assert!(
		snapshot.blocks[1]
			.layout
			.links
			.iter()
			.any(|link| { link.url.as_ref() == "https://example.com/doc" })
	);
	let hits =
		SearchIndex::new(&document).find("café", SearchOptions::default());
	assert_eq!(hits.len(), 1);
	assert_eq!(
		snapshot.extract_text(
			snapshot.search_selection(&hits[0], revision).unwrap(),
			revision
		),
		"café"
	);
	// A generated document has no source map at all; an empty map cannot
	// attach a span to its synthetic blocks.
	let map = SourceMap::new(document.revision(), Vec::new());
	for block in document.blocks() {
		assert!(map.block(block.id()).is_none());
	}
	// The collapsed body is still searchable, and opening it makes the hit
	// selectable like any source-backed body.
	let hidden_hits = SearchIndex::new(&document)
		.find("Hidden body", SearchOptions::default());
	assert_eq!(hidden_hits.len(), 1);
	assert!(
		snapshot
			.search_selection(&hidden_hits[0], revision)
			.is_none()
	);
	let opened = engine.layout(
		&document,
		&LayoutOptions {
			force_open: true,
			..options
		},
	);
	assert!(
		opened
			.extract_text(
				opened.search_selection(&hidden_hits[0], revision).unwrap(),
				revision
			)
			.contains("Hidden body")
	);
}

#[test]
fn link_addresses_and_groups_are_semantic_not_visual_style() {
	let parsed =
		markview_commonmark::parse("[one](a)[two](b) [three **four**](a)");
	let BlockKind::Paragraph(text) = parsed.blocks()[0].kind() else {
		panic!()
	};
	let links: Vec<_> = text.iter().filter(|i| i.link.is_some()).collect();
	assert_eq!(links[0].style, links[1].style);
	assert_ne!(links[0].link_url(), links[1].link_url());
	assert!(!links[0].shares_link_occurrence(links[1]));
	assert!(links[2].shares_link_occurrence(links[3]));
	let same =
		markview_commonmark::parse("[one](a)[two](a) [three **four**](a)");
	assert_ne!(parsed.document.content_key(), same.document.content_key());
}
