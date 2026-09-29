//! The windowless server an editor plugin drives.
//!
//! The client owns the text and the viewport; this side owns layout. A request
//! is one line of JSON on stdin. A response is a JSON header on stdout; tile
//! headers are followed by exactly `tile.bytes` raw PNG bytes, without a delimiter. Reaching the end of stdin ends
//! the process, which is what keeps a host that crashes or is killed from
//! leaving a server behind.
use crate::{
	cli::LaunchOptions,
	link::Target,
	render::{Renderer, Theme, View},
	settings::{ExportFormat, ExportSettings},
};
use anyhow::{Context, Result, bail};
use markview_core::{
	document,
	layout::{LayoutEngine, LayoutOptions, LayoutSnapshot},
	scene::{Paint, PlacedBlock, Rect},
	style::{StyleTarget, Stylesheet},
};
use serde_json::{Value, json};
use std::{
	collections::HashMap,
	io::{BufRead, Write},
	path::{Path, PathBuf},
	sync::Arc,
	sync::mpsc::{self, RecvTimeoutError},
	time::Duration,
};

/// A document the client handed over, with the layout it last produced.
struct Open {
	/// Explicit templates retain their palette when the host theme changes.
	templated: bool,
	document: document::Document,
	images: crate::images::Images,
	/// The layout options this document was laid out with. The host resolves
	/// its own settings per document, so they are not the session's.
	options: LayoutOptions,
	layout: LayoutSnapshot,
	/// The directory the document's own relative resources resolve against.
	directory: Option<PathBuf>,
	/// The client's text. An export is written from these bytes rather than
	/// from the file, which may not have them yet.
	text: Arc<str>,
	/// The path the document was opened under. Resources resolve against its
	/// parent, which is the directory above is not.
	path: PathBuf,
	/// Whether the file is observed. A client owns the text until it reports a
	/// save, which hands observation back.
	observed: bool,
	/// The file's modification time when it was last read.
	stamp: Option<std::time::SystemTime>,
	/// Set when the geometry changed without the client being told, which
	/// happens when a draw settles rasters the client has not heard about.
	publish: bool,
}

/// Where one document stands, in the shape the client reads it.
fn state(id: &str, open: &Open) -> Value {
	json!({
		"id": id,
		"height": open.layout.height,
		"images": open.layout.images.entries.len(),
		"blocks": blocks(&open.layout, open.directory.as_deref()),
		"complete": open.images.busy() == 0,
		// What the engine could not draw, so the host can report it where its
		// own diagnostics live rather than leaving the reader to guess.
		"degraded": open.layout.degraded,
		"math_errors": open.layout.math_errors,
		"deferred": open.images.deferred_remote(),
	})
}

/// A response header, optionally followed by an encoded image body.
pub struct Reply {
	header: String,
	pixels: Vec<u8>,
}

impl From<String> for Reply {
	fn from(header: String) -> Self {
		Self {
			header,
			pixels: Vec::new(),
		}
	}
}

impl Reply {
	fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
		writeln!(writer, "{}", self.header)?;
		writer.write_all(&self.pixels)
	}
}

/// The state a session carries across requests.
pub struct Session {
	engine: LayoutEngine,
	options: LayoutOptions,
	offline: bool,
	theme: Theme,
	/// The sheet a theme is merged onto. A theme is a palette, not a whole
	/// appearance, so naming one must not drop what the session started with.
	base: Arc<Stylesheet>,
	/// Built on the first tile request, so a session that only lays text out
	/// never pays for a device.
	renderer: Option<Renderer>,
	documents: HashMap<String, Open>,
}

impl Session {
	fn new(args: &LaunchOptions) -> Self {
		Self {
			engine: LayoutEngine::new(),
			options: args.options.clone(),
			offline: args.offline,
			theme: args.theme.unwrap_or_default(),
			base: args.options.stylesheet.clone(),
			renderer: None,
			documents: HashMap::new(),
		}
	}

	/// Handles one request line and returns its header and optional image body.
	///
	/// A malformed request is answered rather than fatal, so one bad message
	/// from a client does not take the session's other documents down.
	pub fn handle(&mut self, line: &str) -> Reply {
		let request: Value = match serde_json::from_str(line) {
			Ok(request) => request,
			Err(error) => {
				return failure(&format!("Malformed request: {error}")).into();
			}
		};
		let Some((method, params)) = request
			.as_object()
			.filter(|fields| fields.len() == 1)
			.and_then(|fields| fields.iter().next())
		else {
			return failure("A request is one { method: params } pair").into();
		};
		match method.as_str() {
			"open" => self.open(params),
			"close" => self.close(params),
			"tile" => return self.tile(params),
			"text" => self.text(params),
			"rendered" => self.rendered(params),
			"appearance" => self.appearance(params),
			"export" => self.export(params),
			"styles" => self.styles(params),
			"saved" => self.saved(params),
			_ => failure(&format!("Unknown method {method}")),
		}
		.into()
	}

	/// Takes the client's text and answers with its geometry.
	fn open(&mut self, params: &Value) -> String {
		let Some(id) = params.get("id").and_then(Value::as_str) else {
			return failure("open needs an id");
		};
		let Some(text) = params.get("text").and_then(Value::as_str) else {
			return failure("open needs text");
		};
		// `path` names the document for relative resources. A client whose
		// document has no path yet still names a directory, so an unsaved
		// buffer resolves the images beside it.
		let path = params
			.get("path")
			.and_then(Value::as_str)
			.map_or_else(|| PathBuf::from("untitled.md"), PathBuf::from);
		// Waiting for the images gives a complete answer in one round trip,
		// which is what a client that has nothing to show yet wants. A client
		// that would rather paint the text immediately asks not to wait and
		// receives the refinements as they settle.
		if let Some(settle) = params.get("settle")
			&& !settle.is_boolean()
		{
			return failure("settle must be true or false");
		}
		let settle = params
			.get("settle")
			.and_then(Value::as_bool)
			.unwrap_or(true);
		let options = match self.options_with(params) {
			Ok(options) => options,
			Err(error) => return failure(&format!("{error:#}")),
		};
		let document = document::parse(text.to_owned());
		// Images take part in geometry, so a document that names one lays out
		// exactly as the same bytes would from disk.
		let mut images = crate::images::Images::new(
			self.offline,
			self.options.fonts.clone(),
		);
		images.prepare(
			&document,
			&path,
			1,
			false,
			&options.stylesheet,
			&options.fonts,
		);
		if settle {
			images.wait();
		}
		let mut layout = self.engine.layout_with_images(
			&document,
			&options,
			&images.snapshot,
		);
		// A served document has no later frame to wait for, so the cosmetic
		// highlighting pass settles here rather than after the answer.
		if self.engine.wait_highlights() {
			layout = self.engine.layout_with_images(
				&document,
				&options,
				&images.snapshot,
			);
		}
		// An image that arrived while the pass ran changes the geometry, so
		// the layout is taken again once the snapshot has settled.
		if settle && layout.images.entries != images.snapshot.entries {
			layout = self.engine.layout_with_images(
				&document,
				&options,
				&images.snapshot,
			);
		}
		let directory = path.parent().map(Path::to_path_buf);
		let open = Open {
			templated: params.get("template").is_some()
				|| params.get("stylesheet").is_some(),
			document,
			text: Arc::from(text),
			images,
			options,
			layout,
			directory,
			path,
			observed: false,
			stamp: None,
			publish: false,
		};
		let response = json!({ "opened": state(id, &open) });
		self.documents.insert(id.to_owned(), open);
		response.to_string()
	}

	fn close(&mut self, params: &Value) -> String {
		let Some(id) = params.get("id").and_then(Value::as_str) else {
			return failure("close needs an id");
		};
		let Some(open) = self.documents.remove(id) else {
			return failure(&format!("No document {id}"));
		};
		json!({ "closed": {
			"id": id,
			"blocks": open.layout.blocks.len(),
		} })
		.to_string()
	}

	/// The base options with the document's own overrides applied.
	///
	/// Layering belongs to the host, which resolves its settings for the
	/// document at hand — including per folder and per language — and sends
	/// the result. The engine applies what it is given, so two documents
	/// served from one session can differ.
	fn options_with(&self, params: &Value) -> Result<LayoutOptions> {
		let mut options = self.options.clone();
		if params.get("template").is_some()
			|| params.get("stylesheet").is_some()
		{
			let names = params
				.get("template")
				.map(|value| {
					value
						.as_str()
						.context("template must be a name")
						.map(str::to_owned)
				})
				.transpose()?
				.into_iter()
				.collect::<Vec<_>>();
			options.stylesheet = Self::template_stylesheet(&names, params)?;
		}
		let Some(settings) = params.get("settings") else {
			return Ok(options);
		};
		let settings =
			settings.as_object().context("settings must be an object")?;
		for (name, value) in settings {
			match name.as_str() {
				"font_size" => {
					let size = as_number(value, name)?;
					if !(size.is_finite() && size > 0.0) {
						bail!("font_size must be above zero");
					}
					options.font_size = size as f32;
				}
				"width" => {
					let width = as_number(value, name)?;
					if !(width.is_finite() && width > 0.0) {
						bail!("width must be above zero");
					}
					options.width = width as f32;
				}
				"paragraph_indent" => {
					options.paragraph_indent = as_number(value, name)? as f32;
				}
				"justify" => options.justify = as_boolean(value, name)?,
				"hyphenate" => options.hyphenate = as_boolean(value, name)?,
				"codeblock_wrap" => {
					options.codeblock_wrap = as_boolean(value, name)?;
				}
				other => bail!("Unknown setting {other}"),
			}
		}
		Ok(options)
	}

	/// Whether any document has work still in flight, or geometry the client
	/// has not been told about.
	pub fn pending(&self) -> bool {
		self.documents
			.values()
			.any(|open| open.images.busy() > 0 || open.publish || open.observed)
	}

	/// Re-lays out whatever has settled and reports each document that moved.
	///
	/// Nothing here blocks: a caller with nothing pending can wait on its own
	/// input instead of spinning.
	pub fn pump(&mut self) -> Vec<String> {
		let Self {
			engine, documents, ..
		} = self;
		let mut published = Vec::new();
		for (id, open) in documents.iter_mut() {
			// A draw settles rasters without the client hearing about it,
			// because a tile answers with an image rather than a block map.
			let mut moved = std::mem::take(&mut open.publish);
			// An observed file that changed on disk replaces the text, since
			// the client handed observation back when it reported a save.
			if open.observed {
				let stamp = file_stamp(&open.path);
				if stamp != open.stamp
					&& let Ok(text) = crate::file::read_document(&open.path)
				{
					open.document = document::parse(text.clone());
					open.text = Arc::from(text);
					open.stamp = stamp;
					// The new text may name resources the old one did not, so
					// they are taken again here rather than waiting for the
					// client to ask something; the settle branch below then
					// publishes the geometry once they arrive.
					open.images.prepare(
						&open.document,
						&open.path,
						1,
						false,
						&open.options.stylesheet,
						&open.options.fonts,
					);
					moved = true;
				}
			}
			if open.images.busy() > 0 && open.images.poll() {
				moved = true;
			}
			if moved {
				open.layout = engine.layout_with_images(
					&open.document,
					&open.options,
					&open.images.snapshot,
				);
				published
					.push(json!({ "layout": state(id, open) }).to_string());
			}
		}
		published
	}

	/// Writes the document to the format the client asked for.
	///
	/// The export is the command line's own, run on the client's bytes: the
	/// text is written beside the document so a relative image resolves as it
	/// does for a file on disk, and the existing path produces the file.
	fn export(&mut self, params: &Value) -> String {
		match self.run_export(params) {
			Ok(response) => response,
			Err(error) => failure(&format!("{error:#}")),
		}
	}

	/// Preview and export use the same native template resolver.
	fn template_stylesheet(
		names: &[String],
		params: &Value,
	) -> Result<Arc<Stylesheet>> {
		let mut sheet = (*crate::export::export_stylesheet(
			names,
			markview_core::style::CjkType::Sc,
			&[],
		)?)
		.clone();
		if let Some(rules) = params.get("stylesheet") {
			sheet.merge(&Stylesheet::parse(
				rules.as_str().context("stylesheet must be MVSS text")?,
			)?);
		}
		Ok(Arc::new(sheet))
	}

