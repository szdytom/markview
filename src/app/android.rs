//! Android reader setup and application debug state.
use super::{App, Event};
use winit::event_loop::ControlFlow;
pub(crate) fn run(
	android: markview_platform::android::AndroidApp,
) -> anyhow::Result<()> {
	let event_loop = markview_platform::android::event_loop::<Event>(android)?;
	event_loop.set_control_flow(ControlFlow::Wait);
	let proxy = event_loop.create_proxy();
	let mut args = crate::cli::LaunchOptions::default();
	crate::fonts::join_download_directory(
		&mut args.options.fonts,
		crate::fonts::directory(),
	);
	let mut app = App::new(args, proxy.clone());
	app.tab_strip.phone = markview_platform::android::phone_layout();
	super::open_document::install(proxy);
	let result = event_loop.run_app(&mut app);
	markview_platform::shutdown_events();
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
					"action": b.action.debug_name(), "enabled": b.enabled,
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
		// A web tab's path is a synthetic `web:` name, so the snapshot reports
		// the URL it stands for instead.
		let path = match &session.security.origin {
			crate::security::Origin::Web(url) => serde_json::json!(url),
			_ => serde_json::json!(session.path),
		};
		let tabs: Vec<String> = (0..self.readers.entries().len())
			.map(|index| self.readers.card_labels(index).1)
			.collect();
		let mut snapshot = serde_json::json!({
			"tabs": tabs,
			"active": self.readers.active(), "path": path,
			"trust": format!("{:?}", session.security.origin.trust()),
			"ready": session.snapshot_complete, "blocks": session.snapshot.blocks.len(),
			"math_errors": session.snapshot.math_errors,
			"scroll": session.scrolling.offset, "height": session.snapshot.height,
			"images": session.snapshot.images.entries.iter().map(|(src,info)| serde_json::json!({"src":src,"size":info.size,"error":info.error})).collect::<Vec<_>>(),
			"selected_styles": self.preferences.values.style,
			"window_layout":format!("{:?}", self.preferences.values.window_layout),
			"frame_layout":format!("{:?}", self.frame.layout),
			"single_instance":self.preferences.values.single_instance,
			"restore_session":self.preferences.values.restore_session,
			"phone_layout":self.tab_strip.phone,

			"column_width":self.preferences.values.width, "layout_width":session.snapshot.width,
			"drawer_scroll":self.tab_strip.drawer_scroll.offset(),
			"outline_open":self.interaction.overlays.outline.open(),
			"folder_root":self.folder.tree.as_ref().map(|t| &t.root),
			"folder_name":self.folder.tree.as_ref().map(|t| &t.name),
			"folder_open":self.folder.open || self.phone_folder_open(),
			"folder_entries":self.folder.tree.as_ref().map(|t| t.entries.iter().map(|e| serde_json::json!({"id":e.id,"path":e.path,"directory":e.directory,"depth":e.depth})).collect::<Vec<_>>()),
			"instance_listener":self.instance.is_some(),
			"theme": self.preferences.values.theme, "font_size":self.preferences.values.font_size,
			"font_catalog": self.preferences.fonts.view().catalog.len(),
			"styles":self.preferences.style_entries.iter().map(|e|e.id.clone()).collect::<Vec<_>>(),
			"font_revision":self.fonts_config.revision,
			"font_families":markview_layout::fonts::families(&self.fonts_config,false).len(),
			"panel":format!("{:?}",self.interaction.panel), "buttons":buttons,
			"panel_rect":[panel.x, panel.y, panel.w, panel.h],
			"dimensions":self.dimensions(), "insets":self.insets(),
			"search_open":session.search.open, "matches":session.search.matches.len(),
			"backend":self.renderer.as_ref().map(|r|format!("{:?}",r.backend)),
			"status":self.status.message()
		});
		snapshot["file_query"] =
			serde_json::json!(self.folder.start.input.text());
		snapshot["file_search_pending"] =
			serde_json::json!(self.folder.start.pending);

		snapshot["folder_show_excluded"] = self
			.folder
			.tree
			.as_ref()
			.is_some_and(|tree| tree.show_excluded)
			.into();
		snapshot["sidebar_tab"] =
			format!("{:?}", self.interaction.sidebar_tab).into();
		snapshot["sidebar_widths"] = serde_json::json!({
			"folder":self.preferences.values.folder_width,
			"outline":self.preferences.values.outline_width,
			"tabs":self.preferences.values.tabs_width
		});
		snapshot["search_query"] =
			self.readers.session.search.input.text().into();
		snapshot["url_query"] = self.url_input.text().into();
		let rect = self.search_input_rect();
		snapshot["search_rect"] =
			serde_json::json!([rect.x, rect.y, rect.w, rect.h]);
		snapshot.to_string()
	}
}
