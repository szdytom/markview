// These shape with the committed subset faces, so they need a
// filesystem to read them from.
#![cfg(feature = "font-directories")]

use markview_core::{
	document,
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
				*rect,
				decoration.unwrap_or(BoxDecoration {
					edges: [*border; 4],
					corners: [*radius; 4],
				}),
			)),
			_ => None,
		})
		.collect()
}

#[test]
fn table_grid_has_single_borders_and_mode_changes_invalidate_reuse() {
	let doc = document::parse("| a | b |\n|---|---|\n| c | d |\n| e | f |\n");
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
				let (rect, cell) = grid[row * 2 + col];
				if col == 0 {
					let next = grid[row * 2 + 1];
					assert_eq!(rect.x + rect.w, next.0.x);
					assert_eq!(cell.edges[1] + next.1.edges[3], width);
					assert_eq!(cell.edges[3], width);
				} else {
					assert_eq!(cell.edges[1], width);
				}
				if row < 2 {
					let next = grid[(row + 1) * 2 + col];
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
	let doc = document::parse("| a | b |\n|---|---|\n| c | d |\n");
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
