//! Desktop application and platform services.
pub use markview_core::{document, layout, paginate, profile};
pub use markview_render as render;
pub mod app;
mod benchmark;
mod cli;
mod diagnostics;
mod export;
mod export_cli;
mod file;
mod fonts;
mod images;
mod lang;
mod latency;
mod link;
mod logging;
mod mailbox;
mod net;
mod paste;
mod pdf;
mod platform;
pub mod security;
mod services;
mod settings;
mod shutdown;
mod state;
mod stylesheet;
#[cfg(test)]
mod test_support;
mod watch;
mod web_page;
mod worker;

use std::time::Instant;

static PROCESS_START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

/// Marks process entry, so diagnostic modes can report true end-to-end latency
/// including dynamic linking, font discovery and GPU initialization.
pub fn mark_process_start() {
	let _ = PROCESS_START.set(Instant::now());
}

/// The instant the process started, or the first call if `main` did not mark it.
pub fn process_started() -> Instant {
	*PROCESS_START.get_or_init(Instant::now)
}
