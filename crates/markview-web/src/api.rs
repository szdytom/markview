//! The `Markview` handle: state, update loop and frames.
//!
//! Everything here is driven from JavaScript, one call at a time, on the
//! browser's own thread: there is nowhere to block on the GPU and nowhere to
//! lay a document out but here.

use crate::{
	fonts,
	images::Images,
	selection::{Pointer, Reading},
	state::{Published, SelectionLength},
};
use markview_core::{
	document::{Document, parse},
	fonts::FontConfig,
	image::Pixels,
	layout::{LayoutEngine, LayoutOptions, ProgressiveLayout, Viewport},
	scene::Rect,
	source::{SourceIndex, SourcePosition, byte_to_utf16, utf16_to_byte},
	style::Stylesheet,
};
use markview_render::{FrameStatus, Renderer, SurfaceSource, Theme, View};
use markview_selection::{
	DocumentInteraction, Horizontal, Host, Hover, Modifiers, Point,
	ScrollBounds, ScrollState, Selection,
};
use serde::Deserialize;
use std::{sync::Arc, time::Duration};
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;
use web_time::Instant;

/// Logical pixels kept clear above and below the reading column.
const INSET: f32 = 10.0;
/// Logical pixels kept clear beside the reading column. The column narrows so
/// that this margin survives, because nothing here can pan sideways to text a
/// too-wide column would clip.
const MARGIN: f32 = 20.0;
/// The largest canvas edge the first surface configure may report.
///
/// The device limit is only knowable after the device exists, so the
/// configure inside `Renderer::new` keeps to the smallest limit Markview ever
/// asks a device for, which is the GL backend's. `create` resizes to the real
/// limit as soon as it can read it.
const INITIAL_LIMIT: u32 =
	wgpu::Limits::downlevel_webgl2_defaults().max_texture_dimension_2d;
/// The largest logical canvas edge the layout viewport accepts.
const MAX_LOGICAL: f32 = 8_192.0;
/// The largest device pixel ratio the viewport accepts.
const MAX_DPR: f32 = 8.0;
/// The longest layout budget one `stepUpdate` call may be asked for.
const MAX_STEP_MS: f64 = 60_000.0;

/// Installs host font bytes for subsequently created readers.
#[wasm_bindgen(js_name = configureFonts)]
pub fn configure_fonts(faces: js_sys::Array) -> Result<(), JsValue> {
	let faces = faces
		.iter()
		.map(|face| {
			face.dyn_into::<js_sys::Uint8Array>()
				.map(|data| data.to_vec())
				.map_err(|_| fail("host fonts must be Uint8Array values"))
		})
		.collect::<Result<Vec<_>, _>>()?;
	fonts::install(faces).map_err(fail)
}

/// Builds a handle that draws into `canvas`, importing `config_json` when the
/// page passes one.
#[wasm_bindgen]
pub async fn create(
	canvas: HtmlCanvasElement,
	config_json: Option<String>,
) -> Result<Markview, JsValue> {
	console_error_panic_hook::set_once();
	let config = Config::parse(config_json.as_deref())?;
	let fonts = fonts::config();
	let dpr = ratio(web_sys::window().map_or(1.0, |w| w.device_pixel_ratio()));
	// The page may not have sized the canvas yet, so start from its CSS box
	// when there is one and let `resize` keep it current afterwards.
	let rect = canvas.get_bounding_client_rect();
	let logical = if rect.width() > 0.0 && rect.height() > 0.0 {
		(logical_edge(rect.width()), logical_edge(rect.height()))
	} else {
		(
			logical_edge(f64::from(canvas.width().max(1))),
			logical_edge(f64::from(canvas.height().max(1))),
		)
	};
	// The device limit is only knowable after the device exists, so the
	// renderer is created from the backing store the page already has — the
	// HTML default for an untouched canvas — and the canvas is sized under
	// the limit only once it can be read.
	let mut renderer = Renderer::new(Some(Box::new(Present {
		canvas: canvas.clone(),
	})))
	.await
	.map_err(|error| fail(format!("{error:#}")))?;
	let limit = renderer.max_texture_dimension_2d();
	let scale = effective_scale(logical, dpr, limit);
	let (width, height) = size_canvas(&canvas, logical, scale, limit);
	renderer.resize(width, height);
	let mut options = config.options(fonts);
	options.width = column_width(config.width, logical.0);
	let document = Arc::new(parse(""));
	Ok(Markview {
		canvas,
		renderer,
		engine: LayoutEngine::new(),
		options,
		config,
		published: Published::default(),
		pointer: Pointer::default(),
		published_document: document.clone(),
		source_index: SourceIndex::new(&document),
		source_text: document.source.clone(),
		document_version: 0,
		document,
		pending: None,
		images: Images::default(),
		images_dirty: false,
		cursor: None,
		logical,
		dpr,
		scale,
		scrolling: ScrollState::default(),
		horizontal: Horizontal::default(),
		internal_scroll: true,
		images_clickable: false,
		pressed_image: None,
		overflow_drag: None,
		pending_anchor: None,
		jump_origin: None,
		parse_ms: 0.0,
		layout_ms: 0.0,
		frame_ms: 0.0,
		frames: 0,
		glyphs: 0,
		selection_chars: SelectionLength::default(),
	})
}

