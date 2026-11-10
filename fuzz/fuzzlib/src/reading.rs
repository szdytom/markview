//! Search hits consumed by real shaping, disclosure expansion and selection.
use std::{collections::HashMap, ops::Range};

use markview_document::{
	Document, FieldText, InlineKind, plain_text, visit_fields,
};
use markview_layout::{
	layout::LayoutOptions,
	search::{SearchIndex, SearchOptions},
	source::SourceIndex,
	text::TextSelection,
};

use crate::{coverage, oracle, pipeline};

/// Independent, non-overlapping literal scan. ASCII folding is restricted to
/// ASCII fields so Unicode case folding cannot change byte offsets.
pub fn literal_ranges(
	text: &str,
	query: &str,
	fold: bool,
) -> Vec<Range<usize>> {
	if query.is_empty() {
		return Vec::new();
	}
	let (text, query) = if fold {
		(text.to_ascii_lowercase(), query.to_ascii_lowercase())
	} else {
		(text.to_owned(), query.to_owned())
	};
	text.match_indices(&query)
		.map(|(at, value)| at..at + value.len())
		.collect()
}

/// A versioned header separates controls from Markdown. Legacy inputs remain
/// raw Markdown; structured inputs synthesize plain fields with known text.
pub fn check(data: &[u8]) {
	if data.starts_with(b"RDB1") && data.len() >= 12 {
		if data.len() <= 64 * 1024 {
			check_boundary(data);
		}
		return;
	}
	if data.len() > 32 * 1024 {
		return;
	}
	let (controls, body) = if data.starts_with(b"RDP1") && data.len() >= 12 {
		(&data[4..12], &data[12..])
	} else {
		(&[0; 8][..], data)
	};
	let payload = String::from_utf8_lossy(body);
	let mut generated_fields = None;
	let md = if controls[0] % 2 == 0 {
		payload.into_owned()
	} else {
		let words = ["alpha", "中文", "مرحبا", "e\u{301}", "👩‍💻", "office"];
		let line = body
			.iter()
			.take(128)
			.map(|b| words[usize::from(*b) % words.len()])
			.collect::<Vec<_>>()
			.join(" ");
		generated_fields = Some(vec![
			line.clone(),
			"summary alpha".into(),
			line.clone(),
			"left".into(),
			"right".into(),
			line.clone(),
			"alpha".into(),
		]);
		format!(
			"{line}\n\n<details>\n<summary>summary alpha</summary>\n\n{line}\n\n</details>\n\n| left | right |\n| --- | --- |\n| {line} | alpha |\n"
		)
	};
	let parsed = markview_commonmark::parse(md);
	if let Some(expected) = &generated_fields {
		let expected: Vec<_> = expected
			.iter()
			.filter(|text| !text.is_empty())
			.cloned()
			.collect();
		let mut actual = Vec::new();
		for block in parsed.blocks() {
			visit_fields(
				block,
				&mut Vec::new(),
				&|| false,
				&mut |_, _, text, _| {
					actual.push(match text {
						FieldText::Code(code) => code.to_owned(),
						FieldText::Rich(rich) => plain_text(rich),
					});
				},
			);
		}
		actual.retain(|text: &String| !text.is_empty());
		assert_eq!(actual, expected, "generated field ownership/order changed");
	}
	let source =
		SourceIndex::new(parsed.source.clone(), &parsed.document, &parsed.map);
	let options = LayoutOptions {
		fonts: pipeline::pinned_fonts(),
		..Default::default()
	};
	check_document(
		&parsed.document,
		&options,
		controls,
		Some(&source),
		generated_fields.is_some(),
	);
	if let Some(needle) = generated_fields
		.as_ref()
		.and_then(|fields| fields.iter().find(|text| !text.is_empty()))
	{
		check_exact_source(&parsed, &source, &options, needle);
	}
}

/// The generated body starts with plain text at source byte zero, so its first
/// line must map to the same source range and back to the same line start.
fn check_exact_source(
	parsed: &markview_commonmark::MarkdownParse,
	source: &SourceIndex,
	options: &LayoutOptions,
	needle: &str,
) {
	let mut engine = pipeline::differential_engine();
	let snapshot = engine.layout(&parsed.document, options);
	let at = parsed
		.source
		.find(needle)
		.expect("generated plain text is in the source");
	let Some(position) =
		source.source_to_preview(&snapshot, &Default::default(), at)
	else {
		panic!("generated source has no preview");
	};
	assert_eq!(
		position.source.start, at,
		"generated source lost its first byte"
	);
	let back = source
		.preview_to_source(
			&snapshot,
			&Default::default(),
			position.rect.y + position.rect.h / 2.,
		)
		.expect("generated preview maps back");
	let line_start = |offset: usize| {
		source.source_text()[..offset]
			.rfind('\n')
			.map_or(0, |newline| newline + 1)
	};
	assert_eq!(
		line_start(back.source.start),
		line_start(at),
		"generated source round trip moved to another line"
	);
	coverage::hit(coverage::Hit::ReadingSourceRoundtrip);
}

