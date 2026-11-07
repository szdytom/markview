use markview_document::{
	BlockKind, CellAlign, CellKind, ImageSource, InlineKind, TableGroupKind,
	plain_text,
};
use markview_webpage::{Error, extract};

fn article(body: &str) -> String {
	format!(
		"<!doctype html><html><head><title>Test article</title></head><body><article><h1>Test article</h1>{body}</article></body></html>"
	)
}

fn article_without_heading(body: &str) -> String {
	format!(
		"<!doctype html><html><head><title>Test article</title></head><body><article>{body}</article></body></html>"
	)
}

fn paragraph_texts(document: &markview_document::Document) -> Vec<String> {
	document
		.blocks()
		.iter()
		.filter_map(|block| match block.kind() {
			BlockKind::Paragraph(text) => Some(plain_text(text)),
			_ => None,
		})
		.collect()
}

fn all_paragraph_texts(document: &markview_document::Document) -> Vec<String> {
	let mut texts = Vec::new();
	for block in document.blocks() {
		markview_document::visit_blocks(block, &mut |block| {
			if let BlockKind::Paragraph(text) = block.kind() {
				texts.push(plain_text(text));
			}
		});
	}
	texts
}

fn document_math(
	document: &markview_document::Document,
) -> Vec<(String, bool)> {
	let mut math = Vec::new();
	for block in document.blocks() {
		markview_document::visit_blocks(block, &mut |block| {
			let BlockKind::Paragraph(text) = block.kind() else {
				return;
			};
			for inline in text {
				if let InlineKind::Math { latex, display } = &inline.kind {
					math.push((latex.clone(), *display));
				}
			}
		});
	}
	math
}

#[test]
fn extracts_title_links_images_code_and_math_once() {
	let html = article(
		r#"<p>Intro with <a href="/relative">a link</a> and an image
		<img src="pics/a.png" alt="Diagram">.</p>
		<p><span class="math inline">\(x^2\)</span> and <span class="math display">\[y\]</span></p>
		<pre><code class="language-rust">fn main() {}</code></pre>
		"#,
	);
	let article = extract(&html, "https://example.org/blog/post").unwrap();
	assert_eq!(article.metadata.title, "Test article");
	assert_eq!(article.metadata.final_url, "https://example.org/blog/post");
	let paragraphs = paragraph_texts(&article.document);
	assert!(
		paragraphs
			.iter()
			.any(|text| text.contains("Intro with a link"))
	);
	assert!(paragraphs.iter().any(|text| text.contains("x^2")));
	let links: Vec<_> = article
		.document
		.blocks()
		.iter()
		.flat_map(|block| match block.kind() {
			BlockKind::Paragraph(text) => text.as_slice(),
			_ => &[],
		})
		.filter_map(|inline| inline.link_url().map(str::to_owned))
		.collect();
	assert!(links.contains(&"https://example.org/relative".to_owned()));
	let images: Vec<_> = article
		.document
		.blocks()
		.iter()
		.flat_map(|block| {
			let mut images = Vec::new();
			block.images(&mut images);
			images
		})
		.collect();
	assert_eq!(images[0].address(), "https://example.org/blog/pics/a.png");
	assert_eq!(images[0].alt, "Diagram");
	let math: Vec<_> = article
		.document
		.blocks()
		.iter()
		.flat_map(|block| match block.kind() {
			BlockKind::Paragraph(text) => text.as_slice(),
			_ => &[],
		})
		.filter_map(|inline| match &inline.kind {
			InlineKind::Math { latex, display } => {
				Some((latex.as_str(), *display))
			}
			_ => None,
		})
		.collect();
	assert_eq!(math, [("x^2", false), ("y", true)]);
	let code: Vec<_> = article
		.document
		.blocks()
		.iter()
		.filter_map(|block| match block.kind() {
			BlockKind::Code { language, text } => {
				Some((language.as_str(), text.as_str()))
			}
			_ => None,
		})
		.collect();
	assert_eq!(code, [("rust", "fn main() {}")]);
}

#[test]
fn complex_table_spans_groups_and_caption_are_kept() {
	let html = article(
		r#"<table>
		<caption>Coverage</caption>
		<thead><tr><th rowspan="2">Kind</th><th colspan="2">Rates</th></tr>
		<tr><th>A</th><th>B</th></tr></thead>
		<tbody><tr><td>x</td><td>1</td><td>2</td></tr>
		<tr><td>y</td><td colspan="2">3</td></tr></tbody>
		<tfoot><tr><td colspan="3">Foot</td></tr></tfoot>
		</table>
		"#,
	);
	let article = extract(&html, "https://example.org/t").unwrap();
	let table = article
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::Table(table) => Some(table),
			_ => None,
		})
		.expect("a table");
	assert_eq!(plain_text(table.caption.as_ref().unwrap()), "Coverage");
	assert_eq!(table.columns.len(), 3);
	assert_eq!(table.groups.len(), 3);
	assert_eq!(table.groups[0].kind, TableGroupKind::Head);
	assert_eq!(table.groups[1].kind, TableGroupKind::Body);
	assert_eq!(table.groups[2].kind, TableGroupKind::Foot);
	let head = &table.groups[0];
	assert_eq!(head.rows[0].cells[0].rowspan, 2);
	assert_eq!(head.rows[0].cells[0].kind, CellKind::Header);
	assert_eq!(head.rows[0].cells[1].colspan, 2);
	let body = &table.groups[1];
	assert_eq!(body.rows[1].cells[1].colspan, 2);
	let text = |cell: &markview_document::TableCell| {
		let BlockKind::Paragraph(text) = cell.blocks[0].kind() else {
			panic!("ceil paragraph");
		};
		plain_text(text)
	};
	assert_eq!(text(&body.rows[0].cells[2]), "2");
	assert_eq!(text(&table.groups[2].rows[0].cells[0]), "Foot");
}

#[test]
fn figure_definition_list_and_details_keep_semantics() {
	let html = article(
		r#"<figure><img src="plot.png" alt="Plot"><figcaption>A plot</figcaption></figure>
		<dl><dt>Term one</dt><dt>Term two</dt><dd>Definition <b>bold</b></dd></dl>
		<details open><summary>More</summary><p>Hidden body</p></details>
		"#,
	);
	let article = extract(&html, "https://example.org/f").unwrap();
	let figure = article
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::Figure { caption, .. } => Some(caption.clone()),
			_ => None,
		})
		.expect("a figure");
	assert_eq!(plain_text(figure.as_ref().unwrap()), "A plot");
	let dl = article
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::DefinitionList(items) => Some(items),
			_ => None,
		})
		.expect("a definition list");
	assert_eq!(dl[0].terms.len(), 2);
	assert_eq!(plain_text(&dl[0].terms[1]), "Term two");
	let details = article
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::Details {
				default_open,
				summary,
				..
			} => Some((*default_open, plain_text(summary))),
			_ => None,
		})
		.expect("details");
	assert_eq!(details, (true, "More".into()));
}

#[test]
fn html_punctuation_is_not_reexplained_as_markdown() {
	let html = article(
		"<p>Cost $5 and a\\b | pipe \\[brackets\\] and `code` <code>\\$x</code>.</p>",
	);
	let article = extract(&html, "https://example.org/p").unwrap();
	let paragraphs = paragraph_texts(&article.document);
	let text = paragraphs.join("\n");
	assert!(text.contains("Cost $5"));
	assert!(text.contains("a\\b | pipe \\[brackets\\]"));
	assert!(!text.contains("\\("));
	assert!(
		!article
			.document
			.blocks()
			.iter()
			.any(|block| matches!(block.kind(), BlockKind::Table(_)))
	);
}

#[test]
fn page_supplied_internal_markers_are_not_trusted() {
	let html = article(
		r#"<p data-mv-math="1" data-mv-latex="evil">Literal text only.</p>"#,
	);
	let article = extract(&html, "https://example.org/blog/post").unwrap();
	assert!(document_math(&article.document).is_empty());
	assert!(
		paragraph_texts(&article.document)
			.iter()
			.any(|text| text.contains("Literal text only."))
	);
}

#[test]
fn empty_pages_report_no_readable_content() {
	// Readability reports GrabFailed for an empty document; that is an
	// extraction failure, not a later empty-body decision.
	assert!(matches!(
		extract("<html><body></body></html>", "https://example.org/"),
		Err(Error::Extraction(_))
	));
	assert!(
		extract(
			"<html><body><p> \n </p></body></html>",
			"https://example.org/"
		)
		.is_err()
	);
}

#[test]
fn the_new_entry_point_and_the_legacy_parser_see_the_same_text() {
	let html = article("<p>Same text with <b>markup</b> and more words.</p>");
	let config = dom_smoothie::Config {
		text_mode: dom_smoothie::TextMode::Raw,
		max_elements_to_parse: 100_000,
		..Default::default()
	};
	let legacy = dom_smoothie::Readability::with_document(
		dom_smoothie::dom_query::Document::from(html.as_str()),
		Some("https://example.org/"),
		Some(config.clone()),
	)
	.unwrap()
	.parse()
	.unwrap();
	let extracted = dom_smoothie::Readability::with_document(
		dom_smoothie::dom_query::Document::from(html.as_str()),
		Some("https://example.org/"),
		Some(config),
	)
	.unwrap()
	.extract()
	.unwrap();
	assert_eq!(
		legacy.text_content.as_ref(),
		extracted.root().text().as_ref()
	);
}

/// The extraction configuration `markview_webpage` uses, so the comparison
/// below checks the same conversion the native reader runs.
fn profile_config() -> dom_smoothie::Config {
	let profile = &markview_webpage::PROFILE_V1;
	dom_smoothie::Config {
		candidate_select_mode: dom_smoothie::CandidateSelectMode::Readability,
		char_threshold: 500,
		n_top_candidates: 5,
		min_score_to_adjust: 5.0,
		disable_json_ld: false,
		keep_classes: false,
		max_elements_to_parse: profile.max_elements,
		classes_to_preserve: profile
			.preserved_classes
			.iter()
			.map(|class| (*class).to_owned())
			.collect(),
		readable_min_score: 20.0,
		readable_min_content_length: 140,
		text_mode: dom_smoothie::TextMode::Raw,
	}
}