	fn run_export(&mut self, params: &Value) -> Result<String> {
		let started = std::time::Instant::now();
		let id = params
			.get("id")
			.and_then(Value::as_str)
			.context("export needs an id")?;
		let output = PathBuf::from(
			params
				.get("output")
				.and_then(Value::as_str)
				.context("export needs an output path")?,
		);
		let settings = Self::export_settings(params)?;
		let sheet = Self::template_stylesheet(&settings.style, params)?;
		// Only explicit page settings override the selected template.
		let mut page = sheet.page().clone();
		if params.get("paper").is_some() {
			page.size = Some(settings.paper.clone());
		}
		if params.get("landscape").is_some() {
			page.landscape = Some(settings.landscape);
		}
		if params.get("margin").is_some() {
			page.margin = Some(settings.margin.to_vec());
		}
		let geometry =
			markview_core::paginate::PageGeometry::from_style(&page)?;
		let stylesheet = sheet;
		let open = self
			.documents
			.get(id)
			.with_context(|| format!("No document {id}"))?;
		if crate::cli::same_target(&output, &open.path) {
			bail!("The export cannot overwrite its source");
		}
		let mut fonts = open.options.fonts.clone();
		if !fonts.ignore_system_fonts {
			crate::fonts::join_download_directory(
				&mut fonts,
				crate::fonts::directory(),
			);
		}
		let format = settings.format;
		let (width, height) = match format {
			ExportFormat::Pdf => {
				let mut job = crate::export::pdf_request(
					open.path.clone(),
					output.clone(),
					&settings,
					fonts,
					markview_core::style::CjkType::Sc,
					&[],
					self.offline,
				)?;
				job.options.stylesheet = stylesheet;
				job.page = crate::export::PageOverrides {
					paper: params.get("paper").map(|_| settings.paper.clone()),
					landscape: settings.landscape,
					margin: params.get("margin").map(|_| settings.margin),
					..Default::default()
				};
				crate::pdf::export_buffer(&job, &open.text)?;
				(None, None)
			}
			ExportFormat::Png => {
				let options = crate::export::layout_options(
					&settings,
					geometry.text_px().0,
					stylesheet.clone(),
					fonts,
				);
				let image = crate::export::png_buffer(
					&open.path,
					open.text.clone(),
					options,
					self.offline,
				)?;
				let (width, height) = self.export_png(
					&output, &settings, stylesheet, geometry, image,
				)?;
				(Some(width), Some(height))
			}
		};
		let bytes = std::fs::metadata(&output).map_or(0, |meta| meta.len());
		Ok(json!({ "exported": {
			"id": id,
			"output": output.display().to_string(),
			"format": match format {
				ExportFormat::Pdf => "pdf",
				ExportFormat::Png => "png",
			},
			"width": width,
			"height": height,
			"bytes": bytes,
			"elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
		} })
		.to_string())
	}

	/// Draws the whole document into one PNG with the export's own measure.
	///
	/// A PDF is written by the command line's own PDF path, but a PNG is
	/// pixels, so the session's renderer draws it: one strip at a time, each
	/// sized to the device's texture limit, into one image.
	fn export_png(
		&mut self,
		output: &Path,
		settings: &ExportSettings,
		stylesheet: Arc<Stylesheet>,
		geometry: markview_core::paginate::PageGeometry,
		mut image: crate::export::PngDocument,
	) -> Result<(u32, u32)> {
		// The renderer is the session's, and the export borrows it with its own
		// stylesheet: what is installed on it must stay the session's, or the
		// next tile would be drawn in the export's appearance.
		let mut renderer = match self.renderer.take() {
			Some(renderer) => renderer,
			None => {
				let mut renderer = pollster::block_on(Renderer::new(None))?;
				renderer.set_stylesheet(self.options.stylesheet.clone());
				renderer
			}
		};
		let drawn = (|| -> Result<(u32, u32)> {
			let plan = crate::export::plan(
				&geometry,
				image.snapshot.height,
				settings.scale,
				renderer.max_texture_dimension_2d(),
			)?;
			let left =
				geometry.margin_pt[3] / markview_core::paginate::PT_PER_PX;
			let mut rgba =
				vec![0u8; plan.width_px as usize * plan.height_px as usize * 4];
			for tile in &plan.tiles {
				loop {
					crate::export::draw_tile(
						&mut renderer,
						&image.snapshot,
						&plan,
						&stylesheet,
						*tile,
						settings.scale,
						left,
						Theme::Light,
						&mut rgba,
					)?;
					if !image.settle() {
						break;
					}
				}
			}
			crate::export::write_png(
				output,
				&rgba,
				plan.width_px,
				plan.height_px,
			)?;
			Ok((plan.width_px, plan.height_px))
		})();
		renderer.set_stylesheet(self.options.stylesheet.clone());
		self.renderer = Some(renderer);
		drawn
	}

	/// The stylesheets a client can name for an export.
	///
	/// The ids are the engine's, so a host offers the same list the reader's
	/// own export panel does rather than a copy of it.
	fn styles(&mut self, params: &Value) -> String {
		let selected: Option<&[String]> = None;
		let _ = params;
		let directory = crate::stylesheet::directory();
		let catalog = crate::stylesheet::catalog_for(
			directory.as_deref(),
			selected,
			StyleTarget::Pdf,
		);
		let entries: Vec<Value> = catalog
			.iter()
			.map(|entry| {
				json!({
					"id": entry.id,
					"name": entry.name,
					"installed": !markview_core::style::Stylesheet::PDF_THEMES
						.contains(&entry.id.as_str()),
					"error": entry.error,
				})
			})
			.collect();
		json!({ "styles": { "templates": entries } }).to_string()
	}

	/// The export's own settings, as the request describes them.
	///
	/// Everything the client leaves out is the reader's export default, so an
	/// export nobody configured is byte for byte what the command line writes.
	fn export_settings(params: &Value) -> Result<ExportSettings> {
		let mut settings = ExportSettings::default();
		if let Some(format) = params.get("format") {
			settings.format = match format
				.as_str()
				.context("format must be pdf or png")?
			{
				"pdf" => ExportFormat::Pdf,
				"png" => ExportFormat::Png,
				other => bail!("Unknown export format {other}: use pdf or png"),
			};
		}
		if let Some(style) = params.get("style") {
			settings.style = match style {
				Value::String(name) => vec![name.clone()],
				Value::Array(names) => names
					.iter()
					.map(|name| {
						name.as_str()
							.map(str::to_owned)
							.context("style names must be strings")
					})
					.collect::<Result<Vec<_>>>()?,
				_ => bail!("style must be a name or a list of names"),
			};
		}
		if let Some(number) = params.get("font_size") {
			settings.font_size =
				number.as_f64().context("font_size must be a number")? as f32;
		}
		if let Some(number) = params.get("paragraph_indent") {
			settings.paragraph_indent = number
				.as_f64()
				.context("paragraph_indent must be a number")?
				as f32;
		}
		if let Some(number) = params.get("scale") {
			settings.scale =
				number.as_f64().context("scale must be a number")? as f32;
		}
		if let Some(paper) = params.get("paper") {
			settings.paper = paper
				.as_str()
				.context("paper must be a name or a size")?
				.to_owned();
		}
		if let Some(landscape) = params.get("landscape") {
			settings.landscape =
				landscape.as_bool().context("landscape must be a boolean")?;
		}
		if let Some(margin) = params.get("margin") {
			let values = margin
				.as_array()
				.context("margin must be a list of millimetres")?;
			if values.len() != 4 {
				bail!(
					"margin needs top, right, bottom and left in millimetres"
				);
			}
			let mut edges = [0.0f32; 4];
			for (edge, value) in edges.iter_mut().zip(values) {
				*edge = value
					.as_f64()
					.context("margin values must be numbers")? as f32;
			}
			settings.margin = edges;
		}
		settings.validate()?;
		Ok(settings)
	}

	/// Reports that the client has written the document to disk.
	///
	/// The text is the client's and stays the client's: the document is never
	/// re-read, so a save cannot revert the preview to what was on disk
	/// before. What a save can change is the local resources the document
	/// names, which the engine does read, so those are taken again and the
	/// geometry is republished if it moved.
	fn saved(&mut self, params: &Value) -> String {
		match self.refresh_resources(params) {
			Ok(response) => response,
			Err(error) => failure(&format!("{error:#}")),
		}
	}

	fn refresh_resources(&mut self, params: &Value) -> Result<String> {
		let id = params
			.get("id")
			.and_then(Value::as_str)
			.context("saved needs an id")?;
		if !self.documents.contains_key(id) {
			bail!("No document {id}");
		}
		let mut open = self.documents.remove(id).expect("checked above");
		let before = open.layout.images.entries.clone();
		// Resources resolve against the document's parent, so they are taken
		// again through the path the document was opened under.
		open.images.prepare(
			&open.document,
			&open.path,
			1,
			false,
			&open.options.stylesheet,
			&open.options.fonts,
		);
		open.images.wait();
		let mut moved =
			open.layout.images.entries != open.images.snapshot.entries;
		if moved {
			open.layout = self.engine.layout_with_images(
				&open.document,
				&open.options,
				&open.images.snapshot,
			);
			// The answer to `saved` carries no block map, so the client is
			// owed the geometry separately.
			open.publish = true;
			moved = open.layout.images.entries != before;
		}
		// The client owned the text while it was editing; reporting a save
		// hands the file back to the engine, so a later change on disk is
		// picked up rather than going unnoticed.
		open.observed = true;
		open.stamp = file_stamp(&open.path);
		let response = json!({ "saved": { "id": id, "changed": moved } });
		self.documents.insert(id.to_owned(), open);
		Ok(response.to_string())
	}

	/// Sets the appearance the client asked for and reports what it cost.
	fn appearance(&mut self, params: &Value) -> String {
		match self.set_appearance(params) {
			Ok(response) => response,
			Err(error) => failure(&format!("{error:#}")),
		}
	}

	/// A theme is a palette the renderer resolves, so changing one is a
	/// repaint. A stylesheet is the whole appearance, geometry included, so a
	/// change to it may reflow; the host is told which happened rather than
	/// being made to assume either.
	fn set_appearance(&mut self, params: &Value) -> Result<String> {
		let started = std::time::Instant::now();
		let mut sheet = None;
		if let Some(theme) = params.get("theme") {
			let dark = match theme.as_str().context("theme must be a name")? {
				"light" => false,
				"dark" => true,
				other => bail!(
					"Unknown theme {other}: a host resolves an automatic theme itself"
				),
			};
			self.theme = if dark { Theme::Dark } else { Theme::Light };
			// A theme is a palette, so it is merged onto the sheet the client
			// last set: replacing the sheet would drop the builtin defaults,
			// and taking the theme's geometry with it would reflow a document
			// for a change that only recolours it.
			let mut themed = (*self.base).clone();
			themed.merge_palette(&Stylesheet::bundled_rules(dark));
			crate::images::preserve_diagram_geometry(&mut themed, &self.base);
			sheet = Some(Arc::new(themed));
		}
		let mut base = None;
		if let Some(style) = params.get("style") {
			let source = style.as_str().context("style must be MVSS text")?;
			// Transactional: a sheet that does not parse leaves the last one in
			// place, so a client cannot leave a document unstyled.
			sheet = Some(Arc::new(
				markview_core::style::Stylesheet::parse(source)
					.context("The stylesheet is not valid MVSS")?,
			));
			// The client's own sheet is what a later theme recolours.
			base = sheet.clone();
		}
		let mut reflow = sheet.as_ref().is_some_and(|sheet| {
			sheet.layout_key() != self.options.stylesheet.layout_key()
		});
		if let Some(sheet) = sheet {
			self.options.stylesheet = sheet.clone();
			if let Some(base) = base {
				self.base = base;
			}
			for open in self.documents.values_mut() {
				if open.templated {
					continue;
				}
				let diagrams_changed = open.options.stylesheet.diagram_key()
					!= sheet.diagram_key();
				open.options.stylesheet = sheet.clone();
				if diagrams_changed {
					open.images.prepare(
						&open.document,
						&open.path,
						1,
						false,
						&sheet,
						&open.options.fonts,
					);
					open.images.wait();
					reflow |=
						open.layout.images.entries.iter().any(|(id, old)| {
							open.images.snapshot.entries.get(id).is_none_or(
								|new| {
									old.size != new.size
										|| old.error != new.error
								},
							)
						});
					open.layout.images = open.images.snapshot.clone();
				}
			}
			if let Some(renderer) = &mut self.renderer {
				renderer.set_stylesheet(sheet);
			}
		}
		if reflow {
			let Self {
				engine, documents, ..
			} = self;
			for open in documents.values_mut() {
				if open.templated {
					continue;
				}
				open.layout = engine.layout_with_images(
					&open.document,
					&open.options,
					&open.images.snapshot,
				);
				open.publish = true;
			}
		}
		Ok(json!({ "appearance": {
			"reflow": reflow,
			"elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
		} })
		.to_string())
	}

