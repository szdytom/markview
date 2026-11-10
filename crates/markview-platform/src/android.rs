//! Android OS services; all reader state stays in the shared application.
#![allow(unsafe_code)]
use anyhow::{Context, Result, anyhow};
#[path = "android/errors.rs"]
mod errors;
#[cfg(debug_assertions)]
#[path = "android/testing.rs"]
pub mod testing;
pub use jni;
use jni::{
	Env, EnvUnowned, JavaVM, jni_sig, jni_str,
	objects::{JClass, JObject, JString, JValue},
	refs::Cast,
	strings::JNIString,
	sys::jint,
};
use std::{path::PathBuf, sync::Mutex};
pub use winit::platform::android::activity::AndroidApp;

static APP: Mutex<Option<AndroidApp>> = Mutex::new(None);
static REQUESTS: Mutex<crate::requests::Requests> =
	Mutex::new(crate::requests::Requests {
		picked: None,
		output: None,
	});

#[derive(Clone, Copy)]
struct Layout {
	smallest_width: i32,
	phone: bool,
}
static LAYOUT: Mutex<Option<Layout>> = Mutex::new(None);

pub fn initialize(app: AndroidApp) -> Result<()> {
	*APP.lock().unwrap() = Some(app);
	refresh_layout()
}
pub fn shutdown() {
	REQUESTS.lock().unwrap().clear();
	APP.lock().unwrap().take();
	crate::shutdown_events();
}
pub fn data_path() -> Option<PathBuf> {
	APP.lock().unwrap().as_ref()?.internal_data_path()
}
fn with_env<T>(
	call: impl FnOnce(&mut Env, &JObject) -> Result<T>,
) -> Result<T> {
	let app = APP
		.lock()
		.unwrap()
		.as_ref()
		.cloned()
		.ok_or_else(|| anyhow!("Android activity unavailable"))?;
	// SAFETY: `AndroidApp` owns a reference to the live VM for this call.
	let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };

	vm.attach_current_thread(|env| {
		env.with_local_frame(16, |env| {
			// SAFETY: The glue retains the activity's global reference while `app`
			// and the attached environment live; the wrapper stays inside this frame.
			let raw = app.activity_as_ptr().cast();
			let activity = unsafe { Cast::<JObject>::from_raw(env, &raw) }?;
			call(env, &activity)
		})
	})
}
pub fn call_string(method: &str, text: &str) -> Result<()> {
	with_env(|env, activity| {
		let text = env.new_string(text)?;
		env.call_method(
			activity,
			JNIString::new(method),
			jni_sig!("(Ljava/lang/String;)V"),
			&[JValue::Object(&text)],
		)?;
		Ok(())
	})
	.with_context(|| format!("Android {method}(String)"))
	.inspect_err(|error| report_error(&errors::describe(error), false))
}
fn refresh_layout() -> Result<()> {
	let smallest_width = with_env(|env, activity| {
		Ok(env
			.call_method(
				activity,
				jni_str!("smallestWidthDp"),
				jni_sig!("()I"),
				&[],
			)?
			.i()?)
	})
	.context("Android smallestWidthDp()I")?;
	let phone = with_env(|env, activity| {
		Ok(env
			.call_method(
				activity,
				jni_str!("phoneLayout"),
				jni_sig!("()Z"),
				&[],
			)?
			.z()?)
	})
	.context("Android phoneLayout()Z")?;
	*LAYOUT.lock().unwrap() = Some(Layout {
		smallest_width,
		phone,
	});
	Ok(())
}
pub fn smallest_width_dp() -> i32 {
	LAYOUT
		.lock()
		.unwrap()
		.expect("layout is initialized before the reader starts")
		.smallest_width
}
pub fn phone_layout() -> bool {
	LAYOUT
		.lock()
		.unwrap()
		.expect("layout is initialized before the reader starts")
		.phone
}
pub fn clipboard_read() -> Result<String> {
	with_env(|env, activity| {
		let text = env
			.call_method(
				activity,
				jni_str!("pasteText"),
				jni_sig!("()Ljava/lang/String;"),
				&[],
			)?
			.l()?;
		Ok(JString::cast_local(env, text)?.try_to_string(env)?)
	})
	.context("Android pasteText()String")
	.inspect_err(|error| report_error(&errors::describe(error), false))
}

