use super::*;

#[test]
fn decoration_fields_are_strict() {
	for declaration in [
		"border_edges=[1,2]",
		"corner_radii=[1,-2,3,4]",
		"letter_spacing=nan",
		"orphans=0",
		"widows=-1",
		"heading_marker=[1,inf,0]",
	] {
		assert!(
			Stylesheet::parse(&format!(
				"format_version=2\nversion=1\n[[rule]]\nwhen=['h2']\n{declaration}"
			))
			.is_err(),
			"{declaration}"
		);
	}
	assert!(
		Stylesheet::parse(
			"format_version=2\nversion=1\n[[rule]]\nwhen=['p']\nheading_marker=[1,1,1]"
		)
		.is_err()
	);
}

#[test]
fn page_bands_validate_cascade_and_leave_layout_unchanged() {
	let mut sheet = (*Stylesheet::bundled_print()).clone();
	let layout = sheet.layout_key();
	let geometry =
		crate::paginate::PageGeometry::from_style(sheet.page()).unwrap();
	sheet.merge(&Stylesheet::parse("format_version=2\nversion=1\n[page.footer]\nrule_width=2\nrule_color='#112233'").unwrap());
	for source in [
		"header.rule_width=3.0\nheader.rule_color='#244C8080'",
		"header.rule_width=4.5",
	] {
		sheet.merge(
			&Stylesheet::parse(&format!(
				"format_version=2\nversion=1\n[page]\n{source}"
			))
			.unwrap(),
		);
	}
	assert_eq!(
		sheet.page().header.rule(100.0),
		Some((4.5, Color(0x244C8080)))
	);
	assert_eq!(
		sheet.page().header.rule(2.0),
		Some((2.0, Color(0x244C8080)))
	);
	assert_eq!(sheet.layout_key(), layout);
	assert_eq!(
		sheet.page().footer.rule(100.0),
		Some((2.0, Color(0x112233FF)))
	);
	for section in ["header", "footer"] {
		for field in [
			"rule_width=-1",
			"rule_width=nan",
			"rule_width='3'",
			"rule_color=''",
			"unknown=1",
		] {
			assert!(
				Stylesheet::parse(&format!(
					"format_version=2\nversion=1\n[page.{section}]\n{field}"
				))
				.is_err()
			);
		}
	}
	assert_eq!(
		crate::paginate::PageGeometry::from_style(sheet.page()).unwrap(),
		geometry
	);
	for source in [
		"header.rule_width=-1",
		"header.rule_width=nan",
		"header.rule_width=inf",
		"header.rule_color='red'",
	] {
		assert!(
			Stylesheet::parse(&format!(
				"format_version=2\nversion=1\n[page]\n{source}"
			))
			.is_err()
		);
	}
	sheet.merge(
		&Stylesheet::parse(
			"format_version=2\nversion=1\n[page]\nheader.rule_width=0",
		)
		.unwrap(),
	);
	assert_eq!(sheet.page().header.rule(100.0), None);
	assert_eq!(PageStyle::default().header.rule(100.0), None);
}

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
fn bundled_emoji_definition_is_marked_as_the_emoji_face() {
	for dark in [false, true] {
		assert!(Stylesheet::bundled(dark).fontdefs["emoji"].emoji);
	}
	assert!(Stylesheet::bundled_print().fontdefs["emoji"].emoji);
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
		"format_version=2\nversion=1\n[mermaid]\nprimarycolor='#ffffff'",
		"format_version=2\nversion=1\n[mermaid]\ntheme='solarized'",
		"format_version=2\nversion=1\n[mermaid]\nbackground='white'",
		"format_version=2\nversion=1\n[mermaid]\nfont_size=0.0",
		"format_version=2\nversion=1\n[mermaid]\nfont_family=[]",
		"format_version=2\nversion=1\n[mermaid]\nfont_family=['  ']",
		"format_version=2\nversion=1\n[mermaid]\nfont_family=['a,b']",
		"format_version=2\nversion=1\n[mermaid]\nfont_family='serif'",
		"format_version=2\nversion=1\n[mermaid]\ngit_colors=['#000000']",
		"format_version=2\nversion=1\n[mermaid]\npie_opacity=2.0",
		"format_version=2\nversion=1\n[svg.generic_font_family]\nunknown=['serif']",
		"format_version=2\nversion=1\n[svg.generic_font_family]\nserif=[]",
		"format_version=2\nversion=1\n[svg.generic_font_family]\nserif=['a,b']",
	] {
		assert!(Stylesheet::parse(bad).is_err(), "{bad}");
	}
}
#[test]
fn mermaid_table_names_a_preset_and_merges_field_by_field() {
	let bare = Stylesheet::parse("format_version=2\nversion=1").unwrap();
	assert_eq!(bare.mermaid.preset(), "modern");
	assert_eq!(bare.mermaid.background, None);
	let low = Stylesheet::parse(
		"format_version=2\nversion=1\n[mermaid]\ntheme='dark'\nbackground='#202630'\nprimary_text_color='#dce3ed'",
	)
	.unwrap();
	let high = Stylesheet::parse(
		"format_version=2\nversion=1\n[mermaid]\nbackground='#101418'",
	)
	.unwrap();
	let mut sheet = (*Stylesheet::builtin()).clone();
	sheet.merge(&low);
	sheet.merge(&high);
	// The leftmost selected stylesheet wins per field, as rules do.
	assert_eq!(sheet.mermaid.preset(), "dark");
	assert_eq!(sheet.mermaid.background, Some(Color(0x101418ff)));
	assert_eq!(sheet.mermaid.primary_text_color, Some(Color(0xdce3edff)));
	assert_ne!(low.mermaid, high.mermaid);
	assert_eq!(
		Stylesheet::parse(
			"format_version=2\nversion=1\n[mermaid]\ntheme='DARK'"
		)
		.unwrap()
		.mermaid
		.preset(),
		"DARK"
	);
}

