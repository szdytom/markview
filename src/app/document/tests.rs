use super::*;
use crate::{
	app::SendEvent,
	cli::{LaunchOptions, Mode},
};
use std::{fs, sync::mpsc, time::Duration};

#[derive(Clone)]
struct Proxy(mpsc::Sender<Event>);
impl SendEvent for Proxy {
	fn try_send(&self, event: Event) -> bool {
		self.0.send(event).is_ok()
	}
}

struct Harness {
	app: App<Proxy>,
	events: mpsc::Receiver<Event>,
	dir: tempfile::TempDir,
}
impl Harness {
	fn new() -> Self {
		let dir = tempfile::tempdir().unwrap();
		for (name, color) in [
			("first.png", [255, 0, 255, 255]),
			("second.png", [0, 255, 0, 255]),
		] {
			image::RgbaImage::from_pixel(512, 512, image::Rgba(color))
				.save(dir.path().join(name))
				.unwrap();
		}
		fs::write(
			dir.path().join("diagram.svg"),
			br##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="30"><rect width="40" height="30" fill="#00ffff"/></svg>"##,
		)
		.unwrap();
		fs::write(
			dir.path().join("README.md"),
			format!(
				"![first](first.png)\n\n<img src=\"diagram.svg\" width=\"80\">{}",
				"\n\nReading paragraph.".repeat(50)
			),
		)
		.unwrap();
		fs::write(dir.path().join("README.zh-cn.md"), "![second](second.png)")
			.unwrap();
		let (tx, events) = mpsc::channel();
		let mut app = App::new(
			LaunchOptions {
				mode: Mode::Smoke,
				offline: true,
				width: 400,
				height: 300,
				options: crate::test_support::options(),
				..Default::default()
			},
			Proxy(tx),
		);
		app.preferences.values = Default::default();
		Self { app, events, dir }
	}

	fn open(&mut self, name: &str, count: usize) {
		self.app.open(self.dir.path().join(name));
		self.wait_images(count);
	}

	fn wait_images(&mut self, count: usize) {
		let deadline = Instant::now() + Duration::from_secs(10);
		while self.app.readers.session.layout_pending
			|| self.app.readers.session.snapshot.images.decoded().len() != count
		{
			let event = self
				.events
				.recv_timeout(
					deadline.saturating_duration_since(Instant::now()),
				)
				.expect("active tab images did not load");
			if let Event::Ready(update) = event
				&& update.version == self.app.readers.session.version
				&& Some(&update.path) == self.app.readers.session.path.as_ref()
				&& let Some(result) = update.result
			{
				let reader = result.unwrap();
				let viewport = self.app.viewport();
				if !self.app.readers.session.can_display(&reader, viewport) {
					continue;
				}
				self.app.readers.session.accept(
					reader,
					viewport,
					update.counts,
				);
				self.app.readers.session.displayed_version = update.version;
			}
		}
	}
}

struct StubLoop;
impl crate::app::window::Loop for StubLoop {
	fn exit(&self) {}
}

#[test]
fn web_tabs_open_immediately_and_complete_without_stealing_focus() {
	let mut h = Harness::new();
	h.app.args.offline = false;
	let permit = h
		.app
		.services
		.handle
		.transfers
		.clone()
		.try_acquire_many_owned(4)
		.unwrap();
	let url = "https://example.org/".to_string();
	h.app.open_web_page(url.clone());
	assert_eq!(h.app.readers.entries().len(), 1);
	assert!(h.app.readers.session.web_loading);
	assert!(h.app.readers.session.layout_pending);
	assert!(h.app.readers.session.document.is_none());
	let pending_path = h.app.readers.session.path.clone().unwrap();
	h.app.open_web_page(url.clone());
	assert_eq!(h.app.readers.entries().len(), 1);
	h.open("README.md", 2);
	let active_path = h.app.readers.session.path.clone();
	let markdown = "# Web article\n\nNative article text.\n";
	h.app.web_page_loaded(
		url.clone(),
		pending_path,
		Ok(crate::web_page::Article {
			markdown: markdown.into(),
			security: crate::security::Security::web(url.clone()),
		}),
	);
	assert_eq!(h.app.readers.session.path, active_path);
	assert_eq!(h.app.readers.active(), 1);
	h.app.open_web_page(url);
	assert_eq!(h.app.readers.active(), 0);
	h.wait_images(0);
	let loaded_path = h.app.readers.session.path.clone().unwrap();
	assert!(loaded_path.starts_with(h.app.paste_dir.path()));
	assert!(
		loaded_path
			.file_name()
			.unwrap()
			.to_string_lossy()
			.contains("Web article")
	);
	assert_eq!(fs::read_to_string(loaded_path).unwrap(), markdown);
	assert!(!h.app.readers.session.web_loading);
	assert!(!h.app.readers.session.snapshot.blocks.is_empty());
	drop(h);
	drop(permit);
}

#[test]
fn failed_tabs_keep_their_errors_and_closed_web_tabs_ignore_completions() {
	let mut h = Harness::new();
	h.app.preferences.values.lang = Some(crate::lang::Lang::ZhHans);
	let url = "https://example.org/".to_string();
	h.app.open_web_page(url.clone());
	assert_eq!(h.app.readers.entries().len(), 1);
	let first_path = h.app.readers.session.path.clone().unwrap();
	let error = h.app.readers.session.load_error.clone().unwrap();
	assert!(error.contains("离线"));
	assert!(!h.app.readers.session.layout_pending);
	let version = h.app.readers.session.version;
	h.app.request(false);
	assert_eq!(h.app.readers.session.version, version);
	assert_eq!(h.app.readers.session.load_error.as_ref(), Some(&error));
	h.open("README.md", 2);
	h.app.select_tab(0);
	assert_eq!(h.app.readers.session.load_error.as_ref(), Some(&error));
	assert!(!h.app.readers.session.layout_pending);
	h.app.close_tab(0);
	h.app.web_page_loaded(
		url.clone(),
		first_path.clone(),
		Ok(crate::web_page::Article {
			markdown: "# Closed article".into(),
			security: crate::security::Security::web(url.clone()),
		}),
	);
	assert_eq!(h.app.readers.entries().len(), 1);
	h.app.open_web_page(url.clone());
	let second_path = h.app.readers.session.path.clone().unwrap();
	assert_ne!(first_path, second_path);
	h.app.web_page_loaded(
		url.clone(),
		first_path,
		Ok(crate::web_page::Article {
			markdown: "# Stale article".into(),
			security: crate::security::Security::web(url.clone()),
		}),
	);
	assert_eq!(h.app.readers.session.path.as_ref(), Some(&second_path));
	assert!(h.app.readers.session.load_error.is_some());

	h.app.args.offline = false;
	let permit = h
		.app
		.services
		.handle
		.transfers
		.clone()
		.try_acquire_many_owned(4)
		.unwrap();
	h.app.open_web_page(url.clone());
	assert_eq!(h.app.readers.entries().len(), 2);
	assert!(h.app.readers.session.web_loading);
	assert!(h.app.readers.session.load_error.is_none());
	h.app.web_page_loaded(
		url,
		second_path,
		Err(anyhow::anyhow!("download failed")),
	);
	assert!(
		h.app
			.readers
			.session
			.load_error
			.as_deref()
			.unwrap()
			.contains("download failed")
	);

	h.app.open(h.dir.path().join("missing.md"));
	assert_eq!(h.app.readers.entries().len(), 3);
	assert!(h.app.readers.session.layout_pending);
	let deadline = Instant::now() + Duration::from_secs(10);
	while h.app.readers.session.load_error.is_none() {
		let event = h
			.events
			.recv_timeout(deadline.saturating_duration_since(Instant::now()))
			.unwrap();
		h.app.handle_user_event(&StubLoop, event);
	}
	let failed_path = h.app.readers.session.path.clone();
	let error = h.app.readers.session.load_error.clone();
	assert!(!h.app.readers.session.layout_pending);
	h.app.select_tab(0);
	h.app.select_tab(2);
	assert_eq!(h.app.readers.session.path, failed_path);
	assert_eq!(h.app.readers.session.load_error, error);
	assert!(!h.app.readers.session.layout_pending);
	drop(h);
	drop(permit);
}

