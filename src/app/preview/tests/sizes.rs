use super::*;

#[test]
fn preview_sizes_keep_small_and_split_the_previous_medium_limit() {
	assert_eq!(PreviewSize::Off.dimensions(), None);
	assert_eq!(PreviewSize::Small.dimensions(), Some((420.0, 320.0)));
	assert_eq!(PreviewSize::Medium.dimensions(), Some((525.0, 400.0)));
	assert_eq!(PreviewSize::Large.dimensions(), Some((630.0, 480.0)));
}

#[test]
fn each_hover_source_obeys_its_size_and_off_never_starts_work() {
	let dir = crate::test_support::TestDir::new();
	std::fs::write(
		dir.path().join("target.md"),
		"Preview content. ".repeat(1000),
	)
	.unwrap();
	for kind in [
		PreviewKind::Footnote,
		PreviewKind::Folder,
		PreviewKind::Link,
	] {
		let mut app = match kind {
			PreviewKind::Footnote => {
				let definition =
					format!("[^note]: {}", "Preview content. ".repeat(1000));
				golden::reader_app(
					&definition,
					Theme::Light,
					&Default::default(),
				)
			}
			PreviewKind::Folder => folder::sidebar(dir.path(), Theme::Light),
			PreviewKind::Link => links::file_reader(dir.path(), "target.md").0,
		};
		for other in [
			PreviewKind::Footnote,
			PreviewKind::Folder,
			PreviewKind::Link,
		] {
			app.preferences
				.values
				.set_preview_size(other, PreviewSize::Off);
		}
		match kind {
			PreviewKind::Footnote => {
				let point = interaction_tests::reference_point(&app);
				app.interaction.pointer.move_to((point.x, point.y));
			}
			PreviewKind::Folder => {
				folder::point(&mut app, "target.md");
			}
			PreviewKind::Link => {
				links::hover(&mut app, "target.md");
			}
		}
		let cursor = app.interaction.pointer.cursor();
		let mut previous = (0.0, 0.0);
		for size in [
			PreviewSize::Off,
			PreviewSize::Small,
			PreviewSize::Medium,
			PreviewSize::Large,
		] {
			app.preview.dismiss();
			app.interaction.pointer.move_to(cursor);
			app.preferences.values.set_preview_size(kind, size);
			let now = Instant::now();
			app.hover_preview(now);
			if size == PreviewSize::Off {
				app.preview_tick(now + SHOW_DELAY);
				assert!(!app.preview.tracking());
				assert!(app.preview.pending.is_none());
				assert!(app.preview.deadline().is_none());
				let target = app.target_at_cursor().unwrap();
				assert!(app.link_preview_source(target).is_none());
				continue;
			}
			finish_footnote(&mut app, now);
			let source = &app.preview.pending.as_ref().unwrap().source;
			assert!(source.matches(&app));
			assert_eq!(source.size, size);
			let (width, height) = size.dimensions().unwrap();
			assert_eq!(source.options.width, width);
			assert_eq!(source.frame.max_height, height);
			let floating = app.preview.floating().unwrap();
			let body = floating.body();
			assert!(body.w <= width && body.h <= height);
			assert!(
				body.w > previous.0 && body.h > previous.1,
				"{kind:?} {size:?}: {body:?}"
			);
			previous = (body.w, body.h);
			app.preferences
				.values
				.set_preview_size(kind, PreviewSize::Off);
			assert!(!source.matches(&app));
			app.hover_preview(now + SHOW_DELAY);
			assert!(!app.preview.tracking());
			assert!(app.preview.pending.is_none());
		}
	}
}