/// One rendering of a Markdown document, driven by the page.
#[wasm_bindgen]
pub struct Markview {
	source_index: SourceIndex,
	source_text: Arc<str>,
	document_version: u64,
	canvas: HtmlCanvasElement,
	renderer: Renderer,
	engine: LayoutEngine,
	config: Config,
	options: LayoutOptions,
	published: Published,
	pointer: Pointer,
	document: Arc<Document>,
	/// The document backing the snapshot currently on screen.
	published_document: Arc<Document>,
	/// The document `begin_update` parsed, until its layout completes.
	pending: Option<Pending>,
	images: Images,
	images_dirty: bool,
	/// The last canvas-local point the page reported, in CSS pixels.
	cursor: Option<(f32, f32)>,
	/// The canvas box in CSS pixels, which hit testing and scrolling use.
	logical: (f32, f32),
	/// The device pixel ratio the page asked for.
	dpr: f32,
	/// The scale the canvas is actually drawn at: the requested ratio,
	/// lowered until both logical edges fit the device limit. Sharing it with
	/// the drawn view keeps rendering and hit testing on one viewport.
	scale: f32,
	scrolling: ScrollState,
	horizontal: Horizontal,
	internal_scroll: bool,
	images_clickable: bool,
	pressed_image: Option<(String, Point)>,
	overflow_drag: Option<(usize, usize, f32)>,
	pending_anchor: Option<String>,
	jump_origin: Option<(String, f32)>,
	parse_ms: f64,
	layout_ms: f64,
	frame_ms: f64,
	frames: u64,
	glyphs: u64,
	/// The UTF-16 length of the selected text, cached between changes.
	selection_chars: SelectionLength,
}

#[wasm_bindgen]
impl Markview {
	/// The complete parsed outline, available before any layout step.
	pub fn outline(&self) -> String {
		let document = self
			.pending
			.as_ref()
			.and_then(|p| p.parsed.as_ref())
			.unwrap_or(&self.document);
		let entries: Vec<_> = document.outline().into_iter().map(|entry| serde_json::json!({
			"text": entry.text, "level": entry.level, "anchor": entry.anchor,
			"source": { "start": byte_to_utf16(&document.source, entry.source.start), "end": byte_to_utf16(&document.source, entry.source.end) },
		})).collect();
		serde_json::json!({ "documentVersion": self.document_version, "entries": entries }).to_string()
	}

	/// Queries current-version geometry without advancing layout.
	#[wasm_bindgen(js_name = sourceToPreview)]
	pub fn source_to_preview(&self, offset: usize) -> String {
		let position = self
			.current_source_geometry()
			.then(|| {
				self.source_index.source_to_preview(
					&self.published.snapshot,
					&self.horizontal,
					utf16_to_byte(&self.source_text, offset),
				)
			})
			.flatten();
		self.source_position_json(position)
	}

	/// `y` is measured from the document top, in CSS pixels.
	#[wasm_bindgen(js_name = previewToSource)]
	pub fn preview_to_source(&self, y: f32) -> String {
		let position = self
			.current_source_geometry()
			.then(|| {
				self.source_index.preview_to_source(
					&self.published.snapshot,
					&self.horizontal,
					y,
				)
			})
			.flatten();
		self.source_position_json(position)
	}

	/// Heading navigation uses the same deferred, expanding path as anchor links.
	#[wasm_bindgen(js_name = navigateHeading)]
	pub fn navigate_heading(&mut self, anchor: String) -> bool {
		let document = self
			.pending
			.as_ref()
			.and_then(|p| p.parsed.as_ref())
			.unwrap_or(&self.document);
		if !document
			.outline()
			.iter()
			.any(|entry| entry.anchor == anchor)
		{
			return false;
		}
		self.activate_document_link(&format!("#{anchor}"))
	}

	/// Every source in the newest document, including unpublished blocks.
	#[wasm_bindgen(js_name = imageSources)]
	pub fn image_sources(&self) -> String {
		serde_json::json!({
			"generation": self.images.snapshot.generation.to_string(),
			"sources": self.images.sources,
		})
		.to_string()
	}

	/// Accepts host pixels; the next flush starts one reflow for the batch.
	#[wasm_bindgen(js_name = resolveImage)]
	pub fn resolve_image(
		&mut self,
		generation: &str,
		src: &str,
		width: u32,
		height: u32,
		rgba: Vec<u8>,
	) {
		self.images_dirty |= self.images.complete(
			generation,
			src,
			Ok(Pixels {
				width,
				height,
				rgba: Arc::from(rgba),
			}),
			self.renderer.max_texture_dimension_2d(),
		);
	}

	#[wasm_bindgen(js_name = rejectImage)]
	pub fn reject_image(
		&mut self,
		generation: &str,
		src: &str,
		message: String,
	) {
		self.images_dirty |= self.images.complete(
			generation,
			src,
			Err(message),
			self.renderer.max_texture_dimension_2d(),
		);
	}

	#[wasm_bindgen(js_name = flushImages)]
	pub fn flush_images(&mut self) -> bool {
		if !std::mem::take(&mut self.images_dirty) {
			return false;
		}
		self.reflow();
		true
	}

	/// Geometry from the newest pass, never from a retired document.
	#[wasm_bindgen(js_name = imagePriorities)]
	pub fn image_priorities(&mut self) -> String {
		let viewport = Rect {
			x: -self.left(),
			y: self.visible_scroll(),
			w: self.logical.0,
			h: (self.logical.1 - 2. * INSET).max(0.),
		};
		let (snapshot, pass) = if let Some(pending) = &self.pending {
			(pending.layout.snapshot(), Some(pending.layout.pass_id()))
		} else {
			(&self.published.snapshot, self.published.pass)
		};
		serde_json::to_string(&self.images.priorities(
			snapshot,
			pass,
			self.published.revision,
			viewport,
			&self.horizontal,
		))
		.unwrap()
	}

	/// Full replace: parses, lays out and publishes.
	#[wasm_bindgen(js_name = setMarkdown)]
	pub fn set_markdown(&mut self, text: String) -> String {
		let started = Instant::now();
		self.reset_document_interaction();
		self.document = Arc::new(parse(text));
		self.replace_source(self.document.clone());
		self.images.prepare(&self.document);
		self.images_dirty = false;
		self.pending = None;
		self.parse_ms = started.elapsed().as_secs_f64() * 1000.0;
		let laid = Instant::now();
		self.relayout();
		self.layout_ms = laid.elapsed().as_secs_f64() * 1000.0;
		self.clamp_scroll();
		self.stats_json()
	}

