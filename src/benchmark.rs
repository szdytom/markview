//! Explicitly scoped timing: full document layout and completed offscreen GPU work.
use crate::{
	document,
	file::read_document,
	layout::{LayoutEngine, LayoutOptions, LayoutSnapshot},
	render::{Renderer, Theme, View},
};
use anyhow::{Context, Result};
use serde::Serialize;
use std::{collections::HashMap, fs, path::Path, time::Instant};

#[derive(Serialize, Clone)]
pub struct Timing {
	pub image_prepare_ms: f64,
	pub read_ms: f64,
	pub parse_ms: f64,
	pub layout_ms: f64,
	pub gpu_prepare_and_complete_ms: f64,
	pub total_ms: f64,
}
#[derive(Serialize)]
pub struct Distribution {
	pub count: usize,
	pub p50_ms: f64,
	pub p95_ms: f64,
	pub max_ms: f64,
}
fn distribution(samples: &[Timing]) -> Distribution {
	let mut times: Vec<f64> = samples.iter().map(|t| t.total_ms).collect();
	times.sort_by(f64::total_cmp);
	Distribution {
		count: times.len(),
		p50_ms: times[(times.len() - 1) / 2],
		p95_ms: times
			[((times.len() as f64 * 0.95).ceil() as usize).saturating_sub(1)],
		max_ms: *times.last().unwrap(),
	}
}

#[derive(Serialize, Default)]
pub struct Memory {
	pub linux_rss_bytes: Option<u64>,
	pub linux_peak_rss_bytes: Option<u64>,
}
pub fn memory() -> Memory {
	let text = fs::read_to_string("/proc/self/status").unwrap_or_default();
	let value = |key: &str| {
		text.lines()
			.find(|l| l.starts_with(key))
			.and_then(|l| l.split_whitespace().nth(1))
			.and_then(|v| v.parse::<u64>().ok())
			.map(|v| v * 1024)
	};
	Memory {
		linux_rss_bytes: value("VmRSS:"),
		linux_peak_rss_bytes: value("VmHWM:"),
	}
}

#[derive(Serialize)]
struct Report {
	scope: &'static str,
	adapter: String,
	file: String,
	bytes: usize,
	content_hash: String,
	physical_size: [u32; 2],
	scale: f32,
	column_width: f32,
	font_size: f32,
	initialization_ms: f64,
	first_open: Timing,
	full_layout_reopens: Distribution,
	cached_refreshes: Distribution,
	full_layout_samples: Vec<Timing>,
	cached_samples: Vec<Timing>,
	memory_after_scroll: Memory,
	tracked_gpu_bytes_excluding_driver: u64,
	reading_text_index_bytes: usize,
	degraded_paragraphs: usize,
	formula_errors: usize,
	/// Inclusive layout sub-stage totals for the cold first open; populated
	/// only when `MARKVIEW_PROFILE=1` is set.
	profile_ms: Option<HashMap<&'static str, f64>>,
}

