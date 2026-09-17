use super::*;
#[test]
fn bundled_emoji_keeps_regular_face_in_headings_and_emphasis() {
	for dark in [false, true] {
		let sheet = Stylesheet::bundled(dark);
		let body = sheet.text(&TextAppearance::default(), Condition::Body);
		for &(role, _) in Condition::ALL {
			let parent = sheet.text(&body, role);
			for (bold, italic) in
				[(false, false), (true, false), (false, true), (true, true)]
			{
				let appearance = sheet.inline(
					&parent,
					&crate::document::TextStyle {
						bold,
						italic,
						..Default::default()
					},
				);
				for font in
					appearance.font.iter().filter(|f| f.family == "emoji")
				{
					assert_eq!(
						font.weight,
						Some(400),
						"dark={dark} role={role:?}"
					);
					assert_eq!(font.variant, Variant::Normal);
				}
			}
		}
		assert_eq!(
			sheet
				.inline(
					&body,
					&crate::document::TextStyle {
						bold: true,
						..Default::default()
					}
				)
				.weight,
			700
		);
	}
}
#[test]
fn strict_schema() {
	for bad in [
		"[body]\ncolor='#ffffff'",
		"format_version=1\nversion=1",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['nope']\ncolor='#ffffff'",
		"format_version=2\nversion=1\n[[rule]]\nwhen=[]\ncolor='#ffffff'",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['em']\ncolor='#ffffff'\n[[rule]]\nwhen=['em']\nsize=1.2",
		"format_version=2",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['em']\nfont=[]",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['p']\nsize=nan",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['body']\nbackground='#ffffff00'",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['em']\ncolorz='#ffffff'",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['ui']\npadding=2",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['math']\nfont=[{family='serif'}]",
		"format_version=2\nversion=1\n[em]\ncolor='#ffffff'",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['em']",
		"format_version=2\nversion=1\n[[rule]]\ncolor='#ffffff'",
		"format_version=2\nversion=1\n[[fontdef]]\nid='cjk'\ntype='none'\nlookfor=['serif']",
	] {
		assert!(Stylesheet::parse(bad).is_err(), "{bad}");
	}
}
#[test]
fn scrollbar_sizes_are_configurable_and_validated() {
	// A stylesheet without a scrollbar rule falls back to the built-in
	// defaults; the bundled theme is free to pick its own sizes.
	let bare = Stylesheet::parse(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['p']\ncolor='#000000'",
	)
	.unwrap();
	assert_eq!(bare.scrollbar_metrics(), ScrollbarMetrics::DOCUMENT);
	assert_eq!(
		bare.overflow_scrollbar_metrics(),
		ScrollbarMetrics::OVERFLOW
	);
	assert_eq!(bare.scrollbar_gutter(), SCROLLBAR_GUTTER);
	let mut sheet = (*Stylesheet::bundled(false)).clone();
	sheet.merge(
		&Stylesheet::parse(
			"format_version=2\nversion=1\n[[rule]]\nwhen=['scrollbar']\nthickness=3.0\nthickness_hover=9.0\noverflow_thickness=4.0\noverflow_thickness_hover=4.0\ngutter=12.0",
		)
		.unwrap(),
	);
	assert_eq!(
		sheet.scrollbar_metrics(),
		ScrollbarMetrics {
			thickness: 3.0,
			thickness_hover: 9.0
		}
	);
	assert_eq!(
		sheet.overflow_scrollbar_metrics(),
		ScrollbarMetrics {
			thickness: 4.0,
			thickness_hover: 4.0
		}
	);
	assert_eq!(sheet.scrollbar_gutter(), 12.0);
	// A theme that only overrides colors keeps the bundled sizes.
	sheet.merge(
		&Stylesheet::parse(
			"format_version=2\nversion=1\n[[rule]]\nwhen=['scrollbar']\nthumb='#000000'",
		)
		.unwrap(),
	);
	assert_eq!(sheet.scrollbar_metrics().thickness, 3.0);
	assert_eq!(sheet.scrollbar_gutter(), 12.0);
	for bad in [
		"format_version=2\nversion=1\n[[rule]]\nwhen=['scrollbar']\nthickness=0.0",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['scrollbar']\nthickness_hover=-1.0",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['scrollbar']\noverflow_thickness=0.0",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['scrollbar']\ngutter=-1.0",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['scrollbar']\ngutter=nan",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['p']\nthickness=4.0",
	] {
		assert!(Stylesheet::parse(bad).is_err(), "{bad}");
	}
}
#[test]
fn fontdefs_are_selected_by_cjk_type() {
	let source = "format_version=2\nversion=1\n[[fontdef]]\nid='cjk'\ntype='SC'\nlookfor=['SC']\n[[fontdef]]\nid='cjk'\ntype='TC'\nlookfor=['TC']";
	let mut sheet = Stylesheet::parse(source).unwrap();
	assert!(!sheet.fontdefs.contains_key("cjk"));
	sheet.set_cjk_type(CjkType::Sc);
	assert_eq!(sheet.fontdefs["cjk"].lookfor, ["SC"]);
	sheet.set_cjk_type(CjkType::Tc);
	assert_eq!(sheet.fontdefs["cjk"].lookfor, ["TC"]);
	sheet.set_cjk_type(CjkType::Jp);
	assert!(!sheet.fontdefs.contains_key("cjk"));
}
#[test]
fn cascade_arrays_and_font_defaults() {
	let mut low=Stylesheet::parse("format_version=2\nversion=1\n[[rule]]\nwhen=['em']\ncolor='#123456'\nfont=[{family='Noto Serif',variant='italic'},{family='落霞文楷'}]").unwrap();
	let high = Stylesheet::parse(
		"format_version=2\nversion=2\n[[rule]]\nwhen=['em']\ncolor='#abcdef'",
	)
	.unwrap();
	low.merge(&high);
	assert_eq!(
		low.rule(Condition::Em).font.as_ref().unwrap()[1].variant,
		Variant::Normal
	);
	assert_eq!(
		low.color(Condition::Em, ColorField::Color),
		Color(0xabcdefff).rgba()
	);
	low.merge(
		&Stylesheet::parse(
			"format_version=2\nversion=3\n[[rule]]\nwhen=['em']\nfont=[{family='serif'}]",
		)
		.unwrap(),
	);
	assert_eq!(low.rule(Condition::Em).font.as_ref().unwrap().len(), 1);
}
#[test]
fn list_indents_are_theme_controlled_per_list_role() {
	let sheet = Stylesheet::parse(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['list']\nindent=0.25\n[[rule]]\nwhen=['enum']\nindent=0.75",
	)
	.unwrap();
	assert_eq!(sheet.list_indent(false), 0.25);
	assert_eq!(sheet.list_indent(true), 0.75);
	// The roles are independent: `[list]` alone leaves ordered lists flush.
	let bullets = Stylesheet::parse(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['list']\nindent=0.25",
	)
	.unwrap();
	assert_eq!(bullets.list_indent(false), 0.25);
	assert_eq!(bullets.list_indent(true), 0.0);
	let bundled = Stylesheet::bundled(false);
	assert_eq!(bundled.list_indent(false), 0.5);
	assert_eq!(bundled.list_indent(true), 0.5);
	for bad in [
		"format_version=2\nversion=1\n[[rule]]\nwhen=['list']\nindent=-1.0",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['enum']\nindent=nan",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['p']\nindent=1.0",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['list']\nindentz=1.0",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['list']\nordered_indent=1.0",
	] {
		assert!(Stylesheet::parse(bad).is_err(), "{bad}");
	}
}
#[test]
fn conditions_compose_without_new_vocabulary() {
	let mut sheet = (*Stylesheet::bundled(false)).clone();
	sheet.merge(
		&Stylesheet::parse(
			"format_version=2\nversion=1\n[[rule]]\nwhen=['strong']\ncolor='#ff0000'\n[[rule]]\nwhen=['code']\nsize=0.9\n[[rule]]\nwhen=['strong','code']\nsize=1.3\ncolor='#00ff00'\nbackground='#010203'\n[[rule]]\nwhen=['h1','code']\nsize=1.4",
		)
		.unwrap(),
	);
	let body = sheet.text(&TextAppearance::default(), Condition::Body);
	let bold_code = sheet.inline(
		&body,
		&crate::document::TextStyle {
			bold: true,
			code: true,
			..Default::default()
		},
	);
	// The two-condition rule wins over both of its parts.
	assert_eq!(bold_code.size, 1.3);
	assert_eq!(sheet.paint(bold_code.paint), Color(0x00ff00ff).rgba());
	assert_eq!(
		sheet.paint(bold_code.background.unwrap()),
		Color(0x010203ff).rgba()
	);
	// Fields the compound leaves alone still come from its parts.
	assert_eq!(bold_code.weight, 700);
	let plain_code = sheet.inline(
		&body,
		&crate::document::TextStyle {
			code: true,
			..Default::default()
		},
	);
	assert_eq!(plain_code.size, 0.9);
	assert_eq!(bold_code.font, plain_code.font);
	// A block condition composes with inline markup the same way.
	let heading = sheet.text(&body, Condition::H1);
	let heading_code = sheet.inline(
		&heading,
		&crate::document::TextStyle {
			code: true,
			..Default::default()
		},
	);
	assert_eq!(heading_code.size, 1.4);
	assert_eq!(plain_code.size, 0.9);
	// State is just another condition: hover reaches `["link", "hover"]`.
	assert_eq!(
		sheet.paint(Paint::Styled(Condition::Hover, ColorField::Color)),
		Color(0x1f4568ff).rgba()
	);
	// The order inside `when` is not part of the rule's identity.
	let mut a = Stylesheet::parse(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['em','strong']\ncolor='#111111'",
	)
	.unwrap();
	a.merge(
		&Stylesheet::parse(
			"format_version=2\nversion=2\n[[rule]]\nwhen=['strong','em']\nsize=1.2",
		)
		.unwrap(),
	);
	let compound = ConditionSet::of(Condition::Em).with(Condition::Strong);
	assert_eq!(a.rules.len(), 1);
	assert_eq!(a.rules[&compound].size, Some(1.2));
	assert_eq!(a.rules[&compound].color, Some(Color(0x111111ff)));
	// A theme without the compound keeps the `code` look at bold weight.
	let bundled = Stylesheet::bundled(false);
	let body = bundled.text(&TextAppearance::default(), Condition::Body);
	let appearance = bundled.inline(
		&body,
		&crate::document::TextStyle {
			bold: true,
			code: true,
			..Default::default()
		},
	);
	assert_eq!(appearance.weight, 700);
	assert_eq!(appearance.size, bundled.rule(Condition::Code).size.unwrap());
	assert_eq!(
		bundled.paint(appearance.background.unwrap()),
		bundled.paint(Paint::Styled(Condition::Code, ColorField::Background))
	);
}

