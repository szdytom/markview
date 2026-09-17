// A release build links for the Windows subsystem, so double-clicking the
// reader opens no console window. The standard handles are then valid only
// when the launcher passed them on, which is why console-facing output goes
// through `logging::report` instead of `println!`.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> anyhow::Result<()> {
	markview::app::run()
}
