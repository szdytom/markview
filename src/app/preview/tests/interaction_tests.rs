use super::*;
use markview_interaction::{Point, ViewInput};
use markview_layout::{image::ImageInfo, scene::LinkTarget};
use winit::keyboard::{Key, ModifiersState, NamedKey};

fn note_app(source: &str) -> App<Proxy> {
	let (mut app, _events) = app();
	let reader = picture_reader(
		source,
		760.0,
		ImageInfo {
			size: Some((64, 64)),
			..Default::default()
		},
	);
	app.readers.session.accept(reader, app.viewport(), None);
	hover_note(&mut app);
	app
}

pub(super) fn reference_point(app: &App<Proxy>) -> Point {
	let anchor = app
		.readers
		.session
		.display
		.snapshot
		.blocks
		.iter()
		.find_map(|block| {
			block.links().find_map(|(link, mut rect)| {
				if !matches!(link.target, LinkTarget::FootnoteDefinition(_)) {
					return None;
				}
				rect.y += block.y;
				Some(app.view_geometry().window_rect(rect))
			})
		})
		.unwrap();
	Point::new(anchor.x + anchor.w / 2.0, anchor.y + anchor.h / 2.0)
}

pub(super) fn hover_note(app: &mut App<Proxy>) {
	let point = reference_point(app);
	app.interaction.pointer.move_to((point.x, point.y));
	let now = Instant::now();
	app.hover_preview(now);
	finish_footnote(app, now);
	assert!(app.preview.floating().is_some());
}

fn click(app: &mut App<Proxy>, point: Point) {
	app.interaction.pointer.move_to((point.x, point.y));
	app.pointer_pressed();
	app.pointer_released();
}
fn content_point(app: &App<Proxy>) -> Point {
	let body = app.preview.floating().unwrap().body();
	Point::new(body.x + body.w / 2.0, body.y + 5.0)
}

#[test]
fn focused_popup_select_all_copies_only_the_note_and_escape_closes_it() {
	let mut app = note_app("Main prose[^n].\n\n[^n]: Note **content**.\n");
	let main_selection = app
		.readers
		.session
		.display
		.snapshot
		.select_all(app.readers.session.accepted_revision);
	app.interaction.selection = main_selection;
	let point = content_point(&app);
	click(&mut app, point);
	app.interaction
		.pointer
		.set_modifiers(ModifiersState::CONTROL);
	assert!(app.preview_key(&Key::Character("a".into())));
	assert_eq!(app.selected_text().as_deref(), Some("Note content."));
	assert_eq!(app.interaction.selection, main_selection);
	app.interaction.pointer.move_to((700.0, 400.0));
	assert!(!app.hover_preview(Instant::now()));
	assert!(app.preview.open().is_some());
	assert!(app.preview_key(&Key::Named(NamedKey::Escape)));
	assert!(app.preview.open().is_none());
	assert!(app.selected_text().unwrap().contains("Main prose"));
}

#[test]
fn outside_click_closes_popup_and_unfocused_hover_still_dismisses() {
	let mut app = note_app("Main[^n].\n\n[^n]: Short note.\n");
	app.interaction.pointer.move_to((700.0, 400.0));
	assert!(app.hover_preview(Instant::now()));
	assert!(app.preview.open().is_none());
	let mut app = note_app("Main[^n].\n\n[^n]: Short note.\n");
	let point = content_point(&app);
	click(&mut app, point);
	click(&mut app, Point::new(700.0, 400.0));
	assert!(app.preview.open().is_none());
}

#[test]
fn scrollbar_drag_and_keyboard_scrolling_leave_the_main_page_in_place() {
	let source = format!(
		"Main[^n].\n\n[^n]: {}\n",
		"A long note with words to wrap. ".repeat(200)
	);
	let mut app = note_app(&source);
	let floating = app.preview.floating().unwrap();
	let bar = floating.scrollbar().unwrap();
	assert_eq!(bar.track.y, floating.rect.y);
	assert_eq!(bar.track.h, floating.rect.h);
	assert_eq!(bar.track.x + bar.track.w, floating.rect.x + floating.rect.w);
	let page = app.readers.session.scrolling.offset;
	let point = Point::new(bar.thumb.x + bar.thumb.w / 2.0, bar.thumb.y + 2.0);
	app.interaction.pointer.move_to((point.x, point.y));
	app.pointer_pressed();
	assert!(app.preview_captured());
	app.preview_input(ViewInput::Move(Point::new(900.0, 900.0)));
	assert!(app.preview.floating().unwrap().scroll > 100.0);
	app.interaction.pointer.move_to((900.0, 900.0));
	app.pointer_released();
	assert!(!app.preview_captured());
	assert!(app.preview_key(&Key::Named(NamedKey::Home)));
	assert_eq!(app.preview.floating().unwrap().scroll, 0.0);
	assert!(app.preview_key(&Key::Named(NamedKey::End)));
	assert!(app.preview.floating().unwrap().scroll > 100.0);
	assert_eq!(app.readers.session.scrolling.offset, page);
	assert!(app.interaction.selection.is_none());
}

