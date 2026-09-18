//! End-to-end export tests: the bytes a viewer would open, checked with a PDF
//! reader rather than with the writer's own bookkeeping.
use markview_core::{
	document,
	fonts::FontConfig,
	image::ImageSnapshot,
	layout::{LayoutEngine, LayoutOptions},
	paginate::{PT_PER_PX, PageGeometry, paginate},
	style::{CjkType, SYNTHETIC_ITALIC_ANGLE_DEG, Stylesheet},
};
use markview_pdf::{Export, Metadata};
use std::sync::Arc;

struct Exported {
	bytes: Vec<u8>,
	pages: usize,
	pdf: lopdf::Document,
	geometry: PageGeometry,
	anchors: std::collections::HashMap<String, (usize, f32)>,
}

/// The committed subset faces, so an export is the same on every platform.
fn fonts() -> FontConfig {
	FontConfig {
		ignore_system_fonts: true,
		directories: vec![
			std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
				.join("../markview-core/tests/fonts"),
		],
	}
}

fn export(source: &str, sheet: Arc<Stylesheet>, links: bool) -> Exported {
	export_with(
		source,
		sheet,
		links,
		Metadata {
			title: Some("Test".into()),
			..Default::default()
		},
	)
}

fn export_with(
	source: &str,
	sheet: Arc<Stylesheet>,
	links: bool,
	metadata: Metadata,
) -> Exported {
	let geometry = PageGeometry::from_style(sheet.page()).unwrap();
	export_at(source, sheet, links, metadata, geometry)
}

/// Exports onto a caller-chosen page, so a test can force a break.
fn export_at(
	source: &str,
	sheet: Arc<Stylesheet>,
	links: bool,
	metadata: Metadata,
	geometry: PageGeometry,
) -> Exported {
	let document = document::parse(source.to_owned());
	let options = LayoutOptions {
		width: geometry.text_px().0,
		codeblock_wrap: true,
		stylesheet: sheet.clone(),
		fonts: fonts(),
		..Default::default()
	};
	// Mirrors the export path: the highlighting pass is asynchronous, so wait
	// for it and lay out again, or the code would be drawn uncolored.
	let mut engine = LayoutEngine::new();
	let mut snapshot = engine.layout(&document, &options);
	if engine.wait_highlights() {
		snapshot = engine.layout(&document, &options);
	}
	let pagination = paginate(&document, &snapshot, &geometry);
	let bytes = markview_pdf::export(Export {
		snapshot: &snapshot,
		images: &ImageSnapshot::default(),
		stylesheet: &sheet,
		geometry: &geometry,
		pagination: &pagination,
		metadata,
		path: "test.md".into(),
		body_size_px: options.font_size,
		links,
		fonts: fonts(),
	})
	.unwrap();
	let pdf = lopdf::Document::load_mem(&bytes).expect("the export parses");
	Exported {
		bytes,
		pages: pagination.pages.len(),
		pdf,
		geometry,
		anchors: pagination.anchors,
	}
}

fn print() -> Arc<Stylesheet> {
	let mut sheet = (*Stylesheet::bundled_print()).clone();
	// The pinned subsets name their Han faces under the `SC` definitions, so
	// the tests select the convention those faces carry.
	sheet.set_cjk_type(CjkType::Sc);
	Arc::new(sheet)
}

/// A body of prose long enough to fill more than one A4 page.
fn long_source() -> String {
	let mut source = String::from("# A heading\n\n");
	for index in 0..40 {
		source.push_str(&format!(
			"Paragraph {index} carries enough words to wrap across several lines of the \
			 reading measure, so the page builder has real lines to distribute.\n\n"
		));
	}
	source
}

