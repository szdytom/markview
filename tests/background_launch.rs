use std::{
	io::{Read, Write},
	net::TcpListener,
	path::Path,
	process::Command,
	sync::mpsc,
	thread,
	time::{Duration, Instant},
};

#[test]
fn reader_releases_the_shell_while_the_background_process_is_running() {
	if !markview_platform::test_support::shell_launch() {
		return;
	}
	for arguments in
		[vec![], vec!["notes.md"], vec!["--foreground", "notes.md"]]
	{
		let directory = tempfile::tempdir().unwrap();
		let config = directory.path().join("markview");
		std::fs::create_dir(&config).unwrap();
		std::fs::write(
			config.join("settings.toml"),
			"single-instance = true\n",
		)
		.unwrap();
		let lock = std::fs::File::create(config.join("instance.lock")).unwrap();
		lock.lock().unwrap();
		let listener = TcpListener::bind("127.0.0.1:0").unwrap();
		std::fs::write(
			config.join("instance.json"),
			serde_json::to_vec(&serde_json::json!({
				"address": listener.local_addr().unwrap(), "token": "test"
			}))
			.unwrap(),
		)
		.unwrap();
		let (requests, received) = mpsc::channel();
		let (acknowledge, ack) = mpsc::channel();
		let server = thread::spawn(move || {
			let (mut stream, _) = listener.accept().unwrap();
			stream
				.set_read_timeout(Some(Duration::from_secs(5)))
				.unwrap();
			let mut request = Vec::new();
			stream.read_to_end(&mut request).unwrap();
			requests.send(request).unwrap();
			ack.recv_timeout(Duration::from_secs(5)).unwrap();
			stream.write_all(&[1]).unwrap();
		});
		let mut reader = Command::new(env!("CARGO_BIN_EXE_markview"))
			.args(&arguments)
			.env("XDG_CONFIG_HOME", directory.path())
			.env("XDG_DATA_HOME", directory.path())
			.stdout(std::process::Stdio::piped())
			.stderr(std::process::Stdio::piped())
			.spawn()
			.unwrap();
		let request: serde_json::Value = serde_json::from_slice(
			&received.recv_timeout(Duration::from_secs(5)).unwrap(),
		)
		.unwrap();
		assert_eq!(request["token"], "test");
		assert_eq!(request["path"].is_null(), arguments.is_empty());
		if !arguments.is_empty() {
			assert_eq!(
				request["path"],
				serde_json::to_value(
					std::env::current_dir()
						.unwrap()
						.join("notes.md")
						.into_os_string()
				)
				.unwrap()
			);
		}
		let foreground = arguments.contains(&"--foreground");
		if foreground {
			assert!(reader.try_wait().unwrap().is_none());
			acknowledge.send(()).unwrap();
		}
		let (finished, output) = mpsc::channel();
		thread::spawn(move || {
			finished.send(reader.wait_with_output()).unwrap()
		});
		// The IPC peer has not acknowledged: the detached reader is still alive.
		let result = output.recv_timeout(Duration::from_secs(1));
		if !foreground {
			acknowledge.send(()).unwrap();
		}
		server.join().unwrap();
		let result = result
			.expect("the shell must return before the IPC peer replies")
			.unwrap();
		assert!(result.status.success(), "{:?}", result.stderr);
		assert!(result.stdout.is_empty() && result.stderr.is_empty());
	}
}

#[test]
fn help_errors_and_stylesheet_commands_stay_synchronous() {
	if !markview_platform::test_support::shell_launch() {
		return;
	}
	let directory = tempfile::tempdir().unwrap();
	for (arguments, success, expected) in [
		(vec!["--help"], true, "--foreground"),
		(vec!["web"], false, "<URL>"),
		(vec!["render", "notes.md"], false, "--output"),
		(
			vec!["ss", "validate", "examples/ui-cjk-medium.mvss.toml"],
			true,
			"Valid stylesheet:",
		),
		(
			vec!["ss", "install", "examples/ui-cjk-medium.mvss.toml"],
			true,
			"Installed",
		),
	] {
		let result = Command::new(env!("CARGO_BIN_EXE_markview"))
			.args(arguments)
			.env("XDG_CONFIG_HOME", directory.path())
			.env("XDG_DATA_HOME", directory.path())
			.output()
			.unwrap();
		assert_eq!(result.status.success(), success, "{:?}", result.stderr);
		let text = format!(
			"{}{}",
			String::from_utf8_lossy(&result.stdout),
			String::from_utf8_lossy(&result.stderr)
		);
		assert!(text.contains(expected), "{text}");
	}
}