#[test]
fn the_new_entry_point_matches_legacy_metadata_body_and_policies() {
	let html = r#"<!doctype html><html lang="en"><head>
		<title>Same article</title>
		<meta name="author" content="Ada">
		<meta property="og:site_name" content="Example">
		<meta property="article:published_time" content="2026-01-02T03:04:05Z">
		<link rel="canonical" href="/canonical">
	</head><body><article>
		<h1>Same article</h1>
		<p>Enough readable words to pass the content threshold and keep this
		paragraph in the extracted article body. See <a href="docs">the docs</a>
		and <img src="plot.png" alt="Plot">.</p>
		<dl><dt>Term</dt><dd>Definition</dd></dl>
		<details open><summary>More</summary><p>Hidden body.</p></details>
		<table><caption>Numbers</caption><thead><tr><th>A</th><th>B</th></tr></thead>
		<tbody><tr><td>1</td><td>2</td></tr></tbody></table>
	</article></body></html>"#;
	let url = "https://example.org/blog/post";
	let config = profile_config();
	let legacy = dom_smoothie::Readability::with_document(
		dom_smoothie::dom_query::Document::from(html),
		Some(url),
		Some(config.clone()),
	)
	.unwrap()
	.parse()
	.unwrap();
	let extracted = dom_smoothie::Readability::with_document(
		dom_smoothie::dom_query::Document::from(html),
		Some(url),
		Some(config.clone()),
	)
	.unwrap()
	.extract()
	.unwrap();
	assert_eq!(legacy.title, extracted.metadata.title);
	assert_eq!(legacy.byline, extracted.metadata.byline);
	assert_eq!(legacy.site_name, extracted.metadata.site_name);
	assert_eq!(legacy.lang, extracted.metadata.lang);
	assert_eq!(legacy.dir, extracted.metadata.dir);
	assert_eq!(legacy.excerpt, extracted.metadata.excerpt);
	assert_eq!(legacy.published_time, extracted.metadata.published_time);
	assert_eq!(legacy.modified_time, extracted.metadata.modified_time);
	assert_eq!(legacy.image, extracted.metadata.image);
	assert_eq!(legacy.url, extracted.metadata.url);
	assert_eq!(legacy.content, extracted.selection().html());
	assert_eq!(
		legacy.text_content.as_ref(),
		extracted.root().text().as_ref()
	);
	// The new entry point returns the DOM itself, so the body semantics are
	// inspected directly instead of through a serialized copy.
	assert!(
		extracted
			.document
			.select("p")
			.text()
			.contains("Enough readable words")
	);
	for (selector, expected) in [
		("dl dt", "Term"),
		("details summary", "More"),
		("table caption", "Numbers"),
		("img", ""),
	] {
		let selection = extracted.document.select(selector);
		assert_eq!(selection.length(), 1, "{selector}");
		assert!(selection.text().contains(expected), "{selector}");
	}
	// The old policy API still works after the shared extraction core, and
	// each policy remains a usable one-attempt entry point.
	for policy in [
		dom_smoothie::ParsePolicy::Strict,
		dom_smoothie::ParsePolicy::Moderate,
		dom_smoothie::ParsePolicy::Clean,
		dom_smoothie::ParsePolicy::Raw,
	] {
		let mut readability = dom_smoothie::Readability::with_document(
			dom_smoothie::dom_query::Document::from(html),
			Some(url),
			Some(config.clone()),
		)
		.unwrap();
		let article = readability.parse_with_policy(policy).unwrap();
		assert!(!article.text_content.is_empty(), "{policy:?}");
	}
}

#[test]
fn base_and_relative_images_resolve_against_each_page() {
	let html = article(
		r#"<p>Enough readable words to pass the content threshold and keep this paragraph in the extracted article body.
		<img src="img/a.png" alt="Plot"></p>"#,
	);
	let html = html.replace("<head>", "<head><base href=\"/assets/\">");
	let page = extract(&html, "https://example.org/blog/post").unwrap();
	let images: Vec<_> = page
		.document
		.blocks()
		.iter()
		.flat_map(|block| {
			let mut images = Vec::new();
			block.images(&mut images);
			images
		})
		.collect();
	assert_eq!(images[0].address(), "https://example.org/assets/img/a.png");
	// Without a `<base>`, the same relative image belongs to the page that
	// contains it, so two pages cannot share one resolved address.
	let plain = article(
		r#"<p>Enough readable words to pass the content threshold and keep this paragraph in the extracted article body.
		<img src="img/a.png" alt="Plot"></p>"#,
	);
	let first = extract(&plain, "https://example.org/blog/post").unwrap();
	let second = extract(&plain, "https://example.org/other/post").unwrap();
	let address = |article: &markview_webpage::WebArticle| {
		article
			.document
			.blocks()
			.iter()
			.find_map(|block| {
				let mut images = Vec::new();
				block.images(&mut images);
				images.first().map(|image| image.address().to_owned())
			})
			.expect("the article image")
	};
	assert_eq!(address(&first), "https://example.org/blog/img/a.png");
	assert_eq!(address(&second), "https://example.org/other/img/a.png");
	assert_ne!(address(&first), address(&second));
}

#[test]
fn rowspan_zero_and_headerless_tables_become_valid_grids() {
	let html = article(
		r#"<table><tbody>
		<tr><td rowspan="0">all</td><td>one</td></tr>
		<tr><td>two</td></tr>
		<tr><td>three</td></tr>
		</tbody></table>
		<table><tr><td>a</td><td>b</td></tr></table>
		"#,
	);
	let article = extract(&html, "https://example.org/grid").unwrap();
	let tables: Vec<_> = article
		.document
		.blocks()
		.iter()
		.filter_map(|block| match block.kind() {
			BlockKind::Table(table) => Some(table),
			_ => None,
		})
		.collect();
	assert_eq!(tables.len(), 2);
	let first = tables[0];
	assert_eq!(first.groups[0].kind, TableGroupKind::Body);
	assert_eq!(first.groups[0].rows[0].cells[0].rowspan, 3);
	assert_eq!(first.groups[0].rows[1].cells.len(), 1);
	let second = tables[1];
	assert_eq!(second.groups[0].kind, TableGroupKind::Body);
	assert!(
		second.groups[0].rows[0]
			.cells
			.iter()
			.all(|cell| { cell.kind == CellKind::Data })
	);
}

#[test]
fn budget_errors_are_explicit() {
	let html =
		article(&"<table><tr><td>x</td><td>y</td></tr></table>".repeat(3));
	let mut profile = markview_webpage::PROFILE_V1.clone();
	profile.max_table_cells = 1;
	assert!(matches!(
		markview_webpage::extract_with(&html, "https://example.org/", &profile),
		Err(markview_webpage::Error::TableTooLarge)
	));
	let mut profile = markview_webpage::PROFILE_V1.clone();
	profile.max_text_bytes = 1;
	assert!(matches!(
		markview_webpage::extract_with(
			&article("<p>long enough text</p>"),
			"https://example.org/",
			&profile
		),
		Err(markview_webpage::Error::TooManyText { .. })
	));
}

#[test]
fn table_row_budget_counts_every_table_in_the_article() {
	let html = article(
		r#"<table><tbody><tr><td>a</td></tr><tr><td>b</td></tr></tbody></table>
		<table><tbody><tr><td>c</td></tr><tr><td>d</td></tr></tbody></table>"#,
	);
	let mut profile = markview_webpage::PROFILE_V1.clone();
	profile.max_table_rows = 3;
	assert!(matches!(
		markview_webpage::extract_with(&html, "https://example.org/", &profile),
		Err(markview_webpage::Error::TableTooLarge)
	));
}

#[test]
fn encoded_inline_svg_counts_toward_the_text_budget() {
	let body = format!(
		"<p>{}</p><svg><path d=\"{}\"/></svg>",
		"Readable text. ".repeat(40),
		"M0 0 ".repeat(200)
	);
	let html = article(&body);
	let mut profile = markview_webpage::PROFILE_V1.clone();
	profile.max_text_bytes = 1_000;
	assert!(matches!(
		markview_webpage::extract_with(&html, "https://example.org/", &profile),
		Err(markview_webpage::Error::TooManyText { .. })
	));
}

#[test]
fn formula_and_image_alternatives_count_toward_the_text_budget() {
	let latex = "x".repeat(2_000);
	let html = article(&format!(
		"<p>{}</p><span class=\"math inline\">\\({latex}\\)</span>",
		"Readable text. ".repeat(40)
	));
	let mut profile = markview_webpage::PROFILE_V1.clone();
	profile.max_text_bytes = 1_500;
	assert!(matches!(
		markview_webpage::extract_with(&html, "https://example.org/", &profile),
		Err(markview_webpage::Error::TooManyText { .. })
	));
	let alt = "a".repeat(2_000);
	let html = article(&format!(
		"<p>{}</p><img src=\"a.png\" alt=\"{alt}\">",
		"Readable text. ".repeat(40)
	));
	assert!(matches!(
		markview_webpage::extract_with(&html, "https://example.org/", &profile),
		Err(markview_webpage::Error::TooManyText { .. })
	));
}

#[test]
fn inline_boundaries_keep_words_apart_and_figures_keep_images() {
	let html = article(
		r#"<p>Hello <b>bold</b> world.</p>
		<figure><img src="plot.png" alt="Plot"><figcaption>A plot</figcaption></figure>"#,
	);
	let article = extract(&html, "https://example.org/space").unwrap();
	let text = paragraph_texts(&article.document).join("\n");
	assert!(text.contains("Hello bold world."), "{text}");
	let figure_images: Vec<_> = article
		.document
		.blocks()
		.iter()
		.filter_map(|block| match block.kind() {
			BlockKind::Figure { blocks, .. } => Some(blocks),
			_ => None,
		})
		.flat_map(|blocks| {
			let mut images = Vec::new();
			for block in blocks {
				block.images(&mut images);
			}
			images
		})
		.collect();
	assert_eq!(figure_images.len(), 1);
	assert_eq!(figure_images[0].address(), "https://example.org/plot.png");
}

