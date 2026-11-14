use markview_commonmark::{Dialect, parse_with};
use markview_document::{BlockKind, CalloutKind, plain_text};
use markview_layout::{
	fonts::FontConfig,
	layout::{LayoutEngine, LayoutOptions},
	scene::Draw,
	search::{SearchIndex, SearchOptions},
	style::{Condition, Stylesheet},
};
use std::{path::Path, sync::Arc};

fn options(dark: bool) -> LayoutOptions {
	LayoutOptions {
		width: 240.,
		stylesheet: Stylesheet::bundled(dark),
		fonts: FontConfig {
			ignore_system_fonts: true,
			directories: vec![
				Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts"),
			],
			..Default::default()
		},
		..Default::default()
	}
}

#[test]
fn both_dialects_draw_vector_types_and_keep_icons_out_of_copy() {
	for ofm in [false, true] {
		let types = if ofm {
			&[
				("note", CalloutKind::Note, Condition::CalloutNote),
				("abstract", CalloutKind::Abstract, Condition::CalloutTip),
				("info", CalloutKind::Info, Condition::CalloutNote),
				("todo", CalloutKind::Todo, Condition::CalloutNote),
				("tip", CalloutKind::Tip, Condition::CalloutTip),
				("success", CalloutKind::Success, Condition::CalloutSuccess),
				("question", CalloutKind::Question, Condition::CalloutWarning),
				("warning", CalloutKind::Warning, Condition::CalloutWarning),
				("failure", CalloutKind::Failure, Condition::CalloutDanger),
				("danger", CalloutKind::Danger, Condition::CalloutDanger),
				("bug", CalloutKind::Bug, Condition::CalloutDanger),
				("example", CalloutKind::Example, Condition::CalloutExample),
				("quote", CalloutKind::Quote, Condition::CalloutQuote),
				("important", CalloutKind::Tip, Condition::CalloutTip),
				("caution", CalloutKind::Warning, Condition::CalloutWarning),
			][..]
		} else {
			&[
				("NOTE", CalloutKind::Note, Condition::CalloutNote),
				("TIP", CalloutKind::Tip, Condition::CalloutTip),
				(
					"IMPORTANT",
					CalloutKind::Important,
					Condition::CalloutExample,
				),
				("WARNING", CalloutKind::Warning, Condition::CalloutWarning),
				("CAUTION", CalloutKind::Caution, Condition::CalloutDanger),
			][..]
		};
		for dark in [false, true] {
			let options = options(dark);
			for &(name, kind, family) in types {
				let source = format!("> [!{name}]\n> Body.\n");
				let doc = parse_with(
					source,
					Dialect {
						ofm,
						..Default::default()
					},
				);
				let BlockKind::Quote {
					callout: Some(callout),
					..
				} = doc.blocks()[0].kind()
				else {
					panic!("expected callout");
				};
				assert_eq!(callout.kind, kind);
				let snapshot =
					LayoutEngine::new().layout(&doc, &options).flattened();
				let draws = &snapshot.blocks[0].layout.draws;
				let icons: Vec<_> = draws
					.iter()
					.filter_map(|draw| match draw {
						Draw::Icon { paths, paint, .. } => Some((paths, paint)),
						_ => None,
					})
					.collect();
				assert_eq!(icons.len(), 1, "{name}");
				assert!(!icons[0].0.is_empty());
				assert!(draws.iter().any(
					|d| matches!(d, Draw::Box { condition, .. } if condition == family)
				));
				let color = options.stylesheet.paint(icons[0].1);
				assert!(draws.iter().any(|d| matches!(d, Draw::Glyph(g) if options.stylesheet.paint(g.paint) == color)));
				let selection = snapshot.select_all(1).unwrap();
				assert_eq!(
					snapshot.extract_text(selection, 1),
					format!(
						"{}\n\nBody.",
						plain_text(
							callout
								.title
								.as_ref()
								.expect("a plain callout owns its title")
						)
					)
				);
			}
		}
	}
}

#[test]
fn folded_headers_wrap_toggle_and_share_the_unfolded_title_style() {
	let title =
		"A long title that wraps onto several lines at a narrow reading width";
	let source = format!(
		"> [!bug] {title}\n> Body.\n\n> [!bug]- {title}\n> Hidden.\n> > [!tip] Nested\n> > Inside.\n"
	);
	let doc = parse_with(
		source,
		Dialect {
			ofm: true,
			..Default::default()
		},
	);
	let mut engine = LayoutEngine::new();
	let mut options = options(true);
	let closed = engine.layout(&doc, &options);
	let flat = closed.flattened();
	let icon = |block: usize| {
		flat.blocks[block]
			.layout
			.draws
			.iter()
			.find_map(|draw| match draw {
				Draw::Icon {
					paths, paint, size, ..
				} => Some((paths.as_ptr(), paint, size)),
				_ => None,
			})
			.unwrap()
	};
	assert_eq!(icon(0), icon(1));
	let headers: Vec<_> = flat
		.blocks
		.iter()
		.map(|block| &block.layout.text[0])
		.collect();
	for header in &headers {
		let top = header.clusters.at(0).rect.y;
		assert!(
			header
				.clusters
				.iter()
				.any(|cluster| cluster.rect.y > top + options.font_size)
		);
	}
	let link = flat.blocks[1]
		.layout
		.links
		.iter()
		.find(|l| l.url.starts_with("details:"))
		.unwrap();
	assert!(link.rect.h > options.font_size * 2.);
	let selection = closed.select_all(1).unwrap();
	assert!(!closed.extract_text(selection, 1).contains("Hidden"));
	let BlockKind::Quote { blocks, .. } = doc.blocks()[1].kind() else {
		panic!();
	};
	options.details_open = Arc::new([(blocks[0].id(), true)].into());
	let opened = engine.layout(&doc, &options);
	let text = opened.extract_text(opened.select_all(1).unwrap(), 1);
	assert!(
		text.contains("Hidden.")
			&& text.contains("Nested")
			&& text.contains("Inside."),
		"{text}"
	);
	assert!(opened.height > closed.height);
	options.force_open = true;
	options.details_open = Arc::default();
	assert_eq!(engine.layout(&doc, &options).height, opened.height);
}

