use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn picker_preview_cancels_inside_a_top_level_container() {
	for source in [
		"- A list item.\n".repeat(128),
		"> A quoted paragraph.\n>\n".repeat(128),
	] {
		let document = crate::loaded::LoadedDocument::markdown(Arc::new(
			markview_commonmark::parse(source),
		));
		assert_eq!(document.document().blocks().len(), 1);
		let token = CancellationToken::new();
		let scene = Scene::new(
			Content {
				security: crate::security::Security::default(),
				workspace: None,
				offline: true,
				document: Some(document),
				destination: Destination {
					path: None,
					anchor: None,
				},
				stamp: None,
				options: crate::layout::LayoutOptions {
					preview: true,
					viewport_height: 120.0,
					..crate::test_support::options()
				},
				images: Default::default(),
				fit: false,
			},
			&token,
		)
		.unwrap();
		let checks = AtomicUsize::new(0);
		let snapshot = scene.layout(&|| {
			// Cancellation starts after layout enters the container.
			if checks.fetch_add(1, Ordering::Relaxed) == 8 {
				token.cancel();
			}
			token.is_cancelled()
		});
		assert!(
			snapshot.is_none(),
			"picker layout must observe cancellation inside its first block; checked {} times",
			checks.load(Ordering::Relaxed)
		);
		assert!(token.is_cancelled());
		assert!(scene.layout(&|| false).is_some());
	}
}
