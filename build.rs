//! Records build diagnostics and embeds the Windows executable icon.
use std::io::Write;

fn main() {
	println!("cargo:rerun-if-changed=build.rs");
	build_metadata();
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

fn git(args: &[&str]) -> Option<String> {
	let output = std::process::Command::new("git").args(args).output().ok()?;
	output
		.status
		.success()
		.then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn build_metadata() {
	for path in ["src", "crates", "assets", "Cargo.toml", "Cargo.lock"] {
		println!("cargo:rerun-if-changed={path}");
	}
	for name in ["HEAD", "packed-refs", "refs"] {
		if let Some(path) = git(&["rev-parse", "--git-path", name])
			&& std::path::Path::new(&path).exists()
		{
			println!("cargo:rerun-if-changed={path}");
		}
	}
	if let Some(reference) = git(&["symbolic-ref", "-q", "HEAD"])
		&& let Some(path) = git(&["rev-parse", "--git-path", &reference])
		&& std::path::Path::new(&path).exists()
	{
		println!("cargo:rerun-if-changed={path}");
	}
	let commit = git(&["rev-parse", "HEAD"])
		.unwrap_or_else(|| "Unknown (source archive)".into());
	println!("cargo:rustc-env=MARKVIEW_COMMIT={commit}");
	println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
	let seconds = std::env::var("SOURCE_DATE_EPOCH")
		.ok()
		.map(|value| {
			value
				.parse::<u64>()
				.expect("SOURCE_DATE_EPOCH must be Unix seconds")
		})
		.unwrap_or_else(|| {
			std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.unwrap()
				.as_secs()
		});
	let mut days = seconds / 86400;
	let mut year = 1970;
	let leap = |year: u64| {
		year.is_multiple_of(4)
			&& (!year.is_multiple_of(100) || year.is_multiple_of(400))
	};
	loop {
		let length = if leap(year) { 366 } else { 365 };
		if days < length {
			break;
		}
		days -= length;
		year += 1;
	}
	let mut month = 1;
	for length in [
		31,
		if leap(year) { 29 } else { 28 },
		31,
		30,
		31,
		30,
		31,
		31,
		30,
		31,
		30,
		31,
	] {
		if days < length {
			break;
		}
		days -= length;
		month += 1;
	}
	println!(
		"cargo:rustc-env=MARKVIEW_BUILD_DATE={year:04}-{month:02}-{:02} UTC",
		days + 1
	);
}
