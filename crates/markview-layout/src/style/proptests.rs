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