	/// Parses `text` and starts a resumable layout.
	#[wasm_bindgen(js_name = beginUpdate)]
	pub fn begin_update(&mut self, text: String) -> String {
		let started = Instant::now();
		self.reset_document_interaction();
		let document = Arc::new(parse(text));
		self.replace_source(document.clone());
		self.images.prepare(&document);
		self.images_dirty = false;
		self.parse_ms = started.elapsed().as_secs_f64() * 1000.0;
		self.layout_ms = 0.0;
		let images = &self.images.snapshot;
		let layout = self.engine.begin_layout(&document, &self.options, images);
		self.pending = Some(Pending {
			parsed: Some(document),
			layout,
		});
		self.stats_json()
	}

	/// Advances the pending layout by at most `budget_ms` of work, publishing
	/// the prefix it reached.
	#[wasm_bindgen(js_name = stepUpdate)]
	pub fn step_update(&mut self, budget_ms: f64) -> String {
		let budget = Duration::from_secs_f64(step_budget(budget_ms));
		self.step(budget);
		self.clamp_scroll();
		self.stats_json()
	}

	/// True while `begin_update` has not finished.
	#[wasm_bindgen(js_name = updatePending)]
	pub fn update_pending(&self) -> bool {
		self.pending.is_some()
	}

	/// Forces the remaining layout to complete synchronously.
	#[wasm_bindgen(js_name = finishUpdate)]
	pub fn finish_update(&mut self) {
		let Some(mut pending) = self.pending.take() else {
			return;
		};
		let laid = Instant::now();
		// The pass is over the parsed text when there is one, and over the
		// accepted document otherwise, which is the case for a reflow.
		let accepted = &self.document;
		let document = pending.parsed.as_ref().unwrap_or(accepted);
		self.engine
			.advance(&mut pending.layout, document, Duration::MAX);
		self.layout_ms += laid.elapsed().as_secs_f64() * 1000.0;
		let source = document.source.clone();
		let pass = pending.layout.pass_id();
		self.published_document = document.clone();
		self.published.accept(
			pending.layout.into_snapshot(),
			source,
			Some(pass),
			&mut self.pointer,
		);
		if let Some(parsed) = pending.parsed {
			self.document = parsed;
		}
		self.selection_chars.forget();
		self.clamp_scroll();
	}

	/// Renders and presents one frame from the latest published snapshot.
	pub fn frame(&mut self) -> Result<String, JsValue> {
		let (width, height) = self.physical();
		if width == 0 || height == 0 {
			return Ok(self.stats_json());
		}
		let started = Instant::now();
		self.advance_interaction(started);
		let hover = self.hover();
		let view = View {
			selection: self.pointer.selection(),
			revision: self.published.revision,
			width,
			height,
			scale: self.scale,
			scroll: self.visible_scroll(),
			left: self.left(),
			// A small inset keeps the first and last line off the canvas edge.
			top: INSET,
			bottom: INSET,
			theme: self.theme(),
			horizontal: &self.horizontal,
			hovered_link: hover.link.as_deref(),
			hovered_overflow: hover.overflow,
			held_overflow: self.overflow_drag.map(|(b, o, _)| (b, o)),
		};
		self.renderer.set_pointer(self.cursor);
		let status = self
			.renderer
			.acquire()
			.map_err(|error| fail(format!("{error:#}")))?;
		let FrameStatus::Ready(frame, suboptimal) = status else {
			self.frame_ms = started.elapsed().as_secs_f64() * 1000.0;
			return Ok(self.stats_json());
		};
		let target = frame.texture.create_view(&Default::default());
		let before = self.renderer.raster_stats().rasterized;
		self.renderer
			.render(&self.published.snapshot, &view, &[], &target)
			.map_err(|error| fail(format!("{error:#}")))?;
		self.glyphs += self.renderer.raster_stats().rasterized - before;
		frame.present();
		self.frames += 1;
		if suboptimal {
			self.renderer.resize(width, height);
		}
		self.frame_ms = started.elapsed().as_secs_f64() * 1000.0;
		Ok(self.stats_json())
	}

	/// Sizes the drawing surface from the canvas box and the device ratio, and
	/// reports whether the reading column narrowed, which starts a reflow.
	pub fn resize(
		&mut self,
		css_width: f64,
		css_height: f64,
		dpr: f64,
	) -> bool {
		self.logical = (logical_edge(css_width), logical_edge(css_height));
		self.dpr = ratio(dpr);
		// The page passes its requested ratio on every call, so the scale is
		// derived from it and the box rather than from the last scale, which
		// would shrink the canvas again on every repeat.
		let limit = self.renderer.max_texture_dimension_2d();
		self.scale = effective_scale(self.logical, self.dpr, limit);
		let (width, height) =
			size_canvas(&self.canvas, self.logical, self.scale, limit);
		self.renderer.resize(width, height);
		// A narrower canvas narrows the column, and the lines that were laid
		// out for the wider one no longer fit: lay the document out again.
		let reflowed = self.apply_column();
		if reflowed {
			self.reflow();
		}
		self.clamp_scroll();
		reflowed
	}

	/// Lays the current text out again at the column the canvas now allows.
	///
	/// It runs as a resumable pass, so dragging a window edge never blocks a
	/// frame: each animation frame advances it and presents what is ready.
	fn reflow(&mut self) {
		self.horizontal.clear();
		self.overflow_drag = None;
		if let Some(pending) = self.pending.take()
			&& let Some(parsed) = pending.parsed
		{
			// Keep the newest text rather than the last fully accepted one.
			self.document = parsed;
		}
		let images = &self.images.snapshot;
		let layout =
			self.engine
				.begin_layout(&self.document, &self.options, images);
		self.pending = Some(Pending {
			parsed: None,
			layout,
		});
	}

