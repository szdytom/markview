//! Metric-focused latency and memory measurement.
//!
//! The layout worker, progressive publication, block cache, image scheduler and
//! GPU renderer are the production ones; only the presentation target is
//! offscreen. Window creation, chrome shaping and compositor presentation are
//! therefore excluded, so these numbers lower-bound the interactive frame.
//!
//! Two targets are measured:
//!
//! - **First frame**: process entry to the first rendered frame whose geometry
//!   covers the reader's viewport, with the same prefix-publication rules the
//!   window uses.
//! - **Edit latency**: a fully loaded document is edited on disk and the time
//!   to the first refreshed frame (and to the complete re-layout) is measured.
//!
//! Per-iteration RSS samples also expose long-term growth across reloads.
use crate::{
	benchmark::{Memory, memory},
	document,
	file::read_document,
	layout::{LayoutOptions, LayoutSnapshot},
	render::{Renderer, Theme, View},
	worker::{ReaderSnapshot, Request, Update, Worker},
};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::{
	collections::HashMap,
	fs,
	path::Path,
	sync::mpsc::{self, Receiver, RecvTimeoutError},
	time::{Duration, Instant},
};

/// Reading-area insets, mirroring `src/app.rs` so the measured viewport is the
/// one the reader actually lays out against.
const TOP: f32 = 40.0;
const BOTTOM: f32 = 28.0;
const CLIP_INSET: f32 = 10.0;
/// A very large edit measured while a highlight pass is also running can take
/// seconds; the timeout only prevents a hung benchmark, it is not a budget.
const RECV_TIMEOUT: Duration = Duration::from_secs(120);
/// The edit token, chosen so it cannot occur in the fixtures.
const TOKEN: &str = "Qx9";

fn millis(start: Instant) -> f64 {
	start.elapsed().as_secs_f64() * 1000.0
}

#[derive(Serialize, Clone, Copy, Default)]
struct Dist {
	count: usize,
	p50_ms: f64,
	p95_ms: f64,
	max_ms: f64,
}
fn distribution(mut values: Vec<f64>) -> Dist {
	values.retain(|v| v.is_finite());
	if values.is_empty() {
		return Dist::default();
	}
	values.sort_by(f64::total_cmp);
	Dist {
		count: values.len(),
		p50_ms: values[(values.len() - 1) / 2],
		p95_ms: values
			[((values.len() as f64 * 0.95).ceil() as usize).saturating_sub(1)],
		max_ms: *values.last().unwrap(),
	}
}

#[derive(Serialize, Default)]
struct ColdFirstFrame {
	/// The headline metric: process entry to the first readable frame.
	process_start_to_first_readable_frame_ms: f64,
	process_start_to_complete_ms: f64,
	/// Request submission to the first readable frame, including render+GPU.
	request_to_first_readable_frame_ms: f64,
	render_and_gpu_ms: f64,
	read_ms: f64,
	parse_ms: f64,
	layout_ms: f64,
	first_frame_blocks: usize,
	total_blocks: usize,
	reused_blocks: usize,
}

#[derive(Serialize, Default, Clone, Copy)]
struct EditSample {
	first_frame_ms: f64,
	render_and_gpu_ms: f64,
	complete_ms: f64,
	write_ms: f64,
	read_ms: f64,
	parse_ms: f64,
	layout_ms: f64,
	first_frame_blocks: usize,
	total_blocks: usize,
	reused_blocks: usize,
	/// Blocks reused by the complete snapshot, which shows whether the block
	/// cache retained the whole document.
	complete_reused_blocks: usize,
	complete_layout_ms: f64,
	rss_bytes: Option<u64>,
}

#[derive(Serialize, Default)]
struct EditScenario {
	iterations: usize,
	first_frame_ms: Dist,
	render_and_gpu_ms: Dist,
	complete_ms: Dist,
	write_ms: Dist,
	read_ms: Dist,
	parse_ms: Dist,
	layout_ms: Dist,
	first_frame_blocks_p50: f64,
	total_blocks: usize,
	reused_blocks_p50: f64,
	complete_reused_blocks_p50: f64,
	samples: Vec<EditSample>,
}

