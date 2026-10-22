//! The overlays drawn over the reading surface, each owning its own state.
//!
//! A confirmation, an option list, the image viewer and the outline drawer are
//! not panel pages: while one owns input it stands down as a whole. Each
//! overlay model owns the fields only it writes, so its owner file can be
//! found by search.

/// The overlay models; a new overlay adds one field here.
#[derive(Default)]
pub(crate) struct Model {
	pub(crate) outline: super::outline::Model,
}
