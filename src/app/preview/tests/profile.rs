//! Opt-in offscreen measurements; latency runs exclude allocator instrumentation.
use super::*;
use crate::{
	memory,
	render::{Renderer, Theme, View},
};
use anyhow::{Result, ensure};
use serde::Serialize;
use std::{fs, path::Path};

#[derive(Serialize)]
struct Timings {
	wait_ms: f64,
	build_ms: f64,
	frame_ms: f64,
	hover_to_gpu_ms: f64,
	max_tick_ms: f64,
}
#[derive(Serialize)]
struct Sample {
	timings: Timings,
	#[cfg(feature = "diagnostics")]
	allocations: crate::test_support::allocations::Stats,
	rss_after: memory::Memory,
	rss_after_close: memory::Memory,
}
#[derive(Serialize)]
struct Measurement {
	case: String,
	instrumented: bool,
	definition_bytes: usize,
	adapter: String,
	intentional_delay_ms: f64,
	prepare_delay_ms: f64,
	baseline: memory::Memory,
	first: Sample,
	repeated: Vec<Sample>,
}
struct Gpu {
	renderer: Renderer,
	target: wgpu::TextureView,
}
impl Gpu {
	fn new(app: &App<Proxy>) -> Result<Self> {
		let mut renderer = pollster::block_on(Renderer::new(None))?;
		renderer.set_fonts(app.options().fonts);
		renderer.set_stylesheet(app.options().stylesheet);
		let target = renderer
			.offscreen(1200, 800)
			.create_view(&Default::default());
		Ok(Self { renderer, target })
	}
	fn frame(&mut self, app: &mut App<Proxy>) -> Result<()> {
		let overlay = app.overlay();
		let scene = app.preview.scene(&app.readers, Theme::Light);
		let geometry = app.view_geometry();
		let view = View {
			width: 1200,
			height: 800,
			scale: 1.0,
			left: geometry.left,
			top: geometry.top,
			bottom: geometry.bottom,
			scroll: geometry.scroll,
			theme: Theme::Light,
			horizontal: &app.readers.session.display.horizontal,
			selection: None,
			revision: app.readers.session.accepted_revision,
			hovered_link: None,
			held_overflow: None,
		};
		let submission = self.renderer.render_document_scenes(
			&app.readers.session.display.snapshot,
			&view,
			&overlay,
			scene.as_slice(),
			&self.target,
		)?;
		self.renderer.wait(Some(submission))?;
		Ok(())
	}
}

fn milliseconds(duration: Duration) -> f64 {
	duration.as_secs_f64() * 1000.0
}
fn pipeline(
	app: &mut App<Proxy>,
	gpu: &mut Gpu,
	hover: Instant,
) -> Result<Timings> {
	app.hover_preview(hover);
	let mut max_tick = Duration::ZERO;
	let timeout = hover + Duration::from_secs(30);
	while app.preview.floating().is_none() {
		let due = app.preview.deadline().unwrap_or(timeout).min(timeout);
		if let Some(completion) = app
			.preview
			.worker
			.wait(due.saturating_duration_since(Instant::now()))
		{
			app.preview.accept(completion);
		}
		ensure!(Instant::now() < timeout, "preview task timed out");
		let tick = Instant::now();
		app.preview_tick(tick);
		max_tick = max_tick.max(tick.elapsed());
	}
	let wait_ms = milliseconds(hover.elapsed());
	let build_ms = milliseconds(app.preview.last_build);
	let frame = Instant::now();
	gpu.frame(app)?;
	Ok(Timings {
		wait_ms,
		build_ms,
		frame_ms: milliseconds(frame.elapsed()),
		hover_to_gpu_ms: milliseconds(hover.elapsed()),
		max_tick_ms: milliseconds(max_tick),
	})
}

fn sample(app: &mut App<Proxy>, gpu: &mut Gpu) -> Result<Sample> {
	let hover = Instant::now();
	#[cfg(feature = "diagnostics")]
	let (timings, allocations) =
		crate::test_support::allocations::measure(|| pipeline(app, gpu, hover));
	#[cfg(not(feature = "diagnostics"))]
	let timings = pipeline(app, gpu, hover);
	let timings = timings?;
	let rss_after = memory::memory();
	app.dismiss_preview();
	gpu.frame(app)?;
	Ok(Sample {
		timings,
		#[cfg(feature = "diagnostics")]
		allocations,
		rss_after,
		rss_after_close: memory::memory(),
	})
}

#[test]
#[ignore = "release-mode hover latency and heap measurement; one case per process"]
fn measure_hover_preview() -> Result<()> {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	let case =
		std::env::var("MARKVIEW_HOVER_CASE").unwrap_or_else(|_| "short".into());
	let definition = if case == "stress" {
		format!("[^note]: A large note.\n\n{}", "    This paragraph exercises full subtree layout even though only a small slice is visible. The note keeps its original document identities and must not copy the semantic tree to build a preview.\n\n".repeat(128))
	} else {
		ensure!(
			["short", "long", "image", "tall-image"].contains(&case.as_str()),
			"unknown hover case"
		);
		fs::read_to_string(
			root.join(format!("tests/fixtures/hover-preview/{case}.md")),
		)?
	};
	let images = super::golden::images(root)?;
	let mut app = super::golden::reader_app(&definition, Theme::Light, &images);
	let point = super::interaction_tests::reference_point(&app);
	app.interaction.pointer.move_to((point.x, point.y));
	let mut gpu = Gpu::new(&app)?;
	gpu.frame(&mut app)?;
	let adapter = gpu.renderer.adapter_name.clone();
	let baseline = memory::memory();
	let first = sample(&mut app, &mut gpu)?;
	let mut repeated = Vec::with_capacity(30);
	for _ in 0..30 {
		repeated.push(sample(&mut app, &mut gpu)?);
	}
	let measurement = Measurement {
		case,
		instrumented: cfg!(feature = "diagnostics"),
		definition_bytes: definition.len(),
		adapter,
		intentional_delay_ms: milliseconds(SHOW_DELAY),
		prepare_delay_ms: milliseconds(PREPARE_DELAY),
		baseline,
		first,
		repeated,
	};
	let json = serde_json::to_vec_pretty(&measurement)?;
	if let Ok(output) = std::env::var("MARKVIEW_HOVER_OUTPUT") {
		fs::write(output, &json)?;
	}
	println!("{}", String::from_utf8(json)?);
	Ok(())
}
