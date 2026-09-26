use serde_json::{Value, json};
use std::{
	io::{BufRead, BufReader, Write},
	process::{Child, ChildStdin, ChildStdout, Command, Stdio},
	time::{Duration, Instant},
};

struct Host {
	child: Child,
	input: Option<ChildStdin>,
	output: BufReader<ChildStdout>,
}
impl Host {
	fn new(state: &std::path::Path) -> Self {
		let mut child = Command::new(env!("CARGO_BIN_EXE_markview"))
			.args(["serve", "--offline", "--state-dir"])
			.arg(state)
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::null())
			.spawn()
			.unwrap();
		Self {
			input: child.stdin.take(),
			output: BufReader::new(child.stdout.take().unwrap()),
			child,
		}
	}
	fn send(&mut self, request: &Value) {
		writeln!(self.input.as_mut().unwrap(), "{request}").unwrap();
	}
	fn read(&mut self) -> Value {
		let mut line = String::new();
		self.output.read_line(&mut line).unwrap();
		serde_json::from_str(&line).unwrap()
	}
	fn disconnect(&mut self) {
		drop(self.input.take());
		let deadline = Instant::now() + Duration::from_secs(3);
		loop {
			if let Some(status) = self.child.try_wait().unwrap() {
				assert!(status.success());
				break;
			}
			assert!(Instant::now() < deadline, "engine survived stdin EOF");
			std::thread::sleep(Duration::from_millis(10));
		}
	}
}
impl Drop for Host {
	fn drop(&mut self) {
		let _ = self.child.kill();
		let _ = self.child.wait();
	}
}

#[test]
fn export_service_uses_private_storage_and_exits_on_eof() {
	let state = tempfile::tempdir().unwrap();
	std::fs::create_dir(state.path().join("styles")).unwrap();
	std::fs::write(state.path().join("settings.toml"), "not valid settings")
		.unwrap();
	std::fs::write(state.path().join("styles/private.mvss.toml"),
        "format_version = 2\nversion = 1\ntargets = [\"pdf\"]\n[meta]\nname = \"Private template\"\n").unwrap();
	let mut host = Host::new(state.path());
	writeln!(host.input.as_mut().unwrap(), "malformed").unwrap();
	assert!(host.read().get("error").is_some());
	host.send(&json!({"styles":{}}));
	assert!(
		host.read()["styles"]["templates"]
			.as_array()
			.unwrap()
			.iter()
			.any(|entry| entry["id"] == "private")
	);
	host.disconnect();
	assert_eq!(
		std::fs::read_to_string(state.path().join("settings.toml")).unwrap(),
		"not valid settings"
	);
}

#[cfg(unix)]
#[test]
fn eof_cancels_an_export_blocked_on_a_resource() {
	let dir = tempfile::tempdir().unwrap();
	let fifo = dir.path().join("blocked.png");
	assert!(
		Command::new("mkfifo")
			.arg(&fifo)
			.status()
			.unwrap()
			.success()
	);
	let mut host = Host::new(&dir.path().join("state"));
	host.send(&json!({"open":{"id":"doc","path":dir.path().join("source.md"),"text":"![blocked](blocked.png)"}}));
	assert!(host.read().get("opened").is_some());
	let (send, connected) = std::sync::mpsc::channel();
	std::thread::spawn(move || {
		let writer =
			std::fs::OpenOptions::new().write(true).open(fifo).unwrap();
		let _ = send.send(writer);
	});
	host.send(
		&json!({"export":{"id":"doc","output":dir.path().join("out.pdf")}}),
	);
	// A connected FIFO writer proves export is blocked inside a resource read.
	let writer = connected.recv_timeout(Duration::from_secs(5)).unwrap();
	host.disconnect();
	drop(writer);
	assert!(!dir.path().join("out.pdf").exists());
	assert!(!std::fs::read_dir(dir.path()).unwrap().any(|entry| {
		entry
			.unwrap()
			.file_name()
			.to_string_lossy()
			.starts_with(".markview-export-")
	}));
}
