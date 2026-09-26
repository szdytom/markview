//! Export-only JSON-lines service for an editor-owned Markdown buffer.
//! One response is written per request; EOF ends the private engine process.
use crate::{
	export,
	render::{Renderer, Theme},
	settings::{ExportFormat, ExportSettings},
};
use anyhow::{Context, Result, bail};
use markview_core::{
	fonts::FontConfig,
	style::{CjkType, StyleTarget, Stylesheet},
};
use serde_json::{Value, json};
use std::{
	collections::HashMap,
	io::{BufRead, Write},
	path::PathBuf,
	sync::Arc,
};

struct Document {
	text: String,
	path: PathBuf,
}

#[derive(Default)]
struct Session {
	fonts: FontConfig,
	documents: HashMap<String, Document>,
	renderer: Option<Renderer>,
	offline: bool,
}

pub(crate) fn run(offline: bool) -> Result<()> {
	let mut session = Session {
		offline,
		..Default::default()
	};
	let (send, requests) = std::sync::mpsc::channel();
	std::thread::spawn(move || {
		for line in std::io::stdin().lock().lines() {
			if send.send(line).is_err() {
				return;
			}
		}
		// EOF means the owner has gone away, even if a resource read is blocked.
		let _writing = export::OUTPUT_WRITE.lock().unwrap();
		std::process::exit(0);
	});
	let mut output = std::io::stdout().lock();
	for line in requests {
		let answer = session.handle(&line?);
		serde_json::to_writer(&mut output, &answer)?;
		writeln!(output)?;
		output.flush()?;
	}
	Ok(())
}

impl Session {
	fn handle(&mut self, line: &str) -> Value {
		self.request(line)
			.unwrap_or_else(|error| json!({"error": format!("{error:#}")}))
	}

	fn request(&mut self, line: &str) -> Result<Value> {
		let request: Value = serde_json::from_str(line)?;
		let fields =
			request.as_object().context("Expected one request object")?;
		if fields.len() != 1 {
			bail!("Expected exactly one method");
		}
		let (method, params) = fields.iter().next().unwrap();
		match method.as_str() {
			"open" => {
				let id = string(params, "id")?;
				let text = string(params, "text")?.to_owned();
				let path = PathBuf::from(string(params, "path")?);
				self.documents
					.insert(id.to_owned(), Document { text, path });
				Ok(json!({"opened": {"id": id}}))
			}
			"close" => {
				let id = string(params, "id")?;
				self.documents.remove(id);
				Ok(json!({"closed": {"id": id}}))
			}
			"styles" => {
				let directory = crate::stylesheet::directory();
				let entries: Vec<_> = crate::stylesheet::catalog_for(directory.as_deref(), None, StyleTarget::Pdf)
                    .into_iter().map(|entry| json!({"id": entry.id, "name": entry.name,
                        "installed": !Stylesheet::PDF_THEMES.contains(&entry.id.as_str()), "error": entry.error})).collect();
				Ok(json!({"styles": {"templates": entries}}))
			}
			"export" => self.export(params),
			_ => bail!("Unknown method {method}"),
		}
	}