#[test]
fn colors_do_not_change_layout_identity() {
	let mut s = (*Stylesheet::bundled(false)).clone();
	let k = s.layout_key();
	s.merge(
		&Stylesheet::parse(
			"format_version=2\nversion=2\n[[rule]]\nwhen=['em']\ncolor='#ffffff'",
		)
		.unwrap(),
	);
	assert_eq!(k, s.layout_key());
	s.merge(
		&Stylesheet::parse(
			"format_version=2\nversion=2\n[[rule]]\nwhen=['em']\nsize=1.2",
		)
		.unwrap(),
	);
	assert_ne!(k, s.layout_key());
	assert!(
		Stylesheet::bundled(true)
			.rule(Condition::Body)
			.background
			.is_some()
	);
	let hover = Stylesheet::parse(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['link']\ncolor='#123456'\n[[rule]]\nwhen=['link','hover']\ncolor='#abcdef'",
	)
	.unwrap();
	assert_eq!(
		hover.paint(Paint::Styled(Condition::Hover, ColorField::Color)),
		Color(0xabcdefff).rgba()
	);
}

#[test]
fn container_geometry_does_not_inherit() {
	let sheet = Stylesheet::parse(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['list']\npadding=1.0\n[[rule]]\nwhen=['list_item']\npadding=0.5\n[[rule]]\nwhen=['p']\nspace_after=0.25",
	)
	.unwrap();
	let chain = chain_of(&[
		Condition::Body,
		Condition::List,
		Condition::ListItem,
		Condition::P,
	]);
	// A paragraph keeps its own spacing and takes no ancestor padding.
	let paragraph = sheet.element_rule(chain, Condition::P);
	assert_eq!(paragraph.padding, None);
	assert_eq!(paragraph.space_after, Some(0.25));
	assert_eq!(
		sheet.element_rule(chain, Condition::ListItem).padding,
		Some(Padding::All(0.5))
	);
	assert_eq!(
		sheet.element_rule(chain, Condition::List).padding,
		Some(Padding::All(1.0))
	);
}

