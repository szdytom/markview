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
	export::{MetadataOverrides, PageOverrides, PdfRequest},
	file::read_document,
	images::Images,
	layout::{LayoutEngine, LayoutOptions},
	paginate::{PageGeometry, paginate},
	watch::FileWatch,
};
use anyhow::{Context, Result};
use log::{info, warn};
use markview_core::style::{PageStyle, Stylesheet};
use markview_pdf::{Export, Renderer};
use std::{
	path::{Path, PathBuf},
	sync::{Arc, mpsc},
	time::{Duration, Instant},
};

/// How long the watch loop waits before it checks local image stamps again.
const IMAGE_POLL: Duration = Duration::from_millis(500);

/// The paper the export uses: the print stylesheet with the flags on top.
pub fn styled(sheet: Arc<Stylesheet>, page: &PageOverrides) -> Arc<Stylesheet> {
	let mut sheet = (*sheet).clone();
	if let Some(paper) = &page.paper {
		sheet.page.size = Some(paper.clone());
	}
	if page.landscape {
		sheet.page.landscape = Some(true);
	}
	if let Some(margin) = page.margin {
		sheet.page.margin = Some(margin.to_vec());
	}
	for (slot, value) in page.header.iter().enumerate() {
		if let Some(value) = value {
			slot_of(&mut sheet.page, true, slot, value.clone());
		}
	}
	for (slot, value) in page.footer.iter().enumerate() {
		if let Some(value) = value {
			slot_of(&mut sheet.page, false, slot, value.clone());
		}
	}
	Arc::new(sheet)
}

fn slot_of(page: &mut PageStyle, header: bool, slot: usize, value: String) {
	let target = match (header, slot) {
		(true, 0) => &mut page.header_left,
		(true, 1) => &mut page.header_center,
		(true, _) => &mut page.header_right,
		(false, 0) => &mut page.footer_left,
		(false, 1) => &mut page.footer_center,
		(false, _) => &mut page.footer_right,
	};
	*target = Some(value);
}

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
				if exporter.image_changed() {
					rebuild(exporter, true);
				}
			}
			Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
		}
	}
}

/// Exports editor-owned bytes while keeping the logical document path for
/// relative resources, metadata and page furniture.
pub(crate) fn export_buffer(args: &PdfRequest, text: &str) -> Result<()> {
	Exporter::new(args)?.export_text(text, true)?;
	Ok(())
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
	path: PathBuf,
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
	document: Option<Arc<document::Document>>,
	revision: u64,
}

impl Exporter {
	fn new(args: &PdfRequest) -> Result<Self> {
		let output = args.output.clone();
		let stylesheet = styled(args.options.stylesheet.clone(), &args.page);
		let geometry = PageGeometry::from_style(stylesheet.page())?;
		// The page's text measure replaces the reader's reading column, and a
		// printed sheet shows every `<details>` body but no front matter.
		let options = LayoutOptions {
			width: geometry.text_px().0,
			codeblock_wrap: true,
			force_open: true,
			hide_front_matter: true,
			stylesheet,
			..args.options.clone()
		};
		let mut engine = LayoutEngine::new();
		engine.validate_stylesheet(&options.stylesheet)?;
		Ok(Self {
			path: args.path.clone(),
			output,
			options,
			geometry,
			metadata: args.metadata.clone(),
			links: args.links,
			engine,
			images: Images::new(args.offline, args.options.fonts.clone()),
			renderer: Renderer::default(),
			source: None,
			dirty: true,
			document: None,
			revision: 0,
		})
	}

	/// Rebuilds the PDF, or returns `None` when the document is unchanged and
	/// `force` did not ask for a rebuild anyway.
	fn export(&mut self, force: bool) -> Result<Option<ExportStats>> {
		let text = read_document(&self.path)?;
		self.export_text(&text, force)
	}

