//! File rows supply destinations and geometry to the shared hover preview.
use super::*;

impl<P: super::super::SendEvent> App<P> {
	fn folder_preview_rows(&self) -> Vec<(usize, Rect)> {
		if !self.folder.open || self.folder.reveal(self.frame_now()) != 1.0 {
			return Vec::new();
		}
		let list = self.folder.list(self.folder_rect(), self.frame_now());
		self.folder
			.list_rows(&list, self.frame_now())
			.into_iter()
			.filter_map(|row| {
				let rect = super::super::chrome::folder::entry_rect(&list, row)
					.intersect(list.viewport)?;
				Some((row.index, rect))
			})
			.collect()
	}

	/// Keeps the open preview reachable across the sidebar gutter and gap.
	pub(super) fn folder_preview_bridge(
		&self,
		x: f32,
		y: f32,
	) -> Option<Target> {
		let open = self.preview.open()?;
		let Target::File(_) = open.target else {
			return None;
		};
		let floating = open.floating.as_ref()?;
		let start = floating.anchor.x + floating.anchor.w - 4.0;
		Rect {
			x: start,
			y: floating.anchor.y,
			w: floating.rect.x - start,
			h: floating.anchor.h,
		}
		.contains(x, y)
		.then_some(open.target)
	}

	pub(super) fn folder_preview_at_cursor(&self) -> Option<Target> {
		let (x, y) = self.interaction.pointer.cursor();
		if self.sidebar_edge().is_some()
			|| self.folder_bar().is_some_and(|bar| bar.hit(x, y))
		{
			return None;
		}
		self.folder_preview_rows()
			.into_iter()
			.find_map(|(id, rect)| {
				(rect.contains(x, y)
					&& !self.folder.tree.as_ref()?.entries.get(id)?.directory)
					.then_some(Target::File(id))
			})
	}

	pub(super) fn scene_preview_anchor(&self, target: Target) -> Option<Rect> {
		if let Target::File(id) = target {
			return self
				.folder_preview_rows()
				.into_iter()
				.find_map(|(index, rect)| (id == index).then_some(rect));
		}
		let (x, y) = self.preview.floating().map_or_else(
			|| self.interaction.pointer.cursor(),
			|floating| {
				(
					floating.anchor.x + floating.anchor.w / 2.0,
					floating.anchor.y + floating.anchor.h / 2.0,
				)
			},
		);
		self.document_interaction()
			.link_rect(markview_interaction::Point::new(x, y))
	}

	pub(super) fn scene_preview_bounds(&self, target: Target) -> Rect {
		let bounds = self.document_interaction().viewport.clip();
		if matches!(target, Target::File(_)) {
			let drawer = self.folder_rect();
			let x = drawer.x + drawer.w;
			Rect {
				x,
				w: (self.dimensions().0 - x).max(1.0),
				..bounds
			}
		} else {
			bounds
		}
	}

	pub(super) fn preview_security(
		&self,
		target: Target,
	) -> crate::security::Security {
		if matches!(target, Target::File(_)) {
			crate::security::Security::local(crate::security::Trust::Trusted)
		} else {
			self.readers.session.security.clone()
		}
	}

	pub(super) fn folder_preview_path(
		&self,
		id: usize,
	) -> Option<&std::path::Path> {
		let entry = self.folder.tree.as_ref()?.entries.get(id)?;
		(!entry.directory).then_some(entry.path.as_path())
	}

	pub(super) fn folder_preview_source(
		&self,
		id: usize,
	) -> Option<work::Source> {
		let path = self.folder_preview_path(id)?;
		let target = Target::File(id);
		self.scene_preview_source(
			target,
			self.scene_preview_anchor(target)?,
			self.scene_preview_bounds(target),
			self.document_interaction().snapshot.revision,
			work::Destination {
				path: Some(path.to_owned()),
				anchor: None,
			},
		)
	}
}