#[test]
fn rowspans_contribute_to_the_column_count() {
	let html = article(
		r#"<table><tbody>
		<tr><td rowspan="2">A</td><td>B</td></tr>
		<tr><td>C</td><td>D</td></tr>
		</tbody></table>"#,
	);
	let article = extract(&html, "https://example.org/spans").unwrap();
	let table = article
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::Table(table) => Some(table),
			_ => None,
		})
		.unwrap();
	assert_eq!(table.columns.len(), 3);
	let cells = |row: &markview_document::TableRow| {
		row.cells
			.iter()
			.map(|cell| {
				let BlockKind::Paragraph(text) = cell.blocks[0].kind() else {
					panic!("cell paragraph");
				};
				plain_text(text)
			})
			.collect::<Vec<_>>()
	};
	assert_eq!(cells(&table.groups[0].rows[1]), ["C", "D"]);
}

#[test]
fn reversed_and_explicit_list_numbers_are_preserved() {
	let html = article(
		r#"<ol reversed><li>a</li><li>b</li><li>c</li></ol>
		<ol><li value="5">x</li><li>y</li></ol>"#,
	);
	let article = extract(&html, "https://example.org/lists").unwrap();
	let lists: Vec<_> = article
		.document
		.blocks()
		.iter()
		.filter_map(|block| match block.kind() {
			BlockKind::List {
				start,
				reversed,
				items,
				..
			} => Some((*start, *reversed, items)),
			_ => None,
		})
		.collect();
	assert_eq!(lists[0].0, Some(3));
	assert!(lists[0].1);
	assert_eq!(lists[1].2[0].number, Some(5));
}

#[test]
fn adjacent_identical_formulas_stay_distinct() {
	let html = article(
		r#"<p><span class="math">x</span><span class="math">x</span></p>"#,
	);
	let article = extract(&html, "https://example.org/math").unwrap();
	let math = article
		.document
		.blocks()
		.iter()
		.flat_map(|block| match block.kind() {
			BlockKind::Paragraph(text) => text.as_slice(),
			_ => &[],
		})
		.filter(|inline| matches!(inline.kind, InlineKind::Math { .. }))
		.count();
	assert_eq!(math, 2, "{:?}", article.document.blocks());
}

#[test]
fn absurd_colspans_error_instead_of_overflowing() {
	let html = article(
		r#"<table><tr><td>a</td><td colspan="18446744073709551615">b</td></tr></table>"#,
	);
	assert!(matches!(
		extract(&html, "https://example.org/colspan"),
		Err(markview_webpage::Error::TableTooLarge)
	));
}

#[test]
fn mathml_annotation_and_styled_whitespace_are_safe() {
	let html = article(
		r#"<p>Hello <b> bold </b> world.</p>
		<math><semantics><mrow><mi>x</mi></mrow><annotation encoding="application/x-tex">x^2</annotation></semantics></math>"#,
	);
	let article = extract(&html, "https://example.org/mathml").unwrap();
	let text = paragraph_texts(&article.document).join("\n");
	assert!(text.contains("Hello bold world.\nx^2"), "{text}");
}

#[test]
fn deep_disclosures_and_list_targets_and_block_links_are_safe() {
	let nested = article(
		"<details><details><details>body</details></details></details>",
	);
	let mut profile = markview_webpage::PROFILE_V1.clone();
	profile.max_semantic_depth = 2;
	assert!(matches!(
		markview_webpage::extract_with(
			&nested,
			"https://example.org/deep",
			&profile
		),
		Err(markview_webpage::Error::DepthExceeded)
	));

	let html = article(
		r#"<a href="/destination"><h2>Linked section</h2></a>
		<ol><li id="note">Note body</li></ol>"#,
	);
	let article = extract(&html, "https://example.org/links").unwrap();
	let heading = article
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::Heading { text, .. }
				if plain_text(text) == "Linked section" =>
			{
				Some(text)
			}
			_ => None,
		})
		.unwrap();
	assert!(
		heading.iter().all(|inline| {
			inline.link_url() == Some("https://example.org/destination")
		}),
		"{heading:?}"
	);
	let list = article
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::List { items, .. } => Some(items),
			_ => None,
		})
		.unwrap();
	assert!(
		list[0].blocks[0].anchors().iter().any(|a| a == "note"),
		"{:?}",
		list[0].blocks[0]
	);
}

#[test]
fn nbsp_edges_survive_trimming_and_outline_and_cell_anchors_are_kept() {
	let html = article(
		"<p>&nbsp;Hi&nbsp;</p><figure><h2>Figure heading</h2></figure><table><thead><tr><th id=\"cell\">Cell</th></tr></thead></table>",
	);
	let article = extract(&html, "https://example.org/edges").unwrap();
	let paragraph = paragraph_texts(&article.document).join("\n");
	assert!(paragraph.contains('\u{a0}'), "{paragraph:?}");
	assert!(
		article
			.document
			.outline()
			.iter()
			.any(|entry| entry.text == "Figure heading")
	);
	let table = article
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::Table(table) => Some(table),
			_ => None,
		})
		.unwrap();
	assert!(
		table.groups[0].rows[0].cells[0].blocks[0]
			.anchors()
			.iter()
			.any(|anchor| anchor == "cell")
	);
}

#[test]
fn explicit_ids_win_over_generated_heading_slugs() {
	let html = article(r#"<h2>Target</h2><p id="target">Destination</p>"#);
	let article = extract(&html, "https://example.org/ids").unwrap();
	let heading = article
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::Heading { anchor, .. } => Some(anchor.clone()),
			_ => None,
		})
		.unwrap();
	assert_ne!(heading, "target");
	let destination = article
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::Paragraph(text) if plain_text(text) == "Destination" => {
				Some(block.anchors().to_vec())
			}
			_ => None,
		})
		.unwrap();
	assert!(destination.iter().any(|anchor| anchor == "target"));
}

#[test]
fn definition_list_group_wrappers_keep_terms_and_definitions() {
	let html =
		article(r#"<dl><div><dt>Term</dt><dd>Definition</dd></div></dl>"#);
	let article = extract(&html, "https://example.org/dl").unwrap();
	let list = article
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::DefinitionList(items) => Some(items),
			_ => None,
		})
		.expect("definition list");
	assert_eq!(plain_text(&list[0].terms[0]), "Term");
	let definitions = &list[0].definitions[0];
	assert!(definitions.iter().any(|block| match block.kind() {
		BlockKind::Paragraph(text) => plain_text(text).contains("Definition"),
		_ => false,
	}));
}

#[test]
fn aria_hidden_definition_wrappers_and_table_groups_stay_hidden() {
	// A hidden wrapper owns its subtree even when the wrapper is not itself a
	// `dt`/`dd` or a row.
	let html = article(
		r#"<dl><div aria-hidden="true"><dt>dup term</dt><dd>dup body</dd></div><dt>Kept</dt><dd>Kept body</dd></dl>
		<table><tbody aria-hidden="true"><tr><td>dup a</td><td>dup b</td></tr></tbody>
		<tbody><tr><td>Kept one</td><td>Kept two</td></tr>
		<tr><td>Kept three</td><td>Kept four</td></tr></tbody></table>"#,
	);
	let article = extract(&html, "https://example.org/hidden-groups").unwrap();
	let texts = all_paragraph_texts(&article.document);
	assert!(texts.iter().all(|text| !text.contains("dup")), "{texts:?}");
	let list = article
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::DefinitionList(items) => Some(items),
			_ => None,
		})
		.expect("a definition list");
	let terms: Vec<String> =
		list.iter().map(|item| plain_text(&item.terms[0])).collect();
	assert_eq!(terms, ["Kept"]);
	let table = article
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::Table(table) => Some(table),
			_ => None,
		})
		.expect("a table");
	assert_eq!(table.groups.len(), 1, "a hidden group leaves no rows");
	assert_eq!(table.groups[0].rows.len(), 2);
}

#[test]
fn foot_groups_follow_body_groups_regardless_of_source_order() {
	// HTML renders the head, then every body, then the foot, so a `<tfoot>`
	// written before `<tbody>` must still come last.
	let html = article(
		r#"<table><tfoot><tr><td>Foot</td></tr></tfoot>
		<tbody><tr><td>Body</td></tr></tbody>
		<thead><tr><th>Head</th></tr></thead></table>"#,
	);
	let article = extract(&html, "https://example.org/foot").unwrap();
	let table = article
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::Table(table) => Some(table),
			_ => None,
		})
		.expect("a table");
	let kinds: Vec<TableGroupKind> =
		table.groups.iter().map(|group| group.kind).collect();
	assert_eq!(
		kinds,
		[
			TableGroupKind::Head,
			TableGroupKind::Body,
			TableGroupKind::Foot
		]
	);
	let first_cell = |group: &markview_document::TableGroup| {
		let BlockKind::Paragraph(text) =
			group.rows[0].cells[0].blocks[0].kind()
		else {
			panic!("a cell paragraph");
		};
		plain_text(text)
	};
	assert_eq!(first_cell(&table.groups[0]), "Head");
	assert_eq!(first_cell(&table.groups[1]), "Body");
	assert_eq!(first_cell(&table.groups[2]), "Foot");
}

#[test]
fn pre_language_attributes_and_leading_blank_lines_survive() {
	let html = article(
		"<pre class=\"language-mermaid\"><code>graph TD; A--&gt;B;</code></pre><pre>\n\nfirst line</pre>",
	);
	let article = extract(&html, "https://example.org/pre").unwrap();
	let image = article
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::Paragraph(text) => {
				text.iter().find_map(|inline| match &inline.kind {
					InlineKind::Image(image) => Some(&image.source),
					_ => None,
				})
			}
			_ => None,
		});
	assert!(
		matches!(
			image,
			Some(ImageSource::Diagram(diagram))
				if diagram.language == "mermaid"
					&& diagram.source == "graph TD; A-->B;"
		),
		"{image:?}"
	);
	let code =
		article
			.document
			.blocks()
			.iter()
			.find_map(|block| match block.kind() {
				BlockKind::Code { text, .. } => Some(text.clone()),
				_ => None,
			});
	assert!(
		code.as_deref().is_some_and(|text| text.starts_with('\n')),
		"{code:?}"
	);
}

