//! Shared configuration for reader, file and buffer exports.
use super::{PageOverrides, PdfRequest};
use crate::{layout::LayoutOptions, settings::ExportSettings};
use anyhow::{Context, Result, bail};
use markview_layout::{
	fonts::FontConfig,
	paginate::PageGeometry,
	style::{CjkType, StyleTarget, Stylesheet},
};
use std::{path::PathBuf, sync::Arc};

pub(crate) const MIN_FONT_SIZE_PX: f32 = 1.0;
pub(crate) const MAX_FONT_SIZE_PX: f32 = 200.0;

pub(crate) struct Config {
	pub(crate) styles: Vec<String>,
	pub(crate) style_file: Option<PathBuf>,
	pub(crate) cjk: CjkType,
	pub(crate) font_overrides: Vec<crate::settings::FontDefOverride>,
	pub(crate) font_files: Vec<PathBuf>,
	pub(crate) options: LayoutOptions,
	pub(crate) page: PageOverrides,
	pub(crate) scale: f32,
}

impl Default for Config {
	fn default() -> Self {
		Self {
			styles: Vec::new(),
			style_file: None,
			cjk: CjkType::Sc,
			font_overrides: Vec::new(),
			font_files: Vec::new(),
			options: LayoutOptions {
				font_size: ExportSettings::DEFAULT_FONT_SIZE_PX,
				..Default::default()
			},
			page: PageOverrides::default(),
			scale: 2.0,
		}
	}
}

pub(crate) struct Prepared {
	pub(crate) options: LayoutOptions,
	pub(crate) geometry: PageGeometry,
	pub(crate) scale: f32,
}

impl Config {
	pub(crate) fn from_settings(
		settings: &ExportSettings,
		fonts: FontConfig,
		cjk: CjkType,
		overrides: &[crate::settings::FontDefOverride],
	) -> Self {
		Self {
			styles: settings.style.clone(),
			scale: settings.scale,
			cjk,
			font_overrides: overrides.to_vec(),
			options: LayoutOptions {
				font_size: settings.font_size,
				paragraph_indent: settings.paragraph_indent,
				fonts,
				..Default::default()
			},
			page: PageOverrides {
				paper: Some(settings.paper.clone()),
				landscape: Some(settings.landscape),
				margin: Some(settings.margin),
				..Default::default()
			},
			..Default::default()
		}
	}

	/// Parses the export with the dialect the reader window reads with, so the
	/// file shows the same document the export was started from. Without this,
	/// every export falls back to the default dialect.
	pub(crate) fn with_dialect(
		mut self,
		dialect: crate::settings::EffectiveDialect,
	) -> Self {
		self.options.hard_breaks = dialect.hard_breaks;
		self.options.highlight_marks = dialect.highlight_marks;
		self
	}

	pub(crate) fn prepare(self) -> Result<Prepared> {
		validate_font_size(self.options.font_size)?;
		validate_indent(self.options.paragraph_indent)?;
		validate_scale(self.scale)?;
		let sheet = crate::stylesheet::load_for_pdf(
			(!self.styles.is_empty()).then_some(self.styles.as_slice()),
			crate::stylesheet::directory().as_deref(),
			self.cjk,
			self.style_file.as_deref(),
		)?;
		let sheet = crate::stylesheet::apply_font_overrides(
			sheet,
			&self.font_overrides,
		)?;
		let stylesheet = styled(sheet, &self.page)?;
		let geometry = PageGeometry::from_style(stylesheet.page())?;
		let fonts =
			crate::fonts::export_config(self.options.fonts, &self.font_files)?;
		Ok(Prepared {
			options: LayoutOptions {
				width: geometry.text_px().0,
				stylesheet,
				fonts,
				codeblock_wrap: true,
				force_open: true,
				hide_front_matter: true,
				..self.options
			},
			geometry,
			scale: self.scale,
		})
	}
}

impl Prepared {
	pub(crate) fn pdf_request(
		self,
		path: PathBuf,
		output: PathBuf,
		offline: bool,
	) -> PdfRequest {
		PdfRequest {
			security: crate::security::Security::local(
				crate::security::Trust::Trusted,
			),
			path,
			output,
			options: self.options,
			geometry: self.geometry,
			metadata: Default::default(),
			links: true,
			offline,
		}
	}
}

pub(crate) fn validate_font_size(value: f32) -> Result<()> {
	validate_range("Font size", value, MIN_FONT_SIZE_PX, MAX_FONT_SIZE_PX)
}

pub(crate) fn validate_indent(value: f32) -> Result<()> {
	validate_range("Paragraph indent", value, 0.0, 4.0)
}

pub(crate) fn validate_scale(value: f32) -> Result<()> {
	validate_range("PNG scale", value, 0.1, 8.0)
}

fn validate_range(name: &str, value: f32, min: f32, max: f32) -> Result<()> {
	if !value.is_finite() || !(min..=max).contains(&value) {
		bail!("{name} must be between {min} and {max}");
	}
	Ok(())
}

/// Page overrides and media resolution apply equally to PDF and PNG.
pub(crate) fn styled(
	sheet: Arc<Stylesheet>,
	page: &PageOverrides,
) -> Result<Arc<Stylesheet>> {
	page.validate()?;
	let mut sheet = (*sheet).clone();
	if let Some(paper) = &page.paper {
		sheet.page.size = Some(paper.clone());
	}
	if let Some(landscape) = page.landscape {
		sheet.page.landscape = Some(landscape);
	}
	if let Some(margin) = page.margin {
		sheet.page.margin = Some(margin.to_vec());
	}
	for (slots, header) in [(&page.header, true), (&page.footer, false)] {
		for (slot, value) in slots.iter().enumerate() {
			if let Some(value) = value {
				let target = match (header, slot) {
					(true, 0) => &mut sheet.page.header_left,
					(true, 1) => &mut sheet.page.header_center,
					(true, _) => &mut sheet.page.header_right,
					(false, 0) => &mut sheet.page.footer_left,
					(false, 1) => &mut sheet.page.footer_center,
					(false, _) => &mut sheet.page.footer_right,
				};
				*target = Some(value.clone());
			}
		}
	}
	let (width, height) = sheet
		.page
		.paper_mm()
		.context("Invalid template paper size")?;
	sheet.set_media(
		crate::stylesheet::media_context(StyleTarget::Pdf)
			.with_size(width, height),
	);
	Ok(Arc::new(sheet))
}

impl PageOverrides {
	pub(crate) fn validate(&self) -> Result<()> {
		if let Some(paper) = &self.paper
			&& markview_layout::style::parse_paper_size(paper).is_none()
		{
			bail!(
				"Invalid paper {paper:?}; use a3, a4, a5, a6, b5, letter, legal, tabloid or WIDTHxHEIGHT in millimetres"
			);
		}
		if let Some(margin) = self.margin {
			validate_margin(&margin)?;
		}
		for value in self.header.iter().chain(&self.footer).flatten() {
			if !markview_layout::paginate::template_is_valid(value) {
				bail!(
					"Unknown page placeholder; use {{page}}, {{pages}}, {{title}} or {{path}}"
				);
			}
		}
		Ok(())
	}
}

pub(crate) fn validate_margin(margin: &[f32; 4]) -> Result<()> {
	if margin
		.iter()
		.any(|value| !value.is_finite() || *value < 0.0)
	{
		bail!("Invalid margin");
	}
	Ok(())
}
