//! Background loading and in-memory tabs for web reading.
use super::{App, Event};
use crate::loaded::LoadedDocument;
use std::path::PathBuf;
use std::sync::Arc;

/// A readable label for a web page: its host when the URL parses, else the URL.
pub(super) fn web_label(url: &str) -> String {
	url::Url::parse(url)
		.ok()
		.and_then(|url| url.host_str().map(str::to_owned))
		.unwrap_or_else(|| url.to_owned())
}

/// The stable session path a web tab uses. It is never written as a file or
/// watched; the session store keeps it only to identify the tab.
pub(crate) fn web_path(url: &str) -> PathBuf {
	let mut path = String::with_capacity(4 + url.len() * 2);
	path.push_str("web:");
	for byte in url.bytes() {
		use std::fmt::Write;
		let _ = write!(path, "{byte:02x}");
	}
	PathBuf::from(path)
}

impl<P: super::SendEvent> App<P> {
	pub(super) fn open_web_page(&mut self, url: String) {
		// A requested URL that redirected before is the same tab as its final
		// URL, so opening either must not start a duplicate.
		let url = self.web_pages.alias(&url).unwrap_or(url);
		let validated = crate::web_page::validate_url(&url);
		let url = validated.as_ref().map_or(url, ToString::to_string);
		let label = web_label(&url);
		let path = web_path(&url);
		if let Some(index) = self.readers.find_web(&path) {
			self.select_tab(index);
			if self.readers.entries()[index].page_title.is_none() {
				self.readers.set_title(index, label.clone());
			}
			if self.readers.session.load_error.is_none()
				&& self.readers.session.document.is_some()
			{
				if self.readers.session.layout_pending {
					self.request(false);
				}
				return;
			}
			if self.readers.session.web_loading {
				return;
			}
			self.readers.session.load_error = None;
			self.readers.session.web_loading = true;
			self.readers.session.layout_pending = true;
			self.watch = None;
			self.redraw();
		} else {
			self.open_pending(
				path.clone(),
				crate::security::Security::web(url.clone()),
			);
			self.readers.set_title(self.readers.active(), label);
		}
		let generation = self.web_pages.insert(url.clone(), path.clone());
		if let Err(error) = validated {
			self.web_page_loaded(url, path, generation, Err(error));
			return;
		}
		if self.args.offline {
			let message = self.preferences.values.lang().status_web_offline();
			self.web_page_loaded(
				url,
				path,
				generation,
				Err(anyhow::anyhow!(message)),
			);
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
				result = result => proxy.send(Event::WebLoaded { url, path, generation, result: Box::new(result) }),
			}
		}) {
			self.web_page_loaded(
				fallback_url,
				fallback_path,
				generation,
				Err(anyhow::anyhow!("Network service closed")),
			);
		}
	}

	pub(super) fn web_page_loaded(
		&mut self,
		url: String,
		path: PathBuf,
		generation: u64,
		result: anyhow::Result<crate::web_page::Article>,
	) {
		if !self.web_pages.matches(&url, &path, generation) {
			return;
		}
		let Some(index) = self.readers.find_web(&path) else {
			return;
		};
		let result = result.map(|article| {
			let input = article.article.input_fingerprint;
			let frontend = 100 + markview_webpage::PROFILE_VERSION;
			let loaded = LoadedDocument::semantic_with_versions(
				Arc::new(article.article.document),
				input,
				frontend,
			);
			self.readers
				.set_title(index, article.article.metadata.title.clone());
			let final_url = match &article.security.origin {
				crate::security::Origin::Web(url) => Some(url.clone()),
				_ => None,
			};
			{
				let session = self.readers.session_mut(index);
				session.discard_stale_disclosures(input, frontend);
				session.security = article.security;
				session.document = Some(loaded.clone());
				session.web_loading = false;
				session.content_version += 1;
				session.layout_pending = true;
			}
			self.retarget_web_tab(index, &url, final_url.as_deref(), &path);
			loaded
		});
		self.readers.session_mut(index).web_loading = false;
		match result {
			Ok(_) => {
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

	/// Keeps a redirected web tab identified by the URL it ended at.
	///
	/// The load rewrites the tab's origin to the final URL, so the session path
	/// and the load record follow it: opening that URL again then selects this
	/// tab instead of a duplicate, and a restored session computes the same
	/// path.
	fn retarget_web_tab(
		&mut self,
		index: usize,
		requested: &str,
		final_url: Option<&str>,
		path: &std::path::Path,
	) {
		let Some(final_url) = final_url else {
			return;
		};
		let retargeted = web_path(final_url);
		if retargeted.as_path() == path {
			return;
		}
		self.readers.rename(index, retargeted.clone());
		self.web_pages
			.rekey(requested, final_url.to_owned(), retargeted);
	}
}