#[test]
fn inline_backgrounds_stay_within_the_inline_run() {
	let sheet = Stylesheet::parse(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['body']\nbackground='#00ff00'\n[[rule]]\nwhen=['blockquote']\nbackground='#eeeeee'\n[[rule]]\nwhen=['blockquote','code']\nbackground='#0000ff'",
	)
	.unwrap();
	let mut body = sheet.text(&TextAppearance::default(), Condition::Body);
	body = sheet.text(&body, Condition::Blockquote);
	body = sheet.text(&body, Condition::P);
	// A compound that names the inline run still paints it.
	let code = sheet.inline(
		&body,
		&crate::document::TextStyle {
			code: true,
			..Default::default()
		},
	);
	assert_eq!(
		sheet.paint(code.background.unwrap()),
		Color(0x0000ffff).rgba()
	);
	// A plain container background does not reach an inline run.
	let bold = sheet.inline(
		&body,
		&crate::document::TextStyle {
			bold: true,
			..Default::default()
		},
	);
	assert_eq!(sheet.paint(bold.background.unwrap()), Color(0).rgba());
	// The window still clears to the body background.
	assert_eq!(sheet.paint(Paint::Background), Color(0x00ff00ff).rgba());
}

#[test]
fn math_error_text_carries_both_conditions() {
	let sheet = Stylesheet::parse(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['math','error']\ncolor='#ff0000'",
	)
	.unwrap();
	let body = sheet.text(&TextAppearance::default(), Condition::Body);
	let run = sheet.inline(
		&body,
		&crate::document::TextStyle {
			code: true,
			math_error: true,
			..Default::default()
		},
	);
	assert_eq!(sheet.paint(run.paint), Color(0xff0000ff).rgba());
}

