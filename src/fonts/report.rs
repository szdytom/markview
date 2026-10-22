//! What a run reports: the progress a reader draws, the events one family
//! sends, and the summary a run settles on.
use anyhow::{Result, bail};
use futures_util::{StreamExt, stream::FuturesUnordered};
use markview_layout::style::FontFamily;
use std::{
	collections::{HashMap, HashSet},
	path::Path,
	time::Instant,
};

use super::{
	PROGRESS_INTERVAL, Transport,
	download::{Download, FamilyJob, Outcome},
};

/// What the reader draws about one family's download.
#[derive(Clone, Debug)]
pub struct Progress {
	pub id: String,
	pub phase: Phase,
	/// Files finished so far, and how many the source holds.
	pub files_done: usize,
	pub files_total: usize,
	/// Bytes written so far across all transfers.
	pub bytes_done: u64,
	/// Sum of per-transfer completion fractions, including active downloads.
	pub files_progress: f64,
	pub current: Option<String>,
	/// A failure reason, or the name of the source being tried.
	pub note: Option<String>,
}
impl Progress {
	pub fn queued(id: &str) -> Self {
		Self {
			id: id.to_owned(),
			phase: Phase::Queued,
			files_done: 0,
			files_total: 0,
			bytes_done: 0,
			files_progress: 0.0,
			current: None,
			note: None,
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
	Queued,
	Downloading,
	Extracting,
	Done,
	Failed,
	Cancelled,
}

/// What a run did, for a command's exit status and its summary line.
#[derive(Clone, Debug, Default)]
pub struct Summary {
	/// The families this run set out to download. A reader retires exactly
	/// these from its progress table, so two runs cannot clear each other.
	pub requested: Vec<String>,
	/// Families that ended up on disk.
	pub stored: usize,
	pub bytes: u64,
	pub failed: Vec<(String, String)>,
	pub cancelled: Vec<String>,
}

#[derive(Clone)]
pub(super) struct Reporter {
	pub(super) id: String,
	pub(super) run: usize,
	pub(super) attempt: usize,
	pub(super) tx: tokio::sync::mpsc::Sender<Event>,
}
pub(super) struct Event {
	id: String,
	run: usize,
	attempt: usize,
	change: Change,
}
pub(super) enum Change {
	Attempt {
		total: usize,
		note: Option<String>,
	},
	File {
		file: usize,
		bytes: u64,
		total: Option<u64>,
		done: bool,
		phase: Phase,
		name: String,
	},
	Terminal {
		phase: Phase,
		note: Option<String>,
	},
}
impl Reporter {
	fn event(&self, change: Change) -> Event {
		Event {
			id: self.id.clone(),
			run: self.run,
			attempt: self.attempt,
			change,
		}
	}
	pub(super) fn send(
		&self,
		change: Change,
	) -> impl std::future::Future<Output = ()> + Send + use<> {
		let tx = self.tx.clone();
		let event = self.event(change);
		async move {
			let _ = tx.send(event).await;
		}
	}
	pub(super) async fn fetch<T: Transport>(
		&self,
		file: usize,
		name: &str,
		url: &str,
		path: &Path,
		cap: u64,
		download: &Download<'_, T>,
	) -> Result<u64> {
		let Download {
			transport,
			services,
			cancel,
		} = *download;
		if cancel.is_cancelled() {
			bail!("Cancelled");
		}
		let _permit = services.permit(&cancel.token).await?;
		let mut last_report: Option<Instant> = None;
		transport
			.fetch(
				url,
				path,
				cap,
				&mut |bytes, total| {
					if last_report
						.is_some_and(|at| at.elapsed() < PROGRESS_INTERVAL)
						&& total != Some(bytes)
					{
						return;
					}
					let _ = self.tx.try_send(self.event(Change::File {
						file,
						bytes,
						total,
						done: false,
						phase: Phase::Downloading,
						name: name.to_owned(),
					}));
					last_report = Some(Instant::now());
				},
				&cancel.token,
			)
			.await?;
		drop(_permit);
		let bytes = tokio::fs::metadata(path).await?.len();
		self.send(Change::File {
			file,
			bytes,
			total: Some(bytes),
			done: false,
			phase: Phase::Downloading,
			name: name.to_owned(),
		})
		.await;
		Ok(bytes)
	}
}
#[derive(Default)]
struct FileProgress {
	bytes: u64,
	fraction: f64,
	done: bool,
}
pub(super) struct FamilyProgress {
	value: Progress,
	attempt: usize,
	files: HashMap<usize, FileProgress>,
}

pub(super) fn initial_states(
	families: &[FontFamily],
	report: &mut (impl FnMut(Progress) + Send),
) -> HashMap<String, FamilyProgress> {
	families
		.iter()
		.map(|f| {
			let value = Progress::queued(&f.id);
			report(value.clone());
			(
				f.id.clone(),
				FamilyProgress {
					value,
					attempt: 0,
					files: HashMap::new(),
				},
			)
		})
		.collect()
}

/// Merges file events, finished families and the redraw tick until the run ends.
pub(super) async fn merge_progress(
	summary: &mut Summary,
	running: &mut FuturesUnordered<FamilyJob<'_>>,
	rx: &mut tokio::sync::mpsc::Receiver<Event>,
	states: &mut HashMap<String, FamilyProgress>,
	run: usize,
	report: &mut (impl FnMut(Progress) + Send),
) {
	let mut dirty = HashSet::new();
	let mut tick = tokio::time::interval(PROGRESS_INTERVAL);
	loop {
		tokio::select! {
			event = rx.recv() => {
				let Some(event) = event else { if running.is_empty() { break; } else { continue; } };
				if event.run != run { continue; }
				let Some(state) = states.get_mut(&event.id) else { continue; };
				match event.change {
					Change::Attempt { total, note } => {
						if event.attempt < state.attempt { continue; }
						state.attempt = event.attempt; state.files.clear();
						state.value = Progress { phase: Phase::Downloading, files_total: total, note, ..Progress::queued(&event.id) };
						dirty.insert(event.id);
					}
					Change::File { file, bytes, total, done, phase, name } => {
						if event.attempt != state.attempt { continue; }
						state.files.insert(file, FileProgress { bytes, fraction: if done { 1.0 } else { total.filter(|n| *n > 0).map_or(0.0, |n| (bytes as f64 / n as f64).min(1.0)) }, done });
						state.value.bytes_done = state.files.values().map(|f| f.bytes).sum();
						state.value.files_progress = state.files.values().map(|f| f.fraction).sum();
						state.value.files_done = state.files.values().filter(|f| f.done).count();
						state.value.phase = phase; state.value.current = Some(name);
						// Finished files are useful immediately, even beside a stalled transfer.
						if done { report(state.value.clone()); dirty.remove(&event.id); } else { dirty.insert(event.id); }
					}
					Change::Terminal { phase, note } => {
						state.value.phase = phase; state.value.note = note; state.value.current = None;
						report(state.value.clone()); dirty.remove(&event.id);
					}
				}
			}
			Some((id, outcome)) = running.next(), if !running.is_empty() => {
				match outcome {
					Outcome::Stored { bytes } => { summary.stored += 1; summary.bytes += bytes; }
					Outcome::Failed(reason) => summary.failed.push((id, reason)),
					Outcome::Cancelled => summary.cancelled.push(id),
				}
			}
			_ = tick.tick() => { for id in dirty.drain() { report(states[&id].value.clone()); } }
		}
	}
}

#[cfg(test)]
mod tests {
	use super::super::{
		run, run_async,
		test_support::{Fake, family, fan, font_bytes},
	};
	use super::*;
	use std::{
		fs,
		sync::{Arc, atomic::Ordering},
		time::Duration,
	};
	use tokio_util::sync::CancellationToken;

	#[tokio::test]
	async fn simultaneous_runs_share_the_transfer_budget() {
		use std::sync::atomic::{AtomicUsize, Ordering};
		struct Controlled {
			active: AtomicUsize,
			peak: AtomicUsize,
			entered: tokio::sync::mpsc::UnboundedSender<()>,
			release: tokio::sync::Semaphore,
			bytes: Vec<u8>,
		}
		impl Transport for Controlled {
			async fn fetch(
				&self,
				_: &str,
				path: &Path,
				_: u64,
				progress: &mut (dyn FnMut(u64, Option<u64>) + Send),
				cancel: &CancellationToken,
			) -> Result<()> {
				let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
				self.peak.fetch_max(active, Ordering::SeqCst);
				self.entered.send(()).unwrap();
				let permit = tokio::select! { permit = self.release.acquire() => permit?, _ = cancel.cancelled() => bail!("Cancelled") };
				permit.forget();
				tokio::fs::write(path, &self.bytes).await?;
				progress(
					self.bytes.len() as u64,
					Some(self.bytes.len() as u64),
				);
				self.active.fetch_sub(1, Ordering::SeqCst);
				Ok(())
			}
			async fn probe(&self, _: &str) -> Result<Duration> {
				Ok(Duration::ZERO)
			}
		}
		let services = crate::services::Services::new(2);
		let (entered, mut events) = tokio::sync::mpsc::unbounded_channel();
		let transport = Controlled {
			active: AtomicUsize::new(0),
			peak: AtomicUsize::new(0),
			entered,
			release: tokio::sync::Semaphore::new(0),
			bytes: font_bytes(),
		};
		let a = tempfile::tempdir().unwrap();
		let b = tempfile::tempdir().unwrap();
		let first = [family(
			"first",
			&["https://example.com/a.otf", "https://example.com/b.otf"],
		)];
		let second = [family(
			"second",
			&["https://example.com/c.otf", "https://example.com/d.otf"],
		)];
		let release = async {
			events.recv().await.unwrap();
			events.recv().await.unwrap();
			assert_eq!(transport.active.load(Ordering::SeqCst), 2);
			transport.release.add_permits(4);
		};
		let (first, second, ()) = tokio::join!(
			run_async(
				&first,
				a.path(),
				&transport,
				&services.handle,
				HashMap::new(),
				|_| {}
			),
			run_async(
				&second,
				b.path(),
				&transport,
				&services.handle,
				HashMap::new(),
				|_| {}
			),
			release
		);
		assert!(first.failed.is_empty(), "{:?}", first.failed);
		assert!(second.failed.is_empty(), "{:?}", second.failed);
		assert_eq!(first.stored + second.stored, 2);
		let count = |dir: &Path| {
			fs::read_dir(dir)
				.unwrap()
				.filter_map(Result::ok)
				.filter(|entry| {
					entry.path().extension().is_some_and(|ext| ext == "otf")
				})
				.count()
		};
		assert_eq!(count(a.path()) + count(b.path()), 4);
		assert_eq!(transport.peak.load(Ordering::SeqCst), 2);
		assert_eq!(transport.active.load(Ordering::SeqCst), 0);
	}

	#[test]
	fn completed_files_report_before_a_slower_first_file_finishes() {
		struct Delayed {
			fake: Fake,
			release: tokio::sync::Notify,
		}
		impl Transport for Delayed {
			async fn fetch(
				&self,
				url: &str,
				path: &Path,
				cap: u64,
				progress: &mut (dyn FnMut(u64, Option<u64>) + Send),
				cancel: &CancellationToken,
			) -> Result<()> {
				if url.ends_with("slow.otf") {
					let body = &self.fake.bodies[url];
					let half = body.len() / 2;
					fs::write(path, &body[..half])?;
					progress(half as u64, Some(body.len() as u64));
					tokio::time::timeout(
						Duration::from_secs(5),
						self.release.notified(),
					)
					.await?;
				}
				self.fake.fetch(url, path, cap, progress, cancel).await
			}
			async fn probe(&self, url: &str) -> Result<Duration> {
				self.fake.probe(url).await
			}
		}
		let dir = tempfile::tempdir().unwrap();
		let font = font_bytes();
		let urls = [
			"https://good.example/slow.otf",
			"https://good.example/fast.otf",
		];

		let transport = Delayed {
			fake: Fake {
				bodies: urls
					.iter()
					.map(|url| (url.to_string(), font.clone()))
					.collect(),
				..Default::default()
			},
			release: tokio::sync::Notify::new(),
		};
		let mut events = Vec::new();
		let summary = run(
			&[fan("noto", &urls)],
			dir.path(),
			&transport,
			2,
			Arc::new(|_| false),
			&mut |progress| {
				if progress.files_done == 1 && progress.files_progress > 1.0 {
					transport.release.notify_one();
				}
				events.push(progress);
			},
		);
		assert!(summary.failed.is_empty(), "{:?}", summary.failed);
		assert_eq!(summary.stored, 1);
		assert!(events.iter().any(|p| p.files_done == 1
			&& p.files_progress > 1.0
			&& p.files_progress < 2.0
			&& p.phase == Phase::Downloading));
		assert!(
			events
				.windows(2)
				.all(|pair| pair[0].bytes_done <= pair[1].bytes_done
					&& pair[0].files_progress <= pair[1].files_progress)
		);
		let last = events.last().unwrap();
		assert_eq!(last.files_done, 2);
		assert_eq!(last.files_progress, 2.0);
		assert_eq!(last.bytes_done, 2 * font.len() as u64);
	}

	/// Cancellation that lands as the last byte arrives must leave one coherent
	/// final state: the summary, the last progress event and the directory agree.
	#[test]
	fn cancellation_racing_completion_keeps_one_final_state() {
		struct Racing {
			bytes: Vec<u8>,
			armed: Arc<std::sync::atomic::AtomicBool>,
		}
		impl Transport for Racing {
			async fn fetch(
				&self,
				_: &str,
				path: &Path,
				_: u64,
				progress: &mut (dyn FnMut(u64, Option<u64>) + Send),
				_: &tokio_util::sync::CancellationToken,
			) -> Result<()> {
				fs::write(path, &self.bytes)?;
				progress(
					self.bytes.len() as u64,
					Some(self.bytes.len() as u64),
				);
				// The transfer is complete; cancellation arrives before it lands.
				self.armed.store(true, Ordering::SeqCst);
				Ok(())
			}
			async fn probe(&self, _: &str) -> Result<Duration> {
				Ok(Duration::ZERO)
			}
		}
		let dir = tempfile::tempdir().unwrap();
		let font = font_bytes();
		let armed = Arc::new(std::sync::atomic::AtomicBool::new(false));
		let transport = Racing {
			bytes: font.clone(),
			armed: armed.clone(),
		};
		let mut events = Vec::new();
		let summary = run(
			&[fan("noto", &["https://x.example/a.otf"])],
			dir.path(),
			&transport,
			1,
			Arc::new(move |_| armed.load(Ordering::SeqCst)),
			&mut |progress| events.push(progress),
		);
		assert!(summary.failed.is_empty(), "{:?}", summary.failed);
		// Exactly one outcome is recorded, never both.
		assert_eq!(summary.stored + summary.cancelled.len(), 1);
		let last = events
			.iter()
			.rev()
			.find(|progress| progress.id == "noto")
			.unwrap();
		assert!(last.current.is_none(), "{last:?}");
		let names: Vec<String> = fs::read_dir(dir.path())
			.unwrap()
			.flatten()
			.map(|entry| entry.file_name().to_string_lossy().into_owned())
			.collect();
		if summary.stored == 1 {
			assert_eq!(last.phase, Phase::Done);
			assert_eq!(names.len(), 1, "{names:?}");
		} else {
			assert_eq!(summary.cancelled, ["noto"]);
			assert_eq!(last.phase, Phase::Cancelled);
			assert!(names.is_empty(), "{names:?}");
		}
	}
}
