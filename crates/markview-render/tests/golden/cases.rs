use markview_core::{
	layout::LayoutOptions,
	style::{CjkType, Media, MediaContext, StyleTarget, Stylesheet},
};
use std::{path::Path, sync::Arc};

pub struct Case {
	pub name: String,
	pub fixture: &'static str,
	pub width: f32,
	pub height: f32,
	pub scale: f32,
	pub theme: &'static str,
	pub cjk: CjkType,
	pub device: Media,
	pub platform: Media,
	pub variant: &'static str,
	pub disclosures: Option<&'static [bool]>,
	pub rules: String,
}

impl Case {
	fn new(fixture: &'static str, height: f32) -> Self {
		Self {
			name: fixture.into(),
			fixture,
			width: 560.,
			height,
			scale: 1.,
			theme: "light",
			cjk: CjkType::Sc,
			device: Media::Desktop,
			platform: Media::Linux,
			variant: "",
			disclosures: None,
			rules: String::new(),
		}
	}

	pub fn options(&self, root: &Path) -> anyhow::Result<LayoutOptions> {
		let pdf = Stylesheet::PDF_THEMES.contains(&self.theme);
		let target = if pdf {
			StyleTarget::Pdf
		} else {
			StyleTarget::Ui
		};
		let mut sheet = (*Stylesheet::builtin()).clone();
		if pdf {
			sheet.merge(&Stylesheet::named_rules("print").unwrap());
		}
		if self.theme != "builtin" {
			sheet.merge(&Stylesheet::named_rules(self.theme).unwrap());
		}
		if self.variant.starts_with("mvss-") {
			sheet.merge(&Stylesheet::parse(&std::fs::read_to_string(
				root.join(format!("tests/fixtures/{}.mvss.toml", self.variant)),
			)?)?);
		}
		if !self.rules.is_empty() {
			sheet.merge(&Stylesheet::parse(&format!(
				"format_version=2\nversion=1\n{}",
				self.rules
			))?);
		}
		sheet.set_cjk_type(self.cjk);
		sheet.set_media(
			MediaContext::new(target, self.device, Some(self.platform))
				.with_size(self.width, self.height),
		);
		let mut options = LayoutOptions {
			width: self.width - 40.,
			stylesheet: Arc::new(sheet),
			..Default::default()
		};
		match self.variant {
			"expanded" => options.force_open = true,
			"export" => {
				options.force_open = true;
				options.hide_front_matter = true;
			}
			"ragged" => options.justify = false,
			"no-hyphenation" => options.hyphenate = false,
			"greedy" => options.greedy = true,
			"indent" => options.paragraph_indent = 2.,
			"large" => options.font_size = 24.,
			"wrap" => options.codeblock_wrap = true,
			_ => {}
		}
		Ok(options)
	}
}

