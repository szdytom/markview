//! Android OS services; all reader state stays in the shared application.
#![allow(unsafe_code)]
use anyhow::{Result, anyhow};
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

pub fn initialize(app: AndroidApp) {
	*APP.lock().unwrap() = Some(app);
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
}
pub fn smallest_width_dp() -> i32 {
	with_env(|env, activity| {
		Ok(env
			.call_method(
				activity,
				jni_str!("smallestWidthDp"),
				jni_sig!("()I"),
				&[],
			)?
			.i()?)
	})
	.unwrap()
}
pub fn phone_layout() -> bool {
	with_env(|env, activity| {
		Ok(env
			.call_method(
				activity,
				jni_str!("phoneLayout"),
				jni_sig!("()Z"),
				&[],
			)?
			.z()?)
	})
	.unwrap()
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
}

// SAFETY: The VM supplies a valid `EnvUnowned` and class for this debug-only test.
#[cfg(debug_assertions)]
pub extern "system" fn bridge_references_released(
	unowned: EnvUnowned,
	_: JClass,
) -> jni::sys::jboolean {
	// SAFETY: Java owns the current thread attachment for this invocation.
	let mut attachment =
		unsafe { jni::AttachGuard::from_unowned(unowned.as_raw()) };
	let env = attachment.borrow_env_mut();
	let mut check = || -> Result<()> {
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
		Ok(())
	};
	match check() {
		Ok(()) => true,
		Err(error) => {
			log::error!("JNI reference test: {error:#}");
			false
		}
	}
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
		if let Some(done) = REQUESTS.lock().unwrap().picked.take() {
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

// SAFETY: Java passes a non-null `String`; the environment and references
// remain valid throughout this native call.
pub fn shared_url(
	unowned: EnvUnowned,
	_: JClass,
	text: JString,
	parse: fn(&str) -> Option<String>,
) -> jni::sys::jstring {
	// SAFETY: Java owns the current thread attachment for this invocation.
	let mut attachment =
		unsafe { jni::AttachGuard::from_unowned(unowned.as_raw()) };
	let env = attachment.borrow_env_mut();
	let text = text.try_to_string(env).unwrap();
	parse(&text).map_or(std::ptr::null_mut(), |url| {
		env.new_string(url).unwrap().into_raw()
	})
}

// SAFETY: The VM resolves this symbol for the declared Java native method;
// `EnvUnowned` and the local references are valid for this invocation.
pub extern "system" fn native_result(
	unowned: EnvUnowned,
	_: JClass,
	kind: jint,
	path: JString,
) {
	// SAFETY: Java owns the current thread attachment for this invocation.
	let mut attachment =
		unsafe { jni::AttachGuard::from_unowned(unowned.as_raw()) };
	let env = attachment.borrow_env_mut();
	let path = (!path.is_null())
		.then(|| path.try_to_string(env))
		.transpose();
	let path = match path {
		Ok(path) => path,
		Err(error) => {
			log::error!("Document handoff: {error}");
			return;
		}
	};
	match kind {
		0 => picked_document(path.map(PathBuf::from)),
		1 => {
			if let Some(send) = REQUESTS.lock().unwrap().output.take() {
				let _ = send.send(path.map(PathBuf::from));
			}
		}
		2 => crate::events::send(crate::NativeEvent::AssetsChanged),
		3 => crate::events::send(crate::NativeEvent::Back),
		4 => crate::events::send(crate::NativeEvent::Configuration),
		5 => {
			if let Some(url) = path {
				crate::events::send(crate::NativeEvent::OpenUrl(url));
			}
		}
		6 => {
			if let Some(path) = path {
				crate::events::send(crate::NativeEvent::Shared(PathBuf::from(
					path,
				)));
			}
		}
		7 => {
			if let Some(text) = path {
				crate::events::send(crate::NativeEvent::InputText(text));
			}
		}
		8 | 9 => {
			let counts = path.unwrap();
			let (before, after) = counts.split_once(',').unwrap();
			crate::events::send(crate::NativeEvent::InputDelete {
				before: before.parse().unwrap(),
				after: after.parse().unwrap(),
				code_points: kind == 9,
			});
		}
		_ => unreachable!(),
	}
}

fn picked_document(path: Option<PathBuf>) {
	if let Some(done) = REQUESTS.lock().unwrap().picked.take() {
		done(path);
	} else if let Some(path) = path {
		crate::events::send(crate::NativeEvent::Open(path));
	}
}

pub fn insets(width: u32, height: u32, scale: f32) -> [f32; 4] {
	let app = APP.lock().unwrap();
	let rect = app.as_ref().unwrap().content_rect();
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
pub fn start(
	android: AndroidApp,
	mark_start: fn(),
	run: fn(AndroidApp) -> Result<()>,
) {
	mark_start();
	android_logger::init_once(
		android_logger::Config::default()
			.with_tag("Markview")
			.with_max_level(log::LevelFilter::Info),
	);
	initialize(android.clone());
	if let Err(error) = run(android) {
		log::error!("Markview: {error:#}");
	}
	shutdown();
}
#[cfg(debug_assertions)]
pub fn snapshot(unowned: EnvUnowned) -> jni::sys::jstring {
	// SAFETY: Java owns the current thread attachment for this invocation.
	let mut attachment =
		unsafe { jni::AttachGuard::from_unowned(unowned.as_raw()) };
	let env = attachment.borrow_env_mut();
	let (send, receive) = std::sync::mpsc::channel();
	crate::events::send(crate::NativeEvent::Inspect(send));
	let state = receive
		.recv_timeout(std::time::Duration::from_secs(3))
		.unwrap_or_else(|_| "{}".into());
	env.new_string(state).unwrap().into_raw()
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
        #[cfg(debug_assertions)]
        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "system" fn Java_io_github_szdytom_markview_MarkviewActivity_nativeSnapshot(env: $crate::android::jni::EnvUnowned, _: $crate::android::jni::objects::JClass) -> $crate::android::jni::sys::jstring { $crate::android::snapshot(env) }
        #[cfg(debug_assertions)]
        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "system" fn Java_io_github_szdytom_markview_MarkviewActivity_nativeBridgeReferencesReleased(env: $crate::android::jni::EnvUnowned, class: $crate::android::jni::objects::JClass) -> $crate::android::jni::sys::jboolean { $crate::android::bridge_references_released(env, class) }
    };
}
