use markview_layout::style::{
	CjkType, Color, ColorField, Condition, ConditionSet, Media, MediaContext,
	StyleTarget, Stylesheet, StylesheetSource, TextAppearance, chain_of,
};
use std::sync::Arc;

#[test]
fn parsed_declarations_resolve_independently_and_preserve_runtime_overrides() {
	let mut source = StylesheetSource::parse(
		r#"
format_version = 3
version = 1
[[fontdef]]
id = "reading"
lookfor = ["Original"]
[[fontdef]]
id = "han"
type = "SC"
lookfor = ["Han"]
[[rule]]
when = ["p"]
media = ["ui"]
size = 1.2
[[rule]]
when = ["p"]
media = ["pdf"]
size = 1.4
[[rule]]
when = ["code"]
media = ["pdf"]
weight = 700
"#,
	)
	.unwrap();
	source.merge(
		&StylesheetSource::parse(
			"format_version=3\nversion=1\n[[rule]]\nwhen=['p']\nsize=2.0",
		)
		.unwrap(),
	);
	let source = Arc::new(source);
	assert_eq!(source.rule_count(), 4);
	let ui = MediaContext::new(StyleTarget::Ui, Media::Desktop, None);
	let pdf = MediaContext::new(StyleTarget::Pdf, Media::Desktop, None);
	let mut reader = source.resolve(ui);
	let paper = source.resolve(pdf);
	assert!(Arc::ptr_eq(&reader.source(), &source));
	assert!(Arc::ptr_eq(&paper.source(), &source));
	assert!(
		!reader
			.rules
			.contains_key(&ConditionSet::of(Condition::Code))
	);
	assert_eq!(paper.rule(Condition::Code).weight, Some(700));
	assert_eq!(reader.rule(Condition::P).size, Some(2.0));
	assert_eq!(paper.rule(Condition::P).size, Some(2.0));
	reader.set_cjk_type(CjkType::Sc);
	reader
		.apply_font_overrides(&[("reading".into(), "Override".into())])
		.unwrap();
	reader.page.margin = Some(vec![25.0]);
	let switched = reader.for_media(pdf);
	assert!(Arc::ptr_eq(&switched.source(), &source));
	assert_eq!(switched.rule(Condition::Code).weight, Some(700));
	assert_eq!(switched.fontdefs["reading"].lookfor, ["Override"]);
	assert_eq!(switched.fontdefs["han"].lookfor, ["Han"]);
	assert_eq!(switched.page.margin, Some(vec![25.0]));
	assert_eq!(reader.media(), ui);
	assert_eq!(reader.rule(Condition::Code).weight, None);
	assert_eq!(paper.fontdefs["reading"].lookfor, ["Original"]);
	assert_eq!(paper.cjk_type(), CjkType::None);
	assert!(reader.set_media(pdf));
	assert_eq!(reader, switched);
	assert!(!reader.set_media(pdf));
	assert!(reader.set_media(ui));
	assert_eq!(reader.rule(Condition::Code).weight, None);
}

#[test]
fn media_matches_any_entry_and_updates_text_geometry_and_paint() {
	let mut sheet = Stylesheet::parse(
		r##"
format_version = 3
version = 1
[[rule]]
when = ["body"]
color = "#112233"
[[rule]]
when = ["strong", "code"]
media = ["android", "linux"]
size = 1.5
background = "#445566"
padding = 0.25
[[rule]]
when = ["strong", "code"]
size = 0.9
"##,
	)
	.unwrap();
	let chain =
		chain_of(&[Condition::Body, Condition::Strong, Condition::Code]);
	let base = TextAppearance::default();
	for (platform, matches) in [
		(Media::Android, true),
		(Media::Linux, true),
		(Media::Windows, false),
	] {
		sheet.set_media(MediaContext::new(
			StyleTarget::Ui,
			Media::Desktop,
			Some(platform),
		));
		let strong =
			sheet.text(&sheet.text(&base, Condition::Body), Condition::Strong);
		assert_eq!(
			sheet.text(&strong, Condition::Code).size,
			if matches { 1.5 } else { 0.9 }
		);
		assert_eq!(
			sheet.element_rule(chain, Condition::Code).padding.is_some(),
			matches
		);
		assert_eq!(
			sheet.paint(markview_layout::scene::Paint::Scoped(
				chain,
				Condition::Code,
				ColorField::Background
			)),
			if matches {
				Color(0x445566ff).rgba()
			} else {
				Color(0).rgba()
			}
		);
		assert_eq!(
			sheet.color(Condition::Body, ColorField::Color),
			Color(0x112233ff).rgba()
		);
		let code_only = sheet.text(&base, Condition::Code);
		assert_eq!(code_only.size, base.size);
	}
}