// SAFETY: The VM supplies a valid `EnvUnowned` and class for this debug-only test.
#[cfg(debug_assertions)]
pub extern "system" fn bridge_references_released(
	unowned: EnvUnowned,
	_: JClass,
) -> jni::sys::jboolean {
	errors::boundary(unowned, "JNI reference test", |env| {
		let mut references = Vec::new();
		for fail in [false, true] {
			for _ in 0..32 {
				let result = with_env(|env, _| {
					let text = env.new_string("bridge reference")?;
					references.push(env.new_weak_ref(&text)?);
					if fail {
						anyhow::bail!("Test bridge failure");
					}
					Ok(())
				});
				anyhow::ensure!(result.is_err() == fail);
			}
		}
		env.call_static_method(
			jni_str!("java/lang/System"),
			jni_str!("gc"),
			jni_sig!("()V"),
			&[],
		)?;
		for reference in references {
			anyhow::ensure!(
				reference.is_garbage_collected(env)?,
				"JNI local reference retained"
			);
		}
		Ok(true)
	})
}
pub fn pick_document(done: impl FnOnce(Option<PathBuf>) + Send + 'static) {
	pick("pickDocument", done);
}
pub fn pick_folder(done: impl FnOnce(Option<PathBuf>) + Send + 'static) {
	pick("pickFolder", done);
}
fn pick(method: &str, done: impl FnOnce(Option<PathBuf>) + Send + 'static) {
	REQUESTS.lock().unwrap().picked = Some(Box::new(done));
	if let Err(error) = call_string(method, "") {
		log::error!("File picker: {error:#}");
		let done = REQUESTS.lock().unwrap().picked.take();
		if let Some(done) = done {
			done(None);
		}
	}
}
pub fn choose_output(
	name: String,
) -> impl Future<Output = Option<PathBuf>> + Send {
	let (send, receive) = tokio::sync::oneshot::channel();
	REQUESTS.lock().unwrap().output = Some(send);
	if let Err(error) = call_string("chooseOutput", &name) {
		log::error!("Export picker: {error:#}");
		REQUESTS.lock().unwrap().output.take();
	}
	async move { receive.await.ok().flatten() }
}

pub fn shared_url(
	unowned: EnvUnowned,
	_: JClass,
	text: JString,
	parse: fn(&str) -> Option<String>,
) -> jni::sys::jstring {
	errors::boundary(unowned, "Parse shared URL", |env| {
		let text = text.try_to_string(env).context("Read shared text")?;
		match parse(&text) {
			Some(url) => {
				Ok(env.new_string(url).context("Return shared URL")?.into_raw())
			}
			None => Ok(std::ptr::null_mut()),
		}
	})
}

// SAFETY: The VM resolves this symbol for the declared Java native method;
// `EnvUnowned` and the local references are valid for this invocation.
pub fn native_result(
	unowned: EnvUnowned,
	_: JClass,
	kind: jint,
	path: JString,
) {
	errors::boundary(unowned, "Android event", |env| {
		let path = (!path.is_null())
			.then(|| path.try_to_string(env))
			.transpose();
		let path = match path {
			Ok(path) => path,
			Err(error) => {
				cancel_request(kind);
				return Err(error)
					.with_context(|| format!("Read event {kind} payload"));
			}
		};
		match kind {
			0 => picked_document(path.map(PathBuf::from)),
			1 => {
				let send = REQUESTS.lock().unwrap().output.take();
				if let Some(send) = send {
					let _ = send.send(path.map(PathBuf::from));
				}
			}
			2 => crate::events::send(crate::NativeEvent::AssetsChanged),
			3 => crate::events::send(crate::NativeEvent::Back),
			4 => {
				refresh_layout()?;
				crate::events::send(crate::NativeEvent::Configuration);
			}
			5 => {
				if let Some(url) = path {
					crate::events::send(crate::NativeEvent::OpenUrl(url));
				}
			}
			6 => {
				if let Some(path) = path {
					crate::events::send(crate::NativeEvent::Shared(
						PathBuf::from(path),
					));
				}
			}
			7 => {
				if let Some(text) = path {
					crate::events::send(crate::NativeEvent::InputText(text));
				}
			}
			_ => anyhow::bail!("Unknown Android event kind {kind}"),
		}
		Ok(())
	});
}

fn cancel_request(kind: jint) {
	match kind {
		0 => {
			let done = REQUESTS.lock().unwrap().picked.take();
			if let Some(done) = done {
				done(None);
			}
		}
		1 => {
			REQUESTS.lock().unwrap().output.take();
		}
		_ => {}
	}
}

pub fn native_delete(
	unowned: EnvUnowned,
	before: jint,
	after: jint,
	code_points: jni::sys::jboolean,
) {
	errors::boundary(unowned, "Delete input text", |_| {
		crate::events::send(crate::NativeEvent::InputDelete {
			before: before.max(0) as usize,
			after: after.max(0) as usize,
			code_points,
		});
		Ok(())
	});
}

fn picked_document(path: Option<PathBuf>) {
	let done = REQUESTS.lock().unwrap().picked.take();
	if let Some(done) = done {
		done(path);
	} else if let Some(path) = path {
		crate::events::send(crate::NativeEvent::Open(path));
	}
}