#[test]
fn pages_carry_selectable_text_and_embedded_subset_fonts() {
	let exported = export(&long_source(), print(), true);
	assert!(exported.pages > 1, "{}", exported.pages);
	assert_eq!(exported.pdf.get_pages().len(), exported.pages);
	let text = exported.pdf.extract_text(&[1]).unwrap();
	assert!(
		text.contains("A heading") && text.contains("Paragraph 0"),
		"{text}"
	);
	// Every page names its characters and embeds its faces.
	let first = exported.pdf.get_pages()[&1];
	let fonts = exported.pdf.get_page_fonts(first).unwrap();
	assert!(!fonts.is_empty(), "the first page embeds a font");
	for font in fonts.values() {
		assert!(font.has(b"ToUnicode"), "a font without a character map");
	}
	let raw = String::from_utf8_lossy(&exported.bytes);
	assert!(raw.contains("/FontFile2") || raw.contains("/FontFile3"));
	// The whole text of the document survives, page by page.
	let all: String = (1..=exported.pages as u32)
		.map(|page| exported.pdf.extract_text(&[page]).unwrap())
		.collect();
	for marker in ["Paragraph 0", "Paragraph 39", "A heading"] {
		assert!(all.contains(marker), "missing {marker}");
	}
}

#[test]
fn a_bullet_list_keeps_its_items_and_drops_the_bullet() {
	// A bullet is a filled path, not a character, so the page carries the item
	// text without a marker glyph. The pinned serif substitutes an `fi`
	// ligature, so the item also guards that a ligature keeps every character
	// in its map.
	let exported = export("- first item\n- second item\n", print(), false);
	let text = exported.pdf.extract_text(&[1]).unwrap();
	assert!(
		text.contains("first item") && text.contains("second item"),
		"{text}"
	);
	assert!(!text.contains('\u{2022}'), "{text}");
	assert!(!text.contains('\u{fffd}'), "{text}");
}

#[test]
fn an_ordered_list_embeds_its_numbering_format() {
	// A number stays text, so the page carries what the theme's pattern
	// spells, not a fixed "1.".
	let mut sheet = (*print()).clone();
	sheet.merge(
		&Stylesheet::parse(
			"format_version=2\nversion=1\n[[rule]]\nwhen=['enum']\nnumbering='a)'",
		)
		.unwrap(),
	);
	let exported =
		export("1. first item\n2. second item\n", Arc::new(sheet), false);
	let text = exported.pdf.extract_text(&[1]).unwrap();
	assert!(text.contains("a)"), "{text}");
	assert!(text.contains("b)"), "{text}");
	// Every marker glyph keeps its own character map entry, so the number
	// copies as its own word instead of trailing an unmapped glyph.
	assert!(text.contains("a) "), "{text}");
	assert!(!text.contains('\u{fffd}'), "{text}");
	assert!(
		text.contains("first item") && text.contains("second item"),
		"{text}"
	);
}

#[test]
fn links_become_annotations_and_headings_become_destinations() {
	let source = "# Top\n\n[web](https://example.com/) and [jump](#later).\n\n\
	              ## Later\n\nBack to [top](#top).\n";
	let exported = export(source, print(), true);
	let first = exported.pdf.get_pages()[&1];
	let annotations = exported.pdf.get_page_annotations(first).unwrap();
	assert_eq!(annotations.len(), 3, "{annotations:?}");
	assert!(
		annotations.iter().any(|a| a.has(b"Dest") && !a.has(b"A")),
		"a fragment link points at a destination in this document"
	);
	assert!(
		annotations.iter().any(|a| a.has(b"A")),
		"a web link carries a URI action"
	);
	let raw = String::from_utf8_lossy(&exported.bytes);
	assert!(raw.contains("/URI") && raw.contains("https://example.com/"));
	assert!(raw.contains("/Dest"), "the fragment link is a destination");

	// Without links the pages stay bare.
	let bare = export(source, print(), false);
	let bare_page = bare.pdf.get_pages()[&1];
	assert!(bare.pdf.get_page_annotations(bare_page).unwrap().is_empty());
}

