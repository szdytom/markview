//! Markview runs the desktop reader with Android's activity and event loop.
#![allow(unsafe_code)]
use super::{App, Event};
use std::{path::PathBuf, sync::Mutex};
use winit::{
	event_loop::{ControlFlow, EventLoop, EventLoopProxy},
	platform::android::{EventLoopBuilderExtAndroid, activity::AndroidApp},
};

static PROXY: Mutex<(Option<EventLoopProxy<Event>>, Vec<PathBuf>)> =
	Mutex::new((None, Vec::new()));
pub(crate) fn open(path: PathBuf) {
	let mut bridge = PROXY.lock().unwrap();
	if let Some(proxy) = &bridge.0 {
		let _ = proxy.send_event(Event::Open(Some(path)));
	} else {
		bridge.1.push(path);
	}
}

pub(crate) fn back() {
	if let Some(proxy) = &PROXY.lock().unwrap().0 {
		let _ = proxy.send_event(Event::AndroidBack);
	}
}

pub(crate) fn configuration_changed() {
	if let Some(proxy) = &PROXY.lock().unwrap().0 {
		let _ = proxy.send_event(Event::AndroidConfiguration);
	}
}

pub(crate) fn assets_changed() {
	if let Some(proxy) = &PROXY.lock().unwrap().0 {
		let _ = proxy.send_event(Event::Fonts(
			super::font_panel::Message::Settled(Box::new(
				crate::fonts::Summary {
					stored: 1,
					..Default::default()
				},
			)),
		));
		let _ = proxy.send_event(Event::StylesChanged);
	}
}

// SAFETY: `android-activity` calls this symbol on its application thread with
// its owned `AndroidApp`, as required by `winit`'s native activity backend.
#[unsafe(no_mangle)]
fn android_main(android: AndroidApp) {
	crate::mark_process_start();
	android_logger::init_once(
		android_logger::Config::default()
			.with_tag("Markview")
			.with_max_level(log::LevelFilter::Info),
	);
	crate::platform::android::initialize(android.clone());
	if let Err(error) = run(android) {
		log::error!("Markview: {error:#}");
	}
	*PROXY.lock().unwrap() = (None, Vec::new());
	crate::platform::android::shutdown();
}
fn run(android: AndroidApp) -> anyhow::Result<()> {
	let event_loop = EventLoop::<Event>::with_user_event()
		.with_android_app(android)
		.build()?;
	event_loop.set_control_flow(ControlFlow::Wait);
	let proxy = event_loop.create_proxy();
	let mut args = crate::cli::LaunchOptions::default();
	crate::fonts::join_download_directory(
		&mut args.options.fonts,
		crate::fonts::directory(),
	);
	let mut app = App::new(args, proxy.clone());
	app.tab_strip.phone = crate::platform::android::phone_layout();
	{
		let mut bridge = PROXY.lock().unwrap();
		bridge.0 = Some(proxy.clone());
		for path in bridge.1.drain(..) {
			let _ = proxy.send_event(Event::Open(Some(path)));
		}
	}
	let result = event_loop.run_app(&mut app);
	PROXY.lock().unwrap().0 = None;
	app.flush_settings();
	result?;
	if let Some(error) = app.fatal.take() {
		anyhow::bail!(error);
	}
	Ok(())
}

#[cfg(debug_assertions)]
impl<P: super::SendEvent> App<P> {
	pub(super) fn android_snapshot(&mut self) -> String {
		let mut buttons: Vec<_> = self
			.buttons()
			.into_iter()
			.map(|b| {
				serde_json::json!({
					"action": format!("{:?}", b.action), "enabled": b.enabled,
					"x": b.rect.x, "y": b.rect.y, "w": b.rect.w, "h": b.rect.h
				})
			})
			.collect();
		let layout = self.tab_layout();
		let tab_buttons: Vec<_> = layout.rects.iter().enumerate().filter_map(|(index, rect)| {
			let rect = rect.intersect(layout.viewport)?;
			Some(serde_json::json!({"action": format!("SelectTab({index})"), "enabled": true, "x":rect.x,"y":rect.y,"w":(rect.w-24.0).max(1.0),"h":rect.h}))
		}).collect();
		buttons.extend(tab_buttons);
		let session = &self.readers.session;
		let (width, height, _) = self.dimensions();
		let panel = super::chrome::panel_rect(width, height);
		serde_json::json!({
			"tabs": self.readers.entries().iter().map(|tab| tab.path.display().to_string()).collect::<Vec<_>>(),
			"active": self.readers.active(), "path": session.path,
			"ready": session.snapshot_complete, "blocks": session.snapshot.blocks.len(),
			"math_errors": session.snapshot.math_errors,
			"scroll": session.scrolling.offset, "height": session.snapshot.height,
			"images": session.snapshot.images.entries.iter().map(|(src,info)| serde_json::json!({"src":src,"size":info.size,"error":info.error})).collect::<Vec<_>>(),
			"selected_styles": self.preferences.values.style,
			"window_layout":format!("{:?}", self.preferences.values.window_layout),
			"frame_layout":format!("{:?}", self.frame.layout),
			"single_instance":self.preferences.values.single_instance,
			"phone_layout":self.tab_strip.phone,
			"column_width":self.preferences.values.width, "layout_width":session.snapshot.width,
			"drawer_scroll":self.tab_strip.drawer_scroll,
			"outline_open":self.interaction.outline_open,
			"tab_style":format!("{:?}", self.preferences.values.tab_style),
			"instance_listener":self.instance.is_some(),
			"theme": self.preferences.values.theme, "font_size":self.preferences.values.font_size,
			"font_catalog": self.font_panel.view().catalog.len(),
			"styles":self.preferences.style_entries.iter().map(|e|e.id.clone()).collect::<Vec<_>>(),
			"font_revision":self.fonts_config.revision,
			"font_families":markview_core::fonts::families(&self.fonts_config,false).len(),
			"panel":format!("{:?}",self.interaction.panel), "buttons":buttons,
			"panel_rect":[panel.x, panel.y, panel.w, panel.h],
			"dimensions":self.dimensions(), "insets":self.insets(),
			"search_open":session.search.open, "matches":session.search.matches.len(),
			"backend":self.renderer.as_ref().map(|r|format!("{:?}",r.backend)),
			"status":self.status
		}).to_string()
	}
}

// SAFETY: The VM resolves this test-only symbol for the Java native method;
// its environment and class reference are valid throughout this call.
#[cfg(debug_assertions)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_szdytom_markview_MarkviewActivity_nativeSnapshot(
	env: jni::JNIEnv,
	_: jni::objects::JClass,
) -> jni::sys::jstring {
	let (send, receive) = std::sync::mpsc::channel();
	let proxy = PROXY.lock().unwrap().0.clone();
	let state = proxy
		.and_then(|proxy| {
			proxy.send_event(Event::AndroidInspect(send)).ok()?;
			receive.recv_timeout(std::time::Duration::from_secs(3)).ok()
		})
		.unwrap_or_else(|| "{}".into());
	env.new_string(state).unwrap().into_raw()
}