#[test]
fn cached_tab_refresh_defers_fragment_links_until_new_content_arrives() {
	for background in [false, true] {
		for anchor in ["new-heading", "existing", "missing"] {
			let mut h = Harness::new();
			let path = h.dir.path().join("target.md");
			fs::write(&path, "# Existing\n\nOriginal text.\n").unwrap();
			h.open("target.md", 0);
			h.open("README.zh-cn.md", 1);
			fs::write(
				&path,
				format!(
					"{}\n\n# New heading\n\nNew text.\n\n# Existing\n\nUpdated text.\n",
					"Paragraph.\n\n".repeat(30),
				),
			)
			.unwrap();
			h.app.open_link(&format!("target.md#{anchor}"), background);
			if background {
				h.app.select_tab(0);
			}
			let session = &h.app.readers.session;
			assert!(session.layout_pending);
			assert!(session.snapshot_complete);
			assert_ne!(session.accepted_revision, session.content_version);
			assert_eq!(session.pending_anchor.as_deref(), Some(anchor));
			assert!(!h.app.status.is_error());
			let deadline = Instant::now() + Duration::from_secs(10);
			while h.app.readers.session.layout_pending {
				let event = h
					.events
					.recv_timeout(
						deadline.saturating_duration_since(Instant::now()),
					)
					.unwrap();
				h.app.handle_user_event(&StubLoop, event);
			}
			assert!(h.app.readers.session.pending_anchor.is_none());
			if anchor == "missing" {
				assert!(h.app.status.is_error());
			} else {
				assert!(!h.app.status.is_error());
				let viewport = h.app.viewport();
				let session = &mut h.app.readers.session;
				let y = session.snapshot.anchor_y(anchor).unwrap().clamp(
					0.0,
					crate::state::scroll_limit(
						session.snapshot.height,
						viewport,
					),
				);
				session.advance_scroll(
					Instant::now() + Duration::from_secs(1),
					viewport,
				);
				assert!((session.scrolling.offset - y).abs() < 0.01);
			}
		}
	}
}

#[test]
fn cached_text_tabs_refresh_background_edits_on_every_activation_path() {
	for activation in 0..4 {
		let mut h = Harness::new();
		let path = h.dir.path().join("text.md");
		let before = "# Before\n\nOriginal text.\n";
		let after = "# After!\n\nModified text.\n";
		assert_eq!(before.len(), after.len());
		fs::write(&path, before).unwrap();
		h.open("text.md", 0);
		let document = h.app.readers.session.document.clone().unwrap();
		let revision = h.app.readers.session.content_version;
		h.open("README.zh-cn.md", 1);
		h.app.select_tab(0);
		assert!(!h.app.readers.session.layout_pending);
		assert_eq!(h.app.readers.session.content_version, revision);
		assert!(std::sync::Arc::ptr_eq(
			h.app.readers.session.document.as_ref().unwrap(),
			&document,
		));
		h.app.readers.session.load_all_images = true;
		h.app.readers.session.remote_notice_dismissed = true;
		h.app.readers.session.details_open =
			std::sync::Arc::new([(1, true)].into());
		h.app.readers.session.security.grant(
			crate::security::Resource::SelectedImage {
				source: "chosen.png".into(),
				path: h.dir.path().join("first.png"),
			},
		);
		if activation == 3 {
			h.app.new_page();
		} else {
			h.app.select_tab(1);
		}
		if activation % 2 == 0 {
			fs::write(&path, after).unwrap();
			let modified = fs::metadata(&path).unwrap().modified().unwrap();
			fs::OpenOptions::new()
				.write(true)
				.open(&path)
				.unwrap()
				.set_times(
					fs::FileTimes::new()
						.set_modified(modified + Duration::from_secs(2)),
				)
				.unwrap();
		} else {
			let replacement = h.dir.path().join("replacement.md");
			fs::write(&replacement, format!("{after}\nNew paragraph.\n"))
				.unwrap();
			fs::rename(replacement, &path).unwrap();
		}
		match activation {
			0 | 3 => h.app.select_tab(0),
			1 => h.app.open(path.clone()),
			2 => h.app.close_tab(1),
			_ => unreachable!(),
		}
		assert!(h.app.readers.session.layout_pending);
		assert_eq!(h.app.readers.session.content_version, revision + 1);
		assert!(!h.app.readers.session.load_all_images);
		assert!(!h.app.readers.session.remote_notice_dismissed);
		assert!(h.app.readers.session.details_open.is_empty());
		assert!(
			h.app
				.readers
				.session
				.security
				.selected_image("chosen.png")
				.is_none()
		);
		h.wait_images(0);
		assert_eq!(
			h.app
				.readers
				.session
				.document
				.as_ref()
				.unwrap()
				.source
				.as_ref(),
			fs::read_to_string(&path).unwrap(),
		);
	}
}

#[test]
fn cached_text_tabs_notice_background_deletion_and_recreation() {
	let mut h = Harness::new();
	let path = h.dir.path().join("text.md");
	fs::write(&path, "Original text.").unwrap();
	h.open("text.md", 0);
	h.open("README.zh-cn.md", 1);
	fs::remove_file(&path).unwrap();
	h.app.select_tab(0);
	let deadline = Instant::now() + Duration::from_secs(10);
	while h.app.readers.session.load_error.is_none() {
		let event = h
			.events
			.recv_timeout(deadline.saturating_duration_since(Instant::now()))
			.unwrap();
		h.app.handle_user_event(&StubLoop, event);
	}
	h.app.select_tab(1);
	fs::write(&path, "Recreated text.").unwrap();
	h.app.select_tab(0);
	assert!(h.app.readers.session.load_error.is_none());
	h.wait_images(0);
	assert_eq!(
		h.app
			.readers
			.session
			.document
			.as_ref()
			.unwrap()
			.source
			.as_ref(),
		"Recreated text.",
	);
}

#[test]
fn returning_to_cached_tabs_reloads_images_after_switching_or_closing() {
	for close in [false, true] {
		let mut h = Harness::new();
		h.open("README.md", 2);
		let old_pixels = h.app.readers.session.snapshot.images.clone();
		let revision = h.app.readers.session.content_version;
		h.app.readers.session.scrolling.offset = 300.;
		h.app.readers.session.load_all_images = true;
		h.open("README.zh-cn.md", 1);
		assert!(
			old_pixels.decoded().is_empty(),
			"inactive pixels were retained"
		);
		if close {
			h.app.close_tab(1);
			h.wait_images(2);
		} else {
			h.open("README.md", 2);
		}
		let session = &h.app.readers.session;
		assert_eq!(session.content_version, revision);
		assert_eq!(session.scrolling.offset, 300.);
		assert!(session.load_all_images);
		let pixels = session.snapshot.images.decoded();
		assert_eq!(&pixels["first.png"].rgba[..4], &[255, 0, 255, 255]);
		assert_eq!(&pixels["diagram.svg"].rgba[..4], &[0, 255, 255, 255]);
	}
}

#[test]
fn gpu_restored_tabs_draw_images_after_switching_or_closing()
-> anyhow::Result<()> {
	use crate::render::{Renderer, Theme, View};
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	let target = renderer.offscreen(400, 300);
	let horizontal = Default::default();
	let view = View {
		width: 400,
		height: 300,
		scale: 1.,
		left: 0.,
		top: 0.,
		bottom: 0.,
		scroll: 0.,
		theme: Theme::Light,
		horizontal: &horizontal,
		selection: None,
		revision: 1,
		hovered_link: None,
		hovered_overflow: None,
		held_overflow: None,
	};
	for close in [false, true] {
		let mut h = Harness::new();
		for step in 0..3 {
			match step {
				0 => h.open("README.md", 2),
				1 => h.open("README.zh-cn.md", 1),
				_ if close => {
					h.app.close_tab(1);
					h.wait_images(2);
				}
				_ => h.open("README.md", 2),
			}
			let submission = renderer.render(
				&h.app.readers.session.snapshot,
				&view,
				&[],
				&target.create_view(&Default::default()),
			)?;
			renderer.wait(Some(submission))?;
			let output = h.dir.path().join("frame.png");
			renderer.save_png(&target, &output)?;
			let want = if step == 1 {
				[0, 255, 0]
			} else {
				[255, 0, 255]
			};
			let count = image::open(output)?
				.to_rgb8()
				.pixels()
				.filter(|pixel| {
					(0..3).all(|i| pixel.0[i].abs_diff(want[i]) <= 6)
				})
				.count();
			assert!(
				count > 800,
				"image missing after step {step}, close={close}"
			);
		}
	}
	Ok(())
}

