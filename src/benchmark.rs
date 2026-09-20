//! Explicitly scoped timing: full document layout and completed offscreen GPU work.
use crate::{
	document,
	file::read_document,
	layout::{LayoutEngine, LayoutOptions, LayoutSnapshot},
	render::{Renderer, Theme, View},
};
use anyhow::{Context, Result};
use serde::Serialize;
use std::{
	collections::HashMap,
	fs,
	path::Path,
	time::{Duration, Instant},
};

#[derive(Serialize, Clone)]
pub struct Timing {
	pub image_prepare_ms: f64,
	pub read_ms: f64,
	pub parse_ms: f64,
	pub layout_ms: f64,
	pub gpu_prepare_and_complete_ms: f64,
	pub gpu_prepare_and_submit_ms: f64,
	pub gpu_wait_ms: f64,
	pub total_ms: f64,
}
#[derive(Serialize)]
pub struct Distribution {
	pub count: usize,
	pub p50_ms: f64,
	pub p95_ms: f64,
	pub max_ms: f64,
}
fn percentiles(mut times: Vec<f64>) -> Distribution {
	times.sort_by(f64::total_cmp);
	Distribution {
		count: times.len(),
		p50_ms: times[(times.len() - 1) / 2],
		p95_ms: times
			[((times.len() as f64 * 0.95).ceil() as usize).saturating_sub(1)],
		max_ms: *times.last().unwrap(),
	}
}
fn distribution(samples: &[Timing]) -> Distribution {
	percentiles(samples.iter().map(|t| t.total_ms).collect())
}

/// What one frame costs while scrolling through the whole document.
#[derive(Serialize)]
pub struct ScrollPass {
	/// Geometry, rasterization and upload; the CPU work before the GPU sees it.
	pub prepare_ms: Distribution,
	/// The whole frame, including completed GPU work.
	pub total_ms: Distribution,
	/// Frames that miss a 120 Hz and a 60 Hz budget.
	pub over_8_33_ms: usize,
	pub over_16_7_ms: usize,
	/// Glyphs and paths rasterized during the pass, including any a prewarm
	/// pass prepared before the frame that needed them.
	pub rasterized: u64,
	/// Of those, the ones rasterized inside a measured frame. This is the
	/// work the reader waits for.
	pub in_frame: u64,
	pub atlas_resets: u64,
	/// Atlas pressure at the end of the pass: how full the mask atlas is and
	/// how many entries it holds. A pass that stops helping is one whose
	/// document needs more glyphs than the atlas can keep.
	pub atlas_fill_percent: u8,
	pub atlas_entries: usize,
}

