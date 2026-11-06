use super::*;
use proptest::prelude::*;

const MEDIA: &[&str] = &[
	"ui",
	"pdf",
	"desktop",
	"mobile",
	"tablet",
	"phone",
	"landscape",
	"portrait",
	"android",
	"linux",
	"windows",
	"macos",
	"ios",
	"web",
];

proptest! {
	#[test]
	fn finite_cascade_preserves_inheritance_specificity_and_ordered_partial_merges(
		layers in prop::collection::vec(prop::collection::vec((0usize..6,
			prop::option::of(1u16..17), prop::option::of(100u16..901), prop::option::of(any::<u8>())), 0..10), 1..5),
	) {
		// This selector subset has no equal-specificity ties at the same chain depth.
		let masks = [1u8, 2, 3, 4, 6, 7];
		let conditions = [Condition::Body, Condition::Blockquote, Condition::Strong];
		let mut declarations = [[None; 3]; 6];
		let mut merged = Stylesheet::parse("format_version=2\nversion=0").unwrap();
		let mut source = StylesheetSource::default();
		for layer in layers {
			let mut local = [[None; 3]; 6];
			for (selector, size, weight, color) in layer {
				let size = size.filter(|_| masks[selector] & 1 == 0);
				for (field, value) in [size, weight, color.map(u16::from)].into_iter().enumerate() {
					if value.is_some() { local[selector][field] = value; }
				}
			}
			let mut toml = "format_version=2\nversion=0\n".to_owned();
			for (selector, values) in local.into_iter().enumerate() {
				if values == [None; 3] { continue; }
				let names: Vec<_> = ["body", "blockquote", "strong"].into_iter().enumerate()
					.filter(|(index, _)| masks[selector] & (1 << index) != 0)
					.map(|(_, name)| format!("\"{name}\"")).collect();
				toml.push_str(&format!("[[rule]]\nwhen=[{}]\n", names.join(",")));
				for (field, value) in values.into_iter().enumerate() {
					if let Some(value) = value {
						declarations[selector][field] = Some(value);
						let line = match field {
							0 => format!("size={}\n", f32::from(value) / 4.0),
							1 => format!("weight={value}\n"),
							_ => format!("color='#{value:02x}1234'\n"),
						};
						toml.push_str(&line);
					}
				}
			}
			merged.merge(&Stylesheet::parse(&toml).unwrap());
			source.merge(&StylesheetSource::parse(&toml).unwrap());
			let resolved = Arc::new(source.clone()).resolve(merged.media());
			for sheet in [&merged, &resolved] {
				let mut appearance = TextAppearance::default();
				let mut expected_size = appearance.size;
				let mut expected_weight = appearance.weight;
				for (depth, condition) in conditions.into_iter().enumerate() {
					let active = (1u8 << (depth + 1)) - 1;
					let winner = |field: usize| (0..masks.len())
						.filter(|&i| masks[i] & active == masks[i] && declarations[i][field].is_some())
						.max_by_key(|&i| (7 - masks[i].leading_zeros(), masks[i].count_ones()));
					if let Some(i) = winner(0).filter(|&i| masks[i] & (1 << depth) != 0) {
						expected_size = f32::from(declarations[i][0].unwrap()) / 4.0;
					}
					if let Some(i) = winner(1).filter(|&i| masks[i] & (1 << depth) != 0) {
						expected_weight = declarations[i][1].unwrap();
					}
					appearance = sheet.text(&appearance, condition);
					prop_assert_eq!(appearance.size, expected_size);
					prop_assert_eq!(appearance.weight, expected_weight);
					let color = winner(2).map_or(Color(0x262b30ff), |i| Color((u32::from(declarations[i][2].unwrap()) << 24) | 0x1234ff));
					prop_assert_eq!(sheet.paint(appearance.paint), color.rgba());
				}
			}
		}
	}

	#[test]
	fn media_rules_follow_environment_changes_and_or_filters(
		filter in prop::collection::vec(0usize..MEDIA.len(), 1..8),
		pdf in any::<bool>(), platform in 0usize..7,
		changes in prop::collection::vec((0usize..4, 1u16..1000, 1u16..1000), 1..20),
	) {
		let mut names: Vec<_> = filter.iter().map(|&i| MEDIA[i]).collect();
		names.sort_unstable();
		names.dedup();
		let media = names.iter().map(|s| format!("\"{s}\"")).collect::<Vec<_>>().join(",");
		let source = Arc::new(StylesheetSource::parse(&format!(
			"format_version = 2\nversion = 0\ntargets = [\"ui\", \"pdf\"]\n[[rule]]\nwhen = [\"body\"]\nmedia = [{media}]\ncolor = \"#123456\"\n"
		)).unwrap());
		let target = if pdf { StyleTarget::Pdf } else { StyleTarget::Ui };
		let platforms = [None, Some(Media::Android), Some(Media::Linux), Some(Media::Windows), Some(Media::Macos), Some(Media::Ios), Some(Media::Web)];
		let mut context = MediaContext::new(target, Media::Desktop, platforms[platform]);
		let mut sheet = source.resolve(context);
		for (device, width, height) in changes {
			context = context.with_device([Media::Desktop, Media::Mobile, Media::Tablet, Media::Phone][device])
				.with_size(f32::from(width), f32::from(height));
			let mut active = vec![if pdf { "pdf" } else { "ui" }, ["desktop", "mobile", "tablet", "phone"][device]];
			if device >= 2 { active.push("mobile"); }
			if platform > 0 { active.push(MEDIA[7 + platform]); }
			active.push(if width > height { "landscape" } else { "portrait" });
			let applies = names.iter().any(|name| active.contains(name));
			let before = sheet.rules.clone();
			let variant = sheet.for_media(context);
			let changed = sheet.set_media(context);
			prop_assert_eq!(changed, before != sheet.rules);
			prop_assert!(!sheet.set_media(context));
			prop_assert_eq!(&variant.rules, &sheet.rules);
			prop_assert_eq!(sheet.media(), context);
			prop_assert_eq!(sheet.rules.get(&ConditionSet::of(Condition::Body)).and_then(|r| r.color), applies.then_some(Color(0x123456ff)));
		}
	}

	#[test]
	fn condition_chains_keep_the_most_recent_occurrence_and_match_set_operations(
		operations in prop::collection::vec((0usize..types::Condition::ALL.len(), any::<bool>()), 0..MAX_CHAIN + 1),
	) {
		let mut ordered = Vec::new();
		let mut set = ConditionSet::EMPTY;
		let mut members = Vec::new();
		let mut chain = 0;
		for (index, add) in operations {
			let condition = Condition::ALL[index].0;
			ordered.retain(|c| *c != condition);
			ordered.push(condition);
			chain = chain_push(chain, condition);
			if add {
				if !members.contains(&condition) { members.push(condition); }
				set = set.with(condition);
			} else {
				members.retain(|c| *c != condition);
				set = set.without(condition);
			}
			let mut decoded = Vec::new();
			let mut packed = chain;
			while packed != 0 {
				decoded.push(Condition::ALL[(packed & types::CHAIN_MASK) as usize - 1].0);
				packed >>= types::CHAIN_BITS;
			}
			decoded.reverse();
			prop_assert_eq!(&decoded, &ordered);
			let chain_members = chain_set(chain);
			for &(c, _) in Condition::ALL {
				prop_assert_eq!(chain_members.contains(c), ordered.contains(&c));
				prop_assert_eq!(set.contains(c), members.contains(&c));
			}
			prop_assert_eq!(set.len() as usize, members.len());
			prop_assert_eq!(set.is_empty(), members.is_empty());
			prop_assert_eq!(set.is_subset_of(chain_members), members.iter().all(|c| ordered.contains(c)));
			prop_assert_eq!(set.intersects(chain_members), members.iter().any(|c| ordered.contains(c)));
		}
	}
}