	/// Sets the document scroll in logical pixels, clamped to the range. A
	/// non-finite request leaves the scroll where it was.
	#[wasm_bindgen(js_name = setScroll)]
	pub fn set_scroll(&mut self, y: f64) {
		if y.is_finite() {
			let before = self.visible_scroll();
			self.pending_anchor = None;
			self.hold_scroll(y.min(f64::from(f32::MAX)) as f32);
			// A request the clamp absorbs, or one that a pending pass holds
			// above its temporary height, leaves the offset on screen alone,
			// and with it every reading position under the pointer.
			if self.visible_scroll() == before {
				return;
			}
			self.follow_pointer();
		}
	}

	#[wasm_bindgen(js_name = scrollBy)]
	pub fn scroll_by(&mut self, dy: f64) {
		if !dy.is_finite() {
			return;
		}
		self.cancel_scroll_animation();
		let base = self
			.scrolling
			.target
			.filter(|target| target.is_finite())
			.unwrap_or(self.scrolling.offset);
		self.set_scroll(f64::from(base) + dy);
	}

	/// Queues the final document end while layout is incomplete.
	#[wasm_bindgen(js_name = scrollToEnd)]
	pub fn scroll_to_end(&mut self) {
		self.pending_anchor = None;
		self.scrolling.offset = self.visible_scroll();
		self.scrolling.by(f32::INFINITY, self.scroll_bounds());
		self.follow_pointer();
	}

	/// The offset on screen, logical px.
	pub fn scroll(&self) -> f64 {
		f64::from(self.visible_scroll())
	}

	#[wasm_bindgen(js_name = maxScroll)]
	pub fn max_scroll(&self) -> f64 {
		f64::from(self.scroll_range())
	}

	/// The laid-out document height in logical pixels.
	#[wasm_bindgen(js_name = contentHeight)]
	pub fn content_height(&self) -> f64 {
		f64::from(self.published.snapshot.height)
	}

	/// Starts a press: a word or block selection on a repeated click.
	#[wasm_bindgen(js_name = pointerDown)]
	pub fn pointer_down(
		&mut self,
		x: f64,
		y: f64,
		shift: bool,
		control: bool,
		alt: bool,
		meta: bool,
	) {
		self.cursor = Some((x as f32, y as f32));
		self.pointer.set_modifiers(Modifiers {
			shift,
			control,
			alt,
			meta,
		});
		self.cancel_scroll_animation();
		self.pending_anchor = None;
		self.pointer.set_drag(None);
		self.pointer.set_dragged(false);
		self.pointer.set_auto_scroll_at(None);
		self.pressed_image = None;
		let point = Point::new(x as f32, y as f32);
		if let Some((b, o, bar)) = self.context().overflow_bar_at(
			point,
			self.options.stylesheet.overflow_scrollbar_metrics(),
		) {
			let grab = if bar.on_thumb(point.x, point.y) {
				bar.grab(point.x, point.y)
			} else {
				self.horizontal
					.insert((b, o), bar.scroll_for(point.x, point.y, 0.0));
				0.0
			};
			self.overflow_drag = Some((b, o, grab));
			return;
		}
		if self.images_clickable && self.context().link(point).is_none() {
			self.pressed_image = self
				.context()
				.image(point)
				.map(|(src, _, _)| (src.to_owned(), point));
		}
		let reading = Reading {
			snapshot: &self.published.snapshot,
			revision: self.published.revision,
			viewport: self.viewport(),
			horizontal: &self.horizontal,
		};
		self.pointer.press(&reading, x as f32, y as f32);
		self.selection_chars.forget();
	}

	/// Extends the press in flight. A hover that is not dragging, or one that
	/// lands on the same reading position, leaves the selection alone, so its
	/// cached length survives.
	#[wasm_bindgen(js_name = pointerMove)]
	pub fn pointer_move(&mut self, x: f64, y: f64) {
		self.cursor = Some((x as f32, y as f32));
		if let Some((b, o, grab)) = self.overflow_drag {
			if let Some(bar) = self.context().overflow_bar(
				b,
				o,
				self.options.stylesheet.overflow_scrollbar_metrics(),
			) {
				self.horizontal
					.insert((b, o), bar.scroll_for(x as f32, y as f32, grab));
			}
			return;
		}
		if self.pressed_image.as_ref().is_some_and(|(_, start)| {
			(start.x - x as f32).hypot(start.y - y as f32) >= 4.0
		}) {
			self.pressed_image = None;
		}
		let reading = Reading {
			snapshot: &self.published.snapshot,
			revision: self.published.revision,
			viewport: self.viewport(),
			horizontal: &self.horizontal,
		};
		if self.pointer.drag_to(&reading, x as f32, y as f32) {
			self.selection_chars.forget();
		}
	}

	/// Ends the press in flight.
	#[wasm_bindgen(js_name = pointerUp)]
	pub fn pointer_up(&mut self, x: f64, y: f64) -> String {
		self.cursor = Some((x as f32, y as f32));
		if self.overflow_drag.take().is_some() {
			return "null".into();
		}
		self.pointer_move(x, y);
		let point = Point::new(x as f32, y as f32);
		let link = self.context().link(point).map(str::to_owned);
		let activated = self.pointer.release(point.x, point.y, link.as_deref());
		self.selection_chars.forget();
		if let Some(link) = activated {
			self.pressed_image = None;
			let previous_pass =
				self.pending.as_ref().map(|p| p.layout.pass_id());
			if self.activate_document_link(&link) {
				return serde_json::json!({"kind":"document", "reflowed":previous_pass != self.pending.as_ref().map(|p| p.layout.pass_id())}).to_string();
			}
			return self.action_json("link", &link);
		}
		if let Some((src, _)) = self.pressed_image.take()
			&& !self.pointer.dragged()
			&& self
				.context()
				.image(point)
				.is_some_and(|(release, _, _)| release == src)
		{
			return self.action_json("image", &src);
		}

		"null".into()
	}

