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
		let snapshot = scene.layout(
			&|| {
				// Cancellation starts after layout enters the container.
				if checks.fetch_add(1, Ordering::Relaxed) == 8 {
					token.cancel();
				}
				token.is_cancelled()
			},
			|_| {},
		);
		assert!(
			snapshot.is_none(),
			"picker layout must observe cancellation inside its first block; checked {} times",
			checks.load(Ordering::Relaxed)
		);
		assert!(token.is_cancelled());
		assert!(scene.layout(&|| false, |_| {}).is_some());
	}
}

thread_local! {
	pub(super) static LAYOUT_BLOCKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[test]
fn text_only_picker_measures_each_paragraph_once() {
	let dir = crate::test_support::TestDir::new();
	let path = dir.path().join("text.md");
	std::fs::write(
		&path,
		(0..1000)
			.map(|i| format!("Paragraph {i}.\n\n"))
			.collect::<String>(),
	)
	.unwrap();
	let cancel = CancellationToken::new();
	let services = crate::services::Services::new(1);
	let mut scene = Scene::new(
		Content {
			security: Default::default(),
			workspace: None,
			offline: true,
			document: None,
			stamp: crate::watch::file_stamp(&path),
			destination: Destination {
				path: Some(path),
				anchor: None,
			},
			options: crate::layout::LayoutOptions {
				preview: true,
				..crate::test_support::options()
			},
			images: Default::default(),
			fit: false,
		},
		&cancel,
	)
	.unwrap();
	LAYOUT_BLOCKS.set(0);
	let (snapshot, _) = scene
		.refresh(services.handle.clone(), Arc::new(|| {}), &cancel, |_| {})
		.unwrap();
	assert_eq!(snapshot.blocks.len(), 1000);
	assert!(snapshot.images.entries.is_empty());
	assert_eq!(LAYOUT_BLOCKS.get(), 1000);
}

#[test]
fn first_picker_publication_covers_the_viewport_before_full_layout() {
	let dir = crate::test_support::TestDir::new();
	let path = dir.path().join("large.md");
	let text = (0..10000)
		.map(|i| format!("Paragraph {i}.\n\n"))
		.collect::<String>();
	std::fs::write(&path, &text).unwrap();
	let options = crate::layout::LayoutOptions {
		preview: true,
		viewport_height: 300.0,
		..crate::test_support::options()
	};
	let parsed = markview_commonmark::parse(text);
	let baseline = crate::layout::LayoutEngine::new()
		.layout_prefix_cancellable(
			&parsed.document,
			&options,
			&Default::default(),
			options.viewport_height,
			|| false,
		)
		.unwrap();
	assert!(baseline.height >= 300.0 && baseline.blocks.len() < 10000);
	let services = crate::services::Services::new(1);
	let mut worker = Worker::default();
	let first_wake = Arc::new(std::sync::atomic::AtomicBool::new(true));
	let gate = Arc::new(std::sync::Barrier::new(2));
	let publisher = gate.clone();
	worker.submit_content(
		&services.handle,
		1,
		Content {
			security: Default::default(),
			workspace: None,
			offline: true,
			document: None,
			stamp: crate::watch::file_stamp(&path),
			destination: Destination {
				path: Some(path),
				anchor: None,
			},
			options,
			images: Default::default(),
			fit: false,
		},
		move || {
			if first_wake.swap(false, Ordering::Relaxed) {
				publisher.wait();
			}
		},
	);
	let first = worker
		.wait(Duration::from_secs(60))
		.unwrap()
		.snapshot
		.unwrap();
	gate.wait();
	assert!(first.height >= 300.0);
	assert_eq!(first.blocks.len(), baseline.blocks.len());
	let full = loop {
		let result = worker.wait(Duration::from_secs(60)).unwrap();
		if !result.partial {
			break result.snapshot.unwrap();
		}
	};
	assert_eq!(full.blocks.len(), 10000);
}
