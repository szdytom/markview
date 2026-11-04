use super::*;
use proptest::prelude::*;

fn demand(size: (u32, u32), needs_pixels: bool) -> ImageDemand {
	ImageDemand { size, needs_pixels }
}
proptest! {
	#[test]
	fn demand_merge_is_a_join(values in prop::collection::vec((any::<u32>(),any::<u32>(),any::<bool>()),0..32)) {
		let mut a = demand((0, 0), false);
		let mut b = a;
		for &(w, h, p) in &values {
			a.merge(demand((w, h), p));
		}
		for &(w, h, p) in values.iter().rev() {
			b.merge(demand((w, h), p));
		}
		prop_assert_eq!(a, b);
		let original = a;
		a.merge(a);
		prop_assert_eq!(a, original);
		prop_assert_eq!(
			a.size,
			(
				values.iter().map(|v| v.0).max().unwrap_or(0),
				values.iter().map(|v| v.1).max().unwrap_or(0)
			)
		);
		prop_assert_eq!(a.needs_pixels, values.iter().any(|v| v.2));
	}
	#[test]
	fn versioned_resources_and_frames(ops in prop::collection::vec((0u8..6,0u8..4,0u64..4,any::<u8>()),0..64)) {
		let pixels = Arc::new(ImagePixels::default());
		let mut model = HashMap::new();
		let mut frame = None;
		let mut snapshots = Vec::new();
		for (step, (op, alias, version, value)) in ops.into_iter().enumerate() {
			let alias = alias.to_string();
			match op {
				0 | 1 => {
					let p = Arc::new(Pixels {
						width: 1,
						height: 1,
						rgba: Arc::from([value; 4]),
					});
					if op == 1 {
						pixels.replace(HashMap::new());
						model.clear();
					}
					pixels.insert(alias.clone(), version, p);
					model.insert((alias.clone(), version), value);
				}
				2 => {
					snapshots.push(ImageSnapshot {
						generation: version,
						keys: HashMap::from([(
							ImageSource::Resource(ResourceRef::document(
								alias.clone(),
							)),
							alias.clone(),
						)]),
						entries: HashMap::from([(
							alias.clone(),
							ImageInfo {
								version,
								..Default::default()
							},
						)]),
						pixels: pixels.clone(),
					});
				}
				3 => {
					let entries = HashMap::from([(
						alias.clone(),
						demand((value as u32, 1), true),
					)]);
					pixels.publish_demand(version, entries.clone());
					frame = Some((version, entries));
				}
				4 => {
					let expected =
						frame.take().filter(|(g, _)| *g == version).map(|(_, e)| e);
					prop_assert_eq!(
						pixels.take_demand(version),
						expected,
						"step={}",
						step
					);
				}
				_ => {
					let expected = frame
						.as_ref()
						.filter(|(g, _)| *g == version)
						.map(|(_, e)| e.clone())
						.unwrap_or_default();
					prop_assert_eq!(
						pixels.demand(version),
						expected,
						"step={}",
						step
					);
				}
			}
			for a in 0..4 {
				for v in 0..4 {
					let key = (a.to_string(), v);
					prop_assert_eq!(
						pixels.get(&key.0, v).map(|p| p.rgba[0]),
						model.get(&key).copied(),
						"step={}",
						step
					);
				}
			}
			for s in &snapshots {
				let actual: HashMap<_, _> = s
					.decoded()
					.into_iter()
					.map(|(alias, pixels)| {
						(alias, (pixels.width, pixels.height, pixels.rgba.to_vec()))
					})
					.collect();
				let expected: HashMap<_, _> = s
					.entries
					.iter()
					.filter_map(|(alias, info)| {
						model
							.get(&(alias.clone(), info.version))
							.map(|&value| (alias.clone(), (1, 1, vec![value; 4])))
					})
					.collect();
				prop_assert_eq!(actual, expected, "step={}", step);
			}
		}
	}
	#[test]
	fn raster_respects_caps_and_proportions(w in 0f32..1_000_000., h in 0f32..1_000_000.) {
		let (rw, rh) = raster_size(w, h);
		prop_assert!((1..=4000).contains(&rw) && (1..=4000).contains(&rh));
		prop_assert!(u64::from(rw) * u64::from(rh) <= MAX_RASTER_PIXELS);
		prop_assert_eq!(raster_size(rw as f32, rh as f32), (rw, rh));
		let scale = (4000. / w.max(h).max(1.)).min(1.);
		// Nearest-pixel rounding and the minimum-one-pixel floor each cost at most one pixel.
		prop_assert!((rw as f32 - w.max(1.) * scale).abs() <= 1.);
		prop_assert!((rh as f32 - h.max(1.) * scale).abs() <= 1.);
	}
	#[test]
	fn image_dimensions_follow_attribute_priority(iw in 0u32..1000, ih in 0u32..1000, w in prop::option::of(0u32..1000), h in prop::option::of(0u32..1000), available in 0u16..1000, known in any::<bool>()) {
		let spec = ImageSpec {
			source: ImageSource::Resource(ResourceRef::document("")),
			alt: String::new(),
			title: String::new(),
			width: w,
			height: h,
		};
		let info = known.then_some(ImageInfo {
			size: Some((iw, ih)),
			..Default::default()
		});
		let (iw, ih) = if known {
			(iw as f64, ih as f64)
		} else {
			(160., 96.)
		};
		let raw = match (w, h) {
			(Some(w), Some(h)) => (w as f64, h as f64),
			(Some(w), None) => (w as f64, w as f64 * ih / iw.max(1.)),
			(None, Some(h)) => (h as f64 * iw / ih.max(1.), h as f64),
			_ => (iw, ih),
		};
		let scale = (f64::from(available.max(1)) / raw.0.max(1.)).min(1.);
		let actual = image_size(&spec, info.as_ref(), available as f32);
		// Relative error for the two multiplies/divides in the aspect calculation.
		prop_assert!(
			(actual.0 as f64 - raw.0 * scale).abs()
				<= 0.00001 * (raw.0 * scale).max(1.)
		);
		prop_assert!(
			(actual.1 as f64 - raw.1 * scale).abs()
				<= 0.00001 * (raw.1 * scale).max(1.)
		);
	}
}

#[test]
fn raster_thresholds_and_extreme_aspect_ratios() {
	for (w, h, expected) in [
		(0., 0., (1, 1)),
		(4000., 4000., (4000, 4000)),
		(4001., 4001., (4000, 4000)),
		(1_000_000., 1., (4000, 1)),
		(1., 1_000_000., (1, 4000)),
		(3999.5, 1., (4000, 1)),
	] {
		assert_eq!(raster_size(w, h), expected);
	}
}