	#[wasm_bindgen(js_name = pointerLeave)]
	pub fn pointer_leave(&mut self) {
		if self.pointer.drag().is_none() && self.overflow_drag.is_none() {
			self.cursor = None;
		}
	}
	#[wasm_bindgen(js_name = cancelPointer)]
	pub fn cancel_pointer(&mut self) {
		self.pointer.reset_clicks();
		self.pointer.set_drag(None);
		self.pointer.set_auto_scroll_at(None);
		self.overflow_drag = None;
		self.pressed_image = None;
		self.cursor = None;
	}
	pub fn cursor(&self) -> String {
		let Some((x, y)) = self.cursor else {
			return "default".into();
		};
		if self.overflow_drag.is_some() {
			return "default".into();
		}
		let idle = self.pointer.drag().is_none();
		self.context()
			.cursor(Point::new(x, y), idle, idle && self.images_clickable)
			.css()
			.into()
	}
	#[wasm_bindgen(js_name = setImagesClickable)]
	pub fn set_images_clickable(&mut self, clickable: bool) {
		self.images_clickable = clickable;
	}
	#[wasm_bindgen(js_name = setScrollMode)]
	pub fn set_scroll_mode(&mut self, mode: &str) -> Result<(), JsValue> {
		match mode {
			"external" => self.internal_scroll = false,
			"internal" => self.internal_scroll = true,
			_ => return Err(fail("invalid scroll mode")),
		}
		self.cancel_scroll_animation();
		Ok(())
	}
	#[wasm_bindgen(js_name = scrollInput)]
	pub fn scroll_input(
		&mut self,
		dx: f64,
		dy: f64,
		kind: &str,
	) -> Result<(), JsValue> {
		if !matches!(kind, "external" | "step") {
			return Err(fail("invalid scroll input kind"));
		}
		if !dx.is_finite() || !dy.is_finite() {
			return Ok(());
		}
		self.pending_anchor = None;
		let external = kind == "external" || !self.internal_scroll;
		if external {
			self.cancel_scroll_animation();
		}
		let consumed = self.cursor.is_some_and(|(x, y)| {
			markview_selection::horizontal_by(
				&self.published.snapshot,
				self.viewport(),
				&mut self.horizontal,
				Point::new(x, y),
				dx.clamp(-f64::from(f32::MAX), f64::from(f32::MAX)) as f32,
			)
		});
		let delta = if consumed && dx.abs() >= dy.abs() {
			0.0
		} else {
			dy.clamp(-f64::from(f32::MAX), f64::from(f32::MAX)) as f32
		};
		if delta != 0.0 {
			if external {
				self.scroll_by(f64::from(delta));
			} else {
				self.scrolling.wheel_by(delta, Instant::now());
			}
		}
		self.follow_pointer();
		Ok(())
	}

	#[wasm_bindgen(js_name = selectAll)]
	pub fn select_all(&mut self) {
		self.pointer
			.select_all(&self.published.snapshot, self.published.revision);
		self.selection_chars.forget();
	}

	#[wasm_bindgen(js_name = clearSelection)]
	pub fn clear_selection(&mut self) {
		self.pointer.clear();
		self.selection_chars.forget();
	}

	/// The reading text of the selection, `""` when empty or stale.
	#[wasm_bindgen(js_name = selectedText)]
	pub fn selected_text(&self) -> String {
		self.pointer
			.selected_text(&self.published.snapshot, self.published.revision)
	}

	/// A `Stats` JSON string for the current state.
	pub fn stats(&self) -> String {
		self.stats_json()
	}

	/// The GPU adapter and backend this handle draws through.
	pub fn adapter(&self) -> String {
		self.renderer.adapter_name.clone()
	}

	/// Applies a new `Config` and lays the document out again.
	#[wasm_bindgen(js_name = setConfig)]
	pub fn set_config(
		&mut self,
		config_json: Option<String>,
	) -> Result<(), JsValue> {
		let config = Config::parse(config_json.as_deref())?;
		let details_open = self.options.details_open.clone();
		self.options = config.options(self.options.fonts.clone());
		self.options.details_open = details_open;
		self.horizontal.clear();
		self.overflow_drag = None;
		self.config = config;
		self.apply_column();
		if let Some(pending) = self.pending.take()
			&& let Some(parsed) = pending.parsed
		{
			self.document = parsed;
		}
		let laid = Instant::now();
		self.relayout();
		self.layout_ms = laid.elapsed().as_secs_f64() * 1000.0;
		self.clamp_scroll();
		Ok(())
	}
}

