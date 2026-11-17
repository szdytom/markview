//! A standalone picture fills its floating viewport without document furniture.
use crate::layout::{Draw, LayoutSnapshot, Rect};
use markview_layout::{
	image::{ImageSource, ResourceRef},
	scene::BlockLayout,
};
use std::sync::Arc;

pub(super) fn fit(snapshot: &mut LayoutSnapshot, bounds: Rect) -> bool {
	let source = ImageSource::Resource(ResourceRef::document(
		crate::file::STANDALONE_IMAGE_SOURCE,
	));
	let Some(key) = snapshot.images.key(&source) else {
		return false;
	};
	let margin = 8.0_f32.min(bounds.w / 2.0).min(bounds.h / 2.0);
	let max_width = 420.0_f32.min((bounds.w - 2.0 * margin).max(1.0));
	let max_height = 320.0_f32.min((bounds.h - 2.0 * margin).max(1.0));
	let info = &snapshot.images.entries[key];
	let Some((width, height)) =
		info.size.filter(|_| !info.loading && info.error.is_none())
	else {
		// Loading and error labels share the picture's non-scrolling host.
		snapshot.width = snapshot.width.min(max_width);
		snapshot.height = snapshot.height.min(max_height);
		return true;
	};
	let Some((block, mut draw)) = snapshot.blocks.iter().find_map(|block| {
		block.draws().find_map(|(_, draw, _)| match draw {
			Draw::Image { ref src, .. } if src == key => {
				Some((block.clone(), draw))
			}
			_ => None,
		})
	}) else {
		return false;
	};
	let Draw::Image { rect, .. } = &mut draw else {
		unreachable!()
	};
	let scale = (max_width / width as f32)
		.min(max_height / height as f32)
		.min(1.0);
	*rect = Rect {
		x: 0.0,
		y: 0.0,
		w: (width as f32 * scale).min(max_width),
		h: (height as f32 * scale).min(max_height),
	};
	snapshot.width = rect.w;
	snapshot.height = rect.h;
	snapshot.centered = true;
	snapshot.document_box = None;
	snapshot.blocks = vec![markview_layout::scene::PlacedBlock {
		y: 0.0,
		flow: Default::default(),
		layout: Arc::new(BlockLayout {
			width: snapshot.width,
			height: snapshot.height,
			draws: vec![draw].into(),
			..Default::default()
		}),
		..block
	}];
	true
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::app::document_view::{FloatingDocument, FrameStyle};
	use markview_layout::{image::ImageInfo, scene::PlacedBlock};
	use proptest::prelude::*;

	proptest! {
		#[test]
		fn pictures_fit_every_viewport_without_cropping_or_scrolling(
			width in 1u32..100_000,
			height in 1u32..100_000,
			window_width in 32.0f32..1200.0,
			window_height in 32.0f32..900.0,
			x in 0.0f32..1.0,
			y in 0.0f32..1.0,
			right_centered in any::<bool>(),
			loaded in any::<bool>(),
			loading in any::<bool>(),
			failed in any::<bool>(),
		) {
			let mut snapshot = LayoutSnapshot { width: 420.0, height: 600.0, ..Default::default() };
			snapshot.images.keys.insert(
				ImageSource::Resource(ResourceRef::document(crate::file::STANDALONE_IMAGE_SOURCE)),
				"picture".into(),
			);
			snapshot.images.entries.insert("picture".into(), ImageInfo {
				size: loaded.then_some((width, height)), loading,
				error: failed.then(|| "Image decoding failed".into()), ..Default::default()
			});
			snapshot.blocks.push(PlacedBlock {
				id: markview_document::BlockId::new(1).unwrap(),
				content: Default::default(), y: 0.0, flow: Default::default(),
				layout: Arc::new(BlockLayout {
					draws: vec![Draw::Image {
						src: "picture".into(), version: 0, rect: Rect::default(), title: String::new(),
					}].into(), ..Default::default()
				}),
			});
			let bounds = Rect { x: 30.0, y: 40.0, w: window_width, h: window_height };
			let anchor = Rect { x: bounds.x + x * bounds.w, y: bounds.y + y * bounds.h, w: 12.0, h: 18.0 };
			let floating = FloatingDocument::new(snapshot, Default::default(), anchor, bounds, FrameStyle {
				right_centered, picture: true, padding: [10.0; 4], ..Default::default()
			});
			let rect = floating.rect;
			prop_assert_eq!(floating.body(), rect);
			prop_assert_eq!(floating.view().max_scroll(), 0.0);
			prop_assert!(floating.scrollbar().is_none());
			prop_assert!(rect.w <= 420.0 && rect.h <= 320.0);
			prop_assert!(rect.x >= bounds.x && rect.y >= bounds.y);
			prop_assert!(rect.x + rect.w <= bounds.x + bounds.w + 0.001);
			prop_assert!(rect.y + rect.h <= bounds.y + bounds.h + 0.001);
			if loaded && !loading && !failed {
				prop_assert!(((rect.w / rect.h) / (width as f32 / height as f32) - 1.0).abs() < 0.00001);
			}
		}
	}
}