#[test]
fn the_same_document_exports_identical_bytes() {
	let source = long_source();
	let first = export(&source, print(), true);
	let second = export(&source, print(), true);
	assert_eq!(first.bytes, second.bytes);
}

#[test]
fn page_furniture_is_drawn_and_named() {
	let mut sheet = (*print()).clone();
	sheet.page.footer_center = Some("{page}/{pages}".into());
	sheet.page.header_left = Some("Draft".into());
	let source = format!("{}\n\n{}", "one ".repeat(900), "two ".repeat(900));
	let exported = export(&source, Arc::new(sheet), true);
	assert!(exported.pages > 1);
	for page in 1..=exported.pages as u32 {
		// The extractor writes each text fragment on its own line, so compare
		// without whitespace.
		let text: String = exported
			.pdf
			.extract_text(&[page])
			.unwrap()
			.split_whitespace()
			.collect();
		assert!(text.contains("Draft"), "page {page} header: {text}");
		assert!(
			text.contains(&format!("{page}/{}", exported.pages)),
			"page {page} footer: {text}"
		);
	}
}

#[test]
fn a_page_size_and_margins_come_from_the_stylesheet() {
	let mut sheet = (*print()).clone();
	sheet.page.size = Some("letter".into());
	sheet.page.landscape = Some(true);
	sheet.page.margin = Some(vec![10.0, 12.0]);
	let sheet = Arc::new(sheet);
	let geometry = PageGeometry::from_style(sheet.page()).unwrap();
	assert!((geometry.width_pt - 279.4 * 72.0 / 25.4).abs() < 0.01);
	assert!((geometry.height_pt - 215.9 * 72.0 / 25.4).abs() < 0.01);
	let exported = export("# Land\n\nA short page.\n", sheet, true);
	assert_eq!(exported.pdf.get_pages().len(), 1);
	let page = exported.pdf.get_pages().into_values().next().unwrap();
	let media = exported
		.pdf
		.get_object(page)
		.unwrap()
		.as_dict()
		.unwrap()
		.get(b"MediaBox")
		.unwrap()
		.as_array()
		.unwrap();
	let width: f32 = media[2].as_float().unwrap();
	assert!((width - geometry.width_pt).abs() < 0.5);
}

/// The information dictionary, as a viewer reads it.
fn information(exported: &Exported) -> lopdf::Dictionary {
	let info = exported
		.pdf
		.trailer
		.get(b"Info")
		.expect("an information dictionary");
	let info = exported
		.pdf
		.get_object(info.as_reference().expect("an indirect Info"))
		.expect("resolvable Info");
	info.as_dict().unwrap().clone()
}

/// A text entry of a PDF dictionary: UTF-16 with a byte-order mark, or
/// PDFDocEncoding, which is ASCII-compatible.
fn text_of(dictionary: &lopdf::Dictionary, key: &[u8]) -> String {
	match dictionary.get(key).unwrap() {
		lopdf::Object::String(bytes, _) if bytes.starts_with(&[0xFE, 0xFF]) => {
			String::from_utf16_lossy(
				&bytes[2..]
					.chunks(2)
					.map(|c| {
						u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)])
					})
					.collect::<Vec<_>>(),
			)
		}
		lopdf::Object::String(bytes, _) => {
			String::from_utf8_lossy(bytes).into_owned()
		}
		other => format!("{other:?}"),
	}
}

