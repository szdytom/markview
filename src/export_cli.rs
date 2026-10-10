//! In-memory document export with JSON progress on stdout.
use crate::{export, layout::LayoutOptions};
use anyhow::{Context, Result, bail};
use clap::Args;
use markview_core::{
	fonts::FontConfig,
	paginate::{PT_PER_PX, PageGeometry},
	style::{CjkType, StyleTarget, Stylesheet},
};
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
	#[arg(long, default_value = "print")]
	style: String,
	#[arg(long)]
	style_file: Option<PathBuf>,
	#[arg(long)]
	fonts: Vec<PathBuf>,
	#[arg(long)]
	font_file: Vec<PathBuf>,
	#[arg(long)]
	ignore_system_fonts: bool,
	#[arg(long, default_value_t = 16.0)]
	font_size: f32,
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
	fn sheet(&self) -> Result<Arc<Stylesheet>> {
		let target = StyleTarget::Pdf;
		let mut source = (*Stylesheet::bundled_print().source()).clone();
		if let Some(path) = &self.style_file {
			let higher = crate::stylesheet::validate(path)?;
			if !higher.targets.contains(&target) {
				bail!(
					"{}: targets do not include {}",
					path.display(),
					target.as_str()
				);
			}
			source.merge(&higher);
		} else {
			let higher = markview_core::style::StylesheetSource::named_rules(
				&self.style,
			)
			.with_context(|| {
				format!("Unknown bundled template {:?}", self.style)
			})?;
			if !higher.targets.contains(&target) {
				bail!(
					"{}: targets do not include {}",
					self.style,
					target.as_str()
				);
			}
			source.merge(&higher);
		}
		let mut sheet =
			Arc::new(source).resolve(crate::stylesheet::media_context(target));
		sheet.set_cjk_type(CjkType::Sc);
		if target == StyleTarget::Pdf {
			let (width, height) = sheet
				.page
				.paper_mm()
				.context("Invalid template paper size")?;
			sheet.set_media(
				crate::stylesheet::media_context(target)
					.with_size(width, height),
			);
		}
		Ok(Arc::new(sheet))
	}
	fn cancelled(&self) -> Result<()> {
		if self.cancel_file.as_ref().is_some_and(|path| path.exists()) {
			bail!("Cancelled");
		}
		Ok(())
	}
	pub(crate) fn run(&self, offline: bool) -> Result<()> {
		self.cancelled()?;
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
		let sheet = self.sheet()?;
		if !self.stdin {
			bail!("export requires --stdin");
		}
		let output =
			self.output.as_ref().context("export requires --output")?;
		if !self.font_size.is_finite()
			|| !(1.0..=200.0).contains(&self.font_size)
		{
			bail!("--font-size must be between 1 and 200");
		}
		if !self.scale.is_finite() || !(0.1..=8.0).contains(&self.scale) {
			bail!("--scale must be between 0.1 and 8");
		}
		for directory in &self.fonts {
			if !directory.is_dir() {
				bail!("Font directory does not exist: {}", directory.display());
			}
		}
		let mut text = String::new();
		std::io::stdin()
			.take(crate::file::MAX_FILE_BYTES + 1)
			.read_to_string(&mut text)?;
		if text.len() as u64 > crate::file::MAX_FILE_BYTES {
			bail!("Markdown exceeds 32 MiB");
		}
		let base = self.base_dir.clone().unwrap_or(std::env::current_dir()?);
		let resource_path = base.join("Untitled.md");
		let parent = output
			.parent()
			.filter(|path| !path.as_os_str().is_empty())
			.unwrap_or(std::path::Path::new("."));
		std::fs::create_dir_all(parent)?;
		let staged = output.clone();
		let geometry = PageGeometry::from_style(sheet.page())?;
		let mut fonts = if self.font_file.is_empty() {
			FontConfig::default()
		} else {
			let faces = self
				.font_file
				.iter()
				.map(|path| {
					std::fs::read(path)
						.map(|data| parley::fontique::Blob::new(Arc::new(data)))
				})
				.collect::<std::io::Result<Vec<_>>>()?;
			FontConfig::from_faces(0, faces)
		};
		fonts.directories = self.fonts.clone();
		if self.font_file.is_empty() {
			fonts.ignore_system_fonts = self.ignore_system_fonts;
		}
		let options = LayoutOptions {
			stylesheet: sheet.clone(),
			fonts,
			width: geometry.text_px().0,
			font_size: self.font_size,
			codeblock_wrap: true,
			force_open: true,
			hide_front_matter: true,
			..Default::default()
		};
		let services = Arc::new(crate::services::Services::new(4));
		progress("layout", 0.05);
		self.cancelled()?;
		let done = if self.format == "pdf" {
			let request = export::PdfRequest {
				security,
				path: resource_path,
				output: staged.clone(),
				options,
				page: Default::default(),
				metadata: Default::default(),
				links: true,
				offline,
			};
			// The host stages this output and performs the final replacement.
			let stats =
				crate::pdf::export_text(&request, &text, services, || {
					self.cancelled()
				})?;
			self.cancelled()?;
			json!({"type":"done","fraction":1.0,"pages":stats.pages,"bytes":stats.bytes})
		} else {
			let snapshot = export::png_snapshot_text(
				&resource_path,
				&text,
				options,
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
				self.scale,
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
					self.scale,
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
			export::write_png(&staged, &rgba, plan.width_px, plan.height_px)?;
			json!({"type":"done","fraction":1.0,"width":plan.width_px,"height":plan.height_px})
		};
		self.cancelled()?;
		report(done);
		Ok(())
	}
}