#[test]
fn new_page_preserves_tabs_and_opening_or_selecting_resumes_documents() {
	let mut h = Harness::new();
	h.open("README.md", 2);
	let first = h.app.readers.session.path.clone();
	h.app.interaction.viewer = Some(crate::state::Viewer {
		src: "first.png".into(),
		pixels: (512., 512.),
		scale: 1.,
		zoom: 1.,
		pan: (0., 0.),
		grab: None,
		pressed_at: None,
		dragged: false,
	});
	h.app.readers.session.scrolling.offset = 120.;
	h.app.show_panel(crate::state::PanelPage::Tabs);
	h.app.action(crate::state::Command::NewPage);
	assert_eq!(h.app.readers.entries().len(), 1);
	assert!(h.app.readers.session.path.is_none());
	assert!(h.app.readers.session.snapshot.blocks.is_empty());
	assert!(h.app.watch.is_none());
	assert!(h.app.interaction.viewer.is_none());
	assert_eq!(h.app.interaction.panel, crate::state::PanelPage::Closed);
	assert!(
		h.app
			.buttons()
			.iter()
			.any(|b| b.action == crate::state::Command::NewPage)
	);
	h.app.action(crate::state::Command::NewPage);
	assert_eq!(h.app.readers.entries().len(), 1);
	h.app.select_tab(0);
	h.wait_images(2);
	assert_eq!(h.app.readers.session.path, first);
	assert_eq!(h.app.readers.session.scrolling.offset, 120.);
	h.app.new_page();
	h.open("README.zh-cn.md", 1);
	assert_eq!(h.app.readers.entries().len(), 2);
	assert_eq!(h.app.readers.active(), 1);
	h.app.new_page();
	h.app.close_tab(1);
	assert!(h.app.readers.session.path.is_none());
	h.app.open(h.dir.path().join("README.md"));
	h.wait_images(2);
	assert_eq!(h.app.readers.entries().len(), 1);
	assert_eq!(h.app.readers.session.path, first);
}

#[test]
fn closing_last_parked_tab_releases_the_workers_document() {
	let mut h = Harness::new();
	fs::write(h.dir.path().join("first.md"), "First document").unwrap();
	fs::write(h.dir.path().join("second.md"), "Second document").unwrap();
	h.open("first.md", 0);
	h.open("second.md", 0);
	let document = std::sync::Arc::downgrade(
		h.app.readers.session.document.as_ref().unwrap(),
	);
	h.app.new_page();
	h.app.close_tab(0);
	assert_eq!(h.app.readers.entries().len(), 1);
	assert!(h.app.readers.session.path.is_none());
	assert!(document.upgrade().is_some());
	h.app.show_panel(crate::state::PanelPage::Tabs);
	h.app.close_tab(0);
	assert!(h.app.readers.entries().is_empty());
	assert!(h.app.readers.session.path.is_none());
	let deadline = Instant::now() + Duration::from_secs(5);
	while document.upgrade().is_some() {
		assert!(
			Instant::now() < deadline,
			"worker retained the closed document"
		);
		std::thread::sleep(Duration::from_millis(10));
	}
	assert_eq!(h.app.interaction.panel, crate::state::PanelPage::Closed);
}

#[test]
fn untrusted_local_navigation_and_images_need_grants_without_reusing_trusted_tabs()
 {
	use crate::{
		security::{Origin, Security, Trust},
		state::{Command, Modal},
	};
	let mut h = Harness::new();
	h.open("README.zh-cn.md", 1);
	let source = fs::canonicalize(h.dir.path().join("README.md")).unwrap();
	h.app
		.open_document(source.clone(), Security::local(Trust::Untrusted));
	h.wait_images(0);
	assert!(
		h.app
			.readers
			.session
			.blocked_images
			.contains_key("first.png")
	);
	let resource = h.app.readers.session.blocked_images["first.png"].clone();
	h.app
		.ask_permission(resource, crate::state::PermissionAction::Images);
	h.app.action(Command::ModalConfirm);
	h.wait_images(1);
	h.app.action(Command::RemoteLoadAll);
	h.wait_images(1);
	assert!(
		h.app
			.readers
			.session
			.blocked_images
			.contains_key("diagram.svg")
	);
	assert_eq!(
		h.app.readers.session.security.origin.trust(),
		Trust::Untrusted
	);
	h.app.open_link("README.zh-cn.md", false);
	assert_eq!(h.app.readers.entries().len(), 2);
	assert!(matches!(
		h.app.interaction.modal,
		Some(Modal::Permission { .. })
	));
	h.app.action(Command::ModalConfirm);
	h.wait_images(0);
	assert_eq!(h.app.readers.entries().len(), 3);
	assert_eq!(
		h.app.readers.session.security.origin,
		Origin::Local(Trust::Untrusted)
	);
	assert!(
		h.app
			.readers
			.session
			.blocked_images
			.contains_key("second.png")
	);
	h.app.select_tab(1);
	h.wait_images(1);
	h.app.handle_user_event(&StubLoop, Event::Changed(source));
	h.wait_images(0);
	assert!(
		h.app
			.readers
			.session
			.blocked_images
			.contains_key("first.png")
	);
}

#[test]
fn clipboard_images_use_selected_files_and_web_relative_images_use_the_url() {
	use crate::{
		security::{Origin, Resource, Trust},
		state::Command,
	};
	let mut h = Harness::new();
	h.app.open_text("# Pasted\n\n![image](first.png)");
	h.wait_images(0);
	assert_eq!(h.app.readers.session.security.origin, Origin::Clipboard);
	assert_eq!(
		h.app.readers.session.blocked_images["first.png"],
		Resource::SelectImage("first.png".into())
	);
	let document = h.app.readers.session.path.clone();
	let revision = h.app.readers.session.content_version;
	h.app.handle_user_event(
		&StubLoop,
		Event::ImageSelected {
			document,
			revision,
			source: "first.png".into(),
			file: Some(h.dir.path().join("first.png")),
		},
	);
	h.wait_images(1);
	h.app.action(Command::RemoteLoadAll);
	assert_eq!(
		h.app.readers.session.security.origin.trust(),
		Trust::Untrusted
	);
	let staged = h.app.readers.session.path.clone().unwrap();
	h.app.open(staged);
	h.wait_images(0);
	assert_eq!(h.app.readers.entries().len(), 2);
	assert_eq!(
		h.app.readers.session.security.origin,
		Origin::Local(Trust::Trusted)
	);
}

