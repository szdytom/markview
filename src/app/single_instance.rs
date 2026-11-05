//! Per-user window discovery and authenticated local file-open requests.
use super::Event;
use anyhow::{Context, Result, bail};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::{
	fs::{File, OpenOptions},
	io::{self, Read, Write},
	net::{Shutdown, SocketAddr, TcpListener, TcpStream},
	path::{Path, PathBuf},
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
	thread::{self, JoinHandle},
	time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(2);
const READ_POLL: Duration = Duration::from_millis(100);
const MAX_REQUEST: usize = 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
struct Endpoint {
	address: SocketAddr,
	token: String,
	#[serde(default)]
	folder: Option<std::ffi::OsString>,
}
#[derive(Serialize, Deserialize)]
struct Request {
	token: String,
	path: Option<std::ffi::OsString>,
}

pub(super) enum Start {
	Primary(Primary),
	Forwarded(Remote),
	Independent,
}

pub(super) struct Remote {
	lock_path: PathBuf,
}
impl Remote {
	pub(super) fn forward(&self, path: Option<&Path>) -> Result<()> {
		if let Some(endpoint) = find(&self.lock_path, path)? {
			forward(&endpoint, path)
		} else {
			spawn(path, false)
		}
	}
}

pub(super) struct Primary {
	_lock: File,
	listener: TcpListener,
	endpoint: Endpoint,
	endpoint_path: PathBuf,
	owns_session: bool,
}

fn private_file(path: &Path) -> io::Result<File> {
	let mut options = OpenOptions::new();
	options.read(true).write(true).create(true).truncate(false);
	markview_platform::private_file_options(&mut options);
	options.open(path)
}

fn registry_guard(lock_path: &Path) -> io::Result<File> {
	std::fs::create_dir_all(lock_path.parent().unwrap())?;
	let guard = private_file(&lock_path.with_extension("routing"))?;
	guard.lock()?;
	Ok(guard)
}

struct Windows {
	endpoints: Vec<Endpoint>,
	free: (File, PathBuf),
}

fn lock_paths(lock_path: &Path) -> io::Result<Vec<PathBuf>> {
	let directory = lock_path.with_extension("windows");
	std::fs::create_dir_all(&directory)?;
	let mut paths = vec![lock_path.to_owned()];
	for entry in std::fs::read_dir(&directory)? {
		let path = entry?.path();
		if path.extension().is_some_and(|ext| ext == "lock") {
			paths.push(path);
		}
	}
	paths[1..].sort();
	Ok(paths)
}

fn windows(lock_path: &Path) -> Result<Windows> {
	let mut endpoints = Vec::new();
	let mut free = None;
	for path in lock_paths(lock_path)? {
		let lock = private_file(&path)?;
		match lock.try_lock() {
			Ok(()) => {
				if free.is_none() {
					free = Some((lock, path));
				}
			}
			Err(std::fs::TryLockError::WouldBlock) => {
				endpoints.push(serde_json::from_slice(&std::fs::read(
					path.with_extension("json"),
				)?)?);
			}
			Err(std::fs::TryLockError::Error(error)) => {
				return Err(error.into());
			}
		}
	}
	let free = match free {
		Some(free) => free,
		None => {
			let path = lock_path
				.with_extension("windows")
				.join(format!("{}.lock", endpoints.len()));
			let lock = private_file(&path)?;
			lock.lock()?;
			(lock, path)
		}
	};
	Ok(Windows { endpoints, free })
}

fn normalized(path: &Path) -> PathBuf {
	let path = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
	// Missing files still inherit their existing ancestor's canonical prefix.
	for ancestor in path.ancestors() {
		if let Ok(root) = ancestor.canonicalize() {
			return root.join(path.strip_prefix(ancestor).unwrap());
		}
	}
	path
}

fn select(endpoints: &[Endpoint], path: Option<&Path>) -> Option<Endpoint> {
	let Some(path) = path else {
		return endpoints.first().cloned();
	};
	let path = normalized(path);
	let directory = path.is_dir();
	endpoints
		.iter()
		.filter_map(|endpoint| {
			let rank = match &endpoint.folder {
				Some(folder) => {
					let folder = Path::new(folder);
					if (directory && path == folder)
						|| (!directory && path.starts_with(folder))
					{
						folder.components().count() + 1
					} else {
						return None;
					}
				}
				None if !directory => 0,
				None => return None,
			};
			Some((rank, endpoint))
		})
		.max_by_key(|(rank, _)| *rank)
		.map(|(_, endpoint)| endpoint.clone())
}

fn find(lock_path: &Path, path: Option<&Path>) -> Result<Option<Endpoint>> {
	let _guard = registry_guard(lock_path)?;
	Ok(select(&windows(lock_path)?.endpoints, path))
}

pub(super) fn start(
	lock_path: &Path,
	enabled: bool,
	path: Option<PathBuf>,
	new_window: bool,
) -> Result<Start> {
	let deadline = Instant::now() + TIMEOUT;
	loop {
		let _guard = registry_guard(lock_path)?;
		let windows = windows(lock_path)?;
		if !enabled && !new_window && !windows.endpoints.is_empty() {
			return Ok(Start::Independent);
		}
		if enabled
			&& !new_window
			&& let Some(endpoint) = select(&windows.endpoints, path.as_deref())
		{
			if forward(&endpoint, path.as_deref()).is_ok() {
				return Ok(Start::Forwarded(Remote {
					lock_path: lock_path.to_owned(),
				}));
			}
			if Instant::now() >= deadline {
				bail!("Cannot contact the running Markview instance");
			}
			drop(windows);
			drop(_guard);
			thread::sleep(Duration::from_millis(25));
			continue;
		}
		let owns_session = windows.endpoints.is_empty();
		return Ok(Start::Primary(create_primary(
			windows.free,
			path.as_deref(),
			owns_session,
		)?));
	}
}

fn publish(endpoint: &Endpoint, path: &Path) -> Result<()> {
	let mut metadata = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
	serde_json::to_writer(&mut metadata, endpoint)?;
	metadata.flush()?;
	metadata.persist(path)?;
	Ok(())
}

fn create_primary(
	(lock, lock_path): (File, PathBuf),
	path: Option<&Path>,
	owns_session: bool,
) -> Result<Primary> {
	let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
	let mut secret = [0; 32];
	getrandom::fill(&mut secret)?;
	let endpoint = Endpoint {
		address: listener.local_addr()?,
		token: base64::engine::general_purpose::STANDARD.encode(secret),
		folder: path
			.filter(|path| path.is_dir())
			.map(|path| normalized(path).into_os_string()),
	};
	let endpoint_path = lock_path.with_extension("json");
	publish(&endpoint, &endpoint_path)?;
	Ok(Primary {
		_lock: lock,
		listener,
		endpoint,
		endpoint_path,
		owns_session,
	})
}

pub(super) fn spawn(path: Option<&Path>, new_window: bool) -> Result<()> {
	let mut child = std::process::Command::new(std::env::current_exe()?);
	child.arg("--background-child");
	if new_window {
		child.arg("--new-window");
	}
	if let Some(path) = path {
		child.arg("--").arg(path);
	}
	markview_platform::detach(&mut child);
	child.spawn().context("Cannot start a reader window")?;
	Ok(())
}

impl<P: super::SendEvent> super::App<P> {
	pub(super) fn register_instance(&mut self) {
		if markview_platform::touch_layout()
			|| self.instance.is_some()
			|| !self.preferences.values.single_instance
		{
			return;
		}
		let Some(lock) = &self.instance_path else {
			return;
		};
		let register = || -> Result<Primary> {
			let _guard = registry_guard(lock)?;
			let windows = windows(lock)?;
			create_primary(
				windows.free,
				self.folder.root().map(|p| p.as_path()),
				windows.endpoints.is_empty(),
			)
		};
		match register() {
			Ok(primary) => {
				let proxy = self.proxy.clone();
				self.instance =
					Some(primary.listen(move |event| proxy.try_send(event)));
			}
			Err(error) => {
				log::warn!("Instance discovery unavailable: {error:#}")
			}
		}
	}

	pub(super) fn route_open(&mut self, path: PathBuf) {
		let route = || -> Result<bool> {
			if markview_platform::touch_layout()
				|| !self.preferences.values.single_instance
			{
				return Ok(false);
			}
			let Some(lock) = &self.instance_path else {
				return Ok(false);
			};
			if let Some(endpoint) = find(lock, Some(&path))? {
				if self.instance.as_ref().is_some_and(|instance| {
					instance.address == endpoint.address
				}) {
					return Ok(false);
				}
				forward(&endpoint, Some(&path))?;
			} else {
				spawn(Some(&path), false)?;
			}
			Ok(true)
		};
		match route() {
			Ok(false) => self.open(path),
			Ok(true) => {}
			Err(error) => self.notify(&error.to_string(), true, 4),
		}
	}

	pub(super) fn sync_instance_folder(&mut self, root: Option<&Path>) {
		if let Some(instance) = &mut self.instance {
			instance.endpoint.folder =
				root.map(|root| normalized(root).into_os_string());
			if let Err(error) =
				publish(&instance.endpoint, &instance.endpoint_path)
			{
				log::warn!("Cannot update window folder: {error:#}");
			}
		}
	}
}

fn forward(endpoint: &Endpoint, path: Option<&Path>) -> Result<()> {
	// Only loopback addresses from the discovery file are usable.
	if !endpoint.address.ip().is_loopback() {
		bail!("Invalid instance address");
	}
	let mut stream = TcpStream::connect_timeout(&endpoint.address, TIMEOUT)?;
	stream.set_read_timeout(Some(TIMEOUT))?;
	stream.set_write_timeout(Some(TIMEOUT))?;
	let path = path
		.map(std::path::absolute)
		.transpose()?
		.map(PathBuf::into_os_string);
	serde_json::to_writer(
		&mut stream,
		&Request {
			token: endpoint.token.clone(),
			path,
		},
	)?;
	stream.shutdown(Shutdown::Write)?;
	let mut ack = [0];
	stream
		.read_exact(&mut ack)
		.context("Instance did not accept the file")?;
	if ack != [1] {
		bail!("Instance rejected the file");
	}
	Ok(())
}

fn read_request(
	stream: &mut TcpStream,
	stop: &AtomicBool,
	deadline: Instant,
) -> io::Result<Vec<u8>> {
	let mut bytes = Vec::new();
	let mut buffer = [0; 8192];
	loop {
		if stop.load(Ordering::Acquire) {
			return Err(io::ErrorKind::ConnectionAborted.into());
		}
		let remaining = deadline.saturating_duration_since(Instant::now());
		if remaining.is_zero() {
			return Err(io::ErrorKind::TimedOut.into());
		}
		stream.set_read_timeout(Some(remaining.min(READ_POLL)))?;
		match stream.read(&mut buffer) {
			Ok(0) => return Ok(bytes),
			Ok(size) => {
				if bytes.len() + size > MAX_REQUEST {
					return Err(io::ErrorKind::InvalidData.into());
				}
				bytes.extend_from_slice(&buffer[..size]);
			}
			Err(error)
				if matches!(
					error.kind(),
					io::ErrorKind::WouldBlock
						| io::ErrorKind::TimedOut
						| io::ErrorKind::Interrupted
				) => {}
			Err(error) => return Err(error),
		}
	}
}

impl Primary {
	pub(super) fn listen(
		self,
		deliver: impl Fn(Event) -> bool + Send + 'static,
	) -> Listener {
		let stop = Arc::new(AtomicBool::new(false));
		let stopping = stop.clone();
		let address = self.endpoint.address;
		let endpoint = self.endpoint.clone();
		let endpoint_path = self.endpoint_path.clone();
		let owns_session = self.owns_session;
		let thread = thread::spawn(move || {
			for stream in self.listener.incoming() {
				if stopping.load(Ordering::Acquire) {
					break;
				}
				let Ok(mut stream) = stream else {
					break;
				};
				let deadline = Instant::now() + TIMEOUT;
				if let Ok(bytes) =
					read_request(&mut stream, &stopping, deadline)
					&& let Ok(request) =
						serde_json::from_slice::<Request>(&bytes)
					&& request.token == self.endpoint.token
					&& !stopping.load(Ordering::Acquire)
					&& Instant::now() < deadline
				{
					let accepted = deliver(Event::Activate(
						request.path.map(PathBuf::from),
					));
					let remaining =
						deadline.saturating_duration_since(Instant::now());
					if !remaining.is_zero()
						&& stream.set_write_timeout(Some(remaining)).is_ok()
					{
						let _ = stream.write_all(&[u8::from(accepted)]);
					}
					if !accepted {
						break;
					}
				}
			}
			// Keep the ownership lock until the listener has stopped.
			drop(self._lock);
		});
		Listener {
			endpoint,
			endpoint_path,
			owns_session,
			stop,
			address,
			thread: Some(thread),
		}
	}
}

pub(super) struct Listener {
	endpoint: Endpoint,
	endpoint_path: PathBuf,
	pub(super) owns_session: bool,
	stop: Arc<AtomicBool>,
	address: SocketAddr,
	thread: Option<JoinHandle<()>>,
}
impl Drop for Listener {
	fn drop(&mut self) {
		self.stop.store(true, Ordering::Release);
		let _ = TcpStream::connect_timeout(&self.address, TIMEOUT);
		if let Some(thread) = self.thread.take() {
			let _ = thread.join();
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::mpsc;

	fn proxy(
		tx: mpsc::Sender<Option<PathBuf>>,
	) -> impl Fn(Event) -> bool + Send + 'static {
		move |event| {
			let Event::Activate(path) = event else {
				panic!()
			};
			tx.send(path).is_ok()
		}
	}

	fn window(
		lock: &Path,
		path: &Path,
	) -> (Listener, mpsc::Receiver<Option<PathBuf>>) {
		let Start::Primary(primary) =
			start(lock, true, Some(path.to_owned()), false).unwrap()
		else {
			panic!("expected a new window for {}", path.display())
		};
		let (tx, rx) = mpsc::channel();
		(primary.listen(proxy(tx)), rx)
	}

	#[test]
	fn explicit_new_window_bypasses_reuse_and_keeps_session_ownership() {
		let dir = tempfile::tempdir().unwrap();
		let lock = dir.path().join("instance.lock");
		let Start::Primary(first) = start(&lock, true, None, false).unwrap()
		else {
			panic!("expected the first window");
		};
		let (tx, rx) = mpsc::channel();
		let first = first.listen(proxy(tx));
		for enabled in [true, false] {
			let Start::Primary(second) =
				start(&lock, enabled, None, true).unwrap()
			else {
				panic!("expected a new window");
			};
			assert!(first.owns_session && !second.owns_session);
			assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
			let (tx, second_rx) = mpsc::channel();
			let _second = second.listen(proxy(tx));
			let path = dir.path().join("note.md");
			assert!(matches!(
				start(&lock, true, Some(path.clone()), false).unwrap(),
				Start::Forwarded(_)
			));
			assert_eq!(second_rx.recv_timeout(TIMEOUT).unwrap(), Some(path));
		}
	}

	#[test]
	fn folders_get_separate_windows_and_files_follow_their_folder() {
		let dir = tempfile::tempdir().unwrap();
		let lock = dir.path().join("instance.lock");
		let root = dir.path().join("notes");
		let nested = root.join("nested");
		let sibling = dir.path().join("notes-other");
		std::fs::create_dir_all(&nested).unwrap();
		std::fs::create_dir(&sibling).unwrap();
		let (first, first_rx) = window(&lock, &root);
		let (second, second_rx) = window(&lock, &sibling);
		let (third, third_rx) = window(&lock, &nested);
		assert!(first.owns_session);
		assert!(!second.owns_session && !third.owns_session);
		let Start::Forwarded(remote) =
			start(&lock, true, Some(root.join(".")), false).unwrap()
		else {
			panic!()
		};
		assert_eq!(
			first_rx.recv_timeout(TIMEOUT).unwrap(),
			Some(root.join("."))
		);
		for (path, rx) in [
			(root.join("first.md"), &first_rx),
			(sibling.join("second.md"), &second_rx),
			(nested.join("third.md"), &third_rx),
		] {
			remote.forward(Some(&path)).unwrap();
			assert_eq!(rx.recv_timeout(TIMEOUT).unwrap(), Some(path));
		}
		let outside = dir.path().join("outside.md");
		let (loose, loose_rx) = window(&lock, &outside);
		let new_folder = dir.path().join("new-folder");
		std::fs::create_dir(&new_folder).unwrap();
		let (_fourth, _) = window(&lock, &new_folder);
		for (path, rx) in [
			(dir.path().join("another.md"), &loose_rx),
			(root.join("inside.md"), &first_rx),
		] {
			assert!(matches!(
				start(&lock, true, Some(path.clone()), false).unwrap(),
				Start::Forwarded(_)
			));
			assert_eq!(rx.recv_timeout(TIMEOUT).unwrap(), Some(path));
		}
		assert!(loose_rx.try_recv().is_err());
		drop(loose);
		drop(first);
		let path = nested.join("still-open.md");
		assert!(matches!(
			start(&lock, true, Some(path.clone()), false).unwrap(),
			Start::Forwarded(_)
		));
		assert_eq!(third_rx.recv_timeout(TIMEOUT).unwrap(), Some(path));
		let (_replacement, _) = window(&lock, &outside);
	}

	#[test]
	fn folder_aliases_reuse_the_window() {
		if !markview_platform::test_support::special_files() {
			return;
		}
		let dir = tempfile::tempdir().unwrap();
		let lock = dir.path().join("instance.lock");
		let root = dir.path().join("notes");
		let alias = dir.path().join("alias");
		std::fs::create_dir(&root).unwrap();
		std::fs::write(root.join("note.md"), "# Note").unwrap();
		markview_platform::test_support::symlink(&root, &alias).unwrap();
		let (_listener, rx) = window(&lock, &root);
		for path in [
			alias.clone(),
			alias.join("note.md"),
			alias.join("missing.md"),
			alias.join("missing/note.md"),
		] {
			assert!(matches!(
				start(&lock, true, Some(path.clone()), false).unwrap(),
				Start::Forwarded(_)
			));
			assert_eq!(rx.recv_timeout(TIMEOUT).unwrap(), Some(path));
		}
	}

	#[test]
	fn launches_forward_only_when_enabled_and_recover_after_exit() {
		let dir = tempfile::tempdir().unwrap();
		let lock = dir.path().join("instance.lock");
		let Start::Primary(primary) = start(&lock, false, None, false).unwrap()
		else {
			panic!()
		};
		let (tx, rx) = mpsc::channel();
		let listener = primary.listen(proxy(tx));
		assert!(matches!(
			start(&lock, false, None, false).unwrap(),
			Start::Independent
		));
		let path = PathBuf::from("tests/fixtures/ordinary-10k.md");
		assert!(matches!(
			start(&lock, true, Some(path.clone()), false).unwrap(),
			Start::Forwarded(_)
		));
		assert_eq!(
			rx.recv_timeout(TIMEOUT).unwrap(),
			Some(std::path::absolute(path).unwrap())
		);
		assert!(matches!(
			start(&lock, true, None, false).unwrap(),
			Start::Forwarded(_)
		));
		assert_eq!(rx.recv_timeout(TIMEOUT).unwrap(), None);
		drop(listener);
		assert!(matches!(
			start(&lock, true, None, false).unwrap(),
			Start::Primary(_)
		));
	}

	#[test]
	fn invalid_token_cannot_open_a_file() {
		let dir = tempfile::tempdir().unwrap();
		let lock = dir.path().join("instance.lock");
		let Start::Primary(primary) = start(&lock, true, None, false).unwrap()
		else {
			panic!()
		};
		let (tx, rx) = mpsc::channel();
		let mut endpoint: Endpoint = serde_json::from_slice(
			&std::fs::read(lock.with_extension("json")).unwrap(),
		)
		.unwrap();
		let _listener = primary.listen(proxy(tx));
		endpoint.token = "wrong".into();
		assert!(forward(&endpoint, Some(Path::new("file.md"))).is_err());
		assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
	}

	#[test]
	fn endpoint_keeps_its_token_out_of_directory_notifications() {
		if !markview_platform::test_support::special_files() {
			return;
		}
		use markview_platform::test_support::symlink;

		let tmp = tempfile::tempdir().unwrap();
		let directory = tmp.path().join("settings");
		std::fs::create_dir(&directory).unwrap();
		let directory = directory.canonicalize().unwrap();
		let alias = tmp.path().join("alias");
		symlink(&directory, &alias).unwrap();
		markview_platform::test_support::set_mode(&directory, 0o755).unwrap();
		let (tx, rx) = mpsc::channel();
		let mut watcher =
			markview_platform::watch::Watcher::new(move |event| {
				tx.send(event).unwrap();
			})
			.unwrap();
		assert!(watcher.watch_directory(&directory));
		let lock = alias.join("instance.lock");
		let Start::Primary(primary) = start(&lock, true, None, false).unwrap()
		else {
			panic!()
		};
		let endpoint_path = lock.with_extension("json").canonicalize().unwrap();
		assert_eq!(
			markview_platform::test_support::permission_mode(
				std::fs::metadata(&endpoint_path).unwrap().permissions()
			),
			0o600
		);
		loop {
			let event = rx.recv_timeout(TIMEOUT).unwrap().unwrap();
			for path in &event.paths {
				assert!(
					!path.to_string_lossy().contains(&primary.endpoint.token)
				);
			}
			if event.paths.contains(&endpoint_path) {
				break;
			}
		}
		assert_eq!(
			base64::engine::general_purpose::STANDARD
				.decode(&primary.endpoint.token)
				.unwrap()
				.len(),
			32
		);
	}

	#[test]
	fn trickling_client_cannot_extend_the_request_deadline() {
		let dir = tempfile::tempdir().unwrap();
		let lock = dir.path().join("instance.lock");
		let Start::Primary(primary) = start(&lock, true, None, false).unwrap()
		else {
			panic!()
		};
		let endpoint = primary.endpoint.address;
		let (tx, rx) = mpsc::channel();
		let _listener = primary.listen(proxy(tx));
		let mut client = TcpStream::connect(endpoint).unwrap();
		client.set_write_timeout(Some(READ_POLL)).unwrap();
		let started = Instant::now();
		let trickle = thread::spawn(move || {
			while started.elapsed() < TIMEOUT * 2 {
				if client.write_all(b" ").is_err() {
					break;
				}
				thread::sleep(READ_POLL);
			}
		});
		thread::sleep(READ_POLL * 2);
		assert!(matches!(
			start(&lock, true, None, false).unwrap(),
			Start::Forwarded(_)
		));
		assert_eq!(rx.recv_timeout(TIMEOUT).unwrap(), None);
		assert!(started.elapsed() < TIMEOUT + Duration::from_secs(1));
		trickle.join().unwrap();
	}

	#[test]
	fn shutdown_cancels_a_client_without_waiting_for_the_deadline() {
		let dir = tempfile::tempdir().unwrap();
		let lock = dir.path().join("instance.lock");
		let Start::Primary(primary) = start(&lock, false, None, false).unwrap()
		else {
			panic!()
		};
		let endpoint = primary.endpoint.address;
		let (tx, rx) = mpsc::channel();
		let listener = primary.listen(proxy(tx));
		let mut client = TcpStream::connect(endpoint).unwrap();
		client.write_all(b" ").unwrap();
		thread::sleep(READ_POLL * 2);
		let started = Instant::now();
		drop(listener);
		assert!(started.elapsed() < Duration::from_secs(1));
		assert!(rx.try_recv().is_err());
		assert!(matches!(
			start(&lock, true, None, false).unwrap(),
			Start::Primary(_)
		));
	}

	#[test]
	fn closed_event_loop_rejects_forwarding_and_releases_ownership() {
		let dir = tempfile::tempdir().unwrap();
		let lock = dir.path().join("instance.lock");
		let Start::Primary(primary) = start(&lock, true, None, false).unwrap()
		else {
			panic!()
		};
		let endpoint: Endpoint = serde_json::from_slice(
			&std::fs::read(lock.with_extension("json")).unwrap(),
		)
		.unwrap();
		let (tx, rx) = mpsc::channel();
		let mut listener = primary.listen(proxy(tx));
		drop(rx);
		assert!(forward(&endpoint, Some(Path::new("file.md"))).is_err());
		listener.thread.take().unwrap().join().unwrap();
		assert!(matches!(
			start(&lock, true, None, false).unwrap(),
			Start::Primary(_)
		));
	}
}
