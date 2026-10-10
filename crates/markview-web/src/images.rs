//! Host-supplied image pixels and the geometry used for scheduling hints.

use markview_core::{
	document::Document,
	image::{ImageInfo, ImageSnapshot, MERMAID_SCHEME, Pixels},
	scene::{Draw, LayoutSnapshot, Rect},
};
use serde::Serialize;
use std::{collections::HashMap, sync::Arc};

#[derive(Default)]
pub(crate) struct Images {
	pub(crate) snapshot: ImageSnapshot,
	pub(crate) sources: Vec<String>,
	geometry: HashMap<String, Vec<ImageGeometry>>,
	geometry_pass: Option<(u64, Option<u64>, u64, u64)>,
	geometry_blocks: usize,
}

struct ImageGeometry {
	rect: Rect,
	block: usize,
	command: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Priority {
	region: &'static str,
	distance: Option<f32>,
}
impl Default for Priority {
	fn default() -> Self {
		Self {
			region: "unknown",
			distance: None,
		}
	}
}

impl Images {
	pub(crate) fn prepare(&mut self, document: &Document) {
		let generation = self.snapshot.generation + 1;
		*self = Self::default();
		self.snapshot.generation = generation;
		let mut specs = Vec::new();
		for block in &document.blocks {
			block.images(&mut specs);
		}
		for spec in specs {
			if !spec.src.starts_with(MERMAID_SCHEME)
				&& !self.snapshot.entries.contains_key(&spec.src)
			{
				self.sources.push(spec.src.clone());
				self.snapshot
					.entries
					.insert(spec.src.clone(), ImageInfo::default());
			}
		}
	}

	/// `generation` is a decimal string at the FFI boundary to preserve `u64`.
	pub(crate) fn complete(
		&mut self,
		generation: &str,
		src: &str,
		result: Result<Pixels, String>,
		limit: u32,
	) -> bool {
		if generation.parse::<u64>().ok() != Some(self.snapshot.generation) {
			return false;
		}
		let Some(info) = self.snapshot.entries.get_mut(src) else {
			return false;
		};
		if info.version != 0 {
			return false;
		}
		info.version = self.snapshot.generation;
		let result = result.and_then(|pixels| {
			if pixels.width == 0
				|| pixels.height == 0
				|| pixels.width > limit
				|| pixels.height > limit
				|| u64::from(pixels.width) * u64::from(pixels.height) * 4
					!= pixels.rgba.len() as u64
			{
				Err("Invalid image dimensions or RGBA length".into())
			} else {
				Ok(pixels)
			}
		});
		match result {
			Ok(pixels) => {
				info.size = Some((pixels.width, pixels.height));
				self.snapshot.pixels.insert(
					src.into(),
					info.version,
					Arc::new(pixels),
				);
			}
			Err(error) => info.error = Some(error),
		}
		true
	}

