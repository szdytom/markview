//! Pipeline helpers shared by the layout, differential, and PDF targets.
//!
//! Layout options are derived deterministically from the input's content, so
//! every comparison inside one process uses the same options, and every
//! corner of `Limits` gets visited as inputs mutate. Fonts are the committed
//! subsets, never the host's set, so results are reproducible across
//! machines.

use std::{
	path::Path,
	sync::{Arc, Once},
};

use markview_core::{
	document,
	fonts::FontConfig,
	image::ImageSnapshot,
	layout::{LayoutEngine, LayoutOptions, LayoutSnapshot},
	paginate::{PageGeometry, Pagination, paginate},
	style::{CjkType, Stylesheet},
};
use markview_pdf::{Export, Metadata};

use crate::oracle::derive;

/// The committed test faces; exports and layouts must not depend on what the
/// machine happens to have installed.
pub fn pinned_fonts() -> FontConfig {
	FontConfig {
		ignore_system_fonts: true,
		directories: vec![
			Path::new(env!("CARGO_MANIFEST_DIR"))
				.join("../../crates/markview-core/tests/fonts")
				.to_path_buf(),
		],
		..Default::default()
	}
}

/// Layout options derived from the input's hash: the reading width and size
/// vary, the `Limits` budgets are shrunk toward their floors so degradation
/// paths are reachable, and the reader toggles exercise the option bits.
pub fn options_for(md: &str) -> LayoutOptions {
	let seed = derive(md.as_bytes());
	let pick = |shift: u32, n: u128| (seed >> shift) % n;
	LayoutOptions {
		width: 120.0 + pick(32, 2600) as f32,
		font_size: 10.0 + pick(40, 40) as f32,
		justify: pick(48, 2) != 0,
		hyphenate: pick(49, 2) != 0,
		paragraph_indent: (pick(50, 8) as f32) / 4.0,
		greedy: pick(51, 4) == 0,
		codeblock_wrap: pick(52, 2) != 0,
		force_open: pick(53, 4) == 0,
		limits: crate::oracle::shrunk_limits(seed),
		// `LayoutOptions::default()` falls back to the host's fonts, which
		// would make the snapshot depend on what this machine happens to have
		// installed; the committed test faces keep comparisons stable.
		fonts: pinned_fonts(),
		..Default::default()
	}
}

/// Completes syntax colors before the first snapshot, so differential
/// fingerprints compare the same highlight state regardless of scheduling.
pub fn differential_engine() -> LayoutEngine {
	LayoutEngine::with_executor(
		Arc::new(markview_core::background::Direct),
		Arc::new(|| {}),
	)
}

/// One layout of a throwaway document, run once per process: the first
/// layout pays for fontconfig's cold caches, which is a warm-up cost, not
/// an input cost, so the layout-family targets call this before their
/// input guard starts.
pub fn warmup() {
	static ONCE: Once = Once::new();
	ONCE.call_once(|| {
		// A fenced block makes this one-time pass pay for fontconfig's cold
		// caches and the syntax-set build, which are warm-up costs, not input
		// costs, so the layout-family targets run it before their guard starts.
		let doc = document::parse(Arc::from("```rust\nlet x = 1;\n```\n"));
		let mut engine = LayoutEngine::new();
		let mut snapshot = engine.layout(&doc, &options_for("warm"));
		if engine.wait_highlights() {
			snapshot = engine.layout(&doc, &options_for("warm"));
		}
		drop(snapshot);
		// The syntax-state build above covers the warm languages; the rest
		// of the set would otherwise cost its first input ~200 ms inside
		// the worker, where the input budget would see it.
		markview_core::prewarm_highlight();
	});
}

/// Parses and lays out `md` with the derived options, waiting for the
/// asynchronous highlight pass so the geometry is the settled one.
pub fn parse_layout(
	md: &str,
) -> (document::Document, LayoutOptions, LayoutSnapshot) {
	let doc = document::parse(md.to_string());
	let options = options_for(md);
	let mut engine = LayoutEngine::new();
	let mut snapshot = engine.layout(&doc, &options);
	if engine.wait_highlights() {
		snapshot = engine.layout(&doc, &options);
	}
	(doc, options, snapshot)
}