impl Markview {
	fn context(&self) -> DocumentInteraction<'_> {
		DocumentInteraction {
			snapshot: &self.published.snapshot,
			viewport: self.viewport(),
			horizontal: &self.horizontal,
			revision: self.published.revision,
		}
	}
	fn hover(&self) -> Hover {
		if self.overflow_drag.is_some() {
			return Hover::default();
		}
		let Some((x, y)) = self.cursor else {
			return Hover::default();
		};
		self.context().hover(
			Point::new(x, y),
			self.pointer.drag().is_some() || self.overflow_drag.is_some(),
			self.images_clickable,
			self.options.stylesheet.overflow_scrollbar_metrics(),
		)
	}
	fn follow_pointer(&mut self) {
		if self.pointer.drag().is_none() {
			return;
		}
		if let Some((x, y)) = self.cursor {
			let reading = Reading {
				snapshot: &self.published.snapshot,
				revision: self.published.revision,
				viewport: self.viewport(),
				horizontal: &self.horizontal,
			};
			if self.pointer.drag_to(&reading, x, y) {
				self.selection_chars.forget();
			}
		}
	}
	fn advance_interaction(&mut self, now: Instant) {
		self.scrolling.advance(now, self.scroll_bounds());
		if self.pointer.drag().is_some()
			&& self.pointer.dragged()
			&& let Some((_, y)) = self.cursor
		{
			let delta = markview_selection::selection_scroll(
				y,
				INSET,
				self.logical.1 - INSET,
				self.visible_scroll(),
				self.scroll_range(),
			);
			if delta != 0.0
				&& self.pointer.auto_scroll_at().is_none_or(|at| at <= now)
			{
				self.scrolling.by(delta, self.scroll_bounds());
				self.pointer
					.set_auto_scroll_at(Some(now + Duration::from_millis(16)));
			}
		}
		self.follow_pointer();
	}
	fn reset_document_interaction(&mut self) {
		self.scrolling.cancel();
		self.horizontal.clear();
		self.overflow_drag = None;
		self.pressed_image = None;
		self.pointer.reset_clicks();
		self.pending_anchor = None;
		self.jump_origin = None;
		self.options.details_open = Arc::default();
	}
	fn action_json(&self, kind: &str, target: &str) -> String {
		let m = self.pointer.modifiers();
		serde_json::json!({"kind":kind,"target":target,"modifiers":{"shift":m.shift,"control":m.control,"alt":m.alt,"meta":m.meta}}).to_string()
	}
	fn activate_document_link(&mut self, link: &str) -> bool {
		self.scrolling.cancel();
		if let Some(id) = markview_core::document::details_id(link) {
			let expanded =
				self.options.details_open.get(&id).copied().unwrap_or_else(
					|| {
						self.published_document
							.details_declared(id)
							.unwrap_or(false)
					},
				);
			Arc::make_mut(&mut self.options.details_open).insert(id, !expanded);
			self.horizontal.clear();
			self.reflow();
			return true;
		}
		let Some(fragment) = link.strip_prefix('#') else {
			return false;
		};
		let fragment = percent_encoding::percent_decode_str(fragment)
			.decode_utf8_lossy()
			.into_owned();
		if fragment.is_empty() {
			self.set_scroll(0.0);
			return true;
		}
		if let Some(label) =
			markview_core::document::footnote::back_label(&fragment)
		{
			if let Some((origin, offset)) = &self.jump_origin
				&& markview_core::document::footnote::label(origin)
					== Some(label)
			{
				let offset = *offset;
				self.set_scroll(f64::from(offset));
				return true;
			}
			self.pending_anchor =
				Some(markview_core::document::footnote::reference(label));
		} else {
			self.jump_origin = Some((fragment.clone(), self.visible_scroll()));
			self.pending_anchor = Some(fragment);
		}
		// A pending source must become the document before its anchors are inspected.
		if self.pending.as_ref().is_some_and(|p| p.parsed.is_some()) {
			self.reflow();
		}
		let mut expanded = false;
		for id in self
			.document
			.details_enclosing(self.pending_anchor.as_deref().unwrap())
		{
			if !self.options.details_open.get(&id).copied().unwrap_or_else(
				|| self.document.details_declared(id).unwrap_or(false),
			) {
				Arc::make_mut(&mut self.options.details_open).insert(id, true);
				expanded = true;
			}
		}
		if expanded {
			self.horizontal.clear();
			self.reflow();
		}
		self.apply_anchor();
		true
	}
	fn apply_anchor(&mut self) {
		let Some(anchor) = self.pending_anchor.as_deref() else {
			return;
		};
		// Old geometry cannot resolve a target while its replacement is publishing.
		if self.pending.is_some()
			&& !self.published.continues(
				&self.document.source,
				self.pending.as_ref().unwrap().layout.pass_id(),
			) {
			return;
		}
		if let Some(y) = self.published.snapshot.anchor_y(anchor) {
			self.scrolling.set(y, self.scroll_bounds());
			self.pending_anchor = None;
		} else if self.pending.is_none() {
			self.pending_anchor = None;
		}
	}

	/// The canvas backing store in physical pixels.
	fn physical(&self) -> (u32, u32) {
		(self.canvas.width(), self.canvas.height())
	}

	/// The layout viewport in logical pixels, mirroring the drawn `View`.
	fn viewport(&self) -> Viewport {
		Viewport {
			width: self.logical.0,
			height: self.logical.1,
			left: self.left(),
			top: INSET,
			bottom: INSET,
			scroll: self.visible_scroll(),
		}
	}

	fn replace_source(&mut self, document: Arc<Document>) {
		self.document_version += 1;
		self.source_index = SourceIndex::new(&document);
		self.source_text = document.source.clone();
	}
	fn current_source_geometry(&self) -> bool {
		self.published
			.source
			.as_ref()
			.is_some_and(|source| Arc::ptr_eq(source, &self.source_text))
	}
	fn source_position_json(&self, position: Option<SourcePosition>) -> String {
		position.map(|position| serde_json::json!({
			"documentVersion": self.document_version, "revision": self.published.revision,
			"source": { "start": byte_to_utf16(&self.source_text, position.source.start), "end": byte_to_utf16(&self.source_text, position.source.end) },
			"rect": { "x": position.rect.x, "y": position.rect.y, "width": position.rect.w, "height": position.rect.h },
		})).unwrap_or(serde_json::Value::Null).to_string()
	}

	/// The x of the layout column's left edge inside the canvas.
	fn left(&self) -> f32 {
		((self.logical.0 - self.published.snapshot.width) / 2.0).max(MARGIN)
	}

	/// Narrows the layout column to what the canvas can show, and reports
	/// whether it moved. A column left wider than the canvas clips its lines
	/// off the edge, and nothing here can pan sideways to reach them.
	fn apply_column(&mut self) -> bool {
		let width = column_width(self.config.width, self.logical.0);
		if (self.options.width - width).abs() < f32::EPSILON {
			return false;
		}
		self.options.width = width;
		true
	}

	/// The offset actually on screen. A pass that is still publishing has only
	/// a temporary height, so the requested offset is held above it rather than
	/// clamped away: later prefixes put the reader back where they were.
	fn visible_scroll(&self) -> f32 {
		self.scrolling.visible(self.scroll_bounds())
	}
	fn scroll_bounds(&self) -> ScrollBounds {
		ScrollBounds {
			max: self.scroll_range(),
			complete: self.pending.is_none(),
		}
	}

	fn theme(&self) -> Theme {
		match self.config.theme {
			ThemeConfig::Light => Theme::Light,
			ThemeConfig::Dark => Theme::Dark,
		}
	}

	/// How far the document scrolls: its height past the page that is actually
	/// visible, which the two viewport insets shrink. Subtracting the whole
	/// canvas would leave the last `2 * INSET` pixels unreachable.
	fn scroll_range(&self) -> f32 {
		let page = (self.logical.1 - 2.0 * INSET).max(0.0);
		(self.published.snapshot.height - page).max(0.0)
	}

	fn clamp_scroll(&mut self) {
		self.scrolling.resolve(self.scroll_bounds());
		self.apply_anchor();
		self.follow_pointer();
	}
	fn hold_scroll(&mut self, y: f32) {
		self.scrolling.set(y, self.scroll_bounds());
	}
	fn cancel_scroll_animation(&mut self) {
		// Animated travel hands off its displayed offset; held requests survive.
		if self.scrolling.animation.is_some() {
			self.scrolling.offset = self.visible_scroll();
		}
		self.scrolling.cancel();
	}

	/// Lays the accepted document out in one uninterrupted pass.
	fn relayout(&mut self) {
		// `accept` rebases the selection onto the new snapshot, so its text
		// may differ even where the reading positions survive.
		self.selection_chars.forget();
		let Markview {
			engine,
			options,
			published,
			pointer,
			document,
			published_document,
			images,
			..
		} = self;
		let snapshot =
			engine.layout_with_images(document, options, &images.snapshot);
		*published_document = document.clone();
		// A full layout is not a prefix of anything, so no pass may extend it.
		published.accept(snapshot, document.source.clone(), None, pointer);
	}

	/// Runs the pending layout until the budget runs out, publishing the prefix
	/// it reached. Returns whether the whole document is laid out.
	fn step(&mut self, budget: Duration) -> bool {
		let Some(mut pending) = self.pending.take() else {
			return true;
		};
		let started = Instant::now();
		// The pass is resumed, never restarted, so a step pays only for the
		// blocks it has not reached yet, however long the published prefix has
		// grown. It is over the parsed text when there is one, and over the
		// accepted document otherwise, which is the case for a reflow.
		let accepted = &self.document;
		let document = pending.parsed.as_ref().unwrap_or(accepted);
		let completed =
			self.engine.advance(&mut pending.layout, document, budget);
		self.layout_ms += started.elapsed().as_secs_f64() * 1000.0;
		if completed {
			let source = document.source.clone();
			let pass = pending.layout.pass_id();
			self.published_document = document.clone();
			self.published.accept(
				pending.layout.into_snapshot(),
				source,
				Some(pass),
				&mut self.pointer,
			);
			if let Some(parsed) = pending.parsed {
				self.document = parsed;
			}
			self.selection_chars.forget();
		} else {
			let prefix = pending.layout.snapshot();
			let pass = pending.layout.pass_id();
			if self.published.continues(&document.source, pass) {
				// The prefix of a pass already on screen only adds blocks, so
				// it is appended rather than copied whole.
				self.published.extend(prefix, &mut self.pointer);
				// `extend` only re-stamps the selection, which reads the same
				// text, so the cached character count still holds here.
			} else if prefix.height >= self.scrolling.offset
				&& self.published.covers_interaction(
					&document.source,
					prefix,
					&self.pointer,
				) {
				// A different document, or the same one laid out for another
				// column, replaces what is on screen. Until its prefix has
				// grown back past the top of the current view, replacing would
				// show the reader a much shorter document, so the snapshot
				// they are reading stays until it does.
				self.published.accept(
					prefix.clone(),
					document.source.clone(),
					Some(pass),
					&mut self.pointer,
				);
				self.published_document = document.clone();
				self.selection_chars.forget();
			}
			self.pending = Some(pending);
		}
		completed
	}

	/// The length of the selection as JavaScript counts it:
	/// [`Markview::selected_text`] scans the whole snapshot and allocates the
	/// selected text, so it is extracted at most once between changes.
	fn selection_length(&self) -> usize {
		// `selectedText().length` counts UTF-16 code units, so a character
		// outside the basic plane counts as two here as well.
		self.selection_chars.get(|| self.selected_text())
	}

	fn stats_json(&self) -> String {
		serde_json::json!({
			"documentVersion": self.document_version,
			"revision": self.published.revision,
			"blocks": self.published.snapshot.blocks.len(),
			"contentHeight": self.published.snapshot.height,
			"width": self.published.snapshot.width,
			"reused": self.published.snapshot.reused,
			"parseMs": self.parse_ms,
			"layoutMs": self.layout_ms,
			"frameMs": self.frame_ms,
			"frames": self.frames,
			"glyphs": self.glyphs,
			"backend": format!("{:?}", self.renderer.backend),
			"adapter": self.renderer.adapter_name,
			"selectionLength": self.selection_length(),
			"pending": self.pending.is_some(),
		})
		.to_string()
	}
}

