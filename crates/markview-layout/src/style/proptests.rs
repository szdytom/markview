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
	fn text_edge_fields_share_font_permissions(
		indices in prop::collection::vec(0usize..Condition::ALL.len(), 1..8),
	) {
		let conditions = indices.into_iter().fold(ConditionSet::EMPTY, |set, index| {
			set.with(Condition::ALL[index].0)
		});
		let names = conditions.names().iter().map(|name| format!("'{name}'"))
			.collect::<Vec<_>>().join(",");
		let source = format!("format_version=2\nversion=0\n[[rule]]\nwhen=[{names}]\n");
		let font_allowed = StylesheetSource::parse(&format!(
			"{source}font=[{{family='serif'}}]"
		)).is_ok();
		for key in ["top_edge", "bottom_edge", "background_top_edge", "background_bottom_edge", "baseline"] {
			let result = StylesheetSource::parse(&format!("{source}{key}=0"));
			prop_assert_eq!(result.is_ok(), font_allowed, "when=[{}], {}: {:?}", names, key, result);
		}
	}

	#[test]
	fn media_cascade_selects_the_last_applicable_value_for_each_field(
		layers in prop::collection::vec(prop::collection::vec((any::<u8>(), prop::option::of(100u16..901), prop::option::of(any::<u8>())), 5), 1..5),
	) {
		let filters = ["", "ui", "pdf", "desktop", "landscape"];
		let mut declarations = Vec::new();
		let mut source = StylesheetSource::default();
		let mut merged = Stylesheet::default();
		for (layer, values) in layers.iter().enumerate() {
			let mut order: Vec<_> = (0..filters.len()).collect();
			order.sort_by_key(|&i| values[i].0);
			let mut toml = "format_version=2\nversion=0\n".to_owned();
			for (position, &filter) in order.iter().enumerate() {
				let (_, weight, color) = values[filter];
				// An absent declaration emits no rule; empty rules are invalid MVSS.
				if weight.is_none() && color.is_none() { continue; }
				toml.push_str("[[rule]]\nwhen=['p']\n");
				if filter != 0 { toml.push_str(&format!("media=['{}']\n", filters[filter])); }
				if let Some(weight) = weight { toml.push_str(&format!("weight={weight}\n")); }
				if let Some(color) = color { toml.push_str(&format!("color='#{color:02x}1234'\n")); }
				declarations.push(((layer, filter != 0, position), filter, weight, color));
			}
			source.merge(&StylesheetSource::parse(&toml).unwrap());
			merged.merge(&Stylesheet::parse(&toml).unwrap());
		}
		let source = Arc::new(source);
		for (target, device, width, height, active) in [
			(StyleTarget::Ui, Media::Desktop, 800., 600., [true, true, false, true, true]),
			(StyleTarget::Ui, Media::Phone, 600., 800., [true, true, false, false, false]),
			(StyleTarget::Pdf, Media::Desktop, 600., 800., [true, false, true, true, false]),
			(StyleTarget::Pdf, Media::Phone, 800., 600., [true, false, true, false, true]),
		] {
			let context = MediaContext::new(target, device, None).with_size(width, height);
			// Priority is layer, filtered over unfiltered, then declaration order.
			let weight = declarations.iter().filter(|d| active[d.1] && d.2.is_some()).max_by_key(|d| d.0).and_then(|d| d.2);
			let color = declarations.iter().filter(|d| active[d.1] && d.3.is_some()).max_by_key(|d| d.0).and_then(|d| d.3)
				.map(|v| Color((u32::from(v) << 24) | 0x1234ff));
			for sheet in [source.resolve(context), merged.for_media(context)] {
				prop_assert_eq!(sheet.rule(Condition::P).weight, weight);
				prop_assert_eq!(sheet.rule(Condition::P).color, color);
				let appearance = sheet.text(&TextAppearance::default(), Condition::P);
				prop_assert_eq!(appearance.weight, weight.unwrap_or(TextAppearance::default().weight));
				prop_assert_eq!(sheet.paint(appearance.paint), color.unwrap_or(Color(0x262b30ff)).rgba());
			}
		}
	}

	#[test]
	fn media_switches_preserve_runtime_overrides_and_previously_shared_views(
		font in "[a-zA-Z中]{1,16}", margins in prop::collection::vec(0u16..101, 4),
		ui_weight in 100u16..501, pdf_weight in 501u16..901,
		changes in prop::collection::vec(any::<bool>(), 0..24),
	) {
		let source = Arc::new(StylesheetSource::parse(&format!(
			"format_version=2\nversion=0\n[[fontdef]]\nid='reading'\nlookfor=['Original']\n[[rule]]\nwhen=['p']\nmedia=['ui']\nweight={ui_weight}\n[[rule]]\nwhen=['p']\nmedia=['pdf']\nweight={pdf_weight}\n"
		)).unwrap());
		let context = |pdf| MediaContext::new(if pdf { StyleTarget::Pdf } else { StyleTarget::Ui }, Media::Desktop, None);
		let original = source.resolve(context(false));
		let mut sheet = original.clone();
		sheet.apply_font_overrides(&[("reading".into(), font.clone())]).unwrap();
		let margins: Vec<_> = margins.into_iter().map(f32::from).collect();
		sheet.page.margin = Some(margins.clone());
		let mut previous = false;
		for pdf in [true, false, false].into_iter().chain(changes) {
			let retained = sheet.clone();
			let variant = sheet.for_media(context(pdf));
			prop_assert_eq!(sheet.set_media(context(pdf)), pdf != previous);
			for view in [&sheet, &variant] {
				prop_assert_eq!(view.rule(Condition::P).weight, Some(if pdf { pdf_weight } else { ui_weight }));
				prop_assert_eq!(view.fontdefs["reading"].lookfor.as_slice(), std::slice::from_ref(&font));
				prop_assert_eq!(view.page.margin.as_deref(), Some(margins.as_slice()));
				prop_assert!(Arc::ptr_eq(&view.source(), &source));
			}
			prop_assert_eq!(retained.rule(Condition::P).weight, Some(if previous { pdf_weight } else { ui_weight }));
			prop_assert_eq!(retained.page.margin.as_deref(), Some(margins.as_slice()));
			prop_assert_eq!(&original.fontdefs["reading"].lookfor, &["Original"]);
			prop_assert_eq!(original.page.margin.as_ref(), None);
			prop_assert_eq!(original.rule(Condition::P).weight, Some(ui_weight));
			previous = pdf;
		}
	}

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