	fn export(&mut self, params: &Value) -> Result<Value> {
		let started = std::time::Instant::now();
		let id = string(params, "id")?;
		let output = PathBuf::from(string(params, "output")?);
		let format = params.get("format").map_or(Ok("pdf"), |v| {
			v.as_str().context("format must be a string")
		})?;
		let format_kind = match format {
			"pdf" => ExportFormat::Pdf,
			"png" => ExportFormat::Png,
			_ => bail!("Unknown export format {format}"),
		};
		let style = params
			.get("style")
			.map(|v| v.as_str().context("style must be a name"))
			.transpose()?;
		let settings = ExportSettings {
			format: format_kind,
			style: style.map(|s| vec![s.to_owned()]).unwrap_or_default(),
			..Default::default()
		};
		let mut sheet =
			(*export::export_stylesheet(&settings.style, CjkType::Sc, &[])?)
				.clone();
		if let Some(rules) = params.get("stylesheet") {
			sheet.merge(&Stylesheet::parse(
				rules.as_str().context("stylesheet must be MVSS text")?,
			)?);
		}
		let stylesheet = Arc::new(sheet);
		let document = self
			.documents
			.get(id)
			.with_context(|| format!("No document {id}"))?;
		if crate::cli::same_target(&output, &document.path) {
			bail!("The export cannot overwrite its source");
		}
		let mut fonts = self.fonts.clone();
		crate::fonts::join_download_directory(
			&mut fonts,
			crate::fonts::directory(),
		);
		let (width, height) = if format_kind == ExportFormat::Pdf {
			let mut job = export::pdf_request(
				document.path.clone(),
				output.clone(),
				&settings,
				fonts,
				CjkType::Sc,
				&[],
				self.offline,
			)?;
			job.options.stylesheet = stylesheet;
			job.page = export::PageOverrides::default();
			crate::pdf::export_buffer(&job, &document.text)?;
			(None, None)
		} else {
			let geometry = markview_core::paginate::PageGeometry::from_style(
				stylesheet.page(),
			)?;
			let options = export::layout_options(
				&settings,
				geometry.text_px().0,
				stylesheet.clone(),
				fonts,
			);
			let mut image = export::png_buffer(
				&document.path,
				Arc::from(document.text.as_str()),
				options,
				self.offline,
			)?;
			if self.renderer.is_none() {
				self.renderer = Some(pollster::block_on(Renderer::new(None))?);
			}
			let renderer = self.renderer.as_mut().unwrap();
			let plan = export::plan(
				&geometry,
				image.snapshot.height,
				settings.scale,
				renderer.max_texture_dimension_2d(),
			)?;
			let mut rgba =
				vec![0; plan.width_px as usize * plan.height_px as usize * 4];
			let left =
				geometry.margin_pt[3] / markview_core::paginate::PT_PER_PX;
			for tile in &plan.tiles {
				loop {
					export::draw_tile(
						renderer,
						&image.snapshot,
						&plan,
						&stylesheet,
						*tile,
						settings.scale,
						left,
						Theme::Light,
						&mut rgba,
					)?;
					// The next tile replaces this frame's image demand.
					if !image.settle() {
						break;
					}
				}
			}
			export::write_png(&output, &rgba, plan.width_px, plan.height_px)?;
			(Some(plan.width_px), Some(plan.height_px))
		};
		Ok(
			json!({"exported": {"id": id, "output": output, "format": format, "width": width, "height": height,
            "bytes": std::fs::metadata(&output)?.len(), "elapsed_ms": started.elapsed().as_secs_f64()*1000.0}}),
		)
	}
}

fn string<'a>(params: &'a Value, name: &str) -> Result<&'a str> {
	params
		.get(name)
		.and_then(Value::as_str)
		.with_context(|| format!("Missing string {name}"))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn protocol_keeps_buffers_and_recovers_from_errors() {
		let mut session = Session::default();
		assert!(session.handle("not json").get("error").is_some());
		assert!(
			session
				.handle(r#"{"open":{},"close":{}}"#)
				.get("error")
				.is_some()
		);
		let open = json!({"open": {"id":"doc", "text":"# 中文 buffer", "path":"/tmp/doc.md"}});
		assert_eq!(session.handle(&open.to_string())["opened"]["id"], "doc");
		assert_eq!(session.documents["doc"].text, "# 中文 buffer");
		assert!(
			session
				.handle(
					r#"{"export":{"id":"doc","output":"/tmp/out","format":"bad"}}"#
				)
				.get("error")
				.is_some()
		);
		assert!(
			session
				.handle(r#"{"close":{"id":"doc"}}"#)
				.get("closed")
				.is_some()
		);
		assert!(session.documents.is_empty());
	}

	#[test]
	fn pdf_exports_buffer_with_template_geometry_without_touching_source() {
		let dir = tempfile::tempdir().unwrap();
		let source = dir.path().join("source.md");
		let output = dir.path().join("out.pdf");
		std::fs::write(&source, "# Disk version").unwrap();
		let mut session = Session {
			fonts: crate::test_support::fonts(),
			offline: true,
			..Default::default()
		};
		let opened = session.handle(&json!({"open":{"id":"doc","path":source,"text":"# Unsaved buffer\n\nNative export."}}).to_string());
		assert!(opened.get("opened").is_some());
		let refused = session.handle(&json!({"export":{"id":"doc","output":dir.path().join("./source.md")}}).to_string());
		assert!(refused["error"].as_str().unwrap().contains("overwrite"));
		let failed = session.handle(&json!({"export":{"id":"doc","output":output,"style":"no-such-template"}}).to_string());
		assert!(failed.get("error").is_some());
		let result = session.handle(&json!({"export":{"id":"doc","output":output,"stylesheet":"format_version = 2\nversion = 1\ntargets = [\"pdf\"]\n[page]\nsize = \"a5\"\nmargin = [5, 5, 5, 5]\nheader_center = \"{path}\"\n[[rule]]\nwhen = [\"h1\"]\ncolor = \"#244C80\""}}).to_string());
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
}
