//! Headless PDF export from the command line.
//!
//! The export re-lays the document at the paper's text measure, breaks it into
//! pages, and writes vector content through `markview-pdf`. Nothing here
//! touches a window, a GPU, or the user's settings.
//!
//! A watch session keeps one [`Exporter`] alive, so every rebuild after the
//! first reuses the previous parse, the layout engine's block cache and the
//! decoded images, exactly as the reader's worker does.
use crate::{
	document,
	export::{MetadataOverrides, PdfRequest},
	file::read_markdown,
	images::Images,
	layout::{LayoutEngine, LayoutOptions},
	paginate::{PageGeometry, paginate},
	watch::FileWatch,
};
use anyhow::{Context, Result};
use log::{info, warn};
use markview_pdf::{Export, Renderer};
use std::{
	path::{Path, PathBuf},
	sync::{Arc, mpsc},
	time::{Duration, Instant},
};

/// How long the watch loop waits before it checks local image stamps again.
const IMAGE_POLL: Duration = Duration::from_millis(500);

pub(crate) fn run(args: &PdfRequest, watching: bool) -> Result<()> {
	let path = &args.path;
	if !watching {
		export_once(args)?;
		return Ok(());
	}
	let mut exporter = Exporter::new(args)?;
	// Register the watcher before the first build reads the source: a save that
	// lands while that build lays out or waits for images must schedule the
	// next one, and the watcher's own baseline stamp would absorb it.
	let (tx, rx) = mpsc::channel();
	let _watch = FileWatch::new(path.to_owned(), move || {
		let _ = tx.send(());
	});
	// The first build has nothing to reuse, and its failure is the command's
	// failure; everything after an edit keeps the last good PDF instead.
	exporter.export(false)?;
	info!(
		"Watching {} for changes; press Ctrl+C to stop",
		path.display()
	);
	watch(&mut exporter, rx)
}

/// Exports one immutable job without a window or GPU.
pub(crate) fn export_once(args: &PdfRequest) -> Result<ExportStats> {
	Exporter::new(args)?
		.export(false)?
		.context("the export produced no output")
}

pub(crate) fn export_with_services(
	args: &PdfRequest,
	services: Arc<crate::services::Services>,
) -> Result<ExportStats> {
	if let Some(loaded) = &args.loaded {
		let exporter = Exporter::with_services(args, services)?;
		return exporter.export_loaded(loaded.with_dialect(
			markview_commonmark::Dialect {
				hard_breaks: args.options.hard_breaks,
				highlight_marks: args.options.highlight_marks,
				ofm: args.options.ofm,
				grid_tables: args.options.grid_tables,
				math_dollars_block: args.options.math_dollars_block,
			},
		));
	}
	let text = read_markdown(&args.path)?;
	Exporter::with_services(args, services)?
		.export_source(&text, false, true, || Ok(()))?
		.context("the export produced no output")
}

/// Exports a host's text using the original path only for resources.
pub(crate) fn export_text(
	args: &PdfRequest,
	text: &str,
	services: Arc<crate::services::Services>,
	before_write: impl Fn() -> Result<()>,
) -> Result<ExportStats> {
	Exporter::with_services(args, services)?
		.export_source(text, true, true, before_write)?
		.context("the export produced no output")
}

/// Rebuilds the PDF whenever the document changes, and whenever a local image
/// it references changes under an unchanged document.
fn watch(exporter: &mut Exporter, rx: mpsc::Receiver<()>) -> Result<()> {
	loop {
		match rx.recv_timeout(IMAGE_POLL) {
			Ok(()) => {
				// A save burst that arrived while an export ran is one rebuild.
				while rx.try_recv().is_ok() {}
				rebuild(exporter, false);
			}
			Err(mpsc::RecvTimeoutError::Timeout) => {
				if exporter.image_changed()
					|| exporter.embed_dependencies.changed()
				{
					rebuild(exporter, true);
				}
			}
			Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
		}
	}
}

/// A failed rebuild keeps the last good PDF and the session alive.
fn rebuild(exporter: &mut Exporter, force: bool) {
	if let Err(error) = exporter.export(force) {
		warn!("Export failed; keeping the last PDF: {error:#}");
	}
}

/// What one build wrote, and how much of it came from the previous one.
#[derive(Debug)]
pub(crate) struct ExportStats {
	pub(crate) pages: usize,
	blocks: usize,
	pub(crate) bytes: usize,
	reused: usize,
	degraded: usize,
	math_errors: usize,
	elapsed: Duration,
}