#[test]
fn startup_errors_use_dialogs_for_background_and_stderr_for_foreground() {
	if !markview_platform::test_support::shell_launch() {
		return;
	}

	let directory = tempfile::tempdir().unwrap();
	let bin = directory.path().join("bin");
	std::fs::create_dir(&bin).unwrap();
	let zenity = bin.join("zenity");
	std::fs::write(
		&zenity,
		"#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$MARKVIEW_TEST_DIALOG.tmp\"\nmv \"$MARKVIEW_TEST_DIALOG.tmp\" \"$MARKVIEW_TEST_DIALOG\"\n",
	)
	.unwrap();
	markview_platform::test_support::set_mode(&zenity, 0o755).unwrap();
	let dialog = directory.path().join("dialog.txt");
	let styles = directory.path().join("markview/styles");
	std::fs::create_dir_all(&styles).unwrap();
	std::fs::write(styles.join("malformed.mvss.toml"), "format_version = [")
		.unwrap();
	std::fs::write(
		styles.join("incompatible.mvss.toml"),
		"format_version=3\nversion=1\ntargets=['pdf']\n[[rule]]\nwhen=['body']\ncolor='#123456'",
	)
	.unwrap();
	let path = std::env::join_paths(
		std::iter::once(bin)
			.chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
	)
	.unwrap();
	let command = || {
		let mut command = Command::new(env!("CARGO_BIN_EXE_markview"));
		command
			.env("PATH", &path)
			.env("XDG_CONFIG_HOME", directory.path())
			.env("XDG_DATA_HOME", directory.path())
			.env("MARKVIEW_TEST_DIALOG", &dialog);
		command
	};
	for (arguments, style, cause) in [
		(vec![], "missing", "Cannot read"),
		(vec!["notes.md"], "malformed", "TOML parse error"),
		(
			vec!["web", "https://example.org", "--offline"],
			"incompatible",
			"targets do not include ui",
		),
	] {
		let result = command()
			.args(&arguments)
			.args(["--style", style])
			.output()
			.unwrap();
		assert!(result.status.success(), "{:?}", result.stderr);
		assert!(result.stdout.is_empty() && result.stderr.is_empty());
		let message = wait_for_dialog(&dialog);
		assert!(
			message.contains("--error\n--title\nMarkview\n--text\n"),
			"{message}"
		);
		assert!(
			message.contains(style) && message.contains(cause),
			"{message}"
		);
		std::fs::remove_file(&dialog).unwrap();

		let result = command()
			.arg("--foreground")
			.args(arguments)
			.args(["--style", style])
			.output()
			.unwrap();
		assert_eq!(result.status.code(), Some(1));
		assert!(String::from_utf8_lossy(&result.stderr).contains(cause));
		assert!(!dialog.exists());
	}
	let result = command()
		.args([
			"render",
			"notes.md",
			"--style",
			"missing",
			"-o",
			"unused.png",
		])
		.output()
		.unwrap();
	assert_eq!(result.status.code(), Some(1));
	assert!(String::from_utf8_lossy(&result.stderr).contains("Cannot read"));
	assert!(!dialog.exists());
	// An unavailable display fails after stylesheet loading, before a window exists.
	let result = command()
		.env("DISPLAY", "invalid-display")
		.env(
			"WAYLAND_DISPLAY",
			directory.path().join("missing-wayland-socket"),
		)
		.env_remove("WINIT_UNIX_BACKEND")
		.output()
		.unwrap();
	assert!(result.status.success());
	let message = wait_for_dialog(&dialog);
	assert!(
		message.contains("--error")
			&& message.contains("Could not find wayland compositor"),
		"{message}"
	);
}

fn wait_for_dialog(path: &Path) -> String {
	let deadline = Instant::now() + Duration::from_secs(5);
	while !path.exists() {
		assert!(
			Instant::now() < deadline,
			"the native error dialog was not requested"
		);
		thread::sleep(Duration::from_millis(10));
	}
	std::fs::read_to_string(path).unwrap()
}
