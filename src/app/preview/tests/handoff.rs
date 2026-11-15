use super::*;
use markview_interaction::Point;
use markview_layout::scene::LinkTarget;
use std::{fs, path::Path, sync::Arc};

struct Case {
	app: App<Proxy>,
	events: mpsc::Receiver<Event>,
	dir: tempfile::TempDir,
	target: PathBuf,
}

impl Case {
	fn new(body: &str, existing: bool) -> Self {
		let dir = tempfile::tempdir().unwrap();
		let target = dir.path().join("other.md");
		fs::write(
			&target,
			format!(
				"# Before\n\n{}## Target\n\n{body}",
				"Earlier paragraph.\n\n".repeat(40)
			),
		)
		.unwrap();
		let target = target.canonicalize().unwrap();
		let source = dir.path().join("source.md");
		fs::write(&source, "[Preview](other.md#target)\n").unwrap();
		let (mut app, events) = app();
		if existing {
			load(&mut app, &events, target.clone());
		}
		load(&mut app, &events, source);
		Self {
			app,
			events,
			dir,
			target,
		}
	}
	fn show(&mut self) {
		let now = links::hover(&mut self.app, "other.md#target");
		finish_footnote(&mut self.app, now);
	}
	fn point(
		&self,
		predicate: impl Fn(&LinkTarget) -> bool,
		index: usize,
	) -> Point {
		let floating = self.app.preview.floating().unwrap();
		let rect = floating
			.snapshot
			.blocks
			.iter()
			.flat_map(|block| {
				block.links().map(move |(link, mut rect)| {
					rect.y += block.y;
					(link, rect)
				})
			})
			.filter(|(link, _)| predicate(link.target))
			.nth(index)
			.unwrap()
			.1;
		let body = floating.body();
		let point = Point::new(
			body.x + rect.x + rect.w / 2.0,
			body.y + rect.y - floating.scroll + rect.h / 2.0,
		);
		assert!(body.contains(point.x, point.y));
		point
	}
	fn click(&mut self, predicate: impl Fn(&LinkTarget) -> bool, index: usize) {
		let point = self.point(predicate, index);
		self.app.interaction.pointer.move_to((point.x, point.y));
		self.app.pointer_pressed();
		self.app.pointer_released();
	}

	fn settle(&mut self) {
		settle(&mut self.app, &self.events);
		self.app.apply_anchor();
	}
	fn update(&self, body: &str) {
		fs::write(&self.target, format!("# Before\n\n## Target\n\n{body}"))
			.unwrap();
	}
	fn until(&mut self, ready: impl Fn(&App<Proxy>) -> bool) {
		let deadline = Instant::now() + Duration::from_secs(5);
		loop {
			self.app.preview_tick(Instant::now() + SHOW_DELAY);
			if ready(&self.app) {
				return;
			}
			assert!(Instant::now() < deadline, "preview did not refresh");
			let _ = self.events.recv_timeout(Duration::from_millis(20));
		}
	}
	fn wait_for_preview(&self) {
		let deadline = Instant::now() + Duration::from_secs(3);
		while !matches!(
			self.events
				.recv_timeout(
					deadline.saturating_duration_since(Instant::now())
				)
				.expect("the preview did not wake"),
			Event::PreviewReady
		) {}
	}
}

fn disclosure(app: &App<Proxy>, title: &str) -> markview_document::BlockId {
	let document = app.readers.session.document.as_ref().unwrap();
	let mut found = None;
	for block in document.blocks() {
		markview_document::visit_blocks(block, &mut |block| {
			if let markview_document::BlockKind::Details { summary, .. } =
				block.kind()
				&& markview_document::plain_text(summary) == title
			{
				found = Some(block.id());
			}
		});
	}
	found.unwrap()
}

fn summary_y(app: &App<Proxy>, id: markview_document::BlockId) -> f32 {
	app.readers
		.session
		.display
		.snapshot
		.blocks
		.iter()
		.find_map(|block| {
			block
				.links()
				.find(|(link, _)| *link.target == LinkTarget::Disclosure(id))
				.map(|(_, rect)| block.y + rect.y)
		})
		.unwrap()
}