#[test]
fn a_page_break_never_repeats_the_lines_that_moved_on() {
	// Every token appears exactly once, so a token on two pages is a line
	// painted twice. Widow control moves the last two lines of a paragraph
	// together and leaves room at the foot of the page; the fragment must not
	// paint the lines that moved.
	let source: String =
		(1..=240).map(|index| format!("w{index:03} ")).collect();
	let sheet = print();
	let geometry = PageGeometry::from_style(sheet.page()).unwrap();
	let probe = LayoutEngine::new().layout(
		&document::parse(source.clone()),
		&LayoutOptions {
			width: geometry.text_px().0,
			codeblock_wrap: true,
			stylesheet: sheet.clone(),
			fonts: fonts(),
			..Default::default()
		},
	);
	let paragraph = probe.blocks[0].layout.height;
	// A page that holds a bit more than half the paragraph.
	let short = PageGeometry {
		height_pt: geometry.margin_pt[0]
			+ geometry.margin_pt[2]
			+ paragraph * 0.62 * 0.75,
		..geometry
	};
	let exported = export_at(&source, sheet, false, Metadata::default(), short);
	assert!(exported.pages > 1, "the fixture must split");
	let mut seen: Vec<(String, usize)> = Vec::new();
	for page in 1..=exported.pages as u32 {
		let text = exported.pdf.extract_text(&[page]).unwrap();
		let tokens: Vec<String> = text
			.split_whitespace()
			.filter(|word| {
				word.len() == 4
					&& word.starts_with('w')
					&& word[1..].chars().all(|c| c.is_ascii_digit())
			})
			.map(str::to_owned)
			.collect();
		assert!(!tokens.is_empty(), "page {page} carries no tokens: {text}");
		for token in tokens {
			assert!(
				!seen.iter().any(|(seen, _)| *seen == token),
				"{token} appears on page {page} and again earlier"
			);
			seen.push((token, page as usize));
		}
	}
	assert_eq!(seen.len(), 240, "every token is exported exactly once");
}

/// The fill color and baseline of every text run a page shows.
fn text_runs(exported: &Exported, page: u32) -> Vec<([f32; 3], f32)> {
	let page = exported.pdf.get_pages()[&page];
	let content = exported.pdf.get_page_content(page);
	// `Tm[...]TJ` glues operators to their operands, so make room around the
	// array brackets before tokenizing.
	let text = String::from_utf8_lossy(&content)
		.replace('[', " [ ")
		.replace(']', " ] ");
	let tokens: Vec<&str> = text.split_whitespace().collect();
	let mut out = Vec::new();
	let mut fill = [0.0_f32; 3];
	let mut baseline = 0.0_f32;
	for (index, token) in tokens.iter().enumerate() {
		if *token == "rg" && index >= 3 {
			if let (Ok(r), Ok(g), Ok(b)) = (
				tokens[index - 3].parse::<f32>(),
				tokens[index - 2].parse::<f32>(),
				tokens[index - 1].parse::<f32>(),
			) {
				fill = [r, g, b];
			}
		} else if *token == "Tm" && index >= 1 {
			if let Ok(y) = tokens[index - 1].parse::<f32>() {
				baseline = y;
			}
		} else if token.ends_with("Tj") || token.ends_with("TJ") {
			out.push((fill, baseline));
		}
	}
	out
}

#[test]
fn a_highlighted_line_keeps_every_token_color() {
	// A line of code is one text node with one face and one size, so only the
	// paint tells the tokens apart: a run that ignored it would show the whole
	// line in whatever color its first token had.
	let source = "```rust\nfn main() { let x = \"text\"; /* note */ }\n```\n";
	let exported = export(source, print(), false);
	let mut lines: std::collections::BTreeMap<
		i64,
		std::collections::BTreeSet<[u32; 3]>,
	> = std::collections::BTreeMap::new();
	for (fill, baseline) in text_runs(&exported, 1) {
		let line = (baseline * 4.0).round() as i64;
		lines.entry(line).or_default().insert([
			fill[0].to_bits(),
			fill[1].to_bits(),
			fill[2].to_bits(),
		]);
	}
	let colors = lines.values().map(|line| line.len()).max().unwrap_or(0);
	assert!(
		colors >= 4,
		"one highlighted line keeps its token colors, saw {colors}: {lines:?}"
	);
}

