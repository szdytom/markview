use super::*;
use markview_layout::image::ImageDemand;
use proptest::prelude::*;

proptest! {
	#[test]
	fn pixel_aliases_count_allocations_once_and_evict_whole_offscreen_allocations_first(
		sizes in prop::array::uniform4(1usize..17), assignments in prop::collection::vec(0usize..4, 0..12),
		replace in prop::collection::vec(0usize..16, 1..8), visible in prop::array::uniform4(any::<bool>()), incoming_size in 1usize..17, budget in 0usize..100,
	) {
		let pool: Vec<_> = sizes.iter().map(|&size| Arc::new(Pixels { width: size as u32, height: 1, rgba: vec![0; size * 4].into() })).collect();
		let mut pixels: HashMap<_, _> = assignments.iter().enumerate().map(|(alias, &allocation)| (format!("alias{alias}"), pool[allocation].clone())).collect();
		let present: HashSet<_> = assignments.iter().copied().collect();
		prop_assert_eq!(pixel_bytes(&pixels), present.iter().map(|&id| sizes[id] * 4).sum::<usize>());
		let aliases: Vec<_> = replace.into_iter().map(|alias| format!("alias{alias}")).collect();
		let demand: HashMap<_, _> = assignments.iter().enumerate().filter(|&(_, &id)| visible[id]).map(|(alias, _)| (format!("alias{alias}"), ImageDemand { size: (1, 1), needs_pixels: true })).collect();
		let remaining: HashMap<_, _> = pixels.iter().filter(|(alias, _)| !aliases.contains(alias)).map(|(alias, pixels)| (alias.clone(), pixels.clone())).collect();
		let incoming = Arc::new(Pixels { width: incoming_size as u32, height: 1, rgba: vec![1; incoming_size * 4].into() });
		let budget = budget * 4;
		cache_pixels(&mut pixels, &aliases, incoming.clone(), &demand, budget);
		for alias in &aliases { prop_assert_eq!(pixels.get(alias).map(|pixels| Arc::ptr_eq(pixels, &incoming)), (incoming_size * 4 <= budget).then_some(true)); }
		prop_assert!(pixel_bytes(&pixels) <= budget);
		for (id, allocation) in pool.iter().enumerate() {
			let original: Vec<_> = remaining.iter().filter(|(_, pixels)| Arc::ptr_eq(pixels, allocation)).map(|(alias, _)| alias).collect();
			let kept = original.iter().filter(|alias| pixels.contains_key(**alias)).count();
			prop_assert!(kept == 0 || kept == original.len(), "allocation {id} was partially evicted");
			if kept == 0 && !original.is_empty() && original.iter().any(|alias| demand.contains_key(*alias)) {
				for (alias, allocation) in &remaining {
					let demanded = remaining.iter().any(|(key, pixels)| demand.contains_key(key) && Arc::ptr_eq(pixels, allocation));
					if !demanded { prop_assert!(!pixels.contains_key(alias)); }
				}
			}
		}
	}
}