#[test]
fn callout_corners_default_to_square_and_follow_mvss() {
	for theme in ["builtin", "light", "dark", "8-bit", "print"] {
		let mut base = (*Stylesheet::builtin()).clone();
		if theme != "builtin" {
			base.merge(&Stylesheet::named_rules(theme).unwrap());
		}
		for (rule, expected) in [
			("", [0.; 4]),
			("radius=6", [6.; 4]),
			("radius=6\ncorner_radii=[0,3,6,9]", [0., 3., 6., 9.]),
		] {
			let mut sheet = base.clone();
			if !rule.is_empty() {
				sheet.merge(
					&Stylesheet::parse(&format!(
						"format_version=2\nversion=1\n[[rule]]\nwhen=['callout']\n{rule}"
					))
					.unwrap(),
				);
			}
			let options = LayoutOptions {
				stylesheet: Arc::new(sheet),
				..options(false)
			};
			for ofm in [false, true] {
				let doc = parse_with(
					"> [!NOTE]\n> Body.\n",
					Dialect {
						ofm,
						..Default::default()
					},
				);
				let snapshot =
					LayoutEngine::new().layout(&doc, &options).flattened();
				let corners = snapshot.blocks[0]
					.layout
					.draws
					.iter()
					.find_map(|draw| match draw {
						Draw::Box {
							condition: Condition::CalloutNote,
							decoration: Some(decoration),
							..
						} => Some(decoration.corners),
						_ => None,
					})
					.unwrap();
				assert_eq!(corners, expected, "{theme}: {rule}");
			}
		}
	}
}

#[test]
fn callouts_inherit_quote_line_widths_across_themes_and_mvss_overrides() {
	for theme in std::iter::once("builtin").chain(Stylesheet::bundled_ids()) {
		for rule in ["", "border_width=4.25", "border_edges=[0,0,0,5.5]"] {
			let mut sheet = (*Stylesheet::builtin()).clone();
			if theme != "builtin" {
				sheet.merge(&Stylesheet::named_rules(theme).unwrap());
			}
			if !rule.is_empty() {
				sheet.merge(
					&Stylesheet::parse(&format!(
						"format_version=2\nversion=1\n[[rule]]\nwhen=['blockquote']\n{rule}"
					))
					.unwrap(),
				);
			}
			let options = LayoutOptions {
				stylesheet: Arc::new(sheet),
				..options(false)
			};
			for ofm in [false, true] {
				let doc = parse_with(
					"> Ordinary quote.\n\n> [!NOTE]\n> Body.\n",
					Dialect {
						ofm,
						..Default::default()
					},
				);
				let snapshot =
					LayoutEngine::new().layout(&doc, &options).flattened();
				let widths: Vec<_> = snapshot
					.blocks
					.iter()
					.map(|block| {
						block
							.layout
							.draws
							.iter()
							.find_map(|draw| match draw {
								Draw::Box {
									border,
									decoration,
									left_only: true,
									..
								} => Some(
									decoration
										.as_deref()
										.map_or(border, |d| d.edges[3]),
								),
								_ => None,
							})
							.unwrap()
					})
					.collect();
				assert_eq!(widths.len(), 2);
				assert_eq!(widths[0], widths[1], "{theme}: {rule}");
			}
		}
	}
}

#[test]
fn plain_callout_titles_are_searchable_fields() {
	let doc = parse_with(
		"> [!note] Heading Words\n> Body.\n",
		Dialect {
			ofm: true,
			..Default::default()
		},
	);
	let index = SearchIndex::new(&doc.document);
	assert_eq!(
		index.find("Heading Words", SearchOptions::default()).len(),
		1
	);
}

#[test]
fn callout_headers_render_math_in_plain_and_folded_titles() {
	let doc = parse_with(
		"> [!note] $a^2$\n> Body.\n\n> [!note]- $b$\n> Hidden.\n",
		Dialect {
			ofm: true,
			..Default::default()
		},
	);
	let snapshot = LayoutEngine::new()
		.layout(&doc, &options(false))
		.flattened();
	assert_eq!(snapshot.math_errors, 0);
	assert_eq!(snapshot.blocks.len(), 2);
	for block in &snapshot.blocks {
		assert!(
			block
				.layout
				.draws
				.iter()
				.any(|draw| matches!(draw, Draw::Math { .. })),
			"callout title did not draw math"
		);
	}
}
