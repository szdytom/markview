//! Shrinkable state histories compared with a fresh synchronous engine.
use crate::{coverage, oracle, pipeline};
use markview_layout::{
	image::{ImageInfo, ImageSnapshot, Pixels},
	layout::LayoutOptions,
	style::Stylesheet,
};
use std::sync::Arc;

/// `LDH1`, operation count, `(opcode, value)` pairs, then Markdown. Deleting a
/// pair and decrementing the count removes one operation without changing text.
pub fn check(data: &[u8]) {
	if data.len() < 5 || !data.starts_with(b"LDH1") {
		return;
	}
	let count = usize::from(data[4]).min(16);
	let Some(body) = data.get(5 + count * 2..) else {
		return;
	};
	if body.len() > 32 * 1024 {
		return;
	}
	let md = String::from_utf8_lossy(body);
	let mut document = markview_commonmark::parse(md.to_string());
	let mut engine = pipeline::differential_engine();
	let mut options = LayoutOptions {
		fonts: pipeline::pinned_fonts(),
		..Default::default()
	};
	let mut images = ImageSnapshot::default();
	// Populate the original cache before applying even a one-operation history.
	engine.layout_with_images(&document, &options, &images);
	let mut trace = Vec::new();
	for pair in data[5..5 + count * 2].as_chunks::<2>().0 {
		let (op, value) = (pair[0] % 10, pair[1]);
		trace.push((op, value));
		match op {
			0 => {
				options.width = [80., 180., 480., 760.][usize::from(value) % 4]
			}
			1 => options.font_size = 10. + f32::from(value % 32),
			2 => {
				let patch = Stylesheet::parse(&format!("format_version = 2\nversion = 1\ntargets = [\"ui\"]\n[[rule]]\nwhen = [\"p\"]\nspace_after = {}\nline_height = {}\n", f32::from(value % 8) / 4., 1. + f32::from(value % 5) / 4.)).unwrap();
				let mut sheet = if value % 2 == 0 {
					(*Stylesheet::bundled(false)).clone()
				} else {
					(*options.stylesheet).clone()
				};
				sheet.merge(&patch);
				options.stylesheet = Arc::new(sheet);
			}
			3 => {
				document = markview_commonmark::parse(if value % 2 == 0 {
					md.to_string()
				} else {
					format!(
						"<figure>\n\n[figure occurrence](https://example.org)\n\n{md}\n\n<figcaption>caption</figcaption>\n</figure>\n"
					)
				})
			}
			4 => options.force_open = value % 2 != 0,
			5 => {
				images.generation += 1;
				let mut sources = Vec::new();
				for block in document.blocks() {
					block.images(&mut sources);
				}
				for image in sources {
					let address = image.address().to_owned();
					let size =
						(1 + u32::from(value % 8), 1 + u32::from(value / 8));
					images.keys.insert(image.source.clone(), address.clone());
					images.entries.insert(
						address.clone(),
						ImageInfo {
							version: images.generation,
							size: Some(size),
							..Default::default()
						},
					);
					images.pixels.insert(
						address,
						images.generation,
						Arc::new(Pixels {
							width: size.0,
							height: size.1,
							rgba: vec![value; (size.0 * size.1 * 4) as usize]
								.into(),
						}),
					);
				}
			}
			6 => {
				engine.layout_progressive_cancellable(
					&document,
					&options,
					&images,
					|_| false,
					|| true,
				);
			}
			7 => engine.clear_document_cache(),
			8 => engine.release_document(),
			9 => document = markview_commonmark::parse(document.source.clone()),
			_ => unreachable!(),
		}
		coverage::hit(coverage::Hit::HistoryState);
		let warm = engine.layout_with_images(&document, &options, &images);
		let cold = pipeline::differential_engine()
			.layout_with_images(&document, &options, &images);
		assert_eq!(
			warm.full_reading_text(),
			cold.full_reading_text(),
			"reading text, trace={trace:?}"
		);
		assert_eq!(
			warm.blocks.len(),
			cold.blocks.len(),
			"block count, trace={trace:?}"
		);
		for (index, (a, b)) in warm.blocks.iter().zip(&cold.blocks).enumerate()
		{
			assert_eq!(
				a.id, b.id,
				"block identity at {index}, trace={trace:?}"
			);
			assert_eq!(a.y, b.y, "block y at {index}, trace={trace:?}");
			assert_eq!(
				a.layout.width, b.layout.width,
				"block width at {index}, trace={trace:?}"
			);
			assert_eq!(
				a.layout.height, b.layout.height,
				"block height at {index}, trace={trace:?}"
			);
			assert_eq!(
				format!("{:?}", a.layout.text),
				format!("{:?}", b.layout.text),
				"reading fields at {index}, trace={trace:?}"
			);
			assert_eq!(
				format!("{:?}", a.layout.links),
				format!("{:?}", b.layout.links),
				"links at {index}, trace={trace:?}"
			);
		}
		assert_eq!(
			oracle::layout(&warm),
			oracle::layout(&cold),
			"scene, trace={trace:?}"
		);
		oracle::assert_snapshot_finite(&warm);
	}
}
