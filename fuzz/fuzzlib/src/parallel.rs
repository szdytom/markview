//! Serial/parallel differentials with synchronous colors and pinned fonts.
use std::sync::atomic::{AtomicBool, Ordering};

use markview_document::Document;
use markview_layout::{
	image::{ImageInfo, ImageSnapshot},
	layout::{LayoutEngine, LayoutOptions, LayoutSnapshot},
	style::Stylesheet,
};

use crate::{edit, oracle, pipeline};

/// Exercise worker measurement even for an empty or single-block fuzz input.
const OPENING: &str = "Parallel probe alpha.\n\nParallel probe beta.\n\nParallel probe gamma.\n\n";

/// Compare cold, cached, resized, original, and cancelled/edited layouts.
pub fn check(md: &str) {
	let seed = oracle::derive(md.as_bytes());
	let mut pair = Pair {
		engines: [
			pipeline::differential_engine(),
			pipeline::differential_engine(),
		],
		worked: AtomicBool::new(false),
		history: Vec::new(),
	};
	let threads = 2 + (seed as usize % 3);
	assert_eq!(pair.engines[1].set_parallelism(threads).unwrap(), threads);
	let mut options = pipeline::options_for(md);
	let mut images = ImageSnapshot::default();
	let document = markview_commonmark::parse(format!("{OPENING}{md}"));
	let sample = 1 + seed as usize % (document.blocks().len() - 1);
	pair.pass(&document, &options, &images, sample, false);
	assert!(
		pair.worked.load(Ordering::Relaxed),
		"no block reached a layout worker"
	);
	pair.pass(&document, &options, &images, sample, false);

	options.width = 120. + options.width / 2.;
	options.stylesheet = Stylesheet::bundled(true);
	options.force_open = !options.force_open;
	let mut sources = Vec::new();
	for block in document.blocks() {
		block.images(&mut sources);
	}
	for image in sources {
		images
			.keys
			.insert(image.source.clone(), image.address().to_owned());
		images.entries.insert(
			image.address().to_owned(),
			ImageInfo {
				loading: false,
				version: 1,
				size: Some((
					1 + (seed as u32 % 800),
					1 + ((seed >> 32) as u32 % 600),
				)),
				error: None,
				svg: false,
			},
		);
	}
	images.generation = 1;
	pair.pass(&document, &options, &images, sample, false);

	// Preserve front matter and other constructs changed by the probe prefix.
	let original = markview_commonmark::parse(md.to_owned());
	pair.pass(&original, &options, &images, sample, false);
	let edited = if md.is_empty() {
		String::new()
	} else {
		edit::apply_deterministic_edit(md.as_bytes()).md1
	};
	let edited = markview_commonmark::parse(format!("{OPENING}{edited}"));
	for engine in &mut pair.engines {
		engine.clear_document_cache();
	}
	let stop = 2 + seed as usize % (edited.blocks().len() - 2);
	pair.pass(&edited, &options, &images, stop, true);
	pair.pass(&edited, &options, &images, stop, false);
}

struct Pair {
	engines: [LayoutEngine; 2],
	worked: AtomicBool,
	history: Vec<(LayoutSnapshot, oracle::Fingerprint)>,
}

impl Pair {
	fn pass(
		&mut self,
		document: &Document,
		options: &LayoutOptions,
		images: &ImageSnapshot,
		sample: usize,
		cancel: bool,
	) {
		for (snapshot, expected) in &self.history {
			assert_eq!(
				oracle::layout(snapshot),
				*expected,
				"retained snapshot changed after a later pass"
			);
		}
		let mut prefixes = Vec::new();
		let mut results = Vec::new();
		let caller = std::thread::current().id();
		for engine in &mut self.engines {
			let mut prefix = None;
			let result = engine.layout_progressive_cancellable(
				document,
				options,
				images,
				|snapshot| {
					if snapshot.blocks.len() == sample {
						prefix = Some(snapshot.clone());
					}
					!cancel || snapshot.blocks.len() < sample
				},
				|| {
					if std::thread::current().id() != caller {
						self.worked.store(true, Ordering::Relaxed);
					}
					false
				},
			);
			prefixes.push(prefix);
			results.push(result);
		}
		assert_eq!(
			prefixes[0].is_some(),
			prefixes[1].is_some(),
			"prefix publication differs"
		);
		if let (Some(a), Some(b)) = (&prefixes[0], &prefixes[1]) {
			assert_equivalent(a, b);
		}
		if cancel {
			assert!(
				results.iter().all(Option::is_none),
				"cancelled pass completed"
			);
		}
		assert_eq!(
			results[0].is_some(),
			results[1].is_some(),
			"cancellation differs"
		);
		if let (Some(a), Some(b)) = (&results[0], &results[1]) {
			assert_equivalent(a, b);
			let mut fresh = pipeline::differential_engine();
			let fresh = fresh.layout_with_images(document, options, images);
			assert_equivalent(a, &fresh);
			self.history.push((a.clone(), oracle::layout(a)));
			let mut a = a.clone();
			let mut b = b.clone();
			for force_open in [false, true] {
				a.set_disclosures(&Default::default(), force_open);
				b.set_disclosures(&Default::default(), force_open);
				assert_equivalent(&a, &b);
			}
		}
	}
}

/// Font blob identities and cache counters may differ; reader output may not.
pub fn assert_equivalent(a: &LayoutSnapshot, b: &LayoutSnapshot) {
	oracle::assert_snapshot_finite(a);
	oracle::assert_snapshot_finite(b);
	assert_eq!(
		oracle::layout(a),
		oracle::layout(b),
		"visible serial/parallel scene differs: {}",
		crate::behavior::difference(
			&crate::behavior::snapshot(a),
			&crate::behavior::snapshot(b),
			"snapshot"
		)
		.unwrap_or_else(|| "snapshot metadata".to_owned())
	);
	assert!(
		a.same_reading_text(b),
		"serial/parallel reading text differs"
	);
	for (a, b) in a.blocks.iter().zip(&b.blocks) {
		let mut ah = oracle::hashers();
		let mut bh = oracle::hashers();
		oracle::block_layout(&a.layout, &mut ah);
		oracle::block_layout(&b.layout, &mut bh);
		assert_eq!(
			oracle::finish(ah),
			oracle::finish(bh),
			"retained serial/parallel block {} differs",
			a.id
		);
	}
}