/// A PDF export that outlives one build.
///
/// The retained engine, images and previous parse are the incremental
/// machinery the reader's worker uses, so a watch session pays for them once.
struct Exporter {
	security: crate::security::Security,
	path: PathBuf,
	workspace: Option<PathBuf>,
	output: PathBuf,
	options: LayoutOptions,
	geometry: PageGeometry,
	metadata: MetadataOverrides,
	links: bool,
	engine: LayoutEngine,
	images: Images,
	/// The PDF writer's own caches, which survive every rebuild: the faces it
	/// has resolved and embedded, and the stylesheet it resolved them against.
	renderer: Renderer,
	source: Option<Arc<str>>,
	/// Set until a build reaches the disk. A failed build, including a forced
	/// one, leaves it set, so the next save of the same content retries instead
	/// of being skipped as unchanged.
	dirty: bool,
	document: Option<Arc<markview_commonmark::MarkdownParse>>,
	revision: u64,
	embeds: crate::embed::Loader,
	embed_dependencies: crate::embed::Dependencies,
}

impl Exporter {
	fn new(args: &PdfRequest) -> Result<Self> {
		Self::with_services(args, Arc::new(crate::services::Services::new(4)))
	}
	fn with_services(
		args: &PdfRequest,
		services: Arc<crate::services::Services>,
	) -> Result<Self> {
		let output = args.output.clone();
		let geometry = args.geometry;
		let options = args.options.clone();
		let mut engine = LayoutEngine::with_executor(
			services.handle.cpu.clone(),
			std::sync::Arc::new(|| {}),
		);
		engine.validate_stylesheet(&options.stylesheet)?;
		let mut images = Images::shared_pdf(
			args.offline,
			args.options.fonts.clone(),
			&services,
		);
		images.set_workspace(args.workspace.clone());
		images.set_standalone_image(
			crate::file::is_picture(&args.path).then(|| args.path.clone()),
		);
		Ok(Self {
			security: args.security.clone(),
			path: args.path.clone(),
			workspace: args.workspace.clone(),
			output,
			options,
			geometry,
			metadata: args.metadata.clone(),
			links: args.links,
			engine,
			images,
			renderer: Renderer::default(),
			source: None,
			dirty: true,
			document: None,
			revision: 0,
			embeds: Default::default(),
			embed_dependencies: Default::default(),
		})
	}

	/// Rebuilds the PDF, or returns `None` when the document is unchanged and
	/// `force` did not ask for a rebuild anyway.
	fn export(&mut self, force: bool) -> Result<Option<ExportStats>> {
		if self.refresh_standalone_source() {
			self.dirty = true;
		}
		let text = read_markdown(&self.path)?;
		self.export_source(&text, false, force, || Ok(()))
	}

	/// Follows a retargeted symlink to the picture a watched export displays.
	fn refresh_standalone_source(&mut self) -> bool {
		self.images.set_standalone_image(
			crate::file::is_picture(&self.path).then(|| self.path.clone()),
		)
	}
	fn export_source(
		&mut self,
		text: &str,
		buffer: bool,
		force: bool,
		before_write: impl Fn() -> Result<()>,
	) -> Result<Option<ExportStats>> {
		anyhow::ensure!(
			!crate::file::same_file(&self.path, &self.output),
			"Cannot export over the source file: {}",
			self.path.display()
		);
		let started = Instant::now();
		let unchanged = !self.dirty && self.source.as_deref() == Some(text);
		if unchanged && !force {
			return Ok(None);
		}
		// A build that fails anywhere below leaves this set, so the next save
		// of the same content is not mistaken for one already on the disk.
		self.dirty = true;
		let changed = self.source.as_deref() != Some(text);
		if changed {
			if self.source.is_some() {
				self.security.revoke();
			}
			let source: Arc<str> = text.into();
			let dialect = markview_commonmark::Dialect {
				hard_breaks: self.options.hard_breaks,
				highlight_marks: self.options.highlight_marks,
				ofm: self.options.ofm,
				grid_tables: self.options.grid_tables,
				math_dollars_block: self.options.math_dollars_block,
			};
			let document = match &self.document {
				Some(previous) => Arc::new(markview_commonmark::reparse_with(
					previous,
					source.clone(),
					dialect,
				)),
				None => Arc::new(markview_commonmark::parse_with(
					source.clone(),
					dialect,
				)),
			};
			self.source = Some(source);
			self.document = Some(document);
		}
		self.revision += 1;
		let document = self
			.document
			.as_ref()
			.expect("a document is parsed before a layout")
			.clone();
		let security = self.security.for_content(&document.source);
		let document = self.prepare_document(
			crate::loaded::LoadedDocument::markdown(document),
			security,
			buffer,
		)?;
		let stats = self.write(&document, started, before_write)?;
		self.embed_dependencies = document
			.composition()
			.map(|composition| composition.dependencies.clone())
			.unwrap_or_default();
		Ok(Some(stats))
	}

