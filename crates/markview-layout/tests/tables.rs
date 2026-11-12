// These shape with the committed subset faces, so they need a
// filesystem to read them from.
#![cfg(feature = "font-directories")]

use markview_layout::{
	fonts::FontConfig,
	layout::{LayoutEngine, LayoutOptions, LayoutSnapshot},
	scene::{BoxDecoration, Draw, Rect},
	style::{Condition, Stylesheet},
};
use std::sync::Arc;

fn fonts() -> FontConfig {
	FontConfig {
		ignore_system_fonts: true,
		directories: vec![
			std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
				.join("tests/fonts"),
		],
		..Default::default()
	}
}

fn sheet(extra: &str) -> Arc<Stylesheet> {
	let mut sheet = (*Stylesheet::bundled(false)).clone();
	sheet.merge(
		&Stylesheet::parse(&format!("format_version=2\nversion=1\n{extra}"))
			.unwrap(),
	);
	Arc::new(sheet)
}

fn cells(snapshot: &LayoutSnapshot) -> Vec<(Rect, BoxDecoration)> {
	snapshot
		.blocks
		.iter()
		.flat_map(|block| &block.layout.draws)
		.filter_map(|draw| match draw {
			Draw::Box {
				rect,
				condition: Condition::Cell | Condition::Header,
				border,
				radius,
				decoration,
				..
			} => Some((
				rect,
				decoration.as_deref().cloned().unwrap_or(BoxDecoration {
					edges: [border; 4],
					corners: [radius; 4],
					border_rects: Box::default(),
				}),
			)),
			_ => None,
		})
		.collect()
}

#[test]
fn table_grid_has_single_borders_and_mode_changes_invalidate_reuse() {
	let doc = markview_commonmark::parse(
		"| a | b |\n|---|---|\n| c | d |\n| e | f |\n",
	);
	let mut engine = LayoutEngine::new();
	for sheet in [Stylesheet::bundled(false), Stylesheet::bundled_print()] {
		let opts = LayoutOptions {
			stylesheet: sheet.clone(),
			fonts: fonts(),
			..Default::default()
		};
		let collapsed = engine.layout(&doc, &opts);
		let grid = cells(&collapsed);
		assert_eq!(grid.len(), 6);
		let width = grid[0].1.edges[0];
		assert!(width > 0.0);
		for row in 0..3 {
			for col in 0..2 {
				let (rect, cell) = grid[row * 2 + col].clone();
				if col == 0 {
					let next = grid[row * 2 + 1].clone();
					assert_eq!(rect.x + rect.w, next.0.x);
					assert_eq!(cell.edges[1] + next.1.edges[3], width);
					assert_eq!(cell.edges[3], width);
				} else {
					assert_eq!(cell.edges[1], width);
				}
				if row < 2 {
					let next = grid[(row + 1) * 2 + col].clone();
					assert!((rect.y + rect.h - next.0.y).abs() < 0.001);
					assert_eq!(cell.edges[2] + next.1.edges[0], width);
				} else {
					assert_eq!(cell.edges[2], width);
				}
			}
		}
		let mut separate = (*sheet).clone();
		separate.merge(&Stylesheet::parse(
			"format_version=2\nversion=1\n[[rule]]\nwhen=['table']\nborder_collapse='separate'"
		).unwrap());
		assert_ne!(separate.layout_key(), opts.stylesheet.layout_key());
		let separate = engine.layout(
			&doc,
			&LayoutOptions {
				stylesheet: Arc::new(separate),
				..opts
			},
		);
		for ((rect, cell), (old_rect, _)) in cells(&separate).iter().zip(grid) {
			assert_eq!(
				(rect.x, rect.y, rect.w, rect.h),
				(old_rect.x, old_rect.y, old_rect.w, old_rect.h)
			);
			assert_eq!(cell.edges, [width; 4]);
		}
		assert!(collapsed.same_reading_text(&separate));
	}
}