	fn export_text(
		&mut self,
		text: &str,
		force: bool,
	) -> Result<Option<ExportStats>> {
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
			let source: Arc<str> = text.into();
			let document = match &self.document {
				Some(previous) => {
					Arc::new(document::reparse(previous, source.clone()))
				}
				None => Arc::new(document::parse(source.clone())),
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
		self.images.prepare(
			&document,
			&self.path,
			self.revision,
			false,
			&self.options.stylesheet,
			&self.options.fonts,
		);
		self.images.wait();
		for entry in self.images.snapshot.entries.values() {
			if let Some(error) = &entry.error {
				warn!("Image: {error}");
			}
		}
		let mut snapshot = self.engine.layout_with_images(
			&document,
			&self.options,
			&self.images.snapshot,
		);
		// Syntax highlighting arrives from a worker, and this run has no event
		// loop to lay out again when it does, so wait for it and keep the colors.
		if self.engine.wait_highlights() {
			snapshot = self.engine.layout_with_images(
				&document,
				&self.options,
				&self.images.snapshot,
			);
		}
		let pagination = paginate(&document, &snapshot, &self.geometry);
		let metadata = metadata_of(&self.metadata, &document, &self.path);
		let bytes = self.renderer.export(&Export {
			snapshot: &snapshot,
			images: &self.images.snapshot,
			stylesheet: &self.options.stylesheet,
			geometry: &self.geometry,
			pagination: &pagination,
			metadata: metadata.clone(),
			path: self.path.display().to_string(),
			body_size_px: self.options.font_size,
			links: self.links,
			fonts: self.options.fonts.clone(),
		})?;
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
		Ok(Some(stats))
	}

	/// Whether a local image the document references changed on disk.
	fn image_changed(&mut self) -> bool {
		self.images.poll()
	}
}

/// Writes `bytes` beside `output` and renames them into place, so a viewer that
/// reopens the PDF never sees a file that is still being written.
fn write_pdf(output: &Path, bytes: &[u8]) -> Result<()> {
	crate::export::write_atomic(output, bytes)
}

/// What the information dictionary holds: the flags win, then the document's
/// first heading, then the file's name. Anything the user did not ask for is
/// left out rather than invented.
fn metadata_of(
	overrides: &MetadataOverrides,
	document: &markview_core::document::Document,
	path: &Path,
) -> markview_pdf::Metadata {
	let title = overrides
		.title
		.clone()
		.or_else(|| title_of(document, path))
		.filter(|title| !title.trim().is_empty())
		.map(|title| title.trim().to_owned());
	markview_pdf::Metadata {
		title,
		authors: overrides.authors.clone(),
		subject: overrides.subject.clone(),
		keywords: overrides.keywords.clone(),
		language: overrides.language.clone(),
		creator: overrides.creator.clone(),
	}
}

/// The document's own title: its first heading, or the file's name.
fn title_of(
	document: &markview_core::document::Document,
	path: &Path,
) -> Option<String> {
	for block in &document.blocks {
		if let markview_core::document::BlockKind::Heading { text, .. } =
			&block.kind
		{
			let title = document::plain_text(text);
			if !title.trim().is_empty() {
				return Some(title.trim().to_owned());
			}
		}
	}
	path.file_stem()
		.map(|stem| stem.to_string_lossy().into_owned())
		.filter(|stem| !stem.is_empty())
}

#[cfg(test)]
mod tests {
	use super::*;
	use markview_core::style::Stylesheet;
	use std::fs;

	fn options(path: &Path, output: &Path) -> PdfRequest {
		PdfRequest {
			path: path.into(),
			output: output.into(),
			options: LayoutOptions {
				stylesheet: Stylesheet::bundled_print(),
				fonts: crate::test_support::fonts(),
				..Default::default()
			},
			page: PageOverrides::default(),
			metadata: MetadataOverrides::default(),
			links: true,
			offline: false,
		}
	}

	fn start(dir: &Path, source: &str) -> (PathBuf, PathBuf, Exporter) {
		let path = dir.join("doc.md");
		let output = dir.join("out.pdf");
		fs::write(&path, source).unwrap();
		let exporter = Exporter::new(&options(&path, &output)).unwrap();
		(path, output, exporter)
	}

	#[test]
	fn an_unchanged_save_is_skipped_and_an_edit_reuses_untouched_blocks() {
		let dir = tempfile::tempdir().unwrap();
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
		let dir = tempfile::tempdir().unwrap();
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
		let dir = tempfile::tempdir().unwrap();
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
		let dir = tempfile::tempdir().unwrap();
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
		let dir = tempfile::tempdir().unwrap();
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
	fn a_failed_forced_rebuild_retries_an_unchanged_save() {
		let dir = tempfile::tempdir().unwrap();
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
