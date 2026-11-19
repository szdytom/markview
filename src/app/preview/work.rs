//! Cancellable hover layout and image updates on bounded CPU capacity.
use super::*;
use std::sync::{Arc, Condvar, Mutex};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

#[cfg(test)]
mod tests;

#[derive(Clone, Debug)]
pub(in crate::app) struct Destination {
	pub path: Option<std::path::PathBuf>,
	pub anchor: Option<markview_document::AnchorId>,
}

/// Content and layout shared by hover cards and the file picker.
pub(in crate::app) struct Content {
	pub security: crate::security::Security,
	pub workspace: Option<std::path::PathBuf>,
	pub offline: bool,
	pub document: Option<Arc<crate::loaded::LoadedDocument>>,
	pub destination: Destination,
	pub stamp: crate::watch::FileStamp,
	pub options: crate::layout::LayoutOptions,
	pub images: markview_layout::image::ImageSnapshot,
	/// Hover cards fit the content; picker previews keep their reading width.
	pub fit: bool,
}

pub(super) struct Source {
	pub size: PreviewSize,
	pub version: u64,
	pub security: crate::security::Security,
	pub workspace: Option<std::path::PathBuf>,
	pub offline: bool,
	pub target: Target,
	pub anchor: Rect,
	pub bounds: Rect,
	pub revision: markview_document::DocumentRevision,
	pub frame: FrameStyle,
	pub document: Option<Arc<crate::loaded::LoadedDocument>>,
	pub destination: Destination,
	pub stamp: crate::watch::FileStamp,
	pub original_options: crate::layout::LayoutOptions,
	pub options: crate::layout::LayoutOptions,
	pub images: markview_layout::image::ImageSnapshot,
}
impl Source {
	pub fn matches<P: super::super::SendEvent>(&self, app: &App<P>) -> bool {
		let same_document =
			match (&app.readers.session.document, &self.document) {
				(Some(current), Some(original)) => {
					Arc::ptr_eq(current, original)
				}
				(None, None) => true,
				_ => false,
			};

		let context = app.document_interaction();
		let images = &context.snapshot.images;
		self.matches_destination(app)
			&& self.target.preview_kind().is_some_and(|kind| {
				app.preferences.preview_size(kind) == self.size
			})
			&& self.version == app.readers.session.version
			&& same_document
			&& app.options() == self.original_options
			&& app.scene_preview_bounds(self.target) == self.bounds
			&& self.matches_anchor(app)
			&& images.generation == self.images.generation
			&& images.entries == self.images.entries
			&& images.keys == self.images.keys
	}

	pub(super) fn matches_anchor<P: super::super::SendEvent>(
		&self,
		app: &App<P>,
	) -> bool {
		app.scene_preview_matches_anchor(
			self.target,
			self.anchor,
			self.revision,
		)
	}

	pub(super) fn matches_destination<P: super::super::SendEvent>(
		&self,
		app: &App<P>,
	) -> bool {
		let same_file = match self.target {
			Target::File(id) => {
				app.folder_preview_path(id) == self.destination.path.as_deref()
			}
			_ => true,
		};
		same_file
			&& self.destination.path.as_ref().is_none_or(|path| {
				app.preview_security(self.target).check_file(path).is_ok()
			})
			&& self.security == app.preview_security(self.target)
			&& self.workspace == app.folder.root().cloned()
	}
}

struct Scene {
	source: Content,
	document: Arc<crate::loaded::LoadedDocument>,
	images: Option<crate::images::Images>,
	blocks: Vec<markview_document::BlockId>,
	picture: bool,
}
impl Scene {
	fn new(source: Content, cancelled: &CancellationToken) -> Option<Self> {
		let document = if let Some(path) = &source.destination.path {
			if !source.fit
				&& !crate::app::folder::markdown(path)
				&& !crate::file::is_picture(path)
			{
				return None;
			}
			let text = crate::file::read_markdown(path).ok()?;
			if cancelled.is_cancelled() {
				return None;
			}
			crate::loaded::LoadedDocument::markdown(Arc::new(
				markview_commonmark::parse_with(
					text,
					markview_commonmark::Dialect {
						hard_breaks: source.options.hard_breaks,
						highlight_marks: source.options.highlight_marks,
						ofm: source.options.ofm,
						grid_tables: source.options.grid_tables,
						math_dollars_block: source.options.math_dollars_block,
					},
				),
			))
		} else {
			source.document.clone()?
		};
		Some(Self {
			source,
			document,
			images: None,
			blocks: Vec::new(),
			picture: false,
		})
	}

