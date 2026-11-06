//! Real layout geometry driven through selection, scroll, pan and overflow.
use crate::{coverage, oracle, pipeline};
use markview_interaction::{
	DocumentInteraction, Drag, Host, Modifiers, Motion, PanFeed, PanStatus,
	Point, Scroll, Selection,
};
use markview_layout::{
	layout::{LayoutEngine, LayoutOptions},
	scene::{LinkTarget, Viewport},
	text::TextSelection,
};
use std::collections::{BTreeMap, HashMap};
use web_time::Instant;

#[derive(Default)]
struct State {
	cursor: Point,
	modifiers: Modifiers,
	selection: Option<TextSelection>,
	drag: Option<Drag>,
	dragged: bool,
	click: Option<(Instant, Point, u8)>,
}

impl Host for State {
	fn cursor(&self) -> Point {
		self.cursor
	}
	fn modifiers(&self) -> Modifiers {
		self.modifiers
	}
	fn selection(&self) -> Option<TextSelection> {
		self.selection
	}
	fn set_selection(&mut self, value: Option<TextSelection>) {
		self.selection = value;
	}
	fn drag(&self) -> Option<&Drag> {
		self.drag.as_ref()
	}
	fn set_drag(&mut self, value: Option<Drag>) {
		self.drag = value;
	}
	fn take_drag(&mut self) -> Option<Drag> {
		self.drag.take()
	}
	fn dragged(&self) -> bool {
		self.dragged
	}
	fn set_dragged(&mut self, value: bool) {
		self.dragged = value;
	}
	fn last_click(&self) -> Option<(Instant, Point, u8)> {
		self.click
	}
	fn set_last_click(&mut self, value: Option<(Instant, Point, u8)>) {
		self.click = value;
	}
	fn blur(&mut self) {}
	fn release_pointer(&mut self) {
		self.drag = None;
	}
}

fn byte(controls: &[u8], at: usize) -> u8 {
	controls.get(at).copied().unwrap_or(0)
}