/// Scrolling is what a reader does most, so the frame it costs is measured
/// directly. Each pass walks the whole document a screenful at a time: `cold`
/// starts with an empty glyph atlas, `warm` reuses what the first pass
/// rasterized, and `prewarmed` gives the renderer the same budgeted prewarm
/// pass the window runs while the reader stays put before it moves on.
/// Compositor presentation is excluded.
#[derive(Serialize)]
pub struct ScrollPacing {
	pub frames: usize,
	pub cold: ScrollPass,
	pub warm: ScrollPass,
	pub prewarmed: ScrollPass,
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
	scroll: ScrollPacing,
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
		images.prepare(&doc, path, 1, false, &options.stylesheet);
		images.wait();
		let mut image_prepare_ms = image_start.elapsed().as_secs_f64() * 1000.;
		let t = Instant::now();
		latest = engine.layout_with_images(&doc, &options, &images.snapshot);
		let mut layout_ms = t.elapsed().as_secs_f64() * 1000.0;
		let t = Instant::now();
		let gpu_start = t;
		let submission = renderer.render(&latest, &view, &[], &target)?;
		let mut gpu_prepare_and_submit_ms = t.elapsed().as_secs_f64() * 1000.;
		let t = Instant::now();
		renderer.wait(Some(submission))?;
		let mut gpu_wait_ms = t.elapsed().as_secs_f64() * 1000.;
		let mut gpu_prepare_and_complete_ms =
			gpu_start.elapsed().as_secs_f64() * 1000.;
		let t = Instant::now();
		images.wait();
		image_prepare_ms += t.elapsed().as_secs_f64() * 1000.;
		if latest.images.entries != images.snapshot.entries {
			let t = Instant::now();
			latest =
				engine.layout_with_images(&doc, &options, &images.snapshot);
			layout_ms += t.elapsed().as_secs_f64() * 1000.;
			let t = Instant::now();
			let gpu_start = t;
			let submission = renderer.render(&latest, &view, &[], &target)?;
			gpu_prepare_and_submit_ms += t.elapsed().as_secs_f64() * 1000.;
			let t = Instant::now();
			renderer.wait(Some(submission))?;
			gpu_wait_ms += t.elapsed().as_secs_f64() * 1000.;
			gpu_prepare_and_complete_ms +=
				gpu_start.elapsed().as_secs_f64() * 1000.;
		}
		Ok(Timing {
			image_prepare_ms,
			read_ms,
			parse_ms,
			layout_ms,
			gpu_prepare_and_complete_ms,
			gpu_prepare_and_submit_ms,
			gpu_wait_ms,
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
	// Scrolling, measured a screenful at a time. The opening screenful is
	// always measured: a document with no geometry has no height to walk.
	let step = (height as f32 / scale - 48.0).max(1.0);
	let mut offsets = vec![0.0];
	let mut scroll = step;
	while scroll < latest.height {
		offsets.push(scroll);
		scroll += step;
	}
	let mut passes = Vec::new();
	for pass in 0..3 {
		// The warm pass keeps what the cold one rasterized; the prewarmed
		// pass starts empty, the way a reader who has just arrived has it.
		if pass != 1 {
			renderer.clear_raster_cache();
		}
		let before = renderer.raster_stats();
		let mut in_frame = 0;
		let (mut prepare, mut total) = (
			Vec::with_capacity(offsets.len()),
			Vec::with_capacity(offsets.len()),
		);
		for &offset in &offsets {
			view.scroll = offset;
			if pass == 2 {
				// The window spends a slice of each idle frame on the
				// screenful ahead; here the reader is assumed to stay put
				// until that work is done.
				while renderer.prewarm(&latest, &view, Duration::from_millis(2))
				{
				}
			}
			let frame_before = renderer.raster_stats().rasterized;
			let start = Instant::now();
			let submission = renderer.render(&latest, &view, &[], &target)?;
			in_frame += renderer.raster_stats().rasterized - frame_before;
			prepare.push(start.elapsed().as_secs_f64() * 1000.0);
			renderer.wait(Some(submission))?;
			total.push(start.elapsed().as_secs_f64() * 1000.0);
		}
		let after = renderer.raster_stats();
		passes.push(ScrollPass {
			prepare_ms: percentiles(prepare),
			over_8_33_ms: total.iter().filter(|t| **t > 8.33).count(),
			over_16_7_ms: total.iter().filter(|t| **t > 16.7).count(),
			total_ms: percentiles(total),
			rasterized: after.rasterized - before.rasterized,
			in_frame,
			atlas_resets: after.resets - before.resets,
			atlas_fill_percent: after.fill_percent,
			atlas_entries: after.entries,
		});
	}
	let scroll_pacing = ScrollPacing {
		frames: offsets.len(),
		cold: passes.remove(0),
		warm: passes.remove(0),
		prewarmed: passes.remove(0),
	};
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
		scroll: scroll_pacing,
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
	crate::logging::report(format_args!("{json}"));
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	#[ignore = "requires a GPU"]
	fn report_separates_submission_from_completed_gpu_wait() -> Result<()> {
		let dir = tempfile::tempdir()?;
		let input = dir.path().join("sample.md");
		let output = dir.path().join("timing.json");
		fs::write(
			&input,
			"# Timing\n\nA paragraph with **bold** text and $x^2$.",
		)?;
		run(
			&input,
			Some(&output),
			800,
			600,
			1.0,
			Theme::Light,
			3,
			LayoutOptions {
				fonts: crate::test_support::fonts(),
				..Default::default()
			},
			true,
		)?;
		let report: serde_json::Value =
			serde_json::from_slice(&fs::read(output)?)?;
		assert_eq!(report["cached_samples"].as_array().unwrap().len(), 3);
		for timing in std::iter::once(&report["first_open"])
			.chain(report["full_layout_samples"].as_array().unwrap())
			.chain(report["cached_samples"].as_array().unwrap())
		{
			let combined =
				timing["gpu_prepare_and_complete_ms"].as_f64().unwrap();
			let submit = timing["gpu_prepare_and_submit_ms"].as_f64().unwrap();
			let wait = timing["gpu_wait_ms"].as_f64().unwrap();
			assert!(submit > 0.0 && wait > 0.0);
			assert!(combined >= submit + wait);
			assert!(timing["total_ms"].as_f64().unwrap() >= combined);
		}
		Ok(())
	}

	#[test]
	#[ignore = "requires a GPU"]
	fn a_document_with_no_geometry_still_measures_its_opening_frame()
	-> Result<()> {
		let dir = tempfile::tempdir()?;
		let input = dir.path().join("empty.md");
		let output = dir.path().join("timing.json");
		fs::write(&input, "")?;
		run(
			&input,
			Some(&output),
			800,
			600,
			1.0,
			Theme::Light,
			1,
			LayoutOptions {
				fonts: crate::test_support::fonts(),
				..Default::default()
			},
			true,
		)?;
		let report: serde_json::Value =
			serde_json::from_slice(&fs::read(output)?)?;
		// No height to walk means one sample, not none: an empty vector used
		// to reach the percentile helper and panic.
		assert_eq!(report["scroll"]["frames"].as_u64(), Some(1));
		assert_eq!(
			report["scroll"]["cold"]["prepare_ms"]["count"].as_u64(),
			Some(1)
		);
		Ok(())
	}
}
