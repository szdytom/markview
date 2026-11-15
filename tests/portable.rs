use std::{
	fs,
	io::{Read, Write},
	net::TcpListener,
	path::{Path, PathBuf},
	process::{Command, Output},
	thread,
	time::{Duration, Instant},
};

const MARKER: &str = "markview-portable-marker.txt";

struct Installation {
	_root: tempfile::TempDir,
	directory: PathBuf,
	system: PathBuf,
	working: PathBuf,
}
impl Installation {
	fn new() -> Self {
		let root = tempfile::tempdir().unwrap();
		let directory = root.path().join("portable 中文 reader");
		let system = root.path().join("system");
		let working = root.path().join("working");
		for path in [&directory, &working, &system.join("markview")] {
			fs::create_dir_all(path).unwrap();
		}
		fs::copy(env!("CARGO_BIN_EXE_markview"), directory.join(Self::name()))
			.unwrap();
		Self {
			_root: root,
			directory,
			system,
			working,
		}
	}

	fn name() -> &'static std::ffi::OsStr {
		Path::new(env!("CARGO_BIN_EXE_markview"))
			.file_name()
			.unwrap()
	}

	fn command(&self) -> Command {
		let mut command = Command::new(self.directory.join(Self::name()));
		command
			.current_dir(&self.working)
			.env("APPDATA", &self.system)
			.env("XDG_CONFIG_HOME", &self.system);
		command
	}

	fn run(&self, args: &[&str]) -> Output {
		self.command().args(args).output().unwrap()
	}

	fn fonts_path(&self, expected: &Path) {
		let output = self.run(&["fonts", "path"]);
		assert!(output.status.success(), "{output:?}");
		assert_eq!(
			Path::new(String::from_utf8(output.stdout).unwrap().trim()),
			expected
		);
	}
}

#[test]
fn portable_data_follows_the_executable_and_never_falls_back_on_write_failure()
{
	if !markview_platform::test_support::portable_mode() {
		return;
	}
	let mut installation = Installation::new();
	let system = installation.system.join("markview");
	fs::write(system.join("settings.toml"), "single-instance = true\n")
		.unwrap();
	fs::write(installation.working.join(MARKER), "").unwrap();
	installation.fonts_path(&system.join("fonts"));
	fs::write(installation.directory.join(MARKER), [0xff, 0x00]).unwrap();
	let data = installation.directory.join("data");
	installation.fonts_path(&data.join("fonts"));
	assert!(!data.exists());
	let source = Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("examples/ui-cjk-medium.mvss.toml");
	let output = installation
		.command()
		.args(["ss", "install"])
		.arg(&source)
		.output()
		.unwrap();
	assert!(output.status.success(), "{output:?}");
	let style = data.join("styles/ui-cjk-medium.mvss.toml");
	assert_eq!(fs::read(&style).unwrap(), fs::read(&source).unwrap());
	let moved = installation.directory.with_file_name("moved 中文 reader");
	fs::rename(&installation.directory, &moved).unwrap();
	installation.directory = moved;
	installation.fonts_path(&installation.directory.join("data/fonts"));
	assert_eq!(
		fs::read(
			installation
				.directory
				.join("data/styles/ui-cjk-medium.mvss.toml")
		)
		.unwrap(),
		fs::read(&source).unwrap()
	);
	fs::remove_dir_all(installation.directory.join("data")).unwrap();
	fs::write(installation.directory.join("data"), "not a directory").unwrap();
	let output = installation
		.command()
		.args(["ss", "install"])
		.arg(source)
		.output()
		.unwrap();
	assert!(!output.status.success(), "{output:?}");
	assert!(!system.join("styles").exists());
	assert_eq!(
		fs::read_to_string(system.join("settings.toml")).unwrap(),
		"single-instance = true\n"
	);
	fs::remove_file(installation.directory.join(MARKER)).unwrap();
	installation.fonts_path(&system.join("fonts"));
}

struct Peer {
	_lock: fs::File,
	listener: TcpListener,
	token: String,
}
impl Peer {
	fn new(directory: &Path, token: &str) -> Self {
		fs::create_dir_all(directory).unwrap();
		fs::write(directory.join("settings.toml"), "single-instance = true\n")
			.unwrap();
		let lock = fs::File::create(directory.join("instance.lock")).unwrap();
		lock.lock().unwrap();
		let listener = TcpListener::bind("127.0.0.1:0").unwrap();
		listener.set_nonblocking(true).unwrap();
		fs::write(
			directory.join("instance.json"),
			serde_json::to_vec(&serde_json::json!({
				"address": listener.local_addr().unwrap(), "token": token
			}))
			.unwrap(),
		)
		.unwrap();
		Self {
			_lock: lock,
			listener,
			token: token.into(),
		}
	}

	fn receive(&self, installation: &Installation) {
		let mut child = installation
			.command()
			.args(["--foreground", "notes.md"])
			.spawn()
			.unwrap();
		let deadline = Instant::now() + Duration::from_secs(10);
		let mut stream = loop {
			match self.listener.accept() {
				Ok((stream, _)) => break stream,
				Err(error)
					if error.kind() == std::io::ErrorKind::WouldBlock => {}
				Err(error) => panic!("{error}"),
			}
			if Instant::now() > deadline {
				let _ = child.kill();
				let _ = child.wait();
				panic!("reader did not contact the expected instance");
			}
			thread::sleep(Duration::from_millis(10));
		};
		// Accepted sockets inherit the listener's nonblocking mode.
		stream.set_nonblocking(false).unwrap();
		stream
			.set_read_timeout(Some(Duration::from_secs(5)))
			.unwrap();
		let mut request = Vec::new();
		stream.read_to_end(&mut request).unwrap();
		let request: serde_json::Value =
			serde_json::from_slice(&request).unwrap();
		assert_eq!(request["token"], self.token);
		assert_eq!(
			request["path"],
			serde_json::to_value(
				installation.working.join("notes.md").into_os_string()
			)
			.unwrap()
		);
		stream.write_all(&[1]).unwrap();
		assert!(child.wait().unwrap().success());
	}
}

#[test]
fn portable_instances_are_isolated_from_each_other_and_the_system_instance() {
	if !markview_platform::test_support::portable_mode() {
		return;
	}
	let first = Installation::new();
	let mut second = Installation::new();
	second.system = first.system.clone();
	let system = Peer::new(&first.system.join("markview"), "system");
	for installation in [&first, &second] {
		fs::write(installation.directory.join(MARKER), "").unwrap();
	}
	let first_peer = Peer::new(&first.directory.join("data"), "first");
	let second_peer = Peer::new(&second.directory.join("data"), "second");
	for (installation, peer) in [(&first, &first_peer), (&second, &second_peer)]
	{
		peer.receive(installation);
		peer.receive(installation);
	}
	fs::remove_file(first.directory.join(MARKER)).unwrap();
	system.receive(&first);
}