	/// Find needs characters and source ranges, never the whole document's geometry.
	fn rendered(&self, params: &Value) -> String {
		let Some(open) = params
			.get("id")
			.and_then(Value::as_str)
			.and_then(|id| self.documents.get(id))
		else {
			return failure("rendered needs an open document id");
		};
		let mut text = String::new();
		let mut ranges: Vec<[usize; 3]> = Vec::new();
		let horizontal = HashMap::new();
		for (index, block) in open.layout.blocks.iter().enumerate() {
			for node in &block.layout.text {
				for cluster in &node.clusters {
					let (offset, clip) = block.layout.command_view(
						cluster.command,
						index,
						&horizontal,
					);
					let rect = Rect {
						x: cluster.rect.x - offset,
						..cluster.rect
					};
					if clip.is_some_and(|clip| rect.intersect(clip).is_none()) {
						continue;
					}
					let source = cluster.source.as_ref().map_or_else(
						|| block.source.clone(),
						|range| {
							block.source.start + range.start
								..block.source.start + range.end
						},
					);
					let drawn = node
						.text
						.get(cluster.range.clone())
						.unwrap_or_default();
					text.push_str(drawn);
					ranges.push([
						source.start,
						source.end,
						drawn.encode_utf16().count(),
					]);
				}
			}
		}
		json!({"rendered": {"text": text, "ranges": ranges}}).to_string()
	}

	/// The text layer over a band of the document.
	fn text(&mut self, params: &Value) -> String {
		match self.text_layer(params) {
			Ok(response) => response,
			Err(error) => failure(&format!("{error:#}")),
		}
	}

	fn text_layer(&mut self, params: &Value) -> Result<String> {
		let id = params
			.get("id")
			.and_then(Value::as_str)
			.context("text needs an id")?;
		let open = self
			.documents
			.get(id)
			.with_context(|| format!("No document {id}"))?;
		// An absent bottom is the whole document below the top.
		let bottom = if params.get("bottom").is_some() {
			number(params, "bottom", 0.0)? as f32
		} else {
			open.layout.height
		};
		let top = number(params, "top", 0.0)? as f32;
		if !(top.is_finite() && top >= 0.0)
			|| !(bottom.is_finite() && bottom >= top)
		{
			bail!(
				"A text band needs a top of zero or more and a bottom below it"
			);
		}
		let (clusters, rows) = layer(&open.layout, top, bottom);
		Ok(json!({ "text": {
			"id": id,
			"clusters": clusters,
			"rows": rows,
		} })
		.to_string())
	}

	/// Renders one viewport rectangle and answers with the encoded image.
	fn tile(&mut self, params: &Value) -> Reply {
		match self.render_tile(params) {
			Ok(response) => response,
			Err(error) => failure(&format!("{error:#}")).into(),
		}
	}

	/// A tile is a pure rendering of the snapshot the document already holds:
	/// the viewport moves what is shown, not where anything sits, so the tiled
	/// layout stays the one the client was given at `open`.
	fn render_tile(&mut self, params: &Value) -> Result<Reply> {
		let id = params
			.get("id")
			.and_then(Value::as_str)
			.context("tile needs an id")?;
		// The column a tile is centred on belongs to the document, not to the
		// session, because a host can lay documents out at different widths.
		let Some(column) =
			self.documents.get(id).map(|open| open.options.width)
		else {
			bail!("No document {id}");
		};
		// A named parameter is a number when it is present, and absent only
		// when the client did not send it: a value of the wrong type is a
		// malformed request rather than a reason to fall back to a default.
		let number = |name: &str, fallback: f64| -> Result<f64> {
			number(params, name, fallback)
		};
		let width = number("width", 1200.0)?;
		let height = number("height", 800.0)?;
		let scale = number("scale", 1.0)? as f32;
		let scroll = number("scroll", 0.0)? as f32;
		// A tile is a crop of the document, so every parameter is either
		// honoured exactly or refused. Substituting a nearby value would make
		// the image the client asked for silently different from the one it
		// got, and tiles that no longer line up cannot be stitched.
		if !(width.is_finite() && width >= 1.0 && width.fract() == 0.0)
			|| !(height.is_finite() && height >= 1.0 && height.fract() == 0.0)
		{
			bail!(
				"A tile needs a whole number of pixels for width and height, at least one"
			);
		}
		if !(scale.is_finite() && scale > 0.0) {
			bail!("A tile needs a scale above zero");
		}
		if !(scroll.is_finite() && scroll >= 0.0) {
			bail!("A tile needs a scroll offset that is not negative");
		}
		let width = width as f32;
		let height = height as f32;
		let horizontal = HashMap::new();
		let view = View {
			selection: None,
			hovered_link: None,
			held_overflow: None,
			hovered_overflow: None,
			revision: 0,
			width: width as u32,
			height: height as u32,
			scale,
			scroll,
			// The column is centred, and a viewport narrower than the column
			// crops it symmetrically rather than shifting it.
			left: (width / scale - column) / 2.0,
			// The reader insets its viewport for the window's own chrome. A
			// tile has no chrome: the whole rectangle is document.
			top: 0.0,
			bottom: 0.0,
			theme: if self.documents[id].templated {
				Theme::Light
			} else {
				self.theme
			},
			horizontal: &horizontal,
		};
		let mut renderer = match self.renderer.take() {
			Some(renderer) => renderer,
			None => {
				let mut renderer = pollster::block_on(Renderer::new(None))?;
				renderer.set_stylesheet(self.options.stylesheet.clone());
				renderer
			}
		};
		// The document is taken out of the map for the duration, because
		// settling a raster needs it mutably while the layout is still being
		// read for the draw.
		let mut open = self.documents.remove(id).expect("checked above");
		renderer.set_stylesheet(open.options.stylesheet.clone());
		let drawn = self.draw_tile(&mut renderer, &mut open, &view);
		self.documents.insert(id.to_owned(), open);
		self.renderer = Some(renderer);
		let png = drawn?;
		// Match the renderer's opaque clear color from this document's sheet.
		let color = self.documents[id]
			.options
			.stylesheet
			.paint(Paint::Background);
		let background =
			[color[0], color[1], color[2]].map(|v| (v * 255.0).round() as u8);
		if png.len() > 64 * 1024 * 1024 {
			bail!("A tile PNG exceeds the 64 MiB transport limit");
		}
		Ok(Reply {
			header: json!({ "tile": {
			"id": id,
			"width": view.width,
			"height": view.height,
			"scale": view.scale,
			"scroll": view.scroll,
			"bytes": png.len(),
			"encoding": "png",
            "background": background,
		} })
			.to_string(),
			pixels: png,
		})
	}

	/// Draws one tile, settling any raster the draw itself asked for.
	///
	/// A draw publishes the size it needs an image at, and an SVG asked for a
	/// size other than its intrinsic one is only rasterized once that demand is
	/// picked up. The pass is therefore taken again after the images settle,
	/// exactly as the offscreen render path does before it writes a file.
	fn draw_tile(
		&mut self,
		renderer: &mut Renderer,
		open: &mut Open,
		view: &View<'_>,
	) -> Result<Vec<u8>> {
		let limit = renderer.max_texture_dimension_2d();
		if view.width > limit || view.height > limit {
			bail!("A tile cannot exceed the {limit} px texture limit");
		}
		let target = renderer.offscreen(view.width, view.height);
		let index = renderer.render(
			&open.layout,
			view,
			&[],
			&target.create_view(&Default::default()),
		)?;
		renderer.wait(Some(index))?;
		open.images.wait();
		if open.layout.images.entries == open.images.snapshot.entries {
			return renderer.png_bytes(&target);
		}
		open.layout = self.engine.layout_with_images(
			&open.document,
			&open.options,
			&open.images.snapshot,
		);
		// The client is holding the geometry from before this, and the tile
		// it is about to receive answers with an image rather than a block
		// map, so the session owes it a layout.
		open.publish = true;
		let index = renderer.render(
			&open.layout,
			view,
			&[],
			&target.create_view(&Default::default()),
		)?;
		renderer.wait(Some(index))?;
		renderer.png_bytes(&target)
	}
}

/// The block map: where each block sits and which bytes it came from, with the
/// links it holds resolved against the document's own directory.
fn blocks(layout: &LayoutSnapshot, directory: Option<&Path>) -> Vec<Value> {
	layout
		.blocks
		.iter()
		.map(|block| {
			json!({
				"id": block.id,
				"source_start": block.source.start,
				"source_end": block.source.end,
				"y": block.y,
				"height": block.layout.height,
				"links": links(layout, block, directory),
			})
		})
		.collect()
}

/// One clickable fragment: what it points at and where it sits.
///
/// The classification is the reader's own, so a client routes a link exactly
/// as the reader would rather than deciding for itself. A bare fragment stays
/// in the document, and `resolve` reports everything else.
fn links(
	layout: &LayoutSnapshot,
	block: &PlacedBlock,
	directory: Option<&Path>,
) -> Vec<Value> {
	block
		.layout
		.links
		.iter()
		.map(|link| {
			let (kind, target) = match (
				link.url.strip_prefix('#'),
				crate::link::resolve(&link.url, directory),
			) {
				(Some(anchor), _) => ("anchor", Some(anchor.to_owned())),
				(None, Some(Target::Remote(url))) => ("remote", Some(url)),
				(None, Some(Target::Markdown(path))) => {
					("document", Some(path.display().to_string()))
				}
				(None, Some(Target::OsDirect(path))) => {
					("file", Some(path.display().to_string()))
				}
				(None, Some(Target::Confirm(path))) => {
					("confirm", Some(path.display().to_string()))
				}
				(None, None) => ("refused", None),
			};
			json!({
				"url": link.url,
				"kind": kind,
				"target": target,
				"x": link.rect.x,
				"y": link.rect.y,
				"width": link.rect.w,
				"height": link.rect.h,
				// A fragment is resolved here rather than by the client: the
				// engine is what knows where a heading or a footnote is.
				"to": match kind {
					"anchor" => target
						.as_deref()
						.and_then(|anchor| layout.anchor_y(anchor)),
					_ => None,
				},
			})
		})
		.collect()
}

/// When a file was last written, if it can be asked.
fn file_stamp(path: &Path) -> Option<std::time::SystemTime> {
	std::fs::metadata(path)
		.and_then(|meta| meta.modified())
		.ok()
}

/// A setting value that must be a number.
fn as_number(value: &Value, name: &str) -> Result<f64> {
	value
		.as_f64()
		.with_context(|| format!("{name} must be a number"))
}

/// A setting value that must be true or false.
fn as_boolean(value: &Value, name: &str) -> Result<bool> {
	value
		.as_bool()
		.with_context(|| format!("{name} must be true or false"))
}

/// A named numeric parameter: an absent one falls back to its default, but a
/// present one of the wrong type is a malformed request.
fn number(params: &Value, name: &str, fallback: f64) -> Result<f64> {
	match params.get(name) {
		None => Ok(fallback),
		Some(value) => value
			.as_f64()
			.with_context(|| format!("{name} must be a number")),
	}
}

