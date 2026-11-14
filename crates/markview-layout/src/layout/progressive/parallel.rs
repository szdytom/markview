//! Bounded speculation; only the coordinator publishes geometry or writes caches.
use super::{
	BlockJob, LayoutEngine, Measurement, ProgressiveLayout, anchor_only,
};
use crate::{
	document::{Block, BlockKind, Document, InlineKind, RichText},
	math::MathEngine,
	shaping::TextShaper,
};
use rayon::{ThreadPool, ThreadPoolBuilder, prelude::*};
use std::sync::Mutex;

pub(crate) struct Parallel {
	pool: Option<ThreadPool>,
	workers: Vec<Mutex<Option<Worker>>>,
}

struct Worker {
	shaper: TextShaper,
	math: MathEngine,
	style: Option<u64>,
}

impl Parallel {
	pub(crate) fn new(threads: usize) -> Self {
		Self {
			pool: None,
			workers: (0..threads).map(|_| Mutex::new(None)).collect(),
		}
	}

	fn start(&mut self) -> anyhow::Result<()> {
		if self.pool.is_none() {
			log::debug!("Starting {} block layout workers", self.threads());
			self.pool = Some(
				ThreadPoolBuilder::new()
					.num_threads(self.threads())
					.thread_name(|i| format!("markview-block-{i}"))
					.stack_size(8 * 1024 * 1024)
					.build()?,
			);
		}
		Ok(())
	}

	pub(crate) fn threads(&self) -> usize {
		self.workers.len()
	}
}

impl LayoutEngine {
	pub(in crate::layout) fn prefetch(
		&mut self,
		layout: &mut ProgressiveLayout,
		document: &Document,
		cancelled: &(dyn Fn() -> bool + Sync),
	) {
		if !self.prefetch_ready(layout) {
			return;
		}
		let Some(parallel) = &mut self.parallel else {
			return;
		};
		let mut jobs: Vec<BlockJob<'_>> = Vec::new();
		let mut visible_index = layout.visible_index;
		for block in layout.selected_blocks(document)[layout.index..]
			.iter()
			.take(parallel.threads() * 4)
		{
			if cancelled() || has_math(block) {
				break;
			}
			let job = layout.pass.job(
				block,
				visible_index,
				layout.visible_count,
				self.highlights.results(),
			);
			// Repeated content must reuse the first geometry, as in a serial pass.
			if self.cache.contains_key(&job.key)
				|| jobs.iter().any(|j| j.key == job.key)
			{
				break;
			}
			jobs.push(job);
			visible_index += usize::from(!anchor_only(block));
		}
		if jobs.len() < 2 {
			return;
		}
		for slot in &parallel.workers {
			let mut worker = slot.lock().unwrap();
			if worker.as_ref().is_none_or(|worker| {
				worker.style != Some(layout.pass.style_key)
			}) {
				*worker = Some(Worker {
					shaper: self.shaper.fork(),
					math: MathEngine::default(),
					style: Some(layout.pass.style_key),
				});
			}
		}
		if let Err(error) = parallel.start() {
			log::warn!(
				"Block workers unavailable: {error}; continuing serially"
			);
			self.parallel = None;
			return;
		}
		let pool = parallel.pool.as_ref().unwrap();
		let measurement = Measurement {
			pass: &layout.pass,
			nested: &self.nested_cache,
			highlights: self.highlights.results(),
			cancelled,
		};
		let geometries = pool.install(|| {
			jobs.par_iter()
				.map(|job| {
					let index = pool.current_thread_index().unwrap();
					let mut slot = parallel.workers[index].lock().unwrap();
					let Worker { shaper, math, .. } = slot.as_mut().unwrap();
					measurement.measure(job, shaper, math)
				})
				.collect::<Vec<_>>()
		});
		if !cancelled() {
			layout.pending.extend(
				jobs.into_iter()
					.zip(geometries)
					.map(|(job, geometry)| (job.key, geometry)),
			);
		}
	}
	fn prefetch_ready(&self, layout: &ProgressiveLayout) -> bool {
		layout.visible_index > 0
			&& layout.result.height >= layout.pass.parallel_start
			&& layout.pending.is_empty()
			&& self.parallel_ready.as_ref().is_none_or(|ready| ready())
	}
}

