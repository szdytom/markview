//! Pinned fonts for tests.
//!
//! Layout asserts exact geometry, so it must not shape with whatever fonts the
//! machine happens to have installed. These are the committed subsets in
//! `crates/markview-layout/tests/fonts`, registered with the system set off, so
//! every platform shapes the same faces.
use markview_layout::fonts::FontConfig;
use markview_layout::layout::{LayoutOptions, TextShaper};

#[path = "../tests/support/render_goldens.rs"]
pub(crate) mod render_goldens;

/// The faces tests shape with.
pub(crate) fn fonts() -> FontConfig {
	FontConfig {
		ignore_system_fonts: true,
		directories: vec![
			std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
				.join("crates/markview-layout/tests/fonts"),
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

/// Leaves a real poisoned lock for recovery tests.
pub(crate) fn poison<T: Send>(lock: &std::sync::Mutex<T>) {
	std::thread::scope(|scope| {
		assert!(
			scope
				.spawn(|| {
					let _state = lock.lock().unwrap();
					panic!("injected failure");
				})
				.join()
				.is_err()
		);
	});
}

#[cfg(feature = "diagnostics")]
pub(crate) mod allocations;