/// A layout pass in flight, and the text it is over.
struct Pending {
	/// The document `begin_update` parsed, when the pass is over text newer
	/// than [`Markview::document`] holds. A reflow re-uses the accepted
	/// document instead, which is why this is optional.
	parsed: Option<Arc<Document>>,
	/// The suspended pass. It holds everything the layout needs between calls,
	/// so a step never re-measures a block an earlier step already laid out.
	layout: ProgressiveLayout,
}

/// The display handle wgpu wants before it accepts a canvas surface.
///
/// A browser borrows nothing for it, but the shared handle enum is neither
/// `Send` nor `Sync`, which wgpu requires, so this names the web variant alone.
#[derive(Debug)]
struct WebDisplay;
impl wgpu::rwh::HasDisplayHandle for WebDisplay {
	fn display_handle(
		&self,
	) -> Result<wgpu::rwh::DisplayHandle<'_>, wgpu::rwh::HandleError> {
		Ok(wgpu::rwh::DisplayHandle::web())
	}
}

/// The canvas the renderer draws into, and its only surface source.
struct Present {
	canvas: HtmlCanvasElement,
}
impl SurfaceSource for Present {
	fn instance_descriptor(&self) -> wgpu::InstanceDescriptor {
		// wgpu insists the instance carries a display handle before it accepts
		// a canvas surface, and asking for WebGL2 alone keeps the reported
		// backend the one the demo asserts.
		wgpu::InstanceDescriptor {
			backends: wgpu::Backends::GL,
			..wgpu::InstanceDescriptor::new_with_display_handle(Box::new(
				WebDisplay,
			))
		}
	}
	fn create_surface(
		&self,
		instance: &wgpu::Instance,
	) -> anyhow::Result<wgpu::Surface<'static>> {
		let target: wgpu::SurfaceTarget<'static> =
			wgpu::SurfaceTarget::Canvas(self.canvas.clone());
		Ok(instance.create_surface(target)?)
	}
	fn size(&self) -> (u32, u32) {
		// The first configure happens before a device exists to ask, so a
		// backing store the page already left oversized is reported at
		// `INITIAL_LIMIT` until `create` reads the real limit.
		(
			self.canvas.width().clamp(1, INITIAL_LIMIT),
			self.canvas.height().clamp(1, INITIAL_LIMIT),
		)
	}
}