/// `INT1`, twelve controls, then Markdown. Raw inputs still run with controls
/// derived from their opening bytes.
pub fn check(data: &[u8]) {
	if data.len() > 32 * 1024 {
		return;
	}
	let (controls, md) = if data.starts_with(b"INT1") && data.len() >= 16 {
		(
			&data[4..16],
			String::from_utf8_lossy(&data[16..]).into_owned(),
		)
	} else {
		(
			&data[..data.len().min(12)],
			String::from_utf8_lossy(data).into_owned(),
		)
	};
	let document = markview_commonmark::parse(md);
	let base = LayoutOptions {
		width: 120. + f32::from(byte(controls, 0)) * 4.,
		fonts: pipeline::pinned_fonts(),
		..Default::default()
	};
	let mut engine = LayoutEngine::with_executor(
		std::sync::Arc::new(markview_layout::background::Direct),
		std::sync::Arc::new(|| {}),
	);
	let mut snapshot = engine.layout(&document, &base).flattened();
	let mut horizontal: HashMap<(usize, usize), f32> = HashMap::new();
	let mut scroll = Scroll::default();
	scroll.set_max((snapshot.height - 400.).max(0.));
	let mut state = State::default();
	let mut motion = Motion::new(Instant::now());
	let mut pan = PanFeed::default();
	let steps = 1 + usize::from(byte(controls, 1) % 12);
	for step in 0..steps {
		if step == steps / 2 && byte(controls, 2) % 3 == 0 {
			let mut options = base.clone();
			options.width = 80. + f32::from(byte(controls, 3)) * 6.;
			snapshot = engine.layout(&document, &options).flattened();
		}
		if byte(controls, 2) % 2 == 0 {
			let open: BTreeMap<_, _> = snapshot
				.blocks
				.iter()
				.enumerate()
				.map(|(index, block)| (block.id, (index + step) % 2 == 0))
				.collect();
			snapshot.set_disclosures(&open, false);
		}
		let viewport = Viewport {
			width: 80. + f32::from(byte(controls, 4)) * 6.,
			height: 200. + f32::from(byte(controls, 5)) * 6.,
			left: 0.,
			top: 0.,
			bottom: 0.,
			scroll: scroll.offset(),
		};
		let interaction = DocumentInteraction {
			snapshot: &snapshot,
			viewport,
			horizontal: &horizontal,
			revision: snapshot.revision.get(),
		};
		let point = Point::new(
			f32::from(byte(controls, 6 + step % 3)) / 255. * viewport.width,
			f32::from(byte(controls, 9 + step % 3)) / 255. * viewport.clip().h
				+ viewport.top,
		);
		state.cursor = point;
		state.modifiers = Modifiers {
			shift: byte(controls, 11) % 2 != 0,
			..Default::default()
		};
		if let Some(position) = interaction.position(point) {
			coverage::hit(coverage::Hit::InteractionHit);
			let now = Instant::now()
				+ std::time::Duration::from_millis(100 * step as u64);
			let count = state.click_count(now);
			assert!((1..=3).contains(&count));
			state.begin_selection(
				position,
				interaction
					.link_ref(point)
					.map(|link| LinkTarget::Url(link.url.into())),
			);
			let target = Point::new(
				point.x + f32::from(byte(controls, 7)) - 20.,
				point.y + f32::from(byte(controls, 8)) - 20.,
			);
			state.cursor = target;
			let moved = interaction.position(target);
			let before = state.selection;
			state.move_selection(moved, &snapshot);
			if state.selection != before {
				coverage::hit(coverage::Hit::InteractionSelection);
				let selection = state.selection.unwrap();
				let _ = snapshot.extract_text(selection, 1);
			}
			state.finish_selection(None);
		}
		if let Some((block, overflow)) = interaction.overflow_at(point) {
			let deltas = [point.x, point.y, 10., -10.];
			let delta = deltas[step % deltas.len()];
			let item = &snapshot.blocks[block].layout.overflow[overflow];
			let max = (item.content_width - item.rect.w).max(0.);
			assert!(markview_interaction::horizontal_by(
				&snapshot,
				viewport,
				&mut horizontal,
				point,
				delta,
			));
			let offset =
				horizontal.get(&(block, overflow)).copied().unwrap_or(0.);
			assert!(offset.is_finite() && (0. ..=max + 0.1).contains(&offset));
			coverage::hit(coverage::Hit::InteractionOverflow);
		}
		let delta = f32::from(byte(controls, 9)) * 8. - 1024.;
		let now =
			Instant::now() + std::time::Duration::from_millis(step as u64 * 20);
		scroll.wheel_by(delta, now);
		scroll.advance(now + std::time::Duration::from_millis(16));
		assert!(scroll.offset().is_finite() && scroll.offset() >= 0.);
		coverage::hit(coverage::Hit::InteractionScroll);
		let pan_status = match byte(controls, 10) % 4 {
			0 => PanStatus::Building,
			1 => PanStatus::Running,
			2 => PanStatus::Inertia,
			_ => PanStatus::Ready,
		};
		let phase = pan.status(pan_status);
		if let Some((x, y)) = pan.transform(delta, delta / 2.) {
			assert!(x.is_finite() && y.is_finite());
			coverage::hit(coverage::Hit::InteractionPan);
		}
		if phase == Some(markview_interaction::PanPhase::Ended) {
			pan.abandon();
		}
		motion.sample((delta / 4., delta / 8.), now);
		let _ = motion.release(now + std::time::Duration::from_millis(16));
		if let Some((x, y)) =
			motion.advance(now + std::time::Duration::from_millis(16))
		{
			assert!(x.is_finite() && y.is_finite());
		}
		oracle::assert_snapshot_finite(&snapshot);
		coverage::hit(coverage::Hit::InteractionStep);
	}
}
