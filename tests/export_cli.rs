//! Real editor-facing CLI jobs, checked independently of the exporter.
use std::{
	io::Write,
	path::Path,
	process::{Command, Output, Stdio},
};

fn font() -> String {
	Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("crates/markview-layout/tests/fonts/NotoSerif-Regular-subset.otf")
		.display()
		.to_string()
}
fn run(args: &[&str], text: &str) -> Output {
	let mut command = Command::new(env!("CARGO_BIN_EXE_markview"));
	command.args(["export"]).args(args).args([
		"--stdin",
		"--font-file",
		&font(),
	]);
	invoke(command, text)
}
fn invoke(mut command: Command, text: &str) -> Output {
	let mut child = command
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
	let events: Vec<serde_json::Value> = lines
		.lines()
		.map(|line| serde_json::from_str(line).unwrap())
		.collect();
	assert_eq!(events.first().unwrap()["type"], "progress");
	assert_eq!(events.last().unwrap()["type"], "done");
	assert!(
		events
			.windows(2)
			.all(|pair| pair[0]["fraction"].as_f64().unwrap()
				<= pair[1]["fraction"].as_f64().unwrap())
	);
	events.last().unwrap().clone()
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
	let outlines = pdf
		.catalog()
		.unwrap()
		.get(b"Outlines")
		.unwrap()
		.as_reference()
		.unwrap();
	let root = pdf.get_dictionary(outlines).unwrap();
	let first = pdf
		.get_dictionary(root.get(b"First").unwrap().as_reference().unwrap())
		.unwrap();
	assert_eq!(
		lopdf::decode_text_string(first.get(b"Title").unwrap()).unwrap(),
		"First snapshot"
	);
	let last = pdf
		.get_dictionary(root.get(b"Last").unwrap().as_reference().unwrap())
		.unwrap();
	assert_eq!(
		lopdf::decode_text_string(last.get(b"Title").unwrap()).unwrap(),
		"Last snapshot"
	);
	let destination = pdf
		.dereference(last.get(b"Dest").unwrap())
		.unwrap()
		.1
		.as_array()
		.unwrap();
	assert_ne!(destination[0].as_reference().unwrap(), pdf.get_pages()[&1]);
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
				&& p[1] < 30
				&& p[2] > 220)
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
	std::fs::write(&style, "format_version=3\nversion=1\ntargets=['pdf']\n[page]\nsize='a5'\n[mermaid]\nfont_family=['Diagram Literal']\n[[rule]]\nwhen=['code_block']\nfont=[{family='Code Literal'}]\n[[fontdef]]\nid='serif'\nlookfor=['Noto Serif']\n").unwrap();
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

#[test]
fn invalid_explicit_fonts_fail_before_replacing_any_export() {
	let dir = tempfile::tempdir().unwrap();
	let bad = dir.path().join("error-page.ttf");
	std::fs::write(&bad, b"<html>Not a font</html>").unwrap();
	let source = dir.path().join("source.md");
	std::fs::write(&source, "# Snapshot").unwrap();
	for (command, format) in
		[("pdf", "pdf"), ("export", "pdf"), ("export", "png")]
	{
		let output = dir.path().join(format!("{command}.{format}"));
		std::fs::write(&output, b"previous result").unwrap();
		let mut process = Command::new(env!("CARGO_BIN_EXE_markview"));
		process.arg(command);
		if command == "pdf" {
			process.arg(&source);
		} else {
			process.args(["--stdin", "--format", format]);
		}
		process
			.arg("--output")
			.arg(&output)
			.arg("--font-file")
			.arg(&bad)
			.args(["--font-file", &font()]);
		let result = invoke(process, "# Snapshot");
		assert!(!result.status.success());
		assert!(
			String::from_utf8_lossy(&result.stderr)
				.contains("Invalid font file")
		);
		assert!(!String::from_utf8_lossy(&result.stdout).contains("\"done\""));
		assert_eq!(std::fs::read(&output).unwrap(), b"previous result");
	}
}