#[test]
fn disclosure_click_opens_or_reuses_the_target_tab_and_executes_once() {
	for existing in [false, true] {
		let mut case = Case::new(
			"<details><summary>Details</summary>\n\nBody.\n\n</details>\n",
			existing,
		);
		let count = case.app.readers.entries().len();
		case.show();
		case.click(|target| matches!(target, LinkTarget::Disclosure(_)), 0);
		assert_eq!(case.app.readers.session.path.as_ref(), Some(&case.target));
		case.settle();
		let id = disclosure(&case.app, "Details");
		assert_eq!(case.app.readers.session.details_open.get(&id), Some(&true));
		assert_eq!(
			case.app.readers.entries().len(),
			count + usize::from(!existing)
		);
		let y = summary_y(&case.app, id)
			- case.app.readers.session.scrolling.offset;
		assert!(y >= 0.0 && y < case.app.viewport());
		case.app.apply_anchor();
		assert_eq!(case.app.readers.session.details_open.get(&id), Some(&true));
	}
}

#[test]
fn an_open_disclosure_collapses_after_transferring_to_its_file() {
	let mut case = Case::new(
		"<details open><summary>Details</summary>\n\nBody.\n\n</details>\n",
		false,
	);
	case.show();
	case.click(|target| matches!(target, LinkTarget::Disclosure(_)), 0);
	case.settle();
	let id = disclosure(&case.app, "Details");
	assert_eq!(case.app.readers.session.details_open.get(&id), Some(&false));
}

#[test]
fn disclosure_transfer_reveals_ancestors_and_preserves_the_clicked_intent() {
	let mut case = Case::new(
		"<details open><summary>Outer</summary>\n\n<details><summary>Inner</summary>\n\nBody.\n\n</details>\n\n</details>\n",
		true,
	);
	case.app.select_tab(0);
	case.settle();
	let outer = disclosure(&case.app, "Outer");
	let inner = disclosure(&case.app, "Inner");
	Arc::make_mut(&mut case.app.readers.session.details_open)
		.insert(outer, false);
	Arc::make_mut(&mut case.app.readers.session.details_open)
		.insert(inner, true);
	case.app.present_disclosures();
	case.app.select_tab(1);
	case.settle();
	case.show();
	case.click(|target| *target == LinkTarget::Disclosure(inner), 0);
	case.settle();
	assert_eq!(
		case.app.readers.session.details_open.get(&outer),
		Some(&true)
	);
	assert_eq!(
		case.app.readers.session.details_open.get(&inner),
		Some(&true)
	);
}

#[test]
fn footnote_transfer_records_the_reference_that_was_actually_clicked() {
	let mut case =
		Case::new("First[^n].\n\nSecond[^n].\n\n[^n]: Note.\n", false);
	case.show();
	case.click(
		|target| matches!(target, LinkTarget::FootnoteDefinition(_)),
		1,
	);
	assert!(case.app.readers.session.pending_preview_action.is_some());
	case.settle();
	let references: Vec<_> = case
		.app
		.readers
		.session
		.display
		.snapshot
		.blocks
		.iter()
		.flat_map(|block| {
			block.anchor_positions().filter_map(|(anchor, y)| {
				matches!(
					anchor,
					markview_document::AnchorId::FootnoteReference { .. }
				)
				.then_some(block.y + y)
			})
		})
		.collect();
	let (target, origin) =
		case.app.readers.session.jump_origin.as_ref().unwrap();
	assert!(matches!(
		target,
		markview_document::AnchorId::FootnoteDefinition(_)
	));
	assert!((*origin - references[1]).abs() < 1.0);
}

#[test]
fn footnote_return_uses_a_typed_reference_in_the_target_file() {
	let mut case = Case::new("Read[^n].\n\n[^n]: Note.\n", false);
	case.show();
	case.click(|target| matches!(target, LinkTarget::FootnoteReturn(_)), 0);
	case.settle();
	assert_eq!(case.app.readers.session.path.as_ref(), Some(&case.target));
	assert!(matches!(
		case.app
			.readers
			.session
			.jump_origin
			.as_ref()
			.map(|(target, _)| target),
		Some(markview_document::AnchorId::FootnoteReference { .. })
	));
}

