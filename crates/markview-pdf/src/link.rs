//! Link annotations.
//!
//! A link that names a heading in the same document becomes an internal
//! destination, so a table of contents still navigates. Everything else is a
//! URI the viewer hands to the system.
use krilla::{
	action::{Action, LinkAction},
	annotation::{Annotation, LinkAnnotation, Target},
	destination::{Destination, XyzDestination},
	geom::{Point, Rect},
};
use markview_core::paginate::{PT_PER_PX, PageGeometry, Pagination};

/// The annotation for one link, or `None` when the target cannot be reached.
/// `rect` is in page points, y measured down from the page's top edge.
pub fn annotation(
	rect: Rect,
	url: &str,
	pagination: &Pagination,
	geometry: &PageGeometry,
) -> Option<Annotation> {
	let target = if let Some(anchor) = url.strip_prefix('#') {
		// An anchor is a layout pixel offset inside the text area; a
		// destination is a page point, so it needs both the top margin and the
		// pixel-to-point conversion.
		let (page, y) = pagination.anchors.get(anchor)?;
		let [left, top, _, _] = geometry.text_pt();
		Target::Destination(Destination::Xyz(XyzDestination::new(
			*page,
			Point::from_xy(left, top + y * PT_PER_PX),
		)))
	} else if url.is_empty() {
		return None;
	} else {
		Target::Action(Action::Link(LinkAction::new(url.to_owned())))
	};
	let link = LinkAnnotation::new(rect, target);
	Some(Annotation::new_link(link, Some(url.to_owned())))
}
