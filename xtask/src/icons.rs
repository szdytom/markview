//! Derives committed platform icon assets from the source SVG.
//!
//! The macOS `.icns` container is written directly: `ic07`–`ic10` elements
//! carry a plain PNG payload, so no extra container crate is needed.
use anyhow::{Context, Result, bail};
use resvg::tiny_skia;
use resvg::usvg;
use std::path::{Path, PathBuf};

/// One committed PNG asset and its square size in pixels.
struct Asset {
	path: &'static str,
	size: u32,
}

const PNG_ASSETS: &[Asset] = &[
	Asset {
		path: "assets/icons/markview-16.png",
		size: 16,
	},
	Asset {
		path: "assets/icons/markview-32.png",
		size: 32,
	},
	Asset {
		path: "assets/icons/markview-48.png",
		size: 48,
	},
	Asset {
		path: "assets/icons/markview-64.png",
		size: 64,
	},
	Asset {
		path: "assets/icons/markview-128.png",
		size: 128,
	},
	Asset {
		path: "assets/icons/markview-256.png",
		size: 256,
	},
	Asset {
		path: "assets/icons/markview-512.png",
		size: 512,
	},
	Asset {
		path: "assets/icons/markview-1024.png",
		size: 1024,
	},
];

/// Sizes Windows uses in the executable icon.
const ICO_SIZES: &[u32] = &[16, 32, 48, 64, 128, 256];

/// ICNS element type and the size its payload must be.
const ICNS_ELEMENTS: &[(&[u8; 4], u32)] = &[
	(b"ic07", 128),
	(b"ic08", 256),
	(b"ic09", 512),
	(b"ic10", 1024),
];

const SOURCE: &str = "assets/markview-icon-color.svg";

pub fn run(check: bool) -> Result<()> {
	let root = workspace_root()?;
	let svg = std::fs::read(root.join(SOURCE))
		.with_context(|| format!("cannot read {SOURCE}"))?;
	let options = usvg::Options::default();
	let tree = usvg::Tree::from_data(&svg, &options)
		.with_context(|| format!("cannot parse {SOURCE}"))?;

	let mut produced: Vec<(PathBuf, Vec<u8>)> = Vec::new();
	for asset in PNG_ASSETS {
		produced.push((
			root.join(asset.path),
			encode_png(&render(&tree, asset.size)?)?,
		));
	}
	produced.push((root.join("assets/icons/markview.ico"), encode_ico(&tree)?));
	produced
		.push((root.join("assets/icons/markview.icns"), encode_icns(&tree)?));

	if check {
		let stale: Vec<_> = produced
			.iter()
			.filter(|(path, expected)| {
				std::fs::read(path).ok().as_deref() != Some(expected.as_slice())
			})
			.map(|(path, _)| path.clone())
			.collect();
		if !stale.is_empty() {
			for path in &stale {
				eprintln!("stale icon asset: {}", path.display());
			}
			bail!("icon assets are stale; run `cargo run -p xtask -- icons`");
		}
		println!("icon assets are up to date");
		return Ok(());
	}

	for (path, bytes) in &produced {
		if let Some(parent) = path.parent() {
			std::fs::create_dir_all(parent)?;
		}
		std::fs::write(path, bytes)?;
		println!("wrote {}", path.display());
	}
	Ok(())
}

fn workspace_root() -> Result<PathBuf> {
	let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
	manifest
		.parent()
		.map(Path::to_path_buf)
		.context("xtask must live inside the workspace")
}

/// Rasterizes the tree at a square `size`, preserving the source aspect ratio.
fn render(tree: &usvg::Tree, size: u32) -> Result<tiny_skia::Pixmap> {
	let source = tree.size();
	let scale = size as f32 / source.width().max(source.height());
	let width = (source.width() * scale).round().max(1.0) as u32;
	let height = (source.height() * scale).round().max(1.0) as u32;
	let mut pixmap = tiny_skia::Pixmap::new(width, height)
		.context("cannot allocate an icon pixmap")?;
	resvg::render(
		tree,
		tiny_skia::Transform::from_scale(scale, scale),
		&mut pixmap.as_mut(),
	);
	Ok(pixmap)
}

fn encode_png(pixmap: &tiny_skia::Pixmap) -> Result<Vec<u8>> {
	pixmap.encode_png().context("cannot encode an icon PNG")
}

/// Packs the sizes Windows uses, with 256 as a PNG frame.
fn encode_ico(tree: &usvg::Tree) -> Result<Vec<u8>> {
	let mut dir = ico::IconDir::new(ico::ResourceType::Icon);
	for &size in ICO_SIZES {
		let frame = render(tree, size)?;
		let image =
			ico::IconImage::from_rgba_data(size, size, frame.data().to_vec());
		dir.add_entry(ico::IconDirEntry::encode_as_png(&image)?);
	}
	let mut out = std::io::Cursor::new(Vec::new());
	dir.write(&mut out)?;
	Ok(out.into_inner())
}

/// Writes an ICNS container whose elements are plain PNG payloads.
fn encode_icns(tree: &usvg::Tree) -> Result<Vec<u8>> {
	let mut body = Vec::new();
	for (kind, size) in ICNS_ELEMENTS {
		let png = encode_png(&render(tree, *size)?)?;
		let length = u32::try_from(png.len() + 8)?;
		body.extend_from_slice(*kind);
		body.extend_from_slice(&length.to_be_bytes());
		body.extend_from_slice(&png);
	}
	let total = u32::try_from(body.len() + 8)?;
	let mut out = Vec::with_capacity(body.len() + 8);
	out.extend_from_slice(b"icns");
	out.extend_from_slice(&total.to_be_bytes());
	out.extend_from_slice(&body);
	Ok(out)
}
