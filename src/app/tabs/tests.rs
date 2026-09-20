use super::Tabs;
use std::{
	path::PathBuf,
	sync::Arc,
	time::{Duration, Instant},
};

#[test]
fn switching_restores_sessions_and_requests_remain_globally_ordered() {
	let mut tabs = Tabs::default();
	let now = Instant::now();
	tabs.open(PathBuf::from("a.md"), now);
	let first = tabs.request(crate::test_support::options(), true).unwrap();
	tabs.session.scroll = 123.0;
	tabs.open(PathBuf::from("b.md"), now);
	let second = tabs.request(crate::test_support::options(), false).unwrap();
	assert!(second.version > first.version);
	assert!(tabs.select(0, now));
	assert_eq!(tabs.session.scroll, 123.0);
	assert!(tabs.session.follow_update);
	let third = tabs.request(crate::test_support::options(), false).unwrap();
	assert!(third.version > second.version);
	assert_eq!(third.content_version, first.content_version);
}

#[test]
fn lifting_the_image_cap_is_per_tab_and_per_revision() {
	let mut tabs = Tabs::default();
	let now = Instant::now();
	tabs.open(PathBuf::from("a.md"), now);
	assert!(
		!tabs
			.request(crate::test_support::options(), false)
			.unwrap()
			.load_all_images
	);
	// The reader lifts the cap while reading `a.md`.
	tabs.session.load_all_images = true;
	assert!(
		tabs.request(crate::test_support::options(), false)
			.unwrap()
			.load_all_images
	);
	// A newly opened document starts capped, whatever `a.md` asked for.
	tabs.open(PathBuf::from("b.md"), now);
	assert!(
		!tabs
			.request(crate::test_support::options(), false)
			.unwrap()
			.load_all_images
	);
	// Switching back keeps each tab's own answer.
	assert!(tabs.select(0, now));
	assert!(
		tabs.request(crate::test_support::options(), false)
			.unwrap()
			.load_all_images
	);
	// New content in `a.md` asks again.
	tabs.session.content_version += 1;
	tabs.session.load_all_images = false;
	assert!(
		!tabs
			.request(crate::test_support::options(), false)
			.unwrap()
			.load_all_images
	);
}

#[test]
fn closing_tabs_preserves_active_state_and_releases_only_inactive_documents() {
	let mut tabs = Tabs::default();
	let now = Instant::now();
	tabs.open(PathBuf::from("a.md"), now);
	tabs.session.document = Some(Arc::new(crate::document::parse("a")));
	tabs.open(PathBuf::from("b.md"), now);
	tabs.session.document = Some(Arc::new(crate::document::parse("b")));
	tabs.release_inactive(now + Duration::from_secs(21));
	assert!(tabs.entries()[0].session.document.is_none());
	assert!(tabs.session.document.is_some());
	assert!(matches!(tabs.close(0, now), super::Closed::Inactive));
	assert_eq!(tabs.active(), 0);
	assert_eq!(tabs.session.path, Some(PathBuf::from("b.md")));
	assert!(matches!(tabs.close(0, now), super::Closed::Active));
	assert!(tabs.session.path.is_none());
	assert!(
		tabs.request(crate::test_support::options(), false)
			.is_none()
	);
}

#[test]
fn closing_active_tab_restores_neighbor_and_invalid_indices_preserve_state() {
	let mut tabs = Tabs::default();
	let now = Instant::now();
	for (name, scroll) in [("a.md", 10.0), ("b.md", 20.0), ("c.md", 30.0)] {
		tabs.open(PathBuf::from(name), now);
		tabs.session.scroll = scroll;
	}
	assert!(tabs.select(1, now));
	assert!(matches!(tabs.close(1, now), super::Closed::Active));
	assert_eq!(tabs.session.path, Some(PathBuf::from("c.md")));
	assert_eq!(tabs.session.scroll, 30.0);
	assert!(tabs.select(0, now));
	assert_eq!(tabs.session.scroll, 10.0);
	assert!(!tabs.select(9, now));
	assert!(matches!(tabs.close(9, now), super::Closed::Missing));
}

