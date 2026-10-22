//! The full-size image viewer an opened image floats in.
//!
//! Like a confirmation it owns input while it is up: a press pans, a wheel
//! zooms, and a click or Escape closes. `Model` owns the picture on show and
//! its zoom, pan and press; the rest of the application opens or closes it
//! with a `Message`, and drawing borrows it read-only.
use markview_ui_core::{Component, Invalidation, Update};

/// The margin the fitted picture keeps to the window's edges.
const VIEWER_MARGIN: f32 = 48.0;

/// The picture an opened image shows.
///
/// The picture is the page's own image texture named the way the document
/// names it, so a diagram shows the same drawing the page does, at whatever
/// resolution the demand has produced.
pub(crate) struct Viewer {
	/// The image's alias, as the page's `Draw::Image` spells it.
	src: String,
	/// The raster's pixel size, which caps how far fitting may upscale.
	pixels: (f32, f32),
	/// The window scale, which puts the pixels in logical units.
	scale: f32,
	/// Zoom over the fitted size, `1.0` showing the whole image.
	zoom: f32,
	/// The picture centre's offset from the window centre.
	pan: (f32, f32),
	/// The last pointer position while panning.
	grab: Option<(f32, f32)>,
	/// Where the press began, to tell a closing click from a pan.
	pressed_at: Option<(f32, f32)>,
	/// Whether this press has ever crossed the drag threshold.
	dragged: bool,
}

impl Viewer {
	fn new(src: String, pixels: (f32, f32), scale: f32) -> Self {
		Self {
			src,
			pixels,
			scale,
			zoom: 1.,
			pan: (0., 0.),
			grab: None,
			pressed_at: None,
			dragged: false,
		}
	}

	/// The image's alias, as the page's `Draw::Image` spells it.
	pub(crate) fn src(&self) -> &str {
		&self.src
	}

	/// The picture's displayed size at the current zoom.
	fn displayed(&self, window: (f32, f32)) -> (f32, f32) {
		// Fitting shows everything without upscaling past the pixels the
		// rasterizer produced, so the fitted size is never blurry.
		let fit = ((window.0 - VIEWER_MARGIN) / self.pixels.0.max(1.))
			.min((window.1 - VIEWER_MARGIN) / self.pixels.1.max(1.))
			.min(1. / self.scale.max(1.));
		(
			(self.pixels.0 * fit).max(1.) * self.zoom,
			(self.pixels.1 * fit).max(1.) * self.zoom,
		)
	}

	/// The picture's rect at the current zoom and pan.
	pub(crate) fn rect(
		&self,
		window: (f32, f32),
	) -> markview_layout::scene::Rect {
		let (w, h) = self.displayed(window);
		markview_layout::scene::Rect {
			x: (window.0 - w) / 2. + self.pan.0,
			y: (window.1 - h) / 2. + self.pan.1,
			w,
			h,
		}
	}

	/// Whether a grab is following the pointer.
	pub(crate) fn grabbing(&self) -> bool {
		self.grab.is_some()
	}

	/// Zooms by `factor` about `pointer`, which stays over the same spot.
	fn zoom_at(
		&mut self,
		factor: f32,
		pointer: (f32, f32),
		window: (f32, f32),
	) {
		let zoom = (self.zoom * factor).clamp(1., 8.);
		let ratio = zoom / self.zoom;
		if (ratio - 1.).abs() <= f32::EPSILON {
			return;
		}
		self.zoom = zoom;
		// The point under the pointer keeps its distance from the picture's
		// centre, which scales with the picture.
		let centre = (window.0 / 2., window.1 / 2.);
		self.pan = (
			pointer.0 - centre.0 - (pointer.0 - centre.0 - self.pan.0) * ratio,
			pointer.1 - centre.1 - (pointer.1 - centre.1 - self.pan.1) * ratio,
		);
		self.clamp_pan(window);
	}

	/// Pans by `delta`, keeping some of the picture inside the window.
	fn pan_by(&mut self, delta: (f32, f32), window: (f32, f32)) {
		self.pan.0 += delta.0;
		self.pan.1 += delta.1;
		self.clamp_pan(window);
	}

	/// Keeps the picture's centre within half a picture of the window's.
	fn clamp_pan(&mut self, window: (f32, f32)) {
		let (w, h) = self.displayed(window);
		self.pan.0 = if w >= window.0 {
			self.pan.0.clamp(-(w - window.0) / 2., (w - window.0) / 2.)
		} else {
			0.
		};
		self.pan.1 = if h >= window.1 {
			self.pan.1.clamp(-(h - window.1) / 2., (h - window.1) / 2.)
		} else {
			0.
		};
	}