	pub(crate) fn priorities(
		&mut self,
		snapshot: &LayoutSnapshot,
		pass: Option<u64>,
		revision: u64,
		viewport: Rect,
		horizontal: &HashMap<(usize, usize), f32>,
	) -> HashMap<String, Priority> {
		if snapshot.images.generation != self.snapshot.generation {
			return HashMap::new();
		}
		let key = (
			snapshot.images.generation,
			pass,
			if pass.is_none() { revision } else { 0 },
			snapshot.presentation_key,
		);
		if self.geometry_pass != Some(key) {
			self.geometry.clear();
			self.geometry_blocks = 0;
			self.geometry_pass = Some(key);
		}
		for (bi, block) in snapshot
			.blocks
			.iter()
			.enumerate()
			.skip(self.geometry_blocks)
		{
			for (command, draw, position) in block.draws() {
				collect(
					draw,
					[position[0], block.y + position[1]],
					None,
					bi,
					command,
					&mut self.geometry,
				);
			}
		}
		self.geometry_blocks = snapshot.blocks.len();
		self.sources
			.iter()
			.map(|src| {
				let nearest = self
					.geometry
					.get(src)
					.into_iter()
					.flatten()
					.map(|image| {
						let block = &snapshot.blocks[image.block];
						let (offset, clip) = block.command_view(
							image.command,
							image.block,
							horizontal,
						);
						let rect = Rect {
							x: image.rect.x - offset,
							..image.rect
						};
						let visible =
							rect.intersect(viewport).is_some_and(|visible| {
								clip.is_none_or(|clip| {
									visible
										.intersect(Rect {
											y: clip.y + block.y,
											..clip
										})
										.is_some()
								})
							});
						let distance = (viewport.y - rect.y - rect.h)
							.max(rect.y - viewport.y - viewport.h)
							.max(0.);
						let region = if visible {
							"visible"
						} else if distance <= viewport.h {
							"near"
						} else {
							"offscreen"
						};
						Priority {
							region,
							distance: Some(distance),
						}
					})
					.min_by(|a, b| {
						(a.region != "visible")
							.cmp(&(b.region != "visible"))
							.then_with(|| {
								a.distance
									.unwrap()
									.total_cmp(&b.distance.unwrap())
							})
					})
					.unwrap_or_default();
				(src.clone(), nearest)
			})
			.collect()
	}
}

fn collect(
	draw: &Draw,
	position: [f32; 2],
	clip: Option<Rect>,
	block: usize,
	command: usize,
	out: &mut HashMap<String, Vec<ImageGeometry>>,
) {
	match draw {
		Draw::Image { src, rect, .. } => {
			let rect = Rect {
				x: rect.x + position[0],
				y: rect.y + position[1],
				..*rect
			};
			if let Some(rect) =
				clip.map_or(Some(rect), |clip| clip.intersect(rect))
			{
				out.entry(src.clone()).or_default().push(ImageGeometry {
					rect,
					block,
					command,
				});
			}
		}
		Draw::Clipped { rect, draws } => {
			let rect = Rect {
				x: rect.x + position[0],
				y: rect.y + position[1],
				..*rect
			};
			if let Some(clip) =
				clip.map_or(Some(rect), |clip| clip.intersect(rect))
			{
				for draw in draws {
					collect(draw, position, Some(clip), block, command, out);
				}
			}
		}
		_ => {}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use markview_core::{
		document::parse,
		scene::{BlockLayout, PlacedBlock},
	};

	fn pixels() -> Pixels {
		Pixels {
			width: 1,
			height: 1,
			rgba: Arc::from([1, 2, 3, 255]),
		}
	}

	#[test]
	fn sources_and_pixels_are_isolated_between_documents() {
		let mut images = Images::default();
		images.prepare(&parse("![a](a) ![duplicate](a) <img src=\"b\">\n\n```mermaid\ngraph TD\nA-->B\n```"));
		assert_eq!(images.sources, ["a", "b"]);
		assert!(images.complete("1", "a", Ok(pixels()), 4096));
		assert!(!images.complete("1", "a", Err("duplicate".into()), 4096));
		let old = images.snapshot.clone();
		images.prepare(&parse("![a](a)"));
		assert!(!images.complete("1", "a", Ok(pixels()), 4096));
		assert_eq!(old.decoded()["a"].rgba.as_ref(), &[1, 2, 3, 255]);
		assert!(images.snapshot.decoded().is_empty());
		assert!(images.complete(
			"2",
			"a",
			Ok(Pixels {
				width: 2,
				..pixels()
			}),
			4096
		));
		assert!(images.snapshot.entries["a"].error.is_some());
		images.prepare(&parse("![a](a)"));
		assert!(images.complete("3", "a", Ok(pixels()), 0));
		assert!(images.snapshot.entries["a"].error.is_some());
	}

	#[test]
	fn priorities_follow_prefixes_and_choose_the_nearest_occurrence() {
		let mut images = Images::default();
		images.prepare(&parse("![a](a) ![b](b)"));
		let image = |src: &str, y| Draw::Image {
			src: src.into(),
			version: 0,
			rect: Rect {
				x: 0.,
				y,
				w: 10.,
				h: 10.,
			},
			title: String::new(),
		};
		let block = |draws| PlacedBlock {
			flow: Default::default(),
			id: 1,
			source: 0..0,
			y: 0.,
			layout: Arc::new(BlockLayout {
				draws,
				..Default::default()
			}),
		};
		let mut snapshot = LayoutSnapshot {
			images: images.snapshot.clone(),
			..Default::default()
		};
		snapshot.blocks.push(block(vec![image("a", 500.)]));
		let view = Rect {
			x: 0.,
			y: 0.,
			w: 100.,
			h: 100.,
		};
		let first =
			images.priorities(&snapshot, Some(1), 1, view, &HashMap::new());
		assert_eq!(first["a"].region, "offscreen");
		assert_eq!(first["b"], Priority::default());
		snapshot
			.blocks
			.push(block(vec![image("a", 20.), image("b", 180.)]));
		let next =
			images.priorities(&snapshot, Some(1), 2, view, &HashMap::new());
		assert_eq!(next["a"].region, "visible");
		assert_eq!(next["b"].region, "near");
		let scrolled = images.priorities(
			&snapshot,
			Some(1),
			2,
			Rect { y: 500., ..view },
			&HashMap::new(),
		);
		assert_eq!(scrolled["a"].region, "visible");
		assert_eq!(scrolled["b"].distance, Some(310.));
		assert_eq!(images.geometry["a"].len(), 2);
	}

	#[test]
	fn overflow_clipping_and_nested_clips_follow_command_offsets() {
		let mut images = Images::default();
		images.prepare(&parse("![a](a) ![b](b)"));
		let image = |src: &str, x| Draw::Image {
			src: src.into(),
			version: 0,
			rect: Rect {
				x,
				y: 10.,
				w: 10.,
				h: 10.,
			},
			title: String::new(),
		};
		let snapshot = LayoutSnapshot {
			images: images.snapshot.clone(),
			blocks: vec![PlacedBlock {
				flow: Default::default(),
				id: 1,
				source: 0..0,
				y: 30.,
				layout: Arc::new(BlockLayout {
					draws: vec![
						image("a", 5.),
						Draw::Clipped {
							rect: Rect {
								x: 40.,
								y: 0.,
								w: 20.,
								h: 100.,
							},
							draws: vec![image("b", 40.)],
						},
					],
					overflow: vec![markview_core::scene::Overflow {
						rect: Rect {
							x: 0.,
							y: 0.,
							w: 20.,
							h: 100.,
						},
						content_width: 70.,
						commands: 0..2,
						gutter: 0.,
					}],
					..Default::default()
				}),
			}],
			..Default::default()
		};
		let view = Rect {
			x: -50.,
			y: 0.,
			w: 100.,
			h: 100.,
		};
		let initial =
			images.priorities(&snapshot, Some(1), 1, view, &HashMap::new());
		assert_eq!(initial["a"].region, "visible");
		assert_eq!(initial["b"].region, "near");
		let panned = images.priorities(
			&snapshot,
			Some(1),
			1,
			view,
			&HashMap::from([((0, 0), 40.)]),
		);
		assert_eq!(panned["a"].region, "near");
		assert_eq!(panned["b"].region, "visible");
		assert_eq!(images.geometry["b"].len(), 1);
	}
}
