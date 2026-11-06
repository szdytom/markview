use super::*;
use markview_document::{
	BlockKind, DocumentBuilder, DocumentRevision, ImageSpec, Inline,
	InlineKind, ResourceRef, TextStyle,
};
use markview_layout::scene::{BlockLayout, PlacedBlock};
use proptest::prelude::*;

fn resource(slot: u8, vault: bool) -> ImageSource {
	ImageSource::Resource(if vault {
		ResourceRef::vault(format!("image-{slot}"))
	} else {
		ResourceRef::document(format!("image-{slot}"))
	})
}

fn address(slot: u8, vault: bool) -> String {
	format!("{}image-{slot}", if vault { "vault:" } else { "" })
}

fn document(specs: &[(u8, bool)]) -> Document {
	let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
	let text = specs
		.iter()
		.map(|&(slot, vault)| Inline {
			kind: InlineKind::Image(Box::new(ImageSpec {
				source: resource(slot, vault),
				alt: String::new(),
				title: String::new(),
				width: None,
				height: None,
			})),
			style: TextStyle::default(),
			link: None,
		})
		.collect();
	let block = builder.block(Vec::new(), BlockKind::Paragraph(text));
	builder.finish(vec![block])
}

#[derive(Clone, Debug)]
enum Operation {
	Prepare(Vec<(u8, bool)>),
	Complete(u8, bool, u8, u8),
}

fn operations() -> impl Strategy<Value = Vec<Operation>> {
	prop::collection::vec(
		prop_oneof![
			1 => prop::collection::vec((0u8..5, any::<bool>()), 0..8).prop_map(Operation::Prepare),
			4 => (0u8..5, any::<bool>(), 0u8..5, 0u8..4).prop_map(|(slot, vault, quality, generation)| Operation::Complete(slot, vault, quality, generation)),
		],
		0..24,
	)
}

