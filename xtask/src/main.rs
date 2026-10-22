//! Repository tasks that must stay out of the shipped crates.
//!
//! `cargo run -p xtask -- icons [--check]` regenerates the platform icon
//! assets from the source SVGs.
use std::process::ExitCode;

mod complexity;
mod coupling;
mod icons;

fn main() -> ExitCode {
	let mut args = std::env::args().skip(1);
	match args.next().as_deref() {
		Some("complexity") if args.next().is_none() => {
			match complexity::run() {
				Ok(()) => ExitCode::SUCCESS,
				Err(error) => {
					eprintln!("{error:#}");
					ExitCode::FAILURE
				}
			}
		}
		Some("coupling") => match coupling_options(args) {
			Ok(options) => match coupling::run(options) {
				Ok(()) => ExitCode::SUCCESS,
				Err(error) => {
					eprintln!("{error:#}");
					ExitCode::FAILURE
				}
			},
			Err(error) => {
				eprintln!("{error:#}");
				ExitCode::FAILURE
			}
		},
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
			eprintln!(
				"Usage: cargo run -p xtask -- icons [--check] | complexity | coupling [--check] [--update] [--json]"
			);
			ExitCode::FAILURE
		}
	}
}

fn coupling_options(
	args: impl Iterator<Item = String>,
) -> anyhow::Result<coupling::Options> {
	let mut options = coupling::Options {
		check: false,
		update: false,
		json: false,
	};
	for arg in args {
		match arg.as_str() {
			"--check" => options.check = true,
			"--update" => options.update = true,
			"--json" => options.json = true,
			_ => anyhow::bail!(
				"Usage: cargo run -p xtask -- coupling [--check] [--update] [--json]"
			),
		}
	}
	if options.check && options.update {
		anyhow::bail!("`--check` and `--update` cannot be combined");
	}
	Ok(options)
}