#[test]
fn svg_generic_families_parse_resolve_and_merge_by_name() {
	let low = Stylesheet::parse(
		"format_version=2\nversion=1\n[svg.generic_font_family]\nserif=['serif']\nsans-serif=['sans-serif']",
	)
	.unwrap();
	let high = Stylesheet::parse(
		"format_version=2\nversion=1\n[svg.generic_font_family]\nserif=['sans-serif']",
	)
	.unwrap();
	let mut sheet = low.clone();
	sheet.merge(&high);
	assert_eq!(
		sheet.svg_generic_font_families(),
		[
			("sans-serif".into(), vec!["sans-serif".into()]),
			("serif".into(), vec!["sans-serif".into()])
		]
	);
	assert_ne!(low.diagram_key(), sheet.diagram_key());
}

#[test]
fn han_faces_follow_the_body_text_cjk_candidate() {
	let sheet = |body: &str, defs: &str| {
		let mut sheet = Stylesheet::parse(&format!(
			"format_version=2\nversion=1\n{defs}\n\
			 [[rule]]\nwhen=['body']\nfont=[{body}]"
		))
		.unwrap();
		sheet.set_cjk_type(CjkType::Sc);
		sheet
	};
	let han = sheet(
		"{family='han',weight=500},{family='emoji',weight=400}",
		"[[fontdef]]\nid='han'\ntype='SC'\nlookfor=['Songti SC','serif']\n\
		 [[fontdef]]\nid='emoji'\nemoji=true\nlookfor=['Noto Color Emoji']",
	);
	assert_eq!(han.cjk_families(), ["Songti SC", "serif"]);
	// A body that names no CJK candidate asks for no Han face, and neither
	// does one the sheet declared for another convention.
	let plain = sheet("{family='serif'}", "");
	assert!(plain.cjk_families().is_empty());
	let mut other = sheet(
		"{family='han'}",
		"[[fontdef]]\nid='han'\ntype='TC'\nlookfor=['Songti TC']",
	);
	other.set_cjk_type(CjkType::Sc);
	assert!(other.cjk_families().is_empty());
	// The Han faces are part of what a diagram is drawn from.
	assert_ne!(han.diagram_key(), plain.diagram_key());
}