#[test]
fn the_article_title_heading_is_not_duplicated() {
	// Readability drops the leading site header that matches the title and
	// demotes the article's own title heading, so matching only level 1 would
	// insert a synthetic title above it.
	let html = "<!doctype html><html><head><title>Test article</title></head><body>\
		<header><h1>Test article</h1></header>\
		<article><h1>Test article</h1>\
		<p>Body text for the article, with plenty of words to pass the threshold, and more words to ensure readability keeps this paragraph as the main content.</p>\
		<p>Another paragraph with more text to make the extraction viable and readable for the library.</p>\
		</article></body></html>";
	let article = extract(html, "https://example.org/blog/post").unwrap();
	let title = article.metadata.title.clone();
	let headings: Vec<_> = article
		.document
		.blocks()
		.iter()
		.filter_map(|block| match block.kind() {
			BlockKind::Heading { level, text, .. } => {
				Some((*level, plain_text(text)))
			}
			_ => None,
		})
		.collect();
	let titles: Vec<_> =
		headings.iter().filter(|(_, text)| *text == title).collect();
	assert_eq!(titles.len(), 1, "{headings:?}");
	// Readability demotes the article's own title heading, but it still owns
	// the document title and must lower as level 1.
	assert_eq!(titles[0].0, 1, "{headings:?}");
}

#[test]
fn a_matching_section_heading_does_not_suppress_the_synthetic_title() {
	// Readability drops the site header that matches the title, so a later
	// section may still repeat it. Only the article's leading heading stands
	// for the title; a later one is content.
	let html = "<!doctype html><html><head><title>Test article</title></head><body>\
		<header><h1>Test article</h1></header>\
		<article><h2>Introduction</h2>\
		<p>Opening paragraph with enough words to keep the article readable, so the extraction keeps the body instead of falling back, and it keeps going for a while to pass the threshold.</p>\
		<h2>Test article</h2>\
		<p>Closing paragraph that repeats the title as a section heading, with more words to stay in the extracted body.</p>\
		</article></body></html>";
	let article = extract(html, "https://example.org/blog/post").unwrap();
	let headings: Vec<_> = article
		.document
		.blocks()
		.iter()
		.filter_map(|block| match block.kind() {
			BlockKind::Heading { level, text, .. } => {
				Some((*level, plain_text(text)))
			}
			_ => None,
		})
		.collect();
	assert_eq!(
		headings
			.first()
			.map(|(level, text)| (*level, text.as_str())),
		Some((1, "Test article")),
		"{headings:?}"
	);
	assert_eq!(
		headings
			.iter()
			.filter(|(_, text)| text == "Test article")
			.count(),
		2,
		"{headings:?}"
	);
}

#[test]
fn a_body_without_its_own_heading_gets_the_synthetic_title_and_source() {
	let html = article_without_heading(
		"<p>Only body text, with no heading in the article itself.</p>",
	);
	let article = extract(&html, "https://example.org/blog/post").unwrap();
	let heading = article
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::Heading { level, text, .. } => {
				Some((*level, plain_text(text)))
			}
			_ => None,
		})
		.expect("a synthetic title heading");
	assert_eq!(heading, (1, "Test article".to_owned()));
	let paragraphs = paragraph_texts(&article.document);
	assert!(
		paragraphs
			.iter()
			.any(|text| text == "Source: https://example.org/blog/post"),
		"{paragraphs:?}"
	);
}

#[test]
fn definition_terms_and_definitions_keep_fragment_targets() {
	let html = article(
		r#"<dl><dt id="term">Term</dt><dd id="definition">Definition</dd></dl>"#,
	);
	let article = extract(&html, "https://example.org/anchors").unwrap();
	let list = article
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::DefinitionList(items) => Some(items),
			_ => None,
		})
		.unwrap();
	assert!(list[0].terms[0].iter().any(|inline| {
		matches!(&inline.kind, InlineKind::Anchor(anchor) if anchor == "term")
	}));
	assert!(
		list[0].definitions[0][0]
			.anchors()
			.iter()
			.any(|anchor| anchor == "definition")
	);
}

#[test]
fn stored_web_fixture_keeps_math_and_empty_table_grid_cells() {
	let html = include_str!("../../../tests/fixtures/web-math-table.html");
	let article = extract(html, "https://example.org/math").unwrap();
	let mut math = Vec::new();
	let mut code = Vec::new();
	for block in article.document.blocks() {
		markview_document::visit_blocks(block, &mut |block| {
			let BlockKind::Paragraph(text) = block.kind() else {
				return;
			};
			for inline in text {
				match &inline.kind {
					InlineKind::Math { latex, display } => {
						math.push((latex.clone(), *display));
					}
					InlineKind::Text(text) if inline.style.code => {
						code.push(text.clone());
					}
					_ => {}
				}
			}
		});
	}
	// Table pipes stay escaped inside cell math instead of re-splitting cells.
	assert!(math.contains(&("a|b".to_owned(), false)), "{math:?}");
	assert!(math.contains(&("\\|x\\|".to_owned(), false)), "{math:?}");
	// A static MathJax script becomes one formula with the right display mode.
	assert!(math.contains(&("x < y".to_owned(), false)), "{math:?}");
	assert!(
		math.contains(&("x^2 + y^2 = z^2".to_owned(), true)),
		"{math:?}"
	);
	// Inline code that merely looks like math stays literal text.
	assert!(code.iter().any(|text| text == "\\(not math\\)"), "{code:?}");
	// Both display formulas appear exactly once.
	for latex in [
		"W(k_1,k_2)=\\Pr\\!\\big(\\Sigma_{\\mathrm{us}}\\le\\Sigma_{\\mathrm{th}}\\big)",
		"x^2 + y^2 = z^2",
	] {
		assert_eq!(
			math.iter()
				.filter(|(formula, display)| *display && formula == latex)
				.count(),
			1,
			"{math:?}"
		);
	}
	assert_eq!(
		math.iter().filter(|(_, display)| *display).count(),
		2,
		"{math:?}"
	);
	let tables: Vec<_> = article
		.document
		.blocks()
		.iter()
		.filter_map(|block| match block.kind() {
			BlockKind::Table(table) => Some(table),
			_ => None,
		})
		.collect();
	assert_eq!(tables.len(), 2);
	let cell_text = |cell: &markview_document::TableCell| {
		let BlockKind::Paragraph(text) = cell.blocks[0].kind() else {
			panic!("cell paragraph");
		};
		plain_text(text)
	};
	let first = tables[0];
	assert_eq!(first.columns.len(), 5);
	assert_eq!(first.groups[0].rows[0].cells.len(), 5);
	assert_eq!(first.groups[1].rows[0].cells.len(), 5);
	assert_eq!(cell_text(&first.groups[0].rows[0].cells[2]), "");
	// The header keeps its formulas and the numeric cells keep plain digits.
	let cell_math = |cell: &markview_document::TableCell| {
		let BlockKind::Paragraph(text) = cell.blocks[0].kind() else {
			panic!("cell paragraph");
		};
		text.iter()
			.filter_map(|inline| match &inline.kind {
				InlineKind::Math { latex, .. } => Some(latex.clone()),
				_ => None,
			})
			.collect::<Vec<_>>()
	};
	assert_eq!(cell_math(&first.groups[0].rows[0].cells[0]), ["(k_1,k_2)"]);
	assert_eq!(cell_math(&first.groups[0].rows[0].cells[1]), ["W"]);
	assert_eq!(cell_text(&first.groups[1].rows[0].cells[1]), "0.840");
	assert_eq!(cell_text(&first.groups[1].rows[0].cells[4]), "0.372");
	assert_eq!(cell_text(&first.groups[1].rows[0].cells[2]), "");
	let second = tables[1];
	assert_eq!(second.columns.len(), 2);
	assert_eq!(second.groups[0].rows[0].cells.len(), 2);
	assert!(
		second.groups[0].rows[0]
			.cells
			.iter()
			.all(|cell| cell_text(cell).is_empty())
	);
}

#[test]
fn web_checklist_state_survives_input_removal() {
	// Readability removes `input` elements, so the state must already be on
	// the list item before extraction.
	let html = article(
		r#"<ul><li><input type="checkbox" checked> Done</li><li><input type="checkbox"> Pending</li><li>Plain</li></ul>"#,
	);
	let article = extract(&html, "https://example.org/tasks").unwrap();
	let list = article
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::List { items, .. } => Some(items),
			_ => None,
		})
		.expect("a list");
	let states: Vec<_> = list.iter().map(|item| item.checked).collect();
	assert_eq!(states, [Some(true), Some(false), None]);
}

#[test]
fn block_math_uses_display_delimiters_without_a_display_class() {
	let html = article(
		r#"<div class="math">\[y\]</div>
		<p>Inline <span class="math inline">\(x\)</span> value.</p>"#,
	);
	let article = extract(&html, "https://example.org/math").unwrap();
	let math = document_math(&article.document);
	assert!(math.contains(&("y".to_owned(), true)), "{math:?}");
	assert!(math.contains(&("x".to_owned(), false)), "{math:?}");
}

#[test]
fn aria_hidden_containers_hide_text_but_presentation_does_not() {
	let html = article(
		r#"<ul><li aria-hidden="true">dup list</li><li>Kept list</li><li role="presentation">Presented list</li></ul>
		<table><tr><td aria-hidden="true">dup cell</td><td>Kept cell</td><td role="presentation">Presented cell</td></tr></table>"#,
	);
	let article = extract(&html, "https://example.org/hidden").unwrap();
	let texts = all_paragraph_texts(&article.document);
	assert!(texts.iter().all(|text| !text.contains("dup")), "{texts:?}");
	for expected in
		["Kept list", "Presented list", "Kept cell", "Presented cell"]
	{
		assert!(
			texts.iter().any(|text| text == expected),
			"{expected}: {texts:?}"
		);
	}
}

