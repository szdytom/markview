use super::*;
use markview_layout::{
	document::{Affinity, BlockId, DocumentRevision, FieldId},
	scene::LinkTarget,
};
use proptest::prelude::*;

#[derive(Default)]
struct State {
	cursor: Point,
	modifiers: Modifiers,
	selection: Option<TextSelection>,
	drag: Option<Drag>,
	dragged: bool,
	click: Option<(Instant, Point, u8)>,
	blurs: usize,
	releases: usize,
}

impl Host for State {
	fn cursor(&self) -> Point {
		self.cursor
	}
	fn modifiers(&self) -> Modifiers {
		self.modifiers
	}
	fn selection(&self) -> Option<TextSelection> {
		self.selection
	}
	fn set_selection(&mut self, value: Option<TextSelection>) {
		self.selection = value;
	}
	fn drag(&self) -> Option<&Drag> {
		self.drag.as_ref()
	}
	fn set_drag(&mut self, value: Option<Drag>) {
		self.drag = value;
	}
	fn take_drag(&mut self) -> Option<Drag> {
		self.drag.take()
	}
	fn dragged(&self) -> bool {
		self.dragged
	}
	fn set_dragged(&mut self, value: bool) {
		self.dragged = value;
	}
	fn last_click(&self) -> Option<(Instant, Point, u8)> {
		self.click
	}
	fn set_last_click(&mut self, value: Option<(Instant, Point, u8)>) {
		self.click = value;
	}
	fn blur(&mut self) {
		self.blurs += 1;
	}
	fn release_pointer(&mut self) {
		self.releases += 1;
	}
}

fn position(offset: usize) -> SemanticPosition {
	SemanticPosition {
		revision: DocumentRevision::new(1),
		block: BlockId::new(1).unwrap(),
		field: FieldId::from_index(0),
		offset,
		affinity: Affinity::Before,
		projection: None,
	}
}

fn target(id: u8) -> LinkTarget {
	LinkTarget::Url(format!("https://example.test/{id}").into())
}