#[test]
fn file_and_buffer_pdf_exports_share_templates_fonts_and_reading_options() {
	let dir = tempfile::tempdir().unwrap();
	let source = dir.path().join("source.md");
	let text = "# Snapshot\n\nBody paragraph.\n\n中文\n";
	std::fs::write(&source, text).unwrap();
	let style = dir.path().join("custom.mvss.toml");
	std::fs::write(&style, "format_version=3\nversion=1\ntargets=['pdf']\n[page]\nsize='a5'\nlandscape=true\n[[rule]]\nwhen=['p']\nmedia=['landscape','pdf']\nline_height=1.8\n").unwrap();
	let cjk = Path::new(env!("CARGO_MANIFEST_DIR")).join(
		"crates/markview-render/tests/fonts/NotoSerifCJKtc-Regular-subset.otf",
	);
	let faces = Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("crates/markview-layout/tests/fonts");
	let sans = faces.join("NotoSans-Regular-subset.otf");
	let bold = faces.join("NotoSans-Bold-subset.otf");
	let common = [
		"--style",
		"monochrome",
		"--style-file",
		style.to_str().unwrap(),
		"--font-file",
		&font(),
		"--font-file",
		cjk.to_str().unwrap(),
		"--font-file",
		sans.to_str().unwrap(),
		"--font-file",
		bold.to_str().unwrap(),
		"--font-size",
		"72",
		"--cjk-type",
		"TC",
		"--paragraph-indent",
		"2",
		"--left",
		"--no-hyphens",
	];
	let file_pdf = dir.path().join("file.pdf");
	let mut file_command = Command::new(env!("CARGO_BIN_EXE_markview"));
	file_command
		.arg("pdf")
		.arg(&source)
		.arg("--output")
		.arg(&file_pdf)
		.args(common);
	let result = invoke(file_command, "");
	assert!(
		result.status.success(),
		"{}",
		String::from_utf8_lossy(&result.stderr)
	);
	let buffer_pdf = dir.path().join("buffer.pdf");
	let mut buffer_command = Command::new(env!("CARGO_BIN_EXE_markview"));
	buffer_command
		.args(["export", "--stdin", "--output"])
		.arg(&buffer_pdf)
		.args(common);
	success(invoke(buffer_command, text));
	assert_eq!(
		std::fs::read(&file_pdf).unwrap(),
		std::fs::read(&buffer_pdf).unwrap()
	);
	let pdf = lopdf::Document::load(&buffer_pdf).unwrap();
	assert!(pdf.extract_text(&[1]).unwrap().contains("Snapshot"));
	let page = pdf
		.get_dictionary(*pdf.get_pages().values().next().unwrap())
		.unwrap();
	let media = page.get(b"MediaBox").unwrap().as_array().unwrap();
	assert!((media[2].as_float().unwrap() - 595.28).abs() < 2.0);
	assert!((media[3].as_float().unwrap() - 419.53).abs() < 2.0);
}

#[test]
fn file_and_buffer_exports_use_downloaded_fonts_unless_pinned() {
	if !markview_platform::test_support::isolated_fontconfig() {
		return;
	}
	let dir = tempfile::tempdir().unwrap();
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	let system = dir.path().join("system");
	std::fs::create_dir(&system).unwrap();
	std::fs::copy(
		root.join(
			"crates/markview-layout/tests/fonts/NotoSans-Regular-subset.otf",
		),
		system.join("sans.otf"),
	)
	.unwrap();
	let config = dir.path().join("config");
	let personal = config.join("markview/fonts");
	std::fs::create_dir_all(&personal).unwrap();
	std::fs::copy(font(), personal.join("serif.otf")).unwrap();
	let fontconfig = dir.path().join("fonts.conf");
	std::fs::write(
		&fontconfig,
		format!("<fontconfig><dir>{}</dir></fontconfig>", system.display()),
	)
	.unwrap();
	let source = dir.path().join("source.md");
	let text = "# Snapshot\n\nBody paragraph.\n";
	std::fs::write(&source, text).unwrap();
	let mut baseline = None;
	for (index, command) in ["pdf", "export", "export", "export"]
		.into_iter()
		.enumerate()
	{
		let output = dir.path().join(format!("{index}.pdf"));
		let mut process = Command::new(env!("CARGO_BIN_EXE_markview"));
		process
			.env("XDG_CONFIG_HOME", &config)
			.env("FONTCONFIG_FILE", &fontconfig)
			.arg(command);
		if command == "pdf" {
			process.arg(&source);
		} else {
			process.arg("--stdin");
		}
		process.arg("--output").arg(&output);
		if index == 2 {
			process
				.arg("--ignore-system-fonts")
				.arg("--fonts")
				.arg(&system);
		} else if index == 3 {
			process.arg("--font-file").arg(system.join("sans.otf"));
		}
		let result = invoke(process, text);
		assert!(
			result.status.success(),
			"{}",
			String::from_utf8_lossy(&result.stderr)
		);
		let pdf = lopdf::Document::load(&output).unwrap();
		assert!(pdf.extract_text(&[1]).unwrap().contains("Body paragraph"));
		let has_serif = pdf
			.objects
			.values()
			.filter_map(|object| object.as_dict().ok())
			.filter_map(|dict| dict.get(b"BaseFont").ok())
			.filter_map(|name| name.as_name().ok())
			.any(|name| {
				String::from_utf8_lossy(name).contains("NotoSerif-Regular")
			});
		assert_eq!(has_serif, index < 2);
		if index == 0 {
			baseline = Some(std::fs::read(&output).unwrap());
		} else if index == 1 {
			assert_eq!(
				std::fs::read(&output).unwrap(),
				baseline.take().unwrap()
			);
		}
	}
}