/// The JavaScript-injected configuration. Every field is optional and unknown
/// keys are ignored.
#[derive(Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Config {
	width: f32,
	font_size: f32,
	theme: ThemeConfig,
	justify: bool,
	hyphenate: bool,
	paragraph_indent: f32,
	greedy: bool,
	hide_front_matter: bool,
	front_matter_label: String,
}
impl Default for Config {
	fn default() -> Self {
		Self {
			width: 760.0,
			font_size: 18.0,
			theme: ThemeConfig::Light,
			justify: true,
			hyphenate: true,
			paragraph_indent: 0.0,
			greedy: false,
			hide_front_matter: false,
			front_matter_label: "Metadata".into(),
		}
	}
}
impl Config {
	fn parse(json: Option<&str>) -> Result<Self, JsValue> {
		match json.map(str::trim) {
			None | Some("") => Ok(Self::default()),
			Some(text) => serde_json::from_str(text)
				.map_err(|error| fail(format!("invalid config: {error}"))),
		}
	}

	fn options(&self, fonts: FontConfig) -> LayoutOptions {
		LayoutOptions {
			width: finite(self.width, 760.0).clamp(1.0, MAX_LOGICAL),
			font_size: finite(self.font_size, 18.0).clamp(1.0, 200.0),
			justify: self.justify,
			hyphenate: self.hyphenate,
			paragraph_indent: self.paragraph_indent,
			greedy: self.greedy,
			hide_front_matter: self.hide_front_matter,
			front_matter_label: self.front_matter_label.clone(),
			stylesheet: Stylesheet::bundled(self.theme == ThemeConfig::Dark),
			fonts,
			..LayoutOptions::default()
		}
	}
}

#[derive(Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum ThemeConfig {
	Light,
	Dark,
}

/// The reading column `desired` logical pixels wide, narrowed until [`MARGIN`]
/// survives on both sides of a canvas `canvas` logical pixels wide. Text that
/// overflows sideways cannot be reached, so the column always fits.
fn column_width(desired: f32, canvas: f32) -> f32 {
	desired.max(1.0).min((canvas - 2.0 * MARGIN).max(1.0))
}

/// A finite layout budget in seconds, bounded so `Duration` always accepts
/// it. A non-finite request becomes a zero budget, which still completes one
/// prefix per call rather than laying the document out in one go.
fn step_budget(millis: f64) -> f64 {
	if millis.is_finite() {
		millis.clamp(0.0, MAX_STEP_MS) / 1000.0
	} else {
		0.0
	}
}

/// A finite, non-negative logical edge, capped where layout stops making
/// sense. A non-finite request becomes zero, which draws nothing.
fn logical_edge(value: f64) -> f32 {
	if value.is_finite() {
		(value as f32).clamp(0.0, MAX_LOGICAL)
	} else {
		0.0
	}
}

/// A finite device pixel ratio. Zero, negative and non-finite all become one,
/// because a ratio scales the canvas and is never a way to ask for none.
fn ratio(value: f64) -> f32 {
	if value.is_finite() {
		(value as f32).clamp(0.1, MAX_DPR)
	} else {
		1.0
	}
}

/// A finite value, or `fallback` when it is not a number.
fn finite(value: f32, fallback: f32) -> f32 {
	if value.is_finite() { value } else { fallback }
}

/// The scale the canvas and the drawn view share: the ratio the page asked
/// for, lowered until neither logical edge asks for more device pixels than
/// the device accepts. A zero edge asks for none, so it caps nothing.
fn effective_scale(logical: (f32, f32), dpr: f32, limit: u32) -> f32 {
	let limit = limit as f32;
	let fit = |edge: f32| if edge > 0.0 { limit / edge } else { dpr };
	dpr.min(fit(logical.0)).min(fit(logical.1))
}

/// Sizes the canvas backing store from its logical box and the scale the
/// device accepts.
fn size_canvas(
	canvas: &HtmlCanvasElement,
	logical: (f32, f32),
	scale: f32,
	limit: u32,
) -> (u32, u32) {
	let width = device_pixels(logical.0, scale, limit);
	let height = device_pixels(logical.1, scale, limit);
	canvas.set_width(width);
	canvas.set_height(height);
	(width, height)
}

/// One edge in device pixels, capped at what the device accepts.
fn device_pixels(logical: f32, scale: f32, limit: u32) -> u32 {
	let edge = (logical * scale).round();
	if edge.is_finite() {
		(edge.max(0.0) as u32).min(limit)
	} else {
		0
	}
}

/// Reports an error to JavaScript as a string.
fn fail(error: impl std::fmt::Display) -> JsValue {
	JsValue::from_str(&error.to_string())
}
