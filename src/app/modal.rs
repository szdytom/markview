//! The blocking confirmation.
//!
//! A permission prompt or an untrusted local file is a question the reader
//! answers before anything behind it receives input. The `Model` owns the
//! pending question and changes it only through its own messages; the rest of
//! the application asks `overlays.modal_open()` whether input is taken and
//! `overlays.modal()` what is being asked.
use crate::state::TabPlacement;
use markview_ui_core::{Component, Invalidation, Update};
use std::path::PathBuf;

/// A blocking question awaiting the reader's answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Modal {
	Permission {
		resource: crate::security::Resource,
		resume: PermissionAction,
	},
	/// A local file whose type is not on the inert allowlist.
	OpenLocal {
		path: PathBuf,
		/// The file's directory, for the "open folder" action.
		dir: PathBuf,
		/// The open document's directory, for a shorter relative display.
		document_dir: Option<PathBuf>,
	},
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PermissionAction {
	Images,
	Markdown {
		path: PathBuf,
		anchor: Option<String>,
		placement: TabPlacement,
	},
	Web(String),
}

/// The confirmation's own state: the question awaiting an answer, if any.
#[derive(Default)]
pub(crate) struct Model {
	current: Option<Modal>,
}

/// The confirmation's control-flow transitions.
pub(crate) enum Message {
	/// Ask `modal` and take input until it is answered.
	Open(Modal),
	/// Withdraw the question without answering it.
	Dismiss,
}

impl Component for Model {
	type Message = Message;
	type Effect = ();
	type View<'a> = Option<&'a Modal>;

	fn view(&self) -> Option<&Modal> {
		self.current.as_ref()
	}

	fn update(&mut self, message: Message) -> Update<()> {
		match message {
			Message::Open(modal) => self.current = Some(modal),
			Message::Dismiss => self.current = None,
		}
		Update::new(Invalidation::Paint)
	}
}

impl Model {
	/// Sends one control-flow message.
	///
	/// The confirmation gates input in the same event that opens or dismisses
	/// it, so the message applies before this returns; the host redraws where
	/// it always did.
	pub(crate) fn send(&mut self, message: Message) {
		self.update(message);
	}

	/// Whether a confirmation currently owns input.
	pub(crate) fn open(&self) -> bool {
		self.view().is_some()
	}

	/// The question awaiting an answer, if any.
	pub(crate) fn current(&self) -> Option<&Modal> {
		self.view()
	}

	/// Removes the pending question, as answering it does.
	pub(crate) fn take(&mut self) -> Option<Modal> {
		self.current.take()
	}
}
