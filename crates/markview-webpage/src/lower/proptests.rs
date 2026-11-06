use super::*;
use proptest::prelude::*;
use std::collections::HashSet;

const BASE: &str = "https://a.test/article/page";

fn extracted(body: &str) -> dom_smoothie::Extracted {
	let document = dom_smoothie::dom_query::Document::from(format!(
		"<html><body>{body}</body></html>"
	));
	let root = document.select("body").nodes()[0].id;
	dom_smoothie::Extracted {
		document,
		root,
		metadata: Default::default(),
	}
}

fn lower(body: &str, profile: &Profile) -> Result<Document, Error> {
	let extracted = extracted(body);
	Lower::new(&extracted, url::Url::parse(BASE).unwrap(), profile.clone())
		.run("Title", BASE)
		.map(|(document, _)| document)
}

fn escape(text: &str) -> String {
	text.replace('&', "&amp;")
		.replace('<', "&lt;")
		.replace('>', "&gt;")
		.replace('"', "&quot;")
}

fn blank(ch: char) -> bool {
	matches!(ch, '\t' | '\n' | '\u{c}' | '\r' | ' ')
}

// Rectangles come from partitioning a complete matrix, independently of HTML placement.
fn rectangles(
	height: usize,
	width: usize,
	cuts: &[(u8, bool, u8)],
) -> Vec<(usize, usize, usize, usize)> {
	let mut cells = vec![(0, 0, height, width)];
	for &(index, vertical, cut) in cuts {
		let index = usize::from(index) % cells.len();
		let (row, col, h, w) = cells[index];
		let length = if vertical { w } else { h };
		if length == 1 {
			continue;
		}
		let at = 1 + usize::from(cut) % (length - 1);
		if vertical {
			cells[index] = (row, col, h, at);
			cells.push((row, col + at, h, w - at));
		} else {
			cells[index] = (row, col, at, w);
			cells.push((row + at, col, h - at, w));
		}
	}
	cells.sort_unstable();
	cells
}

#[test]
fn rectangle_model_can_partition_a_staggered_grid() {
	assert_eq!(
		rectangles(3, 4, &[(0, true, 1), (0, false, 0), (1, false, 1)]),
		vec![(0, 0, 1, 2), (0, 2, 2, 2), (1, 0, 2, 2), (2, 2, 1, 2)],
	);
}

