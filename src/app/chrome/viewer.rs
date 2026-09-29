//! The full-size image viewer: a scrim over the window and the picture.
use crate::state::Viewer;
use markview_core::scene::{Draw, Paint, Rect};

/// The version the page currently knows the picture by, so the viewer shows
/// the same raster the images pipeline has produced — and a re-raster for a
/// bigger demand arrives here without a reflow.
fn version(snapshot: &crate::layout::LayoutSnapshot, src: &str) -> u64 {
	snapshot
		.images
		.entries
		.get(src)
		.map(|info| info.version)
		.unwrap_or(0)
}

pub(in crate::app) fn draw_viewer(
	viewer: &Viewer,
	snapshot: &crate::layout::LayoutSnapshot,
	window: (f32, f32),
) -> Vec<Draw> {
	vec![
		Draw::Rect(
			Rect {
				x: 0.0,
				y: 0.0,
				w: window.0,
				h: window.1,
			},
			Paint::Scrim,
		),
		Draw::Image {
			src: viewer.src.clone(),
			version: version(snapshot, &viewer.src),
			rect: viewer.rect(window),
			title: String::new(),
		},
	]
}
