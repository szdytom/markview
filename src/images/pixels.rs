//! Allocation-aware eviction for shared image aliases.
use markview_core::image::Pixels;
use std::{
	collections::{HashMap, HashSet},
	sync::Arc,
};

pub(super) fn pixel_bytes(pixels: &HashMap<String, Arc<Pixels>>) -> usize {
	let mut seen = HashSet::new();
	pixels
		.values()
		.filter(|p| seen.insert(Arc::as_ptr(p)))
		.map(|p| p.rgba.len())
		.sum()
}

pub(super) fn cache_pixels(
	pixels: &mut HashMap<String, Arc<Pixels>>,
	aliases: &[String],
	incoming: Arc<Pixels>,
	demand: &HashMap<String, markview_core::image::ImageDemand>,
	budget: usize,
) {
	for a in aliases {
		pixels.remove(a);
	}
	let mut bytes = pixel_bytes(pixels);
	let mut victims: Vec<_> = pixels.keys().cloned().collect();
	// Evict an entire allocation, not one alias. Prefer offscreen resources.
	let visible: HashSet<_> = demand
		.keys()
		.filter_map(|a| pixels.get(a).map(Arc::as_ptr))
		.collect();
	victims.sort_by_key(|a| {
		(visible.contains(&Arc::as_ptr(&pixels[a])), a.clone())
	});
	for alias in victims {
		if bytes + incoming.rgba.len() <= budget {
			break;
		}
		if let Some(victim) = pixels.get(&alias).cloned() {
			pixels.retain(|_, p| !Arc::ptr_eq(p, &victim));
			bytes -= victim.rgba.len();
		}
	}
	if incoming.rgba.len() <= budget {
		for alias in aliases {
			pixels.insert(alias.clone(), incoming.clone());
		}
	}
}
