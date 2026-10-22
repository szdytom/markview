//! The overlays drawn over the reading surface, each owning its own state.
//!
//! A confirmation, an option list, the image viewer and the outline drawer are
//! not panel pages: while one owns input it stands down as a whole. Each
//! overlay model owns the fields only it writes, so its owner file can be
//! found by search.

use super::dropdown::Dropdown;
use super::modal::Modal;

/// The overlay models; a new overlay adds one field here.
#[derive(Default)]
pub(crate) struct Model {
	pub(crate) outline: super::outline::Model,
	pub(crate) modal: super::modal::Model,
	pub(crate) dropdown: super::dropdown::Model,
}

impl Model {
	/// Whether a confirmation currently owns input.
	pub(crate) fn modal_open(&self) -> bool {
		self.modal.open()
	}

	/// The confirmation awaiting an answer, if any.
	pub(crate) fn modal(&self) -> Option<&Modal> {
		self.modal.current()
	}

	/// Whether an option list currently owns input.
	pub(crate) fn dropdown_open(&self) -> bool {
		self.dropdown.is_open()
	}

	/// The open option list, if any.
	pub(crate) fn dropdown(&self) -> Option<&Dropdown> {
		self.dropdown.current()
	}
}