pub fn cases() -> Vec<Case> {
	let mut cases = Vec::new();
	for (name, states, variant, width) in [
		("declared", None, "", 560.),
		("closed", Some(&[false, false, false, false][..]), "", 560.),
		("opened", Some(&[true, true, true, true][..]), "", 560.),
		("nested", Some(&[true, true, false, false][..]), "", 320.),
		(
			"selected",
			Some(&[true, true, true, false][..]),
			"selected",
			560.,
		),
		(
			"scrolled",
			Some(&[true, true, true, false][..]),
			"scrolled",
			320.,
		),
	] {
		let mut case = Case::new("details-flow", 2400.);
		case.name = format!("details-flow-{name}");
		case.disclosures = states;
		case.variant = variant;
		case.width = width;
		case.rules = "[[rule]]\nwhen=['details']\npadding=0.6\nborder_width=1\nborder_color='#557799'\nbackground='#eef3f8'\nradius=7\nspace_before=0.4\nspace_after=0.7\n[[rule]]\nwhen=['list_item']\npadding=0.3\nborder_width=1\nborder_color='#bb8866'\n".into();
		cases.push(case);
	}
	// Keep the original eight baselines as independent regression cases.
	for fixture in ["prose", "lists", "table", "code", "math", "images"] {
		let mut case = Case::new(fixture, 640.);
		case.cjk = CjkType::None;
		cases.push(case);
	}
	let mut narrow = Case::new("prose", 960.);
	narrow.name = "prose-narrow".into();
	narrow.width = 320.;
	narrow.cjk = CjkType::None;
	cases.push(narrow);
	let mut dark = Case::new("prose", 640.);
	dark.name = "prose-dark-125".into();
	dark.theme = "dark";
	dark.scale = 1.25;
	dark.cjk = CjkType::None;
	cases.push(dark);
	for (fixture, height) in [
		("headings", 960.),
		("inline", 800.),
		("breaks", 900.),
		("alerts", 1200.),
		("footnotes", 1000.),
		("details", 1100.),
		("combinations", 1100.),
		("math-delimiters", 720.),
		("diagnostics", 800.),
		("html", 1800.),
		("code-forms", 1200.),
		("list-forms", 1500.),
		("table-forms", 900.),
		("typography-en", 960.),
		("typography-zh", 960.),
		("overflow", 800.),
	] {
		for theme in ["light", "dark"] {
			let mut case = Case::new(fixture, height);
			case.name = format!("{fixture}-{theme}");
			case.theme = theme;
			cases.push(case);
		}
	}
	for theme in std::iter::once("builtin")
		.chain(Stylesheet::READER_THEMES.iter().copied())
		.chain(Stylesheet::PDF_THEMES.iter().copied())
	{
		for (device, width, height) in
			[(Media::Desktop, 560., 2000.), (Media::Phone, 320., 2600.)]
		{
			let mut case = Case::new("theme-sampler", height);
			case.name = format!(
				"theme-{theme}-{}",
				if device == Media::Phone {
					"phone"
				} else {
					"desktop"
				}
			);
			case.theme = theme;
			case.device = device;
			case.width = width;
			cases.push(case);
		}
	}
	for (fixture, variants) in [
		("details", &["expanded", "export"][..]),
		(
			"typography-en",
			&["ragged", "no-hyphenation", "greedy", "indent", "large"][..],
		),
		(
			"typography-zh",
			&["ragged", "greedy", "indent", "large"][..],
		),
		("overflow", &["wrap", "scrolled"][..]),
		("inline", &["selected", "hovered"][..]),
	] {
		for &variant in variants {
			let mut case = Case::new(fixture, 2200.);
			case.width = 320.;
			case.name = format!("{fixture}-{variant}");
			case.variant = variant;
			cases.push(case);
		}
	}
	for (cjk, region) in [
		(CjkType::Sc, "SC"),
		(CjkType::Tc, "TC"),
		(CjkType::Jp, "JP"),
	] {
		let mut case = Case::new("typography-zh", 1600.);
		case.name = format!("typography-zh-{}-125", region.to_lowercase());
		case.width = 320.;
		case.scale = 1.25;
		case.cjk = cjk;
		// Avoid unordered collection fallback for CJK inline code.
		case.rules = format!(
			"[[fontdef]]\nid='monospace[cjk]'\ntype='{region}'\nlookfor=['Noto Sans Mono CJK {region}']\n"
		);
		cases.push(case);
	}
	for (fixture, variant, height) in [
		("inline", "mvss-text", 1000.),
		("theme-sampler", "mvss-boxes", 2400.),
		("mvss-markers", "mvss-markers", 1000.),
		("theme-sampler", "mvss-page", 2400.),
	] {
		let mut case = Case::new(fixture, height);
		case.name = variant.into();
		case.variant = variant;
		if variant == "mvss-page" {
			case.theme = "print";
		}
		cases.push(case);
	}
	for align in ["left", "center", "right"] {
		let mut case = Case::new("mvss-markers", 1000.);
		case.name = format!("mvss-markers-{align}");
		case.variant = "mvss-markers";
		for condition in ["marker", "enum", "task_marker"] {
			case.rules +=
				&format!("[[rule]]\nwhen=['{condition}']\nalign='{align}'\n");
		}
		cases.push(case);
	}
	for source in ["title", "alt", "title_or_alt", "none"] {
		let mut case = Case::new("images", 640.);
		case.name = format!("mvss-caption-{source}");
		case.rules =
			format!("[[rule]]\nwhen=['img','caption']\nsource='{source}'\n");
		cases.push(case);
	}
	for (top, bottom) in [
		("ascender", "descender"),
		("cap-height", "baseline"),
		("x-height", "baseline"),
		("baseline", "descender"),
		("bounds", "bounds"),
	] {
		let mut case = Case::new("typography-zh", 1100.);
		case.name = format!("mvss-edges-{top}-{bottom}");
		case.rules = format!(
			"[[rule]]\nwhen=['body']\ntop_edge='{top}'\nbottom_edge='{bottom}'\nbackground_top_edge='{top}'\nbackground_bottom_edge='{bottom}'\n"
		);
		cases.push(case);
	}
	for (name, numbering) in [
		("alphabet", "a)"),
		("roman", "I."),
		("chinese", "一、"),
		("circled", "①"),
	] {
		let mut case = Case::new("mvss-markers", 1000.);
		case.name = format!("mvss-numbering-{name}");
		case.variant = "mvss-markers";
		case.rules =
			format!("[[rule]]\nwhen=['enum']\nnumbering='{numbering}'\n");
		cases.push(case);
	}
	for (fixture, role) in
		[("details", "front_matter"), ("diagnostics", "error")]
	{
		let mut case = Case::new(fixture, 1200.);
		case.name = format!("mvss-hide-{role}");
		case.rules = format!("[[rule]]\nwhen=['{role}']\nshow=false\n");
		cases.push(case);
	}
	let mut scrollbars = Case::new("overflow", 800.);
	scrollbars.name = "mvss-scrollbars".into();
	scrollbars.variant = "scrolled";
	scrollbars.width = 320.;
	scrollbars.rules = "[[rule]]\nwhen=['scrollbar']\ntrack='#B4C4AF'\nthumb='#527760'\nthumb_hover='#BB3850'\noverflow_thickness=4\noverflow_thickness_hover=10\ngutter=15".into();
	cases.push(scrollbars);
	for (platform, device) in [
		(Media::Linux, Media::Desktop),
		(Media::Windows, Media::Desktop),
		(Media::Macos, Media::Desktop),
		(Media::Web, Media::Desktop),
		(Media::Android, Media::Phone),
		(Media::Ios, Media::Phone),
		(Media::Android, Media::Tablet),
	] {
		let mut case = Case::new("mvss-media", 1000.);
		case.name = format!("media-{platform:?}-{device:?}").to_lowercase();
		case.variant = "mvss-media";
		case.platform = platform;
		case.device = device;
		if device == Media::Phone {
			case.width = 320.;
		}
		cases.push(case);
	}
	let mut landscape = Case::new("mvss-media", 640.);
	landscape.name = "media-landscape-pdf".into();
	landscape.theme = "print";
	landscape.variant = "mvss-media";
	landscape.width = 960.;
	cases.push(landscape);
	cases
}
