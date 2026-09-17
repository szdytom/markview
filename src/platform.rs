//! OS effects stay outside the reading core.

/// Attaches the process to the console it was launched from.
///
/// A release build runs in the Windows subsystem, so the standard handles are
/// inherited only from launchers that pass them on. Attaching gives the
/// diagnostic entry points a console to report on; an Explorer launch has no
/// parent console to borrow, and its output is dropped.
#[cfg(windows)]
#[allow(unsafe_code)]
pub(crate) fn attach_parent_console() {
	// SAFETY: `AttachConsole` reads one process id and attaches this process
	// to that console; failing leaves the process unchanged.
	unsafe {
		let _ = windows_sys::Win32::System::Console::AttachConsole(
			windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
		);
	}
}

/// Other platforms keep the console they were launched from.
#[cfg(not(windows))]
pub(crate) fn attach_parent_console() {}

#[derive(Default)]
pub struct Clipboard {
	inner: Option<arboard::Clipboard>,
}
impl Clipboard {
	pub fn read(&mut self) -> anyhow::Result<String> {
		if self.inner.is_none() {
			self.inner = Some(arboard::Clipboard::new()?);
		}
		Ok(self.inner.as_mut().unwrap().get_text()?)
	}

	pub fn write(&mut self, text: String) -> anyhow::Result<()> {
		if self.inner.is_none() {
			self.inner = Some(arboard::Clipboard::new()?);
		}
		self.inner.as_mut().unwrap().set_text(text)?;
		Ok(())
	}
}
