//! In-memory document export with JSON progress on stdout.
use crate::export;
use anyhow::{Context, Result, bail};
use clap::Args;
use markview_layout::paginate::PT_PER_PX;
use serde_json::json;
use std::{io::Read, path::PathBuf, sync::Arc};

#[derive(Args)]
pub(crate) struct Command {
	/// Resource trust supplied by the host, never inferred from `--base-dir`.
	#[arg(long, value_enum, default_value = "untrusted")]
	document_trust: crate::security::Trust,
	/// Authorize this local image file for an untrusted export; repeats.
	#[arg(long, value_name = "FILE")]
	allow_local_image: Vec<PathBuf>,
	/// Authorize a network origin and address class; repeats.
	#[arg(long, value_name = "CLASS=ORIGIN", value_parser = crate::cli::network_grant)]
	allow_network: Vec<crate::security::Resource>,
	#[arg(long, value_parser = ["pdf", "png"], default_value = "pdf")]
	format: String,
	#[arg(short, long)]
	output: Option<PathBuf>,
	#[arg(long)]
	stdin: bool,
	/// The original resource directory, independent of the source text.
	#[arg(long)]
	base_dir: Option<PathBuf>,
	#[command(flatten)]
	pub(crate) reading: crate::cli::ExportReading,
	#[arg(long, default_value_t = 2.0)]
	scale: f32,
	/// A host-created marker requests cooperative cancellation.
	#[arg(long)]
	cancel_file: Option<PathBuf>,
}
fn report(value: serde_json::Value) {
	crate::logging::report(format_args!("{value}\n"));
}
fn progress(phase: &str, fraction: f64) {
	report(json!({"type":"progress", "phase":phase, "fraction":fraction}));
}
impl Command {
	fn cancelled(&self) -> Result<()> {
		if self.cancel_file.as_ref().is_some_and(|path| path.exists()) {
			bail!("Cancelled");
		}
		Ok(())
	}
	pub(crate) fn run(
		&self,
		mut config: export::Config,
		offline: bool,
	) -> Result<()> {
		self.cancelled()?;
		let security = self.resource_security()?;

		if !self.stdin {
			bail!("export requires --stdin");
		}
		let output =
			self.output.as_ref().context("export requires --output")?;
		config.scale = self.scale;
		let prepared = config.prepare()?;
		let mut text = String::new();
		std::io::stdin()
			.take(crate::file::MAX_FILE_BYTES + 1)
			.read_to_string(&mut text)?;
		if text.len() as u64 > crate::file::MAX_FILE_BYTES {
			bail!("Markdown exceeds 32 MiB");
		}
		let base = self.base_dir.clone().unwrap_or(std::env::current_dir()?);
		let resource_path = base.join("Untitled.md");
		let services = Arc::new(crate::services::Services::new(4));
		progress("layout", 0.05);
		self.cancelled()?;
		let done = if self.format == "pdf" {
			let mut request =
				prepared.pdf_request(resource_path, output.clone(), offline);
			request.security = security;
			// The host stages this output and performs the final replacement.
			let stats =
				crate::pdf::export_text(&request, &text, services, || {
					self.cancelled()
				})?;
			self.cancelled()?;
			json!({"type":"done","fraction":1.0,"pages":stats.pages,"bytes":stats.bytes})
		} else {
			self.export_png(
				prepared,
				(&resource_path, &text),
				output,
				services,
				security,
				offline,
			)?
		};
		self.cancelled()?;
		report(done);
		Ok(())
	}
	fn resource_security(&self) -> Result<crate::security::Security> {
		let mut security =
			crate::security::Security::local(self.document_trust);
		for path in &self.allow_local_image {
			security.grant(crate::security::Resource::File(
				std::fs::canonicalize(path).with_context(|| {
					format!(
						"Cannot resolve authorized image {}",
						path.display()
					)
				})?,
			));
		}
		for resource in &self.allow_network {
			security.grant(resource.clone());
		}
		Ok(security)
	}

	fn export_png(
		&self,
		prepared: export::config::Prepared,
		input: (&std::path::Path, &str),
		output: &std::path::Path,
		services: Arc<crate::services::Services>,
		security: crate::security::Security,
		offline: bool,
	) -> Result<serde_json::Value> {
		let (resource_path, text) = input;
		let scale = prepared.scale;
		let sheet = prepared.options.stylesheet.clone();
		let geometry = prepared.geometry;
		let snapshot = export::png_snapshot_text(
			resource_path,
			text,
			prepared.options,
			offline,
			services,
			security,
		)?;
		self.cancelled()?;
		let mut renderer =
			pollster::block_on(crate::render::Renderer::new(None))?;
		renderer.set_stylesheet(sheet.clone());
		let plan = export::plan(
			&geometry,
			snapshot.height,
			scale,
			renderer.max_texture_dimension_2d(),
		)?;
		let mut rgba =
			vec![0; plan.width_px as usize * plan.height_px as usize * 4];
		for (index, tile) in plan.tiles.iter().enumerate() {
			self.cancelled()?;
			export::draw_tile(
				&mut renderer,
				&snapshot,
				&plan,
				&sheet,
				*tile,
				scale,
				geometry.margin_pt[3] / PT_PER_PX,
				crate::render::Theme::Light,
				&mut rgba,
			)?;
			progress(
				"render",
				0.2 + 0.75 * (index + 1) as f64 / plan.tiles.len() as f64,
			);
		}
		self.cancelled()?;
		export::write_png(output, &rgba, plan.width_px, plan.height_px)?;
		Ok(
			json!({"type":"done","fraction":1.0,"width":plan.width_px,"height":plan.height_px}),
		)
	}
}