#[test]
fn folder_open_navigation_links_and_refresh_use_the_native_reader() {
	use crate::app::folder::Tree;
	use crate::state::Command;
	let mut h = Harness::new();
	h.app.tab_strip.phone = true;
	let root = h.dir.path().canonicalize().unwrap();
	fs::create_dir(root.join("notes")).unwrap();
	fs::create_dir(root.join(".obsidian")).unwrap();
	fs::write(root.join(".obsidian/private.md"), "hidden").unwrap();
	fs::write(
		root.join("notes/Other note.md"),
		"# A heading\n\nOther note.",
	)
	.unwrap();
	h.app.open(root.clone());
	let event = h.events.recv_timeout(Duration::from_secs(10)).unwrap();
	h.app.handle_user_event(&StubLoop, event);
	h.wait_images(2);
	assert!(h.app.phone_folder_open());
	assert_eq!(
		h.app.readers.session.path.as_ref(),
		Some(&root.join("README.md"))
	);
	let tree = h.app.folder.tree.as_ref().unwrap();
	assert!(
		tree.entries
			.iter()
			.all(|e| !e.path.starts_with(root.join(".obsidian")))
	);
	let note = tree
		.entries
		.iter()
		.position(|e| e.path.ends_with("Other note.md"))
		.unwrap();
	assert_eq!(tree.entries[note].depth, 1);
	h.app.action(Command::FolderEntry(note));
	h.wait_images(0);
	assert!(!h.app.phone_folder_open());
	assert_eq!(
		h.app.readers.session.path.as_ref(),
		Some(&root.join("notes/Other note.md"))
	);
	h.app.open_link("README.md", false);
	h.wait_images(2);
	let note_link =
		markview_commonmark::parse("[[Other note#A heading|Section]]");
	let crate::document::BlockKind::Paragraph(text) = &note_link.blocks[0].kind
	else {
		panic!()
	};
	h.app.open_link(text[0].style.link_url().unwrap(), false);
	h.wait_images(0);
	h.app.apply_anchor();
	assert!(h.app.readers.session.pending_anchor.is_none());
	assert!(!h.app.status.is_error());
	assert_eq!(h.app.readers.entries().len(), 2);
	assert_eq!(
		h.app.readers.session.path.as_ref(),
		Some(&root.join("notes/Other note.md"))
	);
	// Folder lookup never supplies local paths to web or clipboard documents.
	h.app.readers.session.security = crate::security::Security::default();
	h.app.readers.session.security.origin = crate::security::Origin::Clipboard;
	assert!(h.app.folder_link("README.md", Some(&root)).is_none());
	let notes = h
		.app
		.folder
		.tree
		.as_ref()
		.unwrap()
		.entries
		.iter()
		.position(|e| e.path == root.join("notes"))
		.unwrap();
	assert!(h.app.folder.tree.as_ref().unwrap().collapsed(notes));
	h.app.action(Command::FolderEntry(notes));
	h.app.action(Command::FolderRoot);
	fs::remove_file(root.join("notes/Other note.md")).unwrap();
	fs::write(root.join("notes/New.md"), "# A new note").unwrap();
	fs::create_dir(root.join("notes/new folder")).unwrap();
	fs::write(root.join("notes/new folder/Nested.md"), "# Nested").unwrap();
	h.app.folder_loaded(1, false, Tree::read(root.clone()));
	assert!(h.app.folder.tree.as_ref().unwrap().rows().is_empty());
	h.app.action(Command::FolderRoot);
	let tree = h.app.folder.tree.as_ref().unwrap();
	assert!(!tree.collapsed(notes));
	assert!(
		tree.rows()
			.iter()
			.any(|i| tree.entries[*i].path.ends_with("New.md"))
	);
	assert!(
		!tree
			.rows()
			.iter()
			.any(|i| tree.entries[*i].path.ends_with("Nested.md"))
	);
	assert!(
		h.app
			.folder
			.tree
			.as_ref()
			.unwrap()
			.entries
			.iter()
			.all(|e| !e.path.ends_with("Other note.md"))
	);
	// An older folder result cannot replace the latest requested tree.
	h.app.folder_loaded(0, true, Err(anyhow::anyhow!("stale")));
	assert_eq!(h.app.folder.tree.as_ref().unwrap().root, root);
}

#[test]
fn folder_scan_completion_preserves_newer_navigation() {
	for phone in [false, true] {
		for navigation in
			["open", "select", "current", "return", "new", "close"]
		{
			let mut h = Harness::new();
			h.app.tab_strip.phone = phone;
			h.open("README.md", 2);
			h.open("README.zh-cn.md", 1);
			let root = h.dir.path().canonicalize().unwrap();
			fs::write(root.join("new.md"), "# New").unwrap();
			h.app.open(root.clone());
			let completion = loop {
				let event =
					h.events.recv_timeout(Duration::from_secs(10)).unwrap();
				if matches!(event, Event::FolderLoaded { .. }) {
					break event;
				}
			};
			match navigation {
				"open" => h.app.open(root.join("new.md")),
				"select" => h.app.select_tab(0),
				"current" => h.app.select_tab(1),
				"return" => {
					h.app.select_tab(0);
					h.app.select_tab(1);
				}
				"new" => h.app.new_page(),
				"close" => h.app.close_tab(1),
				_ => unreachable!(),
			}
			h.app.set_folder_open(false);
			let path = h.app.readers.session.path.clone();
			let active = h.app.readers.active();
			let count = h.app.readers.entries().len();
			h.app.handle_user_event(&StubLoop, completion);
			assert_eq!(h.app.folder.tree.as_ref().unwrap().root, root);
			assert_eq!(h.app.readers.session.path, path, "{navigation}");
			assert_eq!(h.app.readers.active(), active, "{navigation}");
			assert_eq!(h.app.readers.entries().len(), count, "{navigation}");
			assert!(!h.app.folder.open, "{navigation}");
			assert!(!h.app.phone_folder_open(), "{navigation}");
		}
	}
}

#[test]
fn folder_scan_completion_preserves_newer_input_and_capture() {
	use crate::state::{Command, TextField};
	use winit::event::{Ime, WindowEvent};
	for field in [TextField::Search, TextField::ExportTitle] {
		let mut h = Harness::new();
		let root = h.dir.path().to_path_buf();
		h.app.folder_loaded(
			0,
			true,
			crate::app::folder::Tree::read(root.clone()),
		);
		h.wait_images(2);
		let completion = crate::app::folder::Tree::read(root.clone());
		let capture = if field == TextField::Search {
			h.app.open_search();
			Command::SearchCase
		} else {
			h.app.action(Command::Export);
			h.app.action(Command::FocusInput(field));
			Command::ExportRun
		};
		let captured = h
			.app
			.buttons()
			.iter()
			.find(|b| b.action == capture)
			.unwrap()
			.id;
		h.app.interaction.widgets.captured = Some(captured);
		h.app.folder_loaded(0, false, completion);
		assert_eq!(h.app.focused_input(), Some(field));
		assert_eq!(h.app.interaction.widgets.captured, Some(captured));
		h.app
			.input_event(&WindowEvent::Ime(Ime::Commit("typed".into())));
		let text = match field {
			TextField::Search => h.app.readers.session.search.input.text(),
			TextField::ExportTitle => h.app.readers.session.export_title.text(),
			_ => unreachable!(),
		};
		assert_eq!(text, "typed");
		for command in [Command::FolderRoot, Command::FolderEntry(0)] {
			h.app.interaction.widgets.focus = Some(command.widget_id());
			h.app.interaction.widgets.captured = Some(command.widget_id());
			h.app.folder_loaded(
				0,
				false,
				crate::app::folder::Tree::read(root.clone()),
			);
			assert!(h.app.interaction.widgets.focus.is_none());
			assert!(h.app.interaction.widgets.captured.is_none());
		}
		h.app.tab_strip.phone = true;
		h.app.set_folder_open(true);
		let focus = h.app.interaction.widgets.focus;
		h.app.interaction.widgets.captured = focus;
		h.app
			.folder_loaded(0, false, crate::app::folder::Tree::read(root));
		assert!(focus.is_some());
		assert_eq!(h.app.interaction.widgets.focus, focus);
		assert_eq!(h.app.interaction.widgets.captured, focus);
	}
}

#[test]
fn folder_scan_completion_keeps_the_outline_when_the_folder_was_hidden() {
	let mut h = Harness::new();
	let root = h.dir.path().to_path_buf();
	h.app
		.folder_loaded(0, true, crate::app::folder::Tree::read(root.clone()));
	h.wait_images(2);
	h.app.toggle_outline();
	assert!(h.app.folder.open && h.app.interaction.overlays.outline.open());
	let completion = crate::app::folder::Tree::read(root);
	h.app.set_folder_open(false);
	h.app.folder_loaded(0, false, completion);
	assert!(!h.app.folder.open);
	assert!(h.app.interaction.overlays.outline.open());
}