	/// Writes a document that already has no source text, such as a web page.
	fn export_loaded(
		mut self,
		loaded: Arc<crate::loaded::LoadedDocument>,
	) -> Result<ExportStats> {
		anyhow::ensure!(
			!crate::file::same_file(&self.path, &self.output),
			"Cannot export over the source file: {}",
			self.path.display()
		);
		let started = Instant::now();
		self.revision += 1;
		let security = loaded.source().map_or_else(
			|| self.security.clone(),
			|source| self.security.for_content(source),
		);
		let document = self.prepare_document(loaded, security, false)?;
		self.write(&document, started, || Ok(()))
	}

	fn prepare_document(
		&mut self,
		loaded: Arc<crate::loaded::LoadedDocument>,
		security: crate::security::Security,
		buffer: bool,
	) -> Result<Arc<crate::loaded::LoadedDocument>> {
		let input = if buffer {
			crate::embed::InputPath::Buffer(&self.path)
		} else {
			crate::embed::InputPath::File(&self.path)
		};
		let document = self
			.embeds
			.prepare(
				loaded,
				input,
				self.workspace.as_deref(),
				&security,
				|| false,
			)
			.unwrap();
		document.ensure_export_destination(&self.output)?;
		self.images.set_embed_paths(document.embed_paths());
		self.prepare_images(&document, security);
		Ok(document)
	}

	/// Resolves the document's images and waits for every decode.
	fn prepare_images(
		&mut self,
		document: &markview_document::Document,
		security: crate::security::Security,
	) {
		self.images.set_security(security);
		self.images.prepare(
			document,
			&self.path,
			self.revision,
			false,
			&self.options.stylesheet,
			&self.options.fonts,
		);
		self.images.wait();
		for (src, entry) in &self.images.snapshot.entries {
			if let Some(error) = &entry.error {
				warn!("Image {src:?}: {error}");
			}
		}
	}

	/// Lays the document out at the export measure and writes the PDF.
	fn write(
		&mut self,
		document: &crate::loaded::LoadedDocument,
		started: Instant,
		before_write: impl Fn() -> Result<()>,
	) -> Result<ExportStats> {
		let mut snapshot = self.engine.layout_with_images(
			document,
			&self.options,
			&self.images.snapshot,
		);
		// Syntax highlighting arrives from a worker, and this run has no event
		// loop to lay out again when it does, so wait for it and keep the colors.
		if self.engine.wait_highlights() {
			snapshot = self.engine.layout_with_images(
				document,
				&self.options,
				&self.images.snapshot,
			);
		}
		let pagination = paginate(document, &snapshot, &self.geometry);
		let source = document
			.markdown_parse()
			.map_or(document.document(), |parse| &parse.document);
		let metadata = self.metadata_of(source);
		let bytes = self.renderer.export(&Export {
			outline: document.outline_entries(),
			snapshot: &snapshot,
			images: &self.images.snapshot,
			prepared_images: Some(&self.images.pdf_images()),
			stylesheet: &self.options.stylesheet,
			geometry: &self.geometry,
			pagination: &pagination,
			metadata: metadata.clone(),
			path: self.document_path(),
			body_size_px: self.options.font_size,
			links: self.links,
			fonts: self.options.fonts.clone(),
		})?;
		before_write()?;
		write_pdf(&self.output, &bytes)?;
		// Only now is this build on the disk.
		self.dirty = false;
		let stats = ExportStats {
			pages: pagination.pages.len(),
			blocks: snapshot.blocks.len(),
			bytes: bytes.len(),
			reused: snapshot.reused,
			degraded: snapshot.degraded,
			math_errors: snapshot.math_errors,
			elapsed: started.elapsed(),
		};
		info!(
			"Exported {} pages, {} blocks, {} bytes to {} ({:.1}x{:.1}pt, {} degraded paragraphs, {} formula errors, {} reused blocks in {:.0} ms)",
			stats.pages,
			stats.blocks,
			stats.bytes,
			self.output.display(),
			self.geometry.width_pt,
			self.geometry.height_pt,
			stats.degraded,
			stats.math_errors,
			stats.reused,
			stats.elapsed.as_secs_f64() * 1000.0,
		);
		if let Some(title) = &metadata.title {
			log::debug!("PDF title: {title}");
		}
		Ok(stats)
	}