#[test]
fn reordering_preserves_every_session_and_the_active_request() {
	for active in 0..4 {
		for from in 0..4 {
			for to in 0..4 {
				let mut tabs = Tabs::default();
				let now = Instant::now();
				for i in 0..4 {
					tabs.open(PathBuf::from(format!("{i}.md")), now);
					tabs.session.scroll = 10.0 * i as f32;
				}
				tabs.select(active, now);
				let request = tabs
					.request(crate::test_support::options(), false)
					.unwrap();
				assert_eq!(tabs.move_tab(from, to), from != to);
				let mut order = vec![0, 1, 2, 3];
				let moved = order.remove(from);
				order.insert(to, moved);
				assert_eq!(
					tabs.active(),
					order.iter().position(|i| *i == active).unwrap()
				);
				assert_eq!(tabs.session.version, request.version);
				assert_eq!(tabs.session.path, Some(request.path));
				assert!(!tabs.move_tab(4, 0));
				assert!(!tabs.move_tab(0, 4));
				for (index, id) in order.into_iter().enumerate() {
					tabs.select(index, now);
					assert_eq!(
						tabs.session.path,
						Some(PathBuf::from(format!("{id}.md")))
					);
					assert_eq!(tabs.session.scroll, id as f32 * 10.0);
				}
			}
		}
	}
}

#[test]
fn background_open_preserves_active_reading_and_deduplicates_tabs() {
	let mut tabs = Tabs::default();
	let now = Instant::now();
	tabs.open(PathBuf::from("a.md"), now);
	tabs.session.document = Some(Arc::new(crate::document::parse("current")));
	let document = tabs.session.document.clone().unwrap();
	tabs.session.scroll = 123.0;
	tabs.session.horizontal.insert((0, 0), 42.0);
	let request = tabs.request(crate::test_support::options(), true).unwrap();
	assert!(tabs.open_background(PathBuf::from("b.md"), Some("intro".into())));
	assert!(tabs.open_background(PathBuf::from("c.md"), None));
	assert!(!tabs.open_background(PathBuf::from("b.md"), None));
	assert!(!tabs.open_background(PathBuf::from("a.md"), None));
	assert_eq!(tabs.entries().len(), 3);
	assert_eq!(tabs.active(), 0);
	assert_eq!(tabs.session.path, Some(PathBuf::from("a.md")));
	assert_eq!(tabs.session.version, request.version);
	assert_eq!(tabs.session.scroll, 123.0);
	assert_eq!(tabs.session.horizontal[&(0, 0)], 42.0);
	assert!(tabs.session.follow_update);
	assert!(Arc::ptr_eq(
		tabs.session.document.as_ref().unwrap(),
		&document
	));
	assert!(tabs.select(1, now));
	assert_eq!(tabs.session.path, Some(PathBuf::from("b.md")));
	assert!(tabs.session.document.is_none());
	assert_eq!(tabs.session.pending_anchor.as_deref(), Some("intro"));
	let next = tabs.request(crate::test_support::options(), false).unwrap();
	assert_eq!(next.version, request.version + 1);
	assert_eq!(next.path, PathBuf::from("b.md"));
	assert!(tabs.select(0, now));
	assert_eq!(tabs.session.scroll, 123.0);
}

#[test]
fn background_tabs_can_be_reordered_closed_or_activated_by_closing_current() {
	let mut tabs = Tabs::default();
	let now = Instant::now();
	tabs.open(PathBuf::from("a.md"), now);
	assert!(tabs.open_background(PathBuf::from("b.md"), None));
	assert!(tabs.open_background(PathBuf::from("c.md"), None));
	assert!(tabs.move_tab(2, 1));
	assert!(matches!(tabs.close(2, now), super::Closed::Inactive));
	assert_eq!(tabs.session.path, Some(PathBuf::from("a.md")));
	assert!(matches!(tabs.close(0, now), super::Closed::Active));
	assert_eq!(tabs.session.path, Some(PathBuf::from("c.md")));
	assert!(tabs.session.document.is_none());
	assert_eq!(
		tabs.request(crate::test_support::options(), false)
			.unwrap()
			.path,
		PathBuf::from("c.md")
	);
}

#[test]
fn switching_or_closing_a_tab_ends_its_scroll_animation() {
	let mut tabs = Tabs::default();
	let now = Instant::now();
	tabs.open(PathBuf::from("a.md"), now);
	tabs.session.animate_scroll_to(500.0, now);
	assert!(tabs.session.scroll_animating());
	// Opening another document leaves the first one's animation behind.
	tabs.open(PathBuf::from("b.md"), now);
	assert!(!tabs.session.scroll_animating());
	tabs.session.animate_scroll_to(500.0, now);
	assert!(tabs.select(0, now));
	assert!(!tabs.session.scroll_animating());
	// Closing the active tab swaps in a session with nothing in flight.
	tabs.session.animate_scroll_to(500.0, now);
	tabs.close(0, now);
	assert!(!tabs.session.scroll_animating());
}
