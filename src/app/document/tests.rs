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
#[ignore = "requires a GPU"]
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
