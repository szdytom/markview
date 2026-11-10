//! Debug-only failure injection for the device integration tests.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

static PICK_CANCELLED: AtomicBool = AtomicBool::new(false);
static OUTPUT: Mutex<Option<tokio::sync::oneshot::Receiver<Option<PathBuf>>>> =
	Mutex::new(None);

pub fn failure(unowned: EnvUnowned, kind: jint) {
	errors::boundary(unowned, "JNI failure injection", |env| {
		match kind {
			0 => with_env(|env, _| {
				env.throw_new(
					jni_str!("java/lang/IllegalStateException"),
					jni_str!("Injected Java failure"),
				)?;
				Ok(())
			})
			.context("Injected Rust-to-Java call")?,
			1 => {
				// A pending allocation exception exercises the failed return-string path.
				env.throw_new(
					jni_str!("java/lang/OutOfMemoryError"),
					jni_str!("Injected string allocation failure"),
				)?;
				env.new_string("return value")?;
			}
			2 => {
				REQUESTS.lock().unwrap().picked = Some(Box::new(|_| {
					let _requests = REQUESTS.lock().unwrap();
				}));
				picked_document(None);
				REQUESTS.lock().unwrap().picked = Some(Box::new(|_| {
					let _requests = REQUESTS.lock().unwrap();
				}));
				cancel_request(0);
				let (send, mut receive) = tokio::sync::oneshot::channel();
				REQUESTS.lock().unwrap().output = Some(send);
				cancel_request(1);
				anyhow::ensure!(
					receive.try_recv()
						== Err(
							tokio::sync::oneshot::error::TryRecvError::Closed
						)
				);
			}
			3 => panic!("Injected JNI panic"),
			4 => {
				run_guarded(|| anyhow::bail!("Injected reader startup failure"))
			}
			5 => {
				PICK_CANCELLED.store(false, Ordering::Relaxed);
				REQUESTS.lock().unwrap().picked = Some(Box::new(|path| {
					PICK_CANCELLED.store(path.is_none(), Ordering::Relaxed);
				}));
				let (send, receive) = tokio::sync::oneshot::channel();
				REQUESTS.lock().unwrap().output = Some(send);
				*OUTPUT.lock().unwrap() = Some(receive);
			}
			6 => {
				anyhow::ensure!(
					PICK_CANCELLED.load(Ordering::Relaxed),
					"Picker request was not cancelled"
				);
				let mut receive = OUTPUT
					.lock()
					.unwrap()
					.take()
					.expect("output probe installed");
				anyhow::ensure!(
					receive.try_recv() == Ok(None),
					"Export request was not cancelled"
				);
			}
			7 => run_guarded(|| panic!("Injected reader thread panic")),
			_ => anyhow::bail!("Unknown failure injection {kind}"),
		}
		Ok(())
	});
}