	fn layout(
		&self,
		cancelled: &(dyn Fn() -> bool + Sync),
		mut progress: impl FnMut(&LayoutSnapshot),
	) -> Option<(LayoutSnapshot, navigation::Origins)> {
		let source = &self.source;
		let document = self.document.document();
		let empty = markview_layout::image::ImageSnapshot::default();
		let images = self.images.as_ref().map_or_else(
			|| {
				if source.destination.path.is_some() {
					&empty
				} else {
					&source.images
				}
			},
			|images| &images.snapshot,
		);
		let snapshot = if source.fit {
			let node = match &source.destination.anchor {
				Some(anchor) => document.anchor_block(anchor)?,
				None => document.blocks().first()?,
			};
			super::layout_scene_cancellable(
				document,
				node,
				&source.options,
				images,
				source.destination.anchor.is_none()
					|| matches!(
						node.kind(),
						markview_document::BlockKind::Heading { .. }
					),
				cancelled,
			)?
		} else {
			crate::layout::LayoutEngine::new().layout_progressive_cancellable(
				document,
				&source.options,
				images,
				|snapshot| {
					progress(snapshot);
					true
				},
				cancelled,
			)?
		};
		#[cfg(test)]
		tests::LAYOUT_BLOCKS
			.set(tests::LAYOUT_BLOCKS.get() + snapshot.blocks.len());
		source
			.destination
			.path
			.as_ref()
			.is_none_or(|path| crate::watch::file_stamp(path) == source.stamp)
			.then(|| {
				let origins = if source.destination.path.is_some() {
					navigation::Origins::new(document, &snapshot)
				} else {
					navigation::Origins::default()
				};
				(snapshot, origins)
			})
	}

	fn refresh(
		&mut self,
		services: crate::services::Handle,
		wake: markview_layout::background::Wake,
		cancelled: &CancellationToken,
		progress: impl FnMut(&LayoutSnapshot),
	) -> Option<(LayoutSnapshot, navigation::Origins)> {
		let snapshot = self.layout(&|| cancelled.is_cancelled(), progress)?;
		let Some(path) = &self.source.destination.path else {
			return Some(snapshot);
		};
		let blocks: Vec<_> =
			snapshot.0.blocks.iter().map(|block| block.id).collect();
		if blocks == self.blocks {
			return Some(snapshot);
		}
		self.blocks = blocks;
		if self.images.is_none() {
			self.picture = crate::file::is_picture(path);
		}
		let images = self.images.get_or_insert_with(|| {
			let mut images = crate::images::Images::with_services(
				self.source.offline,
				crate::images::cache_directory(),
				self.source.options.fonts.clone(),
				services,
				None,
				wake,
			);
			images.set_security(self.document.source().map_or_else(
				|| self.source.security.clone(),
				|text| self.source.security.for_content(text),
			));
			images.set_workspace(self.source.workspace.clone());
			images.set_standalone_image(self.picture.then(|| path.clone()));
			images.set_initial_width(self.source.options.width);
			images
		});
		images.prepare_blocks(
			self.blocks
				.iter()
				.map(|id| self.document.document().block(*id).unwrap()),
			path,
			1,
			false,
			&self.source.options.stylesheet,
			&self.source.options.fonts,
		);
		if images.snapshot.entries.is_empty()
			&& snapshot.0.images.entries.is_empty()
		{
			return Some(snapshot);
		}
		self.layout(&|| cancelled.is_cancelled(), |_| {})
	}
}

pub(in crate::app) struct Completion {
	pub serial: u64,
	pub partial: bool,
	pub snapshot: Option<LayoutSnapshot>,
	pub(in crate::app) origins: navigation::Origins,
	pub build: Duration,
	pub picture: bool,
}