#[test]
fn bundled_table_cells_keep_their_grid() {
	let sheet = Stylesheet::bundled(false);
	let chain = chain_of(&[Condition::Body, Condition::Table, Condition::Cell]);
	let cell = sheet.element_rule(chain, Condition::Cell);
	assert_eq!(cell.border_width, Some(1.0));
	assert!(cell.border_color.is_some());
	// The header is a specialization of the cell and keeps them too.
	let chain = chain_of(&[
		Condition::Body,
		Condition::Table,
		Condition::Cell,
		Condition::Header,
	]);
	let header = sheet.element_rule(chain, Condition::Header);
	assert_eq!(header.border_width, Some(1.0));
	assert!(header.border_color.is_some());
}

#[test]
fn theme_belongs_to_the_plain_code_block_condition() {
	assert!(
		Stylesheet::parse(
			"format_version=2\nversion=1\n[[rule]]\nwhen=['code_block']\ntheme='InspiredGitHub'"
		)
		.is_ok()
	);
	for bad in [
		"format_version=2\nversion=1\n[[rule]]\nwhen=['blockquote','code_block']\ntheme='InspiredGitHub'",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['code_block','label']\ntheme='InspiredGitHub'",
	] {
		assert!(Stylesheet::parse(bad).is_err(), "{bad}");
	}
}

#[test]
fn the_cjk_convention_is_layout_relevant() {
	// Each convention resolves `serif[cjk]` to a different family, so a change
	// of convention has to reflow: the old geometry was measured with the old
	// face.
	let mut s = (*Stylesheet::bundled(false)).clone();
	s.set_cjk_type(CjkType::Sc);
	let sc = s.layout_key();
	s.set_cjk_type(CjkType::Jp);
	assert_ne!(sc, s.layout_key());
}