#[test]
fn changed_content_or_a_new_navigation_discards_a_waiting_activation() {
	for changed in [false, true] {
		let mut case = Case::new(
			"<details><summary>Details</summary>\n\nBody.\n\n</details>\n",
			false,
		);
		case.show();
		case.click(|target| matches!(target, LinkTarget::Disclosure(_)), 0);
		assert!(case.app.readers.session.pending_preview_action.is_some());
		if changed {
			let options = case.app.options();
			let mut reader = picture_reader(
				"<details><summary>Replaced</summary>\n\nOther.\n\n</details>\n",
				options.width,
				Default::default(),
			);
			reader.content_version = case.app.readers.session.content_version;
			case.app
				.readers
				.session
				.accept(reader, case.app.viewport(), None);
			case.app.apply_anchor();
			assert!(case.app.readers.session.details_open.is_empty());
		} else {
			case.app.goto_anchor("target".into());
			case.settle();
			assert!(case.app.readers.session.details_open.is_empty());
		}
		assert!(case.app.readers.session.pending_preview_action.is_none());
	}
}

#[test]
fn a_stationary_focused_file_preview_refreshes_after_a_write_and_atomic_save() {
	let mut case = Case::new("Original content.\n", false);
	case.show();
	let body = case.app.preview.floating().unwrap().body();
	case.app
		.interaction
		.pointer
		.move_to((body.x + 3.0, body.y + 3.0));
	case.app.preview.floating_mut().unwrap().state.focused = true;
	for (atomic, content) in [
		(false, "Updated content."),
		(true, "Atomically saved content."),
	] {
		if atomic {
			let replacement = case.dir.path().join("save.tmp");
			fs::write(&replacement, format!("## Target\n\n{content}\n"))
				.unwrap();
			fs::rename(replacement, &case.target).unwrap();
		} else {
			case.update(content);
		}
		case.until(|app| {
			app.preview.floating().is_some()
				&& links::text(app).contains(content)
		});
		assert!(case.app.preview.floating().unwrap().focused());
		assert!(
			case.app
				.readers
				.session
				.path
				.as_ref()
				.unwrap()
				.ends_with(Path::new("source.md"))
		);
	}
}

#[test]
fn deleting_the_target_or_its_anchor_closes_the_preview() {
	for remove in [false, true] {
		let mut case = Case::new("Original content.\n", false);
		case.show();
		if remove {
			fs::remove_file(&case.target).unwrap();
		} else {
			fs::write(&case.target, "## Different heading\n\nChanged.\n")
				.unwrap();
		}
		case.until(|app| !app.preview.tracking());
		assert!(case.app.preview.observed.is_none());
	}
}

#[test]
fn a_file_change_between_press_and_release_cannot_activate_a_stale_link() {
	let mut case = Case::new(
		"<details><summary>Details</summary>\n\nBody.\n\n</details>\n",
		false,
	);
	case.show();
	let point =
		case.point(|target| matches!(target, LinkTarget::Disclosure(_)), 0);
	case.app.interaction.pointer.move_to((point.x, point.y));
	case.app.pointer_pressed();
	case.update("Replacement content.");
	case.app.pointer_released();
	assert!(
		case.app
			.readers
			.session
			.path
			.as_ref()
			.unwrap()
			.ends_with("source.md")
	);
	assert!(case.app.readers.session.pending_preview_action.is_none());
	case.until(|app| {
		app.preview.floating().is_some()
			&& links::text(app).contains("Replacement content")
	});
}

#[test]
fn an_old_completion_cannot_replace_a_newer_file_revision() {
	let mut case = Case::new("Original content.\n", false);
	let now = links::hover(&mut case.app, "other.md#target");
	case.app.preview_tick(now + PREPARE_DELAY);
	let old = case
		.app
		.preview
		.worker
		.wait(Duration::from_secs(5))
		.unwrap();
	case.update("Newest content.");
	case.app.preview_tick(now + SHOW_DELAY);
	case.app.preview.accept(old);
	case.until(|app| {
		app.preview.floating().is_some()
			&& links::text(app).contains("Newest content")
	});
	assert!(!links::text(&case.app).contains("Original content"));
}

#[test]
fn a_file_watch_wakes_the_preview_without_pointer_motion_or_a_frame_timer() {
	let mut case = Case::new("Original content.\n", false);
	case.show();
	case.wait_for_preview();
	while case.events.try_recv().is_ok() {}
	// The shared queue may receive unrelated filesystem notifications first.
	case.app.proxy.send(Event::Changed(case.target.clone()));
	case.update("Watch-delivered content.");
	case.wait_for_preview();
	case.until(|app| {
		app.preview.floating().is_some()
			&& links::text(app).contains("Watch-delivered content")
	});
}

