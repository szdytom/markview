use super::*;
use markview_layout::document::BlockId;
use proptest::prelude::*;

fn dictionary(
	pdf: &lopdf::Document,
	id: lopdf::ObjectId,
) -> &lopdf::Dictionary {
	pdf.get_dictionary(id).unwrap()
}

fn reference(dict: &lopdf::Dictionary, key: &[u8]) -> Option<lopdf::ObjectId> {
	dict.get(key)
		.ok()
		.map(|value| value.as_reference().unwrap())
}

fn walk(
	pdf: &lopdf::Document,
	parent: lopdf::ObjectId,
	parent_index: Option<usize>,
	output: &mut Vec<(lopdf::ObjectId, Option<usize>)>,
) {
	let dict = dictionary(pdf, parent);
	let mut next = reference(dict, b"First");
	let mut previous = None;
	while let Some(id) = next {
		assert!(output.len() < 64, "outline contains a cycle");
		let node = dictionary(pdf, id);
		assert_eq!(reference(node, b"Parent"), Some(parent));
		assert_eq!(reference(node, b"Prev"), previous);
		let index = output.len();
		output.push((id, parent_index));
		walk(pdf, id, Some(index), output);
		previous = Some(id);
		next = reference(node, b"Next");
	}
	assert_eq!(reference(dict, b"Last"), previous);
}

proptest! {
	#[test]
	fn serialized_bookmarks_preserve_order_parents_unicode_and_destinations(
		headings in prop::collection::vec((1u8..=6, any::<bool>(), 0usize..3, 0u16..400), 0..64),
		left in 0u16..80, top in 0u16..80,
	) {
		let entries: Vec<_> = headings.iter().enumerate().map(|(i, &(level, _, _, _))| OutlineEntry {
			block: BlockId::new(i as u32 + 1).unwrap(), level,
			text: format!("标题 😀 ({})", i % 3), anchor: format!("heading-{i}"),
		}).collect();
		let geometry = PageGeometry { width_pt: 500.0, height_pt: 700.0, margin_pt: [f32::from(top), 10.0, 10.0, f32::from(left)] };
		let mut pagination = Pagination::default();
		for (entry, &(_, present, page, y)) in entries.iter().zip(&headings) {
			if present { pagination.anchors.insert(entry.anchor.clone(), (page, f32::from(y))); }
		}
		let mut document = krilla::Document::new();
		if let Some(outline) = build(&entries, &pagination, &geometry) { document.set_outline(outline); }
		for _ in 0..3 {
			document.start_page_with(krilla::page::PageSettings::from_wh(500.0, 700.0).unwrap()).finish();
		}
		let pdf = lopdf::Document::load_mem(&document.finish().unwrap()).unwrap();
		let root = pdf.catalog().unwrap().get(b"Outlines").ok().map(|value| value.as_reference().unwrap());
		let expected: Vec<_> = entries.iter().zip(&headings).filter(|(_, h)| h.1).collect();
		prop_assert_eq!(root.is_none(), expected.is_empty());
		if let Some(root) = root {
			let mut actual = Vec::new();
			walk(&pdf, root, None, &mut actual);
			prop_assert_eq!(actual.len(), expected.len());
			for (i, ((id, parent), (entry, heading))) in actual.iter().zip(&expected).enumerate() {
				let expected_parent = (0..i).rev().find(|&j| expected[j].0.level < entry.level);
				prop_assert_eq!(*parent, expected_parent);
				let node = dictionary(&pdf, *id);
				prop_assert_eq!(lopdf::decode_text_string(node.get(b"Title").unwrap()).unwrap(), entry.text.clone());
				let dest = pdf.dereference(node.get(b"Dest").unwrap()).unwrap().1.as_array().unwrap();
				prop_assert_eq!(dest[0].as_reference().unwrap(), pdf.get_pages()[&(heading.2 as u32 + 1)]);
				prop_assert_eq!(dest[1].as_name().unwrap(), b"XYZ");
				prop_assert!((dest[2].as_float().unwrap() - f32::from(left)).abs() < 0.001);
				prop_assert!((dest[3].as_float().unwrap() - (700.0 - f32::from(top) - f32::from(heading.3) * PT_PER_PX)).abs() < 0.001);
			}
		}
	}
}