#[test]
fn katex_formula_copies_are_lowered_once() {
	// KaTeX and MathJax emit a MathML copy next to a visual rendering marked
	// `aria-hidden`; lowering the hidden copy would duplicate the formula as
	// prose.
	let html = article(
		r#"<p>Inline <span class="katex"><span class="katex-mathml"><math><semantics><mrow><msup><mi>x</mi><mn>2</mn></msup></mrow><annotation encoding="application/x-tex">x^2</annotation></semantics></math></span><span class="katex-html" aria-hidden="true"><span class="base"><span class="mord mathnormal">x</span><span class="msupsub"><span class="vlist">2</span></span></span></span></span> value.</p>
		<p>Block <span class="katex"><span class="katex-mathml"><math display="block"><semantics><mrow><msup><mi>y</mi><mn>2</mn></msup></mrow><annotation encoding="application/x-tex">y^2</annotation></semantics></math></span><span class="katex-html" aria-hidden="true"><span class="base"><span class="mord mathnormal">y</span><span class="msupsub"><span class="vlist">2</span></span></span></span></span> value.</p>
		<p>Presented <span role="presentation">9</span> value.</p>"#,
	);
	let article = extract(&html, "https://example.org/katex").unwrap();
	let mut math = Vec::new();
	for block in article.document.blocks() {
		markview_document::visit_blocks(block, &mut |block| {
			let BlockKind::Paragraph(text) = block.kind() else {
				return;
			};
			for inline in text {
				if let InlineKind::Math { latex, display } = &inline.kind {
					math.push((latex.clone(), *display));
				}
			}
		});
	}
	assert_eq!(
		math,
		[("x^2".to_owned(), false), ("y^2".to_owned(), true)],
		"{math:?}"
	);
	let paragraphs = paragraph_texts(&article.document);
	assert!(
		paragraphs
			.iter()
			.all(|text| !text.contains("x2") && !text.contains("y2")),
		"{paragraphs:?}"
	);
	// `role="presentation"` only removes ARIA semantics, so the text stays.
	assert!(
		paragraphs
			.iter()
			.any(|text| text.contains("Presented 9 value")),
		"{paragraphs:?}"
	);
}

#[test]
fn a_nested_title_heading_is_recognized_without_reading_its_text() {
	// Readability may keep the article's title heading inside wrappers, and
	// the page title may differ from the heading. The marker set before
	// extraction decides, so no text comparison can get this wrong.
	let html = "<!doctype html><html><head><title>A longer page title</title></head><body>\
		<article><div><section><h1>Short heading</h1>\
		<p>Body text for the article, with plenty of words to pass the threshold, and more words so readability keeps this paragraph as the main content.</p>\
		</section></div>\
		<p>Another paragraph with more text to make the extraction viable and readable for the library.</p>\
		</article></body></html>";
	let article = extract(html, "https://example.org/blog/post").unwrap();
	assert_eq!(article.metadata.title, "A longer page title");
	let headings: Vec<_> = article
		.document
		.blocks()
		.iter()
		.filter_map(|block| match block.kind() {
			BlockKind::Heading { level, text, .. } => {
				Some((*level, plain_text(text)))
			}
			_ => None,
		})
		.collect();
	assert_eq!(headings.len(), 1, "{headings:?}");
	assert_eq!(headings[0].1, "Short heading");
}

#[test]
fn a_term_keeps_every_definition_that_follows_it() {
	let html = article("<dl><dt>A</dt><dd>1</dd><dd>2</dd></dl>");
	let article = extract(&html, "https://example.org/definitions").unwrap();
	let list = article
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::DefinitionList(items) => Some(items),
			_ => None,
		})
		.expect("a definition list");
	assert_eq!(list.len(), 1, "{list:?}");
	assert_eq!(list[0].terms.len(), 1);
	assert_eq!(plain_text(&list[0].terms[0]), "A");
	let bodies: Vec<String> = list[0]
		.definitions
		.iter()
		.map(|blocks| match blocks[0].kind() {
			BlockKind::Paragraph(text) => plain_text(text),
			_ => panic!("a definition paragraph"),
		})
		.collect();
	assert_eq!(bodies, ["1", "2"]);
}

#[test]
fn the_synthetic_source_link_keeps_its_own_occurrence() {
	let html = article(
		r#"<p>See <a href="https://example.org/target">the target</a> and <a href="https://example.org/target">the same target</a>.</p>"#,
	);
	let article = extract(&html, "https://example.org/source-link").unwrap();
	let mut occurrences = Vec::new();
	for block in article.document.blocks() {
		markview_document::visit_blocks(block, &mut |block| {
			let BlockKind::Paragraph(text) = block.kind() else {
				return;
			};
			occurrences.extend(
				text.iter()
					.filter_map(|inline| inline.link.as_ref())
					.map(|link| link.occurrence()),
			);
		});
	}
	assert_eq!(occurrences.len(), 3, "{occurrences:?}");
	for (index, occurrence) in occurrences.iter().enumerate() {
		for other in &occurrences[index + 1..] {
			assert_ne!(occurrence, other, "{occurrences:?}");
		}
	}
}

#[test]
fn presentational_roles_keep_readable_text_but_hide_formula_copies() {
	// `role="presentation"` removes ARIA semantics, not content: only a
	// formula's visual copy is duplicated text.
	let html = article(
		r#"<p role="presentation">A presented paragraph with enough words to pass the extraction threshold.</p>
		<table><tr><td role="presentation">Presented cell</td><td>Plain cell</td></tr></table>
		<p>Inline <span class="katex"><span class="katex-mathml"><math><semantics><mrow><msup><mi>x</mi><mn>2</mn></msup></mrow><annotation encoding="application/x-tex">x^2</annotation></semantics></math></span><span role="presentation"><span class="mord">x</span><span class="msupsub">2</span></span></span> value.</p>"#,
	);
	let article = extract(&html, "https://example.org/presentation").unwrap();
	let paragraphs = paragraph_texts(&article.document);
	assert!(
		paragraphs
			.iter()
			.any(|text| text.contains("A presented paragraph")),
		"{paragraphs:?}"
	);
	let table = article
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::Table(table) => Some(table),
			_ => None,
		})
		.expect("a table");
	let cell_text = |cell: &markview_document::TableCell| {
		let BlockKind::Paragraph(text) = cell.blocks[0].kind() else {
			panic!("cell paragraph");
		};
		plain_text(text)
	};
	assert_eq!(
		cell_text(&table.groups[0].rows[0].cells[0]),
		"Presented cell"
	);
	let math: Vec<_> = article
		.document
		.blocks()
		.iter()
		.flat_map(|block| match block.kind() {
			BlockKind::Paragraph(text) => text
				.iter()
				.filter_map(|inline| match &inline.kind {
					InlineKind::Math { latex, display } => {
						Some((latex.as_str(), *display))
					}
					_ => None,
				})
				.collect::<Vec<_>>(),
			_ => Vec::new(),
		})
		.collect();
	assert_eq!(math, [("x^2", false)]);
	assert!(
		paragraphs
			.iter()
			.all(|text| !text.contains("x2") && !text.contains("Presented 2")),
		"{paragraphs:?}"
	);
}

#[test]
fn table_cell_and_column_alignment_survive_readability() {
	let html = article(
		r#"<table>
		<thead><tr><th align="center">Kind</th><th align="right">Rate</th></tr></thead>
		<tbody><tr><td align="center">a</td><td align="right">1</td></tr>
		<tr><td align="center">b</td><td align="right">2</td></tr></tbody>
		</table>"#,
	);
	let article = extract(&html, "https://example.org/align").unwrap();
	let table = article
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::Table(table) => Some(table),
			_ => None,
		})
		.expect("a table");
	assert_eq!(table.columns[0].align, Some(CellAlign::Center));
	assert_eq!(table.columns[1].align, Some(CellAlign::Right));
	let head = &table.groups[0].rows[0];
	assert_eq!(head.cells[0].align, Some(CellAlign::Center));
	assert_eq!(head.cells[1].align, Some(CellAlign::Right));
	let body = &table.groups[1].rows[1];
	assert_eq!(body.cells[0].align, Some(CellAlign::Center));
	assert_eq!(body.cells[1].align, Some(CellAlign::Right));
}

#[test]
fn block_links_reach_figure_captions_and_details_summaries() {
	let html = article(
		r#"<a href="/figure"><figure><img src="plot.png" alt="Plot"><figcaption>Caption text</figcaption></figure></a>
		<a href="/details"><details open><summary>Summary text</summary><p>Body text</p></details></a>"#,
	);
	let article = extract(&html, "https://example.org/block-links").unwrap();
	let mut figure_caption = None;
	let mut details_summary = None;
	for block in article.document.blocks() {
		match block.kind() {
			BlockKind::Figure { caption, .. } => {
				figure_caption = caption.clone();
			}
			BlockKind::Details { summary, .. } => {
				details_summary = Some(summary.clone());
			}
			_ => {}
		}
	}
	let figure_caption = figure_caption.expect("a figure caption");
	assert!(
		figure_caption
			.iter()
			.all(|inline| inline.link_url()
				== Some("https://example.org/figure")),
		"{figure_caption:?}"
	);
	let details_summary = details_summary.expect("a details summary");
	assert!(
		details_summary
			.iter()
			.all(|inline| inline.link_url()
				== Some("https://example.org/details")),
		"{details_summary:?}"
	);
}

#[test]
fn the_stored_article_fixture_keeps_links_emphasis_code_and_prose() {
	let html = include_str!("../../../tests/fixtures/web-article.html");
	let article = extract(html, "https://example.org/read").unwrap();
	let mut text = String::new();
	let mut links = Vec::new();
	let mut images = Vec::new();
	let mut bold = Vec::new();
	let mut code = Vec::new();
	for block in article.document.blocks() {
		markview_document::visit_blocks(block, &mut |block| {
			if let BlockKind::Code { text: body, .. } = block.kind() {
				code.push(body.clone());
			}
			let mut found = Vec::new();
			block.images(&mut found);
			images.extend(found.into_iter().cloned());
			let BlockKind::Paragraph(rich) = block.kind() else {
				return;
			};
			text.push_str(&plain_text(rich));
			text.push('\n');
			for inline in rich {
				if let Some(url) = inline.link_url() {
					links.push(url.to_owned());
				}
				if let InlineKind::Text(value) = &inline.kind {
					if inline.style.bold {
						bold.push(value.clone());
					}
					if inline.style.code {
						code.push(value.clone());
					}
				}
			}
		});
	}
	let first = article.document.blocks()[0].kind();
	let BlockKind::Heading {
		level,
		text: heading,
		..
	} = first
	else {
		panic!("a title heading, found {first:?}");
	};
	assert_eq!(
		(*level, plain_text(heading).as_str()),
		(1, "Native web reading")
	);
	assert!(
		links.contains(&"https://example.org/related".to_owned()),
		"{links:?}"
	);
	assert!(
		images
			.iter()
			.any(|image| image.address() == "https://example.org/diagram.png"
				&& image.alt == "Diagram"),
		"{images:?}"
	);
	assert!(
		bold.iter().any(|value| value.contains("native typography")),
		"{bold:?}"
	);
	assert!(
		code.iter().any(|value| value == "rustup default beta"),
		"{code:?}"
	);
	assert!(
		code.iter().any(|value| value == "\\(keep code\\)"),
		"{code:?}"
	);
	// Prose punctuation is not reinterpreted as Markdown.
	assert!(
		text.lines().any(|line| line
			== "中文 Try (rustup default beta) with [literal brackets] in prose."),
		"{text:?}"
	);
	// Site navigation and scripts are not article content.
	assert!(!text.contains("Navigation link"), "{text:?}");
	assert!(!text.contains("alert("), "{text:?}");
	for block in article.document.blocks() {
		let BlockKind::Paragraph(rich) = block.kind() else {
			continue;
		};
		assert!(
			!rich
				.iter()
				.any(|inline| matches!(inline.kind, InlineKind::Math { .. })),
			"{rich:?}"
		);
	}
}

