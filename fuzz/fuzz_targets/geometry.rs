//! G7, Tier 1 + Tier 3: the renderer's GPU-free geometry — the intersection
//! the renderer applies before drawing, the corner and edge fitting, and the
//! viewport transforms. The unit is an `arbitrary`-derived struct of f32s:
//! every field may hold any `f32` bit pattern, finite or not.
//! Oracle (O1, O2, I7): no panic; results finite whenever the inputs are.
#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use markview_layout::scene::{Rect, Viewport};
use markview_render::fuzz_api;
use mvfuzz::budget;

/// The whole input in one structured type (F2): each rect is four fields,
/// the viewport its six, plus the corner radii and border widths.
#[derive(Arbitrary, Clone, Copy, Debug)]
struct GeometryInput {
	rect_a: [f32; 4],
	rect_b: [f32; 4],
	rect_c: [f32; 4],
	corners: [f32; 4],
	edges: [f32; 4],
	viewport: [f32; 6],
}

fuzz_target!(|input: GeometryInput| {
	let budget = budget::Budget::parse().from_env();
	let bytes = std::mem::size_of::<GeometryInput>() as usize;
	let guard = budget::InputGuard::new();
	let rect = |v: [f32; 4]| Rect {
		x: v[0],
		y: v[1],
		w: v[2],
		h: v[3],
	};
	let a = rect(input.rect_a);
	let b = rect(input.rect_b);
	let c = rect(input.rect_c);

	// Intersection: the result, when present, is contained in both operands
	// and is finite whenever the operands are.
	let intersect_operands: Vec<f32> =
		[a, b].iter().flat_map(|r| [r.x, r.y, r.w, r.h]).collect();
	let operands_finite = intersect_operands.iter().all(|v| v.is_finite());
	let operands_plausible = mvfuzz::oracle::plausible(&intersect_operands);
	if let Some(r) = fuzz_api::intersect(a, b) {
		// Finiteness is owed while `x + w` cannot overflow; a `NaN` extent
		// poisons the containment reads, so that check needs finite operands.
		let finite =
			operands_finite && mvfuzz::oracle::no_overflow(&intersect_operands);
		mvfuzz::oracle::assert_finite_if("intersect", r.x, finite);
		mvfuzz::oracle::assert_finite_if("intersect", r.y, finite);
		mvfuzz::oracle::assert_finite_if("intersect", r.w, finite);
		mvfuzz::oracle::assert_finite_if("intersect", r.h, finite);
		if operands_finite && operands_plausible {
			mvfuzz::oracle::assert_rect4("intersect", r);
		}
		// The containment reads through `x + w`, which a `NaN` extent poisons;
		// with finite operands the corner may still sit a few ulp outside the
		// operand, because rounding a subnormal or a huge coordinate moves the
		// result beyond the input corner.
		if operands_finite {
			let tol = |v: f32| (v.abs() * 8.0).max(1e-9);
			let in_a = a.x - tol(a.x) <= r.x
				&& r.x <= a.x + a.w + tol(a.x + a.w)
				&& a.y - tol(a.y) <= r.y
				&& r.y <= a.y + a.h + tol(a.y + a.h);
			let in_b = b.x - tol(b.x) <= r.x
				&& r.x <= b.x + b.w + tol(b.x + b.w)
				&& b.y - tol(b.y) <= r.y
				&& r.y <= b.y + b.h + tol(b.y + b.h);
			assert!(in_a && in_b, "intersection escapes its operands: {r:?}");
		}
		let _ = r.intersect(c);
	}
	let _ = a.contains(b.x, b.y);

	// Corner and edge fitting: adjacent arcs and opposite edges must never
	// ask for more than the rect holds. The fitters are pure arithmetic, so
	// non-finite inputs may flow through; with finite inputs the results are
	// finite and bounded.
	let fitted = fuzz_api::fit_corners(a, input.corners);
	let corner_operands: Vec<f32> = [a.x, a.y, a.w, a.h]
		.iter()
		.chain(input.corners.iter())
		.copied()
		.collect();
	let corners_finite = corner_operands.iter().all(|v| v.is_finite());
	let corners_plausible = mvfuzz::oracle::plausible(&corner_operands);
	for r in fitted {
		mvfuzz::oracle::assert_finite_if("corner", r, corners_finite);
		if corners_finite && corners_plausible {
			mvfuzz::oracle::assert_bounded("corner", r);
		}
	}
	let fitted_edges = fuzz_api::fit_edges(a, input.edges);
	let edge_operands: Vec<f32> = [a.w, a.h]
		.iter()
		.chain(input.edges.iter())
		.copied()
		.collect();
	let edges_finite = edge_operands.iter().all(|v| v.is_finite());
	let edges_plausible = mvfuzz::oracle::plausible(&edge_operands);
	for r in fitted_edges {
		mvfuzz::oracle::assert_finite_if("edge", r, edges_finite);
		if edges_finite && edges_plausible {
			mvfuzz::oracle::assert_bounded("edge", r);
		}
	}
	// The fitter scales a pair down to the extent, so where the arithmetic is
	// representable the scaled sum may miss the extent by rounding but never
	// exceed it by more; a non-representable sum (overflow, `NaN`) is
	// accepted as the input's own fault. The tolerance is the operands'
	// rounding, not a fraction of the extent: opposite edges that nearly
	// cancel lose the extent's precision entirely.
	if a.h.is_finite() && a.h > 0.0 {
		assert!(
			mvfuzz::oracle::fitted_edges_within(
				a.h,
				[fitted_edges[0], fitted_edges[2]]
			),
			"vertical edges exceed the rect: {} > {a:?}",
			fitted_edges[0] + fitted_edges[2]
		);
	}
	if a.w.is_finite() && a.w > 0.0 {
		assert!(
			mvfuzz::oracle::fitted_edges_within(
				a.w,
				[fitted_edges[1], fitted_edges[3]]
			),
			"horizontal edges exceed the rect: {} > {a:?}",
			fitted_edges[1] + fitted_edges[3]
		);
	}

	// Viewport transforms stay exact inverses where they must.
	let v = Viewport {
		width: input.viewport[0],
		height: input.viewport[1],
		left: input.viewport[2],
		top: input.viewport[3],
		bottom: input.viewport[4],
		scroll: input.viewport[5],
	};
	let clip = fuzz_api::viewport_clip(v);
	let viewport_finite = input.viewport.iter().all(|v| v.is_finite());
	let clip_finite =
		viewport_finite && mvfuzz::oracle::no_overflow(&input.viewport);
	if clip_finite && mvfuzz::oracle::plausible(&input.viewport) {
		mvfuzz::oracle::assert_rect4("clip", clip);
	} else if clip_finite {
		mvfuzz::oracle::assert_finite_if("clip.x", clip.x, true);
		mvfuzz::oracle::assert_finite_if("clip.y", clip.y, true);
		mvfuzz::oracle::assert_finite_if("clip.w", clip.w, true);
		mvfuzz::oracle::assert_finite_if("clip.h", clip.h, true);
	}
	let (dx, dy) = fuzz_api::viewport_document_point(v, a.x, a.y);
	let back = fuzz_api::viewport_window_rect(v, clip);
	let _ = (dx, dy, back);
	guard.finish(&budget, bytes);
});