#[test]
fn collapsed_borders_keep_the_wider_cells_style_and_respect_row_gaps() {
	let doc = markview_commonmark::parse("| a | b |\n|---|---|\n| c | d |\n");
	let rules = "[[rule]]\nwhen=['table','cell']\nborder_edges=[2,1,1,3]\ncorner_radii=[4,4,4,4]\n\
		[[rule]]\nwhen=['table','header']\nborder_edges=[1,1,4,3]\nborder_color='#FF0000'\n";
	let mut engine = LayoutEngine::new();
	for (extra, header_bottom, body_top, corner) in [
		("", 4.0, 0.0, 0.0),
		(
			"[[rule]]\nwhen=['table','header']\nspace_after=0.5\n",
			4.0,
			2.0,
			0.0,
		),
		(
			"[[rule]]\nwhen=['table']\nborder_collapse='separate'\n",
			4.0,
			2.0,
			4.0,
		),
		(
			"[[rule]]\nwhen=['table','header']\nborder_edges=[1,1,1,3]\n",
			0.0,
			2.0,
			0.0,
		),
	] {
		let mut stylesheet = sheet(rules);
		Arc::make_mut(&mut stylesheet).merge(
			&Stylesheet::parse(&format!(
				"format_version=2\nversion=1\n{extra}"
			))
			.unwrap(),
		);
		let snapshot = engine.layout(
			&doc,
			&LayoutOptions {
				stylesheet,
				fonts: fonts(),
				..Default::default()
			},
		);
		let grid = cells(&snapshot);
		assert_eq!(grid.len(), 4);
		assert_eq!(grid[0].1.edges[2], header_bottom);
		assert_eq!(grid[2].1.edges[0], body_top);
		assert_eq!(grid[0].1.corners, [corner; 4]);
		assert_eq!(grid[1].1.edges[3], 3.0);
		assert_eq!(grid[0].1.edges[1], if corner > 0.0 { 1.0 } else { 0.0 });
	}
}

#[test]
fn staggered_grid_spans_have_one_border_per_shared_segment() {
	let doc = markview_commonmark::parse(concat!(
		"+-------+-------+-------+\n",
		"| Wide          | Tall  |\n",
		"+-------+-------+       +\n",
		"| Left  | Mid   | cell  |\n",
		"+       +-------+-------+\n",
		"| cell  | Wide again    |\n",
		"+-------+-------+-------+\n",
	));
	let mut engine = LayoutEngine::new();
	for stylesheet in [
		Stylesheet::bundled(false),
		Stylesheet::bundled(true),
		Stylesheet::bundled_print(),
	] {
		for width in [560., 260., 560.] {
			let snapshot = engine.layout(
				&doc,
				&LayoutOptions {
					width,
					stylesheet: stylesheet.clone(),
					fonts: fonts(),
					..Default::default()
				},
			);
			let grid = cells(&snapshot);
			assert_eq!(grid.len(), 5);
			let width = grid[0].1.edges[0];
			assert!(width > 0.);
			let mut horizontal = 0;
			let mut vertical = 0;
			for (a, first) in &grid {
				for (b, second) in &grid {
					if (a.y + a.h - b.y).abs() < 0.001
						&& (a.x + a.w).min(b.x + b.w) - a.x.max(b.x) > 0.001
					{
						assert_eq!(
							first.edges[2] + second.edges[0],
							width,
							"horizontal shared border: {a:?}, {b:?}"
						);
						horizontal += 1;
					}
					if (a.x + a.w - b.x).abs() < 0.001
						&& (a.y + a.h).min(b.y + b.h) - a.y.max(b.y) > 0.001
					{
						assert_eq!(
							first.edges[1] + second.edges[3],
							width,
							"vertical shared border: {a:?}, {b:?}"
						);
						vertical += 1;
					}
				}
			}
			assert_eq!((horizontal, vertical), (4, 4));
		}
	}
}