#[test]
fn mathml_alttext_stands_in_for_a_missing_tex_annotation() {
	let html = article(
		r#"<p>Formula <math alttext="x^2"><mrow><msup><mi>x</mi><mn>2</mn></msup></mrow></math> value.</p>"#,
	);
	let alttext = extract(&html, "https://example.org/alttext").unwrap();
	let math: Vec<_> = alttext
		.document
		.blocks()
		.iter()
		.flat_map(|block| match block.kind() {
			BlockKind::Paragraph(text) => text
				.iter()
				.filter_map(|inline| match &inline.kind {
					InlineKind::Math { latex, display } => {
						Some((latex.as_str(), *display))
					}
					_ => None,
				})
				.collect::<Vec<_>>(),
			_ => Vec::new(),
		})
		.collect();
	assert_eq!(math, [("x^2", false)]);
	assert!(
		alttext.metadata.diagnostics.is_empty(),
		"{:?}",
		alttext.metadata.diagnostics
	);

	// Without `alttext` or a TeX annotation the glyph text is all there is, so
	// it stays plain text and the diagnostic says why.
	let html = article(
		r#"<p>Formula <math><mrow><msup><mi>x</mi><mn>2</mn></msup></mrow></math> value.</p>"#,
	);
	let article = extract(&html, "https://example.org/glyphs").unwrap();
	let text = paragraph_texts(&article.document).join("\n");
	assert!(text.contains("Formula x2 value"), "{text:?}");
	assert!(
		article
			.metadata
			.diagnostics
			.iter()
			.any(|message| message.contains("MathML without TeX annotation")),
		"{:?}",
		article.metadata.diagnostics
	);
}

#[test]
fn mathml_alttext_delimiters_are_stripped_and_state_display() {
	let html = article(
		r#"<p>Formula <math alttext="\[x\]"><mrow><mi>x</mi></mrow></math> value.</p>"#,
	);
	let article = extract(&html, "https://example.org/alttext-tex").unwrap();
	assert_eq!(document_math(&article.document), [("x".to_owned(), true)]);
}

#[test]
fn nested_definition_wrappers_keep_document_order() {
	// A depth-first walk keeps `A` before `B`; a queue would reach the
	// shallower wrapper's terms first.
	let html = article(
		r#"<dl><div><div><dt>A</dt><dd>First</dd></div></div><div><dt>B</dt><dd>Second</dd></div></dl>"#,
	);
	let article = extract(&html, "https://example.org/dl-order").unwrap();
	let list = article
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::DefinitionList(items) => Some(items),
			_ => None,
		})
		.expect("a definition list");
	let terms: Vec<String> =
		list.iter().map(|item| plain_text(&item.terms[0])).collect();
	assert_eq!(terms, ["A", "B"]);
	let bodies: Vec<String> = list
		.iter()
		.map(|item| match item.definitions[0][0].kind() {
			BlockKind::Paragraph(text) => plain_text(text),
			_ => panic!("a definition paragraph"),
		})
		.collect();
	assert_eq!(bodies, ["First", "Second"]);
}

#[test]
fn definition_wrappers_count_against_the_depth_limit() {
	let html =
		article(r#"<dl><div><div><dt>A</dt><dd>First</dd></div></div></dl>"#);
	let mut profile = markview_webpage::PROFILE_V1.clone();
	profile.max_semantic_depth = 3;
	assert!(matches!(
		markview_webpage::extract_with(
			&html,
			"https://example.org/dl-depth",
			&profile
		),
		Err(markview_webpage::Error::DepthExceeded)
	));
}

#[test]
fn a_checklist_item_owns_only_its_own_checkbox() {
	let html = article(
		r#"<ul><li><input type="checkbox" checked> Outer
		<ul><li><input type="checkbox"> Inner</li></ul></li></ul>"#,
	);
	let article = extract(&html, "https://example.org/nested-tasks").unwrap();
	let mut states = Vec::new();
	for block in article.document.blocks() {
		markview_document::visit_blocks(block, &mut |block| {
			if let BlockKind::List { items, .. } = block.kind() {
				states.extend(items.iter().map(|item| item.checked));
			}
		});
	}
	assert_eq!(states, [Some(true), Some(false)]);
}

#[test]
fn deeply_nested_lists_stop_at_the_element_budget() {
	// One pre-pass walk marks every checkbox, so a page above the element
	// budget fails before any per-item scan can make the cost grow with depth.
	let depth = 2_000;
	let mut body = String::new();
	for _ in 0..depth {
		body.push_str("<ul><li>");
	}
	body.push_str("deep");
	for _ in 0..depth {
		body.push_str("</li></ul>");
	}
	let html = article(&body);
	let mut profile = markview_webpage::PROFILE_V1.clone();
	profile.max_elements = 500;
	let error = markview_webpage::extract_with(
		&html,
		"https://example.org/deep",
		&profile,
	)
	.expect_err("the element budget");
	let Error::TooManyElements { elements, limit } = error else {
		panic!("a TooManyElements error: {error:?}");
	};
	assert!(elements > limit, "{elements} > {limit}");
	assert_eq!(limit, profile.max_elements);
}

#[test]
fn aria_hidden_captions_summaries_and_terms_stay_hidden() {
	let html = article(
		r#"<figure><img src="hidden.png" alt="Plot"><figcaption aria-hidden="true">dup figcaption</figcaption></figure>
		<figure><img src="kept.png" alt="Plot"><figcaption>Kept figcaption</figcaption></figure>
		<details open><summary aria-hidden="true">dup summary</summary><p>Kept body</p></details>
		<details open><summary>Kept summary</summary><p>More body</p></details>
		<table><caption aria-hidden="true">dup caption</caption><tr><td>Kept cell</td></tr></table>
		<table><caption>Kept caption</caption><tr><td>Another cell</td></tr></table>
		<dl><dt aria-hidden="true">dup term</dt><dd>Hidden term body</dd>
		<dt role="presentation">Presented term</dt><dd>Presented body</dd></dl>"#,
	);
	let article = extract(&html, "https://example.org/presented").unwrap();
	let mut captions = Vec::new();
	let mut summaries = Vec::new();
	let mut terms = Vec::new();
	for block in article.document.blocks() {
		match block.kind() {
			BlockKind::Figure { caption, .. } => {
				captions.extend(caption.iter().map(|text| plain_text(text)));
			}
			BlockKind::Details { summary, .. } => {
				summaries.push(plain_text(summary));
			}
			BlockKind::Table(table) => {
				captions
					.extend(table.caption.iter().map(|text| plain_text(text)));
			}
			BlockKind::DefinitionList(items) => {
				terms.extend(items.iter().flat_map(|item| {
					item.terms.iter().map(|text| plain_text(text))
				}));
			}
			_ => {}
		}
	}
	assert_eq!(captions, ["Kept figcaption", "Kept caption"]);
	let visible: Vec<&String> =
		summaries.iter().filter(|text| !text.is_empty()).collect();
	assert_eq!(visible, ["Kept summary"]);
	// A hidden term leaves no empty item behind.
	assert_eq!(terms, ["Presented term"]);
}

#[test]
fn a_later_h1_stays_content_and_keeps_the_metadata_title() {
	let html = article_without_heading(
		"<p>Opening prose with plenty of words to pass the extraction threshold and to keep the whole body readable, so the section heading below stays in the extracted article.</p>\
		<h1>Section</h1>\
		<p>Closing prose with more words, so the extraction keeps the body instead of falling back to a fragment.</p>",
	);
	let article = extract(&html, "https://example.org/later").unwrap();
	let headings: Vec<(u8, String)> = article
		.document
		.blocks()
		.iter()
		.filter_map(|block| match block.kind() {
			BlockKind::Heading { level, text, .. } => {
				Some((*level, plain_text(text)))
			}
			_ => None,
		})
		.collect();
	assert_eq!(
		headings.first(),
		Some(&(1, "Test article".to_owned())),
		"{headings:?}"
	);
	assert!(
		headings.contains(&(1, "Section".to_owned())),
		"{headings:?}"
	);
	assert_eq!(
		headings
			.iter()
			.filter(|(_, text)| text == "Test article")
			.count(),
		1,
		"{headings:?}"
	);
}

#[test]
fn overlapping_spans_place_the_blocked_cell() {
	// `c` cannot span two columns because `b` covers the second one; it must
	// still appear as a plain cell instead of being dropped.
	let html = article(
		r#"<table><tbody>
		<tr><td>a</td><td rowspan="2">b</td></tr>
		<tr><td colspan="2">c</td></tr>
		</tbody></table>"#,
	);
	let article = extract(&html, "https://example.org/overlap").unwrap();
	let table = article
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::Table(table) => Some(table),
			_ => None,
		})
		.expect("a table");
	let cells = |row: &markview_document::TableRow| {
		row.cells
			.iter()
			.map(|cell| match cell.blocks[0].kind() {
				BlockKind::Paragraph(text) => plain_text(text),
				_ => panic!("a cell paragraph"),
			})
			.collect::<Vec<_>>()
	};
	assert_eq!(cells(&table.groups[0].rows[0]), ["a", "b"]);
	assert_eq!(cells(&table.groups[0].rows[1]), ["c"]);
	assert_eq!(table.groups[0].rows[0].cells[1].rowspan, 2);
}