#[test]
fn an_internal_link_points_at_the_heading_in_page_points() {
	// The link sits in a quote, so its own left edge is not the text area's:
	// the destination must come from the target, not from the source link.
	let exported = export(
		"# Top\n\n> [jump](#later)\n\n## Later\n\nBody.\n",
		print(),
		true,
	);
	let (page, y) = exported.anchors["later"];
	assert_eq!(page, 0);
	let [left, top, _, _] = exported.geometry.text_pt();
	// The layout records the anchor in pixels below the text area's top edge;
	// a destination is a page point in PDF space, which measures y upwards.
	let drawn = top + y * PT_PER_PX;
	let expected = [left, exported.geometry.height_pt - drawn];
	let page_id = exported.pdf.get_pages()[&1];
	let annotations = exported.pdf.get_page_annotations(page_id).unwrap();
	let destination = annotations
		.iter()
		.find_map(|annotation| annotation.get(b"Dest").ok())
		.expect("a destination annotation");
	let destination = exported
		.pdf
		.get_object(destination.as_reference().unwrap())
		.unwrap();
	let array = destination.as_array().unwrap();
	assert_eq!(array[1].as_name().unwrap(), b"XYZ");
	let actual = [array[2].as_float().unwrap(), array[3].as_float().unwrap()];
	assert!(
		(actual[0] - expected[0]).abs() < 0.01
			&& (actual[1] - expected[1]).abs() < 0.01,
		"destination {actual:?} should be {expected:?}"
	);
	assert!(drawn > exported.geometry.margin_pt[0]);
}

#[test]
fn the_information_dictionary_carries_what_the_flags_asked_for() {
	let exported = export_with(
		"# Heading\n\nBody.\n",
		print(),
		true,
		Metadata {
			title: Some("A paper".into()),
			authors: vec!["Ada".into(), "Grace".into()],
			subject: Some("Testing".into()),
			keywords: vec!["markdown".into(), "typography".into()],
			language: Some("zh-CN".into()),
			creator: Some("Editor".into()),
		},
	);
	let info = information(&exported);
	assert_eq!(text_of(&info, b"Title"), "A paper");
	assert_eq!(text_of(&info, b"Subject"), "Testing");
	assert_eq!(text_of(&info, b"Creator"), "Editor");
	assert_eq!(
		text_of(&info, b"Producer"),
		concat!("Markview ", env!("CARGO_PKG_VERSION"))
	);
	let authors = text_of(&info, b"Author");
	assert!(
		authors.contains("Ada") && authors.contains("Grace"),
		"{authors}"
	);
	let keywords = text_of(&info, b"Keywords");
	assert!(
		keywords.contains("markdown") && keywords.contains("typography"),
		"{keywords}"
	);
	let catalog = exported.pdf.catalog().expect("a catalog");
	assert_eq!(text_of(catalog, b"Lang"), "zh-CN");
}

#[test]
fn unset_metadata_is_left_out_rather_than_invented() {
	let exported =
		export_with("# Heading\n\nBody.\n", print(), true, Metadata::default());
	let info = information(&exported);
	for key in [
		b"Title".as_slice(),
		b"Author",
		b"Subject",
		b"Keywords",
		b"Creator",
		b"CreationDate",
		b"ModDate",
	] {
		assert!(
			!info.has(key),
			"{} should be absent",
			String::from_utf8_lossy(key)
		);
	}
	assert!(info.has(b"Producer"));
}

#[test]
fn a_document_with_cjk_text_embeds_a_font_that_names_it() {
	let source =
		"中文段落，用于验证字体的嵌入与复制。\n\nA short English tail.\n";
	let exported = export(source, print(), true);
	let text = exported.pdf.extract_text(&[1]).unwrap();
	assert!(text.contains("English"), "{text}");
	assert!(text.contains("中文"), "{text}");
}