/// Directed 64 KiB boundary: padding keeps the document large while one
/// ordinary paragraph near the end stays a searchable, copyable anchor.
fn check_boundary(data: &[u8]) {
	let controls = &data[4..12];
	let body = String::from_utf8_lossy(&data[12..]);
	let target = 32 * 1024 + usize::from(controls[0]) * 128;
	let marker = format!("boundary{}", u32::from(controls[1]) % 10_000);
	let padding = "padding words keep the document near the boundary.\n\n";
	let mut md = String::with_capacity(target + 128);
	while md.len() + marker.len() + 16 < target {
		md.push_str(padding);
	}
	md.push('\n');
	md.push_str(&marker);
	md.push_str(" alpha\n\n");
	md.push_str(&body[..body.floor_char_boundary(256)]);
	let parsed = markview_commonmark::parse(md);
	let source =
		SourceIndex::new(parsed.source.clone(), &parsed.document, &parsed.map);
	let options = LayoutOptions {
		width: 720.,
		force_open: true,
		fonts: pipeline::pinned_fonts(),
		..Default::default()
	};
	let mut engine = pipeline::differential_engine();
	let mut snapshot = engine.layout(&parsed.document, &options);
	let index = SearchIndex::new(&parsed.document);
	let hits = index.find(
		&marker,
		SearchOptions {
			case_sensitive: true,
			whole_word: false,
		},
	);
	assert!(!hits.is_empty(), "boundary marker was not searchable");
	for hit in hits.iter().take(1) {
		let mut open = (*options.details_open).clone();
		for id in hit.enclosing.iter() {
			open.insert(*id, true);
		}
		snapshot.set_disclosures(&open, false);
		let selection = snapshot
			.search_selection(hit, 1)
			.expect("boundary hit has no selection");
		assert_eq!(snapshot.extract_text(selection, 1), marker);
		coverage::hit(coverage::Hit::ReadingSelection);
	}
	let at = parsed
		.source
		.find(&marker)
		.expect("boundary marker is in the source");
	let position = source
		.source_to_preview(&snapshot, &Default::default(), at)
		.expect("boundary source has a preview");
	assert!(
		position.source.start <= at && at < position.source.end,
		"boundary preview missed its source"
	);
	let back = source
		.preview_to_source(&snapshot, &Default::default(), position.rect.y)
		.expect("boundary preview maps back");
	assert!(
		back.source.start <= at && at < back.source.end,
		"boundary round trip missed its source"
	);
	coverage::hit(coverage::Hit::ReadingBoundary);
	coverage::hit(coverage::Hit::ReadingSourceRoundtrip);
}

