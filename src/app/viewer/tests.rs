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

/// Wheeling in asks for a raster the size of the picture on screen, which is
/// what the viewer then fits, so the loop from rect to raster and back has to
/// keep the proportions at every zoom level, past the cap included. Each level
/// feeds the raster back several times, the way redraws keep arriving while
/// the zoom stands still.
#[test]
fn zooming_past_the_raster_cap_keeps_the_diagrams_proportions() {
	let window = (1200., 800.);
	// A 4:3 diagram the page shows at 800x600 device pixels.
	let mut viewer = Viewer {
		src: "mermaid:x".into(),
		pixels: (800., 600.),
		scale: 2.,
		zoom: 1.,
		pan: (0., 0.),
		grab: None,
		pressed_at: None,
		dragged: false,
	};
	for zoom in [1., 2., 4., 8., 16.] {
		viewer.zoom = zoom;
		let mut last = (0, 0);
		for update in 0..5 {
			let rect = viewer.rect(window);
			assert!(
				(rect.w / rect.h - 4. / 3.).abs() < 0.01,
				"{rect:?} at zoom {zoom} update {update}"
			);
			// The renderer asks for the rect at the device scale, capped; the
			// viewer fits whatever raster comes back.
			let size =
				raster_size(rect.w * viewer.scale, rect.h * viewer.scale);
			// Wheeling in may sharpen the raster in doubling steps, but the
			// updates settle instead of changing it forever.
			if update > 2 {
				assert_eq!(size, last, "zoom {zoom} update {update}");
			}
			last = size;
			viewer.pixels = (size.0 as f32, size.1 as f32);
		}
	}
	// Wheeling back to the default size restores the proportions rather than
	// keeping the capped raster's.
	viewer.zoom = 1.;
	for update in 0..3 {
		let rect = viewer.rect(window);
		assert!(
			(rect.w / rect.h - 4. / 3.).abs() < 0.01,
			"{rect:?} update {update}"
		);
		let size = raster_size(rect.w * viewer.scale, rect.h * viewer.scale);
		viewer.pixels = (size.0 as f32, size.1 as f32);
	}
}

/// Repeated updates at a fixed zoom have to settle on one raster that keeps
/// the picture's proportions. The viewer fits whatever raster arrives and the
/// renderer asks for the fit back, so a sizing rule that shaves a sub-pixel
/// per round trip re-rasterizes forever and walks the aspect away: 150%
/// display scaling at 8x zoom used to shed a pixel of height per update.
#[test]
fn repeated_updates_at_a_fixed_zoom_settle_on_the_original_proportions() {
	for window in [(1200., 800.), (1280., 720.)] {
		let mut viewer = Viewer {
			src: "mermaid:x".into(),
			pixels: (800., 600.),
			scale: 1.5,
			zoom: 8.,
			pan: (0., 0.),
			grab: None,
			pressed_at: None,
			dragged: false,
		};
		let mut last = (0, 0);
		for update in 0..8 {
			let rect = viewer.rect(window);
			let size =
				raster_size(rect.w * viewer.scale, rect.h * viewer.scale);
			assert!(
				(size.0 as f32 / size.1 as f32 - 4. / 3.).abs() < 0.01,
				"{size:?} at {window:?} update {update}"
			);
			if update > 0 {
				assert_eq!(size, last, "{window:?} update {update}");
			}
			last = size;
			viewer.pixels = (size.0 as f32, size.1 as f32);
		}
		// Wheeling back out restores the proportions rather than keeping the
		// capped raster's, and settles there too.
		viewer.zoom = 1.;
		for update in 0..4 {
			let rect = viewer.rect(window);
			let size =
				raster_size(rect.w * viewer.scale, rect.h * viewer.scale);
			assert!(
				(size.0 as f32 / size.1 as f32 - 4. / 3.).abs() < 0.01,
				"{size:?} at {window:?} zoomed out update {update}"
			);
			if update > 1 {
				assert_eq!(size, last, "{window:?} zoomed out update {update}");
			}
			last = size;
			viewer.pixels = (size.0 as f32, size.1 as f32);
		}
	}
}
