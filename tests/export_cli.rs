//! Real editor-facing CLI jobs, checked independently of the exporter.
use std::{
	io::Write,
	path::Path,
	process::{Command, Output, Stdio},
};

fn font() -> String {
	Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("crates/markview-core/tests/fonts/NotoSerif-Regular-subset.otf")
		.display()
		.to_string()
}
fn run(args: &[&str], text: &str) -> Output {
	let mut child = Command::new(env!("CARGO_BIN_EXE_markview"))
		.args(["export"])
		.args(args)
		.args(["--stdin", "--font-file", &font()])
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.spawn()
		.unwrap();
	child
		.stdin
		.take()
		.unwrap()
		.write_all(text.as_bytes())
		.unwrap();
	child.wait_with_output().unwrap()
}
fn success(output: Output) -> serde_json::Value {
	assert!(
		output.status.success(),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
	let lines = String::from_utf8(output.stdout).unwrap();
	serde_json::from_str(lines.lines().last().unwrap()).unwrap()
}
fn image(dir: &Path, name: &str, color: [u8; 4]) {
	image::RgbaImage::from_pixel(64, 64, image::Rgba(color))
		.save(dir.join(name))
		.unwrap();
}
#[test]
fn stdin_pdf_keeps_all_text_pagination_and_relative_images() {
	let dir = tempfile::tempdir().unwrap();
	image(dir.path(), "local.png", [255, 0, 0, 255]);
	let source = format!(
		"# First snapshot\r\n\r\n![relative](local.png)\r\n\r\n{}\r\n# Last snapshot\r\n",
		"Unsaved paragraph.\r\n\r\n".repeat(130)
	);
	let output = dir.path().join("snapshot.pdf");
	let result = success(run(
		&[
			"--output",
			output.to_str().unwrap(),
			"--document-trust",
			"trusted",
			"--base-dir",
			dir.path().to_str().unwrap(),
		],
		&source,
	));
	assert!(result["pages"].as_u64().unwrap() > 1);
	let pdf = lopdf::Document::load(output).unwrap();
	let pages: Vec<_> = pdf.get_pages().keys().copied().collect();
	let text = pdf.extract_text(&pages).unwrap();
	assert!(
		text.contains("First snapshot") && text.contains("Last snapshot"),
		"{text}"
	);
	assert!(
		pdf.objects
			.values()
			.any(|object| object.as_stream().is_ok_and(|stream| stream
				.dict
				.get(b"Subtype")
				.is_ok_and(|kind| kind
					.as_name()
					.is_ok_and(|name| name == b"Image"))))
	);
}
#[test]
fn full_png_keeps_first_and_last_images_across_tiles() {
	let dir = tempfile::tempdir().unwrap();
	image(dir.path(), "first.png", [255, 0, 0, 255]);
	image(dir.path(), "last.png", [255, 0, 255, 255]);
	let source = format!(
		"![first](first.png)\n\n{}\n![last](last.png)\n",
		"Short paragraph.\n\n".repeat(900)
	);
	let output = dir.path().join("whole.png");
	success(run(
		&[
			"--format",
			"png",
			"--output",
			output.to_str().unwrap(),
			"--document-trust",
			"trusted",
			"--base-dir",
			dir.path().to_str().unwrap(),
			"--scale",
			"1",
		],
		&source,
	));
	let png = image::open(output).unwrap().to_rgba8();
	assert!(png.height() > 16_384, "{}", png.height());
	assert!(
		png.enumerate_pixels()
			.any(|(_, y, p)| y < 500 && p[0] > 220 && p[1] < 30 && p[2] < 30)
	);
	assert!(
		png.enumerate_pixels()
			.any(|(_, y, p)| y > png.height() - 500
				&& p[0] > 220
				&& p[1] < 30 && p[2] > 220)
	);
}
#[test]
fn buffer_exports_preserve_resource_trust_and_explicit_image_grants() {
	let dir = tempfile::tempdir().unwrap();
	image(dir.path(), "local.png", [255, 0, 255, 255]);
	let local = dir.path().join("local.png");
	for format in ["pdf", "png"] {
		let output = dir.path().join(format!("snapshot.{format}"));
		let args = [
			"--format",
			format,
			"--output",
			output.to_str().unwrap(),
			"--base-dir",
			dir.path().to_str().unwrap(),
		];
		std::fs::write(&output, b"previous result").unwrap();
		let blocked = run(&args, "![local](local.png)");
		if format == "pdf" {
			assert!(!blocked.status.success());
			assert_eq!(std::fs::read(&output).unwrap(), b"previous result");
		} else {
			success(blocked);
			assert!(
				!image::open(&output)
					.unwrap()
					.to_rgba8()
					.pixels()
					.any(|pixel| pixel.0 == [255, 0, 255, 255])
			);
		}
		let mut granted = args.to_vec();
		granted.extend(["--allow-local-image", local.to_str().unwrap()]);
		success(run(&granted, "![local](local.png)"));
		if format == "pdf" {
			let pdf = lopdf::Document::load(&output).unwrap();
			assert!(pdf.objects.values().any(|object| {
				object.as_stream().is_ok_and(|stream| {
					stream.dict.get(b"Subtype").is_ok_and(|kind| {
						kind.as_name().is_ok_and(|name| name == b"Image")
					})
				})
			}));
		} else {
			assert!(
				image::open(&output)
					.unwrap()
					.to_rgba8()
					.pixels()
					.any(|pixel| pixel.0 == [255, 0, 255, 255])
			);
		}
	}
}

#[test]
fn custom_templates_errors_and_cancellation_preserve_target() {
	let dir = tempfile::tempdir().unwrap();
	let style = dir.path().join("custom.mvss.toml");
	std::fs::write(&style, "format_version=2\nversion=1\ntargets=['pdf']\n[page]\nsize='a5'\n[mermaid]\nfont_family=['Diagram Literal']\n[[rule]]\nwhen=['code_block']\nfont=[{family='Code Literal'}]\n[[fontdef]]\nid='serif'\nlookfor=['Noto Serif']\n").unwrap();
	let output = dir.path().join("custom.pdf");
	success(run(
		&[
			"--output",
			output.to_str().unwrap(),
			"--style-file",
			style.to_str().unwrap(),
		],
		"# Custom template\n",
	));
	let pdf = lopdf::Document::load(&output).unwrap();
	let page = pdf
		.get_dictionary(*pdf.get_pages().values().next().unwrap())
		.unwrap();
	let media = page.get(b"MediaBox").unwrap().as_array().unwrap();
	assert!((media[2].as_float().unwrap() - 419.53).abs() < 2.0);
	std::fs::write(&output, b"previous result").unwrap();
	std::fs::write(&style, "invalid TOML = [").unwrap();
	assert!(
		!run(
			&[
				"--output",
				output.to_str().unwrap(),
				"--style-file",
				style.to_str().unwrap()
			],
			"# Changed\n"
		)
		.status
		.success()
	);
	assert_eq!(std::fs::read(&output).unwrap(), b"previous result");
	let marker = dir.path().join("cancel");
	std::fs::write(&marker, "").unwrap();
	assert!(
		!run(
			&[
				"--output",
				output.to_str().unwrap(),
				"--cancel-file",
				marker.to_str().unwrap()
			],
			"# Cancelled\n"
		)
		.status
		.success()
	);
	assert_eq!(std::fs::read(&output).unwrap(), b"previous result");
}