#[test]
fn tex_delimiters_are_stripped_on_every_formula_path() {
	let html = article(
		r#"<p>Script <script type="math/tex">$$x$$</script> and
		<script type="math/tex">\(y\)</script> and annotation
		<math><semantics><annotation encoding="application/x-tex">\[z\]</annotation></semantics></math>.</p>"#,
	);
	let article = extract(&html, "https://example.org/tex").unwrap();
	let math = document_math(&article.document);
	assert!(math.contains(&("x".to_owned(), true)), "{math:?}");
	assert!(math.contains(&("y".to_owned(), false)), "{math:?}");
	assert!(math.contains(&("z".to_owned(), true)), "{math:?}");
}

#[test]
fn a_declared_canonical_url_wins_over_open_graph_metadata() {
	let html = article(
		"<p>Enough readable words to pass the content threshold and keep this \
		 paragraph in the extracted article body.</p>",
	)
	.replace(
		"<head>",
		"<head><link rel=\"canonical\" href=\"/canonical\">\
		 <meta property=\"og:url\" content=\"https://example.org/og\">",
	);
	let page = extract(&html, "https://example.org/blog/post").unwrap();
	assert_eq!(
		page.metadata.canonical_url.as_deref(),
		Some("https://example.org/canonical")
	);
	// Without a declared canonical link, the metadata URL is the fallback.
	let plain = article(
		"<p>Enough readable words to pass the content threshold and keep this \
		 paragraph in the extracted article body.</p>",
	);
	let page = extract(&plain, "https://example.org/blog/post").unwrap();
	assert_ne!(
		page.metadata.canonical_url.as_deref(),
		Some("https://example.org/canonical")
	);
}

#[test]
fn an_explicit_heading_id_is_declared_once() {
	// Readability drops attributes from `<blockquote>` and `<div>` wrappers,
	// so the non-heading case uses a paragraph, which keeps its `id`.
	let html = article(
		r#"<p>Enough readable words to pass the content threshold and keep
		this paragraph in the extracted article body, which also gives the
		article candidate a score high enough to survive the cleanup pass.</p>
		<h2 id="marker">Section</h2>
		<p id="target">A paragraph with its own id survives as the block a
		fragment jump reaches, and it carries enough words to stay in the
		article body.</p>
		<p>A closing paragraph gives the article more prose so candidate
		selection keeps every block above it, including the heading.</p>"#,
	);
	let page = extract(&html, "https://example.org/ids").unwrap();
	let heading = page
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::Heading {
				level: 2, anchor, ..
			} => Some((block.anchors().to_vec(), anchor.clone())),
			_ => None,
		})
		.expect("the heading");
	assert_eq!(heading.1, "marker");
	assert!(
		heading.0.is_empty(),
		"the explicit id is also a block anchor: {:?}",
		heading.0
	);
	// The anchor still addresses the heading through the outline.
	assert!(
		page.document
			.outline()
			.iter()
			.any(|entry| entry.anchor == "marker")
	);
	// A non-heading explicit id still lands on the block it names, so a
	// fragment jump reaches that paragraph.
	let target = page
		.document
		.blocks()
		.iter()
		.find(|block| {
			matches!(block.kind(), BlockKind::Paragraph(text) if plain_text(text).contains("fragment jump reaches"))
		})
		.expect("the targeted paragraph");
	assert_eq!(target.anchors(), ["target"]);
}

#[test]
fn a_colliding_span_is_retried_in_the_next_free_column() {
	let html = article(
		r#"<p>Filler prose keeps this page readable enough for the extractor to
		keep the whole article including the table below.</p>
		<table><tbody>
		<tr><td rowspan="2">A</td><td colspan="2">B</td></tr>
		<tr><td colspan="2">C</td></tr>
		</tbody></table>"#,
	);
	let page = extract(&html, "https://example.org/span").unwrap();
	let table = page
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::Table(table) => Some(table),
			_ => None,
		})
		.expect("a table");
	assert_eq!(table.columns.len(), 3);
	let row = &table.groups[0].rows[1];
	assert_eq!(row.cells.len(), 1);
	// The cursor column is covered by `A`'s rowspan, so `C` moves one column
	// right with its declared span instead of shrinking.
	assert_eq!(row.cells[0].colspan, 2);
}

#[test]
fn a_span_is_clipped_to_the_free_columns_at_the_first_free_slot() {
	let html = article(
		r#"<p>Filler prose keeps this page readable enough for the extractor to
		keep the whole article including the table below.</p>
		<table><tbody>
		<tr><td rowspan="2">A</td><td rowspan="2">B</td><td colspan="2">C</td></tr>
		<tr><td colspan="3">D</td></tr>
		</tbody></table>"#,
	);
	let mut profile = markview_webpage::PROFILE_V1.clone();
	profile.max_table_columns = 4;
	let page = markview_webpage::extract_with(
		&html,
		"https://example.org/fallback",
		&profile,
	)
	.unwrap();
	let table = page
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::Table(table) => Some(table),
			_ => None,
		})
		.expect("a table");
	let row = &table.groups[0].rows[1];
	assert_eq!(row.cells.len(), 1);
	// The four-column cap leaves room for two of `D`'s three columns after
	// the two rowspans, so the span is clipped instead of dropped.
	assert_eq!(row.cells[0].colspan, 2);
	let BlockKind::Paragraph(text) = row.cells[0].blocks[0].kind() else {
		panic!("a cell paragraph");
	};
	assert_eq!(plain_text(text), "D");
}

#[test]
fn block_content_inside_a_heading_stays_a_block() {
	// A page can leave a heading open over the rest of the article; the
	// extractor then keeps that heading with block children. They must not be
	// flattened into the heading's text.
	let html = article(
		r#"<p>Lead paragraph with enough prose for the extractor to keep this
		article and its malformed heading, which is left open over the content
		that follows it.</p>
		<h2>Section<div><p>Nested prose inside the heading becomes its own
		paragraph block instead of joining the heading text.</p>
		<ul><li>item one</li><li>item two</li></ul></div></h2>
		<p>Trailing prose keeps the article's candidate score high enough to
		survive cleanup.</p>"#,
	);
	let page = extract(&html, "https://example.org/nested").unwrap();
	let blocks = page.document.blocks();
	let heading = blocks
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::Heading { level: 2, text, .. } => Some(text),
			_ => None,
		})
		.expect("the heading");
	assert_eq!(plain_text(heading), "Section");
	assert!(
		blocks.iter().any(|block| matches!(block.kind(), BlockKind::Paragraph(text) if plain_text(text).starts_with("Nested prose inside the heading"))),
		"the nested paragraph is not a block: {blocks:?}"
	);
	let list = blocks
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::List { items, .. } => Some(items),
			_ => None,
		})
		.expect("the nested list");
	assert_eq!(list.len(), 2);
}

#[test]
fn heading_text_inside_a_mixed_inline_wrapper_keeps_its_style() {
	// The heading's text is wrapped in `<em>`, which also contains a block;
	// the text stays in the heading with its style, the block goes below.
	let html = article(
		r#"<p>Lead paragraph with enough prose for the extractor to keep this
		article and its malformed heading, which is left open over the content
		that follows it.</p>
		<h2><em>Section<div><p>Nested prose inside the heading becomes its own
		paragraph block instead of joining the heading text.</p></div></em></h2>
		<p>Trailing prose keeps the article's candidate score high enough to
		survive cleanup.</p>"#,
	);
	let page = extract(&html, "https://example.org/mixed").unwrap();
	let heading = page
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::Heading { level: 2, text, .. } => Some(text),
			_ => None,
		})
		.expect("the heading");
	assert_eq!(plain_text(heading), "Section");
	assert!(
		heading.iter().all(|inline| inline.style.italic),
		"the heading lost its style: {heading:?}"
	);
	let nested = page
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::Paragraph(text)
				if plain_text(text)
					.starts_with("Nested prose inside the heading") =>
			{
				Some(text)
			}
			_ => None,
		})
		.expect("the nested paragraph");
	assert!(
		nested.iter().all(|inline| inline.style.italic),
		"the deferred block lost the wrapper's style: {nested:?}"
	);
}

#[test]
fn a_code_heading_wrapper_keeps_its_content_literal() {
	// `<code>` makes its content literal: the block is split out of the
	// heading, both keep the code context, and math markup stays text.
	let html = article(
		r#"<p>Lead paragraph with enough prose for the extractor to keep this
		article and its malformed heading, which is left open over the content
		that follows it.</p>
		<h2><code><span class="math">x^2</span><div><p>Code-wrapped body
		prose stays literal inside the heading.</p></div></code></h2>
		<p>Trailing prose keeps the article's candidate score high enough to
		survive cleanup.</p>"#,
	);
	let page = extract(&html, "https://example.org/code").unwrap();
	let blocks = page.document.blocks();
	let heading = blocks
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::Heading { level: 2, text, .. } => Some(text),
			_ => None,
		})
		.expect("the heading");
	assert_eq!(plain_text(heading), "x^2");
	assert!(
		heading.iter().all(|inline| inline.style.code
			&& !matches!(
				inline.kind,
				markview_document::InlineKind::Math { .. }
			)),
		"the heading left its code context: {heading:?}"
	);
	let nested = blocks
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::Paragraph(text)
				if plain_text(text).starts_with("Code-wrapped body prose") =>
			{
				Some(text)
			}
			_ => None,
		})
		.expect("the deferred paragraph");
	assert!(
		nested.iter().all(|inline| inline.style.code
			&& !matches!(
				inline.kind,
				markview_document::InlineKind::Math { .. }
			)),
		"the deferred block left its code context: {nested:?}"
	);
}