#[test]
fn folder_links_prefer_existing_document_relative_paths() {
	let mut h = Harness::new();
	let root = h.dir.path().canonicalize().unwrap();
	for directory in ["docs/guide", "guide"] {
		fs::create_dir_all(root.join(directory)).unwrap();
	}
	for name in ["docs/index.md", "docs/guide/setup.md", "guide/setup.md"] {
		fs::write(root.join(name), "# Setup").unwrap();
	}
	for folder_loaded in [false, true] {
		if folder_loaded {
			h.app.folder_loaded(
				0,
				false,
				crate::app::folder::Tree::read(root.clone()),
			);
		}
		h.open("docs/index.md", 0);
		h.app.open_link("guide/setup.md#setup", false);
		h.wait_images(0);
		h.app.apply_anchor();
		assert_eq!(
			h.app.readers.session.path,
			Some(root.join("docs/guide/setup.md")),
			"folder loaded: {folder_loaded}"
		);
		assert!(h.app.readers.session.pending_anchor.is_none());
	}
	fs::remove_file(root.join("docs/guide/setup.md")).unwrap();
	h.open("docs/index.md", 0);
	h.app.open_link("guide/setup.md", false);
	h.wait_images(0);
	assert_eq!(
		h.app.readers.session.path,
		Some(root.join("guide/setup.md"))
	);
}

#[test]
fn desktop_folder_toggle_is_hidden_before_loading_a_tree() {
	use crate::{settings::WindowLayout, state::Command};
	let mut h = Harness::new();
	h.app.tab_strip.phone = false;
	for reading in [false, true] {
		if reading {
			h.open("README.md", 2);
		}
		for style in [
			WindowLayout::Linux,
			WindowLayout::Windows,
			WindowLayout::Macos,
		] {
			h.app.frame.layout = style;
			let buttons = h.app.buttons();
			assert!(!buttons.iter().any(|b| b.action == Command::Folder));
			assert!(h.app.folder.tree.is_none());
			assert!(h.app.frame_layout().sidebar_toggle.is_none());
			assert_eq!(h.app.frame_layout().left_inset, 0.);
		}
	}
}

#[test]
fn folder_links_preserve_native_paths_with_url_delimiters() {
	let mut h = Harness::new();
	let root = h.dir.path().join("vault #100%25");
	fs::create_dir(&root).unwrap();
	fs::write(root.join("README.md"), "# Vault").unwrap();
	let mut names = vec!["Note #1.md", "Note %41.md"];
	if markview_platform::test_support::special_files() {
		names.push("Note ?1.md");
	}
	for name in &names {
		fs::write(root.join(name), "# A note").unwrap();
	}
	h.app
		.folder_loaded(0, true, crate::app::folder::Tree::read(root.clone()));
	h.wait_images(0);
	for name in names {
		let url = percent_encoding::utf8_percent_encode(
			name,
			percent_encoding::NON_ALPHANUMERIC,
		)
		.to_string();
		h.app.open_link(&url, false);
		assert!(h.app.interaction.modal.is_none(), "{name}");
		assert_eq!(
			h.app.readers.session.path,
			Some(root.join(name).canonicalize().unwrap())
		);
		h.wait_images(0);
	}
	fs::write(root.join("Attachment.bin"), "data").unwrap();
	h.app.open_link("Attachment.bin", false);
	assert!(matches!(&h.app.interaction.modal,
		Some(crate::state::Modal::OpenLocal { path, .. })
		if *path == root.join("Attachment.bin").canonicalize().unwrap()));
}

#[test]
fn folder_wikilinks_open_literal_percent_names_even_with_a_decoded_match() {
	let mut h = Harness::new();
	let root = h.dir.path().canonicalize().unwrap();
	for name in ["Note %41.md", "Note A.md", "Note %2541.md"] {
		fs::write(root.join(name), "# A heading\n\nA note.").unwrap();
	}
	h.app
		.folder_loaded(0, true, crate::app::folder::Tree::read(root.clone()));
	h.wait_images(2);
	for (source, name) in [
		("[[Note %41]]", "Note %41.md"),
		("[[Note %41.md#A heading|Section]]", "Note %41.md"),
		("[[Note %2541]]", "Note %2541.md"),
		("[Markdown](Note%20%41.md)", "Note A.md"),
	] {
		let doc = markview_commonmark::parse(source);
		let crate::document::BlockKind::Paragraph(text) = &doc.blocks[0].kind
		else {
			panic!()
		};
		h.app.open_link(text[0].style.link_url().unwrap(), false);
		h.wait_images(0);
		h.app.apply_anchor();
		assert_eq!(
			h.app.readers.session.path,
			Some(root.join(name)),
			"{source}"
		);
		assert!(!h.app.status.is_error(), "{source}");
		assert!(h.app.readers.session.pending_anchor.is_none(), "{source}");
	}
}

#[test]
fn reveal_folder_opens_the_active_document_directory_externally() {
	use crate::state::Command;
	if !markview_platform::test_support::shell_launch() {
		return;
	}
	let capture = "MARKVIEW_TEST_REVEAL_FOLDER";
	let Ok(output) = std::env::var(capture) else {
		let dir = tempfile::tempdir().unwrap();
		let opener = dir.path().join("xdg-open");
		fs::write(
			&opener,
			"#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$MARKVIEW_TEST_REVEAL_FOLDER\"\n",
		)
		.unwrap();
		markview_platform::test_support::set_mode(&opener, 0o755).unwrap();
		let result = std::process::Command::new(std::env::current_exe().unwrap())
			.args([
				"--exact",
				"app::document::tests::reveal_folder_opens_the_active_document_directory_externally",
			])
			.env("PATH", dir.path())
			.env(capture, dir.path().join("opened"))
			.output()
			.unwrap();
		assert!(result.status.success(), "{result:?}");
		return;
	};
	let mut h = Harness::new();
	h.app.action(Command::RevealFolder);
	assert!(h.app.status.is_error());
	assert!(!std::path::Path::new(&output).exists());
	let nested = h.dir.path().join("nested notes #1");
	fs::create_dir(&nested).unwrap();
	fs::write(nested.join("note.md"), "A note").unwrap();
	h.app.folder_loaded(
		0,
		true,
		crate::app::folder::Tree::read(h.dir.path().into()),
	);
	h.wait_images(2);
	for sidebar_open in [true, false] {
		if !sidebar_open {
			h.app.toggle_folder();
			h.open("nested notes #1/note.md", 0);
		}
		let action = h
			.app
			.buttons()
			.into_iter()
			.find(|b| b.action == Command::RevealFolder)
			.unwrap()
			.action;
		h.app.action(action);
		let expected = format!(
			"{}\n",
			h.app
				.readers
				.session
				.path
				.as_ref()
				.unwrap()
				.parent()
				.unwrap()
				.display()
		);
		let deadline = Instant::now() + Duration::from_secs(5);
		while fs::read_to_string(&output).unwrap_or_default() != expected {
			assert!(
				Instant::now() < deadline,
				"external folder was not opened"
			);
			std::thread::sleep(Duration::from_millis(10));
		}
		assert_eq!(h.app.folder.open, sidebar_open);
		assert_eq!(h.app.folder.tree.as_ref().unwrap().root, h.dir.path());
		fs::remove_file(&output).unwrap();
	}
}

#[test]
fn folder_overlay_clips_empty_page_mouse_touch_and_focus_targets() {
	use crate::state::{Command, TextField};
	let mut h = Harness::new();
	let empty = h.dir.path().join("empty");
	fs::create_dir(&empty).unwrap();
	h.app
		.folder_loaded(0, true, crate::app::folder::Tree::read(empty));
	h.app
		.advance_drawers(Instant::now() + Duration::from_secs(1));
	assert!(h.app.readers.session.path.is_none());
	for width in [360., 640., 1200.] {
		let mut chrome = h.app.chrome();
		chrome.width = width;
		chrome.height = 800.;
		let drawer = crate::app::chrome::folder::rect(
			width,
			800.,
			crate::app::TOP,
			crate::app::BOTTOM,
			260.,
		);
		let left = drawer.x + drawer.w;
		let buttons = chrome.buttons();
		let layout = crate::app::widgets::layout(&buttons);
		let empty_control = |action| {
			matches!(
				action,
				Command::Open
					| Command::OpenUrl
					| Command::FocusInput(TextField::Url)
			)
		};
		for x in (0..drawer.w as usize).step_by(4) {
			for y in
				(drawer.y as usize..(drawer.y + drawer.h) as usize).step_by(4)
			{
				let point = (x as f32, y as f32);
				// The toolbar's own `Open` button lives outside the drawer.
				assert!(
					!layout
						.hit(point)
						.is_some_and(|w| empty_control(w.message))
				);
				assert!(
					!layout
						.hit_touch(point, 44.)
						.is_some_and(|w| empty_control(w.message))
				);
			}
		}
		for button in buttons
			.iter()
			.filter(|b| empty_control(b.action) && b.rect.y > crate::app::TOP)
		{
			let hit = button.widget().hit_rect().unwrap();
			assert!(hit.x >= left);
			assert!(hit.w > 0.);
			assert!(
				layout
					.hit((hit.x + hit.w / 2., hit.y + hit.h / 2.))
					.is_some()
			);
		}
		let toggle = buttons
			.iter()
			.find(|b| b.action == Command::Folder)
			.unwrap();
		assert_eq!(toggle.rect.y, 6.);
	}
	// Closing the sidebar restores the empty page's original controls.
	h.app.toggle_folder();
	assert!(
		h.app
			.buttons()
			.iter()
			.any(|b| b.action == Command::FocusInput(TextField::Url))
	);
}