#[derive(Serialize, Default)]
struct MemoryTrend {
	after_init: Memory,
	after_cold_frame: Memory,
	after_full_load: Memory,
	after_edits: Memory,
	after_scroll: Memory,
	tracked_gpu_bytes_excluding_driver: u64,
	reading_text_index_bytes: usize,
	first_rss_bytes: Option<u64>,
	last_rss_bytes: Option<u64>,
	max_rss_bytes: Option<u64>,
	/// Least-squares RSS slope over the visible-edit iterations. A leak that
	/// allocates per reload shows up here even when the allocator keeps pages.
	rss_slope_bytes_per_iteration: Option<f64>,
}

#[derive(Serialize)]
struct Report {
	scope: &'static str,
	adapter: String,
	file: String,
	bytes: usize,
	content_hash: String,
	blocks: usize,
	physical_size: [u32; 2],
	scale: f32,
	column_width: f32,
	font_size: f32,
	viewport: f32,
	coverage: f32,
	initialization_ms: f64,
	edit_iterations: usize,
	cold: ColdFirstFrame,
	edits_top: EditScenario,
	edits_far: EditScenario,
	memory: MemoryTrend,
}

/// The reader accepts a partial snapshot only once it covers the viewport.
/// At scroll zero this is the same test `ReaderSession::can_display` applies.
fn displayable(reader: &ReaderSnapshot, viewport: f32) -> bool {
	reader.complete
		|| (!reader.layout.blocks.is_empty()
			&& reader.layout.height >= viewport)
}

fn submit(
	worker: &Worker,
	version: u64,
	content_version: u64,
	path: &Path,
	options: &LayoutOptions,
	coverage: f32,
	requested: Instant,
) {
	worker.submit(Request {
		version,
		content_version,
		path: path.to_path_buf(),
		options: options.clone(),
		requested,
		coverage,
		load_all_images: false,
	});
}

fn recv(rx: &Receiver<Update>) -> Result<Update> {
	match rx.recv_timeout(RECV_TIMEOUT) {
		Ok(update) => Ok(update),
		Err(RecvTimeoutError::Timeout) => {
			bail!("Layout worker produced no update within {RECV_TIMEOUT:?}")
		}
		Err(RecvTimeoutError::Disconnected) => {
			bail!("Layout worker stopped unexpectedly")
		}
	}
}

/// Discards publications left over from the previous request, such as a
/// highlight completion that re-ran the same version.
fn drain(rx: &Receiver<Update>) {
	while rx.try_recv().is_ok() {}
}

struct Bench {
	renderer: Renderer,
	texture: wgpu::Texture,
	horizontal: HashMap<(usize, usize), f32>,
	width: u32,
	height: u32,
	scale: f32,
	theme: Theme,
	column: f32,
	viewport: f32,
	coverage: f32,
}
impl Bench {
	fn render(&mut self, snapshot: &LayoutSnapshot, scroll: f32) -> Result<()> {
		let view = View {
			selection: None,
			hovered_link: None,
			held_overflow: None,
			hovered_overflow: None,
			revision: 0,
			width: self.width,
			height: self.height,
			scale: self.scale,
			scroll,
			left: ((self.width as f32 / self.scale - self.column) * 0.5)
				.max(16.0),
			top: TOP + CLIP_INSET,
			bottom: BOTTOM + CLIP_INSET,
			theme: self.theme,
			horizontal: &self.horizontal,
		};
		// The window also creates one view per acquired frame texture.
		let target = self.texture.create_view(&Default::default());
		let submission = self.renderer.render(snapshot, &view, &[], &target)?;
		self.renderer.wait(Some(submission))?;
		Ok(())
	}
}

fn top_offset(text: &str) -> usize {
	text.find("\n\n").map_or(0, |i| (i + 2).min(text.len()))
}
fn far_offset(text: &str) -> usize {
	text.rfind("\n\n")
		.map_or(text.len(), |i| (i + 2).min(text.len()))
}

/// The first edit iteration applies the token, the next removes it, so the
/// document does not grow and every iteration has the same work.
fn apply_edit(text: &mut String, at: usize) {
	let at = at.min(text.len());
	// `at + TOKEN.len()` need not be a boundary -- the byte after the token may
	// fall inside a multi-byte character -- so test the suffix instead of
	// slicing to a fixed width.
	let present = text.is_char_boundary(at) && text[at..].starts_with(TOKEN);
	if present {
		text.replace_range(at..at + TOKEN.len(), "");
	} else if text.is_char_boundary(at) {
		text.insert_str(at, TOKEN);
	}
}

