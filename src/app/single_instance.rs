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

#[derive(Serialize, Deserialize)]
struct Endpoint {
	address: SocketAddr,
	token: String,
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

pub(super) struct Remote(Endpoint);
impl Remote {
	pub(super) fn forward(&self, path: Option<&Path>) -> Result<()> {
		forward(&self.0, path)
	}
}

pub(super) struct Primary {
	_lock: File,
	listener: TcpListener,
	endpoint: Endpoint,
}

pub(super) fn start(
	lock_path: &Path,
	enabled: bool,
	path: Option<PathBuf>,
) -> Result<Start> {
	let directory = lock_path.parent().unwrap();
	std::fs::create_dir_all(directory)?;
	let mut options = OpenOptions::new();
	options.read(true).write(true).create(true).truncate(false);
	#[cfg(unix)]
	{
		use std::os::unix::fs::OpenOptionsExt;
		options.mode(0o600);
	}
	let lock = options.open(lock_path)?;
	let endpoint_path = lock_path.with_extension("json");
	let deadline = Instant::now() + TIMEOUT;
	loop {
		match lock.try_lock() {
			Ok(()) => {
				let listener =
					TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
				let mut secret = [0; 32];
				getrandom::fill(&mut secret)?;
				let endpoint = Endpoint {
					address: listener.local_addr()?,
					token: base64::engine::general_purpose::STANDARD
						.encode(secret),
				};
				let mut metadata = tempfile::NamedTempFile::new_in(directory)?;
				serde_json::to_writer(&mut metadata, &endpoint)?;
				metadata.flush()?;
				metadata.persist(&endpoint_path)?;
				return Ok(Start::Primary(Primary {
					_lock: lock,
					listener,
					endpoint,
				}));
			}
			Err(std::fs::TryLockError::WouldBlock) if !enabled => {
				return Ok(Start::Independent);
			}
			Err(std::fs::TryLockError::WouldBlock) => {
				// The owner may still be publishing its endpoint or starting its loop.
				if let Ok(remote) =
					std::fs::read(&endpoint_path).and_then(|bytes| {
						serde_json::from_slice::<Endpoint>(&bytes)
							.map(Remote)
							.map_err(std::io::Error::other)
					}) && remote.forward(path.as_deref()).is_ok()
				{
					return Ok(Start::Forwarded(remote));
				}
				if Instant::now() >= deadline {
					bail!("Cannot contact the running Markview instance");
				}
				thread::sleep(Duration::from_millis(25));
			}
			Err(std::fs::TryLockError::Error(error)) => {
				return Err(error.into());
			}
		}
	}
}

impl<P: super::SendEvent> super::App<P> {
	pub(super) fn register_instance(&mut self) {
		if self.instance.is_some() || !self.preferences.values.single_instance {
			return;
		}
		let Some(lock) = &self.instance_path else {
			return;
		};
		// An existing window only claims a free lock; it never forwards itself.
		match start(lock, false, None) {
			Ok(Start::Primary(primary)) => {
				let proxy = self.proxy.clone();
				self.instance =
					Some(primary.listen(move |event| proxy.try_send(event)));
			}
			Ok(Start::Independent) => {}
			Ok(Start::Forwarded(_)) => unreachable!(),
			Err(error) => {
				log::warn!("Instance discovery unavailable: {error:#}")
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
			stop,
			address,
			thread: Some(thread),
		}
	}
}

pub(super) struct Listener {
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

	#[test]
	fn launches_forward_only_when_enabled_and_recover_after_exit() {
		let dir = tempfile::tempdir().unwrap();
		let lock = dir.path().join("instance.lock");
		let Start::Primary(primary) = start(&lock, false, None).unwrap() else {
			panic!()
		};
		let (tx, rx) = mpsc::channel();
		let listener = primary.listen(proxy(tx));
		assert!(matches!(
			start(&lock, false, None).unwrap(),
			Start::Independent
		));
		let path = PathBuf::from("tests/fixtures/ordinary-10k.md");
		assert!(matches!(
			start(&lock, true, Some(path.clone())).unwrap(),
			Start::Forwarded(_)
		));
		assert_eq!(
			rx.recv_timeout(TIMEOUT).unwrap(),
			Some(std::path::absolute(path).unwrap())
		);
		assert!(matches!(
			start(&lock, true, None).unwrap(),
			Start::Forwarded(_)
		));
		assert_eq!(rx.recv_timeout(TIMEOUT).unwrap(), None);
		drop(listener);
		assert!(matches!(
			start(&lock, true, None).unwrap(),
			Start::Primary(_)
		));
	}

	#[test]
	fn invalid_token_cannot_open_a_file() {
		let dir = tempfile::tempdir().unwrap();
		let lock = dir.path().join("instance.lock");
		let Start::Primary(primary) = start(&lock, true, None).unwrap() else {
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

	#[cfg(unix)]
	#[test]
	fn endpoint_keeps_its_token_out_of_directory_notifications() {
		use notify::Watcher;
		use std::os::unix::fs::PermissionsExt;

		let dir = tempfile::tempdir().unwrap();
		std::fs::set_permissions(
			dir.path(),
			std::fs::Permissions::from_mode(0o755),
		)
		.unwrap();
		let (tx, rx) = mpsc::channel();
		let mut watcher = notify::recommended_watcher(move |event| {
			tx.send(event).unwrap();
		})
		.unwrap();
		watcher
			.watch(dir.path(), notify::RecursiveMode::NonRecursive)
			.unwrap();
		let lock = dir.path().join("instance.lock");
		let Start::Primary(primary) = start(&lock, true, None).unwrap() else {
			panic!()
		};
		let endpoint_path = lock.with_extension("json");
		assert_eq!(
			std::fs::metadata(&endpoint_path)
				.unwrap()
				.permissions()
				.mode() & 0o777,
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
		let Start::Primary(primary) = start(&lock, true, None).unwrap() else {
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
			start(&lock, true, None).unwrap(),
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
		let Start::Primary(primary) = start(&lock, false, None).unwrap() else {
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
			start(&lock, true, None).unwrap(),
			Start::Primary(_)
		));
	}

	#[test]
	fn closed_event_loop_rejects_forwarding_and_releases_ownership() {
		let dir = tempfile::tempdir().unwrap();
		let lock = dir.path().join("instance.lock");
		let Start::Primary(primary) = start(&lock, true, None).unwrap() else {
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
			start(&lock, true, None).unwrap(),
			Start::Primary(_)
		));
	}
}