proptest! {
	#[test]
	fn word_and_block_drags_extend_a_fixed_base_in_both_directions(
		reverse in any::<bool>(), moves in prop::collection::vec((0usize..3, 0usize..3, 0usize..2), 0..24),
	) {
		let at = |block: usize, offset, affinity| SemanticPosition { block: BlockId::new(3 - block as u32).unwrap(), offset, affinity, ..position(0) };
		let snapshot = LayoutSnapshot {
			revision: DocumentRevision::new(1),
			blocks: (0..3).map(|block| {
				let mut node = markview_layout::text::TextNode::new("aa bb cc".to_owned(), "\n\n");
				node.search_field = Some(FieldId::from_index(0));
				node.search_ranges = vec![(0..8, 0..8)].into();
				markview_layout::scene::PlacedBlock { id: at(block, 0, Affinity::Before).block, content: Default::default(), y: block as f32 * 20.0, layout: std::sync::Arc::new(markview_layout::scene::BlockLayout { text: vec![node].into(), ..Default::default() }), flow: Default::default() }
			}).collect(),
			..Default::default()
		};
		for grain in [Grain::Word, Grain::Block] {
			let (start, end) = if grain == Grain::Word { (3, 5) } else { (0, 8) };
			let first = at(1, start, Affinity::Before);
			let last = at(1, end, Affinity::After);
			let base = if reverse { TextSelection { anchor: last, focus: first } } else { TextSelection { anchor: first, focus: last } };
			let mut state = State::default();
			prop_assert!(state.begin_grain_selection(Some(base), grain));
			state.cursor = Point::new(10.0, 0.0);
			for (block, word, inside) in [(0, 0, 0), (2, 2, 1), (1, 1, 0)].into_iter().chain(moves.iter().copied()) {
				let offset = word * 3 + inside;
				let (unit_start, unit_end) = if grain == Grain::Word { (word * 3, word * 3 + 2) } else { (0, 8) };
				let expected = if (block, offset) < (1, start) {
					TextSelection { anchor: last, focus: at(block, unit_start, Affinity::Before) }
				} else if (block, offset) > (1, end) {
					TextSelection { anchor: first, focus: at(block, unit_end, Affinity::After) }
				} else { base };
				let changed = state.selection != Some(expected);
				prop_assert_eq!(state.move_selection(Some(at(block, offset, Affinity::Before)), &snapshot), changed);
				prop_assert_eq!(state.selection, Some(expected));
				prop_assert_eq!(state.drag.as_ref().unwrap().base, Some(base));
				prop_assert!(state.dragged);
			}
		}
	}

	#[test]
	fn click_cycles_follow_time_distance_and_explicit_resets(
		events in prop::collection::vec((0u64..602, -16i16..17, -16i16..17, any::<bool>()), 0..48),
	) {
		let mut state = State::default();
		let origin = Instant::now();
		let mut elapsed = 0;
		let mut previous = None;
		let mut run = 0;
		for (delta, x, y, reset) in [(0, 0, 0, false), (500, 6, 0, false), (501, 6, 0, false)].into_iter().chain(events) {
			elapsed += delta;
			if reset { state.reset_clicks(); previous = None; }
			let continues = previous.is_some_and(|(at, px, py): (u64, i16, i16)| {
				elapsed - at <= 500 && i32::from(x - px).pow(2) + i32::from(y - py).pow(2) <= 36
			});
			run = if continues { run + 1 } else { 1 };
			let expected = ((run - 1) % 3 + 1) as u8;
			state.cursor = Point::new(f32::from(x), f32::from(y));
			let now = origin + Duration::from_millis(elapsed);
			prop_assert_eq!(state.click_count(now), expected);
			prop_assert_eq!(state.last_click(), Some((now, state.cursor, expected)));
			previous = Some((elapsed, x, y));
		}
	}

	#[test]
	fn selection_sequences_preserve_anchors_and_activate_only_undragged_matching_links(
		operations in prop::collection::vec((0u8..6, -8i16..9, 0usize..17, any::<bool>(), prop::option::of(0u8..4)), 0..48),
	) {
		let mut state = State::default();
		let snapshot = LayoutSnapshot::default();
		let mut selected: Option<(usize, usize)> = None;
		let mut press: Option<(i16, Option<LinkTarget>)> = None;
		let mut dragged = false;
		let (mut blurs, mut releases) = (0, 0);
		let directed = [(0, 0, 0, false, Some(1)), (1, 3, 1, true, None), (2, 3, 0, false, Some(1)), (0, 0, 0, false, Some(1)), (1, 4, 0, false, None), (1, 0, 0, true, None), (2, 0, 0, false, Some(1)), (3, 0, 0, false, None)];
		for (op, x, offset, flag, link) in directed.into_iter().chain(operations) {
			state.cursor = Point::new(f32::from(x), 0.0);
			state.modifiers = Modifiers { shift: flag, ..Default::default() };
			let link = link.map(target);
			match op {
				0 => {
					state.begin_selection(position(offset), link.clone());
					let anchor = if flag { selected.map_or(offset, |s| s.0) } else { offset };
					selected = Some((anchor, offset));
					press = Some((x, link));
					dragged = flag;
					blurs += 1;
				}
				1 => {
					let before = selected;
					if let Some((start, _)) = &press {
						dragged |= (x - start).abs() >= 4;
						if dragged && flag && let Some(selection) = &mut selected { selection.1 = offset; }
					}
					prop_assert_eq!(state.move_selection(flag.then(|| position(offset)), &snapshot), selected != before);
				}
				2 => {
					let expected = press.take().and_then(|(_, initial)| initial).filter(|initial| !dragged && Some(initial) == link.as_ref());
					prop_assert_eq!(state.finish_selection(link.as_ref()), expected);
				}
				3 => {
					state.clear_selection();
					selected = None;
					press = None;
					releases += 1;
				}
				4 => {
					let link = LinkTarget::Disclosure(BlockId::new(offset as u32 + 1).unwrap());
					state.begin_link_press(link.clone());
					press = Some((x, Some(link)));
					dragged = false;
					blurs += 1;
				}
				_ => { prop_assert!(!state.begin_grain_selection(None, Grain::Word)); }
			}
			prop_assert_eq!(state.selection, selected.map(|(anchor, focus)| TextSelection { anchor: position(anchor), focus: position(focus) }));
			prop_assert_eq!(state.drag.as_ref().map(|drag| (drag.start.x, drag.link.clone())), press.as_ref().map(|(x, link)| (f32::from(*x), link.clone())));
			prop_assert_eq!(state.dragged, dragged);
			prop_assert_eq!((state.blurs, state.releases), (blurs, releases));
		}
	}

	#[test]
	fn grain_selection_owns_its_base_without_activating_a_link(a in 0usize..33, b in 0usize..33) {
		for grain in [Grain::Char, Grain::Word, Grain::Block] {
			let mut state = State::default();
			let selection = TextSelection { anchor: position(a), focus: position(b) };
			prop_assert!(state.begin_grain_selection(Some(selection), grain));
			prop_assert_eq!(state.selection, Some(selection));
			let drag = state.drag.as_ref().unwrap();
			prop_assert_eq!(drag.base, Some(selection));
			prop_assert_eq!(drag.grain, grain);
			prop_assert!(!state.dragged);
			prop_assert!(state.finish_selection(Some(&target(1))).is_none());
			prop_assert!(state.drag.is_none());
		}
	}
}
