//! The reader's vector icons.
//!
//! Each constant is parsed from its SVG file at compile time, so the sources
//! stay editable and no SVG runtime reaches the reader.
use markview_layout::scene::IconPath;

pub(super) const SEARCH: &[IconPath] =
	markview_icon::icon!("assets/ui/search.svg");

pub(super) const OPEN: &[IconPath] =
	markview_icon::icon!("assets/ui/file-open.svg");
pub(super) const REVEAL: &[IconPath] =
	markview_icon::icon!("assets/ui/folder-open.svg");
pub(super) const SIDEBAR: &[IconPath] =
	markview_icon::icon!("assets/ui/sidebar.svg");
pub(super) const TREE_FOLDER: &[IconPath] =
	markview_icon::icon!("assets/ui/tree-folder.svg");
pub(super) const TREE_FOLDER_OPEN: &[IconPath] =
	markview_icon::icon!("assets/ui/tree-folder-open.svg");
pub(super) const TREE_MARKDOWN: &[IconPath] =
	markview_icon::icon!("assets/ui/tree-markdown.svg");
pub(super) const EXPORT: &[IconPath] =
	markview_icon::icon!("assets/ui/export.svg");
pub(super) const SETTINGS: &[IconPath] =
	markview_icon::icon!("assets/ui/settings.svg");
pub(super) const OUTLINE: &[IconPath] =
	markview_icon::icon!("assets/ui/outline.svg");
pub(super) const EXPAND_ALL: &[IconPath] =
	markview_icon::icon!("assets/ui/expand-all.svg");
pub(super) const COLLAPSE_ALL: &[IconPath] =
	markview_icon::icon!("assets/ui/collapse-all.svg");
pub(in crate::app) const CLOSE: &[IconPath] =
	markview_icon::icon!("assets/ui/close.svg");

pub(super) const BACK: &[IconPath] = markview_icon::icon!("assets/ui/back.svg");
pub(in crate::app) const UP: &[IconPath] =
	markview_icon::icon!("assets/ui/arrow-up.svg");
pub(in crate::app) const DOWN: &[IconPath] =
	markview_icon::icon!("assets/ui/arrow-down.svg");

/// Trails the value of a control whose options open in a list.
pub(in crate::app) const CHEVRON: &[IconPath] =
	markview_icon::icon!("assets/ui/chevron.svg");

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

/// Opens dialect / settings help beside a control.
pub(super) const INFO: &[IconPath] = markview_icon::icon!("assets/ui/info.svg");

pub(super) const APP: &[IconPath] =
	markview_icon::icon!("assets/markview-icon.svg");
