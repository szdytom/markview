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
pub(super) const OUTLINE: &[IconPath] =
	markview_icon::icon!("assets/ui/outline.svg");
pub(in crate::app) const CLOSE: &[IconPath] =
	markview_icon::icon!("assets/ui/close.svg");

pub(super) const BACK: &[IconPath] = markview_icon::icon!("assets/ui/back.svg");
pub(super) const UP: &[IconPath] =
	markview_icon::icon!("assets/ui/arrow-up.svg");
pub(super) const DOWN: &[IconPath] =
	markview_icon::icon!("assets/ui/arrow-down.svg");

pub(super) const EYE: &[IconPath] = markview_icon::icon!("assets/ui/eye.svg");
pub(super) const EYE_OFF: &[IconPath] =
	markview_icon::icon!("assets/ui/eye-off.svg");

pub(in crate::app) const DOWNLOAD: &[IconPath] =
	markview_icon::icon!("assets/ui/download.svg");
pub(in crate::app) const REDOWNLOAD: &[IconPath] =
	markview_icon::icon!("assets/ui/redownload.svg");

pub(super) const MINUS: &[IconPath] =
	markview_icon::icon!("assets/ui/minus.svg");
pub(super) const PLUS: &[IconPath] = markview_icon::icon!("assets/ui/plus.svg");

pub(super) const COPY: &[IconPath] = markview_icon::icon!("assets/ui/copy.svg");

pub(super) const APP: &[IconPath] =
	markview_icon::icon!("assets/markview-icon.svg");
