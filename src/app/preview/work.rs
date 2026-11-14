//! One running hover computation and one cancellable waiter on bounded CPU capacity.
use super::*;
use std::sync::{Arc, Condvar, Mutex};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

pub(super) struct Source {
	pub version: u64,
	pub target: Target,
	pub anchor: Rect,
	pub bounds: Rect,
	pub revision: markview_document::DocumentRevision,
	pub frame: FrameStyle,
	pub document: Arc<crate::loaded::LoadedDocument>,
	pub definition: markview_document::FootnoteId,
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
		self.version == app.readers.session.version
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
	fn layout(&self, cancelled: &CancellationToken) -> Option<LayoutSnapshot> {
		let node = self.document.blocks().iter()
			.take_while(|_| !cancelled.is_cancelled())
			.find(|block| {
				matches!(block.kind(), markview_document::BlockKind::Footnote { id, .. } if *id == self.definition)
			})?;
		super::layout_scene_cancellable(
			&self.document,
			node,
			&self.options,
			&self.images,
			&|| cancelled.is_cancelled(),
		)
	}
}

pub(super) struct Completion {
	pub serial: u64,
	pub snapshot: Option<LayoutSnapshot>,
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
				*slot = Some(Completion {
					serial,
					snapshot,
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
