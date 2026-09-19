//! The reader's vector icons.
//!
//! Each constant is parsed from its SVG file at compile time, so the sources
//! stay editable and no SVG runtime reaches the reader.
use markview_core::scene::IconPath;

pub(super) const OPEN: &[IconPath] = markview_icon::icon!("assets/ui/open.svg");
pub(super) const EXPORT: &[IconPath] =
	markview_icon::icon!("assets/ui/export.svg");
pub(super) const SETTINGS: &[IconPath] =
	markview_icon::icon!("assets/ui/settings.svg");
pub(super) const CLOSE: &[IconPath] =
	markview_icon::icon!("assets/ui/close.svg");
