//! Background document layout for the selected file.
use super::*;
use crate::app::document_view::FloatingDocument;
use crate::app::preview::{
	Observed,
	work::{Content, Destination, Worker},
};
use crate::{
	layout::{LayoutOptions, LayoutSnapshot, Rect},
	render::Theme,
};
use markview_platform::{Clipboard, ClipboardStamp};
use std::path::PathBuf;

struct Request {
	path: PathBuf,
	observed: Observed,
	options: LayoutOptions,
	security: crate::security::Security,
	workspace: Option<PathBuf>,
	offline: bool,
}

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(in crate::app) struct Preview {
	request: Option<Request>,
	worker: Worker,
	pub(in crate::app) origins: crate::app::preview::Origins,
	serial: u64,
	pub(in crate::app) pending: bool,
	pub(in crate::app) select_all_pending: bool,
	pub(in crate::app) copy_pending: Option<ClipboardStamp>,
	detached_copy: Option<(ClipboardStamp, Worker)>,
	pub(in crate::app) reflow: Option<FloatingDocument>,
	pub(in crate::app) follow_end: bool,
	pub(in crate::app) document: Option<FloatingDocument>,
}

impl Preview {
	pub(in crate::app) fn complete(
		&mut self,
		snapshot: Option<LayoutSnapshot>,
		body: Rect,
		picture: bool,
	) {
		let metrics = self
			.request
			.as_ref()
			.map(|r| r.options.stylesheet.scrollbar_metrics())
			.unwrap_or_default();
		self.pending = false;
		let previous = self.document.take().or_else(|| self.reflow.take());
		self.document = snapshot.map(|mut snapshot| {
			// The picker paints one background for the entire preview column.
			snapshot.document_box = None;
			let mut doc =
				FloatingDocument::picker(snapshot, body, metrics, picture);
			if let Some(old) = previous.filter(|old| {
				old.snapshot.content_id == doc.snapshot.content_id
			}) {
				let offset = markview_layout::layout::anchored_scroll(
					&old.snapshot,
					&doc.snapshot,
					old.scroll.offset(),
					body.h,
					false,
				);
				doc.scroll = old.scroll;
				doc.scroll.set_max(doc.view().max_scroll());
				if doc.scroll.offset() != offset {
					doc.scroll.jump_to(offset);
				}
				doc.horizontal = old.horizontal;
				crate::state::clamp_horizontal_offsets(
					&doc.snapshot,
					&mut doc.horizontal,
				);
				if doc.revision == old.revision {
					doc.state = old.state;
					doc.refresh_hover();
					return doc;
				}
				doc.state.focused = old.state.focused;
				let remap = |point: markview_document::SemanticPosition| {
					let resolved = old.snapshot.resolve_position(point)?;
					doc.snapshot.semantic_at(
						resolved.block,
						resolved.node,
						resolved.offset,
						point.affinity,
					)
				};
				doc.state.selection = old.state.selection.and_then(|s| {
					Some(markview_layout::text::TextSelection {
						anchor: remap(s.anchor)?,
						focus: remap(s.focus)?,
					})
				});
			}
			doc
		});
	}
	pub(in crate::app) fn select_all(&mut self) {
		self.select_all_pending = self.pending;
	}
	pub(in crate::app) fn defer_copy(&mut self, generation: ClipboardStamp) {
		self.detached_copy = None;
		self.copy_pending = Some(generation);
	}
	fn detached_text(&mut self, clipboard: &mut Clipboard) -> Option<String> {
		let (generation, worker) = self.detached_copy.as_ref()?;
		let result = worker.try_recv()?;
		if result.partial {
			return None;
		}
		let stamp = *generation;
		self.detached_copy = None;
		Self::copied_text(result.snapshot.as_ref(), stamp, clipboard)
	}
	fn copied_text(
		snapshot: Option<&LayoutSnapshot>,
		stamp: ClipboardStamp,
		clipboard: &mut Clipboard,
	) -> Option<String> {
		let snapshot = snapshot?;
		if stamp != clipboard.stamp() {
			return None;
		}

		let rev = snapshot.revision.get();
		Some(snapshot.extract_text(snapshot.select_all(rev)?, rev))
	}
	pub(in crate::app) fn check_file(&self) {
		if let Some(request) = &self.request {
			request.observed.invalidate_if_outdated();
		}
	}
	pub(super) fn clear(&mut self) {
		if let Some(generation) = self.copy_pending.take() {
			self.detached_copy =
				Some((generation, std::mem::take(&mut self.worker)));
		} else {
			self.worker.cancel();
		}
		self.serial = self.serial.wrapping_add(1);
		self.request = None;
		self.origins = Default::default();
		self.reflow = None;
		self.select_all_pending = false;

		self.follow_end = false;
		self.document = None;
		self.pending = false;
	}
	pub(in crate::app) fn message(
		&self,
		lang: crate::lang::Lang,
	) -> Option<&'static str> {
		if self.pending && self.document.is_none() {
			Some(lang.panel_loading())
		} else if self.document.is_none() {
			Some(lang.quick_open_preview_unavailable())
		} else {
			None
		}
	}
	pub(in crate::app) fn scene(
		&self,
		theme: Theme,
	) -> Option<crate::render::Preview<'_>> {
		let doc = self.document.as_ref()?;
		Some(doc.scene(doc.body(), theme))
	}
}

