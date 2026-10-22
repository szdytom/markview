//! Reader-owned resource permission prompts and navigation provenance.
use super::App;
use super::modal::{Message, Modal, PermissionAction};
use crate::state::ChromeCommand;
use crate::{
	security::{Resource, Security, Trust},
	state::{Command, TabPlacement},
};
use std::path::PathBuf;

impl<P: super::SendEvent> App<P> {
	pub(super) fn ask_permission(
		&mut self,
		resource: Resource,
		resume: PermissionAction,
	) {
		self.interaction
			.overlays
			.modal
			.send(Message::Open(Modal::Permission { resource, resume }));
		self.interaction.widgets.focus =
			Some(Command::Chrome(ChromeCommand::ModalDismiss).widget_id());
		self.refresh_hover();
		self.redraw();
	}

	pub(super) fn confirm_permission(
		&mut self,
		resource: Resource,
		resume: PermissionAction,
	) {
		if let Resource::SelectImage(source) = resource {
			self.dialog_open = true;
			let proxy = self.proxy.clone();
			let document = self.readers.session.path.clone();
			let revision = self.readers.session.content_version;
			markview_platform::pick_image(move |file| {
				proxy.send(super::Event::ImageSelected {
					document,
					revision,
					source,
					file,
				});
			});
			return;
		}
		self.readers.session.security.grant(resource);
		if let Some(document) = &self.readers.session.document {
			self.readers.session.security.bind(&document.source);
		}
		match resume {
			PermissionAction::Images => self.request(false),
			PermissionAction::Markdown {
				path,
				anchor,
				placement,
			} => {
				self.open_markdown(
					path,
					anchor,
					placement,
					Security::local(Trust::Untrusted),
				);
			}
			PermissionAction::Web(url) => {
				self.readers.session.load_error = Some(String::new());
				self.open_web_page(url);
			}
		}
	}

	pub(super) fn open_markdown(
		&mut self,
		path: PathBuf,
		anchor: Option<String>,
		placement: TabPlacement,
		security: Security,
	) {
		match placement {
			TabPlacement::Foreground => {
				self.open_document(path, security);
				if let Some(anchor) = anchor {
					self.goto_anchor(anchor);
				}
			}
			TabPlacement::Background => {
				if let Some(index) =
					self.readers.find_origin(&path, &security.origin)
				{
					self.readers.queue_anchor(index, anchor);
					if index == self.readers.active() {
						self.apply_anchor();
					}
				} else {
					self.readers.open_background_secure(path, anchor, security);
				}
				self.redraw();
			}
		}
	}
}
