use markview_layout::{
	background::Direct,
	fonts::FontConfig,
	layout::{Draw, LayoutEngine, LayoutOptions, LayoutSnapshot},
	style::Stylesheet,
};
use std::{sync::Arc, time::Duration};

fn engine(threads: usize) -> LayoutEngine {
	let mut engine =
		LayoutEngine::with_executor(Arc::new(Direct), Arc::new(|| {}));
	let actual = engine.set_parallelism(threads).unwrap();
	assert_eq!(
		actual,
		if cfg!(all(feature = "parallel", not(target_arch = "wasm32"))) {
			threads.max(1)
		} else {
			1
		}
	);
	engine
}

fn options() -> LayoutOptions {
	let mut stylesheet = (*Stylesheet::bundled(false)).clone();
	stylesheet.merge(&Stylesheet::parse("format_version=2\nversion=1\n[[rule]]\nwhen=['p','first_child']\nsize=1.3\n[[rule]]\nwhen=['p','last_child']\nspace_after=2").unwrap());
	LayoutOptions {
		width: 320.,
		stylesheet: Arc::new(stylesheet),
		fonts: FontConfig::from_faces(
			0x706172616c6c656c,
			vec![parley::fontique::Blob::new(Arc::new(
				include_bytes!("fonts/NotoSerif-Regular-subset.otf").as_slice(),
			))],
		),
		..Default::default()
	}
}

fn draws(commands: &[Draw]) -> Vec<String> {
	commands
		.iter()
		.map(|draw| match draw {
			Draw::Glyph(g) => format!(
				"{:?}",
				(
					g.font.index,
					markview_document::fingerprint(&g.font.data.data()),
					&g.coords,
					g.id,
					g.size,
					g.x,
					g.y,
					g.synthetic_italic,
					g.paint
				)
			),
			Draw::Clipped { rect, draws: inner } => {
				format!("{rect:?} {:?}", draws(inner))
			}
			other => format!("{other:?}"),
		})
		.collect()
}

fn equivalent(a: &LayoutSnapshot, b: &LayoutSnapshot) {
	assert_eq!(
		(a.width, a.height, a.reused, a.degraded, a.math_errors),
		(b.width, b.height, b.reused, b.degraded, b.math_errors)
	);
	assert!(a.same_reading_text(b));
	let a = a.flattened();
	let b = b.flattened();
	assert_eq!(a.blocks.len(), b.blocks.len());
	for (a, b) in a.blocks.iter().zip(&b.blocks) {
		assert_eq!(
			(a.id, &a.source, a.y, a.height()),
			(b.id, &b.source, b.y, b.height())
		);
		assert_eq!(draws(&a.layout.draws), draws(&b.layout.draws));
		assert_eq!(
			format!(
				"{:?}",
				(
					&a.layout.text,
					&a.layout.links,
					&a.layout.anchors,
					&a.layout.overflow,
					&a.layout.page_constraints
				)
			),
			format!(
				"{:?}",
				(
					&b.layout.text,
					&b.layout.links,
					&b.layout.anchors,
					&b.layout.overflow,
					&b.layout.page_constraints
				)
			)
		);
	}
}

fn source() -> String {
	let mut source = String::from("<p id='opening'>Opening paragraph.</p>\n\n");
	for i in 0..4 {
		source.push_str(&format!("## Heading {i}\n\nParagraph {i} with **bold**, *italic* and a [link](https://example.invalid/{i}).\n\n> Quote {i}.\n>\n> Another paragraph.\n\n- List item {i}\n- Another item\n\n<details>\n<summary>Summary {i}</summary>\n\nHidden body {i}.\n\n</details>\n\n| A | B |\n|---|---|\n| Cell {i} | Content |\n\n```rust\nlet value = {i};\n```\n\n---\n\n"));
	}
	source.push_str(
		"![Caption](image.png)\n\nRepeated paragraph.\n\nRepeated paragraph.\n\n<a id='ending'></a>\n\nFinal paragraph.\n",
	);
	source
}

#[test]
fn parallel_geometry_matches_serial_through_reflows_edits_and_cache_reuse() {
	let document = markview_commonmark::parse(source());
	let mut serial = engine(1);
	let mut parallel = engine(4);
	let mut options = options();
	for width in [320., 320., 220., 460.] {
		options.width = width;
		equivalent(
			&serial.layout(&document, &options),
			&parallel.layout(&document, &options),
		);
	}
	for (version, size) in
		[(1, None), (2, Some((120, 80))), (3, Some((240, 100)))]
	{
		let mut images = markview_layout::image::ImageSnapshot::default();
		images.entries.insert(
			"image.png".into(),
			markview_layout::image::ImageInfo {
				version,
				size,
				error: None,
				svg: false,
			},
		);
		equivalent(
			&serial.layout_with_images(&document, &options, &images),
			&parallel.layout_with_images(&document, &options, &images),
		);
	}
	parallel.set_parallelism(2).unwrap();
	let edited =
		markview_commonmark::parse(source().replace(
			"Hidden body 1",
			"An edited hidden body 1 with more words",
		));
	equivalent(
		&serial.layout(&edited, &options),
		&parallel.layout(&edited, &options),
	);
	options.force_open = true;
	equivalent(
		&serial.layout(&edited, &options),
		&parallel.layout(&edited, &options),
	);
	options.stylesheet = Stylesheet::bundled(true);
	options.fonts = FontConfig::from_faces(
		0x706172616c6c656d,
		vec![parley::fontique::Blob::new(Arc::new(
			include_bytes!("fonts/NotoSerif-Regular-subset.otf").as_slice(),
		))],
	);
	equivalent(
		&serial.layout(&edited, &options),
		&parallel.layout(&edited, &options),
	);
	parallel.set_parallelism(0).unwrap();
	equivalent(
		&serial.layout(&edited, &options),
		&parallel.layout(&edited, &options),
	);
}