pub(in crate::app) struct Worker {
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
	#[cfg(test)]
	pub(in crate::app) fn wait_for_final_publication(&self) {
		let slot = self.ready.0.lock().unwrap();
		let (slot, _) = self
			.ready
			.1
			.wait_timeout_while(slot, Duration::from_secs(10), |slot| {
				slot.as_ref().is_none_or(|result| result.partial)
			})
			.unwrap();
		assert!(
			slot.as_ref().is_some_and(
				|result| !result.partial && result.snapshot.is_some()
			)
		);
	}
	pub(super) fn submit(
		&mut self,
		services: &crate::services::Handle,
		serial: u64,
		source: Arc<Source>,
		wake: impl Fn() + Clone + Send + 'static,
	) {
		self.submit_content(
			services,
			serial,
			Content {
				security: source.security.clone(),
				workspace: source.workspace.clone(),
				offline: source.offline,
				document: source.document.clone(),
				destination: source.destination.clone(),
				stamp: source.stamp,
				options: source.options.clone(),
				images: source.images.clone(),
				fit: true,
			},
			wake,
		);
	}
	pub fn submit_content(
		&mut self,
		services: &crate::services::Handle,
		serial: u64,
		source: Content,
		wake: impl Fn() + Clone + Send + 'static,
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
			let changed = Arc::new(tokio::sync::Notify::new());
			let image_wake: markview_layout::background::Wake = Arc::new({
				let changed = changed.clone();
				move || changed.notify_one()
			});
			let stopped = cancel.clone();
			let io = handle.clone();
			let first_wake = image_wake.clone();
			let early = ready.clone();
			let early_wake = wake.clone();
			let initial = handle
				.compute(std::mem::size_of::<Source>(), &cancel, move || {
					let start = Instant::now();
					let Some(mut scene) = Scene::new(source, &stopped) else {
						return Ok(None);
					};
					let document = scene.document.clone();
					let height = scene.source.options.viewport_height;
					let mut next = 1;
					let progress = |page: &LayoutSnapshot| {
						if page.height < height || page.blocks.len() < next {
							return;
						}
						next = page.blocks.len().saturating_mul(2);
						let result = Completion {
							serial,
							partial: true,
							snapshot: Some(page.clone()),
							origins: navigation::Origins::new(
								document.document(),
								page,
							),
							build: start.elapsed(),
							picture: false,
						};
						let mut slot = early.0.lock().unwrap();
						if stopped.is_cancelled() {
							return;
						}
						*slot = Some(result);
						drop(slot);
						early.1.notify_one();
						early_wake();
					};
					let Some(snapshot) =
						scene.refresh(io, first_wake, &stopped, progress)
					else {
						return Ok(None);
					};
					Ok(Some((scene, snapshot, start.elapsed())))
				})
				.await
				.ok()
				.flatten();
			let (mut scene, mut result, mut build) = initial.map_or_else(
				|| (None, None, Duration::ZERO),
				|(scene, snapshot, build)| (Some(scene), Some(snapshot), build),
			);
			loop {
				if cancel.is_cancelled() {
					return;
				}
				if let Some(snapshot) = result.take() {
					let mut slot = ready.0.lock().unwrap();
					if cancel.is_cancelled() {
						return;
					}
					*slot = Some(Completion {
						serial,
						partial: false,
						snapshot: Some(snapshot.0),
						origins: snapshot.1,
						build,
						picture: scene.as_ref().unwrap().picture,
					});
					drop(slot);
					ready.1.notify_one();
					wake();
				} else if scene.is_none() {
					let mut slot = ready.0.lock().unwrap();
					if cancel.is_cancelled() {
						return;
					}
					*slot = Some(Completion {
						serial,
						partial: false,
						snapshot: None,
						origins: navigation::Origins::default(),
						build,
						picture: false,
					});
					drop(slot);
					ready.1.notify_one();
					wake();
					return;
				}
				let Some(current) = scene.take().filter(|scene| {
					scene.images.as_ref().is_some_and(|images| {
						!images.snapshot.entries.is_empty()
					})
				}) else {
					return;
				};
				let deadline = current.images.as_ref().unwrap().poll_deadline();
				tokio::select! {
					biased;
					_ = cancel.cancelled() => return,
					_ = changed.notified() => {},
					_ = tokio::time::sleep_until(deadline.into()) => {},
				}
				let stopped = cancel.clone();
				let io = handle.clone();
				let image_wake = image_wake.clone();
				let update = handle
					.compute(std::mem::size_of::<Scene>(), &cancel, move || {
						let start = Instant::now();
						let mut current = current;
						let snapshot = current
							.images
							.as_mut()
							.unwrap()
							.poll()
							.then(|| {
								current.refresh(
									io,
									image_wake,
									&stopped,
									|_| {},
								)
							})
							.flatten();
						Ok((current, snapshot, start.elapsed()))
					})
					.await;
				let Ok((current, snapshot, elapsed)) = update else {
					return;
				};
				scene = Some(current);
				result = snapshot;
				build = elapsed;
			}
		});
	}
}
impl Drop for Worker {
	fn drop(&mut self) {
		self.cancel();
	}
}