proptest! {
	#[test]
	fn mixed_table_rectangles_preserve_spans_markers_and_group_ownership(
		width in 1usize..7, heights in prop::array::uniform3(1usize..7),
		cuts in prop::collection::vec((any::<u8>(), any::<bool>(), any::<u8>()), 0..24),
		normalize in any::<bool>(),
	) {
		let mut html_groups = Vec::new();
		let mut expected_groups = Vec::new();
		for (group, &height) in heights.iter().enumerate() {
			let cells = rectangles(height, width, &cuts);
			let tag = ["thead", "tbody", "tfoot"][group];
			let mut html = format!("<{tag}>");
			for row in 0..height {
				html.push_str("<tr>");
				for &(r, c, h, w) in cells.iter().filter(|cell| cell.0 == row) {
					let raw_h = if normalize && r + h == height { if c % 2 == 0 { 0 } else { h + 10 } } else { h };
					let raw_w = if normalize && w == 1 { 0 } else { w };
					html.push_str(&format!("<td rowspan='{raw_h}' colspan='{raw_w}'>g{group}r{r}c{c}</td>"));
				}
				html.push_str("</tr>");
			}
			html.push_str(&format!("</{tag}>"));
			html_groups.push(html);
			expected_groups.push(cells);
		}
		let html = format!("<table>{}{}{}</table>", html_groups[2], html_groups[1], html_groups[0]);
		let capacity = heights.iter().sum::<usize>() * width;
		let profile = Profile { max_table_columns: width, max_table_rows: heights.iter().sum(), max_table_cells: capacity, ..crate::PROFILE_V1.clone() };
		let document = lower(&html, &profile).unwrap();
		let table = document.blocks().iter().find_map(|block| match block.kind() { BlockKind::Table(table) => Some(table), _ => None }).unwrap();
		prop_assert_eq!(table.columns.len(), width);
		prop_assert_eq!(table.groups.len(), 3);
		for (group, expected) in expected_groups.iter().enumerate() {
			prop_assert_eq!(table.groups[group].kind, [TableGroupKind::Head, TableGroupKind::Body, TableGroupKind::Foot][group]);
			prop_assert_eq!(table.groups[group].rows.len(), heights[group]);
			let actual: Vec<_> = table.groups[group].rows.iter().enumerate().flat_map(|(r, row)| row.cells.iter().map(move |cell| {
				let marker = cell.blocks.iter().map(|block| match block.kind() { BlockKind::Paragraph(text) => plain_text(text), _ => panic!("marker changed kind") }).collect::<String>();
				(r, cell.rowspan as usize, cell.colspan as usize, marker)
			})).collect();
			let expected: Vec<_> = expected.iter().map(|&(r, c, h, w)| (r, h, w, format!("g{group}r{r}c{c}"))).collect();
			prop_assert_eq!(actual, expected);
		}
		for profile in [Profile { max_table_cells: capacity - 1, ..profile.clone() }, Profile { max_table_columns: width - 1, ..profile.clone() }, Profile { max_table_rows: heights.iter().sum::<usize>() - 1, ..profile }] {
			prop_assert!(matches!(lower(&html, &profile), Err(Error::TableTooLarge)), "table budget must be enforced");
		}
	}

	#[test]
	fn table_rowspans_reserve_columns_and_stop_at_their_group_boundary(
		columns in 1usize..7, heights in prop::array::uniform3(1usize..6),
		spans in prop::array::uniform6(0usize..10), aligns in prop::array::uniform6(0usize..4),
	) {
		let mut html_groups = Vec::new();
		let mut expected = Vec::new();
		let mut column_alignments = vec![Vec::new(); columns];
		for (group, &height) in heights.iter().enumerate() {
			let tag = ["thead", "tbody", "tfoot"][group];
			let mut html = format!("<{tag}>");
			let mut rows = Vec::new();
			for row in 0..height {
				html.push_str("<tr>");
				let mut cells = Vec::new();
				for column in 0..columns {
					let span = if spans[column] == 0 { height } else { spans[column].min(height) };
					if row > 0 && row < span { continue; }
					let text = format!("g{group}r{row}c{column}");
					let cell = if group == 0 { "th" } else { "td" };
					let alignment = [None, Some(CellAlign::Left), Some(CellAlign::Center), Some(CellAlign::Right)][(aligns[column] + row + group) % 4];
					if let Some(alignment) = alignment { column_alignments[column].push(alignment); }
					let raw_span = if row == 0 { spans[column] } else { 1 };
					let align = ["", "left", "center", "right"][(aligns[column] + row + group) % 4];
					html.push_str(&format!("<{cell} rowspan='{raw_span}' align='{align}'>{text}</{cell}>"));
					cells.push((if row == 0 { span as u32 } else { 1 }, alignment, text));
				}
				html.push_str("</tr>"); rows.push(cells);
			}
			html.push_str(&format!("</{tag}>"));
			html_groups.push(html); expected.push(rows);
		}
		let html = format!("<table>{}{}{}</table>", html_groups[2], html_groups[1], html_groups[0]);
		let profile = Profile { max_table_rows: heights.iter().sum(), max_table_columns: columns, max_table_cells: heights.iter().sum::<usize>() * columns, ..crate::PROFILE_V1.clone() };
		let document = lower(&html, &profile).unwrap();
		let table = document.blocks().iter().find_map(|block| match block.kind() { BlockKind::Table(table) => Some(table), _ => None }).unwrap();
		prop_assert_eq!(table.columns.len(), columns);
		prop_assert_eq!(table.groups.len(), 3);
		for (group, rows) in table.groups.iter().enumerate() {
			prop_assert_eq!(rows.kind, [TableGroupKind::Head, TableGroupKind::Body, TableGroupKind::Foot][group]);
			prop_assert_eq!(rows.rows.len(), expected[group].len());
			for (row, expected) in rows.rows.iter().zip(&expected[group]) {
				let actual: Vec<_> = row.cells.iter().map(|cell| {
					assert_eq!(cell.colspan, 1);
					assert_eq!(cell.kind, if group == 0 { CellKind::Header } else { CellKind::Data });
					(cell.rowspan, cell.align, cell.blocks.iter().map(|block| match block.kind() { BlockKind::Paragraph(text) => plain_text(text), _ => panic!("cell text changed kind") }).collect::<String>())
				}).collect();
				prop_assert_eq!(&actual, expected);
			}
		}
		for (column, declared) in table.columns.iter().zip(column_alignments) {
			let first = declared.first().copied();
			let expected = first.filter(|first| declared.iter().all(|value| value == first));
			prop_assert_eq!(column.align, expected);
		}
	}

	#[test]
	fn article_metadata_collapses_only_display_fields_and_resolves_declared_url_precedence(
		title in prop::collection::vec(any::<char>(), 0..24), byline in prop::option::of(prop::collection::vec(any::<char>(), 0..24)), excerpt in prop::option::of(prop::collection::vec(any::<char>(), 0..24)),
		name in "[a-z]{1,12}", canonical in any::<bool>(), language in prop::option::of("[a-z-]{0,12}"), diagnostics in prop::collection::vec(".{0,16}", 0..8),
	) {
		let title: String = title.into_iter().collect();
		let byline: Option<String> = byline.map(|chars| chars.into_iter().collect());
		let excerpt: Option<String> = excerpt.map(|chars| chars.into_iter().collect());
		let normalized = |text: &str| text.split(blank).filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" ");
		let mut extracted = extracted("<p>body</p>");
		extracted.metadata = dom_smoothie::Metadata { title: title.clone(), byline: byline.clone(), site_name: byline.clone(), excerpt: excerpt.clone(), lang: language.clone(), dir: Some("rtl".to_owned()), image: Some(format!("image-{name}.png")), url: Some(format!("/fallback-{name}")), published_time: Some("raw published".to_owned()), modified_time: Some("raw modified".to_owned()), ..Default::default() };
		let declared = canonical.then(|| format!("https://canonical.test/{name}"));
		let metadata = crate::ArticleMetadata::from_extracted(&extracted, &url::Url::parse(BASE).unwrap(), declared.clone()).with_diagnostics(vec!["first".to_owned()]).with_diagnostics(diagnostics.clone());
		let title = normalized(&title);
		prop_assert_eq!(metadata.title, if title.is_empty() { BASE.to_owned() } else { title });
		prop_assert_eq!(metadata.byline, byline.as_deref().map(normalized));
		prop_assert_eq!(metadata.site_name, byline.as_deref().map(normalized));
		prop_assert_eq!(metadata.excerpt, excerpt.as_deref().map(normalized).filter(|text| !text.is_empty()));
		prop_assert_eq!(metadata.lang, language);
		prop_assert_eq!(metadata.dir.as_deref(), Some("rtl"));
		prop_assert_eq!(metadata.published_time.as_deref(), Some("raw published"));
		prop_assert_eq!(metadata.modified_time.as_deref(), Some("raw modified"));
		prop_assert_eq!(metadata.final_url, BASE);
		prop_assert_eq!(metadata.image, Some(format!("https://a.test/article/image-{name}.png")));
		prop_assert_eq!(metadata.canonical_url, declared.or_else(|| Some(format!("https://a.test/fallback-{name}"))));
		prop_assert_eq!(metadata.diagnostics, std::iter::once("first".to_owned()).chain(diagnostics).collect::<Vec<_>>());
	}

	#[test]
	fn html_whitespace_models_preserve_nonbreaking_and_non_ascii_spaces(
		parts in prop::collection::vec(0usize..15, 0..64),
	) {
		let alphabet = ['\t', '\n', '\u{c}', '\r', ' ', 'a', '中', '😀', '\u{a0}', '\u{2003}', '|', '&', '<', '>', '\u{b}'];
		let text: String = parts.into_iter().map(|index| alphabet[index]).collect();
		let expected = text.split(blank).filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" ");
		prop_assert_eq!(crate::metadata::collapse(&text), expected.as_str());
		prop_assert_eq!(crate::metadata::collapse_owned(&text), expected.as_str());
		let mut inline = expected;
		if text.chars().next().is_some_and(blank) && !inline.is_empty() { inline.insert(0, ' '); }
		if text.chars().next_back().is_some_and(blank) { inline.push(' '); }
		prop_assert_eq!(collapse_inline_space(&text), inline);
	}

	#[test]
	fn rich_html_and_full_extraction_preserve_literal_text_styles_and_link_occurrences(
		parts in prop::collection::vec((0u8..32, prop::collection::vec(0usize..10, 0..6), 0usize..5, 0usize..4), 0..12),
	) {
		let mut html = "<p id='fixture'>Body".to_owned();
		let mut expected: Vec<_> = "Body".chars().map(|ch| (ch, TextStyle::default(), None::<String>)).collect();
		let alphabet = ["x", "中", "<", ">", "&", "\"", "\\", "$|", "\u{a0}", "😀"];
		let spaces = [" ", "\t", "\n", "\r", "\u{c}"];
		let mut expected_links = HashSet::new();
		for (index, (flags, letters, whitespace, count)) in parts.into_iter().enumerate() {
			html.push_str(&spaces[whitespace].repeat(count));
			if count > 0 { expected.push((' ', TextStyle::default(), None)); }
			let word = format!("p{index}{}q", letters.into_iter().map(|index| alphabet[index]).collect::<String>());
			let mut markup = escape(&word);
			for (mask, tag) in [(1, "strong"), (2, "em"), (4, "code"), (8, "del")] { if flags & mask != 0 { markup = format!("<{tag}>{markup}</{tag}>"); } }
			let link = (flags & 16 != 0).then(|| format!("https://a.test/link/{index}"));
			if let Some(link) = &link { markup = format!("<a href='/link/{index}'>{markup}</a>"); expected_links.insert(link.clone()); }
			html.push_str(&markup);
			let style = TextStyle { bold: flags & 1 != 0, italic: flags & 2 != 0, code: flags & 4 != 0, strike: flags & 8 != 0, ..Default::default() };
			expected.extend(word.chars().map(|ch| (ch, style.clone(), link.clone())));
		}
		html.push_str("Tail</p>");
		expected.extend("Tail".chars().map(|ch| (ch, TextStyle::default(), None)));
		let raw = format!("<html><head><title>Title</title></head><body><article><h1>Title</h1><p>{}</p>{html}</article></body></html>", "Stable readable prose keeps this article in the extracted reading document. ".repeat(8));
		let profile = Profile { max_input_bytes: raw.len(), ..crate::PROFILE_V1.clone() };
		let public = crate::extract_with(&raw, BASE, &profile).unwrap();
		let small = Profile { max_input_bytes: raw.len() - 1, ..profile };
		prop_assert!(matches!(crate::extract_with(&raw, BASE, &small), Err(Error::InputTooLarge { .. })), "one byte below the input size must be rejected");
		for document in [lower(&html, &crate::PROFILE_V1).unwrap(), public.document] {
			let block = document.blocks().iter().find(|block| block.anchors().iter().any(|anchor| anchor == "fixture")).unwrap();
			let BlockKind::Paragraph(text) = block.kind() else { unreachable!() };
			let mut actual = Vec::new();
			let mut links = std::collections::HashMap::new();
			for inline in text {
				let InlineKind::Text(text) = &inline.kind else { panic!("literal HTML text changed semantic kind") };
				actual.extend(text.chars().map(|ch| (ch, inline.style.clone(), inline.link_url().map(str::to_owned))));
				if let Some(link) = &inline.link { links.entry(link.as_str().to_owned()).or_insert(link.occurrence()); }
			}
			prop_assert_eq!(&actual, &expected);
			prop_assert_eq!(links.keys().cloned().collect::<HashSet<_>>(), expected_links.clone());
			prop_assert_eq!(links.values().copied().collect::<HashSet<_>>().len(), expected_links.len());
			let source = document.blocks().iter().filter_map(|block| match block.kind() { BlockKind::Paragraph(text) => Some(text), _ => None }).flatten().find_map(|inline| inline.link.as_ref().filter(|link| link.as_str() == BASE)).unwrap();
			prop_assert!(!links.values().any(|&occurrence| occurrence == source.occurrence()));
		}
	}

	#[test]
	fn semantic_budgets_include_the_synthetic_title_and_source_line(
		paragraphs in prop::collection::vec("[a-zA-Z0-9]{1,24}", 1..10),
	) {
		let body = paragraphs.iter().map(|text| format!("<p>{text}</p>")).collect::<String>();
		let blocks = paragraphs.len() + 2;
		let bytes = paragraphs.iter().map(String::len).sum::<usize>() + "Title".len() + "Source: ".len() + BASE.len();
		for limit in [blocks - 1, blocks, blocks + 1] {
			let result = lower(&body, &Profile { max_blocks: limit, ..crate::PROFILE_V1.clone() });
			prop_assert_eq!(result.is_ok(), limit >= blocks);
			if let Err(error) = result { prop_assert!(matches!(error, Error::TooManyBlocks { .. }), "unexpected error: {error}"); }
		}
		for limit in [bytes - 1, bytes, bytes + 1] {
			let result = lower(&body, &Profile { max_text_bytes: limit, ..crate::PROFILE_V1.clone() });
			prop_assert_eq!(result.is_ok(), limit >= bytes);
			if let Err(error) = result { prop_assert!(matches!(error, Error::TooManyText { .. }), "unexpected error: {error}"); }
		}
	}

	#[test]
	fn table_group_order_and_column_partitions_preserve_every_cell(
		columns in 1usize..7, rows in prop::array::uniform3(1usize..4), cuts in prop::collection::vec(any::<u8>(), 1..12),
	) {
		let mut html = "<table><caption>Caption</caption>".to_owned();
		let mut expected = Vec::new();
		for group in [2, 1, 0] {
			let tag = ["thead", "tbody", "tfoot"][group];
			html.push_str(&format!("<{tag}>"));
			let mut expected_rows = Vec::new();
			for row in 0..rows[group] {
				html.push_str("<tr>");
				let mut starts = vec![0, columns];
				starts.extend(cuts.iter().map(|&cut| usize::from(cut) % (columns + 1)));
				starts.sort_unstable(); starts.dedup();
				let mut cells = Vec::new();
				for pair in starts.windows(2) {
					let span = pair[1] - pair[0];
					let text = format!("g{group}r{row}c{}", pair[0]);
					let cell = if group == 0 { "th" } else { "td" };
					html.push_str(&format!("<{cell} colspan='{span}'>{text}</{cell}>"));
					cells.push((span as u32, text));
				}
				html.push_str("</tr>"); expected_rows.push(cells);
			}
			html.push_str(&format!("</{tag}>")); expected.push((group, expected_rows));
		}
		html.push_str("</table>"); expected.sort_by_key(|(group, _)| *group);
		let capacity = rows.iter().sum::<usize>() * columns;
		let profile = Profile { max_table_cells: capacity, max_table_columns: columns, max_table_rows: rows.iter().sum(), ..crate::PROFILE_V1.clone() };
		let document = lower(&html, &profile).unwrap();
		let table = document.blocks().iter().find_map(|block| match block.kind() { BlockKind::Table(table) => Some(table), _ => None }).unwrap();
		prop_assert_eq!(table.columns.len(), columns);
		prop_assert_eq!(table.groups.len(), 3);
		prop_assert_eq!(table.caption.as_ref().map(|text| plain_text(text)), Some("Caption".to_owned()));
		for ((group, expected_rows), actual) in expected.iter().zip(&table.groups) {
			prop_assert_eq!(actual.kind, [TableGroupKind::Head, TableGroupKind::Body, TableGroupKind::Foot][*group]);
			let expected_kind = if *group == 0 { CellKind::Header } else { CellKind::Data };
			prop_assert!(actual.rows.iter().flat_map(|row| &row.cells).all(|cell| cell.rowspan == 1 && cell.kind == expected_kind));
			let actual: Vec<_> = actual.rows.iter().map(|row| row.cells.iter().map(|cell| (cell.colspan, cell.blocks.iter().map(|block| match block.kind() { BlockKind::Paragraph(text) => plain_text(text), _ => panic!("cell text changed semantic kind") }).collect::<String>())).collect::<Vec<_>>()).collect();
			prop_assert_eq!(&actual, expected_rows);
		}
		for profile in [Profile { max_table_cells: capacity - 1, ..profile.clone() }, Profile { max_table_columns: columns - 1, ..profile.clone() }, Profile { max_table_rows: rows.iter().sum::<usize>() - 1, ..profile }] {
			prop_assert!(matches!(lower(&html, &profile), Err(Error::TableTooLarge)), "one below a table budget must be rejected");
		}
	}

	#[test]
	fn colliding_table_spans_clip_to_the_first_free_slot(
		columns in 1usize..9, height in 1usize..6, start in any::<usize>(), rows in any::<usize>(), span in any::<usize>(), occupied in prop::collection::vec(any::<bool>(), 40),
	) {
		let start = start % (columns + 1);
		let rows = 1 + rows % height;
		let span = 1 + span % columns;
		let blocked: HashSet<_> = (0..height).flat_map(|row| (0..columns).map(move |column| (row, column))).filter(|&(row, column)| occupied[row * columns + column]).collect();
		let free = |column, rows, span| column + span <= columns && (0..rows).all(|row| (column..column + span).all(|column| !blocked.contains(&(row, column))));
		let expected = (start..columns).find(|&column| !blocked.contains(&(0, column))).map(|column| {
			let width = (column..columns).take_while(|&column| !blocked.contains(&(0, column))).count();
			let colspan = width.min(span).min(columns - column);
			let rowspan = (0..rows).take_while(|&row| (column..column + colspan).all(|column| !blocked.contains(&(row, column)))).count();
			(column, rowspan, colspan)
		});
		prop_assert_eq!(fit_span(start, rows, span, columns, free), expected);
	}
}