#[test]
fn diagram_identity_follows_the_font_definitions_it_names() {
	let sheet = |reading: &str, aside: &str, mermaid: &str| {
		Stylesheet::parse(&format!(
			"format_version=2\nversion=1\n\
			 [[fontdef]]\nid='reading'\nlookfor=[{reading}]\n\
			 [[fontdef]]\nid='aside'\nlookfor=[{aside}]\n\
			 [mermaid]\n{mermaid}"
		))
		.unwrap()
	};
	let base = sheet(
		"'Noto Serif'",
		"'Aside'",
		"font_family=['reading', 'serif']",
	);
	assert_eq!(base.mermaid_font_families(), ["Noto Serif", "serif"]);
	// Changing the definition the table names redraws the diagram without
	// touching a single rule, so layout identity alone cannot see it.
	let edited = sheet(
		"'Source Han Serif'",
		"'Aside'",
		"font_family=['reading', 'serif']",
	);
	assert_eq!(base.layout_key(), edited.layout_key());
	assert_ne!(base.diagram_key(), edited.diagram_key());
	// A definition the table does not name is not part of the theme.
	let aside = sheet(
		"'Noto Serif'",
		"'Other Aside'",
		"font_family=['reading', 'serif']",
	);
	assert_eq!(base.diagram_key(), aside.diagram_key());
	// Everything else in the table still counts.
	let coloured = sheet(
		"'Noto Serif'",
		"'Aside'",
		"font_family=['reading', 'serif']\nbackground='#101418'",
	);
	assert_ne!(base.diagram_key(), coloured.diagram_key());
	// A definition declared but not selected contributes nothing, as it does
	// for a rule.
	let variant = Stylesheet::parse(
		"format_version=2\nversion=1\n\
		 [[fontdef]]\nid='cjk'\ntype='TC'\nlookfor=['Songti TC']\n\
		 [mermaid]\nfont_family=['cjk', 'sans-serif']",
	)
	.unwrap();
	assert_eq!(variant.mermaid_font_families(), ["sans-serif"]);
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
fn font_families_parse_metadata_and_mirrors() {
	let digest =
		"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
	let source = format!(
		"format_version=2\nversion=1\n\
		 [[font-family]]\n\
		 id='noto'\n\
		 lookfor=['Noto Serif SC','Source Han Serif SC']\n\
		 description='Reading serif'\n\
		 license='OFL-1.1'\n\
		 license_url='https://scripts.sil.org/OFL'\n\
		 homepage='https://example.invalid/noto'\n\
		 [[font-family.source]]\n\
		 name='GitHub'\n\
		 files=['https://example.invalid/a.otf',{{url='https://example.invalid/b.otf',sha256='{digest}'}}]\n\
		 [[font-family.source]]\n\
		 archives=[{{url='https://example.invalid/x.tar.gz',members=['**/*.otf']}}]\n\
		 [[font-family]]\n\
		 id='plain'\n\
		 lookfor=['Plain']\n\
		 [[font-family.source]]\n\
		 files=['http://example.invalid/plain.ttf']"
	);
	let sheet = Stylesheet::parse(&source).unwrap();
	let ids: Vec<&str> =
		sheet.font_families.iter().map(|f| f.id.as_str()).collect();
	assert_eq!(ids, ["noto", "plain"]);
	let noto = sheet.font_family("noto").unwrap();
	assert_eq!(noto.display_name(), "Noto Serif SC");
	assert_eq!(noto.description.as_deref(), Some("Reading serif"));
	assert_eq!(noto.license.as_deref(), Some("OFL-1.1"));
	assert_eq!(
		noto.license_url.as_deref(),
		Some("https://scripts.sil.org/OFL")
	);
	assert_eq!(
		noto.homepage.as_deref(),
		Some("https://example.invalid/noto")
	);
	assert_eq!(noto.source.len(), 2);
	assert_eq!(noto.source[0].label(), Some("GitHub"));
	assert_eq!(noto.source[0].files.len(), 2);
	assert_eq!(
		noto.source[0].files[0].url(),
		"https://example.invalid/a.otf"
	);
	assert_eq!(noto.source[0].files[0].sha256(), None);
	assert_eq!(noto.source[0].files[1].sha256(), Some(digest));
	assert!(noto.source[1].files.is_empty());
	assert_eq!(noto.source[1].archives[0].members, ["**/*.otf"]);
	// A family without metadata still lists under a name, and `http` is
	// allowed for a mirror that only serves it.
	let plain = sheet.font_family("plain").unwrap();
	assert_eq!(plain.display_name(), "Plain");
	assert_eq!(plain.license, None);
	assert_eq!(
		plain.source[0].files[0].url(),
		"http://example.invalid/plain.ttf"
	);
	assert!(sheet.font_family("missing").is_none());
}

#[test]
fn font_families_cascade_by_replacing_a_whole_entry() {
	let mut low = Stylesheet::parse(
		"format_version=2\nversion=1\n\
		 [[font-family]]\nid='a'\nlookfor=['A']\ndescription='low'\n\
		 [[font-family.source]]\nfiles=['https://example.invalid/1.otf','https://example.invalid/2.otf']\n\
		 [[font-family]]\nid='b'\nlookfor=['B']\n\
		 [[font-family.source]]\nfiles=['https://example.invalid/b.otf']",
	)
	.unwrap();
	let high = Stylesheet::parse(
		"format_version=2\nversion=1\n\
		 [[font-family]]\nid='a'\nlookfor=['A2']\n\
		 [[font-family.source]]\nfiles=['https://example.invalid/3.otf']",
	)
	.unwrap();
	low.merge(&high);
	assert_eq!(
		low.font_families
			.iter()
			.map(|f| f.id.as_str())
			.collect::<Vec<_>>(),
		["a", "b"]
	);
	let a = low.font_family("a").unwrap();
	assert_eq!(a.lookfor, ["A2"]);
	// A higher layer replaces the whole entry rather than merging into it.
	assert_eq!(a.description, None);
	assert_eq!(a.source[0].files.len(), 1);
	assert_eq!(low.font_family("b").unwrap().source[0].files.len(), 1);
}

#[test]
fn a_font_family_is_validated_where_it_is_declared() {
	let head = "format_version=2\nversion=1\n";
	let digest =
		"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
	for bad in [
		// `fontdef` no longer carries download information at all.
		"[[fontdef]]\nid='a'\nlookfor=['x']\nurls=['https://example.invalid/a.ttf']",
		"[[fontdef]]\nid='a'\nlookfor=['x']\nurl='https://example.invalid/a.ttf'",
		// A family needs a name list and at least one source.
		"[[font-family]]\nid='a'",
		"[[font-family]]\nid='a'\nlookfor=[]\n[[font-family.source]]\nfiles=['https://example.invalid/a.ttf']",
		"[[font-family]]\nid='a'\nlookfor=[' ']\n[[font-family.source]]\nfiles=['https://example.invalid/a.ttf']",
		"[[font-family]]\nid=''\nlookfor=['A']\n[[font-family.source]]\nfiles=['https://example.invalid/a.ttf']",
		"[[font-family]]\nid='a'\nlookfor=['A']",
		"[[font-family]]\nid='a'\nlookfor=['A']\n[[font-family.source]]\nname='empty'",
		// An id is a namespace shared with `fontdef` and unique in one sheet.
		"[[fontdef]]\nid='same'\nlookfor=['x']\n[[font-family]]\nid='same'\nlookfor=['A']\n[[font-family.source]]\nfiles=['https://example.invalid/a.ttf']",
		"[[font-family]]\nid='a'\nlookfor=['A']\n[[font-family.source]]\nfiles=['https://example.invalid/a.ttf']\n[[font-family]]\nid='a'\nlookfor=['A']\n[[font-family.source]]\nfiles=['https://example.invalid/b.ttf']",
		// A URL is absolute http(s) wherever it appears.
		"[[font-family]]\nid='a'\nlookfor=['A']\n[[font-family.source]]\nfiles=['file:///tmp/a.ttf']",
		"[[font-family]]\nid='a'\nlookfor=['A']\n[[font-family.source]]\nfiles=['/tmp/a.ttf']",
		"[[font-family]]\nid='a'\nlookfor=['A']\n[[font-family.source]]\nfiles=['https://']",
		"[[font-family]]\nid='a'\nlookfor=['A']\n[[font-family.source]]\nfiles=['https:///a.ttf']",
		"[[font-family]]\nid='a'\nlookfor=['A']\n[[font-family.source]]\nfiles=['https://example.com/a b.ttf']",
		"[[font-family]]\nid='a'\nlookfor=['A']\n[[font-family.source]]\narchives=[{url='file:///tmp/a.zip',members=['*']}]",
		// A digest is a whole SHA-256, and members are named.
		"[[font-family]]\nid='a'\nlookfor=['A']\n[[font-family.source]]\nfiles=[{url='https://example.invalid/a.ttf',sha256='ab'}]\n",
		"[[font-family]]\nid='a'\nlookfor=['A']\n[[font-family.source]]\narchives=[{url='https://example.invalid/a.zip',members=['*'],sha256='zz'}]\n",
		"[[font-family]]\nid='a'\nlookfor=['A']\n[[font-family.source]]\narchives=[{url='https://example.invalid/a.zip'}]",
		"[[font-family]]\nid='a'\nlookfor=['A']\n[[font-family.source]]\narchives=[{url='https://example.invalid/a.zip',members=[]}]",
		// Unknown fields stay errors inside both tables.
		"[[font-family]]\nid='a'\nlookfor=['A']\nsubset='SC'\n[[font-family.source]]\nfiles=['https://example.invalid/a.ttf']",
		"[[font-family]]\nid='a'\nlookfor=['A']\n[[font-family.source]]\nfiles=['https://example.invalid/a.ttf']\nformat='zip'",
	] {
		let source = format!("{head}{bad}");
		assert!(Stylesheet::parse(&source).is_err(), "{bad}");
	}
	// The digest passes where it is well formed, upper case included.
	let source = format!(
		"{head}[[font-family]]\nid='a'\nlookfor=['A']\n\
		 [[font-family.source]]\nfiles=[{{url='https://example.invalid/a.ttf',sha256='{digest}'}}]"
	);
	assert!(Stylesheet::parse(&source).is_ok());
}

#[test]
fn builtin_offers_the_curated_downloads() {
	let sheet = Stylesheet::builtin();
	let ids: Vec<&str> =
		sheet.font_families.iter().map(|f| f.id.as_str()).collect();
	assert_eq!(
		ids,
		[
			"noto-serif",
			"noto-sans",
			"noto-sans-mono",
			"noto-serif-cjk-sc",
			"noto-sans-cjk-sc",
			"noto-emoji",
			"fira-code"
		]
	);
	for family in &sheet.font_families {
		assert!(!family.lookfor.is_empty(), "{}", family.id);
		assert!(family.license.is_some(), "{}", family.id);
		assert!(family.description.is_some(), "{}", family.id);
		assert!(family.source.len() >= 2, "{}", family.id);
		for source in &family.source {
			assert!(!source.is_empty(), "{}", family.id);
			for file in &source.files {
				assert!(file.url().starts_with("https://"), "{}", file.url());
			}
		}
	}
	// The Noto families offer both CTAN faces of the same archive: the
	// canonical `mirror.ctan.org` redirector and the Tsinghua mirror, for
	// networks that cannot reach the global hosts.
	for family in sheet
		.font_families
		.iter()
		.filter(|f| f.id.starts_with("noto-"))
	{
		for host in [
			"https://mirror.ctan.org/",
			"https://mirrors.tuna.tsinghua.edu.cn/CTAN/fonts/",
		] {
			let mirrored = family
				.source
				.iter()
				.flat_map(|source| &source.files)
				.any(|file| file.url().starts_with(host));
			assert!(mirrored, "{} has no source on {host}", family.id);
		}
	}
	// Fira Code has no CTAN package: it comes from the upstream release and
	// from Arch's `ttf-fira-code` package, the same faces in a zstd tarball.
	let fira = sheet.font_family("fira-code").unwrap();
	for host in [
		"https://github.com/tonsky/FiraCode/releases/download/",
		"https://archlinux.org/packages/",
		"https://mirrors.tuna.tsinghua.edu.cn/archlinux/",
	] {
		assert!(
			fira.source
				.iter()
				.flat_map(|source| &source.archives)
				.any(|archive| archive.url.starts_with(host)),
			"fira-code has no archive on {host}"
		);
	}
	// A download entry is not a font definition: the curated ids stay out of
	// the shaping namespace.
	for family in &sheet.font_families {
		assert!(!sheet.fontdefs.contains_key(&family.id), "{}", family.id);
	}
}

#[test]
fn builtin_downloads_every_static_weight_from_every_mirror() {
	let sheet = Stylesheet::builtin();
	// A source's face names are the basenames of its files and members.
	fn basename(path: &str) -> &str {
		path.rsplit('/').next().unwrap_or(path)
	}
	let faces = |source: &FontSource| {
		let mut names: Vec<String> = source
			.files
			.iter()
			.map(|file| basename(file.url()).to_owned())
			.collect();
		for archive in &source.archives {
			names.extend(
				archive
					.members
					.iter()
					.map(|member| basename(member).to_owned()),
			);
		}
		names.sort();
		names
	};
	// The Latin families publish the nine Noto weights, each beside the italic
	// face where the family has one; Sans Mono is upright only. Every mirror
	// carries the same set.
	for (id, prefix, italic) in [
		("noto-serif", "NotoSerif", true),
		("noto-sans", "NotoSans", true),
		("noto-sans-mono", "NotoSansMono", false),
	] {
		let mut expected = Vec::new();
		for weight in [
			"Thin",
			"ExtraLight",
			"Light",
			"Regular",
			"Medium",
			"SemiBold",
			"Bold",
			"ExtraBold",
			"Black",
		] {
			expected.push(format!("{prefix}-{weight}.ttf"));
			if italic {
				// `Regular`'s italic drops the weight word.
				expected.push(if weight == "Regular" {
					format!("{prefix}-Italic.ttf")
				} else {
					format!("{prefix}-{weight}Italic.ttf")
				});
			}
		}
		expected.sort();
		let family = sheet.font_family(id).unwrap();
		assert_eq!(family.source.len(), 4, "{id}");
		for source in &family.source {
			assert_eq!(faces(source), expected, "{id}");
		}
	}
	// Each CJK family publishes all seven weights its subset release carries,
	// even though the subset mirrors and the full collections name the faces
	// differently.
	for (id, weights) in [
		(
			"noto-serif-cjk-sc",
			[
				"ExtraLight",
				"Light",
				"Regular",
				"Medium",
				"SemiBold",
				"Bold",
				"Black",
			],
		),
		(
			"noto-sans-cjk-sc",
			[
				"Thin",
				"Light",
				"DemiLight",
				"Regular",
				"Medium",
				"Bold",
				"Black",
			],
		),
	] {
		let mut expected = weights.to_vec();
		expected.sort();
		let family = sheet.font_family(id).unwrap();
		for source in &family.source {
			let names = faces(source);
			let mut got: Vec<&str> = names
				.iter()
				.map(|name| {
					let stem = name.strip_suffix(".otf").unwrap();
					stem.split_once('-').unwrap().1
				})
				.collect();
			got.sort();
			assert_eq!(got, expected, "{id}");
		}
	}
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
fn synthetic_italic_requires_a_slanted_variant() {
	let ok = Stylesheet::parse(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['em']\nfont=[{family='serif[cjk]',variant='italic',synthetic_italic=true}]",
	)
	.unwrap();
	assert!(ok.rule(Condition::Em).font.as_ref().unwrap()[0].synthetic_italic);
	// The flag only makes sense for a slanted request; a plain candidate that
	// set it would silently do nothing.
	for bad in [
		"format_version=2\nversion=1\n[[rule]]\nwhen=['em']\nfont=[{family='serif',synthetic_italic=true}]",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['em']\nfont=[{family='serif',variant='normal',synthetic_italic=true}]",
	] {
		assert!(Stylesheet::parse(bad).is_err(), "{bad}");
	}
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
fn marker_alignment_is_theme_controlled_per_marker_role() {
	let sheet = Stylesheet::parse(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['marker']\nalign='right'\n[[rule]]\nwhen=['task_marker']\nalign='left'",
	)
	.unwrap();
	assert_eq!(sheet.marker_align(false), TextAlign::Right);
	assert_eq!(sheet.marker_align(true), TextAlign::Left);
	// A sheet that says nothing keeps the historical left alignment, while the
	// bundled themes center markers in their column.
	let bare = Stylesheet::parse("format_version=2\nversion=1").unwrap();
	assert_eq!(bare.marker_align(false), TextAlign::Left);
	for dark in [false, true] {
		let bundled = Stylesheet::bundled(dark);
		assert_eq!(bundled.marker_align(false), TextAlign::Center);
		assert_eq!(bundled.marker_align(true), TextAlign::Center);
	}
	assert_eq!(
		Stylesheet::bundled_print().marker_align(false),
		TextAlign::Center
	);
	for bad in [
		"format_version=2\nversion=1\n[[rule]]\nwhen=['p']\nalign='center'",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['marker']\nalign='middle'",
	] {
		assert!(Stylesheet::parse(bad).is_err(), "{bad}");
	}
}
#[test]
fn marker_shape_is_theme_controlled() {
	let one = Stylesheet::parse(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['marker']\nshape='triangle'",
	)
	.unwrap();
	assert_eq!(one.marker_shapes(), [MarkerShape::Triangle].as_slice());
	// A list cycles by nesting depth.
	let cycle = Stylesheet::parse(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['marker']\nshape=['plus','minus']",
	)
	.unwrap();
	assert_eq!(
		cycle.marker_shapes(),
		[MarkerShape::Plus, MarkerShape::Minus].as_slice()
	);
	for (name, shape) in [
		("disc", MarkerShape::Disc),
		("square", MarkerShape::Square),
		("triangle", MarkerShape::Triangle),
		("diamond", MarkerShape::Diamond),
		("plus", MarkerShape::Plus),
		("minus", MarkerShape::Minus),
	] {
		let sheet = Stylesheet::parse(&format!(
			"format_version=2\nversion=1\n[[rule]]\nwhen=['marker']\nshape='{name}'"
		))
		.unwrap();
		assert_eq!(sheet.marker_shapes(), [shape].as_slice(), "{name}");
	}
	assert_eq!(
		Stylesheet::bundled(false).marker_shapes(),
		[MarkerShape::Disc].as_slice()
	);
	assert_eq!(
		Stylesheet::bundled_print().marker_shapes(),
		[MarkerShape::Disc].as_slice()
	);
	for bad in [
		"format_version=2\nversion=1\n[[rule]]\nwhen=['task_marker']\nshape='square'",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['marker']\nshape='star'",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['marker']\nshape=[]",
	] {
		assert!(Stylesheet::parse(bad).is_err(), "{bad}");
	}
}
#[test]
fn a_task_checkbox_takes_box_geometry_from_the_theme() {
	let sheet = Stylesheet::parse(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['task_marker']\nborder_width=2.0\nradius=3.0\naccent='#C0392B'",
	)
	.unwrap();
	let chain = chain_of(&[Condition::ListItem, Condition::TaskMarker]);
	let rule = sheet.element_rule(chain, Condition::TaskMarker);
	assert_eq!(rule.border_width, Some(2.0));
	assert_eq!(rule.radius, Some(3.0));
	assert_eq!(rule.accent, Some(crate::style::Color(0xC0392BFF)));
	assert_eq!(
		sheet.paint(crate::scene::Paint::Scoped(
			chain,
			Condition::TaskMarker,
			crate::style::ColorField::Accent,
		)),
		crate::style::Color(0xC0392BFF).rgba()
	);
	// A checkbox is not a container: it still reserves no padding.
	assert!(
		Stylesheet::parse(
			"format_version=2\nversion=1\n[[rule]]\nwhen=['task_marker']\npadding=1.0",
		)
		.is_err()
	);
}
#[test]
fn only_code_and_containers_take_padding() {
	// A code chip is the one inline run with a box of its own, so `padding`
	// names it; another inline run has no box to pad.
	let sheet = Stylesheet::parse(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['code']\npadding=[0.1,0.2,0.1,0.2]",
	)
	.unwrap();
	let chain = chain_of(&[Condition::Body, Condition::P, Condition::Code]);
	assert_eq!(
		sheet
			.element_rule(chain, Condition::Code)
			.padding
			.unwrap()
			.sides(),
		[0.1, 0.2, 0.1, 0.2]
	);
	assert!(
		Stylesheet::parse(
			"format_version=2\nversion=1\n[[rule]]\nwhen=['em']\npadding=0.2",
		)
		.is_err()
	);
}
#[test]
fn ordered_lists_take_a_theme_numbering_pattern() {
	// A sheet that says nothing numbers items "1.", "2.", ...
	let bare = Stylesheet::parse("format_version=2\nversion=1").unwrap();
	assert_eq!(bare.enum_numbering().number(0, 3), "3.");
	// One counting symbol repeats at every nesting depth.
	let alpha = Stylesheet::parse(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['enum']\nnumbering='a)'",
	)
	.unwrap();
	assert_eq!(alpha.enum_numbering().number(0, 3), "c)");
	assert_eq!(alpha.enum_numbering().number(2, 3), "c)");
	// Each level takes its own counting symbol, and the last one repeats.
	let nested = Stylesheet::parse(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['enum']\nnumbering='(1.a.*)'",
	)
	.unwrap();
	assert_eq!(nested.enum_numbering().number(0, 3), "(3)");
	assert_eq!(nested.enum_numbering().number(1, 3), "(c)");
	assert_eq!(nested.enum_numbering().number(2, 3), "(‡)");
	assert_eq!(nested.enum_numbering().number(3, 3), "(‡)");
	// The same notation reaches the numeral systems Typst knows.
	let roman = Stylesheet::parse(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['enum']\nnumbering='I.'",
	)
	.unwrap();
	assert_eq!(roman.enum_numbering().number(0, 4), "IV.");
	let circled = Stylesheet::parse(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['enum']\nnumbering='①'",
	)
	.unwrap();
	assert_eq!(circled.enum_numbering().number(0, 50), "㊿");
	// A system that cannot write the number falls back to decimal.
	assert_eq!(circled.enum_numbering().number(0, 51), "51");
	assert_eq!(alpha.enum_numbering().number(0, 0), "0)");
	for bad in [
		"format_version=2\nversion=1\n[[rule]]\nwhen=['enum']\nnumbering=''",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['enum']\nnumbering='(())'",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['marker']\nnumbering='1.'",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['enum','marker']\nnumbering='1.'",
	] {
		assert!(Stylesheet::parse(bad).is_err(), "{bad}");
	}
}
#[test]
fn a_numbering_that_grows_with_the_number_stops_at_a_marker() {
	// `999999999.` is a valid Markdown list start, and `*` repeats a symbol
	// every six items, so the marker must fall back to decimal rather than
	// spell out a label hundreds of megabytes long.
	let symbols = Stylesheet::parse(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['enum']\nnumbering='*'",
	)
	.unwrap();
	assert_eq!(symbols.enum_numbering().number(0, 7), "**");
	assert_eq!(symbols.enum_numbering().number(0, 999_999_999), "999999999");
	// Additive systems repeat their largest numeral, so Hebrew and Roman grow
	// the same way and must stop at the same bound.
	let hebrew = Stylesheet::parse(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['enum']\nnumbering='א'",
	)
	.unwrap();
	assert_eq!(hebrew.enum_numbering().number(0, 3), "ג");
	let roman = Stylesheet::parse(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['enum']\nnumbering='i.'",
	)
	.unwrap();
	assert_eq!(roman.enum_numbering().number(0, 4), "iv.");
	assert_eq!(roman.enum_numbering().number(0, 999_999_999), "999999999.");
	for sheet in [&symbols, &hebrew, &roman] {
		assert!(
			sheet.enum_numbering().number(0, u64::MAX).len() <= 24,
			"a numeral cannot outgrow a marker"
		);
	}
}
#[test]
fn ordered_numbers_align_independently_of_bullets() {
	let sheet = Stylesheet::parse(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['marker']\nalign='center'\n[[rule]]\nwhen=['enum']\nalign='right'",
	)
	.unwrap();
	assert_eq!(sheet.enum_align(), TextAlign::Right);
	assert_eq!(sheet.marker_align(false), TextAlign::Center);
	// Without an `enum` alignment the number follows the shared marker one.
	let shared = Stylesheet::parse(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['marker']\nalign='center'",
	)
	.unwrap();
	assert_eq!(shared.enum_align(), TextAlign::Center);
	assert_eq!(
		Stylesheet::parse("format_version=2\nversion=1")
			.unwrap()
			.enum_align(),
		TextAlign::Left
	);
	for dark in [false, true] {
		assert_eq!(Stylesheet::bundled(dark).enum_align(), TextAlign::Center);
	}
	assert_eq!(Stylesheet::bundled_print().enum_align(), TextAlign::Center);
	for bad in [
		"format_version=2\nversion=1\n[[rule]]\nwhen=['enum']\nalign='middle'",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['p']\nalign='center'",
	] {
		assert!(Stylesheet::parse(bad).is_err(), "{bad}");
	}
}
#[test]
fn a_numbering_pattern_changes_layout_identity() {
	let mut sheet = (*Stylesheet::bundled(false)).clone();
	let key = sheet.layout_key();
	sheet.merge(
		&Stylesheet::parse(
			"format_version=2\nversion=2\n[[rule]]\nwhen=['enum']\nnumbering='a)'",
		)
		.unwrap(),
	);
	assert_ne!(key, sheet.layout_key());
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
		Color(0x17436cff).rgba()
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

#[test]
fn the_print_stylesheet_parses_and_names_its_paper() {
	let print = Stylesheet::bundled_print();
	assert_eq!(print.meta.name.as_deref(), Some("Print"));
	let page = print.page();
	assert_eq!(page.paper_mm(), Some((210.0, 297.0)));
	assert_eq!(page.margin_mm(), Some([22.0, 20.0, 22.0, 20.0]));
	assert_eq!(page.slots(true), ["", "", ""]);
	assert_eq!(page.slots(false), ["", "{page} / {pages}", ""]);
	// The paper is white and the body is opaque, so a page renders the same
	// whatever the reader's theme is.
	assert_eq!(
		print.rule(Condition::Page).background.unwrap().rgba()[3],
		1.0
	);
}

#[test]
fn the_page_table_overlays_field_by_field_and_validates_placeholders() {
	let mut base = (*Stylesheet::bundled_print()).clone();
	base.merge(
		&Stylesheet::parse(
			"format_version=2\nversion=1\n[page]\nlandscape=true\nfooter_left=\"h\"",
		)
		.unwrap(),
	);
	let page = base.page();
	assert_eq!(page.landscape, Some(true));
	assert_eq!(page.footer_left.as_deref(), Some("h"));
	// Untouched fields keep the print sheet's values.
	assert_eq!(page.margin_mm(), Some([22.0, 20.0, 22.0, 20.0]));
	assert_eq!(page.slots(false), ["h", "{page} / {pages}", ""]);
	for bad in [
		"format_version=2\nversion=1\n[page]\nsize=\"tabloidish\"",
		"format_version=2\nversion=1\n[page]\nsize=\"nonsense\"",
		"format_version=2\nversion=1\n[page]\nmargin=[1,2,3]",
		"format_version=2\nversion=1\n[page]\nmargin=[-4]",
		"format_version=2\nversion=1\n[page]\nfooter_center=\"{date}\"",
		"format_version=2\nversion=1\n[page]\nheader_left=\"{page\"",
	] {
		assert!(Stylesheet::parse(bad).is_err(), "{bad}");
	}
	assert!(
		Stylesheet::parse(
			"format_version=2\nversion=1\n[page]\nsize=\"letter\"\nmargin=[10,12]"
		)
		.is_ok()
	);
}

#[test]
fn page_furniture_rules_accept_only_text_fields() {
	assert!(
		Stylesheet::parse(
			"format_version=2\nversion=1\n[[rule]]\nwhen=['page']\nbackground='#FFFFFF'\n[[rule]]\nwhen=['page_number']\nsize=0.5\ncolor='#000000'"
		)
		.is_ok()
	);
	for bad in [
		"format_version=2\nversion=1\n[[rule]]\nwhen=['page']\ncolor='#000000'",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['page_footer']\nbackground='#000000'",
	] {
		assert!(Stylesheet::parse(bad).is_err(), "{bad}");
	}
}

#[test]
fn the_print_sheet_survives_a_merge_over_the_reader_sheet() {
	// `--render --style print` merges the print sheet over the reader's light
	// sheet, while the PDF starts from the print sheet alone. The two must set
	// the same fields, or the two pipelines would disagree about one document.
	let print = Stylesheet::bundled_print();
	let mut over = (*Stylesheet::bundled(false)).clone();
	over.merge(&print);
	for (key, rule) in &print.rules {
		assert_eq!(&over.rules[key], rule, "[{}]", key.display());
	}
	assert_eq!(over.page, print.page);
	// Light may hold on to reader-only conditions; no export draws those.
	for key in over
		.rules
		.keys()
		.filter(|key| !print.rules.contains_key(*key))
	{
		let reader_only = key.ui()
			|| key.contains(Condition::Selection)
			|| key.contains(Condition::Scrollbar)
			|| key.contains(Condition::Hover);
		assert!(reader_only, "[{}] reaches the export", key.display());
	}
}

#[test]
fn targets_are_an_optional_nonempty_set_of_known_destinations() {
	let prefix = "format_version=2\nversion=1\n";
	for (declaration, expected) in [
		("", vec![StyleTarget::Ui, StyleTarget::Pdf]),
		("targets=['ui']", vec![StyleTarget::Ui]),
		("targets=['pdf']", vec![StyleTarget::Pdf]),
		(
			"targets=['pdf','ui']",
			vec![StyleTarget::Pdf, StyleTarget::Ui],
		),
	] {
		assert_eq!(
			Stylesheet::parse(&format!("{prefix}{declaration}"))
				.unwrap()
				.targets,
			expected
		);
	}
	for bad in [
		"targets=[]",
		"targets=['ui','ui']",
		"targets=['both']",
		"targets=['web']",
		"targets='ui'",
		"targets=[1]",
		"target='ui'",
	] {
		assert!(
			Stylesheet::parse(&format!("{prefix}{bad}")).is_err(),
			"{bad}"
		);
	}
}

#[test]
fn details_and_summary_conditions_are_styleable() {
	let sheet = Stylesheet::parse(
		"format_version=2\nversion=1\n\
		 [[rule]]\nwhen=['details']\nbackground='#f0f0f0'\npadding=0.5\nborder_width=1.0\nradius=4.0\nspace_before=0.4\nspace_after=0.4\n\
		 [[rule]]\nwhen=['summary']\ncolor='#333333'\nweight=600\nsize=0.95\n\
		 [[rule]]\nwhen=['summary','hover']\ncolor='#000000'",
	)
	.unwrap();
	let details = sheet.text(&TextAppearance::default(), Condition::Details);
	let summary = sheet.text(&details, Condition::Summary);
	assert!(chain_set(summary.chain).contains(Condition::Summary));
	assert!(chain_set(summary.chain).contains(Condition::Details));
	let rule = sheet.element_rule(details.chain, Condition::Details);
	assert_eq!(rule.padding, Some(Padding::All(0.5)));
	assert_eq!(rule.radius, Some(4.0));
	assert_eq!(rule.border_width, Some(1.0));
	assert_eq!(summary.weight, 600);
	assert_eq!(summary.size, 0.95);
	assert_eq!(
		sheet.resolve(summary.chain, ColorField::Color),
		Color(0x333333ff).rgba()
	);
	// Unknown or container-only fields stay errors under both conditions.
	for bad in [
		"format_version=2\nversion=1\n[[rule]]\nwhen=['details']\ntrack='#000000'",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['details']\nshape='disc'",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['summary']\npadding=1.0",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['summary']\ntrack='#000000'",
		"format_version=2\nversion=1\n[[rule]]\nwhen=['summary']\nnumbering='1.'",
	] {
		assert!(Stylesheet::parse(bad).is_err(), "{bad}");
	}
}