/// The text layer over a band of the document.
///
/// A cluster carries the geometry the engine gave it and the bytes it was set
/// from, so a client can place a selection, map a click back to the source, or
/// find the row a byte sits on without laying anything out itself. A cluster
/// whose own source range is unknown keeps the enclosing block's, which is the
/// same fallback the block map offers.
fn layer(
	layout: &LayoutSnapshot,
	top: f32,
	bottom: f32,
) -> (Vec<Value>, Vec<Value>) {
	let mut clusters = Vec::new();
	// Every byte needs a vertical position, including bytes a wide block has
	// scrolled out of sight, so the rows are published separately from the hit
	// targets rather than being whatever survived the clip.
	let mut rows: Vec<Value> = Vec::new();
	let horizontal = HashMap::new();
	for (index, block) in layout.blocks.iter().enumerate() {
		if block.y + block.layout.height < top || block.y > bottom {
			continue;
		}
		let mut occurrences = HashMap::new();
		for node in &block.layout.text {
			for cluster in &node.clusters {
				// A wide block is shown through a viewport, so a cluster the
				// viewport hides is not a target: publishing its raw rectangle
				// would let a point in the blank margin resolve to a character
				// the reader cannot see.
				let (offset, clip) = block.layout.command_view(
					cluster.command,
					index,
					&horizontal,
				);
				let mut rect = Rect {
					x: cluster.rect.x - offset,
					..cluster.rect
				};
				let y = block.y + rect.y;

				// A cluster's range is relative to the block, because a block's
				// geometry is cached by content and reused wherever the same
				// content appears; the block's own range puts it back.
				let source = match &cluster.source {
					Some(rel) => {
						block.source.start + rel.start
							..block.source.start + rel.end
					}
					None => block.source.clone(),
				};
				let occurrence = occurrences
					.entry((source.start, source.end))
					.or_insert(0usize);
				let id = format!(
					"{index}:{}:{}:{occurrence}",
					source.start, source.end
				);
				*occurrence += 1;
				if y + rect.h < top || y > bottom {
					continue;
				}
				rows.push(json!({
					"source_start": source.start,
					"source_end": source.end,
					"y": y,
					"height": rect.h,
				}));
				if let Some(clip) = clip {
					let Some(clipped) = rect.intersect(clip) else {
						continue;
					};
					rect = clipped;
				}
				clusters.push(json!({
					"id": id,
					"x": rect.x,
					"y": y,
					"width": rect.w,
					"height": rect.h,
					"text": node
						.text
						.get(cluster.range.clone())
						.unwrap_or_default(),
					"source_start": source.start,
					"source_end": source.end,
				}));
			}
		}
	}
	(clusters, rows)
}

fn failure(message: &str) -> String {
	json!({ "error": message }).to_string()
}

/// Serves until the client closes stdin.
/// How often a session with work in flight looks for it settling.
const PUMP: Duration = Duration::from_millis(8);