/// Raw documents use parsed fields as the search model. Exact copy assertions
/// apply only to ordinary text; atomic projections have a different contract.
pub fn check_document(
	document: &Document,
	options: &LayoutOptions,
	controls: &[u8],
	source: Option<&SourceIndex>,
	require_selection: bool,
) {
	let byte = |at: usize| controls.get(at).copied().unwrap_or(0);
	let mut fields = Vec::new();
	for block in document.blocks() {
		visit_fields(
			block,
			&mut Vec::new(),
			&|| false,
			&mut |_, field, text, _| {
				let (text, ordinary) = match text {
					FieldText::Code(code) => (code.to_owned(), true),
					FieldText::Rich(rich) => (
						plain_text(rich),
						rich.iter().all(|inline| {
							matches!(inline.kind, InlineKind::Text(_))
						}),
					),
				};
				fields.push((block.id(), field, text, ordinary));
			},
		);
	}
	if fields.is_empty() {
		return;
	}
	let index = SearchIndex::new(document);
	let chosen = &fields[usize::from(byte(0)) % fields.len()].2;
	let query = chosen
		.split_whitespace()
		.nth(usize::from(byte(1)) % 4)
		.unwrap_or("alpha");
	// Keep a single query's work independent of the number of characters.
	let query = &query[..query.floor_char_boundary(32)];
	let fold = byte(2) % 2 != 0
		&& query.is_ascii()
		&& fields.iter().all(|f| f.2.is_ascii());
	let expected: Vec<_> = fields
		.iter()
		.flat_map(|(block, field, text, _)| {
			literal_ranges(text, query, fold)
				.into_iter()
				.map(move |range| (*block, *field, range))
		})
		.collect();
	let hits = index.find(
		query,
		SearchOptions {
			case_sensitive: !fold,
			whole_word: false,
		},
	);
	assert_eq!(
		hits.iter()
			.map(|hit| (hit.block, hit.field, hit.range.clone()))
			.collect::<Vec<_>>(),
		expected,
		"query={query:?}"
	);
	if !hits.is_empty() {
		coverage::hit(coverage::Hit::ReadingHit);
	}
	let mut engine = pipeline::differential_engine();
	let mut options = options.clone();
	let mut previous = None;
	for step in 0..1 + usize::from(byte(3) % 8) {
		if step > 0 {
			coverage::hit(coverage::Hit::ReadingReflow);
		}
		options.width =
			[80., 180., 480., 900.][usize::from(byte(4 + step % 4)) % 4];
		options.force_open = false;
		let mut snapshot = engine.layout(document, &options);
		// Search navigation opens exactly the disclosure chain of the hit.
		for hit in hits.iter().take(4) {
			let mut open = (*options.details_open).clone();
			for id in hit.enclosing.iter() {
				open.insert(*id, true);
			}
			snapshot.set_disclosures(&open, false);
			let selection = snapshot.search_selection(hit, 1);
			let ordinary = fields
				.iter()
				.find(|f| f.0 == hit.block && f.1 == hit.field)
				.unwrap();
			if require_selection {
				assert!(
					selection.is_some(),
					"unresolved generated hit: query={query:?} hit={hit:?} step={step}"
				);
			}
			if let Some(selection) = selection {
				coverage::hit(coverage::Hit::ReadingSelection);
				let actual = snapshot.extract_text(selection, 1);
				if ordinary.3 {
					assert_eq!(
						actual,
						ordinary.2[hit.range.clone()],
						"query={query:?} hit={hit:?} endpoints={selection:?} step={step} width={}",
						options.width
					);
				}
				let reversed = TextSelection {
					anchor: selection.focus,
					focus: selection.anchor,
				};
				assert_eq!(actual, snapshot.extract_text(reversed, 1));
				let horizontal: HashMap<_, _> = snapshot
					.blocks
					.iter()
					.enumerate()
					.flat_map(|(b, block)| {
						block.layout.overflow.iter().enumerate().map(
							move |(n, overflow)| {
								(
									(b, n),
									(overflow.content_width - overflow.rect.w)
										.max(0.) * 0.5,
								)
							},
						)
					})
					.collect();
				for rect in snapshot.selection_rects(selection, &horizontal, 1)
				{
					assert!(
						rect.x.is_finite()
							&& rect.y.is_finite() && rect.w.is_finite()
							&& rect.h.is_finite() && rect.w >= 0.
							&& rect.h >= 0.,
						"query={query:?} hit={hit:?} rect={rect:?}"
					);
				}
				if let Some((old, text)) = &previous
					&& snapshot.contains_selection(*old)
					&& !snapshot
						.selection_rects(*old, &horizontal, 1)
						.is_empty()
				{
					assert_eq!(
						snapshot.extract_text(*old, 1),
						*text,
						"reflow changed copy: step={step} width={} old={old:?} current={selection:?} previous={text:?}",
						options.width
					);
				}
				previous = Some((selection, actual));
			}
		}
		oracle::assert_snapshot_finite(&snapshot);
		if let Some(source) = source {
			let offset =
				usize::from(byte(step % 8)) * source.source_text().len() / 256;
			if let Some(position) =
				source.source_to_preview(&snapshot, &Default::default(), offset)
			{
				assert!(position.source.end <= source.source_text().len());
				assert!(
					source
						.source_text()
						.is_char_boundary(position.source.start)
						&& source
							.source_text()
							.is_char_boundary(position.source.end)
				);
				if let Some(back) = source.preview_to_source(
					&snapshot,
					&Default::default(),
					position.rect.y,
				) {
					assert!(back.source.end <= source.source_text().len());
					coverage::hit(coverage::Hit::ReadingSourceRoundtrip);
				}
			}
		}
	}
}
