use super::*;
use markview_document::{BlockId, DocumentRevision};
use markview_layout::scene::{BlockLayout, PlacedBlock};
use proptest::prelude::*;

fn prefix(count: u8, stamp: u8) -> LayoutSnapshot {
	LayoutSnapshot {
		blocks: (0..count)
			.map(|index| PlacedBlock {
				id: BlockId::new(u32::from(index) + 1).unwrap(),
				content: Default::default(),
				y: f32::from(index) * 10.0,
				layout: Arc::new(BlockLayout {
					height: 10.0,
					..Default::default()
				}),
				flow: Arc::default(),
			})
			.collect(),
		height: f32::from(count) * 10.0,
		width: f32::from(stamp),
		presentation_key: u64::from(stamp),
		revision: DocumentRevision::new(1),
		reused: usize::from(stamp),
		..Default::default()
	}
}

proptest! {
	#[test]
	fn selection_length_caches_unicode_units_until_explicitly_forgotten(
		operations in prop::collection::vec((any::<bool>(), prop::collection::vec(any::<char>(), 0..32)), 0..32),
	) {
		let cache = SelectionLength::default();
		let mut expected = None;
		let mut extractions = 0;
		for (forget, chars) in operations {
			if forget { cache.forget(); expected = None; }
			let units: usize = chars.iter().map(|&ch| if u32::from(ch) > 0xffff { 2 } else { 1 }).sum();
			let text: String = chars.into_iter().collect();
			let value = cache.get(|| { extractions += 1; text });
			prop_assert_eq!(value, *expected.get_or_insert(units));
			prop_assert_eq!(cache.extractions(), extractions);
			prop_assert_eq!(cache.get(|| panic!("a cached read must not extract")), value);
		}
	}

	#[test]
	fn publication_sequences_preserve_source_identity_prefixes_and_wrapping_revisions(
		initial_revision in any::<u64>(), operations in prop::collection::vec((0u8..3, 0u8..12, any::<u8>(), 0u8..3), 0..24),
	) {
		for initial_revision in [initial_revision, u64::MAX] {
		let source: Arc<str> = Arc::from("source");
		let equal_source: Arc<str> = Arc::from("source");
		let mut published = Published { revision: initial_revision, ..Default::default() };
		let mut pointer = Pointer::default();
		let mut revision = initial_revision;
		let mut count = 0;
		let mut last_stamp = 0;
		let mut pass = None;
		let mut has_source = false;
		for (operation, next_count, stamp, source_kind) in [(2, 0, 0, 0)].into_iter().chain(operations.iter().copied()) {
			let next = prefix(next_count, stamp);
			match operation {
				0 => { published.accept(next, source.clone(), Some(u64::from(stamp)), &mut pointer); count = next_count; last_stamp = stamp; pass = Some(u64::from(stamp)); revision = revision.wrapping_add(1); has_source = true; }
				1 => {
					let retained: Vec<_> = published.snapshot.blocks.iter().map(|block| block.layout.clone()).collect();
					prop_assert_eq!(published.extend(&next, &mut pointer), next_count >= count);
					if next_count >= count { count = next_count; last_stamp = stamp; revision = revision.wrapping_add(1); }
					for (block, original) in published.snapshot.blocks.iter().zip(&retained) { prop_assert!(Arc::ptr_eq(&block.layout, original)); }
				}
				_ => { published.present_disclosures(&BTreeMap::new(), false, &mut pointer); revision = revision.wrapping_add(1); pass = None; }
			}
			prop_assert_eq!(published.revision, revision);
			prop_assert_eq!(published.snapshot.blocks.len(), usize::from(count));
			prop_assert_eq!(published.snapshot.width, f32::from(last_stamp));
			prop_assert_eq!(published.snapshot.reused, usize::from(last_stamp));
			let candidate = if source_kind == 0 { &source } else { &equal_source };
			prop_assert_eq!(published.continues(candidate, u64::from(stamp)), has_source && source_kind == 0 && pass == Some(u64::from(stamp)));
		}
		}
	}
}
