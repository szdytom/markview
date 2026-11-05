//! Link annotations.
//!
//! A link that names a heading in the same document becomes an internal
//! destination, so a table of contents still navigates. Everything else is a
//! URI the viewer hands to the system.
#[cfg(test)]
mod proptests;

use krilla::{
	action::{Action, LinkAction},
	annotation::{Annotation, LinkAnnotation, Target},
	destination::{Destination, XyzDestination},
	geom::{Point, Rect},
};
use markview_layout::{
	document::AnchorId,
	paginate::{PT_PER_PX, PageGeometry, Pagination},
	scene::LinkTarget,
};

/// The annotation for one link, or `None` when the target cannot be reached.
/// `rect` is in page points, y measured down from the page's top edge.
pub fn annotation(
	rect: Rect,
	target: &LinkTarget,
	url: &str,
	pagination: &Pagination,
	geometry: &PageGeometry,
) -> Option<Annotation> {
	let destination = match target {
		LinkTarget::Url(url) => url
			.strip_prefix('#')
			.map(|fragment| AnchorId::Fragment(fragment.to_owned())),
		LinkTarget::FootnoteDefinition(id) => {
			Some(AnchorId::FootnoteDefinition(id.clone()))
		}
		LinkTarget::FootnoteReturn(id) => pagination
			.targets
			.keys()
			.find(|anchor| {
				matches!(
					anchor,
					AnchorId::FootnoteReference { definition, .. }
						if definition == id
				)
			})
			.cloned()
			.or_else(|| Some(AnchorId::FootnoteDefinition(id.clone()))),
		LinkTarget::Disclosure(_) => None,
	};
	let target = if let Some(anchor) = destination {
		let (page, y) = match &anchor {
			AnchorId::Fragment(fragment) => {
				let fragment = percent_encoding::percent_decode_str(fragment)
					.decode_utf8_lossy();
				pagination.anchors.get(fragment.as_ref())?
			}
			_ => pagination.targets.get(&anchor)?,
		};
		// An anchor is a layout pixel offset inside the text area; a
		// destination is a page point, so it needs both the top margin and the
		// pixel-to-point conversion.
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