pub fn insets(width: u32, height: u32, scale: f32) -> [f32; 4] {
	let app = APP.lock().unwrap();
	let rect = app
		.as_ref()
		.expect("the reader owns an initialized Android activity")
		.content_rect();
	if rect.right <= rect.left || rect.bottom <= rect.top {
		return [0.0; 4];
	}
	[
		rect.left as f32 / scale,
		rect.top as f32 / scale,
		(width as f32 - rect.right as f32).max(0.0) / scale,
		(height as f32 - rect.bottom as f32).max(0.0) / scale,
	]
}

pub fn event_loop<T: 'static>(
	android: AndroidApp,
) -> Result<winit::event_loop::EventLoop<T>> {
	use winit::platform::android::EventLoopBuilderExtAndroid;
	Ok(winit::event_loop::EventLoop::<T>::with_user_event()
		.with_android_app(android)
		.build()?)
}
fn report_error(details: &str, fatal: bool) {
	log::error!("{details}");
	let result = with_env(|env, activity| {
		let text = env.new_string(details)?;
		env.call_method(
			activity,
			jni_str!("showError"),
			jni_sig!("(Ljava/lang/String;Z)V"),
			&[JValue::Object(&text), JValue::Bool(fatal)],
		)?;
		Ok(())
	});
	if let Err(error) = result {
		log::error!("Cannot display Android error: {error:#}");
	}
}

pub fn start(
	android: AndroidApp,
	mark_start: fn(),
	run: fn(AndroidApp) -> Result<()>,
) {
	errors::initialize();
	run_guarded(|| {
		mark_start();
		initialize(android.clone()).context("Initialize Android layout")?;
		run(android).context("Run Android reader")
	});
	shutdown();
}

fn run_guarded(run: impl FnOnce() -> Result<()>) {
	let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run));
	match outcome {
		Ok(Err(error)) => report_error(&errors::describe(&error), true),
		Err(payload) => report_error(&errors::panic_details(&*payload), true),
		Ok(Ok(())) => {}
	}
}
#[cfg(debug_assertions)]
pub fn snapshot(unowned: EnvUnowned) -> jni::sys::jstring {
	errors::boundary(unowned, "Inspect reader", |env| {
		let (send, receive) = std::sync::mpsc::channel();
		crate::events::send(crate::NativeEvent::Inspect(send));
		let state = receive
			.recv_timeout(std::time::Duration::from_secs(3))
			.unwrap_or_else(|_| "{}".into());
		Ok(env.new_string(state)?.into_raw())
	})
}

/// Expand exports in the final library so Android's loader retains their ABI.
#[macro_export]
macro_rules! android_entry {
    ($start:path, $run:path, $url:path) => {
        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        fn android_main(app: $crate::android::AndroidApp) { $crate::android::start(app, $start, $run); }
        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "system" fn Java_io_github_szdytom_markview_MarkviewActivity_nativeResult(env: $crate::android::jni::EnvUnowned, class: $crate::android::jni::objects::JClass, kind: $crate::android::jni::sys::jint, path: $crate::android::jni::objects::JString) { $crate::android::native_result(env, class, kind, path); }
        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "system" fn Java_io_github_szdytom_markview_MarkviewActivity_nativeSharedUrl(env: $crate::android::jni::EnvUnowned, class: $crate::android::jni::objects::JClass, text: $crate::android::jni::objects::JString) -> $crate::android::jni::sys::jstring { $crate::android::shared_url(env, class, text, $url) }
        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "system" fn Java_io_github_szdytom_markview_MarkviewActivity_nativeDelete(env: $crate::android::jni::EnvUnowned, _: $crate::android::jni::objects::JClass, before: $crate::android::jni::sys::jint, after: $crate::android::jni::sys::jint, code_points: $crate::android::jni::sys::jboolean) { $crate::android::native_delete(env, before, after, code_points); }
        #[cfg(debug_assertions)]
        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "system" fn Java_io_github_szdytom_markview_MarkviewActivity_nativeSnapshot(env: $crate::android::jni::EnvUnowned, _: $crate::android::jni::objects::JClass) -> $crate::android::jni::sys::jstring { $crate::android::snapshot(env) }
        #[cfg(debug_assertions)]
        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "system" fn Java_io_github_szdytom_markview_MarkviewActivity_nativeBridgeReferencesReleased(env: $crate::android::jni::EnvUnowned, class: $crate::android::jni::objects::JClass) -> $crate::android::jni::sys::jboolean { $crate::android::bridge_references_released(env, class) }
        #[cfg(debug_assertions)]
        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "system" fn Java_io_github_szdytom_markview_MarkviewActivity_nativeTestFailure(env: $crate::android::jni::EnvUnowned, _: $crate::android::jni::objects::JClass, kind: $crate::android::jni::sys::jint) { $crate::android::testing::failure(env, kind); }
    };
}