#[expect(clippy::too_many_arguments, reason = "CLI benchmark parameters")]
pub fn run(
	path: &Path,
	output: Option<&Path>,
	width: u32,
	height: u32,
	scale: f32,
	theme: Theme,
	iterations: usize,
	options: LayoutOptions,
	offline: bool,
) -> Result<()> {
	let init = Instant::now();
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	renderer.set_stylesheet(options.stylesheet.clone());
	let mut engine = LayoutEngine::new();
	let mut images = crate::images::Images::new(offline);
	engine.validate_stylesheet(&options.stylesheet)?;
	let _ =
		engine.label("Markview", 14.0, 0.0, 0.0, crate::layout::Paint::Text);
	let texture = renderer.offscreen(width, height);
	let target = texture.create_view(&Default::default());
	let initialization_ms = init.elapsed().as_secs_f64() * 1000.0;
	let horizontal = HashMap::new();
	let mut view = View {
		selection: None,
		hovered_link: None,
		held_overflow: None,
		hovered_overflow: None,
		revision: 0,
		width,
		height,
		scale,
		scroll: 0.0,
		left: ((width as f32 / scale - options.width) * 0.5).max(16.0),
		top: 24.0,
		bottom: 24.0,
		theme,
		horizontal: &horizontal,
	};
	let mut latest = LayoutSnapshot::default();
	let mut sample = |reuse: bool| -> Result<Timing> {
		if !reuse {
			engine.clear_document_cache();
		}
		let start = Instant::now();
		let text = read_document(path)?;
		let read_ms = start.elapsed().as_secs_f64() * 1000.0;
		let t = Instant::now();
		let doc = document::parse(text);
		let parse_ms = t.elapsed().as_secs_f64() * 1000.0;
		let image_start = Instant::now();
		images.prepare(&doc, path, 1, false);
		images.wait();
		let mut image_prepare_ms = image_start.elapsed().as_secs_f64() * 1000.;
		let t = Instant::now();
		latest = engine.layout_with_images(&doc, &options, &images.snapshot);
		let mut layout_ms = t.elapsed().as_secs_f64() * 1000.0;
		let t = Instant::now();
		let submission = renderer.render(&latest, &view, &[], &target)?;
		renderer.wait(Some(submission))?;
		let mut gpu_prepare_and_complete_ms = t.elapsed().as_secs_f64() * 1000.;
		let t = Instant::now();
		images.wait();
		image_prepare_ms += t.elapsed().as_secs_f64() * 1000.;
		if latest.images.entries != images.snapshot.entries {
			let t = Instant::now();
			latest =
				engine.layout_with_images(&doc, &options, &images.snapshot);
			layout_ms += t.elapsed().as_secs_f64() * 1000.;
			let t = Instant::now();
			let submission = renderer.render(&latest, &view, &[], &target)?;
			renderer.wait(Some(submission))?;
			gpu_prepare_and_complete_ms += t.elapsed().as_secs_f64() * 1000.;
		}
		Ok(Timing {
			image_prepare_ms,
			read_ms,
			parse_ms,
			layout_ms,
			gpu_prepare_and_complete_ms,
			total_ms: start.elapsed().as_secs_f64() * 1000.0,
		})
	};
	let profiling = markview_core::profile::requested();
	if profiling {
		markview_core::profile::enable();
		markview_core::profile::reset();
	}
	let first_open = sample(false)?;
	let profile_ms = profiling.then(markview_core::profile::totals);
	let full_layout_samples = (0..iterations)
		.map(|_| sample(false))
		.collect::<Result<Vec<_>>>()?;
	let cached_samples = (0..iterations)
		.map(|_| sample(true))
		.collect::<Result<Vec<_>>>()?;
	let mut scroll = 0.0;
	while scroll < latest.height {
		view.scroll = scroll;
		let submission = renderer.render(&latest, &view, &[], &target)?;
		renderer.wait(Some(submission))?;
		scroll += (height as f32 / scale - 48.0).max(1.0);
	}
	let text = read_document(path)?;
	let report = Report {
		scope: "Release-mode target. Offscreen full layout + completed first-viewport GPU rendering; window/compositor presentation excluded. First open has cold document/glyph/math caches. Reopens clear block layouts but retain text-engine, math and glyph caches. OS file cache is not flushed.",
		adapter: renderer.adapter_name.clone(),
		file: path.display().to_string(),
		bytes: text.len(),
		content_hash: format!("{:016x}", document::fingerprint(&text)),
		physical_size: [width, height],
		scale,
		column_width: options.width,
		font_size: options.font_size,
		initialization_ms,
		first_open,
		full_layout_reopens: distribution(&full_layout_samples),
		cached_refreshes: distribution(&cached_samples),
		full_layout_samples,
		cached_samples,
		memory_after_scroll: memory(),
		reading_text_index_bytes: latest.text_index_bytes(),
		tracked_gpu_bytes_excluding_driver: renderer.gpu_bytes()
			+ (width as u64 * height as u64 * 4),
		degraded_paragraphs: latest.degraded,
		formula_errors: latest.math_errors,
		profile_ms,
	};
	let json = serde_json::to_string_pretty(&report)?;
	if let Some(path) = output {
		if let Some(parent) =
			path.parent().filter(|p| !p.as_os_str().is_empty())
		{
			fs::create_dir_all(parent)?;
		}
		fs::write(path, &json)
			.with_context(|| format!("Write {}", path.display()))?;
	}
	println!("{json}");
	Ok(())
}
