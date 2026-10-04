//! Android OS services; all reader state stays in the shared application.
#![allow(unsafe_code)]
use anyhow::{Result, anyhow};
use jni::{
	JNIEnv, JavaVM,
	objects::{JClass, JObject, JString, JValue},
	sys::jint,
};
use std::{path::PathBuf, sync::Mutex};
use winit::platform::android::activity::AndroidApp;

type Picked = Box<dyn FnOnce(Option<PathBuf>) + Send>;
static APP: Mutex<Option<AndroidApp>> = Mutex::new(None);
static PICKED: Mutex<Option<Picked>> = Mutex::new(None);
static OUTPUT: Mutex<Option<tokio::sync::oneshot::Sender<Option<PathBuf>>>> =
	Mutex::new(None);

pub(crate) fn initialize(app: AndroidApp) {
	*APP.lock().unwrap() = Some(app);
}
pub(crate) fn data_path() -> Option<PathBuf> {
	APP.lock().unwrap().as_ref()?.internal_data_path()
}
fn with_env<T>(
	call: impl FnOnce(&mut JNIEnv, &JObject) -> Result<T>,
) -> Result<T> {
	let app = APP
		.lock()
		.unwrap()
		.as_ref()
		.cloned()
		.ok_or_else(|| anyhow!("Android activity unavailable"))?;
	// SAFETY: `AndroidApp` owns a reference to the live VM for this call.
	let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
	let mut env = vm.attach_current_thread()?;
	// SAFETY: The glue retains the activity's global reference; the borrowed
	// `JObject` is only used while `app` and the attached environment live.
	let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
	call(&mut env, &activity)
}
pub(crate) fn call_string(method: &str, text: &str) -> Result<()> {
	with_env(|env, activity| {
		let text = env.new_string(text)?;
		env.call_method(
			activity,
			method,
			"(Ljava/lang/String;)V",
			&[JValue::Object(&text)],
		)?;
		Ok(())
	})
}
pub(crate) fn clipboard_read() -> Result<String> {
	with_env(|env, activity| {
		let text = env
			.call_method(activity, "pasteText", "()Ljava/lang/String;", &[])?
			.l()?;
		Ok(env.get_string(&JString::from(text))?.into())
	})
}
pub(crate) fn pick_document(
	done: impl FnOnce(Option<PathBuf>) + Send + 'static,
) {
	*PICKED.lock().unwrap() = Some(Box::new(done));
	if let Err(error) = call_string("pickDocument", "") {
		log::error!("File picker: {error:#}");
		if let Some(done) = PICKED.lock().unwrap().take() {
			done(None);
		}
	}
}
pub(crate) fn choose_output(
	name: String,
) -> impl Future<Output = Option<PathBuf>> + Send {
	let (send, receive) = tokio::sync::oneshot::channel();
	*OUTPUT.lock().unwrap() = Some(send);
	if let Err(error) = call_string("chooseOutput", &name) {
		log::error!("Export picker: {error:#}");
		OUTPUT.lock().unwrap().take();
	}
	async move { receive.await.ok().flatten() }
}

// SAFETY: The VM resolves this symbol for the declared Java native method;
// `JNIEnv` and the local references are valid for this invocation.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_szdytom_markview_MarkviewActivity_nativeResult(
	mut env: JNIEnv,
	_: JClass,
	kind: jint,
	path: JString,
) {
	let path = (!path.is_null())
		.then(|| {
			env.get_string(&path)
				.map(|s| PathBuf::from(String::from(s)))
		})
		.transpose();
	let path = match path {
		Ok(path) => path,
		Err(error) => {
			log::error!("Document handoff: {error}");
			return;
		}
	};
	match kind {
		0 => {
			if let Some(done) = PICKED.lock().unwrap().take() {
				done(path);
			} else if let Some(path) = path {
				crate::app::android::open(path);
			}
		}
		1 => {
			if let Some(send) = OUTPUT.lock().unwrap().take() {
				let _ = send.send(path);
			}
		}
		2 => crate::app::android::assets_changed(),
		3 => crate::app::android::back(),
		_ => unreachable!(),
	}
}

pub(crate) fn insets(width: u32, height: u32, scale: f32) -> [f32; 4] {
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