#[test]
fn popup_images_use_the_existing_viewer_and_original_resource_identity() {
	let mut app = note_app("Main[^n].\n\n[^n]: ![Picture](picture.png)\n");
	let floating = app.preview.floating().unwrap();
	let (source, mut rect) = floating
		.snapshot
		.blocks
		.iter()
		.find_map(|block| {
			block.draws().find_map(|(_, draw, _)| match draw {
				Draw::Image { src, rect, .. } => {
					let mut rect = rect;
					rect.y += block.y;
					Some((src.to_owned(), rect))
				}
				_ => None,
			})
		})
		.unwrap();
	rect = floating.view().context.viewport.window_rect(rect);
	let body = floating.body();
	click(
		&mut app,
		Point::new(
			body.x + rect.x + rect.w / 2.0,
			body.y + rect.y + rect.h / 2.0,
		),
	);
	assert!(app.preview.open().is_none());
	assert_eq!(app.interaction.overlays.viewer().unwrap().src(), source);
}

#[test]
fn footnote_links_navigate_in_the_source_document_without_nested_previews() {
	let mut app = note_app(
		"Main[^one].\n\n[^one]: First note refers to [^two].\n\n[^two]: Second note.\n",
	);
	let floating = app.preview.floating().unwrap();
	let link = floating
		.snapshot
		.blocks
		.iter()
		.find_map(|block| {
			block.links().find_map(|(link, mut rect)| {
				if !matches!(link.target, LinkTarget::FootnoteDefinition(_)) {
					return None;
				}
				rect.y += block.y;
				Some(rect)
			})
		})
		.unwrap();
	let body = floating.body();
	let point = Point::new(
		body.x + link.x + link.w / 2.0,
		body.y + link.y + link.h / 2.0,
	);
	let original_target = app.preview.open().unwrap().target;
	app.interaction.pointer.move_to((point.x, point.y));
	app.preview_input(ViewInput::Move(point));
	app.hover_preview(Instant::now());
	app.preview_tick(Instant::now() + SHOW_DELAY);
	assert_eq!(app.preview.open().unwrap().target, original_target);
	click(&mut app, point);
	assert!(app.preview.open().is_none());
	assert!(matches!(
		app.readers
			.session
			.pending_anchor
			.as_ref()
			.map(|p| &p.target),
		Some(markview_document::AnchorId::FootnoteDefinition(_))
	));
}

#[test]
fn horizontal_input_over_popup_cannot_pan_the_page_below_it() {
	let source = format!(
		"Main[^n].\n\n[^n]: Code:\n\n    ```text\n    {}\n    ```\n",
		"wide ".repeat(150)
	);
	let mut app = note_app(&source);
	let floating = app.preview.floating().unwrap();
	let (block, overflow, rect) = floating
		.snapshot
		.blocks
		.iter()
		.enumerate()
		.find_map(|(index, block)| {
			block.layout.overflow.iter().enumerate().next().map(
				|(overflow, _)| {
					let mut rect = block.overflow_rect(overflow).unwrap();
					rect.y += block.y;
					(index, overflow, rect)
				},
			)
		})
		.unwrap();
	let body = floating.body();
	app.interaction
		.pointer
		.move_to((body.x + rect.x + 5.0, body.y + rect.y + 5.0));
	assert!(app.horizontal_by(100.0));
	assert_eq!(
		app.preview
			.floating()
			.unwrap()
			.horizontal
			.get(&(block, overflow)),
		Some(&100.0)
	);
	assert!(app.readers.session.display.horizontal.is_empty());
}

