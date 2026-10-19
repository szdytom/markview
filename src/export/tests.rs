use super::*;
use crate::lang::Lang;
use crate::settings::{MarkdownFlavor, ReaderSettings};
use markview_layout::style::CjkType;

#[test]
fn in_memory_exports_keep_permissions_bound_to_the_supplied_text() {
	use crate::security::{Resource, Security, Trust};
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("unsaved.md");
	let image = dir.path().join("picture.png");
	image::RgbaImage::from_pixel(3, 2, image::Rgba([12, 34, 56, 255]))
		.save(&image)
		.unwrap();
	let text = "# Unsaved snapshot\n\n![image](picture.png)";
	let mut granted = Security::local(Trust::Untrusted);
	granted.grant(Resource::File(std::fs::canonicalize(image).unwrap()));
	granted.bind(text);
	let mut stale = granted.clone();
	stale.bind("Previous content");
	let services = Arc::new(crate::services::Services::new(4));
	for (index, (security, expected_images)) in [
		(Security::local(Trust::Trusted), 1),
		(Security::local(Trust::Untrusted), 0),
		(granted, 1),
		(stale, 0),
	]
	.into_iter()
	.enumerate()
	{
		let snapshot = png_snapshot_text(
			&path,
			text,
			crate::test_support::options(),
			true,
			services.clone(),
			security.clone(),
		)
		.unwrap();
		assert_eq!(snapshot.images.decoded().len(), expected_images);
		let mut request = Config::from_settings(
			&ExportSettings::default(),
			crate::test_support::fonts(),
			CjkType::Sc,
			&[],
		)
		.prepare()
		.map(|prepared| {
			prepared.pdf_request(
				path.clone(),
				dir.path().join(format!("{index}.pdf")),
				true,
			)
		})
		.unwrap();
		request.security = security;
		let result = crate::pdf::export_text(
			&request,
			text,
			services.clone(),
			|| Ok(()),
		);
		if expected_images == 0 {
			assert!(
				result
					.unwrap_err()
					.to_string()
					.contains("Permission required")
			);
			assert!(!request.output.exists());
		} else {
			result.unwrap();
			let pdf = lopdf::Document::load(&request.output).unwrap();
			assert!(
				pdf.extract_text(&[1]).unwrap().contains("Unsaved snapshot")
			);
		}
	}
	assert!(!path.exists());
}

fn settings(format: ExportFormat) -> ExportSettings {
	ExportSettings {
		format,
		..Default::default()
	}
}

#[test]
fn paper_geometry_follows_the_export_settings() {
	let mut s = settings(ExportFormat::Pdf);
	let a4 = geometry(&s).unwrap();
	assert!((a4.width_pt - 210.0 * MM_TO_PT).abs() < 0.01);
	assert!((a4.margin_pt[0] - 22.0 * MM_TO_PT).abs() < 0.01);
	s.paper = "letter".into();
	s.landscape = true;
	let letter = geometry(&s).unwrap();
	assert!((letter.width_pt - 279.4 * MM_TO_PT).abs() < 0.01);
	s.paper = "tabloidish".into();
	assert!(geometry(&s).is_err());
}

#[test]
fn the_summary_reports_only_the_derived_measure() {
	let pdf = geometry_summary(&settings(ExportFormat::Pdf), Lang::En).unwrap();
	// 210 - 2×20 wide, 297 - 2×22 tall, and none of it repeats a row.
	assert_eq!(pdf, "text 170×253 mm");
	let png = geometry_summary(&settings(ExportFormat::Png), Lang::En).unwrap();
	assert!(png.ends_with("px wide"), "{png}");
	assert!(!png.contains("A4") && !png.contains("2×"), "{png}");
}

#[test]
fn an_export_layers_its_own_styles_on_the_print_sheet() {
	for (styles, succeeds) in [
		(vec!["print".into()], true),
		(vec!["missing".into()], false),
		(vec![], true),
	] {
		let result = Config {
			styles,
			options: crate::test_support::options(),
			..Default::default()
		}
		.prepare();
		assert_eq!(result.is_ok(), succeeds);
		if let Ok(prepared) = result {
			assert!(prepared.options.stylesheet.page().size.is_some());
		}
	}
}

#[test]
fn a_document_is_split_into_covering_tiles() {
	let geometry = geometry(&settings(ExportFormat::Png)).unwrap();
	let short = plan(&geometry, 1000.0, 1.0, 8192).unwrap();
	assert_eq!(short.tiles.len(), 1);
	assert_eq!(short.tiles[0].y_px, 0);
	assert!(short.tiles[0].scroll < 0.0);

	let tall = plan(&geometry, 40_000.0, 1.0, 8192).unwrap();
	assert!(tall.tiles.len() >= 5, "{tall:?}");
	// The strips tile the image exactly, in order, within the texture limit.
	let mut next = 0;
	for tile in &tall.tiles {
		assert_eq!(tile.y_px, next);
		assert!(tile.height_px <= 8192);
		next += tile.height_px;
	}
	assert_eq!(next, tall.height_px);
}

