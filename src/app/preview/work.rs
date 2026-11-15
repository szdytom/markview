//! One running hover computation and one cancellable waiter on bounded CPU capacity.
use super::*;
use std::sync::{Arc, Condvar, Mutex};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub(super) struct Destination {
	pub path: Option<std::path::PathBuf>,
	pub anchor: Option<markview_document::AnchorId>,
}

pub(super) struct Source {
	pub version: u64,
	pub target: Target,
	pub anchor: Rect,
	pub bounds: Rect,
	pub revision: markview_document::DocumentRevision,
	pub frame: FrameStyle,
	pub document: Arc<crate::loaded::LoadedDocument>,
	pub destination: Destination,
	pub stamp: crate::watch::FileStamp,
	pub original_options: crate::layout::LayoutOptions,
	pub options: crate::layout::LayoutOptions,
	pub images: markview_layout::image::ImageSnapshot,
}
impl Source {
	pub fn matches<P: super::super::SendEvent>(&self, app: &App<P>) -> bool {
		let Some(document) = &app.readers.session.document else {
			return false;
		};
		let context = app.document_interaction();
		let images = &context.snapshot.images;
		self.destination.path.as_ref().is_none_or(|path| {
			app.readers.session.security.check_file(path).is_ok()
		}) && self.version == app.readers.session.version
			&& Arc::ptr_eq(document, &self.document)
			&& context.snapshot.revision == self.revision
			&& app.options() == self.original_options
			&& context.viewport.clip() == self.bounds
			&& context.link_rect(markview_interaction::Point::new(
				self.anchor.x + self.anchor.w / 2.0,
				self.anchor.y + self.anchor.h / 2.0,
			)) == Some(self.anchor)
			&& images.generation == self.images.generation
			&& images.entries == self.images.entries
			&& images.keys == self.images.keys
	}
	fn layout(
		&self,
		cancelled: &CancellationToken,
	) -> Option<(LayoutSnapshot, navigation::Origins)> {
		let loaded;
		let document = if let Some(path) = &self.destination.path {
			let source = crate::file::read_markdown(path).ok()?;
			if cancelled.is_cancelled() {
				return None;
			}
			loaded = markview_commonmark::parse_with(
				source,
				markview_commonmark::Dialect {
					hard_breaks: self.options.hard_breaks,
					highlight_marks: self.options.highlight_marks,
					ofm: self.options.ofm,
					grid_tables: self.options.grid_tables,
				},
			);
			&loaded.document
		} else {
			self.document.document()
		};
		let node = match &self.destination.anchor {
			Some(anchor) => document.anchor_block(anchor)?,
			None => document.blocks().first()?,
		};
		let images = if self.destination.path.is_some() {
			&markview_layout::image::ImageSnapshot::default()
		} else {
			&self.images
		};
		let snapshot = super::layout_scene_cancellable(
			document,
			node,
			&self.options,
			images,
			self.destination.anchor.is_none()
				|| matches!(
					node.kind(),
					markview_document::BlockKind::Heading { .. }
				),
			&|| cancelled.is_cancelled(),
		)?;
		self.destination
			.path
			.as_ref()
			.is_none_or(|path| crate::watch::file_stamp(path) == self.stamp)
			.then(|| {
				let origins = if self.destination.path.is_some() {
					navigation::Origins::new(document, &snapshot)
				} else {
					navigation::Origins::default()
				};
				(snapshot, origins)
			})
	}
}

pub(super) struct Completion {
	pub serial: u64,
	pub snapshot: Option<LayoutSnapshot>,
	pub origins: navigation::Origins,
	pub build: Duration,
}

pub(super) struct Worker {
	gate: Arc<Semaphore>,
	cancel: Option<CancellationToken>,
	ready: Arc<(Mutex<Option<Completion>>, Condvar)>,
}
impl Default for Worker {
	fn default() -> Self {
		Self {
			gate: Arc::new(Semaphore::new(1)),
			cancel: None,
			ready: Arc::new((Mutex::new(None), Condvar::new())),
		}
	}
}
impl Worker {
	pub fn cancel(&mut self) {
		if let Some(cancel) = self.cancel.take() {
			cancel.cancel();
		}
		*self.ready.0.lock().unwrap() = None;
	}
	#[cfg(test)]
	pub fn token(&self) -> Option<CancellationToken> {
		self.cancel.clone()
	}
	pub fn try_recv(&self) -> Option<Completion> {
		self.ready.0.lock().unwrap().take()
	}
	#[cfg(test)]
	pub fn wait(&self, timeout: Duration) -> Option<Completion> {
		let slot = self.ready.0.lock().unwrap();
		let (mut slot, _) = self
			.ready
			.1
			.wait_timeout_while(slot, timeout, |slot| slot.is_none())
			.unwrap();
		slot.take()
	}
	pub fn submit(
		&mut self,
		services: &crate::services::Handle,
		serial: u64,
		source: Arc<Source>,
		wake: impl FnOnce() + Send + 'static,
	) {
		self.cancel();
		let cancel = services.cancel.child_token();
		self.cancel = Some(cancel.clone());
		let gate = self.gate.clone();
		let ready = self.ready.clone();
		let handle = services.clone();
		services.submit(async move {
			let _permit = tokio::select! {
				biased;
				_ = cancel.cancelled() => return,
				permit = gate.acquire_owned() => permit.expect("hover gate stays open"),
			};
			let stopped = cancel.clone();
			// The document is shared; queuing does not copy its bytes.
			let result = handle
				.compute(std::mem::size_of::<Source>(), &cancel, move || {
					let start = Instant::now();
					let snapshot = source.layout(&stopped);
					Ok((snapshot, start.elapsed()))
				})
				.await;
			let mut slot = ready.0.lock().unwrap();
			if !cancel.is_cancelled() {
				let (snapshot, build) =
					result.unwrap_or((None, Duration::ZERO));
				let (snapshot, origins) = snapshot.map_or_else(
					|| (None, navigation::Origins::default()),
					|(snapshot, origins)| (Some(snapshot), origins),
				);
				*slot = Some(Completion {
					serial,
					snapshot,
					origins,
					build,
				});
				drop(slot);
				ready.1.notify_one();
				wake();
			}
		});
	}
}
impl Drop for Worker {
	fn drop(&mut self) {
		self.cancel();
	}
}
