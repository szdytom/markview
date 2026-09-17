//! Process-wide console output: `log`-based stderr logging, filtered by
//! `RUST_LOG`, and the plain stdout the diagnostic entry points report on.
//!
//! Lines are `LEVEL message`, with no timestamp or module target: the reader
//! prints a handful of lifecycle lines and diagnostics, and the tracked
//! performance scripts parse them. The default level depends on the launch
//! mode, so the window stays quiet while the diagnostic entry points report
//! their timings.
use crate::cli::Mode;
use env_logger::{Builder, Env};
use std::io::Write;

/// Reports a diagnostic result on stdout.
///
/// A release build is linked for the Windows subsystem, so the standard
/// handles exist only when the launcher passed them on. Output with nowhere
/// to go is dropped rather than turned into a panic.
pub(crate) fn report(arguments: std::fmt::Arguments<'_>) {
	let _ = std::io::stdout().write_fmt(arguments);
}

/// Install the logger once, after the command line fixed the mode.
pub(crate) fn init(mode: &Mode) {
	// Targets match by prefix, so `markview` also covers `markview_core`.
	let default = match mode {
		Mode::Window => "warn",
		Mode::Render | Mode::Bench | Mode::Latency | Mode::Smoke => {
			"warn,markview=debug"
		}
	};
	Builder::from_env(Env::default().default_filter_or(default))
		.format(|buffer, record| {
			writeln!(buffer, "{} {}", record.level(), record.args())
		})
		.init();
}