#[test]
fn a_document_past_the_pixel_cap_is_refused() {
	let geometry = geometry(&settings(ExportFormat::Png)).unwrap();
	let error = plan(&geometry, 10_000_000.0, 2.0, 8192)
		.unwrap_err()
		.to_string();
	assert!(error.contains("PDF"), "{error}");
}

#[test]
fn a_panel_export_preserves_its_settings_in_the_pdf_request() {
	let mut s = settings(ExportFormat::Pdf);
	s.paper = "letter".into();
	s.landscape = true;
	s.font_size = 20.0;
	s.paragraph_indent = 2.0;
	let args = Config::from_settings(
		&s,
		crate::test_support::fonts(),
		CjkType::Sc,
		&[],
	)
	.prepare()
	.map(|prepared| {
		prepared.pdf_request("doc.md".into(), "doc.pdf".into(), false)
	})
	.unwrap();
	assert_eq!(
		args.options.stylesheet.page().size.as_deref(),
		Some("letter")
	);
	assert_eq!(args.options.stylesheet.page().landscape, Some(true));
	assert_eq!(
		args.options.stylesheet.page().margin,
		Some(vec![22.0, 20.0, 22.0, 20.0])
	);
	assert_eq!(args.options.font_size, 20.0);
	assert_eq!(args.options.paragraph_indent, 2.0);
	assert!(args.options.codeblock_wrap);
	assert!(args.links);

	let dir = tempfile::tempdir().unwrap();
	let style = dir.path().join("landscape.mvss.toml");
	std::fs::write(&style, "format_version=2\nversion=1\ntargets=['pdf']\n[page]\nsize='a5'\nlandscape=true\n").unwrap();
	s.landscape = false;
	let mut config = Config::from_settings(
		&s,
		crate::test_support::fonts(),
		CjkType::Sc,
		&[],
	);
	config.style_file = Some(style);
	let prepared = config.prepare().unwrap();
	assert!(prepared.geometry.width_pt < prepared.geometry.height_pt);
	assert_eq!(
		prepared.options.stylesheet.page().size.as_deref(),
		Some("letter")
	);
	assert_eq!(prepared.options.stylesheet.page().landscape, Some(false));
}

#[test]
fn a_write_replaces_the_destination_atomically() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("out.pdf");
	write_atomic(&path, b"first").unwrap();
	write_atomic(&path, b"second").unwrap();
	assert_eq!(std::fs::read(&path).unwrap(), b"second");
	assert!(
		std::fs::read_dir(dir.path())
			.unwrap()
			.all(|entry| entry.unwrap().file_name() == "out.pdf")
	);
}

#[test]
fn a_planted_temporary_symlink_is_never_written_through() {
	if !markview_platform::test_support::special_files() {
		return;
	}
	use markview_platform::test_support::symlink;
	let dir = tempfile::tempdir().unwrap();
	let victim = dir.path().join("victim");
	std::fs::write(&victim, b"safe").unwrap();
	// The name the helper used before it took a random one, so the planting
	// attacker's best guess is covered rather than a name this test chose.
	let planted = dir
		.path()
		.join(format!(".out.pdf.{}.tmp", std::process::id()));
	symlink(&victim, &planted).unwrap();
	// A plain write in the same directory is the mode an export must match.
	let plain = dir.path().join("plain");
	std::fs::write(&plain, b"plain").unwrap();

	write_atomic(&dir.path().join("out.pdf"), b"exported").unwrap();

	assert_eq!(std::fs::read(&victim).unwrap(), b"safe");
	assert_eq!(
		std::fs::read(dir.path().join("out.pdf")).unwrap(),
		b"exported"
	);
	assert_eq!(
		markview_platform::test_support::permission_mode(
			std::fs::metadata(dir.path().join("out.pdf"))
				.unwrap()
				.permissions()
		),
		markview_platform::test_support::permission_mode(
			std::fs::metadata(&plain).unwrap().permissions()
		)
	);
}

#[test]
fn a_symlinked_destination_is_replaced_rather_than_followed() {
	if !markview_platform::test_support::special_files() {
		return;
	}
	use markview_platform::test_support::symlink;
	let dir = tempfile::tempdir().unwrap();
	let victim = dir.path().join("victim");
	std::fs::write(&victim, b"safe").unwrap();
	let destination = dir.path().join("out.pdf");
	symlink(&victim, &destination).unwrap();

	write_atomic(&destination, b"exported").unwrap();

	assert_eq!(std::fs::read(&victim).unwrap(), b"safe");
	assert!(
		!std::fs::symlink_metadata(&destination)
			.unwrap()
			.is_symlink()
	);
	assert_eq!(std::fs::read(&destination).unwrap(), b"exported");
}