#[test]
fn folder_sidebar_geometry_keyboard_and_pointer_ownership() {
	use crate::app::folder::Tree;
	use crate::state::Command;
	use winit::keyboard::{Key, NamedKey};
	let mut h = Harness::new();
	h.app.args.width = 1000;
	h.app.args.height = 400;
	for i in 0..60 {
		fs::write(h.dir.path().join(format!("note-{i:02}.md")), "A note")
			.unwrap();
	}
	// Use the same completion path without starting a second scan.
	h.app
		.folder_loaded(0, true, Tree::read(h.dir.path().into()));
	h.wait_images(2);
	h.app
		.advance_drawers(Instant::now() + Duration::from_secs(1));
	assert_eq!(h.app.folder_inset(), 260.);
	assert!(h.app.view_geometry().left >= 280.);
	assert!(
		h.app.options().width
			<= h.app.dimensions().0 - h.app.folder_inset() - 40.
	);
	h.app.interaction.pointer.move_to((20., 150.));
	assert!(h.app.pointer_in_folder());
	h.app.folder.scroll.reset();
	h.app.wheel_folder(100.);
	assert_eq!(
		h.app.folder.scroll.offset(),
		0.,
		"the wheel eases the tree instead of jumping"
	);
	assert!(h.app.folder.scroll.animating());
	let (_, thumb) = h.app.folder_bar().expect("the tree scrolls").bars(false);
	h.app
		.interaction
		.pointer
		.move_to((thumb.x + thumb.w / 2., thumb.y + thumb.h / 2.));
	assert!(h.app.begin_folder_drag());
	assert!(
		!h.app.folder.scroll.animating(),
		"the grabbed thumb stops the ease"
	);
	let held = h.app.folder.scroll.offset();
	h.app
		.advance_control_scroll(Instant::now() + Duration::from_secs(1));
	assert_eq!(
		h.app.folder.scroll.offset(),
		held,
		"the thumb stayed under the held pointer"
	);
	h.app.folder.grab = None;
	h.app.interaction.pointer.move_to((20., 150.));
	h.app.wheel_folder(100.);
	assert!(h.app.folder.scroll.animating());
	h.app
		.advance_control_scroll(Instant::now() + Duration::from_secs(1));
	assert!(h.app.folder.scroll.offset() > 0.);
	assert!(!h.app.folder.scroll.animating());
	h.app.scroll_folder(10000.);
	assert!(h.app.folder.scroll.offset() > 0.);
	let document_scroll = h.app.readers.session.scrolling.offset;
	let (track, _) = h.app.folder_bar().unwrap().bars(false);
	h.app
		.interaction
		.pointer
		.move_to((track.x + track.w / 2., track.y + 5.));
	assert!(h.app.begin_folder_drag());
	assert_eq!(h.app.readers.session.scrolling.offset, document_scroll);
	h.app.folder.grab = None;
	let buttons = h.app.buttons();
	let ids: std::collections::HashSet<_> =
		buttons.iter().map(|b| b.id).collect();
	assert_eq!(
		ids.len(),
		buttons.len(),
		"Sidebar and toolbar controls have distinct identities"
	);
	let entry = buttons
		.iter()
		.find(|b| matches!(b.action, Command::FolderEntry(_)))
		.unwrap();
	assert!(
		entry.widget().hit_rect().unwrap().y >= h.app.folder_rect().y + 66.
	);
	h.app.interaction.widgets.focus = Some(Command::FolderEntry(0).widget_id());
	assert!(h.app.key_pressed(&Key::Named(NamedKey::ArrowDown)));
	assert_eq!(
		h.app.interaction.widgets.focus,
		Some(Command::FolderEntry(1).widget_id())
	);
	assert!(h.app.folder.scroll.offset() <= 28.);
	h.app.interaction.widgets.focus = Some(Command::FolderRoot.widget_id());
	assert!(h.app.key_pressed(&Key::Named(NamedKey::ArrowLeft)));
	assert!(h.app.folder.tree.as_ref().unwrap().rows().is_empty());
	assert!(
		!h.app
			.buttons()
			.iter()
			.any(|b| matches!(b.action, Command::FolderEntry(_)))
	);
	assert!(h.app.key_pressed(&Key::Named(NamedKey::ArrowRight)));
	assert!(h.app.key_pressed(&Key::Named(NamedKey::ArrowDown)));
	assert_eq!(
		h.app.interaction.widgets.focus,
		Some(Command::FolderEntry(0).widget_id())
	);
	// The top-left control owns a complete press and toggles both ways.
	for open in [false, true] {
		h.app
			.advance_drawers(Instant::now() + Duration::from_secs(1));
		let buttons = h.app.buttons();
		let toggle = buttons
			.iter()
			.find(|b| b.action == Command::Folder)
			.unwrap();
		h.app
			.interaction
			.pointer
			.move_to((toggle.rect.x + 14., toggle.rect.y + 14.));
		h.app.press_widget(&buttons);
		let action = h.app.release_widget().unwrap();
		h.app.action(action);
		assert_eq!(h.app.folder.open, open);
	}
	h.app.interaction.modal = Some(crate::state::Modal::OpenLocal {
		path: "unknown".into(),
		dir: "".into(),
		document_dir: None,
	});
	assert!(!h.app.pointer_in_folder());
	assert!(!h.app.folder_key(&Key::Named(NamedKey::ArrowDown)));
	h.app.interaction.modal = None;
	h.app.toggle_folder();
	assert_eq!(h.app.folder_inset(), 0.);
	h.app
		.advance_drawers(Instant::now() + Duration::from_secs(1));
	assert!(h.app.view_geometry().left < 280.);
}

#[test]
fn folder_overlay_resize_keeps_search_and_its_focus() {
	use crate::state::{Command, TextField};
	use winit::{dpi::PhysicalSize, event::WindowEvent, window::WindowId};
	let mut h = Harness::new();
	h.app.folder_loaded(
		0,
		true,
		crate::app::folder::Tree::read(h.dir.path().into()),
	);
	h.wait_images(2);
	h.app.open_search();
	assert!(h.app.folder.open && h.app.readers.session.search.open);
	for (width, folder_open) in [(760, true), (536, false), (900, false)] {
		h.app.handle_window_event(
			&StubLoop,
			WindowId::dummy(),
			WindowEvent::Resized(PhysicalSize::new(width, 800)),
		);
		assert_eq!(h.app.folder.open, folder_open);
		assert!(h.app.readers.session.search.open);
		assert_eq!(h.app.focused_input(), Some(TextField::Search));
		if !folder_open {
			assert_eq!(h.app.folder.reveal(), 0.);
			assert!(!h.app.buttons().iter().any(|b| matches!(
				b.action,
				Command::FolderRoot | Command::FolderEntry(_)
			)));
		}
	}
}