/// One export: the print stylesheet, page geometry, pagination, and the PDF
/// bytes. Mirrors the reader's export path, including the highlight wait.
pub fn export_pdf(
	md: &str,
) -> (
	Vec<u8>,
	document::Document,
	LayoutSnapshot,
	Pagination,
	PageGeometry,
) {
	let sheet = {
		let mut sheet = (*Stylesheet::bundled_print()).clone();
		sheet.set_cjk_type(CjkType::Sc);
		Arc::new(sheet)
	};
	let document = document::parse(md.to_string());
	let geometry =
		PageGeometry::from_style(sheet.page()).expect("print page is valid");
	// The derived `Limits` keep the export's degradation paths reachable;
	// the page geometry stays the stylesheet's, like the reader's export.
	let options = LayoutOptions {
		width: geometry.text_px().0,
		codeblock_wrap: true,
		force_open: true,
		hide_front_matter: true,
		limits: crate::oracle::shrunk_limits(derive(md.as_bytes())),
		stylesheet: sheet.clone(),
		fonts: pinned_fonts(),
		..Default::default()
	};
	let mut engine = LayoutEngine::new();
	let mut snapshot = engine.layout(&document, &options);
	if engine.wait_highlights() {
		snapshot = engine.layout(&document, &options);
	}
	let snapshot = snapshot.flattened();
	let pagination = paginate(&document, &snapshot, &geometry);
	let images = synthetic_images(&snapshot);
	let bytes = markview_pdf::export(Export {
		snapshot: &snapshot,
		images: &images,
		prepared_images: None,
		stylesheet: &sheet,
		geometry: &geometry,
		pagination: &pagination,
		metadata: Metadata {
			title: Some("Fuzz".into()),
			..Default::default()
		},
		path: "fuzz.md".into(),
		body_size_px: options.font_size,
		links: true,
		fonts: pinned_fonts(),
	})
	.expect("the export succeeds");
	(bytes, document, snapshot, pagination, geometry)
}

/// A decoded 1×1 image for every source the layout drew, so an export of a
/// document that contains images reaches the image path instead of failing
/// there.
///
/// The reader supplies decoded pixels and precompressed resources; the harness
/// has neither, and `markview_pdf::export` correctly refuses to write a
/// document whose image it cannot embed. Without this, every input containing
/// an image aborts the target at the exporter's own precondition — the
/// `![alt](url)` shape the mutator produces constantly — and the export path
/// behind it is never exercised. The image's content is not what the PDF
/// oracles check; its presence on the page is.
fn synthetic_images(snapshot: &LayoutSnapshot) -> ImageSnapshot {
	use markview_core::image::{ImageInfo, ImagePixels, Pixels};

	let sources: std::collections::HashSet<&str> = snapshot
		.blocks
		.iter()
		.flat_map(|block| block.layout.draws.iter())
		.filter_map(|draw| match draw {
			markview_core::scene::Draw::Image { src, .. } => Some(src.as_str()),
			_ => None,
		})
		.collect();
	if sources.is_empty() {
		return ImageSnapshot::default();
	}
	let pixels = Arc::new(ImagePixels::default());
	let mut entries = std::collections::HashMap::new();
	for src in sources {
		entries.insert(
			src.to_owned(),
			ImageInfo {
				version: 0,
				size: Some((1, 1)),
				error: None,
			},
		);
		pixels.insert(
			src.to_owned(),
			0,
			Arc::new(Pixels {
				width: 1,
				height: 1,
				rgba: Arc::from([0u8, 0, 0, 255].as_slice()),
			}),
		);
	}
	ImageSnapshot {
		generation: 0,
		entries,
		pixels,
	}
}