/// A writable copy of `source` beside the original document.
///
/// Relative image URLs in the document resolve against its directory, so the
/// copy must live in the same directory the reader would use; a copy in the
/// system temporary directory would turn valid images into placeholders and
/// measure different work.
fn bench_copy(path: &Path, source: &str) -> Result<tempfile::NamedTempFile> {
	let parent = match path.parent() {
		Some(parent) if !parent.as_os_str().is_empty() => parent,
		_ => Path::new("."),
	};
	let file = tempfile::Builder::new()
		.prefix(".markview-latency-")
		.suffix(".md")
		.tempfile_in(parent)
		.with_context(|| {
			format!("Create a benchmark copy beside {}", path.display())
		})?;
	fs::write(file.path(), source)?;
	Ok(file)
}

#[expect(
	clippy::too_many_arguments,
	reason = "The scenario borrows the shared harness plus its own edit inputs"
)]
fn edit_scenario(
	bench: &mut Bench,
	worker: &Worker,
	rx: &Receiver<Update>,
	path: &Path,
	options: &LayoutOptions,
	mut text: String,
	offset: fn(&str) -> usize,
	iterations: usize,
	version: &mut u64,
	content_version: &mut u64,
) -> Result<EditScenario> {
	let mut scenario = EditScenario {
		iterations,
		..Default::default()
	};
	for _ in 0..iterations {
		let at = offset(&text);
		apply_edit(&mut text, at);
		let t = Instant::now();
		fs::write(path, &text)
			.with_context(|| format!("Write edit to {}", path.display()))?;
		let write_ms = millis(t);
		*version += 1;
		*content_version += 1;
		drain(rx);
		let start = Instant::now();
		submit(
			worker,
			*version,
			*content_version,
			path,
			options,
			bench.coverage,
			start,
		);
		let mut sample = EditSample {
			write_ms,
			..Default::default()
		};
		loop {
			let update = recv(rx)?;
			if update.version != *version {
				continue;
			}
			let reader = match update.result {
				Some(Ok(reader)) => reader,
				Some(Err(error)) => bail!("Edit reload failed: {error}"),
				None => continue,
			};
			if sample.first_frame_ms == 0.0
				&& displayable(&reader, bench.viewport)
			{
				let r = Instant::now();
				bench.render(&reader.layout, 0.0)?;
				sample.render_and_gpu_ms = millis(r);
				sample.first_frame_ms = millis(start);
				sample.read_ms = update.read_ms;
				sample.parse_ms = update.parse_ms;
				sample.layout_ms = update.layout_ms;
				sample.first_frame_blocks = reader.layout.blocks.len();
				sample.reused_blocks = reader.layout.reused;
			}
			if reader.complete {
				sample.complete_ms = millis(start);
				sample.total_blocks = reader.layout.blocks.len();
				sample.complete_reused_blocks = reader.layout.reused;
				sample.complete_layout_ms = update.layout_ms;
				sample.rss_bytes = memory().linux_rss_bytes;
				break;
			}
		}
		scenario.samples.push(sample);
	}
	let reuse = |s: &EditSample| s.reused_blocks as f64;
	let frame_blocks = |s: &EditSample| s.first_frame_blocks as f64;
	scenario.first_frame_ms = distribution(
		scenario.samples.iter().map(|s| s.first_frame_ms).collect(),
	);
	scenario.render_and_gpu_ms = distribution(
		scenario
			.samples
			.iter()
			.map(|s| s.render_and_gpu_ms)
			.collect(),
	);
	scenario.complete_ms =
		distribution(scenario.samples.iter().map(|s| s.complete_ms).collect());
	scenario.write_ms =
		distribution(scenario.samples.iter().map(|s| s.write_ms).collect());
	scenario.read_ms =
		distribution(scenario.samples.iter().map(|s| s.read_ms).collect());
	scenario.parse_ms =
		distribution(scenario.samples.iter().map(|s| s.parse_ms).collect());
	scenario.layout_ms =
		distribution(scenario.samples.iter().map(|s| s.layout_ms).collect());
	scenario.first_frame_blocks_p50 =
		median(scenario.samples.iter().map(frame_blocks).collect());
	scenario.reused_blocks_p50 =
		median(scenario.samples.iter().map(reuse).collect());
	scenario.complete_reused_blocks_p50 = median(
		scenario
			.samples
			.iter()
			.map(|s| s.complete_reused_blocks as f64)
			.collect(),
	);
	scenario.total_blocks =
		scenario.samples.last().map_or(0, |s| s.total_blocks);
	Ok(scenario)
}

