use super::*;
use crate::{Horizontal, document::proptests::fixture};
use markview_layout::text::{TextCluster, TextNode};
use proptest::prelude::*;
use std::sync::Arc;

fn snapshot() -> LayoutSnapshot {
	let mut snapshot = fixture(0.0, 160.0);
	snapshot.revision = markview_layout::document::DocumentRevision::new(1);
	snapshot.width = 200.0;
	snapshot.height = 1000.0;
	let layout = Arc::make_mut(&mut snapshot.blocks[0].layout);
	layout.overflow = Default::default();
	let mut node = TextNode::new("hello world".into(), "\n\n");
	node.search_field = Some(markview_layout::document::FieldId::from_index(0));
	node.search_ranges = vec![(0..11, 0..11)].into();
	for index in 0..11 {
		node.push(TextCluster {
			mixed_spacing: (0.0, 0.0),
			range: index..index + 1,
			rect: Rect {
				x: index as f32 * 10.0,
				y: 5.0,
				w: 10.0,
				h: 10.0,
			},
			rtl: false,
			atomic: false,
			command: 0,
		});
	}
	layout.text = vec![node].into();
	snapshot
}
fn bounds() -> Rect {
	Rect {
		x: 100.0,
		y: 200.0,
		w: 200.0,
		h: 100.0,
	}
}
fn press(
	state: &mut ViewState,
	view: &DocumentView<'_>,
	point: Point,
) -> ViewUpdate {
	state.handle(
		view,
		ViewInput::Press {
			point,
			modifiers: Modifiers::default(),
			at: Instant::now(),
		},
		ScrollbarMetrics::default(),
	)
}
fn event(
	state: &mut ViewState,
	view: &DocumentView<'_>,
	input: ViewInput,
) -> ViewUpdate {
	state.handle(view, input, ScrollbarMetrics::default())
}

#[test]
fn selections_and_copy_belong_to_one_surface() {
	let snapshot = snapshot();
	let horizontal = Horizontal::new();
	let view =
		DocumentView::embedded(&snapshot, bounds(), 1.0, 0.0, &horizontal, 1);
	let mut first = ViewState::default();
	let second = ViewState::default();
	press(&mut first, &view, Point::new(101.0, 210.0));
	event(&mut first, &view, ViewInput::Move(Point::new(149.0, 210.0)));
	event(
		&mut first,
		&view,
		ViewInput::Release(Point::new(149.0, 210.0)),
	);
	assert_eq!(first.selected_text(&view).as_deref(), Some("hello"));
	assert!(second.selected_text(&view).is_none());
	assert!(!first.captured());
	event(&mut first, &view, ViewInput::SelectAll);
	assert_eq!(first.selected_text(&view).as_deref(), Some("hello world"));
	let mut changed = snapshot.clone();
	changed.revision = markview_layout::document::DocumentRevision::new(2);
	let stale =
		DocumentView::embedded(&changed, bounds(), 1.0, 0.0, &horizontal, 2);
	assert!(first.selected_text(&stale).is_none());
}

#[test]
fn links_activate_on_matching_click_and_cancel_after_a_drag() {
	let snapshot = snapshot();
	let horizontal = Horizontal::new();
	let view =
		DocumentView::embedded(&snapshot, bounds(), 1.0, 0.0, &horizontal, 1);
	let point = Point::new(130.0, 210.0);
	let mut state = ViewState::default();
	press(&mut state, &view, point);
	assert!(matches!(
		event(&mut state, &view, ViewInput::Release(point)).action,
		Some(ViewAction::Link(LinkTarget::Url(_)))
	));
	state = ViewState::default();
	press(&mut state, &view, point);
	event(&mut state, &view, ViewInput::Move(Point::new(140.0, 210.0)));
	assert!(
		event(&mut state, &view, ViewInput::Release(point))
			.action
			.is_none()
	);
	assert!(!state.captured());
	assert!(!event(&mut state, &view, ViewInput::Release(point)).consumed);
}

#[test]
fn image_clicks_are_actions_and_moving_cancels_them() {
	let snapshot = snapshot();
	let horizontal = Horizontal::new();
	let view =
		DocumentView::embedded(&snapshot, bounds(), 1.0, 0.0, &horizontal, 1);
	let point = Point::new(180.0, 220.0);
	let mut state = ViewState::default();
	press(&mut state, &view, point);
	assert!(
		matches!(event(&mut state, &view, ViewInput::Release(point)).action, Some(ViewAction::Image { source, .. }) if source == "image")
	);
	press(&mut state, &view, point);
	event(&mut state, &view, ViewInput::Move(Point::new(190.0, 220.0)));
	assert!(state.captured());
	assert!(
		event(&mut state, &view, ViewInput::Release(point))
			.action
			.is_none()
	);
	assert!(!state.captured());
}

