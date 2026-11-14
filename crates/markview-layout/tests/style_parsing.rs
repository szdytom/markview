use markview_layout::style::StylesheetSource;

fn parse_rule(
	when: &str,
	declaration: &str,
) -> anyhow::Result<StylesheetSource> {
	StylesheetSource::parse(&format!(
		"format_version=3\nversion=0\n[[rule]]\nwhen=[{when}]\n{declaration}"
	))
}

#[test]
fn restricted_rule_fields_do_not_fall_back() {
	for (when, allowed, rejected) in [
		("'tab_preview'", "radius=2", "color='#123456'"),
		("'scrollbar'", "thickness=2", "radius=2"),
		("'selection'", "background='#123456'", "color='#123456'"),
		("'caption'", "source='alt'", "letter_spacing=0.1"),
		("'placeholder'", "size=12", "line_height=1.5"),
		("'img'", "padding=2", "color='#123456'"),
		("'hr'", "border_width=2", "padding=2"),
		("'math'", "size=12", "background='#123456'"),
		("'page'", "background='#123456'", "color='#123456'"),
		("'page_header'", "size=12", "background='#123456'"),
		("'page_footer'", "size=12", "line_height=1.5"),
		("'page_number'", "size=12", "letter_spacing=0.1"),
		("'error'", "show=true", "letter_spacing=0.1"),
	] {
		parse_rule(when, allowed)
			.unwrap_or_else(|error| panic!("{when}: {allowed}: {error}"));
		assert!(parse_rule(when, rejected).is_err(), "{when}: {rejected}");
	}
}

#[test]
fn restricted_rule_fields_follow_condition_precedence() {
	for (when, allowed, rejected) in [
		("'tab_preview','scrollbar'", "radius=2", "thickness=2"),
		(
			"'scrollbar','selection'",
			"thumb='#123456'",
			"background='#123456'",
		),
		(
			"'selection','caption'",
			"background='#123456'",
			"source='alt'",
		),
		(
			"'caption','placeholder'",
			"source='alt'",
			"letter_spacing=0.1",
		),
		("'placeholder','img'", "color='#123456'", "padding=2"),
		("'img','hr'", "padding=2", "color='#123456'"),
		("'hr','math'", "space_before=2", "size=12"),
		("'math','page'", "size=12", "background='#123456'"),
		("'math','error'", "show=true", "padding=2"),
		("'page','page_header'", "size=12", "background='#123456'"),
		("'page','page_footer'", "size=12", "background='#123456'"),
		("'page','page_number'", "size=12", "background='#123456'"),
		("'page','error'", "background='#123456'", "show=true"),
		("'page_header','error'", "size=12", "show=true"),
		("'ui','error'", "show=true", "shadow='#123456'"),
	] {
		parse_rule(when, allowed)
			.unwrap_or_else(|error| panic!("{when}: {allowed}: {error}"));
		assert!(parse_rule(when, rejected).is_err(), "{when}: {rejected}");
	}
}

#[test]
fn generic_rule_fields_preserve_scope() {
	for (when, declaration, valid) in [
		("'enum'", "numbering='1.'", true),
		("'enum','list_item'", "numbering='1.'", false),
		("'enum'", "align='left'", true),
		("'enum','list_item'", "align='left'", false),
		("'front_matter'", "show=true", true),
		("'front_matter','strong'", "show=true", false),
		("'code_block'", "theme='base16-ocean.dark'", true),
		("'code_block','strong'", "theme='base16-ocean.dark'", false),
		("'code_block'", "wrap=true", true),
		("'code_block','label'", "wrap=true", false),
		("'body'", "size=12", false),
		("'p'", "size=12", true),
		("'preview'", "background='#ffeecc'", true),
		("'preview'", "padding=0.75", true),
		("'preview','p'", "line_height=1.5", true),
		("'preview','p'", "color='#123456'", true),
		("'p'", "padding=2", true),
		("'code'", "padding=2", true),
		("'em'", "padding=2", false),
		("'task_marker'", "radius=2", true),
		("'task_marker'", "padding=2", false),
		("'task_marker'", "accent='#123456'", true),
		("'p'", "accent='#123456'", false),
		("'p'", "border_edges=[1,2,3,4]", true),
		("'code'", "border_edges=[1,2,3,4]", false),
		("'p'", "orphans=2", true),
		("'em'", "orphans=2", false),
	] {
		let result = parse_rule(when, declaration);
		assert_eq!(result.is_ok(), valid, "{when}: {declaration}: {result:?}");
	}
}

#[test]
fn unsupported_rule_fields_report_the_requested_name() {
	for condition in
		["tab_preview", "scrollbar", "selection", "img", "hr", "page"]
	{
		for key in [
			"top_edge",
			"bottom_edge",
			"background_top_edge",
			"background_bottom_edge",
			"baseline",
			"unknown_field",
		] {
			let error =
				parse_rule(&format!("'{condition}'"), &format!("{key}=0"))
					.unwrap_err();
			assert_eq!(
				error.to_string(),
				format!("rule [{condition}].{key}: unsupported field")
			);
		}
	}
	assert_eq!(
		parse_rule("'p'", "unknown_field=0")
			.unwrap_err()
			.to_string(),
		"rule [p].unknown_field: unsupported field"
	);
}