#[test]
fn folder_drawers_reconcile_on_resize_to_an_overlay() {
	use crate::state::Command;
	use winit::{dpi::PhysicalSize, event::WindowEvent, window::WindowId};
	let mut h = Harness::new();
	h.app.folder_loaded(
		0,
		true,
		crate::app::folder::Tree::read(h.dir.path().into()),
	);
	h.wait_images(2);
	h.app.toggle_outline();
	h.app
		.advance_drawers(Instant::now() + Duration::from_secs(1));
	assert!(h.app.folder.open);
	assert!(h.app.interaction.overlays.outline.open());
	h.app.folder.grab = Some(0.);
	h.app.interaction.widgets.focus = Some(Command::FolderRoot.widget_id());
	h.app.interaction.widgets.captured = Some(Command::FolderRoot.widget_id());
	for (width, folder_open) in [(760, true), (536, false), (900, false)] {
		h.app.handle_window_event(
			&StubLoop,
			WindowId::dummy(),
			WindowEvent::Resized(PhysicalSize::new(width, 800)),
		);
		assert_eq!(h.app.folder.open, folder_open);
		assert!(h.app.interaction.overlays.outline.open());
		if width == 536 {
			assert_eq!(h.app.folder.reveal(), 0.);
			assert!(h.app.folder.grab.is_none());
			assert!(h.app.interaction.widgets.focus.is_none());
			assert!(h.app.interaction.widgets.captured.is_none());
			assert!(!h.app.pointer_in_folder());
			assert!(!h.app.buttons().iter().any(|button| matches!(
				button.action,
				Command::FolderEntry(_) | Command::FolderRoot
			)));
		}
	}
}

#[test]
fn folder_sidebar_reuses_drawer_motion_and_keeps_outgoing_coverage() {
	use crate::layout::Draw;
	use crate::state::Command;
	let mut h = Harness::new();
	let empty = h.dir.path().join("empty");
	fs::create_dir(&empty).unwrap();
	h.app
		.folder_loaded(0, true, crate::app::folder::Tree::read(empty));
	assert!(h.app.drawer_deadline(Instant::now()).is_some());
	assert!(
		h.app
			.advance_drawers(Instant::now() + Duration::from_secs(1))
	);
	let open = h.app.folder_rect();
	assert_eq!(open.x, 0.);
	let toggle = |app: &mut App<Proxy>| {
		app.buttons()
			.into_iter()
			.find(|b| b.action == Command::Folder)
			.unwrap()
	};
	let open_toggle = toggle(&mut h.app);
	assert_eq!(open_toggle.rect.x, open.w - 36.);
	assert_eq!(open_toggle.rect.y, 6.);
	assert!(!open_toggle.active);
	let document_left = h.app.view_geometry().left;
	let has_sidebar = |draws: Vec<Draw>| {
		draws.iter().any(|draw| {
		matches!(draw, Draw::Rect(rect, _) if rect.w == open.w && rect.h == 800.)
	})
	};
	assert!(has_sidebar(h.app.overlay()));
	h.app.toggle_folder();
	h.app.folder.freeze_animation(Duration::from_millis(110));
	assert!(!h.app.folder.open);
	assert_eq!(h.app.folder.reveal(), 0.125);
	let closing_toggle = toggle(&mut h.app);
	assert_eq!(closing_toggle.id, open_toggle.id);
	assert!(!closing_toggle.active);
	assert!(h.app.drawer_deadline(Instant::now()).is_some());
	assert!(has_sidebar(h.app.overlay()));
	let drawer = h.app.folder_rect();
	assert_eq!(drawer.x, -drawer.w * 0.875);
	let buttons = h.app.buttons();
	assert!(!buttons.iter().any(|b| matches!(
		b.action,
		Command::FolderRoot | Command::FolderEntry(_)
	)));
	// The outgoing surface still covers empty-page widgets while sliding away.
	for button in buttons.iter().filter(|b| {
		matches!(b.action, Command::Open | Command::OpenUrl)
			&& b.rect.y > crate::app::TOP
	}) {
		assert!(button.widget().hit_rect().unwrap().x >= drawer.x + drawer.w);
	}
	// Reversing a running close keeps using the shared animation and scheduler.
	h.app.toggle_folder();
	assert!(h.app.folder.open);
	assert!(h.app.folder.animating());
	h.app
		.advance_drawers(Instant::now() + Duration::from_secs(1));
	assert_eq!(h.app.folder_rect(), open);
	h.app.toggle_folder();
	h.app
		.advance_drawers(Instant::now() + Duration::from_secs(1));
	assert!(h.app.drawer_deadline(Instant::now()).is_none());
	assert!(!has_sidebar(h.app.overlay()));
	assert_eq!(h.app.folder_rect().x + open.w, 0.);
	assert!(!h.app.pointer_in_folder());
	let closed_toggle = toggle(&mut h.app);
	assert_eq!(closed_toggle.id, open_toggle.id);
	assert_eq!(closed_toggle.rect.y, 6.);
	assert!(
		closed_toggle.rect.x + closed_toggle.rect.w
			< h.app.frame_layout().tabs.x
	);
	assert!(!closed_toggle.active);
	assert!(h.app.view_geometry().left < document_left);
	h.app.toggle_folder();
	let opening_toggle = toggle(&mut h.app);
	assert_eq!(opening_toggle.id, closed_toggle.id);
	assert_eq!(opening_toggle.rect.y, closed_toggle.rect.y);
}

#[test]
fn file_sidebar_owns_the_full_left_column_without_moving_the_outline() {
	use crate::{app::folder::Tree, layout::Draw, state::Command};
	let mut h = Harness::new();
	h.app
		.folder_loaded(0, true, Tree::read(h.dir.path().into()));
	h.wait_images(2);
	h.app
		.advance_drawers(Instant::now() + Duration::from_secs(1));
	let surface = h.app.folder_surface();
	assert_eq!((surface.y, surface.h), (0., 800.));
	assert!(h.app.frame_layout().tabs.x >= surface.w + 10.);
	assert!(h.app.frame_layout().new_page.unwrap().x >= surface.w);
	h.app.interaction.overlays.outline.set_open(true);
	let body = h.app.folder_rect();
	h.app.readers.session.search.open = true;
	let outline = h.app.outline_drawer();
	assert_eq!(h.app.folder_rect(), body);
	assert!(h.app.search_input_rect().x >= surface.w + 12.);
	assert!(
		h.app
			.search_buttons()
			.iter()
			.all(|b| b.rect.intersect(surface).is_none())
	);
	assert!(
		h.app
			.buttons()
			.iter()
			.filter(|b| matches!(
				b.action,
				Command::NewPage
					| Command::Open | Command::Settings
					| Command::Outline
			))
			.all(|b| b.widget().hit_rect().is_none_or(|r| r.x >= surface.w))
	);
	let draws = h.app.overlay();
	let file_layer = draws
		.iter()
		.position(
			|draw| matches!(draw, Draw::Rect(rect, _) if *rect == surface),
		)
		.unwrap();
	let toolbar_layer = draws.iter().position(|draw| matches!(draw, Draw::Rect(rect, _) if rect.y == 0. && rect.w == 1200. && rect.h == crate::app::TOP)).unwrap();
	assert!(file_layer > toolbar_layer);
	h.app.interaction.pointer.move_to((surface.w, 20.));
	assert!(h.app.begin_sidebar_resize());
	let (x, y) = h.app.interaction.pointer.cursor();
	h.app.interaction.pointer.move_to((x + 60., y));
	h.app.drag_sidebar();
	h.app.interaction.panels.sidebar_mut().clear();
	assert_eq!(h.app.frame_layout().left_inset, surface.w + 60.);
	assert_eq!(h.app.outline_drawer(), outline);
	h.app.toggle_folder();
	h.app
		.advance_drawers(Instant::now() + Duration::from_secs(1));
	assert_eq!(h.app.frame_layout().left_inset, 0.);
	assert_eq!(h.app.outline_drawer(), outline);
}