#[test]
fn interactive_popup_renders_selection_and_a_visible_scrollbar()
-> anyhow::Result<()> {
	let source = format!(
		"# Hover previews\n\nRead the supporting note[^n].\n\n{}\n\n[^n]: A **formatted** note with a [link](https://example.test).\n\n    - Select and copy text inside this popup.\n    - Scroll to read the rest of the note.\n\n    {}\n",
		"The page keeps its place while you read the note.\n\n".repeat(20),
		"Further explanation stays inside the floating document. ".repeat(40)
	);
	let (mut app, events) = app();
	let directory = tempfile::tempdir()?;
	let path = directory.path().join("hover-previews.md");
	std::fs::write(&path, source)?;
	load(&mut app, &events, path);
	hover_note(&mut app);
	let point = content_point(&app);
	click(&mut app, point);
	let floating = app
		.preview
		.open
		.as_mut()
		.unwrap()
		.floating
		.as_mut()
		.unwrap();
	floating.input(ViewInput::SelectAll);
	assert!(floating.scrollbar().is_some());
	let overlay = app.overlay();
	let scene = app
		.preview
		.scene(&app.readers, crate::render::Theme::Light)
		.unwrap();
	assert!(scene.interaction.unwrap().selection.is_some());
	let geometry = app.view_geometry();
	let frame = crate::render::View {
		width: geometry.width as u32,
		height: geometry.height as u32,
		scale: 1.0,
		left: geometry.left,
		top: geometry.top,
		bottom: geometry.bottom,
		scroll: geometry.scroll,
		theme: crate::render::Theme::Light,
		horizontal: &app.readers.session.display.horizontal,
		selection: None,
		revision: 0,
		hovered_link: None,
		held_overflow: None,
	};
	let mut renderer = pollster::block_on(crate::render::Renderer::new(None))?;
	renderer.set_stylesheet(app.options().stylesheet);
	let texture = renderer.offscreen(frame.width, frame.height);
	let target = texture.create_view(&Default::default());
	renderer.render_document_scenes(
		&app.readers.session.display.snapshot,
		&frame,
		&overlay,
		std::slice::from_ref(&scene),
		&target,
	)?;
	let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("artifacts/preview");
	std::fs::create_dir_all(&directory)?;
	renderer.save_png(
		&texture,
		&directory.join("footnote-hover-interactive.png"),
	)?;
	let selected = renderer.read_pixels(&texture)?;
	let bar = app.preview.floating().unwrap().scrollbar().unwrap();
	let (_, thumb) = bar.bars();
	let pixel =
		((thumb.y as u32 + 3) * frame.width + thumb.x as u32 + 2) as usize * 4;
	let surface = app.preview.floating().unwrap().rect;
	let background =
		((surface.y as u32 + 2) * frame.width + surface.x as u32 + 2) as usize
			* 4;
	assert_ne!(
		&selected.rgba[pixel..pixel + 4],
		&selected.rgba[background..background + 4]
	);
	use markview_layout::style::{ColorField, Condition};
	let track_color = app
		.preferences
		.values
		.stylesheet
		.color(Condition::Scrollbar, ColorField::Track)
		.map(|c| (c * 255.0).round() as u8);
	let thumb_color = app
		.preferences
		.values
		.stylesheet
		.color(Condition::Scrollbar, ColorField::Thumb)
		.map(|c| (c * 255.0).round() as u8);
	let right = (surface.x + surface.w).floor() as u32 - 1;
	for (y, expected) in [
		(surface.y.ceil() as u32, thumb_color),
		((surface.y + surface.h).floor() as u32 - 1, track_color),
	] {
		let index = ((y * frame.width + right) * 4) as usize;
		assert_eq!(
			&selected.rgba[index..index + 4],
			&expected,
			"the track must fill the popup edge through both ends"
		);
	}
	Ok(())
}

#[test]
fn trackpad_streams_stay_on_the_popup_even_after_it_closes() {
	use crate::app::gestures::Inertia;
	use markview_platform::event::TouchPhase;
	let source =
		format!("Main[^n].\n\n[^n]: {}\n", "A long note. ".repeat(300));
	let mut app = note_app(&source);
	let point = content_point(&app);
	app.interaction.pointer.move_to((point.x, point.y));
	let page = app.readers.session.scrolling.offset;
	app.trackpad_scroll(0.0, -80.0, TouchPhase::Started, Inertia::Native);
	app.trackpad_scroll(0.0, -80.0, TouchPhase::Moved, Inertia::Native);
	assert!(app.preview.floating().unwrap().scroll >= 80.0);
	assert_eq!(app.readers.session.scrolling.offset, page);
	app.dismiss_preview();
	app.trackpad_scroll(0.0, -80.0, TouchPhase::Moved, Inertia::Native);
	app.trackpad_scroll(0.0, 0.0, TouchPhase::Ended, Inertia::Native);
	assert_eq!(app.readers.session.scrolling.offset, page);
}

#[test]
fn scrolling_refreshes_the_link_under_a_stationary_pointer() {
	let source = format!(
		"Main[^n].\n\n[^n]: [Link](https://example.test). {}\n",
		"More note text. ".repeat(300)
	);
	let mut app = note_app(&source);
	let floating = app.preview.floating().unwrap();
	let rect = floating
		.snapshot
		.blocks
		.iter()
		.find_map(|block| {
			block.links().find_map(|(link, mut rect)| {
				if !matches!(link.target, LinkTarget::Url(_)) {
					return None;
				}
				rect.y += block.y;
				Some(rect)
			})
		})
		.unwrap();
	let body = floating.body();
	let point = Point::new(
		body.x + rect.x + rect.w / 2.0,
		body.y + rect.y + rect.h / 2.0,
	);
	app.interaction.pointer.move_to((point.x, point.y));
	app.preview_input(ViewInput::Move(point));
	assert!(app.preview.floating().unwrap().state.hover.link.is_some());
	app.scroll_wheel(100.0, markview_interaction::WheelMode::Direct);
	assert!(app.preview.floating().unwrap().state.hover.link.is_none());
}