pub fn run(args: &LaunchOptions) -> Result<()> {
	let mut session = Session::new(args);
	session
		.engine
		.validate_stylesheet(&session.options.stylesheet)?;
	// Reading on its own thread lets the session report settling work between
	// requests instead of only when the client happens to ask something.
	let (sender, receiver) = mpsc::channel();
	std::thread::spawn(move || {
		let stdin = std::io::stdin();
		for line in stdin.lock().lines() {
			let Ok(line) = line else { break };
			if sender.send(Some(line)).is_err() {
				return;
			}
		}
		// The end of stdin ends the session, which is what keeps a host that
		// crashes or is killed from leaving a server behind.
		let _writing = crate::export::OUTPUT_WRITE.lock().unwrap();
		std::process::exit(0);
	});
	let mut stdout = std::io::stdout().lock();
	loop {
		let request = if session.pending() {
			match receiver.recv_timeout(PUMP) {
				Ok(line) => line,
				Err(RecvTimeoutError::Timeout) => None,
				Err(RecvTimeoutError::Disconnected) => break,
			}
		} else {
			match receiver.recv() {
				Ok(line) => line,
				Err(_) => break,
			}
		};
		if let Some(line) = request.filter(|line| !line.trim().is_empty()) {
			let response = session.handle(&line);
			response.write_to(&mut stdout)?;
		}
		for published in session.pump() {
			writeln!(stdout, "{published}")?;
		}
		stdout.flush()?;
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	fn session() -> Session {
		Session::new(&LaunchOptions {
			options: crate::test_support::options(),
			offline: true,
			..Default::default()
		})
	}

	fn answered(session: &mut Session, request: Value) -> Value {
		let reply = session.handle(&request.to_string());
		let mut header: Value = serde_json::from_str(&reply.header)
			.expect("a response header is JSON");
		if !reply.pixels.is_empty() {
			assert_eq!(header["tile"]["bytes"], reply.pixels.len());
			assert_eq!(header["tile"]["encoding"], "png");
			assert!(header["tile"].get("png").is_none());
			// Keep the body alongside the header for in-process pixel assertions.
			header["tile"]["png"] = json!(reply.pixels);
		}
		header
	}

	#[test]
	fn binary_reply_keeps_headers_and_png_bytes_separate() {
		let pixels = vec![137, 80, 78, 71, 13, 10, 26, 10, 0, 255, 123, 125];
		let header =
			json!({"tile": {"encoding": "png", "bytes": pixels.len()}})
				.to_string();
		let reply = Reply {
			header: header.clone(),
			pixels: pixels.clone(),
		};
		let mut wire = Vec::new();
		reply.write_to(&mut wire).unwrap();
		Reply::from(failure("next response"))
			.write_to(&mut wire)
			.unwrap();
		let boundary = header.len() + 1;
		assert_eq!(&wire[..boundary], format!("{header}\n").as_bytes());
		assert_eq!(&wire[boundary..boundary + pixels.len()], pixels);
		assert_eq!(
			&wire[boundary + pixels.len()..],
			format!("{}\n", failure("next response")).as_bytes()
		);
	}

	#[test]
	fn an_opened_document_answers_with_its_block_map() {
		let mut session = session();
		let response = answered(
			&mut session,
			json!({ "open": { "id": "d1", "text": "# Title\n\nBody text.\n" } }),
		);
		let opened = &response["opened"];
		assert_eq!(opened["id"], "d1");
		assert!(opened["height"].as_f64().unwrap() > 0.0);
		let blocks = opened["blocks"].as_array().unwrap();
		assert_eq!(blocks.len(), 2, "{blocks:?}");
		assert_eq!(blocks[0]["source_start"], 0);
		assert_eq!(blocks[1]["source_start"], 9);
	}

	#[test]
	fn the_text_comes_from_the_client_not_the_filesystem() {
		let mut session = session();
		let response = answered(
			&mut session,
			json!({ "open": {
				"id": "d1",
				"text": "A paragraph that exists nowhere on disk.\n",
			} }),
		);
		assert!(response.get("error").is_none(), "{response}");
		assert_eq!(response["opened"]["blocks"].as_array().unwrap().len(), 1);
	}

	#[test]
	fn a_bad_request_is_answered_and_the_session_survives() {
		let mut session = session();
		assert!(
			answered(&mut session, json!("not an object"))["error"].is_string()
		);
		assert!(
			answered(&mut session, json!({ "nonsense": {} }))["error"]
				.is_string()
		);
		let response = answered(
			&mut session,
			json!({ "open": { "id": "d1", "text": "still here\n" } }),
		);
		assert!(response.get("opened").is_some(), "{response}");
	}

	#[test]
	fn closing_an_unknown_document_is_an_error() {
		let mut session = session();
		let response =
			answered(&mut session, json!({ "close": { "id": "d9" } }));
		assert!(response["error"].is_string(), "{response}");
	}

	#[test]
	fn two_documents_are_kept_apart() {
		let mut session = session();
		answered(
			&mut session,
			json!({ "open": { "id": "a", "text": "one\n" } }),
		);
		answered(
			&mut session,
			json!({ "open": { "id": "b", "text": "one\n\ntwo\n" } }),
		);
		let closed = answered(&mut session, json!({ "close": { "id": "a" } }));
		assert_eq!(closed["closed"]["id"], "a");
		assert!(
			answered(&mut session, json!({ "close": { "id": "b" } }))
				.get("closed")
				.is_some()
		);
	}

	/// A 1x1 PNG, so a test needs no fixture on disk.
	fn tiny_png() -> Vec<u8> {
		let mut bytes = std::io::Cursor::new(Vec::new());
		image::DynamicImage::new_rgb8(1, 1)
			.write_to(&mut bytes, image::ImageFormat::Png)
			.expect("a PNG encodes");
		bytes.into_inner()
	}

	#[test]
	fn an_embedded_image_reaches_the_layout() {
		use base64::Engine as _;
		let encoded =
			base64::engine::general_purpose::STANDARD.encode(tiny_png());
		let text =
			format!("Before.\n\n![dot](data:image/png;base64,{encoded})\n");
		let response = answered(
			&mut session(),
			json!({ "open": { "id": "d1", "text": text } }),
		);
		assert!(response.get("error").is_none(), "{response}");
		assert_eq!(response["opened"]["images"], 1, "{response}");
	}

	#[test]
	fn a_relative_image_resolves_against_the_named_path() {
		let directory = tempfile::tempdir().expect("a temporary directory");
		std::fs::write(directory.path().join("dot.png"), tiny_png())
			.expect("the image is written");
		let document = directory.path().join("doc.md");
		let response = answered(
			&mut session(),
			json!({ "open": {
				"id": "d1",
				"path": document.to_string_lossy(),
				"text": "![dot](dot.png)\n",
			} }),
		);
		assert!(response.get("error").is_none(), "{response}");
		assert_eq!(response["opened"]["images"], 1, "{response}");
	}

	#[test]
	fn a_code_block_settles_its_highlighting_before_the_answer() {
		let mut session = session();
		let response = answered(
			&mut session,
			json!({ "open": {
				"id": "d1",
				"text": "```rust\nfn main() { println!(\"hi\"); }\n```\n",
			} }),
		);
		assert!(response.get("error").is_none(), "{response}");
		// The answer carries the highlighted geometry, so nothing is left
		// pending that the client would have to ask for again.
		assert!(
			!session.engine.wait_highlights(),
			"highlighting was still pending once open had answered",
		);
	}

	#[test]
	fn links_are_classified_the_way_the_reader_would() {
		let directory = tempfile::tempdir().expect("a temporary directory");
		let document = directory.path().join("doc.md");
		let response = answered(
			&mut session(),
			json!({ "open": {
				"id": "d1",
				"path": document.to_string_lossy(),
				"text": "[web](https://example.com) [doc](other.md) [part](#one)\n",
			} }),
		);
		let links = response["opened"]["blocks"][0]["links"]
			.as_array()
			.expect("a block carries its links");
		let kinds: Vec<&str> = links
			.iter()
			.map(|link| link["kind"].as_str().expect("a kind"))
			.collect();
		assert_eq!(kinds, ["remote", "document", "anchor"], "{response}");
		// The reader passes a remote link through `Url`, which normalizes it.
		assert_eq!(links[0]["target"], "https://example.com/");
		// A relative link resolves against the document's own directory, so a
		// document that has never been saved still routes its links.
		let expected = document.with_file_name("other.md");
		assert_eq!(
			links[1]["target"].as_str(),
			Some(expected.to_string_lossy().as_ref())
		);
		assert_eq!(links[2]["target"], "one");
	}

	#[test]
	fn one_state_directory_moves_everything_this_instance_owns() {
		let root = tempfile::tempdir().expect("a temporary directory");
		crate::settings::set_state_dir(root.path().to_path_buf());
		assert_eq!(
			crate::settings::config_path(),
			Some(root.path().join("settings.toml"))
		);
		assert_eq!(
			crate::stylesheet::directory(),
			Some(root.path().join("styles"))
		);
		assert_eq!(crate::fonts::directory(), Some(root.path().join("fonts")));
		assert_eq!(
			crate::images::cache::directory(),
			Some(root.path().join("cache/images"))
		);
	}

	fn decode_tile(response: &Value) -> Vec<u8> {
		serde_json::from_value(response["tile"]["png"].clone())
			.expect("PNG bytes")
	}

	fn png_size(png: &[u8]) -> (u32, u32) {
		let width = u32::from_be_bytes(png[16..20].try_into().expect("width"));
		let height =
			u32::from_be_bytes(png[20..24].try_into().expect("height"));
		(width, height)
	}

	fn open_paragraphs(session: &mut Session, count: usize) {
		let text: String =
			(0..count).map(|i| format!("Paragraph {i}.\n\n")).collect();
		let response =
			answered(session, json!({ "open": { "id": "d1", "text": text } }));
		assert!(response.get("error").is_none(), "{response}");
	}

	#[test]
	#[ignore = "requires a GPU; run the serve tests with --ignored"]
	fn a_tile_answers_with_an_image_of_the_requested_size() {
		let mut session = session();
		open_paragraphs(&mut session, 4);
		let response = answered(
			&mut session,
			json!({ "tile": {
				"id": "d1",
				"width": 900,
				"height": 200,
				"scale": 1.0,
				"scroll": 0.0,
			} }),
		);
		assert!(response.get("error").is_none(), "{response}");
		let png = decode_tile(&response);
		assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
		assert_eq!(png_size(&png), (900, 200));
		assert_eq!(response["tile"]["bytes"], png.len());
	}

	#[test]
	#[ignore = "requires a GPU; run the serve tests with --ignored"]
	fn a_tile_follows_the_scroll_offset() {
		let mut session = session();
		open_paragraphs(&mut session, 200);
		let tile = |session: &mut Session, scroll: f64| {
			answered(
				session,
				json!({ "tile": {
					"id": "d1",
					"width": 900,
					"height": 300,
					"scale": 1.0,
					"scroll": scroll,
				} }),
			)
		};
		let first = tile(&mut session, 0.0);
		let later = tile(&mut session, 900.0);
		assert!(first.get("error").is_none(), "{first}");
		assert!(later.get("error").is_none(), "{later}");
		assert_ne!(decode_tile(&first), decode_tile(&later));
	}

	#[test]
	fn a_tile_for_an_unknown_document_is_an_error() {
		let mut session = session();
		let response =
			answered(&mut session, json!({ "tile": { "id": "d9" } }));
		assert!(response["error"].is_string(), "{response}");
	}

	/// Whether every pixel is the same, which is what a crop of empty margin
	/// looks like.
	fn is_blank(png: &[u8]) -> bool {
		let image = image::load_from_memory(png)
			.expect("a decodable png")
			.to_rgba8();
		let first = *image.get_pixel(0, 0);
		image.pixels().all(|pixel| *pixel == first)
	}

	#[test]
	#[ignore = "requires a GPU; run the serve tests with --ignored"]
	fn a_scrolled_band_carries_the_text_that_is_there() {
		let mut session = session();
		open_paragraphs(&mut session, 40);
		// A band far shorter than any window inset still shows its text, so a
		// tile is a crop of the document rather than a window over it.
		let response = answered(
			&mut session,
			json!({ "tile": {
				"id": "d1",
				"width": 900,
				"height": 40,
				"scale": 1.0,
				"scroll": 100.0,
			} }),
		);
		assert!(response.get("error").is_none(), "{response}");
		assert!(
			!is_blank(&decode_tile(&response)),
			"a 40 px band at scroll 100 came back blank",
		);
	}

	#[test]
	#[ignore = "requires a GPU; run the serve tests with --ignored"]
	fn a_tile_honours_exactly_what_was_asked_for() {
		let mut session = session();
		open_paragraphs(&mut session, 4);
		let response = answered(
			&mut session,
			json!({ "tile": {
				"id": "d1",
				"width": 20,
				"height": 40,
				"scale": 0.25,
				"scroll": 0.0,
			} }),
		);
		assert!(response.get("error").is_none(), "{response}");
		assert_eq!(response["tile"]["width"], 20);
		assert_eq!(response["tile"]["height"], 40);
		assert_eq!(response["tile"]["scale"], 0.25);
		assert_eq!(png_size(&decode_tile(&response)), (20, 40));
	}

	#[test]
	#[ignore = "requires a GPU; run the serve tests with --ignored"]
	fn a_tile_refuses_what_it_cannot_honour() {
		let mut session = session();
		open_paragraphs(&mut session, 4);
		for request in [
			json!({ "tile": { "id": "d1", "width": 0, "height": 40 } }),
			json!({ "tile": { "id": "d1", "width": 900, "height": 0 } }),
			json!({ "tile": {
				"id": "d1", "width": 900, "height": 40, "scale": 0.0 } }),
			json!({ "tile": {
				"id": "d1", "width": 900, "height": 40, "scroll": -1.0 } }),
			json!({ "tile": {
				"id": "d1", "width": 1_000_000, "height": 40 } }),
			// A pixel count is whole, and a wrongly typed parameter is a
			// malformed request rather than a reason to use a default.
			json!({ "tile": { "id": "d1", "width": 20.4, "height": 40 } }),
			json!({ "tile": { "id": "d1", "width": "20", "height": 40 } }),
			json!({ "tile": {
				"id": "d1", "width": 900, "height": 40, "scale": "0.25" } }),
			json!({ "tile": {
				"id": "d1", "width": 900, "height": 40, "scroll": null } }),
		] {
			let response = answered(&mut session, request.clone());
			assert!(
				response["error"].is_string(),
				"{request} was answered with {response}",
			);
		}
	}

	#[test]
	#[ignore = "requires a GPU; run the serve tests with --ignored"]
	fn a_tile_falls_back_only_when_a_parameter_is_absent() {
		let mut session = session();
		open_paragraphs(&mut session, 4);
		let response =
			answered(&mut session, json!({ "tile": { "id": "d1" } }));
		assert!(response.get("error").is_none(), "{response}");
		assert_eq!(response["tile"]["width"], 1200);
		assert_eq!(response["tile"]["height"], 800);
		assert_eq!(response["tile"]["scale"], 1.0);
		assert_eq!(response["tile"]["scroll"], 0.0);
	}

	/// A session with one image beside the document, opened either waiting for
	/// it or not. The temporary directory is handed back so it outlives the
	/// session that reads from it.
	fn image_session(settle: bool) -> (Session, Value, tempfile::TempDir) {
		let directory = tempfile::tempdir().expect("a temporary directory");
		std::fs::write(directory.path().join("dot.png"), tiny_png())
			.expect("the image is written");
		let document = directory.path().join("doc.md");
		let mut session = session();
		let response = answered(
			&mut session,
			json!({ "open": {
				"id": "d1",
				"path": document.to_string_lossy(),
				"text": "![dot](dot.png)\n",
				"settle": settle,
			} }),
		);
		(session, response, directory)
	}

	#[test]
	fn a_document_can_answer_before_its_images_settle() {
		let (mut session, response, _directory) = image_session(false);
		let opened = &response["opened"];
		assert!(opened.get("error").is_none(), "{response}");
		assert!(opened["complete"].is_boolean(), "{response}");
		// The session keeps reporting until nothing is left in flight, so a
		// client that asked not to wait still learns the settled geometry.
		for _ in 0..400 {
			session.pump();
			if !session.pending() {
				return;
			}
			std::thread::sleep(Duration::from_millis(5));
		}
		panic!("the image never settled");
	}

	#[test]
	fn a_progressive_session_converges_on_the_settled_layout() {
		let (mut progressive, opened, _a) = image_session(false);
		let (_, settled, _b) = image_session(true);
		let mut latest = opened["opened"].clone();
		for _ in 0..400 {
			for published in progressive.pump() {
				let value: Value =
					serde_json::from_str(&published).expect("json");
				latest = value["layout"].clone();
			}
			if !progressive.pending() {
				break;
			}
			std::thread::sleep(Duration::from_millis(5));
		}
		// However the client asked, the answer it ends with is the same one.
		assert_eq!(latest["height"], settled["opened"]["height"]);
		assert_eq!(latest["images"], settled["opened"]["images"]);
		assert_eq!(latest["complete"], true);
	}

	#[test]
	#[ignore = "requires a GPU; run the serve tests with --ignored"]
	fn a_tile_during_settling_still_reaches_the_client() {
		let (mut progressive, opened, _a) = image_session(false);
		let (_, settled, _b) = image_session(true);
		// The tile is asked for before the images have settled, and answering
		// it settles them, so the geometry it changes has to be published.
		let tile = answered(
			&mut progressive,
			json!({ "tile": {
				"id": "d1",
				"width": 900,
				"height": 400,
			} }),
		);
		assert!(tile.get("error").is_none(), "{tile}");
		let mut latest = opened["opened"].clone();
		for _ in 0..400 {
			for published in progressive.pump() {
				let value: Value =
					serde_json::from_str(&published).expect("json");
				latest = value["layout"].clone();
			}
			if !progressive.pending() {
				break;
			}
			std::thread::sleep(Duration::from_millis(5));
		}
		assert_eq!(
			latest["height"], settled["opened"]["height"],
			"the client never learned the geometry the tile settled",
		);
		assert_eq!(latest["images"], settled["opened"]["images"]);
		assert_eq!(latest["complete"], true);
	}

	#[test]
	fn settle_must_be_a_boolean_when_it_is_present() {
		let mut session = session();
		let response = answered(
			&mut session,
			json!({ "open": { "id": "d1", "text": "hi\n", "settle": "no" } }),
		);
		assert_eq!(response["error"], "settle must be true or false");
	}

	/// A draw publishes the size it needs an image at, and an SVG shown larger
	/// than its intrinsic size is rasterized only once that demand is picked
	/// up. A tile therefore has to settle what its own draw asked for, the way
	/// the offscreen render path does before it writes a file.
	#[test]
	#[ignore = "requires a GPU; run the serve tests with --ignored"]
	fn a_tile_settles_the_raster_its_own_draw_asks_for() {
		let directory = tempfile::tempdir().expect("a temporary directory");
		let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\">\
			<circle cx=\"5\" cy=\"5\" r=\"4\" fill=\"red\"/></svg>";
		std::fs::write(directory.path().join("shape.svg"), svg)
			.expect("the image is written");
		let document = directory.path().join("doc.md");
		let mut session = session();
		let opened = answered(
			&mut session,
			json!({ "open": {
				"id": "d1",
				"path": document.to_string_lossy(),
				"text": "<img src=\"shape.svg\" width=\"200\" height=\"200\">\n",
			} }),
		);
		assert!(opened["opened"].get("error").is_none(), "{opened}");
		let response = answered(
			&mut session,
			json!({ "tile": {
				"id": "d1",
				"width": 900,
				"height": 400,
				"scale": 1.0,
				"scroll": 0.0,
			} }),
		);
		assert!(response.get("error").is_none(), "{response}");
		// Nothing the draw asked for is left outstanding once the tile has
		// answered, which is what lets it match the offscreen render.
		let open = session.documents.get("d1").expect("the document is open");
		assert_eq!(
			open.layout.images.entries, open.images.snapshot.entries,
			"the tile answered with rasters still outstanding",
		);
	}

	fn text_layer(session: &mut Session, request: Value) -> Vec<Value> {
		let response = answered(session, request);
		assert!(response.get("error").is_none(), "{response}");
		response["text"]["clusters"]
			.as_array()
			.expect("clusters")
			.clone()
	}

	#[test]
	fn merged_runs_keep_interior_source_ranges() {
		for text in [
			"a &amp; b <!-- hidden --> c d",
			"a b <!-- hidden --> c d",
			"a b<!-- hidden -->c d",
		] {
			let mut session = session();
			answered(
				&mut session,
				json!({"open": {"id": "doc", "text": text}}),
			);
			let clusters =
				text_layer(&mut session, json!({"text": {"id": "doc"}}));
			for word in ["b", "c", "d"] {
				let cluster = clusters
					.iter()
					.find(|cluster| cluster["text"] == word)
					.expect("letter");
				let at = text.rfind(word).unwrap();
				assert_eq!(cluster["source_start"], at, "{text}: {cluster}");
				assert_eq!(cluster["source_end"], at + 1, "{text}: {cluster}");
			}
		}
	}

	#[test]
	fn decoded_entity_clusters_have_distinct_stable_ids() {
		let mut session = session();
		answered(
			&mut session,
			json!({"open": {"id": "doc", "text": "a &fjlig; b"}}),
		);
		let clusters = text_layer(&mut session, json!({"text": {"id": "doc"}}));
		let entity: Vec<_> = clusters
			.iter()
			.filter(|cluster| cluster["source_start"] == 2)
			.collect();
		assert_eq!(entity.len(), 2);
		assert_ne!(entity[0]["id"], entity[1]["id"]);
		let repeated = text_layer(
			&mut session,
			json!({"text": {"id": "doc", "top": 0, "bottom": 100}}),
		);
		assert_eq!(clusters, repeated);
	}

	/// Text whose reading and source are the same length maps offset for
	/// offset, so each cluster's own range is exactly the characters it shows.
	#[test]
	fn a_text_cluster_points_back_at_the_bytes_it_was_set_from() {
		for (text, reading) in [
			("Plain words here.\n", "Plain words here."),
			(
				"A **bold** word and a [link](other.md).\n",
				"A bold word and a link.",
			),
		] {
			let mut session = session();
			answered(
				&mut session,
				json!({ "open": { "id": "d1", "text": text } }),
			);
			let clusters =
				text_layer(&mut session, json!({ "text": { "id": "d1" } }));
			assert!(!clusters.is_empty(), "{text}");
			let shown: String = clusters
				.iter()
				.map(|cluster| cluster["text"].as_str().expect("text"))
				.collect();
			assert_eq!(shown, reading, "{text}");
			for cluster in &clusters {
				let start =
					cluster["source_start"].as_u64().expect("start") as usize;
				let end = cluster["source_end"].as_u64().expect("end") as usize;
				assert_eq!(
					&text[start..end],
					cluster["text"].as_str().expect("text"),
					"{cluster}",
				);
			}
		}
	}

	/// A block's geometry is cached by content and reused wherever the same
	/// content appears, so a repeated block has to resolve to its own bytes
	/// rather than to the first occurrence's.
	#[test]
	fn a_repeated_block_resolves_to_its_own_source() {
		let text = "Same.\n\nSame.\n";
		let mut session = session();
		answered(
			&mut session,
			json!({ "open": { "id": "d1", "text": text } }),
		);
		let clusters =
			text_layer(&mut session, json!({ "text": { "id": "d1" } }));
		for cluster in &clusters {
			let start =
				cluster["source_start"].as_u64().expect("start") as usize;
			let end = cluster["source_end"].as_u64().expect("end") as usize;
			assert_eq!(
				&text[start..end],
				cluster["text"].as_str().expect("text"),
				"{cluster}",
			);
		}
		let last = clusters.last().expect("a cluster");
		assert_eq!(last["source_start"], 11, "{last}");
	}

	/// A decoded character still owns the bytes it was written as, so an
	/// entity reference is one cluster's range rather than the whole paragraph.
	#[test]
	fn a_decoded_character_keeps_the_source_it_was_written_as() {
		let text = "Fish &amp; chips.\n";
		let mut session = session();
		answered(
			&mut session,
			json!({ "open": { "id": "d1", "text": text } }),
		);
		let clusters =
			text_layer(&mut session, json!({ "text": { "id": "d1" } }));
		let shown: String = clusters
			.iter()
			.map(|cluster| cluster["text"].as_str().expect("text"))
			.collect();
		assert_eq!(shown, "Fish & chips.");
		let ampersand = clusters
			.iter()
			.find(|cluster| cluster["text"] == "&")
			.expect("the ampersand");
		let start = ampersand["source_start"].as_u64().expect("start") as usize;
		let end = ampersand["source_end"].as_u64().expect("end") as usize;
		assert_eq!(&text[start..end], "&amp;", "{ampersand}");
	}

	/// A code block maps onto its contents however it is written, and the
	/// positions are checked rather than the text, because a search for the
	/// text alone would happily land on a fence's language or a container's
	/// marker, which read the same as the body they precede.
	#[test]
	fn a_code_block_maps_onto_its_body_however_it_is_written() {
		for (text, first, last) in [
			("```rust\nfn main() {}\n```\n", ("f", 8), ("}", 19)),
			// A language that repeats the body must not be matched instead.
			("```rust\nrust\n```\n", ("r", 8), ("t", 11)),
			("  ```\n  abc\n  def\n  ```\n", ("a", 8), ("f", 16)),
			("> ```\n> abc\n> def\n> ```\n", ("a", 8), ("f", 16)),
			// The literal keeps one quote marker, so its text starts at the
			// second one and not at the container's.
			("> ```\n> > abc\n> ```\n", (">", 8), ("c", 12)),
			("    abc\n    def\n", ("a", 4), ("f", 14)),
			("- item\n\n  ```\n  abc\n  ```\n", ("a", 16), ("c", 18)),
			("```\nx\n```\n\n```\nx\n```\n", ("x", 4), ("x", 15)),
		] {
			let mut session = session();
			answered(
				&mut session,
				json!({ "open": { "id": "d1", "text": text } }),
			);
			let clusters =
				text_layer(&mut session, json!({ "text": { "id": "d1" } }));
			assert!(!clusters.is_empty(), "{text}");
			for (character, at) in [first, last] {
				let found = clusters.iter().any(|cluster| {
					cluster["text"] == character
						&& cluster["source_start"].as_u64() == Some(at)
				});
				assert!(
					found,
					"{text}: {character} is not at {at} in {clusters:?}"
				);
			}
			for cluster in &clusters {
				let end = cluster["source_end"].as_u64().expect("end") as usize;
				assert!(
					end <= text.len(),
					"{text}: {cluster} runs past the end"
				);
			}
		}
	}

	/// Every entity maps back to the bytes it was written as, including the
	/// ones that decode to more than one character and the numeric forms.
	#[test]
	fn an_entity_maps_back_to_what_it_was_written_as() {
		for (text, entity) in [
			("A &amp; B.\n", "&amp;"),
			("A &fjlig; B.\n", "&fjlig;"),
			("A &NotEqualTilde; B.\n", "&NotEqualTilde;"),
			("A &#65; B.\n", "&#65;"),
			("A &#x42; B.\n", "&#x42;"),
		] {
			let mut session = session();
			answered(
				&mut session,
				json!({ "open": { "id": "d1", "text": text } }),
			);
			let clusters =
				text_layer(&mut session, json!({ "text": { "id": "d1" } }));
			let from = text.find(entity).expect("the entity") as u64;
			let to = from + entity.len() as u64;
			let inside: Vec<(u64, u64)> = clusters
				.iter()
				.filter_map(|cluster| {
					let start = cluster["source_start"].as_u64()?;
					let end = cluster["source_end"].as_u64()?;
					(from..to).contains(&start).then_some((start, end))
				})
				.collect();
			assert!(!inside.is_empty(), "{text}: {clusters:?}");
			let lowest = inside.iter().map(|(s, _)| *s).min();
			let highest = inside.iter().map(|(_, e)| *e).max();
			assert_eq!(
				(lowest, highest),
				(Some(from), Some(to)),
				"{text}: {inside:?}",
			);
		}
	}

	/// A cache keyed by content must tell apart blocks whose text sits in
	/// different places, which only shows when both are opened in one session.
	#[test]
	fn successive_opens_do_not_borrow_each_others_ranges() {
		let mut session = session();
		let mut seen = Vec::new();
		for text in ["# a\n", "#  a\n", "> a\n\n>  a\n"] {
			answered(
				&mut session,
				json!({ "open": { "id": "d1", "text": text } }),
			);
			let clusters =
				text_layer(&mut session, json!({ "text": { "id": "d1" } }));
			for cluster in &clusters {
				let start =
					cluster["source_start"].as_u64().expect("start") as usize;
				let end = cluster["source_end"].as_u64().expect("end") as usize;
				assert_eq!(
					&text[start..end],
					cluster["text"].as_str().expect("text"),
					"{text}: {cluster}",
				);
				seen.push((text.to_owned(), start));
			}
		}
		assert!(seen.contains(&("#  a\n".to_owned(), 3)), "{seen:?}");
	}

	/// Settings are per document, because the host resolves them for the
	/// document at hand; one session can serve documents that differ.
	#[test]
	fn documents_can_be_served_with_different_settings() {
		let text = "A paragraph long enough to wrap at either size.\n";
		let mut session = session();
		let height = |session: &mut Session, id: &str, size: f64| {
			let response = answered(
				session,
				json!({ "open": { "id": id, "text": text,
					"settings": { "font_size": size } } }),
			);
			assert!(response.get("error").is_none(), "{response}");
			response["opened"]["height"].as_f64().expect("height")
		};
		let small = height(&mut session, "small", 12.0);
		let large = height(&mut session, "large", 30.0);
		assert!(large > small, "{small} is not below {large}");
	}

	#[test]
	fn a_setting_that_is_not_understood_is_an_error() {
		let mut session = session();
		for settings in [
			json!({ "font_size": "big" }),
			json!({ "font_size": 0 }),
			json!({ "width": -1 }),
			json!({ "hyphenate": "yes" }),
			json!({ "nonsense": 1 }),
		] {
			let response = answered(
				&mut session,
				json!({ "open": { "id": "d1", "text": "hi\n",
					"settings": settings } }),
			);
			assert!(
				response["error"].is_string(),
				"{settings} was answered with {response}",
			);
		}
	}

	/// A document keeps the settings it was opened with, because every later
	/// layout — a settling image, a tile redraw — answers for that document
	/// and not for the session.
	#[test]
	fn a_documents_settings_survive_every_layout() {
		let mut session = session();
		let response = answered(
			&mut session,
			json!({ "open": { "id": "d1",
				"text": "A paragraph.\n",
				"settings": { "font_size": 24.0, "width": 300.0, "justify": true } } }),
		);
		assert!(response.get("error").is_none(), "{response}");
		let open = session.documents.get("d1").expect("the document");
		assert_eq!(open.options.font_size, 24.0);
		assert_eq!(open.options.width, 300.0);
		assert!(open.options.justify);
	}

	/// The text is the client's: a file at the document's path is never read
	/// for it, so a save cannot revert the preview to what was on disk.
	#[test]
	fn a_save_never_reverts_the_preview_to_the_file() {
		let directory = tempfile::tempdir().expect("a temporary directory");
		let document = directory.path().join("doc.md");
		std::fs::write(&document, "On disk before.\n").expect("written");
		let mut session = session();
		let opened = answered(
			&mut session,
			json!({ "open": {
				"id": "d1",
				"path": document.to_string_lossy(),
				"text": "In the buffer.\n",
			} }),
		);
		assert!(opened.get("error").is_none(), "{opened}");
		let shown = |session: &mut Session| -> String {
			text_layer(session, json!({ "text": { "id": "d1" } }))
				.iter()
				.map(|cluster| cluster["text"].as_str().expect("text"))
				.collect()
		};
		assert_eq!(shown(&mut session), "In the buffer.");
		// The file changes underneath, and the client reports a save.
		std::fs::write(&document, "On disk after.\n").expect("written");
		let saved = answered(&mut session, json!({ "saved": { "id": "d1" } }));
		assert!(saved.get("error").is_none(), "{saved}");
		assert_eq!(shown(&mut session), "In the buffer.");
	}

	/// Reporting a save hands the file back to the engine, so a change made
	/// on disk afterwards is picked up and published.
	#[test]
	fn a_save_restores_observation_of_the_file() {
		let directory = tempfile::tempdir().expect("a temporary directory");
		let document = directory.path().join("doc.md");
		std::fs::write(&document, "First.\n").expect("written");
		let mut session = session();
		answered(
			&mut session,
			json!({ "open": {
				"id": "d1",
				"path": document.to_string_lossy(),
				"text": "First.\n",
			} }),
		);
		// Nothing is observed while the client owns the text.
		assert!(!session.documents.get("d1").expect("the document").observed);
		let saved = answered(&mut session, json!({ "saved": { "id": "d1" } }));
		assert!(saved.get("error").is_none(), "{saved}");
		assert!(session.documents.get("d1").expect("the document").observed);

		// A change on disk now reaches the preview without being asked for,
		// and the resources it newly names are taken again with it.
		std::fs::write(directory.path().join("pic.png"), tiny_png())
			.expect("written");
		std::thread::sleep(Duration::from_millis(20));
		std::fs::write(&document, "Second.\n\n![new](pic.png)\n")
			.expect("written");
		let mut published = Vec::new();
		for _ in 0..200 {
			published.extend(session.pump());
			let open = session.documents.get("d1").expect("the document");
			if !published.is_empty() && open.images.busy() == 0 {
				break;
			}
			std::thread::sleep(Duration::from_millis(10));
		}
		assert!(!published.is_empty(), "the change was never published");
		// The image the new text named arrived without the client asking
		// anything further, rather than staying a placeholder for ever.
		let open = session.documents.get("d1").expect("the document");
		assert_eq!(
			open.layout.images.entries.len(),
			1,
			"the image the new text named never arrived",
		);
		assert_eq!(open.images.busy(), 0, "the image never settled");
		let shown: String =
			text_layer(&mut session, json!({ "text": { "id": "d1" } }))
				.iter()
				.map(|cluster| cluster["text"].as_str().expect("text"))
				.collect();
		assert!(shown.starts_with("Second."), "{shown}");
	}

	/// The client's bytes are exported, not the file's, and the export is the
	/// command line's own path rather than a second implementation of it.
	#[test]
	fn an_export_writes_the_clients_bytes() {
		let directory = tempfile::tempdir().expect("a temporary directory");
		let document = directory.path().join("doc.md");
		std::fs::write(&document, "On disk.\n").expect("written");
		let mut session = session();
		answered(
			&mut session,
			json!({ "open": {
				"id": "d1",
				"path": document.to_string_lossy(),
				"text": "# In the buffer\n\nNot what the file holds.\n",
			} }),
		);
		let output = directory.path().join("out.pdf");
		let response = answered(
			&mut session,
			json!({ "export": {
				"id": "d1",
				"output": output.to_string_lossy(),
			} }),
		);
		assert!(response.get("error").is_none(), "{response}");
		let written = std::fs::read(&output).expect("the export is written");
		assert!(written.starts_with(b"%PDF"), "not a PDF");
		assert_eq!(
			response["exported"]["bytes"].as_u64(),
			Some(written.len() as u64),
		);
		// Nothing is left beside the document.
		let leftovers: Vec<_> = std::fs::read_dir(directory.path())
			.expect("the directory is readable")
			.filter_map(|entry| entry.ok())
			.map(|entry| entry.file_name().to_string_lossy().into_owned())
			.filter(|name| name.starts_with(".markview-export-"))
			.collect();
		assert!(leftovers.is_empty(), "{leftovers:?}");
	}

	/// The exporter's template reaches both formats: a client that names one
	/// gets it, and a client that holds rules without installing them gets
	/// those too.
	#[test]
	#[ignore = "requires a GPU; run the serve tests with --ignored"]
	fn an_export_honours_the_template_it_is_given() {
		let directory = tempfile::tempdir().expect("a temporary directory");
		let document = directory.path().join("doc.md");
		std::fs::write(&document, "One line.\n").expect("written");
		let mut session = session();
		answered(
			&mut session,
			json!({ "open": {
				"id": "d1",
				"path": document.to_string_lossy(),
				"text": "# Heading\n\nA paragraph with **bold** words.\n",
			} }),
		);
		let export = |session: &mut Session, name: &str, extra: Value| {
			let output =
				directory.path().join(name).to_string_lossy().to_string();
			let mut request = json!({ "export": {
				"id": "d1",
				"output": output,
			} });
			let fields = request["export"].as_object_mut().expect("an object");
			for (key, value) in extra.as_object().expect("an object") {
				fields.insert(key.clone(), value.clone());
			}
			let response = answered(session, request);
			assert!(response.get("error").is_none(), "{response}");
			std::fs::read(&output).expect("the export is written")
		};

		let plain = export(&mut session, "plain.pdf", json!({}));
		let named =
			export(&mut session, "named.pdf", json!({ "style": "mondrian" }));
		assert_ne!(plain, named, "the named template did not reach the PDF");

		// A template the client holds but never installed travels as rules.
		let held = "format_version = 2\ntargets = [\"pdf\"]\nversion = 1\n\
			[[rule]]\nwhen = [\"body\"]\ncolor = \"#0B3D2E\"\nline_height = 1.9\n";
		let inline =
			export(&mut session, "inline.pdf", json!({ "stylesheet": held }));
		assert_ne!(plain, inline, "the held template did not reach the PDF");

		let png = export(&mut session, "plain.png", json!({ "format": "png" }));
		assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"), "not a PNG");
		// The renderer is the session's own, so a drawn band is the reference
		// an export must not disturb: the export draws with its own stylesheet,
		// and the session's has to be what the next tile is drawn with.
		let tile = |session: &mut Session| {
			let response = answered(
				session,
				json!({ "tile": { "id": "d1", "width": 400, "height": 200 } }),
			);
			assert!(response.get("error").is_none(), "{response}");
			decode_tile(&response)
		};
		let before = tile(&mut session);
		let styled = export(
			&mut session,
			"styled.png",
			json!({ "format": "png", "stylesheet": held }),
		);
		assert_ne!(png, styled, "the held template did not reach the PNG");
		assert_eq!(
			tile(&mut session),
			before,
			"the export's template leaked into the preview's appearance",
		);

		let bad = answered(
			&mut session,
			json!({ "export": {
				"id": "d1",
				"output": directory.path().join("bad.pdf").to_string_lossy(),
				"format": "gif",
			} }),
		);
		assert!(
			bad["error"].as_str().is_some_and(|e| e.contains("gif")),
			"{bad}"
		);
		let missing = answered(
			&mut session,
			json!({ "export": {
				"id": "d1",
				"output": directory.path().join("missing.pdf").to_string_lossy(),
				"style": "no-such-template",
			} }),
		);
		assert!(missing.get("error").is_some(), "{missing}");
	}

	/// A host offers the templates the engine has, rather than a copy of the
	/// list that can drift from it.
	#[test]
	fn the_templates_a_host_may_name_are_the_engines_own() {
		let mut session = session();
		let response = answered(&mut session, json!({ "styles": {} }));
		let templates = response["styles"]["templates"]
			.as_array()
			.expect("a list of templates");
		let ids: Vec<&str> = templates
			.iter()
			.filter_map(|entry| entry["id"].as_str())
			.collect();
		for id in ["print", "monochrome", "mondrian"] {
			assert!(ids.contains(&id), "{id} is missing from {ids:?}");
		}
		assert!(
			templates
				.iter()
				.all(|entry| entry["installed"].as_bool().is_some()),
			"every template says whether it is bundled: {templates:?}",
		);
	}

	/// Both mappings hold under overflow: a point resolves only where ink is,
	/// and a byte scrolled out of sight still has a row.
	#[test]
	fn both_mappings_hold_when_a_block_overflows() {
		let text = format!(
			"```\n{}\n{}\n```\n",
			"abcdefghij".repeat(20),
			"klmnopqrst".repeat(20),
		);
		let mut session = session();
		answered(
			&mut session,
			json!({ "open": { "id": "d1", "text": text,
				"settings": { "width": 200.0 } } }),
		);
		let response =
			answered(&mut session, json!({ "text": { "id": "d1" } }));
		let layer = &response["text"];

		// A point resolves to a byte only where the viewport shows ink.
		let clusters = layer["clusters"].as_array().expect("clusters");
		assert!(!clusters.is_empty(), "{response}");
		let rightmost = clusters
			.iter()
			.map(|cluster| {
				cluster["x"].as_f64().expect("x")
					+ cluster["width"].as_f64().expect("width")
			})
			.fold(f64::MIN, f64::max);
		assert!(rightmost <= 200.0, "a hit target reached {rightmost}");

		// A byte resolves to a row even where the viewport hides it.
		let rows = layer["rows"].as_array().expect("rows");
		let row_of = |byte: u64| {
			rows.iter()
				.find(|row| {
					row["source_start"].as_u64().expect("start") <= byte
						&& byte < row["source_end"].as_u64().expect("end")
				})
				.map(|row| row["y"].as_f64().expect("y"))
		};
		let first = row_of(30).expect("a byte of the first row");
		let second = row_of(350).expect("a byte of the second row");
		assert!(
			second > first,
			"the rows did not descend: {first}, {second}"
		);
	}

	/// A palette reaches the pixels, which only shows when a tile is drawn.
	///
	/// TST-3: a theme is colours, so naming one reconfigures no geometry and
	/// the document keeps the shape it had.
	#[test]
	#[ignore = "requires a GPU; run the serve tests with --ignored"]
	fn a_theme_reaches_the_rendered_pixels_without_moving_the_text() {
		let mut session = session();
		// The block map is taken from a fresh layout under the sheet in force,
		// so comparing two of them compares the geometry the sheet produced.
		let open = |session: &mut Session| {
			let response = answered(
				session,
				json!({ "open": { "id": "d1", "text": "Text on a page.\n" } }),
			);
			assert!(response.get("error").is_none(), "{response}");
			response["opened"]["blocks"].clone()
		};
		let tile = |session: &mut Session| {
			let response = answered(
				session,
				json!({ "tile": { "id": "d1", "width": 400, "height": 200 } }),
			);
			assert!(response.get("error").is_none(), "{response}");
			decode_tile(&response)
		};
		let light_map = open(&mut session);
		let light = tile(&mut session);
		let changed = answered(
			&mut session,
			json!({ "appearance": { "theme": "dark" } }),
		);
		assert_eq!(
			changed["appearance"]["reflow"], false,
			"a palette reflowed the document: {changed}"
		);
		assert_eq!(open(&mut session), light_map, "the theme moved the text");
		let dark = tile(&mut session);
		assert_ne!(light, dark, "the theme did not reach the pixels");
		// Naming the first theme again returns the sheet to what it was rather
		// than layering one palette over another.
		let back = answered(
			&mut session,
			json!({ "appearance": { "theme": "light" } }),
		);
		assert_eq!(back["appearance"]["reflow"], false, "{back}");
		assert_eq!(tile(&mut session), light, "the light theme did not return");
	}

	/// A minimal MVSS sheet with one body declaration.
	fn mvss(declaration: &str) -> String {
		format!(
			"format_version = 2\ntargets = [\"ui\"]\nversion = 1\n			 [[rule]]\nwhen = [\"body\"]\n{declaration}\n"
		)
	}

	/// A theme names a palette, so it recolours whatever sheet is in force and
	/// brings none of its own geometry with it. The bundled themes declare the
	/// same `details` padding and radius, so a full merge would look like a
	/// repaint here and reflow a client's own sheet elsewhere.
	#[test]
	#[ignore = "requires a GPU; run the serve tests with --ignored"]
	fn a_theme_recolours_a_clients_own_stylesheet_without_reflowing_it() {
		let mut session = session();
		answered(
			&mut session,
			json!({ "open": { "id": "d1",
				"text": "<details>\n<summary>More</summary>\n\nBody\n\n</details>\n" } }),
		);
		let client = mvss("color = \"#101010\"\nline_height = 1.5");
		let shaped = answered(
			&mut session,
			json!({ "appearance": { "style": client } }),
		);
		assert_eq!(shaped["appearance"]["reflow"], true, "{shaped}");
		let tile = |session: &mut Session| {
			let response = answered(
				session,
				json!({ "tile": { "id": "d1", "width": 400, "height": 200 } }),
			);
			assert!(response.get("error").is_none(), "{response}");
			decode_tile(&response)
		};
		let before = tile(&mut session);
		let themed = answered(
			&mut session,
			json!({ "appearance": { "theme": "dark" } }),
		);
		assert_eq!(themed["appearance"]["reflow"], false, "{themed}");
		assert_ne!(
			tile(&mut session),
			before,
			"the palette reached the pixels"
		);
	}

	/// The same from the session's own sheet: a theme is applied to whatever
	/// the session started with, and naming either one twice is idempotent
	/// rather than layering a palette on itself.
	#[test]
	fn a_theme_does_not_reflow_the_session_it_was_launched_with() {
		let mut session = session();
		let opened = answered(
			&mut session,
			json!({ "open": { "id": "d1", "text": "Text with [a link](a.md).\n" } }),
		);
		for theme in ["dark", "light", "dark"] {
			let answer = answered(
				&mut session,
				json!({ "appearance": { "theme": theme } }),
			);
			assert_eq!(
				answer["appearance"]["reflow"], false,
				"{theme}: {answer}"
			);
		}
		assert!(
			opened["opened"]["blocks"]
				.as_array()
				.is_some_and(|b| b.len() == 1)
		);
	}

	/// A colour is resolved when the frame is painted, so setting one is a
	/// repaint; a line height is geometry, so setting one reflows. The server
	/// says which happened rather than leaving the host to assume.
	#[test]
	fn a_colour_only_stylesheet_repaints_and_a_geometry_one_reflows() {
		let text = "A paragraph long enough to wrap somewhere.\n";
		let mut session = session();
		answered(
			&mut session,
			json!({ "open": { "id": "d1", "text": text } }),
		);
		// Replacing the bundled sheet at all is a change.
		let base = mvss("color = \"#101010\"\nline_height = 1.5");
		let first =
			answered(&mut session, json!({ "appearance": { "style": base } }));
		assert_eq!(first["appearance"]["reflow"], true, "{first}");

		let colour = mvss("color = \"#202020\"\nline_height = 1.5");
		let painted = answered(
			&mut session,
			json!({ "appearance": { "style": colour } }),
		);
		assert_eq!(painted["appearance"]["reflow"], false, "{painted}");

		let geometry = mvss("color = \"#202020\"\nline_height = 1.8");
		let reflowed = answered(
			&mut session,
			json!({ "appearance": { "style": geometry } }),
		);
		assert_eq!(reflowed["appearance"]["reflow"], true, "{reflowed}");
		assert!(
			reflowed["appearance"]["elapsed_ms"].as_f64().expect("ms") >= 0.0,
		);
	}

	#[test]
	fn an_unusable_appearance_leaves_the_last_one_in_place() {
		let mut session = session();
		let first = answered(
			&mut session,
			json!({ "appearance": { "style": mvss("line_height = 1.5") } }),
		);
		assert_eq!(first["appearance"]["reflow"], true, "{first}");
		let bad = answered(
			&mut session,
			json!({ "appearance": { "style": "this is not MVSS" } }),
		);
		assert!(bad["error"].is_string(), "{bad}");
		// The sheet that parsed is still the effective one.
		let again = answered(
			&mut session,
			json!({ "appearance": { "style": mvss("line_height = 1.5") } }),
		);
		assert_eq!(again["appearance"]["reflow"], false, "{again}");
	}

	#[test]
	fn an_automatic_theme_is_the_hosts_to_resolve() {
		let mut session = session();
		for name in ["light", "dark"] {
			let response = answered(
				&mut session,
				json!({ "appearance": { "theme": name } }),
			);
			assert!(response.get("error").is_none(), "{name}: {response}");
		}
		let response = answered(
			&mut session,
			json!({ "appearance": { "theme": "auto" } }),
		);
		assert!(response["error"].is_string(), "{response}");
	}

	/// What the engine could not draw travels with the state, so a host can
	/// report it where its own diagnostics live.
	#[test]
	fn the_state_reports_what_could_not_be_drawn() {
		let mut session = session();
		let response = answered(
			&mut session,
			json!({ "open": { "id": "d1",
				"text": "Text.\n\n$$\n\\frac{\n$$\n" } }),
		);
		let opened = &response["opened"];
		assert!(opened.get("error").is_none(), "{response}");
		for field in ["degraded", "math_errors", "deferred"] {
			assert!(
				opened[field].is_u64(),
				"{field} is not a count: {response}",
			);
		}
	}

	#[test]
	fn a_text_band_answers_only_with_clusters_inside_it() {
		let mut session = session();
		open_paragraphs(&mut session, 40);
		let all = text_layer(&mut session, json!({ "text": { "id": "d1" } }));
		let band = text_layer(
			&mut session,
			json!({ "text": { "id": "d1", "top": 100.0, "bottom": 200.0 } }),
		);
		assert!(!all.is_empty() && !band.is_empty(), "{all:?}");
		assert!(
			band.len() < all.len(),
			"the band did not narrow anything: {} of {}",
			band.len(),
			all.len(),
		);
		for cluster in &band {
			let y = cluster["y"].as_f64().expect("y");
			let height = cluster["height"].as_f64().expect("height");
			assert!(y + height >= 100.0 && y <= 200.0, "{cluster}");
		}
	}

	#[test]
	fn a_text_layer_for_an_unknown_document_is_an_error() {
		let mut session = session();
		let response =
			answered(&mut session, json!({ "text": { "id": "d9" } }));
		assert!(response["error"].is_string(), "{response}");
	}
	#[test]
	fn pdf_exports_buffer_with_template_geometry_without_touching_source() {
		let dir = tempfile::tempdir().unwrap();
		let source = dir.path().join("source.md");
		let output = dir.path().join("out.pdf");
		std::fs::write(&source, "# Disk version").unwrap();
		let mut session = session();
		let opened = serde_json::from_str::<Value>(&session.handle(&json!({"open":{"id":"doc","path":source,"text":"# Unsaved buffer\n\nNative export."}}).to_string()).header).unwrap();
		assert!(opened.get("opened").is_some());
		let refused = serde_json::from_str::<Value>(&session.handle(&json!({"export":{"id":"doc","output":dir.path().join("./source.md")}}).to_string()).header).unwrap();
		assert!(refused["error"].as_str().unwrap().contains("overwrite"));
		let failed = serde_json::from_str::<Value>(&session.handle(&json!({"export":{"id":"doc","output":output,"style":"no-such-template"}}).to_string()).header).unwrap();
		assert!(failed.get("error").is_some());
		let result = serde_json::from_str::<Value>(&session.handle(&json!({"export":{"id":"doc","output":output,"stylesheet":"format_version = 2\nversion = 1\ntargets = [\"pdf\"]\n[page]\nsize = \"a5\"\nmargin = [5, 5, 5, 5]\nheader_center = \"{path}\"\n[[rule]]\nwhen = [\"h1\"]\ncolor = \"#244C80\""}}).to_string()).header).unwrap();
		assert!(result.get("exported").is_some(), "{result}");
		let bytes = std::fs::read(&output).unwrap();
		assert!(bytes.starts_with(b"%PDF"));
		let pdf = String::from_utf8_lossy(&bytes);
		let media = pdf
			.split("/MediaBox[")
			.nth(1)
			.unwrap()
			.split(']')
			.next()
			.unwrap();
		let points: Vec<f32> = media
			.split_whitespace()
			.map(|s| s.parse().unwrap())
			.collect();
		assert!((points[2] - 148.0 * 72.0 / 25.4).abs() < 0.01);
		assert!((points[3] - 210.0 * 72.0 / 25.4).abs() < 0.01);
		assert_eq!(std::fs::read_to_string(&source).unwrap(), "# Disk version");
		assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
		assert!(session.renderer.is_none(), "PDF needs no GPU");
	}
	#[test]
	fn find_text_matches_visible_clusters_without_transmitting_geometry() {
		let mut session = session();
		answered(
			&mut session,
			json!({"open": {"id": "doc", "text": "# Repeated\n\nCafé &amp; 中文 **office** 😀.\n\nCafé &amp; 中文 **office** 😀."}}),
		);
		let full = answered(&mut session, json!({"text": {"id": "doc"}}));
		let compact =
			answered(&mut session, json!({"rendered": {"id": "doc"}}));
		let clusters = full["text"]["clusters"].as_array().unwrap();
		let expected: String = clusters
			.iter()
			.map(|c| c["text"].as_str().unwrap())
			.collect();
		assert_eq!(compact["rendered"]["text"], expected);
		let ranges = compact["rendered"]["ranges"].as_array().unwrap();
		for (range, cluster) in ranges.iter().zip(clusters) {
			assert_eq!(range[0], cluster["source_start"]);
			assert_eq!(range[1], cluster["source_end"]);
			assert_eq!(
				range[2],
				cluster["text"].as_str().unwrap().encode_utf16().count()
			);
		}
		assert_eq!(ranges.len(), clusters.len());
		assert!(compact.to_string().len() * 4 < full.to_string().len());
		assert!(session.renderer.is_none());
	}
	#[test]
	fn appearance_refreshes_diagram_colors_and_reports_changed_geometry() {
		let mut session = session();
		answered(
			&mut session,
			json!({"open": {"id": "doc", "text": "```mermaid\ngraph TD\n A[Start] --> B[End]\n```"}}),
		);
		let pixels = |session: &Session| {
			let open = &session.documents["doc"];
			let decoded = open.layout.images.pixels.decoded.lock().unwrap();
			decoded.values().next().unwrap().rgba.to_vec()
		};
		let before = pixels(&session);
		let geometry = session.documents["doc"].layout.height;
		let dark =
			answered(&mut session, json!({"appearance": {"theme": "dark"}}));
		assert_eq!(dark["appearance"]["reflow"], false, "{dark}");
		assert_ne!(
			before,
			pixels(&session),
			"the open diagram retained its old palette"
		);
		assert_eq!(geometry, session.documents["doc"].layout.height);
		let sheet = |size| {
			format!(
				"format_version=2\nversion=1\ntargets=['ui']\n[mermaid]\ntheme='dark'\nfont_size={size}\n"
			)
		};
		answered(&mut session, json!({"appearance": {"style": sheet(16)}}));
		let small = session.documents["doc"].layout.height;
		let grown =
			answered(&mut session, json!({"appearance": {"style": sheet(32)}}));
		assert_eq!(grown["appearance"]["reflow"], true, "{grown}");
		assert!(session.documents["doc"].layout.height > small);
		assert!(
			!session.pump().is_empty(),
			"new geometry must reach the host"
		);
		assert!(
			session.renderer.is_none(),
			"diagram preparation is CPU-only"
		);
	}
	#[test]
	fn preview_templates_are_per_document_and_invalid_changes_are_atomic() {
		let mut session = session();
		let text = "# Heading\n\nBody text.\n\nAnother paragraph.";
		for (id, extra) in [
			("plain", json!({})),
			("named", json!({"template": "mondrian"})),
			("custom", json!({"stylesheet": mvss("line_height = 3.0")})),
		] {
			let mut open = json!({"id": id, "text": text});
			open.as_object_mut()
				.unwrap()
				.extend(extra.as_object().unwrap().clone());
			let response = answered(&mut session, json!({"open": open}));
			assert!(response.get("opened").is_some(), "{response}");
		}
		assert!(!session.documents["plain"].templated);
		assert!(session.documents["named"].templated);
		let before = session.documents["custom"].layout.height;
		assert_ne!(before, session.documents["plain"].layout.height);
		let bad = answered(
			&mut session,
			json!({"open": {
				"id": "custom", "text": text, "stylesheet": "not valid MVSS"
			}}),
		);
		assert!(bad.get("error").is_some(), "{bad}");
		assert_eq!(session.documents["custom"].layout.height, before);
		answered(&mut session, json!({"appearance": {"theme": "dark"}}));
		assert_eq!(session.documents["custom"].layout.height, before);
		answered(
			&mut session,
			json!({"open": {"id": "custom", "text": text}}),
		);
		assert!(!session.documents["custom"].templated);
	}

	#[test]
	#[ignore = "requires a GPU; run the serve tests with --ignored"]
	fn preview_template_pixels_survive_other_documents_and_host_themes() {
		let mut session = session();
		let text = "# Heading\n\nA paragraph with **bold** words.";
		for (id, template) in [("named", Some("mondrian")), ("plain", None)] {
			let mut open = json!({"id": id, "text": text});
			if let Some(template) = template {
				open["template"] = json!(template);
			}
			let response = answered(&mut session, json!({"open": open}));
			assert!(response.get("opened").is_some(), "{response}");
		}
		let tile = |session: &mut Session, id: &str| {
			let response = answered(
				session,
				json!({"tile": {
					"id": id, "width": 800, "height": 400
				}}),
			);
			assert!(response.get("tile").is_some(), "{response}");
			decode_tile(&response)
		};
		let named = tile(&mut session, "named");
		let plain = tile(&mut session, "plain");
		assert_ne!(named, plain);
		assert_eq!(named, tile(&mut session, "named"));
		answered(&mut session, json!({"appearance": {"theme": "dark"}}));
		assert_eq!(named, tile(&mut session, "named"));
		assert_ne!(plain, tile(&mut session, "plain"));
		assert_eq!(named, tile(&mut session, "named"));
	}
	#[test]
	#[ignore = "requires a GPU; run the serve tests with --ignored"]
	fn tile_background_matches_the_native_clear_pixels() {
		let mut session = session();
		answered(
			&mut session,
			json!({"open": {
				"id": "custom", "text": "Text", "stylesheet": mvss("background = \"#123456\"")
			}}),
		);
		for theme in ["light", "dark"] {
			answered(&mut session, json!({"appearance": {"theme": theme}}));
			let response = answered(
				&mut session,
				json!({"tile": {
					"id": "custom", "width": 800, "height": 40
				}}),
			);
			assert_eq!(response["tile"]["background"], json!([18, 52, 86]));
			let image = image::load_from_memory(&decode_tile(&response))
				.unwrap()
				.to_rgb8();
			assert_eq!(image.get_pixel(0, 0).0, [18, 52, 86]);
		}
	}
}
