use super::*;
use markview_layout::style::{Color, Condition, MediaContext};
use proptest::prelude::*;

proptest! {
	#[test]
	fn stylesheet_sequences_validate_before_replacing_and_select_the_leftmost_layer(
		operations in prop::collection::vec((0u8..5, 0usize..4, any::<u32>(), prop::collection::vec(0usize..5, 0..6)), 0..24),
	) {
		let mut styles = Stylesheets::default();
		let default = Stylesheet::bundled(false);
		let mut registered = [None; 4];
		let mut explicit = false;
		for (operation, slot, rgba, selection) in operations {
			let id = format!("custom-{slot}");
			match operation {
				0 => { styles.register(&id, &format!("format_version = 2\nversion = 1\n[[rule]]\nwhen = ['body']\ncolor = '#{rgba:08x}'")).unwrap(); registered[slot] = Some(rgba); }
				1 => { prop_assert!(styles.register(&id, "invalid TOML").is_err()); }
				2 => { prop_assert!(styles.register(&id, "format_version = 2\nversion = 1\ntargets = ['pdf']").is_err()); }
				3 => { prop_assert!(styles.register("bundled:custom", "format_version = 2\nversion = 1").is_err()); }
				_ => {
					let valid = selection.iter().all(|&index| index < 4 && registered[index].is_some());
					let ids: Vec<_> = selection.iter().map(|index| format!("custom-{index}")).collect();
					let selected = styles.select(&ids, default.clone());
					prop_assert_eq!(selected.is_ok(), valid);
					if valid {
						explicit = !selection.is_empty();
						let selected = selected.unwrap();
						if let Some(&first) = selection.first() { prop_assert_eq!(selected.rule(Condition::Body).color, registered[first].map(Color)); }
						else { prop_assert!(Arc::ptr_eq(&selected, &default)); }
					}
				}
			}
			prop_assert_eq!(styles.explicit, explicit);
			for (index, &color) in registered.iter().enumerate() {
				let resolved = styles.resolve(&format!("custom-{index}"));
				prop_assert_eq!(resolved.is_ok(), color.is_some());
				if let Some(color) = color { prop_assert_eq!(resolved.unwrap().resolve(MediaContext::default()).rule(Condition::Body).color, Some(Color(color))); }
			}
		}
	}
}