#[test]
fn a_refreshed_preview_reloads_an_existing_tab_before_using_new_block_ids() {
	let mut case = Case::new("Original content.\n", true);
	case.show();
	case.update(
		"<details><summary>New details</summary>\n\nUpdated body.\n\n</details>\n",
	);
	case.until(|app| {
		app.preview.floating().is_some()
			&& links::text(app).contains("New details")
	});
	let count = case.app.readers.entries().len();
	case.click(|target| matches!(target, LinkTarget::Disclosure(_)), 0);
	case.settle();
	let id = disclosure(&case.app, "New details");
	assert_eq!(case.app.readers.session.details_open.get(&id), Some(&true));
	assert_eq!(case.app.readers.entries().len(), count);
}

#[test]
fn transfers_rebase_fresh_preview_ids_onto_an_incrementally_parsed_tab() {
	let mut case = Case::new("[Next](next.md#target)\n", true);
	fs::write(case.dir.path().join("next.md"), "# Target\n").unwrap();
	case.app.select_tab(0);
	case.settle();
	let old = case.app.readers.session.document.as_ref().unwrap();
	let crate::loaded::LoadedDocument::Markdown(old) = old.as_ref() else {
		unreachable!();
	};
	let source = old.source.replacen(
		"Earlier paragraph.",
		"[Earlier](#before).\n\nInserted paragraph.",
		1,
	);
	let parsed = Arc::new(
		markview_commonmark::parse_incremental(
			old,
			source.clone().into(),
			old.dialect,
		)
		.unwrap(),
	);
	let fresh = markview_commonmark::parse_with(source.clone(), old.dialect);
	assert_ne!(
		fresh.anchor_block("target").unwrap().id(),
		parsed.anchor_block("target").unwrap().id()
	);
	fs::write(&case.target, source).unwrap();
	let document = crate::loaded::LoadedDocument::markdown(parsed);
	let layout = crate::layout::LayoutEngine::new()
		.layout(&document, &case.app.options());
	let revision = case.app.readers.session.content_version + 1;
	case.app.readers.session.content_version = revision;
	case.app.readers.session.accept(
		crate::worker::ReaderSnapshot {
			document,
			layout,
			content_version: revision,
			complete: true,
			parse_complete: true,
			blocked_images: Default::default(),
			remote_deferred: 0,
		},
		case.app.viewport(),
		None,
	);
	case.app.readers.session.source_stamp =
		Some(crate::watch::file_stamp(&case.target));
	case.app.select_tab(1);
	case.settle();
	case.show();
	case.click(|target| matches!(target, LinkTarget::Url(_)), 0);
	case.settle();
	assert_eq!(
		case.app.readers.session.path,
		Some(case.dir.path().join("next.md").canonicalize().unwrap())
	);
}

#[test]
fn disclosure_ids_are_mapped_by_structure_instead_of_reused_across_documents() {
	let mut case = Case::new(
		"<details><summary>Details</summary>\n\nBody.\n\n</details>\n",
		true,
	);
	case.app.select_tab(0);
	case.settle();
	let document = case.app.readers.session.document.as_ref().unwrap();
	let old_id = disclosure(&case.app, "Details");
	let mut builder = markview_document::DocumentBuilder::continuing(
		document.revision(),
		Some(markview_document::BlockId::new(10000).unwrap()),
	);
	let blocks = document
		.blocks()
		.iter()
		.map(|block| {
			if block.id() == old_id {
				builder.block(block.anchors().to_vec(), block.kind().clone())
			} else {
				block.clone()
			}
		})
		.collect();
	let document = crate::loaded::LoadedDocument::semantic(Arc::new(
		builder.finish(blocks),
	));
	let layout = crate::layout::LayoutEngine::new()
		.layout(&document, &case.app.options());
	case.app.readers.session.accept(
		crate::worker::ReaderSnapshot {
			document,
			layout,
			content_version: case.app.readers.session.content_version,
			complete: true,
			parse_complete: true,
			blocked_images: Default::default(),
			remote_deferred: 0,
		},
		case.app.viewport(),
		None,
	);
	let remapped_id = disclosure(&case.app, "Details");
	assert_ne!(old_id, remapped_id);
	case.app.select_tab(1);
	case.settle();
	case.show();
	case.click(|target| matches!(target, LinkTarget::Disclosure(_)), 0);
	case.settle();
	assert_eq!(
		case.app.readers.session.details_open.get(&remapped_id),
		Some(&true)
	);
}