/// The horizontal skew of every `cm` a page's content stream sets.
fn cm_skews(content: &str) -> Vec<f32> {
	let tokens: Vec<&str> = content.split_whitespace().collect();
	let mut out = Vec::new();
	for (index, token) in tokens.iter().enumerate() {
		if *token == "cm"
			&& index >= 6
			&& let Ok(kx) = tokens[index - 4].parse::<f32>()
		{
			out.push(kx);
		}
	}
	out
}

#[test]
fn cjk_emphasis_is_sheared_in_the_export() {
	// The bundled print styles opts `serif[cjk]` into a synthetic italic, and
	// the CJK convention has to be selected for that definition to resolve.
	let mut sheet = (*print()).clone();
	sheet.set_cjk_type(CjkType::Sc);
	let exported = export("A *中文强调* tail.\n", Arc::new(sheet), false);
	let page = exported.pdf.get_pages()[&1];
	let content = String::from_utf8_lossy(&exported.pdf.get_page_content(page))
		.into_owned();
	let shear = -SYNTHETIC_ITALIC_ANGLE_DEG.to_radians().tan();
	let skews = cm_skews(&content);
	assert!(
		skews.iter().any(|kx| (kx - shear).abs() < 1e-3),
		"no synthetic shear in {skews:?}"
	);
	// Latin emphasis uses the family's real italic face, so no shear is added.
	let latin = export("*Latin*\n", print(), false);
	let page = latin.pdf.get_pages()[&1];
	let content =
		String::from_utf8_lossy(&latin.pdf.get_page_content(page)).into_owned();
	assert!(cm_skews(&content).iter().all(|kx| kx.abs() < 1e-6));
}

#[test]
fn cjk_inside_a_formula_falls_back_to_the_document_fonts() {
	// A `\text{…}` group with CJK names no KaTeX face, so the export must
	// shape it with the document's fonts instead of dropping the characters.
	let source = "$$\\text{车到达 } a_i \\text{ 的时间} \\le v_i$$\n";
	let exported = export(source, print(), true);
	let text = exported.pdf.extract_text(&[1]).unwrap();
	// Each character is its own text object, so the reader may interleave
	// spaces and form feeds; only the ideographs are under test.
	let cjk: String = text
		.chars()
		.filter(|c| ('\u{4e00}'..='\u{9fff}').contains(c))
		.collect();
	assert_eq!(cjk, "车到达的时间", "extracted {text:?}");
}

#[test]
fn percent_encoded_heading_links_keep_their_destinations() {
	let exported = export(
		"# 中文\n\n[encoded](#%E4%B8%AD%E6%96%87) [plain](#中文) [missing](#absent)",
		print(),
		true,
	);
	let first = exported.pdf.get_pages()[&1];
	let annotations = exported.pdf.get_page_annotations(first).unwrap();
	assert_eq!(annotations.len(), 2);
	assert!(annotations.iter().all(|annotation| annotation.has(b"Dest")));
}

#[test]
fn link_hitboxes_stay_inside_the_printed_text_area() {
	let source =
		format!("[`{}`](https://example.com/)", "1234567890".repeat(80));
	let exported = export(&source, print(), true);
	let [left, top, width, height] = exported.geometry.text_pt();
	let first = exported.pdf.get_pages()[&1];
	let annotations = exported.pdf.get_page_annotations(first).unwrap();
	assert!(!annotations.is_empty());
	for annotation in annotations {
		let rect: Vec<f32> = annotation
			.get(b"Rect")
			.unwrap()
			.as_array()
			.unwrap()
			.iter()
			.map(|value| value.as_float().unwrap())
			.collect();
		assert!(
			rect[0] >= left - 0.01 && rect[2] <= left + width + 0.01,
			"{rect:?}"
		);
		assert!(
			rect[1] >= exported.geometry.height_pt - top - height - 0.01
				&& rect[3] <= exported.geometry.height_pt - top + 0.01,
			"{rect:?}"
		);
	}
}
