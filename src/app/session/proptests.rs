use super::*;
use proptest::prelude::*;

proptest! {
	#[test]
	fn session_files_preserve_current_fields_and_migrate_only_legacy_structure(
		version in 0u32..7, count in 0usize..6, active in any::<u16>(),
		path in prop::collection::vec(0u32..8, 0..6), field in prop::option::of(0u32..8), byte in 0usize..1025,
		input in any::<u64>(), frontend in any::<u32>(), local in any::<i16>(), fallback in any::<u16>(), ratio in any::<u8>(),
		details in prop::collection::vec((prop::collection::vec(0u32..8, 0..5), any::<bool>()), 0..8),
		query in prop::collection::vec(any::<char>(), 0..24), flags in prop::array::uniform4(any::<bool>()),
	) {
		let directory = tempfile::tempdir().unwrap();
		let file = directory.path().join("session.json");
		let reading = Reading { position: Position { input, frontend: u64::from(frontend), path, field, byte, local: f32::from(local) / 4.0, fallback: f32::from(fallback) / 4.0, fallback_ratio: f32::from(ratio) / 256.0 }, details: details.into() };
		let search = SavedSearch { open: flags[0], query: query.into_iter().collect(), case_sensitive: flags[1], whole_word: flags[2] };
		let session = Session { version, active: usize::from(active), folder: Some(PathBuf::from("folder 空")), folder_open: flags[3], recent: (0..count).rev().collect(), tabs: (0..count).map(|index| SavedTab { origin: legacy_origin(), path: PathBuf::from(format!("{index}.md")), reading: reading.clone(), search: search.clone() }).collect() };
		session.write(&file).unwrap();
		let json: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
		prop_assert_eq!(json["version"].as_u64(), Some(u64::from(version)));
		prop_assert_eq!(json["tabs"].as_array().unwrap().len(), count);
		for tab in json["tabs"].as_array().unwrap() {
			prop_assert_eq!(tab["reading"]["position"]["input"].as_u64(), Some(input));
			prop_assert_eq!(tab["search"]["query"].as_str(), Some(search.query.as_str()));
		}
		let result = Session::read(&file);
		if !(1..=4).contains(&version) { prop_assert!(result.is_err()); }
		else {
			let restored = result.unwrap();
			prop_assert_eq!(restored.version, 4);
			prop_assert_eq!(restored.active, usize::from(active));
			prop_assert_eq!(restored.recent, (0..count).rev().collect::<Vec<_>>());
			prop_assert_eq!(restored.folder, session.folder);
			prop_assert_eq!(restored.folder_open, flags[3]);
			let expected = if version < 4 { Reading { position: Position { fallback: reading.position.fallback, fallback_ratio: reading.position.fallback_ratio, ..Default::default() }, details: SavedDetails::default() } } else { reading };
			prop_assert_eq!(restored.tabs.len(), count);
			for (index, tab) in restored.tabs.iter().enumerate() {
				prop_assert_eq!(&tab.reading, &expected);
				prop_assert_eq!(&tab.search, &search);
				prop_assert_eq!(&tab.path, &PathBuf::from(format!("{index}.md")));
				prop_assert_eq!(&tab.origin, &legacy_origin());
			}
		}
	}

	#[test]
	fn fallback_restoration_waits_for_completion_then_clamps_and_consumes_the_saved_position(
		height in 0u16..4097, viewport in 0u16..1025, fallback in -100i16..8193, ratio in 0u16..513,
	) {
		let position = Position { fallback: f32::from(fallback), fallback_ratio: f32::from(ratio) / 256.0, ..Default::default() };
		let mut session = ReaderSession { saved_reading: Some(Reading { position, details: SavedDetails::default() }), display: crate::state::DisplayState { snapshot: LayoutSnapshot { height: f32::from(height), ..Default::default() }, ..Default::default() }, snapshot_complete: false, ..Default::default() };
		session.scrolling.offset = 17.0;
		session.restore_reading(f32::from(viewport));
		prop_assert!(session.saved_reading.is_some());
		prop_assert_eq!(session.scrolling.offset, 17.0);
		let limit = session.display.snapshot.scroll_limit(f32::from(viewport));
		let target = if ratio > 0 { f32::from(ratio) * limit / 256.0 } else { f32::from(fallback) };
		session.snapshot_complete = true;
		session.restore_reading(f32::from(viewport));
		prop_assert_eq!(session.scrolling.offset, target.clamp(0.0, limit));
		prop_assert!(session.saved_reading.is_none());
		prop_assert!(session.scrolling.target.is_none());
		session.scrolling.offset = 5.0;
		session.restore_reading(f32::from(viewport));
		prop_assert_eq!(session.scrolling.offset, 5.0);
	}
}