	fn begin_press(&mut self, pointer: (f32, f32)) {
		self.grab = Some(pointer);
		self.pressed_at = Some(pointer);
		self.dragged = false;
	}

	fn move_pointer(&mut self, pointer: (f32, f32), window: (f32, f32)) {
		if let Some((gx, gy)) = self.grab {
			let (px, py) = self.pressed_at.unwrap();
			self.dragged |= (pointer.0 - px).hypot(pointer.1 - py) > 3.;
			self.pan_by((pointer.0 - gx, pointer.1 - gy), window);
			self.grab = Some(pointer);
		}
	}

	fn cancel_press(&mut self) {
		self.grab = None;
		self.pressed_at = None;
		self.dragged = false;
	}

	/// Returns whether a completed press was a closing click.
	fn finish_press(&mut self) -> bool {
		let clicked = self.pressed_at.is_some() && !self.dragged;
		self.cancel_press();
		clicked
	}

	#[cfg(test)]
	pub(crate) fn pan(&self) -> (f32, f32) {
		self.pan
	}

	#[cfg(test)]
	pub(crate) fn pressing(&self) -> bool {
		self.pressed_at.is_some()
	}
}

/// The viewer's own state: the picture on show, if any.
#[derive(Default)]
pub(crate) struct Model {
	current: Option<Viewer>,
}

/// The viewer's control-flow transitions.
pub(crate) enum Message {
	/// Show `src` over the window, fitted from `pixels` at `scale`.
	Open {
		src: String,
		pixels: (f32, f32),
		scale: f32,
	},
	/// Close the viewer, whatever it was showing.
	Close,
	/// Zoom by `factor` about `pointer`, which stays over the same spot.
	Zoom {
		factor: f32,
		pointer: (f32, f32),
		window: (f32, f32),
	},
	/// Follow the pointer, panning while a press is in flight.
	Pan {
		pointer: (f32, f32),
		window: (f32, f32),
	},
	/// Begin a press at `pointer`, which may become a pan or a click.
	Press { pointer: (f32, f32) },
	/// Finish the press; a press that never moved is a closing click.
	Release,
}

impl Component for Model {
	type Message = Message;
	type Effect = ();
	type View<'a> = Option<&'a Viewer>;

	fn view(&self) -> Option<&Viewer> {
		self.current.as_ref()
	}

	fn update(&mut self, message: Message) -> Update<()> {
		match message {
			Message::Open { src, pixels, scale } => {
				self.current = Some(Viewer::new(src, pixels, scale));
			}
			Message::Close => self.current = None,
			Message::Zoom {
				factor,
				pointer,
				window,
			} => {
				if let Some(viewer) = self.current.as_mut() {
					viewer.zoom_at(factor, pointer, window);
				}
			}
			Message::Pan { pointer, window } => {
				if let Some(viewer) = self.current.as_mut() {
					viewer.move_pointer(pointer, window);
				}
			}
			Message::Press { pointer } => {
				if let Some(viewer) = self.current.as_mut() {
					viewer.begin_press(pointer);
				}
			}
			Message::Release => {
				if let Some(viewer) = self.current.as_mut()
					&& viewer.finish_press()
				{
					self.current = None;
				}
			}
		}
		Update::new(Invalidation::Paint)
	}
}

impl Model {
	/// Sends one control-flow message.
	pub(crate) fn send(&mut self, message: Message) {
		self.update(message);
	}

	/// Whether the viewer currently owns input.
	pub(crate) fn is_open(&self) -> bool {
		self.current.is_some()
	}

	/// The picture on show, if any.
	pub(crate) fn current(&self) -> Option<&Viewer> {
		self.view()
	}

	/// Re-fits the picture after decoded pixels or the window scale arrive.
	pub(crate) fn refresh(
		&mut self,
		pixels: Option<(f32, f32)>,
		scale: f32,
		window: (f32, f32),
	) {
		let Some(viewer) = self.current.as_mut() else {
			return;
		};
		if let Some(pixels) = pixels {
			viewer.pixels = pixels;
		}
		viewer.scale = scale;
		viewer.pan_by((0., 0.), window);
	}

	/// Drops the press in flight without treating it as a click.
	pub(crate) fn cancel_press(&mut self) {
		if let Some(viewer) = self.current.as_mut() {
			viewer.cancel_press();
		}
	}
}

#[cfg(test)]
#[path = "viewer/tests.rs"]
mod tests;