#[test]
fn scrollbar_drag_captures_outside_and_never_selects_text() {
	let snapshot = snapshot();
	let horizontal = Horizontal::new();
	let view =
		DocumentView::embedded(&snapshot, bounds(), 1.0, 0.0, &horizontal, 1);
	let mut state = ViewState::default();
	press(&mut state, &view, Point::new(296.0, 210.0));
	assert!(state.scrollbar_captured());
	assert!(state.selection.is_none());
	assert!(matches!(
		event(&mut state, &view, ViewInput::Move(Point::new(500.0, 500.0)))
			.action,
		Some(ViewAction::ScrollTo(900.0))
	));
	assert!(
		event(
			&mut state,
			&view,
			ViewInput::Release(Point::new(500.0, 500.0))
		)
		.consumed
	);
	assert!(!state.captured());
}

#[test]
fn horizontal_scrollbars_use_the_same_capture_machine() {
	let snapshot = fixture(0.0, 160.0);
	let horizontal = Horizontal::new();
	let view =
		DocumentView::embedded(&snapshot, bounds(), 1.0, 0.0, &horizontal, 1);
	let mut state = ViewState::default();
	press(&mut state, &view, Point::new(102.0, 244.0));
	assert!(state.scrollbar_captured());
	assert!(matches!(
		event(&mut state, &view, ViewInput::Move(Point::new(400.0, 244.0)))
			.action,
		Some(ViewAction::HorizontalTo {
			block: 0,
			overflow: 0,
			offset: 96.0
		})
	));
	event(&mut state, &view, ViewInput::Cancel);
	assert!(!state.captured());
	assert!(!state.focused);
}

#[test]
fn dragging_selection_outside_autoscrolls_and_release_stops_it() {
	let snapshot = snapshot();
	let horizontal = Horizontal::new();
	let view =
		DocumentView::embedded(&snapshot, bounds(), 1.0, 0.0, &horizontal, 1);
	let mut state = ViewState::default();
	press(&mut state, &view, Point::new(101.0, 210.0));
	event(&mut state, &view, ViewInput::Move(Point::new(120.0, 340.0)));
	assert!(state.auto_scroll(&view, 0.1).unwrap() > 0.0);
	event(
		&mut state,
		&view,
		ViewInput::Release(Point::new(120.0, 340.0)),
	);
	assert!(state.auto_scroll(&view, 0.1).is_none());
}

proptest! {
	#[test]
	fn translated_scaled_surfaces_hit_the_same_document(
		x in -1000i16..1000, y in -1000i16..1000, scale in 1u8..8,
		scroll in 0u16..100, probe_x in 0u16..180, probe_y in 0u16..80,
	) {
		let snapshot = snapshot();
		let horizontal = Horizontal::new();
		let scale = f32::from(scale) / 2.0;
		let rect = Rect { x: f32::from(x), y: f32::from(y), w: 200.0 * scale, h: 100.0 * scale };
		let view = DocumentView::embedded(&snapshot, rect, scale, f32::from(scroll), &horizontal, 1);
		let local = Point::new(f32::from(probe_x) + 0.5, f32::from(probe_y) + 0.5);
		let point = Point::new(rect.x + local.x * scale, rect.y + local.y * scale);
		prop_assert_eq!(view.local(point), local);
		prop_assert_eq!(view.link(point), view.context.link_ref(local).map(|l| l.target.clone()));
		prop_assert_eq!(view.image(point).map(|i| i.0), view.context.image(local).map(|i| i.0.to_owned()));
		prop_assert_eq!(view.contains(point), view.context.contains(local));
	}
	#[test]
	fn wheel_clamps_and_consumes_at_both_limits(scroll in 0u16..901, delta in -2000i16..2000) {
		let snapshot = snapshot();
		let horizontal = Horizontal::new();
		let view = DocumentView::embedded(&snapshot, bounds(), 1.0, f32::from(scroll), &horizontal, 1);
		let mut state = ViewState::default();
		let update = event(&mut state, &view, ViewInput::Wheel { point: Point::new(110.0, 210.0), delta: f32::from(delta) });
		prop_assert!(update.consumed);
		prop_assert!(matches!(update.action, Some(ViewAction::ScrollTo(offset)) if offset == (f32::from(scroll) + f32::from(delta)).clamp(0.0, 900.0)));
		prop_assert!(!event(&mut state, &view, ViewInput::Wheel { point: Point::new(50.0, 50.0), delta: f32::from(delta) }).consumed, "outside wheel input must pass through");
	}
}

#[test]
fn external_scrollbar_tracks_capture_in_the_padding_and_use_content_extents() {
	let snapshot = snapshot();
	let horizontal = Horizontal::new();
	let track = Rect {
		x: 310.0,
		y: 190.0,
		w: 8.0,
		h: 120.0,
	};
	let view =
		DocumentView::embedded(&snapshot, bounds(), 1.0, 0.0, &horizontal, 1)
			.with_scrollbar_track(track);
	let bar = view.scrollbar(ScrollbarMetrics::default()).unwrap();
	assert_eq!(bar.track, track);
	assert_eq!(bar.max_scroll(), 900.0);
	let mut state = ViewState::default();
	let point = Point::new(314.0, 191.0);
	assert!(view.contains(point));
	assert!(press(&mut state, &view, point).consumed);
	assert!(state.scrollbar_captured());
	assert!(matches!(
		event(&mut state, &view, ViewInput::Move(Point::new(314.0, 310.0)))
			.action,
		Some(ViewAction::ScrollTo(900.0))
	));
	assert!(state.selection.is_none());
}