#[test]
fn buffer_dialects_share_pdf_and_png_parsing_without_reader_settings() {
	let dir = tempfile::tempdir().unwrap();
	let source = "# Dialect\n\nVisible %%SECRET%% text and ==marked==.\n\n```text\n%%CODE%%\n```\n\n> [!warning] Title\n> Body.\n";
	for (flavor, secret, marks) in
		[("gfm", true, true), ("obsidian", false, false)]
	{
		let output = dir.path().join(format!("{flavor}.pdf"));
		success(run(
			&[
				"--output",
				output.to_str().unwrap(),
				"--markdown-flavor",
				flavor,
			],
			source,
		));
		let pdf = lopdf::Document::load(output).unwrap();
		let pages = pdf.get_pages().keys().copied().collect::<Vec<_>>();
		let text = pdf.extract_text(&pages).unwrap();
		assert_eq!(text.contains("SECRET"), secret, "{flavor}: {text}");
		assert_eq!(text.contains("==marked=="), marks, "{flavor}: {text}");
		assert!(text.contains("%%CODE%%"), "{text}");
	}
	let marked = dir.path().join("highlight.pdf");
	success(run(
		&["--output", marked.to_str().unwrap(), "--highlight-marks"],
		"==marked==",
	));
	let pdf = lopdf::Document::load(marked).unwrap();
	let text = pdf.extract_text(&[1]).unwrap();
	assert!(text.contains("marked") && !text.contains("=="), "{text}");
	for flavor in ["gfm", "obsidian"] {
		let mut heights = Vec::new();
		for hard in [false, true] {
			let output = dir.path().join(format!("{flavor}-{hard}.png"));
			let mut args = vec![
				"--format",
				"png",
				"--output",
				output.to_str().unwrap(),
				"--markdown-flavor",
				flavor,
			];
			if hard {
				args.push("--hard-breaks");
			}
			let done = success(run(&args, "alpha\nbeta\ngamma\n"));
			heights.push(done["height"].as_u64().unwrap());
		}
		assert!(heights[1] > heights[0], "{flavor}: {heights:?}");
	}
	let rejected = run(
		&[
			"--markdown-flavor",
			"unknown",
			"--output",
			dir.path().join("bad.pdf").to_str().unwrap(),
		],
		"body",
	);
	assert!(!rejected.status.success());
}

#[test]
fn grid_tables_default_on_and_can_be_disabled_in_both_export_presets() {
	let dir = tempfile::tempdir().unwrap();
	for flavor in ["gfm", "obsidian"] {
		for enabled in [true, false] {
			let output =
				dir.path().join(format!("grid-{flavor}-{enabled}.pdf"));
			let mut args = vec![
				"--output",
				output.to_str().unwrap(),
				"--markdown-flavor",
				flavor,
			];
			if !enabled {
				args.extend(["--grid-tables", "false"]);
			}
			success(run(&args, "+-------+\n| alpha |\n+-------+\n"));
			let pdf = lopdf::Document::load(output).unwrap();
			let text = pdf.extract_text(&[1]).unwrap();
			assert!(text.contains("alpha"), "{text}");
			assert_eq!(text.contains("+-------+"), !enabled, "{text}");
		}
	}
}
