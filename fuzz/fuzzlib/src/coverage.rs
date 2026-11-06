//! Path-hit counters for the integration targets.
//!
//! These report which semantic paths a campaign actually reached, rather than
//! inferring coverage from execution speed. Counters are process-wide atomics,
//! so libFuzzer's in-process executions accumulate; printing is periodic to
//! keep per-input logs out of the campaign.
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug)]
#[repr(usize)]
pub enum Hit {
	WebGenerated,
	WebDirectGrid,
	WebCellGeometry,
	WebSpanLayout,
	WebMultipage,
	WebReadingProjection,
	WebOverflow,
	ReadingHit,
	ReadingSelection,
	ReadingReflow,
	ReadingSourceRoundtrip,
	ReadingBoundary,
	HistoryState,
	HighlightSchedule,
	HighlightConverged,
	ImageDecoded,
	ImageRejected,
	ImageSvg,
	ImagePdf,
	ReparseStep,
	PaginatePages,
	InteractionHit,
	InteractionSelection,
	InteractionOverflow,
	InteractionScroll,
	InteractionPan,
	InteractionStep,
	ArchiveStep,
	ArchiveUnpacked,
	FontFaceValid,
	ShapingStructured,
	MathStructured,
}

const NAMES: [&str; 32] = [
	"web/generated",
	"web/direct-grid",
	"web/cell-geometry",
	"web/span-layout",
	"web/multipage",
	"web/reading-projection",
	"web/overflow",
	"reading/hit",
	"reading/selection",
	"reading/reflow",
	"reading/source-roundtrip",
	"reading/boundary",
	"history/state",
	"highlight/schedule",
	"highlight/converged",
	"image/decoded",
	"image/rejected",
	"image/svg",
	"image/pdf",
	"reparse/step",
	"paginate/pages",
	"interaction/hit",
	"interaction/selection",
	"interaction/overflow",
	"interaction/scroll",
	"interaction/pan",
	"interaction/step",
	"archive/step",
	"archive/unpacked",
	"font/face-valid",
	"shaping/structured",
	"math/structured",
];

static COUNTS: [AtomicU64; NAMES.len()] =
	[const { AtomicU64::new(0) }; NAMES.len()];
static EXECUTIONS: AtomicU64 = AtomicU64::new(0);

pub fn hit(hit: Hit) {
	COUNTS[hit as usize].fetch_add(1, Ordering::Relaxed);
}

/// Counts one target execution and prints the summary every thousand inputs.
pub fn tick() {
	if EXECUTIONS.fetch_add(1, Ordering::Relaxed) % 1000 == 999 {
		eprintln!("[mvfuzz coverage] {}", report());
	}
}

pub fn report() -> String {
	let counts = COUNTS
		.iter()
		.map(|count| count.load(Ordering::Relaxed))
		.collect::<Vec<_>>();
	NAMES
		.iter()
		.zip(counts)
		.filter(|(_, count)| *count > 0)
		.map(|(name, count)| format!("{name}={count}"))
		.collect::<Vec<_>>()
		.join(" ")
}
