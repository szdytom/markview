//! Background loading and temporary tabs for experimental web reading.
use super::{App, Event};
use std::path::PathBuf;

impl<P: super::SendEvent> App<P> {
	pub(super) fn open_web_page(&mut self, url: String) {
		let validated = crate::web_page::validate_url(&url);
		let url = validated.as_ref().map_or(url, ToString::to_string);
		let title = validated
			.as_ref()
			.ok()
			.and_then(|url| url.host_str())
			.unwrap_or(&url);
		let path = if let Some(path) = self.web_pages.get(&url).cloned()
			&& let Some(index) = self.readers.find_web(&path)
		{
			self.select_tab(index);
			if self.readers.session.load_error.is_none() {
				return;
			}
			self.readers.session.load_error = None;
			self.readers.session.web_loading = true;
			self.readers.session.layout_pending = true;
			self.watch = None;
			self.redraw();
			path
		} else {
			let path = self.temporary_path(title);
			self.open_pending(
				path.clone(),
				crate::security::Security::web(url.clone()),
			);
			path
		};
		self.web_pages.insert(url.clone(), path.clone());
		if let Err(error) = validated {
			self.web_page_loaded(url, path, Err(error));
			return;
		}
		if self.args.offline {
			let message = self.preferences.values.lang().status_web_offline();
			self.web_page_loaded(url, path, Err(anyhow::anyhow!(message)));
			return;
		}
		let security = self.readers.session.security.clone();
		let proxy = self.proxy.clone();
		let handle = self.services.handle.clone();
		let cancel = handle.cancel.clone();
		let fallback_url = url.clone();
		let fallback_path = path.clone();
		if !handle.clone().submit(async move {
			let result = async {
				let _permit = handle.permit(&cancel).await?;
				crate::web_page::load(
					&url,
					false,
					&handle.http_headers(),
					security,
				)
				.await
			};
			tokio::select! {
				biased;
				_ = cancel.cancelled() => {},
				result = result => proxy.send(Event::WebLoaded { url, path, result }),
			}
		}) {
			self.web_page_loaded(
				fallback_url,
				fallback_path,
				Err(anyhow::anyhow!("Network service closed")),
			);
		}
	}

	pub(super) fn web_page_loaded(
		&mut self,
		url: String,
		path: PathBuf,
		result: anyhow::Result<crate::web_page::Article>,
	) {
		if self.web_pages.get(&url) != Some(&path) {
			return;
		}
		let Some(index) = self.readers.find_web(&path) else {
			return;
		};
		let result = result.and_then(|article| {
			let markdown = article.markdown;
			self.readers.session_mut(index).security = article.security;
			let title = crate::paste::title_for(
				&markdown,
				self.preferences.values.lang(),
			);
			let loaded_path = self.temporary_path(&title);
			std::fs::write(&loaded_path, markdown)?;
			Ok(loaded_path)
		});
		self.readers.session_mut(index).web_loading = false;
		match result {
			Ok(loaded_path) => {
				self.readers.rename(index, loaded_path.clone());
				self.web_pages.insert(url, loaded_path);
				if index == self.readers.active() {
					self.observe_document();
					self.request(false);
				}
			}
			Err(error) => {
				if index == self.readers.active()
					&& let Some(required) = error
						.downcast_ref::<crate::security::PermissionRequired>(
					) {
					self.ask_permission(
						required.0.clone(),
						crate::app::modal::PermissionAction::Web(url),
					);
				}
				let message = self
					.preferences
					.values
					.lang()
					.status_web_failed(format!("{error:#}"));
				let session = self.readers.session_mut(index);
				session.load_error = Some(message);
				session.layout_pending = false;
			}
		}
		self.redraw();
	}
}