/// Formulas retain the coordinator's ordered cache and document-wide budget.
fn has_math(block: &Block) -> bool {
	let rich = |text: &RichText| {
		text.iter()
			.any(|inline| matches!(inline.kind, InlineKind::Math { .. }))
	};
	match &block.kind() {
		BlockKind::Paragraph(text) | BlockKind::Heading { text, .. } => {
			rich(text)
		}
		BlockKind::Quote { callout, blocks } => {
			callout
				.as_ref()
				.and_then(|callout| callout.title.as_ref())
				.is_some_and(rich)
				|| blocks.iter().any(has_math)
		}
		BlockKind::Footnote { blocks, .. }
		| BlockKind::FrontMatter { blocks, .. } => blocks.iter().any(has_math),
		BlockKind::Details {
			summary, blocks, ..
		} => rich(summary) || blocks.iter().any(has_math),
		BlockKind::List { items, .. } => {
			items.iter().any(|item| item.blocks.iter().any(has_math))
		}
		BlockKind::Table(table) => {
			table.caption.as_ref().is_some_and(rich)
				|| table.cell_blocks().any(has_math)
		}
		BlockKind::DefinitionList(items) => items.iter().any(|item| {
			item.terms.iter().any(rich)
				|| item
					.definitions
					.iter()
					.any(|blocks| blocks.iter().any(has_math))
		}),
		BlockKind::Figure { blocks, caption } => {
			caption.as_ref().is_some_and(rich) || blocks.iter().any(has_math)
		}
		BlockKind::Code { .. } | BlockKind::Rule => false,
	}
}

/// Use half the available logical CPUs, bounded for foreground responsiveness.
pub(crate) fn recommended_threads(cpus: usize) -> usize {
	(cpus / 2).clamp(1, 8)
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn automatic_workers_reserve_capacity_and_stay_lazy_before_the_viewport() {
		for (cpus, workers) in [
			(0, 1),
			(1, 1),
			(2, 1),
			(3, 1),
			(4, 2),
			(8, 4),
			(18, 8),
			(128, 8),
		] {
			assert_eq!(recommended_threads(cpus), workers);
		}
		let mut engine = LayoutEngine::new();
		engine.set_auto_parallelism();
		engine.set_parallel_start(600.);
		let document = markview_commonmark::parse(
			(0..100)
				.map(|i| {
					format!(
						"Paragraph {i}: some words about typesetting and reading.\n\n"
					)
				})
				.collect::<String>(),
		);
		assert!(
			engine
				.layout_progressive(
					&document,
					&Default::default(),
					&Default::default(),
					|prefix| prefix.height < 600.
				)
				.is_none()
		);
		if let Some(parallel) = &engine.parallel {
			assert!(parallel.pool.is_none());
			assert!(
				parallel
					.workers
					.iter()
					.all(|slot| slot.lock().unwrap().is_none())
			);
		}
		let ready =
			std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
		engine.set_parallel_start(0.);
		engine.set_parallel_ready(std::sync::Arc::new({
			let ready = ready.clone();
			move || ready.load(std::sync::atomic::Ordering::Relaxed)
		}));
		engine.layout(&document, &Default::default());
		if let Some(parallel) = &engine.parallel {
			assert!(parallel.pool.is_none());
		}
		ready.store(true, std::sync::atomic::Ordering::Relaxed);
		engine.clear_document_cache();
		engine.layout(&document, &Default::default());
		if let Some(parallel) = &engine.parallel {
			assert!(parallel.pool.is_some());
		}
	}

	#[test]
	fn callout_title_math_stays_on_the_coordinator() {
		let document = markview_commonmark::parse_with(
			"> [!note] $a$\n> Body.\n",
			markview_commonmark::Dialect {
				ofm: true,
				..Default::default()
			},
		);
		assert!(has_math(&document.blocks()[0]));
	}

	#[test]
	fn workers_share_existing_and_lazily_loaded_file_font_blobs() {
		use parley::fontique::{SourceId, SourceInfo, SourceKind};
		let source = || {
			SourceInfo::new(
				SourceId::new(),
				SourceKind::Path(std::sync::Arc::from(
					std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
						.join("tests/fonts/NotoSerif-Regular-subset.otf"),
				)),
			)
		};
		let mut coordinator = TextShaper::new();
		let existing = source();
		let loaded = coordinator
			.font_context()
			.source_cache
			.get(&existing)
			.unwrap();
		let mut a = coordinator.fork();
		let mut b = coordinator.fork();
		assert_eq!(
			a.font_context().source_cache.get(&existing).unwrap().id(),
			loaded.id()
		);
		let future = source();
		let lazy = a.font_context().source_cache.get(&future).unwrap();
		assert_eq!(
			b.font_context().source_cache.get(&future).unwrap().id(),
			lazy.id()
		);
		assert_eq!(
			coordinator
				.font_context()
				.source_cache
				.get(&future)
				.unwrap()
				.id(),
			lazy.id()
		);
	}
}
