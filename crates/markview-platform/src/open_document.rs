//! macOS document-open and application-reopen events.
use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{define_class, msg_send, sel};
use objc2_core_services::{
	kAEOpenDocuments, kAEReopenApplication, kCoreEventClass, keyDirectObject,
};
use objc2_foundation::{
	NSAppleEventDescriptor, NSAppleEventManager, NSNotification,
	NSNotificationCenter, ns_string,
};
use std::path::PathBuf;
use std::sync::OnceLock;

#[allow(unsafe_code)]
pub fn install() {
	// Both outlive the launch, so both are process-wide. `install` runs once,
	// before the loop starts.
	let observer = OpenDocument::new();
	// SAFETY: The observer is the class that implements the selector it is
	// registered for, and the notification centre holds it unretained — which
	// is why the static below keeps it alive.
	unsafe {
		NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
			&observer,
			sel!(applicationWillFinishLaunching:),
			Some(ns_string!("NSApplicationWillFinishLaunchingNotification")),
			None,
		);
	}
	let _ = OBSERVER.set(observer);
}

/// The loop the reader draws on, handed over before the first window exists.
/// The notification centre holds its observers unretained, so the reader owns
/// the one it registered.
static OBSERVER: OnceLock<Retained<OpenDocument>> = OnceLock::new();

define_class!(
	#[unsafe(super(NSObject))]
	#[ivars = ()]
	struct OpenDocument;

	impl OpenDocument {
		/// AppKit installs its own open-documents handler while launching, so
		/// this is the last moment the reader's handler can replace it.
		#[unsafe(method(applicationWillFinishLaunching:))]
		fn will_finish_launching(&self, _notification: &NSNotification) {
			answer_application_events(self);
		}

		#[unsafe(method(handleReopen:withReplyEvent:))]
		fn handle_reopen(
			&self,
			_event: &NSAppleEventDescriptor,
			_reply: &NSAppleEventDescriptor,
		) {
			crate::events::send(crate::NativeEvent::Reopen);
		}

		/// Opens one document per file the desktop sent.
		#[unsafe(method(handleOpenDocuments:withReplyEvent:))]
		fn handle_open_documents(
			&self,
			event: &NSAppleEventDescriptor,
			_reply: &NSAppleEventDescriptor,
		) {
			let Some(files) = event.paramDescriptorForKeyword(keyDirectObject)
			else {
				return;
			};
			for path in documents(&files) {
				crate::events::send(crate::NativeEvent::Open(path));
			}
		}
	}
);

/// Makes the reader answer document-open and Dock-reopen events.
#[allow(unsafe_code)]
fn answer_application_events(observer: &OpenDocument) {
	let manager = NSAppleEventManager::sharedAppleEventManager();
	// SAFETY: Both selectors are implemented above with matching signatures;
	// `OBSERVER` keeps the event target alive for the process.
	unsafe {
		manager.setEventHandler_andSelector_forEventClass_andEventID(
			observer,
			sel!(handleOpenDocuments:withReplyEvent:),
			kCoreEventClass,
			kAEOpenDocuments,
		);
		manager.setEventHandler_andSelector_forEventClass_andEventID(
			observer,
			sel!(handleReopen:withReplyEvent:),
			kCoreEventClass,
			kAEReopenApplication,
		);
	}
}

/// The paths an open-documents event's direct object carries, in the order the
/// desktop sent them.
fn documents(files: &NSAppleEventDescriptor) -> impl Iterator<Item = PathBuf> {
	(1..=files.numberOfItems()).filter_map(|index| {
		let url = files.descriptorAtIndex(index)?.fileURLValue()?;
		Some(PathBuf::from(url.path()?.to_string()))
	})
}

#[allow(unsafe_code)]
impl OpenDocument {
	fn new() -> Retained<Self> {
		let this = Self::alloc().set_ivars(());
		// SAFETY: `init` is `NSObject`'s designated initializer, and the class
		// carries no instance variables to set up.
		unsafe { msg_send![super(this), init] }
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use objc2_foundation::{NSString, NSURL};

	/// The event's direct object is a list with one file URL per document.
	#[test]
	fn every_file_of_the_list_is_a_document() {
		let files = NSAppleEventDescriptor::listDescriptor();
		for name in ["first.md", "second.md"] {
			let url = NSURL::fileURLWithPath_isDirectory(
				&NSString::from_str(&format!("/tmp/{name}")),
				false,
			);
			files.insertDescriptor_atIndex(
				&NSAppleEventDescriptor::descriptorWithFileURL(&url),
				files.numberOfItems() + 1,
			);
		}
		let documents: Vec<_> = documents(&files).collect();
		assert_eq!(
			documents,
			[
				PathBuf::from("/tmp/first.md"),
				PathBuf::from("/tmp/second.md")
			]
		);
	}

	/// An event that carries nothing has nothing to open.
	#[test]
	fn an_empty_list_opens_nothing() {
		assert_eq!(
			documents(&NSAppleEventDescriptor::listDescriptor()).count(),
			0
		);
	}
}