fn median(mut values: Vec<f64>) -> f64 {
	values.retain(|v| v.is_finite());
	if values.is_empty() {
		return 0.0;
	}
	values.sort_by(f64::total_cmp);
	values[(values.len() - 1) / 2]
}

fn rss_slope(samples: &[EditSample]) -> Option<f64> {
	let points: Vec<(f64, f64)> = samples
		.iter()
		.enumerate()
		.filter_map(|(i, s)| s.rss_bytes.map(|rss| (i as f64, rss as f64)))
		.collect();
	if points.len() < 3 {
		return None;
	}
	let n = points.len() as f64;
	let sx: f64 = points.iter().map(|p| p.0).sum();
	let sy: f64 = points.iter().map(|p| p.1).sum();
	let sxx: f64 = points.iter().map(|p| p.0 * p.0).sum();
	let sxy: f64 = points.iter().map(|p| p.0 * p.1).sum();
	let denominator = n * sxx - sx * sx;
	(denominator.abs() > f64::EPSILON)
		.then(|| (n * sxy - sx * sy) / denominator)
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
	let process_start = crate::process_started();
	// The worker starts before the renderer, as it does in the window: its
	// font discovery then overlaps renderer initialization instead of
	// following it, and the measured first frame matches the reader.
	let (tx, rx) = mpsc::channel::<Update>();
	let worker =
		Worker::with_images(offline, options.fonts.clone(), move |update| {
			let _ = tx.send(update);
		});
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	renderer.set_stylesheet(options.stylesheet.clone());
	let after_init = memory();
	let initialization_ms = millis(process_start);
	let texture = renderer.offscreen(width, height);
	let horizontal = HashMap::new();
	let viewport =
		(height as f32 / scale - (TOP + CLIP_INSET) - (BOTTOM + CLIP_INSET))
			.max(1.0);
	// The reader requests its viewport plus half a viewport of prefetch.
	let coverage = viewport * 1.5;

	let source = read_document(path)?;
	let bytes = source.len();
	let content_hash = format!("{:016x}", document::fingerprint(&source));

	// Edits are written beside the document so relative image URLs keep the
	// same base directory, and the fixture itself is never touched.
	let work = bench_copy(path, &source)?;

	let mut bench = Bench {
		renderer,
		texture,
		horizontal,
		width,
		height,
		scale,
		theme,
		column: options.width,
		viewport,
		coverage,
	};

	let mut version = 0u64;
	let mut content_version = 1u64;

	// ---- Cold open: process entry to the first readable GPU frame. ----
	version += 1;
	let request_start = Instant::now();
	submit(
		&worker,
		version,
		content_version,
		work.path(),
		&options,
		coverage,
		request_start,
	);
	let mut cold = ColdFirstFrame::default();
	let last_snapshot = loop {
		let update = recv(&rx)?;
		if update.version != version {
			continue;
		}
		let reader = match update.result {
			Some(Ok(reader)) => reader,
			Some(Err(error)) => bail!("Cold open failed: {error}"),
			None => continue,
		};
		if cold.process_start_to_first_readable_frame_ms == 0.0
			&& displayable(&reader, viewport)
		{
			let r = Instant::now();
			bench.render(&reader.layout, 0.0)?;
			cold.render_and_gpu_ms = millis(r);
			cold.request_to_first_readable_frame_ms = millis(request_start);
			cold.process_start_to_first_readable_frame_ms =
				millis(process_start);
			cold.read_ms = update.read_ms;
			cold.parse_ms = update.parse_ms;
			cold.layout_ms = update.layout_ms;
			cold.first_frame_blocks = reader.layout.blocks.len();
			cold.reused_blocks = reader.layout.reused;
		}
		if reader.complete {
			cold.process_start_to_complete_ms = millis(process_start);
			cold.total_blocks = reader.layout.blocks.len();
			break reader.layout;
		}
	};
	let after_cold_frame = memory();

	// Full layout is the state an edit starts from; the cold loop already
	// reached it, so reuse that snapshot rather than laying out again.
	let after_full_load = memory();

	// Bounded edit work: a full re-layout per iteration is O(document), so cap
	// the sample count by bytes while keeping at least a few samples.
	let edit_iterations =
		iterations.min((8_000_000 / bytes.max(1)).clamp(3, 1_000));

	fs::write(work.path(), &source)?;
	let edits_top = edit_scenario(
		&mut bench,
		&worker,
		&rx,
		work.path(),
		&options,
		source.clone(),
		top_offset,
		edit_iterations,
		&mut version,
		&mut content_version,
	)?;
	fs::write(work.path(), &source)?;
	let edits_far = edit_scenario(
		&mut bench,
		&worker,
		&rx,
		work.path(),
		&options,
		source.clone(),
		far_offset,
		edit_iterations,
		&mut version,
		&mut content_version,
	)?;
	let after_edits = memory();

	// Scroll through the document, sampling memory the way the reader would.
	let mut scroll = 0.0;
	while scroll < last_snapshot.height {
		bench.render(&last_snapshot, scroll)?;
		scroll += (viewport - 48.0).max(1.0);
	}
	let after_scroll = memory();

	let memory = MemoryTrend {
		after_init,
		after_cold_frame,
		after_full_load,
		after_edits,
		after_scroll,
		tracked_gpu_bytes_excluding_driver: bench.renderer.gpu_bytes()
			+ (width as u64 * height as u64 * 4),
		reading_text_index_bytes: last_snapshot.text_index_bytes(),
		first_rss_bytes: edits_top.samples.first().and_then(|s| s.rss_bytes),
		last_rss_bytes: edits_top.samples.last().and_then(|s| s.rss_bytes),
		max_rss_bytes: edits_top
			.samples
			.iter()
			.filter_map(|s| s.rss_bytes)
			.max(),
		rss_slope_bytes_per_iteration: rss_slope(&edits_top.samples),
	};

	let report = Report {
		scope: "Release-mode target. Real layout worker, progressive prefix \
			publication, block cache, image scheduler and GPU renderer; offscreen \
			presentation, so window creation, chrome shaping and compositor \
			presentation are excluded. Edit latency starts when the edited bytes \
			have been written, so the 30 ms file-watch debounce is excluded. \
			First open has cold document/glyph/math caches; OS file cache is not \
			flushed.",
		adapter: bench.renderer.adapter_name.clone(),
		file: path.display().to_string(),
		bytes,
		content_hash,
		blocks: cold.total_blocks,
		physical_size: [width, height],
		scale,
		column_width: options.width,
		font_size: options.font_size,
		viewport,
		coverage,
		initialization_ms,
		edit_iterations,
		cold,
		edits_top,
		edits_far,
		memory,
	};
	let json = serde_json::to_string_pretty(&report)?;
	if let Some(output) = output {
		if let Some(parent) =
			output.parent().filter(|p| !p.as_os_str().is_empty())
		{
			fs::create_dir_all(parent)?;
		}
		fs::write(output, &json)
			.with_context(|| format!("Write {}", output.display()))?;
	}
	crate::logging::report(format_args!("{json}"));
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn an_edit_token_after_a_multibyte_character_toggles() {
		// `top_offset` lands on a character boundary, but one byte past it is
		// inside the second `é`; a fixed-width slice used to panic there.
		let mut text = "# Heading\n\néé paragraph\n".to_string();
		let at = top_offset(&text);
		apply_edit(&mut text, at);
		assert!(text[at..].starts_with(TOKEN));
		apply_edit(&mut text, at);
		assert_eq!(text, "# Heading\n\néé paragraph\n");
	}

	#[test]
	fn a_benchmark_copy_stays_beside_its_document() {
		let dir = tempfile::tempdir().unwrap();
		let original = dir.path().join("document.md");
		let source = "# Heading\n\n![x](images/pic.png)\n";
		fs::write(&original, source).unwrap();
		let copy = bench_copy(&original, source).unwrap();
		assert_eq!(copy.path().parent(), original.parent());
		assert_eq!(fs::read_to_string(copy.path()).unwrap(), source);
	}
}
