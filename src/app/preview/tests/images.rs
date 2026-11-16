use super::*;
use markview_layout::image::{ImageSnapshot, ImageSource, ResourceRef};

fn settle_images(app: &mut App<Proxy>, now: Instant) -> ImageSnapshot {
	settle_preview(app, now, |floating| {
		floating.snapshot.images.entries.values().all(|info| {
			!info.loading && (info.size.is_some() || info.error.is_some())
		})
	});
	app.preview.floating().unwrap().snapshot.images.clone()
}

#[test]
fn file_previews_load_relative_bitmaps_and_vectors_and_refresh_changed_images()
{
	let dir = crate::test_support::TestDir::new();
	let sub = dir.path().join("sub");
	std::fs::create_dir(&sub).unwrap();
	let bitmap = sub.join("picture.png");
	image::RgbaImage::from_pixel(31, 17, image::Rgba([12, 34, 56, 255]))
		.save(&bitmap)
		.unwrap();
	std::fs::write(sub.join("shape.svg"), r#"<svg xmlns="http://www.w3.org/2000/svg" width="42" height="23"><rect width="42" height="23" fill="red"/></svg>"#).unwrap();
	std::fs::write(sub.join("other.md"), "# Target\n\n![Bitmap](picture.png)\n\n![Vector](shape.svg)\n\n# Excluded\n\n![Excluded](missing.png)\n").unwrap();
	let url = "sub/other.md#target";
	let (mut app, _) = links::file_reader(dir.path(), url);
	let original = app.readers.session.path.clone();
	let now = links::hover(&mut app, url);
	finish_footnote(&mut app, now);
	let token = app.preview.worker.token().unwrap();
	assert!(!token.is_cancelled());
	let images = settle_images(&mut app, now);
	assert_eq!(images.decoded().len(), 2);
	assert_eq!(images.entries.len(), 2, "only previewed blocks load images");
	let key = images
		.key(&ImageSource::Resource(ResourceRef::document("picture.png")))
		.unwrap();
	assert_eq!(images.entries[key].size, Some((31, 17)));
	assert_eq!(
		&*images.decoded()[key].rgba,
		&[12, 34, 56, 255].repeat(31 * 17)
	);
	assert_eq!(app.readers.session.path, original);
	assert!(
		app.readers
			.session
			.display
			.snapshot
			.images
			.entries
			.is_empty()
	);
	image::RgbaImage::from_pixel(61, 29, image::Rgba([56, 34, 12, 255]))
		.save(&bitmap)
		.unwrap();
	let deadline = Instant::now() + Duration::from_secs(10);
	loop {
		let completion = app
			.preview
			.worker
			.wait(deadline.saturating_duration_since(Instant::now()))
			.expect("changed image did not refresh");
		app.preview.accept(completion);
		app.preview_tick(now + SHOW_DELAY);
		let snapshot = &app.preview.floating().unwrap().snapshot.images;
		if snapshot.entries[key].size == Some((61, 29))
			&& snapshot.decoded().contains_key(key)
		{
			break;
		}
	}
	app.dismiss_preview();
	assert!(token.is_cancelled());
	assert!(app.preview.worker.try_recv().is_none());
}

#[test]
fn file_preview_images_respect_resource_grants_and_report_missing_sources() {
	let dir = crate::test_support::TestDir::new();
	let path = dir.path().join("other.md");
	std::fs::write(
		&path,
		"![Private](private.png)\n\n![Missing](missing.png)\n",
	)
	.unwrap();
	image::RgbaImage::from_pixel(9, 7, image::Rgba([12, 34, 56, 255]))
		.save(dir.path().join("private.png"))
		.unwrap();
	let (mut app, _) = links::file_reader(dir.path(), "other.md");
	app.readers.session.security =
		crate::security::Security::local(crate::security::Trust::Untrusted);
	app.readers
		.session
		.security
		.grant(crate::security::Resource::File(
			path.canonicalize().unwrap(),
		));
	let now = links::hover(&mut app, "other.md");
	finish_footnote(&mut app, now);
	let images = settle_images(&mut app, now);
	assert!(images.decoded().is_empty());
	assert!(images.entries.values().all(|info| info.error.is_some()));
	app.readers.session.security =
		crate::security::Security::local(crate::security::Trust::Trusted);
	let images = settle_images(&mut app, now);
	assert_eq!(images.decoded().len(), 1);
	assert_eq!(
		images
			.entries
			.values()
			.filter(|info| info.error.is_some())
			.count(),
		1
	);
}

#[test]
fn an_open_footnote_refreshes_when_the_readers_images_finish_loading() {
	let images =
		golden::images(std::path::Path::new(env!("CARGO_MANIFEST_DIR")))
			.unwrap();
	let mut pending = images.clone();
	pending.pixels = Default::default();
	for info in pending.entries.values_mut() {
		info.size = None;
		info.loading = true;
	}
	let mut app = golden::reader_app(
		"[^note]: ![Picture](tiles.png)",
		Theme::Light,
		&pending,
	);
	let point = interaction_tests::reference_point(&app);
	app.interaction.pointer.move_to((point.x, point.y));
	let now = Instant::now();
	app.hover_preview(now);
	finish_footnote(&mut app, now);
	assert!(
		app.preview
			.floating()
			.unwrap()
			.snapshot
			.images
			.decoded()
			.is_empty()
	);
	let document = app.readers.session.document.as_ref().unwrap();
	app.readers.session.display.snapshot = crate::layout::LayoutEngine::new()
		.layout_with_images(document, &app.options(), &images);
	app.preview_tick(now + SHOW_DELAY);
	let completion = app.preview.worker.wait(Duration::from_secs(10)).unwrap();
	app.preview.accept(completion);
	app.preview_tick(now + SHOW_DELAY);
	assert_eq!(
		app.preview
			.floating()
			.unwrap()
			.snapshot
			.images
			.decoded()
			.len(),
		1
	);
}

#[test]
fn clicking_a_file_preview_image_keeps_its_pixels_available_to_the_viewer() {
	for existing in [false, true] {
		let dir = crate::test_support::TestDir::new();
		let sub = dir.path().join("sub");
		std::fs::create_dir(&sub).unwrap();
		let target = sub.join("other.md");
		std::fs::write(&target, "![Picture](picture.png)\n").unwrap();
		image::RgbaImage::from_pixel(31, 17, image::Rgba([12, 34, 56, 255]))
			.save(sub.join("picture.png"))
			.unwrap();
		image::RgbaImage::from_pixel(7, 5, image::Rgba([200, 100, 50, 255]))
			.save(dir.path().join("picture.png"))
			.unwrap();
		let source_path = dir.path().join("source.md");
		std::fs::write(
			&source_path,
			"[Preview](sub/other.md)\n\n![Current](picture.png)\n",
		)
		.unwrap();
		let (mut app, events) = app();
		if existing {
			load(&mut app, &events, target.clone());
		}
		load(&mut app, &events, source_path);
		let now = links::hover(&mut app, "sub/other.md");
		finish_footnote(&mut app, now);
		settle_images(&mut app, now);
		let floating = app.preview.floating().unwrap();
		let rect = floating
			.snapshot
			.blocks
			.iter()
			.find_map(|block| {
				block.draws().find_map(|(_, draw, _)| match draw {
					Draw::Image { mut rect, .. } => {
						rect.y += block.y;
						Some(floating.view().window_rect(
							floating.view().context.viewport.window_rect(rect),
						))
					}
					_ => None,
				})
			})
			.unwrap();
		app.interaction
			.pointer
			.move_to((rect.x + rect.w / 2.0, rect.y + rect.h / 2.0));
		app.pointer_pressed();
		app.pointer_released();
		settle(&mut app, &events);
		app.apply_anchor();
		let viewer = app
			.interaction
			.overlays
			.viewer()
			.expect("the clicked image opens");
		let source = viewer.src().to_owned();
		assert_eq!(
			app.readers.session.path,
			Some(target.canonicalize().unwrap())
		);
		assert!(
			app.readers
				.session
				.display
				.snapshot
				.images
				.entries
				.contains_key(&source),
			"the viewer retains its image source after dismissal of the preview"
		);
		let deadline = Instant::now() + Duration::from_secs(10);
		let pixels = loop {
			let images = &app.readers.session.display.snapshot.images;
			let info = &images.entries[&source];
			if let Some(pixels) = images.pixels.get(&source, info.version) {
				break pixels;
			}
			let event = events
				.recv_timeout(
					deadline.saturating_duration_since(Instant::now()),
				)
				.expect("the viewer's image did not finish loading");
			accept_ready(&mut app, event);
		};
		assert_eq!(&*pixels.rgba, &[12, 34, 56, 255].repeat(31 * 17));
		assert!(app.overlay().iter().any(|draw| matches!(draw, Draw::Image { src, version, .. } if *src == source && *version > 0)));
	}
}

#[test]
fn an_overflow_gesture_cannot_reuse_a_block_removed_by_image_reflow() {
	use crate::app::gestures::Inertia;
	use markview_layout::image::ImageInfo;
	use markview_platform::event::TouchPhase;
	let dir = crate::test_support::TestDir::new();
	let text = format!(
		"# Target\n\n![Tall](tall.png)\n\n```text\n{}\n```\n",
		"wide code ".repeat(150)
	);
	std::fs::write(dir.path().join("other.md"), &text).unwrap();
	let (mut app, _) = links::file_reader(dir.path(), "other.md#target");
	let now = links::hover(&mut app, "other.md#target");
	finish_footnote(&mut app, now);
	app.preview.worker.cancel();
	let source = app.preview.pending.as_ref().unwrap().source.clone();
	let document = markview_commonmark::parse(text);
	let mut images = ImageSnapshot::default();
	images.keys.insert(
		ImageSource::Resource(ResourceRef::document("tall.png")),
		"tall.png".into(),
	);
	images
		.entries
		.insert("tall.png".into(), ImageInfo::default());
	let layout = |images: &ImageSnapshot| {
		layout_scene_cancellable(
			&document,
			&document.blocks()[0],
			&source.options,
			images,
			true,
			&|| false,
		)
		.unwrap()
	};
	let before = layout(&images);
	images.entries.insert(
		"tall.png".into(),
		ImageInfo {
			size: Some((16, 4000)),
			version: 1,
			..Default::default()
		},
	);
	let after = layout(&images);
	assert_eq!(before.presentation_key, after.presentation_key);
	let (block_index, block) = before
		.blocks
		.iter()
		.enumerate()
		.find(|(_, block)| !block.layout.overflow.is_empty())
		.unwrap();
	assert!(
		block_index >= after.blocks.len(),
		"loading the image removes the code block from the excerpt"
	);
	let mut rect = block.layout.overflow[0].rect;
	rect.y += block.y;
	app.preview.show_scene(
		source.target,
		source.anchor,
		source.bounds,
		before,
		source.revision,
		source.frame,
	);
	let floating = app.preview.floating_mut().unwrap();
	floating
		.scroll
		.jump_to(rect.y.min(floating.view().max_scroll()));
	floating.state.focused = true;
	let point = floating
		.view()
		.window_rect(floating.view().context.viewport.window_rect(rect));
	app.interaction
		.pointer
		.move_to((point.x + 5.0, point.y + 5.0));
	app.trackpad_scroll(-20.0, 0.0, TouchPhase::Started, Inertia::Native);
	app.trackpad_scroll(-20.0, 0.0, TouchPhase::Moved, Inertia::Native);
	assert!(
		app.preview.floating().unwrap().horizontal[&(block_index, 0)] > 0.0
	);
	app.preview.accept(work::Completion {
		serial: app.preview.serial,
		snapshot: Some(after),
		origins: navigation::Origins::default(),
		build: Duration::ZERO,
	});
	app.preview_tick(now + SHOW_DELAY);
	let page = app.readers.session.scrolling.offset;
	app.trackpad_scroll(-20.0, 0.0, TouchPhase::Moved, Inertia::Native);
	app.trackpad_scroll(0.0, 0.0, TouchPhase::Ended, Inertia::Native);
	assert_eq!(app.readers.session.scrolling.offset, page);
}

#[test]
fn reader_and_file_preview_keep_both_image_textures_across_frames() {
	use crate::render::{Preview as RenderPreview, Renderer, View};
	use markview_layout::scene::{BlockLayout, PlacedBlock};
	use std::sync::Arc;
	let dir = crate::test_support::TestDir::new();
	let sub = dir.path().join("sub");
	std::fs::create_dir(&sub).unwrap();
	for (path, color) in [
		(dir.path().join("picture.png"), [200, 100, 50, 255]),
		(sub.join("picture.png"), [12, 34, 56, 255]),
	] {
		image::RgbaImage::from_pixel(64, 64, image::Rgba(color))
			.save(path)
			.unwrap();
	}
	std::fs::write(sub.join("other.md"), "![Preview](picture.png)\n").unwrap();
	let path = dir.path().join("source.md");
	std::fs::write(
		&path,
		"[Preview](sub/other.md)\n\n![Reader](picture.png)\n",
	)
	.unwrap();
	let (mut app, events) = app();
	load(&mut app, &events, path);
	let deadline = Instant::now() + Duration::from_secs(10);
	while app
		.readers
		.session
		.display
		.snapshot
		.images
		.decoded()
		.is_empty()
	{
		let event = events
			.recv_timeout(deadline.saturating_duration_since(Instant::now()))
			.expect("the reader image did not load");
		accept_ready(&mut app, event);
	}
	let now = links::hover(&mut app, "sub/other.md");
	finish_footnote(&mut app, now);
	let guest_images = settle_images(&mut app, now);
	let image_scene = |images: &ImageSnapshot| {
		let (source, info) = images.entries.iter().next().unwrap();
		LayoutSnapshot {
			images: images.clone(),
			width: 64.0,
			height: 64.0,
			blocks: vec![PlacedBlock {
				id: markview_document::BlockId::new(1).unwrap(),
				content: Default::default(),
				y: 0.0,
				flow: Default::default(),
				layout: Arc::new(BlockLayout {
					width: 64.0,
					height: 64.0,
					draws: vec![Draw::Image {
						src: source.clone(),
						version: info.version,
						rect: Rect {
							x: 0.0,
							y: 0.0,
							w: 64.0,
							h: 64.0,
						},
						title: String::new(),
					}]
					.into(),
					..Default::default()
				}),
			}],
			..Default::default()
		}
	};
	let main = image_scene(&app.readers.session.display.snapshot.images);
	let guest = image_scene(&guest_images);
	let horizontal = HashMap::new();
	let view = View {
		width: 128,
		height: 128,
		scale: 1.0,
		left: 0.0,
		top: 0.0,
		bottom: 0.0,
		scroll: 0.0,
		theme: Theme::Light,
		horizontal: &horizontal,
		selection: None,
		revision: 0,
		hovered_link: None,
		held_overflow: None,
	};
	let preview = RenderPreview {
		snapshot: &guest,
		rect: Rect {
			x: 64.0,
			y: 0.0,
			w: 64.0,
			h: 64.0,
		},
		scroll: 0.0,
		theme: Theme::Light,
		horizontal: &horizontal,
		interaction: None,
	};
	let mut renderer = pollster::block_on(Renderer::new(None)).unwrap();
	let target = renderer.offscreen(128, 128);
	let target_view = target.create_view(&Default::default());
	let submission = renderer
		.render_document_scenes(&main, &view, &[], &[], &target_view)
		.unwrap();
	renderer.wait(Some(submission)).unwrap();
	let main_bytes = renderer.gpu_bytes();
	let guest_bytes: u64 = guest
		.images
		.decoded()
		.values()
		.map(|pixels| pixels.rgba.len() as u64)
		.sum();
	for _ in 0..3 {
		let submission = renderer
			.render_document_scenes(
				&main,
				&view,
				&[],
				std::slice::from_ref(&preview),
				&target_view,
			)
			.unwrap();
		renderer.wait(Some(submission)).unwrap();
		assert_eq!(
			renderer.gpu_bytes(),
			main_bytes + guest_bytes,
			"both image textures must remain cached after composing the preview"
		);
	}
}

#[test]
fn clicking_an_image_beyond_the_document_remote_cap_opens_only_that_image() {
	use tokio::io::{AsyncReadExt, AsyncWriteExt};
	let dir = crate::test_support::TestDir::new();
	let png = dir.path().join("fixture.png");
	image::RgbaImage::from_pixel(31, 17, image::Rgba([12, 34, 56, 255]))
		.save(&png)
		.unwrap();
	let bytes = std::fs::read(png).unwrap();
	let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
	listener.set_nonblocking(true).unwrap();
	let url = format!("http://{}/chosen.png", listener.local_addr().unwrap());
	let mut text = String::new();
	for index in 0..128 {
		text.push_str(&format!("![Earlier](http://192.0.2.1/{index}.png)\n\n"));
	}
	text.push_str(&format!(
		"# Target\n\n![Chosen]({url})\n\n![Deferred](http://192.0.2.1/deferred.png)\n"
	));
	let target = dir.path().join("other.md");
	std::fs::write(&target, text).unwrap();
	let source = dir.path().join("source.md");
	std::fs::write(&source, "[Preview](other.md#target)\n").unwrap();
	let (mut app, events) = app_with_offline(false);
	let cancelled = app.services.handle.cancel.clone();
	app.services.handle.submit(async move {
		let listener = tokio::net::TcpListener::from_std(listener).unwrap();
		loop {
			let (mut socket, _) = tokio::select! {
				_ = cancelled.cancelled() => return,
				connection = listener.accept() => connection.unwrap(),
			};
			let mut request = [0; 4096];
			if socket.read(&mut request).await.unwrap_or(0) == 0 { continue; }
			let header = format!("HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nCache-Control: max-age=3600\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", bytes.len());
			let _ = socket.write_all(header.as_bytes()).await;
			let _ = socket.write_all(&bytes).await;
		}
	});
	load(&mut app, &events, source);
	let now = links::hover(&mut app, "other.md#target");
	finish_footnote(&mut app, now);
	let images = settle_images(&mut app, now);
	assert!(
		images.decoded().contains_key(&url),
		"the preview has loaded the clicked image"
	);
	let floating = app.preview.floating().unwrap();
	let rect = floating
		.snapshot
		.blocks
		.iter()
		.find_map(|block| {
			block.draws().find_map(|(_, draw, _)| match draw {
				Draw::Image { src, mut rect, .. } if src == url => {
					rect.y += block.y;
					Some(floating.view().window_rect(
						floating.view().context.viewport.window_rect(rect),
					))
				}
				_ => None,
			})
		})
		.unwrap();
	app.interaction
		.pointer
		.move_to((rect.x + rect.w / 2.0, rect.y + rect.h / 2.0));
	app.pointer_pressed();
	app.pointer_released();
	settle(&mut app, &events);
	app.apply_anchor();
	assert_eq!(
		app.interaction.overlays.viewer().map(|viewer| viewer.src()),
		Some(url.as_str()),
		"a displayed image remains viewable after transferring to its document"
	);
	assert!(!app.readers.session.image_loading.all);
	assert_eq!(
		app.readers.session.remote_deferred, 1,
		"other sources beyond the cap remain deferred"
	);
}