	/// Whether the picture or a local image the document references changed.
	fn image_changed(&mut self) -> bool {
		self.refresh_standalone_source() || self.images.poll()
	}

	/// What the information dictionary holds: the flags win, then the document's
	/// first heading, then the URL or file's name.
	fn metadata_of(
		&self,
		document: &markview_document::Document,
	) -> markview_pdf::Metadata {
		let title = self
			.metadata
			.title
			.clone()
			.or_else(|| title_of(document, &self.security, &self.path))
			.filter(|title| !title.trim().is_empty())
			.map(|title| title.trim().to_owned());
		markview_pdf::Metadata {
			title,
			authors: self.metadata.authors.clone(),
			subject: self.metadata.subject.clone(),
			keywords: self.metadata.keywords.clone(),
			language: self.metadata.language.clone(),
			creator: self.metadata.creator.clone(),
		}
	}

	fn document_path(&self) -> String {
		document_path(&self.security, &self.path)
	}
}

/// Writes `bytes` beside `output` and renames them into place, so a viewer that
/// reopens the PDF never sees a file that is still being written.
fn write_pdf(output: &Path, bytes: &[u8]) -> Result<()> {
	crate::export::write_atomic(output, bytes)
}

/// The document's own title: its first heading, else a web page's URL, else the
/// file's name. Anything the user did not ask for is left out rather than
/// invented, and a web page's synthetic session path is never one of them.
fn title_of(
	document: &markview_document::Document,
	security: &crate::security::Security,
	path: &Path,
) -> Option<String> {
	for block in document.blocks() {
		if let markview_document::BlockKind::Heading { text, .. } = block.kind()
		{
			let title = document::plain_text(text);
			if !title.trim().is_empty() {
				return Some(title.trim().to_owned());
			}
		}
	}
	match &security.origin {
		crate::security::Origin::Web(url) => Some(url.clone()),
		_ => path
			.file_stem()
			.map(|stem| stem.to_string_lossy().into_owned())
			.filter(|stem| !stem.is_empty()),
	}
}