impl<P: SendEvent> App<P> {
	pub(in crate::app) fn refresh_quick_open_preview(&mut self) {
		if let Some(text) = self
			.interaction
			.overlays
			.quick_open
			.preview
			.detached_text(&mut self.clipboard)
		{
			self.copy_reading_text(text);
		}
		if self.interaction.overlays.quick_open.open() {
			self.refresh_quick_open_body(self.quick_open_geometry().body);
		}
	}

	fn refresh_quick_open_body(&mut self, body: Option<Rect>) {
		let picker = &self.interaction.overlays.quick_open;
		let body = body.filter(|rect| rect.w > 0.0 && rect.h > 0.0);
		let Some((file, body)) = picker.file().zip(body) else {
			if picker.preview.request.is_some() {
				self.interaction.overlays.quick_open.preview.clear();
			}
			return;
		};
		let path = file.path.clone();
		let mut options = self.options();
		options.width = body.w;
		options.viewport_height = body.h;
		options.preview = true;
		options.details_open = Default::default();
		let security = picker
			.search
			.tab(file.id)
			.or_else(|| {
				self.readers
					.entries()
					.iter()
					.position(|tab| tab.path == path)
			})
			.map_or_else(
				|| {
					crate::security::Security::local(
						crate::security::Trust::Trusted,
					)
				},
				|index| {
					self.readers.session_at(index).unwrap().security.clone()
				},
			);
		let preview = &mut self.interaction.overlays.quick_open.preview;
		let workspace = self.folder.root().cloned();
		if let Some(doc) = &mut preview.document {
			doc.place_in_picker(body);
		}
		let completion = preview.worker.try_recv();
		let copied = completion
			.as_ref()
			.filter(|result| !result.partial && result.serial == preview.serial)
			.and_then(|result| {
				Preview::copied_text(
					result.snapshot.as_ref(),
					preview.copy_pending.take()?,
					&mut self.clipboard,
				)
			});
		let same_file = preview.request.as_ref().is_some_and(|request| {
			request.path == path
				&& request.security == security
				&& request.workspace == workspace
				&& request.offline == self.args.offline
				&& !request.observed.changed()
				&& (completion.is_none()
					|| !request.observed.invalidate_if_outdated())
		});
		let stale = !same_file
			|| preview
				.request
				.as_ref()
				.is_some_and(|r| r.options != options);
		if stale {
			self.refresh_quick_open_request(
				path, options, security, workspace, same_file,
			);
		}
		self.accept_quick_open_completion(completion, body);
		if let Some(text) = copied {
			self.copy_reading_text(text);
		}
	}

	/// Rebuilds the background request, keeping the reflow source when only the
	/// layout options changed.
	fn refresh_quick_open_request(
		&mut self,
		path: PathBuf,
		options: LayoutOptions,
		security: crate::security::Security,
		workspace: Option<PathBuf>,
		same_file: bool,
	) {
		let preview = &mut self.interaction.overlays.quick_open.preview;
		if same_file {
			if let Some(doc) = preview.document.take() {
				preview.reflow = Some(doc);
			}
			preview.serial = preview.serial.wrapping_add(1);
		} else {
			preview.clear();
		}
		let wake = self.proxy.clone();
		let observed = Observed::new(path.clone(), move || {
			wake.send(super::super::Event::QuickOpenPreviewReady)
		});
		let stamp = observed.stamp;
		preview.request = Some(Request {
			path: path.clone(),
			observed,
			options: options.clone(),
			security: security.clone(),
			workspace: workspace.clone(),
			offline: self.args.offline,
		});
		preview.pending = true;
		let wake = self.proxy.clone();
		preview.worker.submit_content(
			&self.services.handle,
			preview.serial,
			Content {
				security,
				workspace,
				offline: self.args.offline,
				document: None,
				destination: Destination {
					path: Some(path),
					anchor: None,
				},
				stamp,
				options,
				images: Default::default(),
				fit: false,
			},
			move || wake.send(super::super::Event::QuickOpenPreviewReady),
		);
	}

	fn accept_quick_open_completion(
		&mut self,
		completion: Option<crate::app::preview::work::Completion>,
		body: Rect,
	) {
		let Some(result) = completion else {
			return;
		};
		let preview = &mut self.interaction.overlays.quick_open.preview;
		let partial_reflow = result.partial && preview.reflow.is_some();
		if result.serial != preview.serial || partial_reflow {
			return;
		}
		preview.origins = result.origins;
		preview.complete(result.snapshot, body, result.picture);
		preview.pending = result.partial;
		if preview.follow_end
			&& let Some(doc) = preview.document.as_mut()
		{
			doc.scroll.jump_to(doc.view().max_scroll());
		}
		if preview.select_all_pending
			&& let Some(doc) = preview.document.as_mut()
		{
			doc.input(markview_interaction::ViewInput::SelectAll);
		}
		if !result.partial {
			preview.follow_end = false;
			preview.select_all_pending = false;
		}
		self.redraw();
	}
}
