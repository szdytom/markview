use anyhow::{Context, Result, bail};
use image::{Rgba, RgbaImage};
use markview_core::scene::Draw;
use std::{
	collections::BTreeSet,
	fs,
	path::{Path, PathBuf},
};

const CHANNEL_TOLERANCE: u8 = 2;

pub struct Baselines {
	pub artifacts: PathBuf,
	directory: PathBuf,
	update: bool,
	names: BTreeSet<String>,
	failures: Vec<String>,
}

impl Baselines {
	pub fn new(directory: PathBuf, artifacts: PathBuf) -> Result<Self> {
		if artifacts.exists() {
			fs::remove_dir_all(&artifacts)?;
		}
		fs::create_dir_all(&artifacts)?;
		let update = std::env::var("MARKVIEW_UPDATE_RENDER_GOLDENS").as_deref()
			== Ok("1");
		if update {
			fs::create_dir_all(&directory)?;
		}
		Ok(Self {
			artifacts,
			directory,
			update,
			names: BTreeSet::new(),
			failures: Vec::new(),
		})
	}

	pub fn record(&mut self, name: &str, actual: &RgbaImage) -> Result<()> {
		anyhow::ensure!(
			self.names.insert(name.into()),
			"Duplicate baseline {name}"
		);
		actual.save(self.artifacts.join(format!("{name}-actual.png")))?;
		let baseline = self.directory.join(format!("{name}.png"));
		if self.update {
			actual.save(baseline)?;
		} else if let Err(error) =
			compare(&baseline, actual, &self.artifacts, name)
		{
			self.failures.push(format!("{name}: {error:#}"));
		}
		Ok(())
	}

	pub fn finish(self) -> Result<()> {
		if !self.failures.is_empty() {
			bail!(
				"{}\nInspect {}",
				self.failures.join("\n"),
				self.artifacts.display()
			);
		}
		let mut files = BTreeSet::new();
		for entry in fs::read_dir(&self.directory)? {
			let path = entry?.path();
			if path.extension().is_some_and(|ext| ext == "png") {
				files.insert(
					path.file_stem().unwrap().to_string_lossy().into_owned(),
				);
			}
		}
		anyhow::ensure!(
			files == self.names,
			"Unused baselines: {:?}",
			files.difference(&self.names).collect::<Vec<_>>()
		);
		eprintln!("Verified {} rendering baselines", self.names.len());
		Ok(())
	}
}

pub fn capture(
	renderer: &mut markview_render::Renderer,
	snapshot: &markview_core::scene::LayoutSnapshot,
	view: &markview_render::View<'_>,
	overlay: &[Draw],
) -> Result<RgbaImage> {
	assert_glyphs(overlay)?;
	for block in &snapshot.blocks {
		assert_glyphs(&block.layout.draws)?;
	}
	let target = renderer.offscreen(view.width, view.height);
	let submission = renderer.render(
		snapshot,
		view,
		overlay,
		&target.create_view(&Default::default()),
	)?;
	renderer.wait(Some(submission))?;
	let pixels = renderer.read_pixels(&target)?;
	Ok(RgbaImage::from_raw(pixels.width, pixels.height, pixels.rgba).unwrap())
}

pub fn assert_glyphs(draws: &[Draw]) -> Result<()> {
	for draw in draws {
		match draw {
			Draw::Glyph(glyph) if glyph.id == 0 => {
				bail!("Missing glyph at ({}, {})", glyph.x, glyph.y)
			}
			Draw::Clipped { draws, .. } => assert_glyphs(draws)?,
			_ => {}
		}
	}
	Ok(())
}

pub fn compare(
	baseline: &Path,
	actual: &RgbaImage,
	artifacts: &Path,
	name: &str,
) -> Result<()> {
	let expected = image::open(baseline)
		.with_context(|| {
			format!("Missing or invalid baseline {}", baseline.display())
		})?
		.into_rgba8();
	if expected == *actual {
		return Ok(());
	}
	let (w, h) = (
		expected.width().max(actual.width()),
		expected.height().max(actual.height()),
	);
	let mut different = 0;
	let mut bounds = (w, h, 0, 0);
	let diff = RgbaImage::from_fn(w, h, |x, y| {
		let before = expected.get_pixel_checked(x, y);
		let after = actual.get_pixel_checked(x, y);
		// Allow two RGBA8 levels for cross-driver rounding.
		let matches = match (before, after) {
			(Some(before), Some(after)) => before
				.0
				.iter()
				.zip(after.0)
				.all(|(a, b)| a.abs_diff(b) <= CHANNEL_TOLERANCE),
			_ => before == after,
		};
		if matches {
			Rgba([0, 0, 0, 255])
		} else {
			different += 1;
			bounds = (
				bounds.0.min(x),
				bounds.1.min(y),
				bounds.2.max(x),
				bounds.3.max(y),
			);
			Rgba([255, 0, 255, 255])
		}
	});
	if different == 0 {
		return Ok(());
	}
	expected.save(artifacts.join(format!("{name}-expected.png")))?;
	diff.save(artifacts.join(format!("{name}-diff.png")))?;
	bail!(
		"{different} differing pixels, bounds {bounds:?}; expected {:?}, actual {:?}",
		expected.dimensions(),
		actual.dimensions()
	);
}
