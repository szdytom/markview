//! Error policy for Java-owned JNI frames and the native reader thread.
use anyhow::Result;
use jni::{Env, EnvUnowned, errors::ErrorPolicy, jni_str, strings::JNIString};
use std::{any::Any, backtrace::Backtrace, cell::RefCell, sync::Once};

thread_local! {
	static PANIC: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub(super) fn initialize() {
	static INIT: Once = Once::new();
	INIT.call_once(|| {
		android_logger::init_once(
			android_logger::Config::default()
				.with_tag("Markview")
				.with_max_level(log::LevelFilter::Info),
		);
		let previous = std::panic::take_hook();
		std::panic::set_hook(Box::new(move |info| {
			let details = format!("{info}\n{}", Backtrace::force_capture());
			log::error!("{details}");
			PANIC.with(|slot| *slot.borrow_mut() = Some(details));
			previous(info);
		}));
	});
}

pub(super) fn panic_details(payload: &(dyn Any + Send)) -> String {
	PANIC
		.with(|slot| slot.borrow_mut().take())
		.unwrap_or_else(|| {
			payload
				.downcast_ref::<String>()
				.cloned()
				.or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).into()))
				.unwrap_or_else(|| "Rust panic (non-string payload)".into())
		})
}

pub(super) fn boundary<'local, T: Default>(
	mut unowned: EnvUnowned<'local>,
	operation: &'static str,
	call: impl FnOnce(&mut Env<'local>) -> Result<T>,
) -> T {
	unowned
		.with_env(|env| {
			initialize();
			call(env)
		})
		.resolve_with::<BoundaryErrors, _>(|| operation)
}

pub(super) fn describe(error: &anyhow::Error) -> String {
	error
		.chain()
		.map(|cause| match cause.downcast_ref::<jni::errors::Error>() {
			Some(jni::errors::Error::CaughtJavaException {
				name,
				msg,
				stack,
				..
			}) => format!("{name}: {msg}\nStack trace:\n{stack}"),
			_ => cause.to_string(),
		})
		.collect::<Vec<_>>()
		.join(": ")
}

fn throw<T: Default>(
	env: &mut Env,
	class: &'static jni::strings::JNIStr,
	details: String,
) -> jni::errors::Result<T> {
	// `throw_new` returns `JavaException` when the exception was thrown successfully.
	if let Err(error) = env.throw_new(class, JNIString::new(details))
		&& !matches!(error, jni::errors::Error::JavaException)
	{
		return Err(error);
	}
	Ok(T::default())
}

struct BoundaryErrors;
impl<T: Default> ErrorPolicy<T, anyhow::Error> for BoundaryErrors {
	type Captures<'local: 'call, 'call> = &'static str;

	fn on_error<'local: 'call, 'call>(
		env: &mut Env<'local>,
		operation: &mut &'static str,
		error: anyhow::Error,
	) -> jni::errors::Result<T> {
		// Preserve the original Java stack before throwing our contextual error.
		let exception = env.exception_catch().err();
		let mut details = format!("{operation}: {}", describe(&error));
		if let Some(exception) = exception {
			details.push_str(&format!("\n{}", describe(&exception.into())));
		}
		log::error!("{details}");
		throw(env, jni_str!("java/lang/RuntimeException"), details)
	}

	fn on_panic<'local: 'call, 'call>(
		env: &mut Env<'local>,
		operation: &mut &'static str,
		payload: Box<dyn Any + Send>,
	) -> jni::errors::Result<T> {
		let details = format!("{operation}: {}", panic_details(&*payload));
		log::error!("{details}");
		let _ = env.exception_catch();
		throw(
			env,
			jni_str!("io/github/szdytom/markview/MarkviewActivity$NativePanic"),
			details,
		)
	}
}