proptest! {
	#[test]
	fn image_completion_sequences_accept_each_current_request_once_and_preserve_old_snapshots(
		operations in operations(), rgba in prop::array::uniform4(any::<u8>()),
	) {
		let mut images = Images::default();
		let mut generation = 0;
		let mut sources = Vec::new();
		let mut bindings = HashMap::new();
		let mut model: HashMap<String, Option<Option<(u32, u32)>>> = HashMap::new();
		let mut previous = Vec::new();
		let initial = (0u8..4).flat_map(|slot| [(slot, false), (slot, true)]).collect();
		for operation in std::iter::once(Operation::Prepare(initial)).chain(operations) {
			match operation {
				Operation::Prepare(specs) => {
					let pixels: HashMap<_, _> = images.snapshot.decoded().iter().map(|(src, pixels)| (src.clone(), pixels.rgba.to_vec())).collect();
					previous.push((images.snapshot.clone(), pixels));
					images.prepare(&document(&specs));
					generation += 1;
					sources.clear(); model.clear(); bindings.clear();
					for (slot, vault) in specs { let key = address(slot, vault); if !sources.contains(&key) { sources.push(key.clone()); } bindings.insert(resource(slot, vault), key.clone()); model.insert(key, None); }
				}
				Operation::Complete(slot, vault, quality, epoch) => {
					let key = address(slot, vault);
					let current = epoch == 0 || epoch == 3;
					let epoch = match epoch { 0 => format!("{generation}"), 1 => format!("{}", generation - 1), 2 => "invalid".to_owned(), _ => format!("{generation:03}") };
					let width = u32::from(slot) + 1;
					let data = rgba.repeat(width as usize * 2);
					let result = match quality { 1 => Err("host failure".to_owned()), 2 => Ok(Pixels { width: 0, height: 2, rgba: Arc::from([]) }), 3 => Ok(Pixels { width: 6, height: 2, rgba: Arc::from([0; 48]) }), _ => Ok(Pixels { width, height: 2, rgba: Arc::from(if quality == 4 { data[..data.len() - 1].to_vec() } else { data }) }) };
					let accepted = current && model.get(&key).is_some_and(Option::is_none);
					prop_assert_eq!(images.complete(&epoch, &key, result, 5), accepted);
					if accepted { model.insert(key, Some((quality == 0).then_some((width, 2)))); }
				}
			}
			prop_assert_eq!(images.snapshot.generation, generation);
			prop_assert_eq!(&images.sources, &sources);
			prop_assert_eq!(&images.snapshot.keys, &bindings);
			prop_assert_eq!(images.snapshot.entries.len(), model.len());
			let decoded = images.snapshot.decoded();
			prop_assert_eq!(decoded.len(), model.values().filter(|state| matches!(state, Some(Some(_)))).count());
			for (key, state) in &model {
				let info = &images.snapshot.entries[key];
				prop_assert_eq!(info.version, if state.is_some() { generation } else { 0 });
				prop_assert_eq!(info.size, state.flatten());
				prop_assert_eq!(info.error.is_some(), *state == Some(None));
				if let Some(Some((width, height))) = state { let expected = rgba.repeat((*width * *height) as usize); prop_assert_eq!(decoded[key].rgba.as_ref(), expected.as_slice()); }
			}
			for (snapshot, expected) in &previous {
				let actual: HashMap<_, _> = snapshot.decoded().iter().map(|(src, pixels)| (src.clone(), pixels.rgba.to_vec())).collect();
				prop_assert_eq!(&actual, expected);
			}
		}
	}

	#[test]
	fn image_priorities_follow_visible_or_nearest_rectangles_across_prefixes_and_new_passes(
		boxes in prop::collection::vec((0u8..5, -100i16..301, -100i16..301, 1u8..65, 1u8..65), 0..16),
		split in any::<usize>(), top in -32i16..161,
	) {
		let mut images = Images::default();
		images.prepare(&document(&(0..5).map(|slot| (slot, false)).collect::<Vec<_>>()));
		let split = split % (boxes.len() + 1);
		for (pass, count, shift) in [(1, split, 0.0), (1, boxes.len(), 0.0), (2, boxes.len(), 128.0)] {
			let mut snapshot = LayoutSnapshot { images: images.snapshot.clone(), ..Default::default() };
			for (index, &(slot, x, y, width, height)) in boxes[..count].iter().enumerate() {
				snapshot.blocks.push(PlacedBlock { id: markview_document::BlockId::new(index as u32 + 1).unwrap(), content: Default::default(), y: shift, layout: Arc::new(BlockLayout { draws: vec![Draw::Image { src: address(slot, false), version: 0, rect: Rect { x: f32::from(x), y: f32::from(y), w: f32::from(width), h: f32::from(height) }, title: String::new() }].into(), ..Default::default() }), flow: Arc::default() });
			}
			let viewport = Rect { x: -0.5, y: f32::from(top) - 0.5, w: 65.0, h: 41.0 };
			let priorities = images.priorities(&snapshot, Some(pass), 1, viewport, &HashMap::new());
			for slot in 0..5 {
				let candidates: Vec<_> = boxes[..count].iter().filter(|&&(candidate, ..)| candidate == slot).map(|&(_, x, y, width, height)| {
					let (left, right, above, below) = (f32::from(x), f32::from(x) + f32::from(width), f32::from(y) + shift, f32::from(y) + shift + f32::from(height));
					let visible = right > viewport.x && left < viewport.x + viewport.w && below > viewport.y && above < viewport.y + viewport.h;
					let distance = if below < viewport.y { viewport.y - below } else if above > viewport.y + viewport.h { above - viewport.y - viewport.h } else { 0.0 };
					(visible, distance)
				}).collect();
				let nearest = candidates.iter().min_by(|(av, ad), (bv, bd)| bv.cmp(av).then_with(|| ad.total_cmp(bd)));
				let expected = nearest.map_or_else(Priority::default, |&(visible, distance)| Priority { region: if visible { "visible" } else if distance <= 41.0 { "near" } else { "offscreen" }, distance: Some(distance) });
				prop_assert_eq!(&priorities[&address(slot, false)], &expected);
			}
		}
	}
}
