use markview_layout::layout::LayoutOptions;

fn fonts() -> markview_layout::fonts::FontConfig {
	markview_layout::fonts::FontConfig::from_faces(
		0x7765627071,
		[
			include_bytes!(
				"../../crates/markview-layout/tests/fonts/NotoSerif-Regular-subset.otf"
			)
			.as_slice(),
			include_bytes!(
				"../../crates/markview-layout/tests/fonts/NotoSans-Regular-subset.otf"
			)
			.as_slice(),
			include_bytes!(
				"../../crates/markview-layout/tests/fonts/NotoSansMono-Regular-subset.otf"
			)
			.as_slice(),
		]
		.into_iter()
		.map(|data| parley::fontique::Blob::new(std::sync::Arc::new(data)))
		.collect(),
	)
}

pub(crate) fn layout_options() -> LayoutOptions {
	LayoutOptions {
		width: 640.,
		viewport_height: 800.,
		fonts: fonts(),
		..Default::default()
	}
}