/// How the PDF names this document: a web page's URL, never its synthetic
/// session path.
fn document_path(security: &crate::security::Security, path: &Path) -> String {
	match &security.origin {
		crate::security::Origin::Web(url) => url.clone(),
		_ => path.display().to_string(),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use markview_layout::style::CjkType;
	use std::fs;

	fn options(path: &Path, output: &Path) -> PdfRequest {
		crate::export::Config::from_settings(
			&crate::settings::ExportSettings::default(),
			crate::test_support::fonts(),
			CjkType::Sc,
			&[],
		)
		.prepare()
		.unwrap()
		.pdf_request(path.into(), output.into(), false)
	}

	fn start(dir: &Path, source: &str) -> (PathBuf, PathBuf, Exporter) {
		let path = dir.join("doc.md");
		let output = dir.join("out.pdf");
		fs::write(&path, source).unwrap();
		let exporter = Exporter::new(&options(&path, &output)).unwrap();
		(path, output, exporter)
	}

	/// A request for a document that arrived as a web page, with page furniture
	/// that names both its title and its path.
	fn web_request(
		dir: &Path,
		url: &str,
	) -> (PdfRequest, PathBuf, Arc<crate::loaded::LoadedDocument>) {
		let path = crate::app::web_page::web_path(url);
		let output = dir.join("article.pdf");
		let mut request = options(&path, &output);
		request.security = crate::security::Security::web(url.to_owned());
		let mut sheet = (*request.options.stylesheet).clone();
		sheet.page.header_left = Some("{path}".into());
		sheet.page.footer_center = Some("{title}".into());
		request.options.stylesheet = Arc::new(sheet);
		let loaded = crate::loaded::LoadedDocument::semantic(Arc::new(
			markview_commonmark::parse("Body without a heading.\n").document,
		));
		request.loaded = Some(loaded.clone());
		(request, output, loaded)
	}

	#[test]
	fn note_embed_pdf_cannot_replace_an_embedded_note() {
		let dir = crate::test_support::TestDir::new();
		let host = dir.path().join("A.md");
		let note = dir.path().join("B.md");
		let original = "# Section\n\nProtected embedded source";
		fs::write(&host, "![[B#Section]]").unwrap();
		fs::write(&note, original).unwrap();
		let output = dir.path().join("out.pdf");
		let mut request = options(&host, &output);
		request.options.ofm = true;
		export_once(&request).unwrap();
		let pdf = lopdf::Document::load(&output).unwrap();
		assert!(
			pdf.extract_text(&[1])
				.unwrap()
				.contains("Protected embedded source")
		);

		request.output = note.clone();
		let result = export_once(&request);
		assert_eq!(
			fs::read(&note).unwrap().get(..5),
			Some(&original.as_bytes()[..5]),
			"the export replaced the embedded source"
		);
		assert_eq!(fs::read_to_string(&note).unwrap(), original);
		assert!(result.is_err());
	}

	#[test]
	fn note_embed_export_protects_an_unreadable_target() {
		let dir = crate::test_support::TestDir::new();
		let host = dir.path().join("A.md");
		let note = dir.path().join("B.md");
		let original = b"not UTF-8: \xff";
		fs::write(&host, "![[B#Section]]").unwrap();
		fs::write(&note, original).unwrap();
		assert!(crate::file::read_document(&note).is_err());
		let mut request = options(&host, &note);
		request.options.ofm = true;
		let result = export_once(&request);
		assert_eq!(
			fs::read(&note).unwrap().get(..5),
			Some(&original[..5]),
			"export overwrote the unreadable note"
		);
		assert_eq!(fs::read(&note).unwrap(), original);
		assert!(result.is_err());
	}

	#[test]
	fn note_embed_buffer_filename_does_not_shadow_disk_notes() {
		let dir = crate::test_support::TestDir::new();
		let virtual_path = dir.path().join("Untitled.md");
		fs::write(&virtual_path, "Untitled external marker ^leaf").unwrap();
		fs::write(dir.path().join("Other.md"), "Other external marker ^leaf")
			.unwrap();
		let mut request = options(&virtual_path, &dir.path().join("out.pdf"));
		request.options.ofm = true;
		request.security =
			crate::security::Security::local(crate::security::Trust::Trusted);
		let services = Arc::new(crate::services::Services::new(2));
		export_text(
			&request,
			"![[Other#^leaf]]\n\n![[Untitled#^leaf]]",
			services,
			|| Ok(()),
		)
		.unwrap();
		let pdf = lopdf::Document::load(&request.output).unwrap();
		let text = pdf.extract_text(&[1]).unwrap();
		assert!(text.contains("Other external marker"), "{text}");
		assert!(text.contains("Untitled external marker"), "{text}");
	}

	#[test]
	fn note_embed_pdf_title_uses_the_host_note() {
		let dir = crate::test_support::TestDir::new();
		fs::write(dir.path().join("B.md"), "# Excerpt\n\nImported passage")
			.unwrap();
		let path = dir.path().join("A.md");
		let output = dir.path().join("A.pdf");
		for (heading, expected) in
			[("# Host Title\n\n", "Host Title"), ("", "A")]
		{
			fs::write(
				&path,
				format!("![[B#Excerpt]]\n\n{heading}Host passage"),
			)
			.unwrap();
			let mut request = options(&path, &output);
			request.options.ofm = true;
			export_once(&request).unwrap();
			let pdf = lopdf::Document::load(&output).unwrap();
			let text = pdf.extract_text(&[1]).unwrap();
			assert!(
				text.contains("Imported passage")
					&& text.contains("Host passage"),
				"{text}"
			);
			let info_id =
				pdf.trailer.get(b"Info").unwrap().as_reference().unwrap();
			let info = pdf.get_object(info_id).unwrap().as_dict().unwrap();
			assert_eq!(
				lopdf::decode_text_string(info.get(b"Title").unwrap()).unwrap(),
				expected
			);
		}
	}

	#[test]
	fn note_embed_pdf_source_links_keep_their_base_in_another_directory() {
		let dir = crate::test_support::TestDir::new();
		let notes = dir.path().join("notes");
		let out = dir.path().join("out");
		fs::create_dir(&notes).unwrap();
		fs::create_dir(&out).unwrap();
		let path = notes.join("A.md");
		let neighbor = notes.join("neighbor.md");
		fs::write(&neighbor, "Neighbor").unwrap();
		fs::write(
			&path,
			"![[#Included]]\n\n# Included\n\n[Next](neighbor.md) and [Local](#other)\n\n# Other\n\nOther section",
		).unwrap();
		let mut request = options(&path, &out.join("A.pdf"));
		request.options.ofm = true;
		export_once(&request).unwrap();
		let pdf = lopdf::Document::load(&request.output).unwrap();
		let annotations =
			pdf.get_page_annotations(pdf.get_pages()[&1]).unwrap();
		assert_eq!(annotations.len(), 4);
		let uris: Vec<_> = annotations
			.iter()
			.filter_map(|annotation| {
				annotation
					.get(b"A")
					.ok()?
					.as_dict()
					.ok()?
					.get(b"URI")
					.ok()?
					.as_str()
					.ok()
			})
			.collect();
		assert_eq!(uris.len(), 2, "bare fragments stay inside the PDF");
		let expected =
			url::Url::from_file_path(neighbor.canonicalize().unwrap()).unwrap();
		assert_eq!(uris, [expected.as_str().as_bytes(), b"neighbor.md"]);
	}

	#[test]
	fn note_embed_rebuilds_a_watched_pdf_from_an_unchanged_host() {
		let dir = crate::test_support::TestDir::new();
		let path = dir.path().join("A.md");
		let note = dir.path().join("B.md");
		let output = dir.path().join("out.pdf");
		fs::write(&path, "![[B#Section]]").unwrap();
		fs::write(&note, "# Section\n\nOriginal contents").unwrap();
		let mut request = options(&path, &output);
		request.options.ofm = true;
		let mut exporter = Exporter::new(&request).unwrap();
		assert!(exporter.export(false).unwrap().is_some());
		assert!(!exporter.embed_dependencies.changed());
		fs::write(&note, "# Section\n\nChanged contents in the target")
			.unwrap();
		assert!(exporter.embed_dependencies.changed());
		rebuild(&mut exporter, true);
		let output = lopdf::Document::load(&output).unwrap();
		let text = output.extract_text(&[1]).unwrap();
		assert!(text.contains("Changed contents in the target"), "{text}");
		assert!(!exporter.embed_dependencies.changed());
	}

	#[test]
	fn web_exports_name_the_url_and_never_the_session_path() {
		let dir = crate::test_support::TestDir::new();
		let url = "https://example.org/article";
		let (request, output, _) = web_request(dir.path(), url);
		export_with_services(
			&request,
			Arc::new(crate::services::Services::new(2)),
		)
		.unwrap();
		let bytes = fs::read(&output).unwrap();
		let pdf = lopdf::Document::load(&output).unwrap();
		let mut text = String::new();
		for page in 1..=pdf.get_pages().len() as u32 {
			text.push_str(&pdf.extract_text(&[page]).unwrap());
		}
		assert!(text.contains("Body without a heading"), "{text:?}");
		assert!(
			!text.contains("web:"),
			"page furniture leaked the synthetic session path: {text:?}"
		);
		assert!(
			!String::from_utf8_lossy(&bytes).contains("web:"),
			"the information dictionary leaked the synthetic session path"
		);
		assert!(
			String::from_utf8_lossy(&bytes).contains(&format!("/Title({url})")),
			"a web page without a heading is titled by its URL"
		);
	}

	#[test]
	fn exports_unsaved_text_and_checks_cancellation_before_replacing_output() {
		let dir = crate::test_support::TestDir::new();
		let path = dir.path().join("unsaved.md");
		let output = dir.path().join("out.pdf");
		let request = options(&path, &output);
		let services = Arc::new(crate::services::Services::new(2));
		export_text(
			&request,
			"# Unsaved snapshot",
			services.clone(),
			|| Ok(()),
		)
		.unwrap();
		assert!(!path.exists());
		let pdf = lopdf::Document::load(&output).unwrap();
		assert!(pdf.extract_text(&[1]).unwrap().contains("Unsaved snapshot"));
		let previous = fs::read(&output).unwrap();
		assert!(
			export_text(&request, "# Replacement", services, || {
				anyhow::bail!("Cancelled")
			})
			.is_err()
		);
		assert_eq!(fs::read(&output).unwrap(), previous);
	}

	#[test]
	fn exports_every_image_beyond_the_reader_pixel_budget() {
		let dir = crate::test_support::TestDir::new();
		let mut source = String::new();
		// Five decodes total 280 MB, exceeding the reader's 256 MiB cache.
		for i in 0..5 {
			image::RgbImage::from_pixel(
				4000,
				3500,
				image::Rgb([i * 40, 100, 200]),
			)
			.save(dir.path().join(format!("{i}.png")))
			.unwrap();
			source.push_str(&format!("![Image {i}]({i}.png)\n\n"));
		}
		let (_, output, mut exporter) = start(dir.path(), &source);
		for force in [false, true] {
			exporter.export(force).unwrap().unwrap();
			assert!(exporter.images.snapshot.decoded().is_empty());
			assert_eq!(exporter.images.pdf_images().len(), 5);
			let pdf = lopdf::Document::load(&output).unwrap();
			assert_eq!(
				pdf.objects
					.values()
					.filter_map(|object| object.as_stream().ok())
					.filter(|stream| stream
						.dict
						.get(b"Subtype")
						.and_then(lopdf::Object::as_name)
						.is_ok_and(|name| name == b"Image"))
					.count(),
				5,
				"each distinct opaque image must be embedded in the PDF"
			);
		}
	}

	#[test]
	fn compressed_images_preserve_alpha_and_share_aliases() {
		let dir = crate::test_support::TestDir::new();
		image::RgbaImage::from_raw(2, 1, vec![255, 0, 0, 255, 0, 255, 0, 128])
			.unwrap()
			.save(dir.path().join("a.png"))
			.unwrap();
		let (path, output, mut exporter) =
			start(dir.path(), "![first](a.png)\n\n![alias](./a.png)\n");
		exporter.export(false).unwrap().unwrap();
		assert!(exporter.images.snapshot.decoded().is_empty());
		let version = exporter.images.snapshot.entries["a.png"].version;
		assert_eq!(
			exporter.images.snapshot.entries["./a.png"].version,
			version
		);
		let pdf = lopdf::Document::load(&output).unwrap();
		let mut channels: Vec<_> = pdf
			.objects
			.values()
			.filter_map(|object| object.as_stream().ok())
			.filter(|stream| {
				stream
					.dict
					.get(b"Subtype")
					.and_then(lopdf::Object::as_name)
					.is_ok_and(|name| name == b"Image")
			})
			.map(|stream| stream.decompressed_content().unwrap())
			.collect();
		channels.sort();
		assert_eq!(channels, vec![vec![255, 0, 0, 0, 255, 0], vec![255, 128]]);
		fs::write(&path, "![alias](./a.png)\n").unwrap();
		exporter.export(false).unwrap().unwrap();
		assert_eq!(
			exporter.images.snapshot.entries["./a.png"].version,
			version
		);
		assert_eq!(exporter.images.pdf_images().len(), 1);
		fs::write(&path, "No images.\n").unwrap();
		exporter.export(false).unwrap().unwrap();
		assert!(exporter.images.pdf_images().is_empty());
	}

	#[test]
	fn an_unchanged_save_is_skipped_and_an_edit_reuses_untouched_blocks() {
		let dir = crate::test_support::TestDir::new();
		let (path, output, mut exporter) = start(
			dir.path(),
			"# Title\n\nFirst paragraph.\n\nSecond paragraph.\n",
		);
		let first = exporter.export(false).unwrap().expect("the first build");
		assert_eq!(first.reused, 0);
		assert!(fs::read(&output).unwrap().starts_with(b"%PDF"));
		assert!(exporter.export(false).unwrap().is_none());
		let before = fs::read(&output).unwrap();
		fs::write(&path, "# Title\n\nFirst paragraph.\n\nSecond paragraph!\n")
			.unwrap();
		let second = exporter.export(false).unwrap().expect("the edited build");
		assert!(second.reused >= 1, "{second:?}");
		assert_ne!(fs::read(&output).unwrap(), before);
	}

	#[test]
	fn a_forced_rebuild_refreshes_an_unchanged_document() {
		let dir = crate::test_support::TestDir::new();
		let (_, _, mut exporter) =
			start(dir.path(), "# Title\n\nA paragraph.\n");
		let first = exporter.export(false).unwrap().unwrap();
		let forced =
			exporter.export(true).unwrap().expect("the forced rebuild");
		assert_eq!(forced.reused, forced.blocks);
		assert_eq!(forced.blocks, first.blocks);
	}

	#[test]
	fn a_failed_rebuild_keeps_the_last_good_pdf() {
		let dir = crate::test_support::TestDir::new();
		let (path, output, mut exporter) =
			start(dir.path(), "# Title\n\nA paragraph.\n");
		exporter.export(false).unwrap().unwrap();
		let good = fs::read(&output).unwrap();
		fs::write(&path, [0xff, 0xfe]).unwrap();
		assert!(exporter.export(false).is_err());
		assert_eq!(fs::read(&output).unwrap(), good);
		fs::write(&path, "# Title\n\nA recovered paragraph.\n").unwrap();
		assert!(exporter.export(false).unwrap().is_some());
	}

	#[test]
	fn an_event_queued_before_the_loop_starts_is_not_lost() {
		let dir = crate::test_support::TestDir::new();
		let (_, output, mut exporter) =
			start(dir.path(), "# Title\n\nA paragraph.\n");
		let (tx, rx) = mpsc::channel();
		// A save during the initial build queues its event before the loop runs.
		tx.send(()).unwrap();
		drop(tx);
		watch(&mut exporter, rx).unwrap();
		assert!(fs::read(&output).unwrap().starts_with(b"%PDF"));
	}

	#[test]
	fn a_failed_write_leaves_the_save_eligible_for_a_retry() {
		let dir = crate::test_support::TestDir::new();
		let (_, output, mut exporter) =
			start(dir.path(), "# Title\n\nA paragraph.\n");
		// An existing directory cannot be replaced by the output rename.
		fs::create_dir(&output).unwrap();
		assert!(exporter.export(false).is_err());
		fs::remove_dir(&output).unwrap();
		// The same content is not skipped as unchanged, so the retry rebuilds.
		assert!(exporter.export(false).unwrap().is_some());
		assert!(fs::read(&output).unwrap().starts_with(b"%PDF"));
	}

	#[test]
	fn a_retargeted_picture_symlink_rebuilds_a_watched_export() {
		if !markview_platform::test_support::symlinks() {
			return;
		}
		let dir = crate::test_support::TestDir::new();
		let first = dir.path().join("first.png");
		let second = dir.path().join("second.png");
		image::RgbaImage::from_pixel(3, 2, image::Rgba([255, 0, 0, 255]))
			.save(&first)
			.unwrap();
		image::RgbaImage::from_pixel(4, 3, image::Rgba([0, 0, 255, 255]))
			.save(&second)
			.unwrap();
		let source = dir.path().join("current.png");
		let output = dir.path().join("out.pdf");
		markview_platform::test_support::symlink(&first, &source).unwrap();
		let mut exporter = Exporter::new(&options(&source, &output)).unwrap();
		exporter.export(false).unwrap().expect("the first build");
		fs::remove_file(&source).unwrap();
		markview_platform::test_support::symlink(&second, &source).unwrap();
		assert!(
			exporter.export(false).unwrap().is_some(),
			"a retargeted picture must rebuild even though its Markdown is unchanged"
		);
		fs::remove_file(&source).unwrap();
		markview_platform::test_support::symlink(&first, &source).unwrap();
		assert!(exporter.image_changed());
		assert!(exporter.export(true).unwrap().is_some());
	}

	#[test]
	fn a_failed_forced_rebuild_retries_an_unchanged_save() {
		let dir = crate::test_support::TestDir::new();
		let (_, output, mut exporter) =
			start(dir.path(), "# Title\n\nA paragraph.\n");
		exporter.export(false).unwrap().unwrap();
		// An image change forces a rebuild, and this one cannot reach the disk.
		fs::remove_file(&output).unwrap();
		fs::create_dir(&output).unwrap();
		assert!(exporter.export(true).is_err());
		fs::remove_dir(&output).unwrap();
		// A later save of the same content still rebuilds the missing PDF.
		assert!(exporter.export(false).unwrap().is_some());
		assert!(fs::read(&output).unwrap().starts_with(b"%PDF"));
	}
}