#[test]
fn folder_sidebar_renders_beside_the_document() -> anyhow::Result<()> {
	use crate::render::{Renderer, Theme, View};
	let mut h = Harness::new();
	let root = h.dir.path().join("Research notes");
	fs::create_dir_all(root.join("Projects/Markview")).unwrap();
	fs::create_dir_all(root.join("Reading list")).unwrap();
	for (name, text) in [
		(
			"README.md",
			"# Research notes\n\nA folder of connected Markdown documents.\n\n## Start here\n\nRead [[Projects/Markview/Design|the design notes]] or browse the file tree.\n\n> Keep the page in focus while navigating a collection.\n",
		),
		(
			"Projects/Markview/Design.md",
			"# Design\n\nNative typography.",
		),
		("Reading list/Typography.md", "# Typography"),
		("Meeting notes.md", "# Meetings"),
	] {
		fs::write(root.join(name), text)?;
	}
	h.app
		.folder_loaded(0, true, crate::app::folder::Tree::read(root));
	h.wait_images(0);
	h.app
		.advance_drawers(Instant::now() + Duration::from_secs(1));
	h.app.toggle_outline();
	h.app
		.advance_drawers(Instant::now() + Duration::from_secs(1));
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	let directory =
		Path::new(env!("CARGO_MANIFEST_DIR")).join("artifacts/folder-sidebar");
	fs::create_dir_all(&directory)?;
	for (dark, expanded) in
		[(false, true), (true, true), (false, false), (true, false)]
	{
		if h.app.folder.open != expanded {
			h.app.toggle_folder();
			h.app
				.advance_drawers(Instant::now() + Duration::from_secs(1));
			h.wait_images(0);
		}
		let sheet = markview_layout::style::Stylesheet::bundled(dark);
		h.app.preferences.values.stylesheet = sheet.clone();
		h.app.preferences.values.theme =
			if dark { Theme::Dark } else { Theme::Light };
		h.app.ui.set_stylesheet(sheet.clone());
		renderer.set_stylesheet(sheet);
		let geometry = h.app.view_geometry();
		let overlay = h.app.overlay();
		let horizontal = Default::default();
		let view = View {
			width: 1200,
			height: 800,
			scale: 1.,
			left: geometry.left,
			top: geometry.top,
			bottom: geometry.bottom,
			scroll: 0.,
			theme: h.app.preferences.values.theme,
			horizontal: &horizontal,
			selection: None,
			revision: h.app.readers.session.accepted_revision,
			hovered_link: None,
			hovered_overflow: None,
			held_overflow: None,
		};
		let target = renderer.offscreen(view.width, view.height);
		let submission = renderer.render(
			&h.app.readers.session.snapshot,
			&view,
			&overlay,
			&target.create_view(&Default::default()),
		)?;
		renderer.wait(Some(submission))?;
		renderer.save_png(
			&target,
			&directory.join(match (dark, expanded) {
				(false, true) => "light.png",
				(true, true) => "dark.png",
				(false, false) => "light-collapsed.png",
				(true, false) => "dark-collapsed.png",
			}),
		)?;
	}
	Ok(())
}

#[test]
fn folder_session_restores_the_root_visibility_and_selected_document() {
	let mut h = Harness::new();
	let root = h.dir.path().canonicalize().unwrap();
	fs::write(root.join("Last read.md"), "# Last read\n\nSaved reading.")
		.unwrap();
	h.app.instance_path = Some(root.join("instance.lock"));
	h.app.persistence.path = Some(root.join("session.json"));
	h.app.persistence.initialized = true;
	h.app.preferences.values.restore_session = true;
	h.app.preferences.values.single_instance = true;
	h.app.register_instance();
	h.app
		.folder_loaded(0, true, crate::app::folder::Tree::read(root.clone()));
	h.wait_images(2);
	h.app.open(root.join("Last read.md"));
	h.wait_images(0);
	h.app.folder.open = false;
	h.app.flush_session();
	h.app.instance = None;

	let mut restored = Harness::new();
	restored.app.instance_path = Some(root.join("instance.lock"));
	restored.app.persistence.path = Some(root.join("session.json"));
	restored.app.preferences.values.restore_session = true;
	restored.app.preferences.values.single_instance = true;
	restored.app.register_instance();
	restored.app.restore_session();
	while restored.app.folder.tree.is_none() {
		let event = restored
			.events
			.recv_timeout(Duration::from_secs(10))
			.unwrap();
		restored.app.handle_user_event(&StubLoop, event);
	}
	assert_eq!(restored.app.folder.tree.as_ref().unwrap().root, root);
	assert!(!restored.app.folder.open);
	assert_eq!(
		restored.app.readers.session.path.as_ref(),
		Some(&root.join("Last read.md"))
	);
}

#[test]
#[ignore = "profiles the local repository, including build directories"]
fn folder_sidebar_repository_scroll_profile() {
	let mut h = Harness::new();
	fs::write(h.dir.path().join("profile.md"), "# A note\n\nOne tab.").unwrap();
	h.open("profile.md", 0);
	let scan = Instant::now();
	let tree = crate::app::folder::Tree::read(PathBuf::from(env!(
		"CARGO_MANIFEST_DIR"
	)))
	.unwrap();
	let entries = tree.entries.len();
	let scan_ms = scan.elapsed().as_secs_f64() * 1000.;
	h.app.folder_loaded(0, false, Ok(tree));
	h.app.set_folder_open(true);
	h.app
		.advance_drawers(Instant::now() + Duration::from_secs(1));
	h.app.interaction.pointer.move_to((120., 250.));
	let mut samples = Vec::new();
	for step in 0..100 {
		let started = Instant::now();
		h.app.scroll_folder(if step < 50 { 17.25 } else { -17.25 });
		std::hint::black_box(h.app.overlay());
		samples.push(started.elapsed().as_secs_f64() * 1000.);
	}
	samples.sort_by(f64::total_cmp);
	eprintln!(
		"folder scroll: {entries} entries, scan={scan_ms:.1} ms, overlay p50={:.2} ms, p95={:.2} ms",
		samples[49], samples[94]
	);
}

#[test]
fn selecting_a_tab_opens_the_folder_onto_its_document() {
	use crate::state::Command;
	let mut h = Harness::new();
	let root = h.dir.path().canonicalize().unwrap();
	fs::create_dir_all(root.join("zoo/deep")).unwrap();
	fs::write(root.join("zoo/deep/Zeta.md"), "# Zeta").unwrap();
	// Enough siblings that the note sits below the list's clip.
	for index in 0..30 {
		fs::write(root.join(format!("note-{index:02}.md")), "note").unwrap();
	}
	h.open("README.md", 2);
	h.open("zoo/deep/Zeta.md", 0);
	h.app
		.folder_loaded(0, false, crate::app::folder::Tree::read(root.clone()));
	h.app.set_folder_open(true);
	h.app
		.advance_drawers(Instant::now() + Duration::from_secs(1));
	let rows = |h: &Harness| -> Vec<PathBuf> {
		let tree = h.app.folder.tree.as_ref().unwrap();
		tree.rows()
			.iter()
			.map(|i| {
				tree.entries[*i]
					.path
					.strip_prefix(&tree.root)
					.unwrap()
					.to_owned()
			})
			.collect()
	};
	// A row is only reachable while its branch is open and on screen.
	let reachable = |h: &mut Harness| -> Vec<PathBuf> {
		let buttons = h.app.buttons();
		let tree = h.app.folder.tree.as_ref().unwrap();
		buttons
			.iter()
			.filter_map(|b| match b.action {
				Command::FolderEntry(i) => Some(
					tree.entries[i]
						.path
						.strip_prefix(&tree.root)
						.unwrap()
						.to_owned(),
				),
				_ => None,
			})
			.collect()
	};
	h.app.action(Command::FolderCollapseAll);
	assert!(rows(&h).is_empty());
	h.app.action(Command::SelectTab(0));
	assert!(reachable(&mut h).contains(&PathBuf::from("README.md")));
	assert!(!reachable(&mut h).iter().any(|p| p.starts_with("zoo")));
	h.app.action(Command::SelectTab(1));
	let opened = rows(&h);
	for name in ["zoo", "zoo/deep", "zoo/deep/Zeta.md"] {
		assert!(opened.contains(&PathBuf::from(name)), "{opened:?}");
	}
	// The row lands inside the clip, not just somewhere in the tree.
	assert!(reachable(&mut h).contains(&PathBuf::from("zoo/deep/Zeta.md")));
	assert!(
		h.app.folder.scroll.offset() > 0.,
		"the row scrolls into the clip"
	);
	// A closed tree stays out of the way of a tab switch.
	h.app.set_folder_open(false);
	h.app.action(Command::SelectTab(0));
	assert!(!h.app.folder.open);
}
