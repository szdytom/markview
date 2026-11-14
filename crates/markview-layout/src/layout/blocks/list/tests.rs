use super::*;
use crate::{fonts::FontConfig, layout::LayoutEngine, style::Stylesheet};
use proptest::prelude::*;
use std::cell::Cell;

fn items(numbers: &[Option<isize>], checked: &[Option<bool>]) -> Vec<ListItem> {
	numbers
		.iter()
		.zip(checked)
		.map(|(number, checked)| ListItem {
			number: *number,
			checked: *checked,
			blocks: Vec::new(),
		})
		.collect()
}

fn integer() -> impl Strategy<Value = isize> {
	prop_oneof![Just(isize::MIN), Just(isize::MAX), any::<isize>()]
}

proptest! {
	#[test]
	fn numbering_matches_bounded_integer_arithmetic(
		start in prop::option::of(integer()),
		reversed in any::<bool>(),
		entries in prop::collection::vec((prop::option::of(integer()), prop::option::of(any::<bool>())), 0..40),
	) {
		let (numbers, checked): (Vec<_>, Vec<_>) = entries.into_iter().unzip();
		let items = items(&numbers, &checked);
		let actual = list_values(start, reversed, &items);
		let expected: Vec<_> = start.map_or_else(Vec::new, |start| {
			let mut base = start as i128;
			let mut distance = 0;
			numbers.iter().map(|number| {
				if let Some(number) = number {
					base = *number as i128;
					distance = 0;
				}
				let value = base + if reversed { -distance } else { distance };
				distance += 1;
				value.clamp(isize::MIN as i128, isize::MAX as i128) as isize
			}).collect()
		});
		prop_assert_eq!(&actual, &expected);
		let unchecked = items.iter().cloned().map(|mut item| {
			item.checked = None;
			item
		}).collect::<Vec<_>>();
		prop_assert_eq!(actual, list_values(start, reversed, &unchecked));
	}
}

#[test]
fn numbering_saturates_and_task_items_keep_their_place() {
	let items = items(
		&[None, None, Some(-2), None],
		&[None, Some(true), Some(false), None],
	);
	assert_eq!(
		list_values(Some(isize::MAX), false, &items),
		[isize::MAX, isize::MAX, -2, -1]
	);
	assert_eq!(
		list_values(Some(isize::MIN), true, &items),
		[isize::MIN, isize::MIN, -2, -3]
	);
	assert!(list_values(None, false, &items).is_empty());
	assert!(list_values(Some(1), false, &[]).is_empty());
}

fn options() -> LayoutOptions {
	LayoutOptions {
		width: 320.,
		fonts: FontConfig::from_faces(
			0x6c697374,
			vec![parley::fontique::Blob::new(Arc::new(
				include_bytes!(concat!(
					env!("CARGO_MANIFEST_DIR"),
					"/tests/fonts/NotoSerif-Regular-subset.otf"
				))
				.as_slice(),
			))],
		),
		..Default::default()
	}
}

#[test]
fn mixed_lists_keep_marker_depths_copy_text_and_number_selection_groups() {
	let document = markview_commonmark::parse(
		"- Outer\n  1. First\n     - [x] Done\n     - Nested\n       1. Deep\n  2. Second\n- Last\n\n1. Next\n\n- Final\n",
	);
	let mut opts = options();
	let mut sheet = (*opts.stylesheet).clone();
	sheet.merge(&Stylesheet::parse("format_version=3\nversion=1\n[[rule]]\nwhen=['marker']\nshape=['square','triangle']\n[[rule]]\nwhen=['enum']\nnumbering='1.a.'\n[[rule]]\nwhen=['task_marker']\nradius=0\nborder_width=1\n").unwrap());
	opts.stylesheet = Arc::new(sheet);
	let snapshot = LayoutEngine::new().layout(&document, &opts);
	assert_eq!(
		snapshot.extract_text(snapshot.select_all(1).unwrap(), 1),
		"Outer\n1. First\nDone\nNested\na. Deep\n2. Second\nLast\n1. Next\nFinal"
	);
	let flat = snapshot.flattened();
	let shapes: Vec<_> = flat
		.blocks
		.iter()
		.flat_map(|b| b.layout.draws.iter())
		.filter_map(|draw| {
			if let Draw::Polygon { points, .. } = draw {
				Some(points.len())
			} else {
				None
			}
		})
		.collect();
	assert_eq!(shapes, [4, 4, 4, 10, 6, 3, 4, 4]);
	let mut numbers = 0;
	for block in &flat.blocks {
		for pair in block.layout.text.windows(2) {
			if ["1. ", "2. ", "a. "].contains(&pair[0].text.as_str()) {
				assert!(pair[0].selection_group.is_some());
				assert_eq!(pair[0].selection_group, pair[1].selection_group);
				assert_eq!(pair[1].separator, "");
				numbers += 1;
			}
		}
	}
	assert_eq!(numbers, 4);
}

#[test]
fn cancelled_and_empty_lists_restore_depths_and_appearance() {
	let opts = options();
	for source in [
		"- Outer\n  1. Inner\n     - [x] Task\n- Last\n",
		"1. Outer\n   - Inner\n     1. Deep\n2. Last\n",
	] {
		let document = markview_commonmark::parse(source);
		let block = &document.blocks()[0];
		for limit in 0..24 {
			let calls = Cell::new(0);
			let cancelled = || {
				let call = calls.get();
				calls.set(call + 1);
				call >= limit
			};
			let mut engine = LayoutEngine::new();
			engine.shaper.set_fonts(&opts.fonts);
			engine.shaper.set_stylesheet(opts.stylesheet.clone());
			let images = Default::default();
			let mut context = BlockContext {
				nested: None,
				cancelled: &cancelled,
				fields: super::super::FieldCursor::new(block),
				shaper: &mut engine.shaper,
				math: &mut engine.math,
				images: &images,
				highlight_cache: engine.highlights.results(),
				marker_depth: 2,
				enum_depth: 3,
				footnote_column: 1,
			};
			let appearance = format!("{:?}", context.shaper.appearance);
			let mut out = BlockLayout::default();
			out.begin_scene(0., 0.);
			let height =
				context.block_inner(block, 0., 0., opts.width, &opts, &mut out);
			out.end_scene(height, f32::NEG_INFINITY);
			assert_eq!(
				(context.marker_depth, context.enum_depth),
				(2, 3),
				"limit {limit}"
			);
			assert_eq!(
				format!("{:?}", context.shaper.appearance),
				appearance,
				"limit {limit}"
			);
			// An empty list must restore the same state without entering an item.
			context.list(
				Some(1),
				false,
				&[],
				[0., height],
				opts.width,
				&opts,
				&mut out,
			);
			context.list(
				None,
				false,
				&[],
				[0., height],
				opts.width,
				&opts,
				&mut out,
			);
			assert_eq!((context.marker_depth, context.enum_depth), (2, 3));
			assert_eq!(format!("{:?}", context.shaper.appearance), appearance);
		}
	}
}