#[test]
fn a_linked_heading_wrapper_links_its_deferred_blocks() {
	// The wrapper link must reach the blocks split out of the heading.
	let html = article(
		r#"<p>Lead paragraph with enough prose for the extractor to keep this
		article and its malformed heading, which is left open over the content
		that follows it.</p>
		<h2><a href="/target">Section<div><p>Nested prose inside the linked
		heading becomes its own paragraph block with the wrapper's link.</p>
		</div></a></h2>
		<p>Trailing prose keeps the article's candidate score high enough to
		survive cleanup.</p>"#,
	);
	let page = extract(&html, "https://example.org/linked").unwrap();
	let blocks = page.document.blocks();
	let heading = blocks
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::Heading { level: 2, text, .. } => Some(text),
			_ => None,
		})
		.expect("the heading");
	assert_eq!(plain_text(heading), "Section");
	let nested = blocks
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::Paragraph(text)
				if plain_text(text)
					.starts_with("Nested prose inside the linked heading") =>
			{
				Some(text)
			}
			_ => None,
		})
		.expect("the nested paragraph");
	let url = "https://example.org/target";
	assert!(
		nested.iter().all(|inline| inline.link_url() == Some(url)),
		"the deferred block lost the wrapper's link: {nested:?}"
	);
	assert!(
		heading.iter().all(|inline| inline.link_url() == Some(url)),
		"the heading lost the wrapper's link: {heading:?}"
	);
}

#[test]
fn a_math_wrapper_with_a_block_still_lowers_as_a_formula() {
	let html = article(
		r#"<p>Lead paragraph with enough prose for the extractor to keep this
		article and its malformed heading, which is left open over the content
		that follows it.</p>
		<h2><span class="math"><div>x^2</div></span></h2>
		<p>Trailing prose keeps the article's candidate score high enough to
		survive cleanup.</p>"#,
	);
	let page = extract(&html, "https://example.org/math").unwrap();
	let heading = page
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::Heading { level: 2, text, .. } => Some(text),
			_ => None,
		})
		.expect("the heading");
	let math: Vec<_> = heading
		.iter()
		.filter_map(|inline| match &inline.kind {
			markview_document::InlineKind::Math { latex, display } => {
				Some((latex.as_str(), *display))
			}
			_ => None,
		})
		.collect();
	assert_eq!(math, [("x^2", false)], "{heading:?}");
}

#[test]
fn specialized_deferred_blocks_keep_the_wrapper_link() {
	// A formula and a diagram are built by dedicated constructors; both must
	// still carry the enclosing wrapper's link.
	let formula = article(
		r#"<p>Lead paragraph with enough prose for the extractor to keep this
		article and its malformed heading, which is left open over the content
		that follows it.</p>
		<h2><a href="/target">Section<div class="math">x^2</div></a></h2>
		<p>Trailing prose keeps the article's candidate score high enough to
		survive cleanup.</p>"#,
	);
	let url = "https://example.org/target";
	let page = extract(&formula, "https://example.org/formula").unwrap();
	let math = page
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::Paragraph(text)
				if text.iter().any(|inline| {
					matches!(
						inline.kind,
						markview_document::InlineKind::Math { .. }
					)
				}) =>
			{
				Some(text)
			}
			_ => None,
		})
		.expect("the deferred formula");
	assert!(
		math.iter().all(|inline| inline.link_url() == Some(url)),
		"the formula lost the wrapper link: {math:?}"
	);
	let diagram = article(
		r#"<p>Lead paragraph with enough prose for the extractor to keep this
		article and its malformed heading, which is left open over the content
		that follows it.</p>
		<h2><a href="/target">Section<div><pre><code class="language-mermaid">graph TD; A--&gt;B;</code></pre></div></a></h2>
		<p>Trailing prose keeps the article's candidate score high enough to
		survive cleanup.</p>"#,
	);
	let page = extract(&diagram, "https://example.org/diagram").unwrap();
	let image = page
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::Paragraph(text) => text.iter().find(|inline| {
				matches!(inline.kind, markview_document::InlineKind::Image(_))
			}),
			_ => None,
		})
		.expect("the deferred diagram");
	assert_eq!(
		image.link_url(),
		Some(url),
		"the diagram lost the wrapper link"
	);
}

#[test]
fn a_deferred_mermaid_block_inside_a_code_wrapper_stays_literal() {
	let html = article(
		r#"<p>Lead paragraph with enough prose for the extractor to keep this
		article and its malformed heading, which is left open over the content
		that follows it.</p>
		<h2><code>Section<div><pre class="language-mermaid">graph TD;
		A--&gt;B;</pre></div></code></h2>
		<p>Trailing prose keeps the article's candidate score high enough to
		survive cleanup.</p>"#,
	);
	let page = extract(&html, "https://example.org/mermaid").unwrap();
	let code = page
		.document
		.blocks()
		.iter()
		.find_map(|block| match block.kind() {
			BlockKind::Code { language, text } => {
				Some((language.as_str(), text.as_str()))
			}
			_ => None,
		})
		.expect("the literal code block");
	assert_eq!(code.0, "mermaid");
	assert!(code.1.contains("graph TD;"), "{code:?}");
	let mut images = Vec::new();
	for block in page.document.blocks() {
		block.images(&mut images);
	}
	assert!(
		images.is_empty(),
		"a code-wrapped diagram was rendered: {images:?}"
	);
}

#[test]
fn common_highlighter_markup_preserves_code_languages_and_text() {
	// Representative formats from Prism, highlight.js, Jekyll/Rouge,
	// GitHub, Prettify and rehype-pretty-code; token spans stay literal text.
	let cases = [
		(
			r#"<div class="language-javascript"><div><pre><code><span class="token keyword">const</span> x = 1;</code></pre></div></div>"#,
			"js",
			"const x = 1;",
		),
		(
			r#"<pre><code class="hljs python"><span class="hljs-built_in">print</span>(1)</code></pre>"#,
			"py",
			"print(1)",
		),
		(
			r#"<figure class="highlight"><pre><code class="language-c--" data-lang="c++">int x = 1;</code></pre></figure>"#,
			"cpp",
			"int x = 1;",
		),
		(
			r#"<div class="highlight highlight-source-ruby"><pre>puts 1</pre></div>"#,
			"rb",
			"puts 1",
		),
		(
			r#"<pre class="prettyprint lang-js">const x = 1;</pre>"#,
			"js",
			"const x = 1;",
		),
		(
			r#"<pre data-language="typescript"><code><span data-line>const x: number = 1;</span></code></pre>"#,
			"ts",
			"const x: number = 1;",
		),
		(
			r#"<div data-lang=" Bash "><pre>echo 1</pre></div>"#,
			"sh",
			"echo 1",
		),
		(
			r#"<pre class="language-python"><code class="language-rust">fn main() {}</code></pre>"#,
			"rust",
			"fn main() {}",
		),
		(
			r#"<pre class="language-"><code data-language="">untyped</code></pre>"#,
			"",
			"untyped",
		),
		(
			r#"<pre><code class="python">print(1)</code></pre>"#,
			"py",
			"print(1)",
		),
		(
			r#"<div class="python"><pre>print(1)</pre></div>"#,
			"",
			"print(1)",
		),
	];
	for (markup, expected_language, expected_text) in cases {
		let page =
			extract(&article(markup), "https://example.org/article").unwrap();
		let mut codes = Vec::new();
		for block in page.document.blocks() {
			markview_document::visit_blocks(block, &mut |block| {
				if let BlockKind::Code { language, text } = block.kind() {
					codes.push((language.clone(), text.clone()));
				}
			});
		}
		assert_eq!(
			codes,
			[(expected_language.into(), expected_text.into())],
			"{markup}"
		);
	}
}

#[test]
fn language_inheritance_opt_out_and_shebang_fallback_are_conservative() {
	let page = extract(&article(r#"
		<p>This article compares code examples across languages, including inherited declarations, explicit plain text and interpreter directives. Each example belongs to this tutorial.</p>
		<div class="language-rust">
			<p>Examples in this section use Rust by default, with individual examples overriding that language when needed.</p>
			<pre><code>fn main() {}</code></pre>
			<div class="language-none"><pre>#!/usr/bin/python3
print(1)</pre></div>
			<pre><code class="nohighlight language-python">print(1)</code></pre>
			<pre class="no-highlight">output</pre>
			<pre><code class="language-plaintext">output</code></pre>
			<pre class="language-unknown">#!/bin/bash
echo 1</pre>
		</div>
		<pre>#!/usr/bin/env -S python3 -u
print(1)</pre>
		<pre>#!/bin/bash
echo 1</pre>
		<pre>print(1)</pre>
		<pre class="shiki"><code><span style="color: red">const</span> x = 1;</code></pre>
		<pre>#!/usr/bin/not-python
print(1)</pre>
	"#), "https://example.org/article").unwrap();
	let mut languages = Vec::new();
	for block in page.document.blocks() {
		markview_document::visit_blocks(block, &mut |block| {
			if let BlockKind::Code { language, .. } = block.kind() {
				languages.push(language.clone());
			}
		});
	}
	assert_eq!(
		languages,
		[
			"rust", "text", "text", "text", "text", "unknown", "py", "sh", "",
			"", ""
		]
	);
}

#[test]
fn web_language_aliases_select_native_highlighting_syntaxes() {
	let syntaxes = two_face::syntax::extra_newlines();
	let themes =
		syntect::highlighting::ThemeSet::from(&two_face::theme::extra());
	for (alias, code) in [
		("javascript", "const x = 1;"),
		("typescript", "const x: number = 1;"),
		("python3", "print('hello')"),
		("ruby", "puts 'hello'"),
		("bash", "echo 'hello'"),
		("c++", "int x = 1;"),
		("csharp", "int x = 1;"),
		("markup", "&lt;p&gt;hello&lt;/p&gt;"),
		("yml", "key: true"),
	] {
		let html =
			article(&format!("<pre data-language=\"{alias}\">{code}</pre>"));
		let page = extract(&html, "https://example.org/article").unwrap();
		let (language, text) = page
			.document
			.blocks()
			.iter()
			.find_map(|block| {
				if let BlockKind::Code { language, text } = block.kind() {
					Some((language, text))
				} else {
					None
				}
			})
			.expect("the extracted code block");
		let syntax = syntaxes
			.find_syntax_by_token(language)
			.or_else(|| syntaxes.find_syntax_by_name(language))
			.unwrap_or_else(|| {
				panic!("no native syntax for {alias}: {language}")
			});
		let mut highlighter = syntect::easy::HighlightLines::new(
			syntax,
			&themes.themes["InspiredGitHub"],
		);
		let line = format!("{text}\n");
		let regions = highlighter.highlight_line(&line, &syntaxes).unwrap();
		assert!(
			regions
				.windows(2)
				.any(|pair| pair[0].0.foreground != pair[1].0.foreground),
			"{alias} has no syntax colors"
		);
	}
}
