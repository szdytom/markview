use super::*;
use markview_layout::image::raster_size;

#[test]
fn viewer_fits_without_upscaling_and_zoom_keeps_the_pointer_spot() {
	let window = (1200., 800.);
	// A tall picture fits the window; a native-size cap keeps small pictures
	// from upscaling past the pixels the rasterizer produced.
	let mut viewer = Viewer {
		src: "mermaid:x".into(),
		pixels: (1000., 4000.),
		intrinsic: None,
		provisional: false,
		scale: 2.,
		zoom: 1.,
		pan: (0., 0.),
		grab: None,
		pressed_at: None,
		dragged: false,
	};
	let rect = viewer.rect(window);
	// The window fit (0.188) is below the native cap (0.5), so the picture
	// fills the window's content height and stays sharp.
	assert!((rect.h - 752.).abs() < 0.5, "{rect:?}");
	assert_eq!(rect.w, rect.h / 4., "the aspect survives fitting");
	for pixels in [(1., 1000.), (4000., 1.)] {
		viewer.pixels = pixels;
		let rect = viewer.rect(window);
		assert!(
			rect.w <= window.0 - VIEWER_MARGIN
				&& rect.h <= window.1 - VIEWER_MARGIN,
			"thin images fit too: {rect:?}"
		);
		assert!(rect.w >= 1. && rect.h >= 1.);
	}
	// A small picture stays at its native logical size rather than blurring.
	viewer.pixels = (300., 200.);
	let rect = viewer.rect(window);
	assert!((rect.w - 150.).abs() < 0.5, "{rect:?}");

	// Zooming about a point keeps that point over the same picture spot.
	// The picture is wider than the window, so both axes can pan and the
	// invariant holds on both.
	viewer.pixels = (4000., 1000.);
	viewer.scale = 1.;
	let spot = (300., 400.);
	let before = viewer.rect(window);
	let covered = (
		(spot.0 - before.x) / before.w,
		(spot.1 - before.y) / before.h,
	);
	viewer.zoom_at(2., spot, window);
	let after = viewer.rect(window);
	let now = ((spot.0 - after.x) / after.w, (spot.1 - after.y) / after.h);
	assert!((covered.0 - now.0).abs() < 0.01);
	assert!((covered.1 - now.1).abs() < 0.01);
	assert_eq!(after.w, before.w * 2.);

	// A pan cannot push the picture out of the window, and a picture smaller
	// than the window stays centred.
	viewer.pan_by((10_000., 10_000.), window);
	let rect = viewer.rect(window);
	assert!(rect.x + rect.w > 0. && rect.x < window.0);
	viewer.zoom = 1.;
	viewer.pixels = (300., 200.);
	viewer.pan_by((10_000., 10_000.), window);
	assert_eq!(viewer.pan, (0., 0.));

	// A press that never moved is a click, not a pan.
	assert!(!viewer.finish_press());
	viewer.begin_press((4., 4.));
	assert!(viewer.finish_press());
	viewer.begin_press((4., 4.));
	viewer.move_pointer((10., 4.), window);
	viewer.move_pointer((4., 4.), window);
	assert!(!viewer.finish_press());
}

#[test]
fn pinch_clamps_after_combining_zoom_and_midpoint_pan() {
	let window = (1200., 800.);
	let mut viewer =
		Viewer::new("picture".into(), (400., 400.), None, true, 1.);
	viewer.zoom = 4.;
	viewer.pan = (150., 0.);
	viewer.pinch(0.8, (600., 400.), (-100., 0.), window);
	assert_eq!(viewer.pan, (20., 0.));
	assert_eq!(viewer.rect(window).x, -20.);
}

/// Wheeling in asks for a raster the size of the picture on screen. The
/// viewer must not fit that raster back: otherwise every sharper copy moves
/// the picture, and wheeling out cannot return to the size it opened at.
#[test]
fn a_sharper_vector_raster_never_moves_the_fitted_picture() {
	let window = (1200., 800.);
	let scale = 2.;
	let intrinsic = Some((800, 600));
	let mut viewer =
		Viewer::new("mermaid:x".into(), (800., 600.), intrinsic, true, scale);
	let fitted = viewer.rect(window);
	assert_eq!((fitted.w, fitted.h), (400., 300.));

	viewer.zoom_at(2., (600., 400.), window);
	let zoomed = viewer.rect(window);
	assert_eq!((zoomed.w, zoomed.h), (800., 600.));
	for _ in 0..5 {
		let size = raster_size(zoomed.w * scale, zoomed.h * scale);
		viewer.refresh(
			Some((size.0 as f32, size.1 as f32)),
			intrinsic,
			true,
			scale,
			window,
		);
		let rect = viewer.rect(window);
		assert_eq!((rect.w, rect.h), (zoomed.w, zoomed.h));
	}

	viewer.zoom_at(0.5, (600., 400.), window);
	for _ in 0..5 {
		let rect = viewer.rect(window);
		let size = raster_size(rect.w * scale, rect.h * scale);
		viewer.refresh(
			Some((size.0 as f32, size.1 as f32)),
			intrinsic,
			true,
			scale,
			window,
		);
		let rect = viewer.rect(window);
		assert!((rect.w - fitted.w).abs() < 0.01);
		assert!((rect.h - fitted.h).abs() < 0.01);
	}
}

#[test]
fn a_first_decoded_raster_sets_the_fit_and_later_sharpening_does_not() {
	let window = (1200., 800.);
	let intrinsic = Some((400, 200));
	let mut viewer =
		Viewer::new("vector:x".into(), (400., 200.), intrinsic, false, 1.);
	assert_eq!((viewer.rect(window).w, viewer.rect(window).h), (400., 200.));

	viewer.refresh(Some((800., 400.)), intrinsic, true, 1., window);
	assert_eq!((viewer.rect(window).w, viewer.rect(window).h), (800., 400.));
	viewer.refresh(Some((1600., 800.)), intrinsic, true, 1., window);
	assert_eq!((viewer.rect(window).w, viewer.rect(window).h), (800., 400.));

	// A real content change carries a new intrinsic size and may move the fit.
	viewer.refresh(Some((600., 300.)), Some((300, 150)), true, 1., window);
	assert_eq!((viewer.rect(window).w, viewer.rect(window).h), (600., 300.));
}
