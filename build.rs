//! Embeds the executable icon into Windows builds.
//!
//! Non-Windows builds have no resources to compile, so this stays empty.
use std::io::Write;

fn main() {
	println!("cargo:rerun-if-changed=build.rs");
	let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
	if target != "windows" {
		return;
	}
	let icon = "assets/icons/markview.ico";
	println!("cargo:rerun-if-changed={icon}");
	if let Err(error) = embed_windows_icon(icon) {
		// A missing resource compiler must not turn into a broken build.
		let _ = writeln!(
			std::io::stderr(),
			"warning: cannot embed the Windows icon: {error}"
		);
	}
}

#[cfg(windows)]
fn embed_windows_icon(icon: &str) -> std::io::Result<()> {
	let mut resource = winresource::WindowsResource::new();
	resource
		.set_icon(icon)
		.set("FileDescription", "Markview")
		.set("ProductName", "Markview")
		.set("LegalCopyright", "MIT licensed");
	resource.compile()
}

#[cfg(not(windows))]
fn embed_windows_icon(_icon: &str) -> std::io::Result<()> {
	Ok(())
}