#[test]
fn media_cascade_keeps_layer_priority_and_source_order_after_context_changes() {
	let mut sheet = Stylesheet::parse(
		r#"
format_version = 3
version = 1
[[rule]]
when = ["p"]
size = 1.0
[[rule]]
when = ["p"]
media = ["mobile"]
size = 1.2
[[rule]]
when = ["p"]
media = ["phone", "tablet"]
size = 1.4
[[rule]]
when = ["p"]
media = ["landscape"]
weight = 700
"#,
	)
	.unwrap();
	for (device, size) in [
		(Media::Phone, 1.4),
		(Media::Tablet, 1.4),
		(Media::Desktop, 1.0),
	] {
		let context =
			MediaContext::new(StyleTarget::Ui, device, Some(Media::Android));
		sheet.set_media(context.with_size(600.0, 800.0));
		assert_eq!(sheet.rule(Condition::P).size, Some(size));
		assert_eq!(sheet.rule(Condition::P).weight, None);
		sheet.set_media(context.with_size(800.0, 600.0));
		assert_eq!(sheet.rule(Condition::P).weight, Some(700));
	}
	let higher = Stylesheet::parse(
		"format_version=3\nversion=1\n[[rule]]\nwhen=['p']\nsize=2.0",
	)
	.unwrap();
	sheet.merge(&higher);
	let mut composed = (*Stylesheet::builtin()).clone();
	composed.merge(&sheet);
	for device in [Media::Desktop, Media::Phone, Media::Tablet] {
		composed.set_media(
			MediaContext::new(StyleTarget::Pdf, device, None)
				.with_size(600.0, 800.0),
		);
		assert_eq!(composed.rule(Condition::P).size, Some(2.0));
		assert_eq!(
			composed.rule(Condition::P).weight,
			Stylesheet::builtin().rule(Condition::P).weight
		);
	}
}

#[test]
fn media_selectors_validate_and_canonicalize_independently_of_when() {
	for media in [
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
	] {
		assert!(Stylesheet::parse(&format!("format_version=3\nversion=1\n[[rule]]\nwhen=['p']\nmedia=['{media}']\nsize=1.0")).is_ok());
	}
	for media in [
		"[]",
		"'ui'",
		"['unknown']",
		"['ui', 'ui']",
		"[1]",
		"['ui', false]",
	] {
		let error = Stylesheet::parse(&format!(
			"format_version=3\nversion=1\n[[rule]]\nwhen=['p']\nmedia={media}\nsize=1.0"
		))
		.unwrap_err();
		assert!(
			format!("{error:#}").contains("rule.media"),
			"{media}: {error:#}"
		);
	}
	let duplicate = Stylesheet::parse("format_version=3\nversion=1\n[[rule]]\nwhen=['strong','code']\nmedia=['ui','pdf']\nsize=1.0\n[[rule]]\nwhen=['code','strong']\nmedia=['pdf','ui']\nsize=2.0").unwrap_err();
	assert!(
		duplicate
			.to_string()
			.contains("duplicate conditions and media")
	);
}
