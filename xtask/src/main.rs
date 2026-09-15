//! Repository tasks that must stay out of the shipped crates.
//!
//! `cargo run -p xtask -- icons [--check]` regenerates the platform icon
//! assets from the source SVGs.
use std::process::ExitCode;

mod icons;

fn main() -> ExitCode {
	let mut args = std::env::args().skip(1);
	match args.next().as_deref() {
		Some("icons") => {
			let check = args.next().as_deref() == Some("--check");
			if args.next().is_some() {
				eprintln!("Usage: cargo run -p xtask -- icons [--check]");
				return ExitCode::FAILURE;
			}
			match icons::run(check) {
				Ok(()) => ExitCode::SUCCESS,
				Err(error) => {
					eprintln!("{error:#}");
					ExitCode::FAILURE
				}
			}
		}
		_ => {
			eprintln!("Usage: cargo run -p xtask -- icons [--check]");
			ExitCode::FAILURE
		}
	}
}