#[test]
fn a_spanning_border_changes_owner_without_losing_width_or_color() {
	use markview_layout::{scene::Paint, style::ColorField};
	let doc = markview_commonmark::parse(concat!(
		"+-------+-------+\n",
		"| Tall  | Top   |\n",
		"+       +-------+\n",
		"| cell  | Mid   |\n",
		"+       +-------+\n",
		"|       | Last  |\n",
		"+-------+-------+\n",
	));
	let stylesheet = sheet(concat!(
		"[[rule]]\nwhen=['table','cell']\nborder_width=1\nbackground='#FFFFFF'\n",
		"[[rule]]\nwhen=['table','first_child','cell']\nborder_width=2\nborder_color='#FF0000'\n",
		"[[rule]]\nwhen=['table','last_child','cell']\nborder_width=3\nborder_color='#0000FF'\n",
	));
	let mut engine = LayoutEngine::new();
	for width in [400., 80., 400.] {
		let snapshot = engine
			.layout(
				&doc,
				&LayoutOptions {
					width,
					fonts: fonts(),
					stylesheet: stylesheet.clone(),
					..Default::default()
				},
			)
			.flattened();
		let grid = cells(&snapshot);
		assert_eq!(grid.len(), 4);
		let tall = grid[0].0;
		assert_eq!(
			grid[0].1.edges, [0.; 4],
			"partially owned sides use border rectangles"
		);
		let draws = &snapshot.blocks[0].layout.draws;
		let segments: Vec<_> = draws
			.iter()
			.flat_map(|draw| match draw {
				Draw::Box {
					rect,
					chain,
					condition,
					decoration: Some(decoration),
					..
				} => decoration
					.border_rects
					.iter()
					.map(|edge| {
						(
							Rect {
								x: rect.x + edge.x,
								y: rect.y + edge.y,
								..*edge
							},
							Paint::Scoped(
								chain,
								condition,
								ColorField::BorderColor,
							),
						)
					})
					.collect(),
				_ => Vec::new(),
			})
			.collect();
		assert!(!segments.is_empty());
		let mut chain = None;
		for (_, paint) in &segments {
			let Paint::Scoped(c, _, _) = paint else {
				unreachable!()
			};
			assert_eq!(
				chain.get_or_insert(*c),
				c,
				"each run keeps its owner's style"
			);
			assert_eq!(stylesheet.paint(*paint), [1., 0., 0., 1.]);
		}
		for (row, (rect, edge)) in grid[1..].iter().enumerate() {
			let y = rect.y + rect.h / 2.;
			let left: f32 = segments
				.iter()
				.filter(|(r, _)| {
					(r.x + r.w - (tall.x + tall.w)).abs() < 0.001
						&& r.y <= y
						&& y < r.y + r.h
				})
				.map(|(r, _)| r.w)
				.sum();
			assert_eq!(left + edge.edges[3], if row == 2 { 3. } else { 2. });
			assert_eq!(left, if row == 2 { 0. } else { 2. });
		}
	}
}

#[test]
fn a_spanning_border_survives_spacing_beside_a_wider_neighbor() {
	let doc = markview_commonmark::parse(concat!(
		"+-------+-------+\n| Tall  | Top   |\n+       +-------+\n",
		"|       | Mid   |\n+       +-------+\n|       | Last  |\n+-------+-------+\n",
	));
	for spacing in ["space_before", "space_after"] {
		let stylesheet = sheet(&format!(
			concat!(
				"[[rule]]\nwhen=['table','cell']\nborder_width=2\n",
				"[[rule]]\nwhen=['table','last_child','cell']\nborder_width=3\n{}=0.5\n",
			),
			spacing
		));
		let opts = LayoutOptions {
			fonts: fonts(),
			stylesheet,
			..Default::default()
		};
		let snapshot = LayoutEngine::new().layout(&doc, &opts).flattened();
		let grid = cells(&snapshot);
		let (tall, decoration) = &grid[0];
		let last = grid[3].0;
		let gap_y = if spacing == "space_before" {
			last.y - opts.font_size * 0.25
		} else {
			last.y + last.h + opts.font_size * 0.25
		};
		let widths_at = |y| {
			decoration.edges[1]
				+ decoration
					.border_rects
					.iter()
					.filter(|edge| {
						(edge.x + edge.w - tall.w).abs() < 0.001
							&& edge.y <= y - tall.y
							&& y - tall.y < edge.y + edge.h
					})
					.map(|edge| edge.w)
					.sum::<f32>()
		};
		assert_eq!(
			widths_at(gap_y),
			2.,
			"the exposed spanning edge remains beside {spacing}"
		);
		assert_eq!(
			widths_at(last.y + last.h / 2.),
			0.,
			"the wider neighboring edge owns the overlap"
		);
	}
}
