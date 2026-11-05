use super::*;
use krilla::page::PageSettings;
use markview_layout::document::{BlockId, FootnoteId, LinkOccurrenceId};
use proptest::prelude::*;

fn serialize(
	annotations: Vec<Annotation>,
	geometry: &PageGeometry,
) -> lopdf::Document {
	let mut document = krilla::Document::new();
	let mut first = Some(annotations);
	for _ in 0..3 {
		let mut page = document.start_page_with(
			PageSettings::from_wh(geometry.width_pt, geometry.height_pt)
				.unwrap(),
		);
		if let Some(annotations) = first.take() {
			for annotation in annotations {
				page.add_annotation(annotation);
			}
		}
		page.finish();
	}
	lopdf::Document::load_mem(&document.finish().unwrap()).unwrap()
}

fn resolve<'a>(
	pdf: &'a lopdf::Document,
	object: &'a lopdf::Object,
) -> &'a lopdf::Object {
	match object {
		lopdf::Object::Reference(id) => pdf.get_object(*id).unwrap(),
		_ => object,
	}
}

fn destination(
	pdf: &lopdf::Document,
	annotation: &lopdf::Dictionary,
) -> (lopdf::ObjectId, f32, f32) {
	let target = resolve(pdf, annotation.get(b"Dest").unwrap())
		.as_array()
		.unwrap();
	assert_eq!(target[1].as_name().unwrap(), b"XYZ");
	(
		target[0].as_reference().unwrap(),
		target[2].as_float().unwrap(),
		target[3].as_float().unwrap(),
	)
}

proptest! {
	#[test]
	fn serialized_link_annotations_preserve_typed_destinations_page_units_and_external_uris(
		name in "[a-z]{1,12}", page in 0usize..3, y in 0u16..257, left in 0u16..65, top in 0u16..65, x in 0u16..129, rect_y in 0u16..129, width in 1u16..65, height in 1u16..65, has_reference in any::<bool>(),
	) {
		let geometry = PageGeometry { width_pt: 400.0, height_pt: 600.0, margin_pt: [f32::from(top), 10.0, 10.0, f32::from(left)] };
		let name = format!("{name}-中😀");
		let encoded = percent_encoding::utf8_percent_encode(&name, percent_encoding::NON_ALPHANUMERIC).to_string();
		let note = FootnoteId::new(name.clone());
		let mut pagination = Pagination::default();
		pagination.anchors.insert(name, (page, f32::from(y)));
		pagination.targets.insert(AnchorId::FootnoteDefinition(note.clone()), (page, f32::from(y)));
		let reference = ((page + 1) % 3, f32::from(y) + 8.0);
		if has_reference { pagination.targets.insert(AnchorId::FootnoteReference { definition: note.clone(), occurrence: LinkOccurrenceId::new(7).unwrap() }, reference); }
		let rect = Rect::from_xywh(f32::from(x), f32::from(rect_y), f32::from(width), f32::from(height)).unwrap();
		let external = format!("https://a.test/{encoded}?q=(x)");
		let targets = [LinkTarget::Url(format!("#{encoded}").into()), LinkTarget::FootnoteDefinition(note.clone()), LinkTarget::FootnoteReturn(note), LinkTarget::Url(external.clone().into())];
		let annotations = targets.iter().enumerate().map(|(index, target)| annotation(rect, target, if index == 3 { &external } else { "source-label" }, &pagination, &geometry).unwrap()).collect();
		let pdf = serialize(annotations, &geometry);
		let annotations = pdf.get_page_annotations(pdf.get_pages()[&1]).unwrap();
		prop_assert_eq!(annotations.len(), 4);
		for (index, &(target_page, target_y)) in [(page, f32::from(y)), (page, f32::from(y)), if has_reference { reference } else { (page, f32::from(y)) }].iter().enumerate() {
			let (actual_page, actual_x, actual_y) = destination(&pdf, annotations[index]);
			prop_assert_eq!(actual_page, pdf.get_pages()[&(target_page as u32 + 1)]);
			prop_assert!((actual_x - f32::from(left)).abs() <= 0.001);
			prop_assert!((actual_y - (600.0 - f32::from(top) - target_y * 0.75)).abs() <= 0.001);
		}
		let action = resolve(&pdf, annotations[3].get(b"A").unwrap()).as_dict().unwrap();
		prop_assert_eq!(action.get(b"S").unwrap().as_name().unwrap(), b"URI");
		prop_assert_eq!(lopdf::decode_text_string(action.get(b"URI").unwrap()).unwrap(), external);
		for annotation in annotations {
			let coordinates: Vec<_> = annotation.get(b"Rect").unwrap().as_array().unwrap().iter().map(|value| value.as_float().unwrap()).collect();
			prop_assert_eq!(coordinates, vec![f32::from(x), 600.0 - f32::from(rect_y + height), f32::from(x + width), 600.0 - f32::from(rect_y)]);
		}
		for (target, display) in [(LinkTarget::Url("#missing".into()), "#missing"), (LinkTarget::FootnoteDefinition(FootnoteId::new("missing")), "legacy"), (LinkTarget::Disclosure(BlockId::new(1).unwrap()), ""), (LinkTarget::Url("".into()), "")] {
			prop_assert!(annotation(rect, &target, display, &pagination, &geometry).is_none());
		}
	}

	#[test]
	fn footnote_returns_choose_the_earliest_reference_in_document_order(label in "[a-z]{1,8}", occurrence in 1u32..128, y in 0u16..65, first_page in 0usize..2, same_page in any::<bool>()) {
		let note = FootnoteId::new(label);
		let mut pagination = Pagination::default();
		for occurrence in [occurrence, occurrence + 1] { pagination.targets.insert(AnchorId::FootnoteReference { definition: note.clone(), occurrence: LinkOccurrenceId::new(occurrence).unwrap() }, (first_page, f32::from(y))); }
		// The first bucket deliberately names the later reference.
		let first_bucket = pagination.targets.keys().next().unwrap().clone();
		*pagination.targets.get_mut(&first_bucket).unwrap() = (first_page + usize::from(!same_page), f32::from(y) + 8.0);
		let geometry = PageGeometry { width_pt: 400.0, height_pt: 600.0, margin_pt: [10.0; 4] };
		let annotation = annotation(Rect::from_xywh(10.0, 10.0, 20.0, 10.0).unwrap(), &LinkTarget::FootnoteReturn(note), "legacy", &pagination, &geometry).unwrap();
		let pdf = serialize(vec![annotation], &geometry);
		let annotations = pdf.get_page_annotations(pdf.get_pages()[&1]).unwrap();
		let (page, _, actual_y) = destination(&pdf, annotations[0]);
		prop_assert_eq!(page, pdf.get_pages()[&(first_page as u32 + 1)]);
		prop_assert!((actual_y - (600.0 - 10.0 - f32::from(y) * PT_PER_PX)).abs() <= 0.001);
	}
}
