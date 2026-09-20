//! Pinned fonts for tests.
//!
//! Layout asserts exact geometry, so it must not shape with whatever fonts the
//! machine happens to have installed. These are the committed subsets in
//! `crates/markview-core/tests/fonts`, registered with the system set off, so
//! every platform shapes the same faces.
use markview_core::fonts::FontConfig;
use markview_core::layout::{LayoutOptions, TextShaper};

/// The faces tests shape with.
pub(crate) fn fonts() -> FontConfig {
	FontConfig {
		ignore_system_fonts: true,
		directories: vec![
			std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
				.join("crates/markview-core/tests/fonts"),
		],
		..Default::default()
	}
}

/// Layout options over the pinned faces.
pub(crate) fn options() -> LayoutOptions {
	LayoutOptions {
		fonts: fonts(),
		..Default::default()
	}
}

/// A shaper that resolves labels against the pinned faces.
pub(crate) fn shaper() -> TextShaper {
	TextShaper::with_fonts(fonts())
}
