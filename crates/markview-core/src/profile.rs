//! Opt-in, zero-cost-when-disabled stage timers for diagnostic breakdowns.
//!
//! The desktop reader never enables this. `MARKVIEW_PROFILE=1` in the
//! environment turns the probes into `Instant` deltas; the diagnostic
//! benchmark reads them back as a flat map. Probes are single relaxed atomic
//! loads when the profiler is off, so the production path keeps its timing.
use std::{
	collections::HashMap,
	sync::atomic::{AtomicBool, AtomicU64, Ordering},
};
use web_time::Instant;

/// Layout sub-stages that the diagnostic `--bench` breakdown reports.
///
/// Every span is inclusive: nested probes are contained in their parent, so
/// `layout.rich_ms` covers `layout.units_ms` and `layout.line_clusters_ms`,
/// and `layout.shape_clusters_ms` is included in both shaping callers.
#[derive(Clone, Copy, Debug)]
pub enum Stage {
	Highlights,
	Blocks,
	Rich,
	FontChoose,
	FontResolve,
	ShapeBuild,
	Prepare,
	Units,
	ShapeClusters,
	LineBreak,
	LineClusters,
	Math,
}

impl Stage {
	fn index(self) -> usize {
		self as usize
	}
	fn label(self) -> &'static str {
		match self {
			Self::Highlights => "layout.highlights_ms",
			Self::Blocks => "layout.blocks_ms",
			Self::Rich => "layout.rich_ms",
			Self::FontChoose => "layout.font_choose_ms",
			Self::FontResolve => "layout.font_resolve_ms",
			Self::ShapeBuild => "layout.shape_build_ms",
			Self::Prepare => "layout.prepare_ms",
			Self::Units => "layout.units_ms",
			Self::ShapeClusters => "layout.shape_clusters_ms",
			Self::LineBreak => "layout.line_break_ms",
			Self::LineClusters => "layout.line_clusters_ms",
			Self::Math => "layout.math_layout_ms",
		}
	}
}

const LABELS: [Stage; 12] = [
	Stage::Highlights,
	Stage::Blocks,
	Stage::Rich,
	Stage::FontChoose,
	Stage::FontResolve,
	Stage::ShapeBuild,
	Stage::Prepare,
	Stage::Units,
	Stage::ShapeClusters,
	Stage::LineBreak,
	Stage::LineClusters,
	Stage::Math,
];
const STAGES: usize = LABELS.len();
static ENABLED: AtomicBool = AtomicBool::new(false);
static NANOS: [AtomicU64; STAGES] = [const { AtomicU64::new(0) }; STAGES];

/// Turn the probes on. Called once before benchmarking.
pub fn enable() {
	ENABLED.store(true, Ordering::Relaxed);
}

/// Whether `MARKVIEW_PROFILE=1` requested sub-stage timings.
pub fn requested() -> bool {
	std::env::var_os("MARKVIEW_PROFILE").is_some_and(|v| v == "1")
}

/// Time `body` as `stage` when the profiler is enabled.
#[inline]
pub(crate) fn measure<T>(stage: Stage, body: impl FnOnce() -> T) -> T {
	if !ENABLED.load(Ordering::Relaxed) {
		return body();
	}
	let start = Instant::now();
	let out = body();
	NANOS[stage.index()]
		.fetch_add(start.elapsed().as_nanos() as u64, Ordering::Relaxed);
	out
}

/// Time `body` as an inclusive span containing smaller probes.
#[inline]
pub fn span<T>(stage: Stage, body: impl FnOnce() -> T) -> T {
	measure(stage, body)
}

/// Read the accumulated probe totals since the last [`reset`].
pub fn totals() -> HashMap<&'static str, f64> {
	let mut out = HashMap::new();
	for stage in LABELS {
		let nanos = NANOS[stage.index()].load(Ordering::Relaxed);
		out.insert(stage.label(), nanos as f64 / 1e6);
	}
	out
}

/// Clear every accumulator.
pub fn reset() {
	for counter in &NANOS {
		counter.store(0, Ordering::Relaxed);
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn disabled_probes_do_not_accumulate() {
		reset();
		let before = totals();
		let value = measure(Stage::Prepare, || 7);
		assert_eq!(value, 7);
		assert_eq!(before["layout.prepare_ms"], totals()["layout.prepare_ms"]);
	}
}
