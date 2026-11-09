use markview_document::{BlockKind, InlineKind, plain_text};
use markview_layout::{
	fonts::FontConfig,
	layout::{LayoutEngine, LayoutOptions},
};

#[test]
fn webpage_status_and_timestamp_keep_natural_spacing() {
	for container in ["div", "p"] {
		let update = format!(
			"<{container}><strong>Investigating</strong> - We are investigating reports of impacted performance for some GitHub services.<br><small>Nov 09, 2026 - 05:59 UTC</small></{container}>"
		);
		let html = format!(
			"<html><head><title>GitHub Status</title></head><body><article>{}</article></body></html>",
			update.repeat(4)
		);
		let article =
			markview_webpage::extract(&html, "https://www.githubstatus.com/")
				.unwrap();
		let paragraphs: Vec<_> = article.document.blocks().iter().filter(|block| {
			matches!(block.kind(), BlockKind::Paragraph(text)
				if plain_text(text).starts_with("Investigating")
				&& text.iter().any(|inline| matches!(inline.kind, InlineKind::LineBreak(_))))
		}).map(|block| block.id()).collect();
		assert_eq!(paragraphs.len(), 4);
		for greedy in [false, true] {
			let mut options = LayoutOptions {
				width: 1600.0,
				greedy,
				fonts: FontConfig {
					ignore_system_fonts: true,
					directories: vec![
						std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
							.join("tests/fonts"),
					],
					..Default::default()
				},
				..Default::default()
			};
			let justified =
				LayoutEngine::new().layout(&article.document, &options);
			options.justify = false;
			let natural =
				LayoutEngine::new().layout(&article.document, &options);
			for id in &paragraphs {
				let block = justified
					.blocks
					.iter()
					.find(|block| block.id == *id)
					.unwrap();
				let reference = natural
					.blocks
					.iter()
					.find(|block| block.id == *id)
					.unwrap();
				let geometry = |block: &markview_layout::scene::PlacedBlock| {
					block
						.layout
						.text
						.iter()
						.flat_map(|node| {
							node.clusters.iter().map(|cluster| cluster.rect)
						})
						.collect::<Vec<_>>()
				};
				let rects = geometry(block);
				assert!(!rects.is_empty());
				assert_eq!(
					rects,
					geometry(reference),
					"{container}, greedy {greedy}"
				);
				assert!(
					rects
						.iter()
						.all(|rect| rect.x + rect.w < options.width - 100.0)
				);
			}
		}
	}
}
