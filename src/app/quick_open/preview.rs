//! Background document prefixes for the selected file; no reader is opened.
use super::*;
use crate::app::preview::{
	Observed,
	work::{Content, Destination, Worker},
};
use crate::{
	layout::{LayoutOptions, LayoutSnapshot, Rect},
	render::Theme,
};
use std::{collections::HashMap, path::PathBuf};

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
	serial: u64,
	pending: bool,
	pub(in crate::app) snapshot: Option<LayoutSnapshot>,
	horizontal: HashMap<(usize, usize), f32>,
}

impl Preview {
	pub(in crate::app) fn complete(
		&mut self,
		snapshot: Option<LayoutSnapshot>,
	) {
		self.pending = false;
		self.snapshot = snapshot.map(|mut snapshot| {
			// The picker paints one background for the entire preview column.
			snapshot.document_box = None;
			snapshot
		});
	}
	pub(super) fn clear(&mut self) {
		self.worker.cancel();
		self.serial = self.serial.wrapping_add(1);
		self.request = None;
		self.snapshot = None;
		self.pending = false;
	}
	pub(in crate::app) fn message(
		&self,
		lang: crate::lang::Lang,
	) -> Option<&'static str> {
		if self.pending {
			Some(lang.panel_loading())
		} else if self.snapshot.is_none() {
			Some(lang.quick_open_preview_unavailable())
		} else {
			None
		}
	}
	pub(in crate::app) fn scene(
		&self,
		rect: Rect,
		theme: Theme,
	) -> Option<crate::render::Preview<'_>> {
		Some(crate::render::Preview {
			interaction: None,
			snapshot: self.snapshot.as_ref()?,
			scroll: 0.0,
			rect,
			theme,
			horizontal: &self.horizontal,
		})
	}
}

impl<P: SendEvent> App<P> {
	pub(in crate::app) fn refresh_quick_open_preview(&mut self) {
		let picker = &self.interaction.overlays.quick_open;
		if !picker.open() {
			return;
		}
		let body = self
			.quick_open_geometry()
			.body
			.filter(|rect| rect.w > 0.0 && rect.h > 0.0);
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
		let completion = preview.worker.try_recv();
		if !preview.request.as_ref().is_some_and(|request| {
			request.path == path
				&& request.options == options
				&& request.security == security
				&& request.workspace == workspace
				&& request.offline == self.args.offline
				&& !request.observed.changed()
				&& (completion.is_none()
					|| !request.observed.invalidate_if_outdated())
		}) {
			preview.clear();
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
			if super::super::folder::markdown(&path) {
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
					move || {
						wake.send(super::super::Event::QuickOpenPreviewReady)
					},
				);
			}
		}
		if let Some(result) = completion
			&& result.serial == preview.serial
		{
			preview.complete(result.snapshot);
			self.redraw();
		}
	}
}