#[test]
fn prefixes_cancel_without_caching_unpublished_speculation_and_steps_stay_serial()
 {
	let document = markview_commonmark::parse(source());
	let options = options();
	let mut parallel = engine(4);
	let mut prefix = None;
	assert!(
		parallel
			.layout_progressive(&document, &options, &Default::default(), |p| {
				assert_eq!(
					p.blocks.len(),
					prefix
						.as_ref()
						.map_or(0, |p: &LayoutSnapshot| p.blocks.len() + 1)
				);
				prefix = Some(p.clone());
				p.blocks.len() < 2
			})
			.is_none()
	);
	let prefix = prefix.unwrap();
	let full = parallel.layout(&document, &options);
	// Speculation must not add cache hits beyond the two published blocks.
	assert_eq!(
		full.reused,
		engine(1).layout(&document, &options).reused + 2
	);
	for (a, b) in prefix.blocks.iter().zip(&full.blocks) {
		assert!(Arc::ptr_eq(&a.layout, &b.layout));
	}
	parallel.clear_document_cache();
	let mut stepped =
		parallel.begin_layout(&document, &options, &Default::default());
	let mut count = 0;
	while !stepped.is_complete() {
		parallel.advance(&mut stepped, &document, Duration::ZERO);
		count += 1;
		assert_eq!(stepped.blocks(), count);
	}
	equivalent(
		&engine(1).layout(&document, &options),
		&stepped.into_snapshot(),
	);
}

#[test]
fn nested_formulas_preserve_ordered_document_budgets() {
	let body = source();
	let document = markview_commonmark::parse(format!(
		"{body}\n$x$\n\n{body}\n<details>\n<summary>$y$</summary>\n\n> $z$\n\n</details>\n\n{body}\n- $a$\n\n| A | B |\n|---|---|\n| $b$ | $c$ |\n\nText[^a].\n\n[^a]: $d$\n"
	));
	let mut options = options();
	options.limits.math_bytes = 2;
	let mut serial = engine(1);
	let mut parallel = engine(3);
	let first = parallel.layout(&document, &options);
	assert!(first.math_errors > 0);
	equivalent(&serial.layout(&document, &options), &first);
	equivalent(
		&serial.layout(&document, &options),
		&parallel.layout(&document, &options),
	);
	options.width = 240.;
	equivalent(
		&serial.layout(&document, &options),
		&parallel.layout(&document, &options),
	);
}

#[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
#[test]
fn workers_observe_cancellation_without_publishing_partial_geometry() {
	use std::sync::atomic::{AtomicBool, Ordering};
	let document = markview_commonmark::parse(source());
	let options = options();
	let mut parallel = engine(4);
	let cancelled = AtomicBool::new(false);
	assert!(
		parallel
			.layout_progressive_cancellable(
				&document,
				&options,
				&Default::default(),
				|p| {
					assert!(p.blocks.len() <= 1);
					true
				},
				|| {
					if std::thread::current()
						.name()
						.is_some_and(|name| name.starts_with("markview-block-"))
					{
						cancelled.store(true, Ordering::Relaxed);
					}
					cancelled.load(Ordering::Relaxed)
				}
			)
			.is_none()
	);
	assert!(cancelled.load(Ordering::Relaxed));
	let snapshot = parallel.layout(&document, &options);
	assert_eq!(
		snapshot.reused,
		engine(1).layout(&document, &options).reused + 1
	);
}

#[test]
fn viewport_coverage_precedes_workers_even_after_a_warm_reflow() {
	use std::sync::atomic::{AtomicBool, Ordering};
	let document = markview_commonmark::parse(source());
	let mut options = options();
	let mut parallel = engine(4);
	parallel.set_parallel_start(500.);
	let viewport = AtomicBool::new(false);
	let worked = AtomicBool::new(false);
	for width in [320., 280., 400.] {
		options.width = width;
		parallel.clear_document_cache();
		viewport.store(false, Ordering::Relaxed);
		let snapshot = parallel
			.layout_progressive_cancellable(
				&document,
				&options,
				&Default::default(),
				|prefix| {
					if prefix.height >= 500. {
						viewport.store(true, Ordering::Relaxed);
					}
					true
				},
				|| {
					if std::thread::current()
						.name()
						.is_some_and(|n| n.starts_with("markview-block-"))
					{
						assert!(viewport.load(Ordering::Relaxed));
						worked.store(true, Ordering::Relaxed);
					}
					false
				},
			)
			.unwrap();
		equivalent(&engine(1).layout(&document, &options), &snapshot);
	}
	assert_eq!(
		worked.load(Ordering::Relaxed),
		cfg!(all(feature = "parallel", not(target_arch = "wasm32")))
	);
}
