use std::{
	fs, io,
	path::Path,
	thread,
	time::{Duration, Instant},
};

/// Move a fixture directory, allowing temporary Windows file handles to close.
pub fn rename_directory(from: &Path, to: &Path) -> io::Result<()> {
	retry_rename(|| fs::rename(from, to), Duration::from_secs(10))
}

fn retry_rename(
	mut rename: impl FnMut() -> io::Result<()>,
	timeout: Duration,
) -> io::Result<()> {
	let deadline = Instant::now() + timeout;
	loop {
		match rename() {
			Err(error)
				if cfg!(windows)
					&& matches!(error.raw_os_error(), Some(5 | 32))
					&& Instant::now() < deadline =>
			{
				// Scanners can keep files open after the child process exits.
				thread::sleep(Duration::from_millis(10));
			}
			result => return result,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[cfg(windows)]
	#[test]
	fn directory_rename_recovers_after_an_open_file_is_released() {
		use std::{io::Write, os::windows::fs::OpenOptionsExt};

		let root = markview_test_support::TestDir::new();
		let from = root.path().join("portable 中文 reader");
		let to = root.path().join("moved 中文 reader");
		fs::create_dir(&from).unwrap();
		let mut file = fs::OpenOptions::new()
			.write(true)
			.create_new(true)
			.share_mode(0)
			.open(from.join("marker"))
			.unwrap();
		file.write_all(b"preserved").unwrap();
		let mut held = Some(file);
		retry_rename(
			|| {
				let result = fs::rename(&from, &to);
				if held.is_some() {
					assert!(
						result.is_err(),
						"the open file must block renaming"
					);
					drop(held.take());
				}
				result
			},
			Duration::from_secs(10),
		)
		.unwrap();
		assert!(!from.exists());
		assert_eq!(fs::read_to_string(to.join("marker")).unwrap(), "preserved");
	}

	#[test]
	fn rename_returns_unrelated_errors_without_retrying() {
		let mut attempts = 0;
		let error = retry_rename(
			|| {
				attempts += 1;
				Err(io::ErrorKind::NotFound.into())
			},
			Duration::from_secs(10),
		)
		.unwrap_err();
		assert_eq!(error.kind(), io::ErrorKind::NotFound);
		assert_eq!(attempts, 1);
	}

	#[test]
	fn rename_returns_access_errors_at_the_deadline() {
		for code in [5, 32] {
			let mut attempts = 0;
			let error = retry_rename(
				|| {
					attempts += 1;
					Err(io::Error::from_raw_os_error(code))
				},
				Duration::ZERO,
			)
			.unwrap_err();
			assert_eq!(error.raw_os_error(), Some(code));
			assert_eq!(attempts, 1);
		}
	}
}
