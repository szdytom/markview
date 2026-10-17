use std::{
	io::{Read, Write},
	net::TcpListener,
	path::Path,
	process::{Command, Output},
	sync::{
		Arc,
		atomic::{AtomicU8, Ordering},
	},
	time::Duration,
};

fn command() -> Command {
	let mut command = Command::new(env!("CARGO_BIN_EXE_markview"));
	command.arg("fonts");
	command
}
fn json(output: Output) -> serde_json::Value {
	assert!(
		output.status.success(),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
	serde_json::from_slice(
		output
			.stdout
			.split(|byte| *byte == b'\n')
			.rfind(|line| !line.is_empty())
			.unwrap(),
	)
	.unwrap()
}

#[test]
fn catalog_resolves_targets_literal_families_and_sources_without_stdin() {
	for destination in ["ui", "pdf"] {
		let catalog = json(
			command()
				.args(["catalog", "--destination", destination])
				.output()
				.unwrap(),
		);
		assert!(!catalog["templates"].as_array().unwrap().is_empty());
		assert!(!catalog["definitions"].as_array().unwrap().is_empty());
	}
	let dir = tempfile::tempdir().unwrap();
	let style = dir.path().join("template.mvss.toml");
	std::fs::write(&style, "format_version=2\nversion=1\ntargets=['pdf']\n[mermaid]\nfont_family=['Diagram Literal']\n[[rule]]\nwhen=['code_block']\nfont=[{family='Code Literal'}]\n").unwrap();
	let catalog = json(
		command()
			.args(["catalog", "--style", "monochrome"])
			.arg("--file")
			.arg(&style)
			.output()
			.unwrap(),
	);
	let inherited = json(
		command()
			.args(["download", "noto-serif", "--file"])
			.arg(&style)
			.args([
				"--destination",
				"pdf",
				"--dry-run",
				"--offline",
				"--json",
				"--force",
			])
			.output()
			.unwrap(),
	);
	assert_eq!(inherited["families"], serde_json::json!(["noto-serif"]));
	for name in ["Code Literal", "Diagram Literal"] {
		assert!(
			catalog["definitions"]
				.as_array()
				.unwrap()
				.iter()
				.any(|item| item["lookfor"][0] == name)
		);
	}
	assert!(
		!command()
			.args(["catalog", "--destination", "ui", "--file"])
			.arg(&style)
			.output()
			.unwrap()
			.status
			.success()
	);
}

#[test]
fn downloads_validate_bytes_retry_and_observe_cancellation() {
	let dir = tempfile::tempdir().unwrap();
	let style = dir.path().join("template.mvss.toml");
	let marker = dir.path().join("cancel");
	let cache = dir.path().join("fonts");

	let bytes = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(
		"crates/markview-layout/tests/fonts/NotoSerif-Regular-subset.otf",
	))
	.unwrap();
	let listener = TcpListener::bind("127.0.0.1:0").unwrap();
	listener.set_nonblocking(true).unwrap();
	let proxy = format!("http://{}", listener.local_addr().unwrap());
	std::fs::write(&style, format!("format_version=2\nversion=1\ntargets=['pdf','ui']\n[[fontdef]]\nid='serif'\nlookfor=['Noto Serif']\n[[font-family]]\nid='fixture'\nlookfor=['Noto Serif']\nlicense='OFL-1.1'\n[[font-family.source]]\nfiles=['{proxy}/fixture.otf']\n")).unwrap();
	let mode = Arc::new(AtomicU8::new(0));
	let (server_mode, server_bytes, server_marker) =
		(mode.clone(), bytes.clone(), marker.clone());
	let server = std::thread::spawn(move || {
		while server_mode.load(Ordering::SeqCst) != 3 {
			let Ok((mut socket, _)) = listener.accept() else {
				std::thread::sleep(Duration::from_millis(5));
				continue;
			};
			socket
				.set_read_timeout(Some(Duration::from_secs(1)))
				.unwrap();
			let mut request = Vec::new();
			let mut byte = [0];
			while !request.ends_with(b"\r\n\r\n")
				&& socket.read_exact(&mut byte).is_ok()
			{
				request.push(byte[0]);
			}
			if write!(
				socket,
				"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
				server_bytes.len()
			)
			.is_err()
			{
				continue;
			}
			if request.starts_with(b"HEAD ") {
				continue;
			}
			match server_mode.load(Ordering::SeqCst) {
				0 => {
					let _ = socket.write_all(&vec![0; server_bytes.len()]);
				}
				1 => {
					let _ = socket.write_all(&server_bytes);
				}
				2 => {
					let _ = socket.write_all(&server_bytes[..10]);
					std::fs::write(&server_marker, "").unwrap();
					while server_mode.load(Ordering::SeqCst) == 2 {
						std::thread::sleep(Duration::from_millis(5));
					}
				}
				_ => {}
			}
		}
	});
	let run = |destination: &Path| {
		let mut cmd = command();
		cmd.args(["download", "fixture", "--json", "--file"])
			.arg(&style)
			.arg("--directory")
			.arg(destination)
			.arg("--cancel-file")
			.arg(&marker);
		cmd.output().unwrap()
	};
	let dry = json(
		command()
			.args([
				"download",
				"fixture",
				"--json",
				"--dry-run",
				"--offline",
				"--file",
			])
			.arg(&style)
			.arg("--directory")
			.arg(&cache)
			.output()
			.unwrap(),
	);
	assert_eq!(dry["families"], serde_json::json!(["fixture"]));
	let listed = command()
		.args(["list", "--all", "--file"])
		.arg(&style)
		.output()
		.unwrap();
	assert!(listed.status.success());
	assert!(String::from_utf8_lossy(&listed.stdout).contains("fixture"));
	let invalid = run(&cache);
	mode.store(1, Ordering::SeqCst);
	let valid = run(&cache);
	let unchanged = run(&cache);
	let forced = command()
		.args(["download", "fixture", "--json", "--force", "--file"])
		.arg(&style)
		.arg("--directory")
		.arg(&cache)
		.output()
		.unwrap();
	mode.store(2, Ordering::SeqCst);
	let cancelled_dir = dir.path().join("cancelled");
	let cancelled = run(&cancelled_dir);
	mode.store(3, Ordering::SeqCst);
	server.join().unwrap();
	assert!(!invalid.status.success());
	assert_eq!(json(unchanged)["stored"], 0);
	assert_eq!(json(forced)["stored"], 1);
	assert!(
		String::from_utf8_lossy(&valid.stdout)
			.contains("\"type\":\"progress\"")
	);
	assert_eq!(json(valid)["type"], "done");
	assert!(
		std::fs::read_dir(cache)
			.unwrap()
			.filter_map(Result::ok)
			.any(|entry| std::fs::read(entry.path())
				.is_ok_and(|data| data == bytes))
	);
	assert!(!cancelled.status.success());
	assert!(String::from_utf8_lossy(&cancelled.stderr).contains("Cancelled"));
	assert!(
		!std::fs::read_dir(cancelled_dir)
			.unwrap()
			.filter_map(Result::ok)
			.any(|entry| entry
				.path()
				.extension()
				.is_some_and(|ext| ext == "otf"))
	);
	assert!(
		!command()
			.args(["download", "fixture", "--json", "--offline", "--directory"])
			.arg(dir.path().join("offline"))
			.arg("--file")
			.arg(style)
			.output()
			.unwrap()
			.status
			.success()
	);
}