#[test]
fn reader_dialects_reach_the_exported_layout() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("dialect.md");
	// `[!example]` is no GitHub alert type, so only OFM lite reads it as a
	// callout; GFM keeps the marker literal. The `+` fold sign makes the
	// callout header a disclosure summary, which is reading text.
	std::fs::write(
		&path,
		"Soft wrapped\nline one.\n\n> [!example]+ Callout title\n> ==marked== body\n\nVisible %%SECRET%% text.\n\n`%%CODEKEEP%%`\n",
	)
	.unwrap();
	let gfm_breaks = ReaderSettings {
		hard_breaks: true,
		highlight_marks: true,
		..Default::default()
	};
	// The stored toggles are stale under OFM lite: the preset forces hard
	// breaks off and highlight marks on.
	let stale_ofm = ReaderSettings {
		markdown_flavor: MarkdownFlavor::OfmLite,
		hard_breaks: true,
		highlight_marks: false,
		..Default::default()
	};
	for (reader, join, highlight, callout) in [
		(&ReaderSettings::default(), " ", false, false),
		(&gfm_breaks, "\n", true, false),
		(&stale_ofm, " ", true, true),
	] {
		let options = Config::from_settings(
			&settings(ExportFormat::Png),
			crate::test_support::fonts(),
			CjkType::Sc,
			&[],
		)
		.with_dialect(reader.effective_dialect())
		.prepare()
		.unwrap()
		.options;
		let snapshot = png_snapshot(&path, options, true).unwrap();
		let text = snapshot.full_reading_text();
		assert!(
			text.contains(&format!("Soft wrapped{join}line one.")),
			"{text:?}"
		);
		assert_eq!(text.contains("==marked=="), !highlight, "{text:?}");
		assert_eq!(text.contains("marked body"), highlight, "{text:?}");
		assert_eq!(!text.contains("[!example]"), callout, "{text:?}");
		assert!(text.contains("Callout title"), "{text:?}");
		assert_eq!(text.contains("SECRET"), !callout, "{text}");
		assert!(text.contains("Visible") && text.contains("%%CODEKEEP%%"));
	}
}

#[test]
fn a_mapped_export_writes_a_real_pdf() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("doc.md");
	let output = dir.path().join("doc.pdf");
	std::fs::write(&path, "# Title\n\nA paragraph.\n").unwrap();
	let args = Config::from_settings(
		&ExportSettings::default(),
		crate::test_support::fonts(),
		CjkType::Sc,
		&[],
	)
	.prepare()
	.map(|prepared| prepared.pdf_request(path.clone(), output.clone(), false))
	.unwrap();
	let stats = crate::pdf::export_once(&args).unwrap();
	assert_eq!(stats.pages, 1);
	assert!(stats.bytes > 0);
	assert!(std::fs::read(&output).unwrap().starts_with(b"%PDF"));
}

#[test]
fn a_page_wider_than_the_gpu_limit_is_refused() {
	let mut s = settings(ExportFormat::Png);
	s.paper = "1000x200".into();
	s.scale = 4.0;
	let geometry = geometry(&s).unwrap();
	let error = plan(&geometry, 1000.0, s.scale, 8192)
		.unwrap_err()
		.to_string();
	assert!(error.contains("wide"), "{error}");
}

#[test]
fn an_export_shows_every_details_body() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("doc.md");
	std::fs::write(
		&path,
		"<details>\n<summary>More</summary>\n\nHidden body.\n\n</details>\n",
	)
	.unwrap();
	let options = Config::from_settings(
		&settings(ExportFormat::Png),
		crate::test_support::fonts(),
		CjkType::Sc,
		&[],
	)
	.prepare()
	.unwrap()
	.options;
	assert!(options.force_open);
	let snapshot = png_snapshot(&path, options, true).unwrap();
	let text = snapshot
		.select_all(1)
		.map(|selection| snapshot.extract_text(selection, 1))
		.unwrap_or_default();
	assert!(text.contains("Hidden body"), "{text}");
	// A printed page has no pointer, so no summary hit region is drawn.
	assert!(
		snapshot
			.blocks
			.iter()
			.all(|block| block.links().next().is_none())
	);
}

#[test]
fn a_pdf_shows_every_details_body() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("doc.md");
	let output = dir.path().join("doc.pdf");
	let mut source = String::from("<details>\n<summary>Long</summary>\n\n");
	for i in 0..120 {
		source.push_str(&format!("Paragraph {i} of the collapsed body.\n\n"));
	}
	source.push_str("</details>\n");
	std::fs::write(&path, source).unwrap();
	let args = Config::from_settings(
		&ExportSettings::default(),
		crate::test_support::fonts(),
		CjkType::Sc,
		&[],
	)
	.prepare()
	.map(|prepared| prepared.pdf_request(path.clone(), output.clone(), false))
	.unwrap();
	assert!(args.options.force_open);
	let stats = crate::pdf::export_once(&args).unwrap();
	// A collapsed body would need exactly one page.
	assert!(stats.pages > 1, "{} pages", stats.pages);
	assert!(std::fs::read(&output).unwrap().starts_with(b"%PDF"));
}
