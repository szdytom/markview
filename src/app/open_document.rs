//! macOS hands a double-clicked document to the reader as an Apple Event, not
//! as a command-line argument.
//!
//! The bundle advertises `CFBundleDocumentTypes`, so Finder offers Markview for
//! a Markdown file and launches it with that file packed into an open-documents
//! event. The reader is a plain `NSApplication` with no `NSDocument` subclass,
//! so AppKit answers the event itself — with its own "cannot open files in the
//! ... format" dialog — and the reader starts with nothing to read. Every other
//! desktop passes the document on the command line, which is why only this one
//! needs the hook.
//!
//! The hook watches for the application's launch notification rather than
//! taking the application delegate, which `winit` owns. That moment is the one
//! where the reader's own handler survives: AppKit installs its document
//! handler while launching, so a handler registered earlier is replaced, and
//! the pending event is dispatched the moment launching ends, so a handler
//! registered later never runs.

use super::Event;
#[cfg(target_os = "macos")]
use super::SendEvent;
use winit::event_loop::EventLoopProxy;

#[cfg(target_os = "macos")]
use objc2::AnyThread;
#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2::runtime::NSObject;
#[cfg(target_os = "macos")]
use objc2::{define_class, msg_send, sel};
#[cfg(target_os = "macos")]
use objc2_core_services::{
	kAEOpenDocuments, kCoreEventClass, keyDirectObject, typeFileURL,
};
#[cfg(target_os = "macos")]
use objc2_foundation::{
	NSAppleEventDescriptor, NSAppleEventManager, NSNotification,
	NSNotificationCenter, ns_string,
};
#[cfg(target_os = "macos")]
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::sync::OnceLock;

/// Hands every document the desktop opens to the reader's event loop.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
pub(super) fn install(proxy: EventLoopProxy<Event>) {
	// Both outlive the launch, so both are process-wide. `install` runs once,
	// before the loop starts.
	let _ = PROXY.set(proxy);
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

/// Every other desktop delivers the document on the command line, so there is
/// nothing to listen for.
#[cfg(not(target_os = "macos"))]
pub(super) fn install(_proxy: EventLoopProxy<Event>) {}

/// The loop the reader draws on, handed over before the first window exists.
#[cfg(target_os = "macos")]
static PROXY: OnceLock<EventLoopProxy<Event>> = OnceLock::new();

/// The notification centre holds its observers unretained, so the reader owns
/// the one it registered.
#[cfg(target_os = "macos")]
static OBSERVER: OnceLock<Retained<OpenDocument>> = OnceLock::new();

#[cfg(target_os = "macos")]
define_class!(
	#[unsafe(super(NSObject))]
	#[ivars = ()]
	struct OpenDocument;

	impl OpenDocument {
		/// AppKit installs its own open-documents handler while launching, so
		/// this is the last moment the reader's handler can replace it.
		#[unsafe(method(applicationWillFinishLaunching:))]
		fn will_finish_launching(&self, _notification: &NSNotification) {
			answer_open_documents(self);
		}

		/// Opens one document per file the desktop sent.
		#[unsafe(method(handleOpenDocuments:withReplyEvent:))]
		fn handle_open_documents(
			&self,
			event: &NSAppleEventDescriptor,
			_reply: &NSAppleEventDescriptor,
		) {
			let (Some(proxy), Some(files)) =
				(PROXY.get(), event.paramDescriptorForKeyword(keyDirectObject))
			else {
				return;
			};
			for path in documents(&files) {
				proxy.send(Event::Open(Some(path)));
			}
		}
	}
);

/// Makes the reader, rather than AppKit, answer the desktop's open-documents
/// event.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn answer_open_documents(observer: &OpenDocument) {
	let manager = NSAppleEventManager::sharedAppleEventManager();
	// SAFETY: `handleOpenDocuments:withReplyEvent:` is the selector the observer
	// implements with the matching signature, and the two codes name the event
	// AppKit would otherwise answer itself.
	unsafe {
		manager.setEventHandler_andSelector_forEventClass_andEventID(
			observer,
			sel!(handleOpenDocuments:withReplyEvent:),
			kCoreEventClass,
			kAEOpenDocuments,
		);
	}
}

/// The paths an open-documents event's direct object carries, in the order the
/// desktop sent them.
#[cfg(target_os = "macos")]
fn documents(files: &NSAppleEventDescriptor) -> impl Iterator<Item = PathBuf> {
	(1..=files.numberOfItems()).filter_map(|index| {
		let url = files
			.descriptorAtIndex(index)?
			.coerceToDescriptorType(typeFileURL)?
			.fileURLValue()?;
		Some(PathBuf::from(url.path()?.to_string()))
	})
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
impl OpenDocument {
	fn new() -> Retained<Self> {
		let this = Self::alloc().set_ivars(());
		// SAFETY: `init` is `NSObject`'s designated initializer, and the class
		// carries no instance variables to set up.
		unsafe { msg_send![super(this), init] }
	}
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
	use super::*;
	use objc2_core_services::typeAlias;
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

	/// Finder can send an alias descriptor for an existing document.
	#[test]
	fn alias_descriptor_opens_its_document() {
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().join("aliased.md");
		std::fs::write(&path, "# Document").unwrap();
		let url = NSURL::fileURLWithPath_isDirectory(
			&NSString::from_str(&path.to_string_lossy()),
			false,
		);
		let alias = NSAppleEventDescriptor::descriptorWithFileURL(&url)
			.coerceToDescriptorType(typeAlias)
			.unwrap();
		let files = NSAppleEventDescriptor::listDescriptor();
		files.insertDescriptor_atIndex(&alias, 1);

		assert_eq!(documents(&files).collect::<Vec<_>>(), [path]);
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
