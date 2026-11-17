//! AppKit owns the asynchronous file and folder chooser on the main thread.
use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSModalResponse, NSModalResponseOK, NSOpenPanel};
use objc2_foundation::{NSArray, NSString};
use objc2_uniform_type_identifiers::UTType;
use std::{cell::Cell, path::PathBuf};

pub(super) fn pick(files: bool, done: impl FnOnce(Option<PathBuf>) + 'static) {
	let panel = NSOpenPanel::openPanel(MainThreadMarker::new().unwrap());
	panel.setCanChooseFiles(files);
	panel.setCanChooseDirectories(true);
	panel.setAllowsMultipleSelection(false);
	if files {
		panel.setAllowedContentTypes(&document_types());
	}
	let selected = panel.clone();
	let done = Cell::new(Some(done));
	panel.beginWithCompletionHandler(&RcBlock::new(
		move |response: NSModalResponse| {
			let path = (response == NSModalResponseOK)
				.then(|| selected.URL().and_then(|url| url.to_file_path()))
				.flatten();
			done.take().unwrap()(path);
		},
	));
}

fn document_types() -> Retained<NSArray<UTType>> {
	let mut types: Vec<_> = ["md", "markdown", "mdown", "txt"]
		.into_iter()
		.filter_map(|extension| {
			UTType::typeWithFilenameExtension(&NSString::from_str(extension))
		})
		.collect();
	// Directories must pass the content filter as well as `canChooseDirectories`.
	types.push(
		UTType::typeWithIdentifier(&NSString::from_str("public.folder"))
			.unwrap(),
	);
	NSArray::from_retained_slice(&types)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn document_filter_accepts_markdown_files_and_folders() {
		let types = document_types();
		let accepts = |candidate: &UTType| {
			types
				.iter()
				.any(|allowed| candidate.conformsToType(&allowed))
		};
		let folder =
			UTType::typeWithIdentifier(&NSString::from_str("public.folder"))
				.unwrap();
		assert!(accepts(&folder));
		for extension in ["md", "markdown", "mdown", "txt"] {
			let kind = UTType::typeWithFilenameExtension(&NSString::from_str(
				extension,
			))
			.unwrap();
			assert!(accepts(&kind), "{extension}");
		}
	}
}
