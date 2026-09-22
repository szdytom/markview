//! Markview Stylesheet v2: strict parsing, field-wise cascading and semantic text styles.
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::BTreeMap;

use super::{
	CjkType, Condition, ConditionSet, FontDefinition, FontFamily, MermaidStyle,
	Metadata, PageStyle, Rule, StyleTarget, Stylesheet, SvgStyle,
};
impl Stylesheet {
	pub fn parse(source: &str) -> Result<Self> {
		let mut doc = source.parse::<toml_edit::DocumentMut>()?;
		let format_version =
			doc.remove("format_version").and_then(|v| v.as_integer());
		if format_version != Some(2) {
			bail!(
				"format_version: expected stylesheet format version = 2 (format 1 is no longer read)"
			);
		}
		let version = doc
			.remove("version")
			.and_then(|v| v.as_integer())
			.context("version: expected a nonnegative integer")?;
		let version = u64::try_from(version)
			.context("version: expected a nonnegative integer")?;
		let mut targets = vec![StyleTarget::Ui, StyleTarget::Pdf];
		if let Some(value) = doc.remove("targets") {
			let values = value
				.as_array()
				.context("targets: expected a nonempty array of ui, pdf")?;
			if values.is_empty() {
				bail!("targets: must not be empty");
			}
			targets.clear();
			for value in values {
				let target = match value.as_str() {
					Some("ui") => StyleTarget::Ui,
					Some("pdf") => StyleTarget::Pdf,
					_ => bail!("targets: expected ui or pdf"),
				};
				if targets.contains(&target) {
					bail!("targets: duplicate {}", target.as_str());
				}
				targets.push(target);
			}
		}
		let fontdefs = parse_fontdefs(&mut doc)?;
		let fontdef_ids: Vec<String> =
			fontdefs.keys().map(|(id, _)| id.clone()).collect();
		let font_families = parse_font_families(&mut doc, &fontdef_ids)?;
		let meta = parse_meta(&mut doc)?;
		let page = parse_page(&mut doc)?;
		let svg = parse_svg(&mut doc)?;
		let mermaid = parse_mermaid(&mut doc)?;
		let mut out = Self {
			version,
			targets,
			fontdefs: BTreeMap::new(),
			fontdef_variants: fontdefs,
			font_families,
			cjk_type: CjkType::None,
			meta,
			page,
			svg,
			mermaid,
			..Self::default()
		};
		out.resolve_fontdefs();
		if let Some(item) = doc.remove("rule") {
			let rules = item
				.as_array_of_tables()
				.context("rule: expected [[rule]] tables")?;
			for table in rules.iter() {
				let (conditions, fields) = split_rule(table)?;
				for (key, _) in fields.iter() {
					validate_field(conditions, key)?;
				}
				let name = conditions.display();
				let rule: Rule = toml_edit::de::from_str(&fields.to_string())
					.with_context(|| format!("rule [{name}]"))?;
				validate_numbers(&name, &rule)?;
				if rule.font.as_ref().is_some_and(Vec::is_empty) {
					bail!("rule [{name}].font: must not be empty");
				}
				if rule
					.shape
					.as_ref()
					.is_some_and(|shapes| shapes.cycle().is_empty())
				{
					bail!("rule [{name}].shape: must not be empty");
				}
				if let Some(fonts) = &rule.font {
					for (i, font) in fonts.iter().enumerate() {
						if font.family.trim().is_empty()
							|| font
								.weight
								.is_some_and(|w| !(1..=1000).contains(&w))
						{
							bail!(
								"rule [{name}].font[{i}]: invalid family or weight"
							);
						}
						if font.synthetic_italic
							&& font.variant == super::Variant::Normal
						{
							bail!(
								"rule [{name}].font[{i}].synthetic_italic: requires variant italic or oblique"
							);
						}
					}
				}
				if conditions == ConditionSet::of(Condition::Body)
					&& rule.background.is_some_and(|c| c.0 & 255 != 255)
				{
					bail!("rule [body].background: must be opaque");
				}
				if out.rules.insert(conditions, rule).is_some() {
					bail!("rule [{name}]: duplicate conditions");
				}
			}
		}
		if let Some((name, _)) = doc.iter().next() {
			bail!("unknown table [{name}]");
		}
		out.reindex();
		Ok(out)
	}
}

/// The `[svg]` table: generic family mappings shared by all SVG renderers.
fn parse_svg(doc: &mut toml_edit::DocumentMut) -> Result<SvgStyle> {
	let Some(item) = doc.remove("svg") else {
		return Ok(SvgStyle::default());
	};
	let mut d = toml_edit::DocumentMut::new();
	d["svg"] = item;
	#[derive(Deserialize)]
	struct S {
		svg: SvgStyle,
	}
	let svg = toml_edit::de::from_str::<S>(&d.to_string())
		.context("svg")?
		.svg;
	svg.validate().context("svg")?;
	Ok(svg)
}

fn parse_fontdefs(
	doc: &mut toml_edit::DocumentMut,
) -> Result<BTreeMap<(String, Option<super::FontDefType>), FontDefinition>> {
	let Some(item) = doc.remove("fontdef") else {
		return Ok(BTreeMap::new());
	};
	let mut d = toml_edit::DocumentMut::new();
	d["fontdef"] = item;
	#[derive(Deserialize)]
	struct D {
		fontdef: Vec<FontDefinition>,
	}
	let defs = toml_edit::de::from_str::<D>(&d.to_string())
		.context("fontdef")?
		.fontdef;
	let mut out = BTreeMap::new();
	for def in defs {
		if def.id.trim().is_empty()
			|| def.id.chars().any(char::is_control)
			|| def.lookfor.is_empty()
			|| def.lookfor.iter().any(|name| name.trim().is_empty())
		{
			bail!("fontdef {:?}: invalid id or lookfor", def.id);
		}
		let key = (def.id.clone(), def.r#type);
		if out.insert(key.clone(), def).is_some() {
			bail!("fontdef {:?} type {:?}: duplicate definition", key.0, key.1);
		}
	}
	Ok(out)
}

/// The `[[font-family]]` tables: the downloadable families a sheet offers.
///
/// A family id shares a namespace with the sheet's `fontdef` ids, because both
/// are quoted in the reader and on the command line, so one sheet may not use
/// the same name for both. Downloading is explicit and never happens here.
fn parse_font_families(
	doc: &mut toml_edit::DocumentMut,
	fontdef_ids: &[String],
) -> Result<Vec<FontFamily>> {
	let Some(item) = doc.remove("font-family") else {
		return Ok(Vec::new());
	};
	let mut d = toml_edit::DocumentMut::new();
	d["font-family"] = item;
	#[derive(Deserialize)]
	struct D {
		#[serde(rename = "font-family")]
		families: Vec<FontFamily>,
	}
	let families = toml_edit::de::from_str::<D>(&d.to_string())
		.context("font-family")?
		.families;
	let mut out: Vec<FontFamily> = Vec::with_capacity(families.len());
	for family in families {
		validate_font_family(&family, fontdef_ids)?;
		if out.iter().any(|other| other.id == family.id) {
			bail!("font-family {:?}: duplicate definition", family.id);
		}
		out.push(family);
	}
	Ok(out)
}

fn validate_font_family(
	family: &FontFamily,
	fontdef_ids: &[String],
) -> Result<()> {
	let context = || format!("font-family {:?}", family.id);
	if family.id.trim().is_empty()
		|| family.id.chars().any(char::is_control)
		|| family.lookfor.is_empty()
		|| family.lookfor.iter().any(|name| name.trim().is_empty())
	{
		bail!("{}: invalid id or lookfor", context());
	}
	if fontdef_ids.iter().any(|id| id == &family.id) {
		bail!("{}: shares its id with a fontdef", context());
	}
	if family.source.is_empty() {
		bail!("{}: needs at least one source", context());
	}
	for (source_index, source) in family.source.iter().enumerate() {
		let source_context = || format!("{}.source[{source_index}]", context());
		if source.is_empty() {
			bail!("{}: needs files or archives", source_context());
		}
		for file in &source.files {
			validate_font_url(file.url()).with_context(source_context)?;
			validate_sha256(file.sha256(), &source_context)?;
		}
		for archive in &source.archives {
			validate_font_url(&archive.url).with_context(source_context)?;
			validate_sha256(archive.sha256.as_deref(), &source_context)?;
			if archive.members.is_empty()
				|| archive.members.iter().any(|member| member.is_empty())
			{
				bail!("{}: members must not be empty", source_context());
			}
		}
	}
	Ok(())
}

/// A digest, when present, is a whole SHA-256 in hexadecimal.
fn validate_sha256(
	sha256: Option<&str>,
	context: &dyn Fn() -> String,
) -> Result<()> {
	if let Some(digest) = sha256
		&& (digest.len() != 64
			|| !digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
	{
		bail!("{}: expected a 64-digit hex sha256", context());
	}
	Ok(())
}

/// A downloadable font is an absolute `http` or `https` URL, and nothing else:
/// the reader never infers a scheme, a host, or a path.
fn validate_font_url(url: &str) -> Result<()> {
	let (scheme, rest) = url.split_once("://").unwrap_or_default();
	let host = rest
		.split(['/', '?', '#'])
		.next()
		.filter(|host| !host.is_empty());
	let scheme = scheme.eq_ignore_ascii_case("http")
		|| scheme.eq_ignore_ascii_case("https");
	if !scheme
		|| host.is_none()
		|| rest.starts_with('/')
		|| url.len() > 4096
		|| url.chars().any(|c| c.is_control() || c.is_whitespace())
	{
		bail!("expected an http(s) URL");
	}
	Ok(())
}
fn parse_meta(doc: &mut toml_edit::DocumentMut) -> Result<Metadata> {
	let Some(item) = doc.remove("meta") else {
		return Ok(Metadata::default());
	};
	let mut d = toml_edit::DocumentMut::new();
	d["meta"] = item;
	#[derive(Deserialize)]
	struct M {
		meta: Metadata,
	}
	Ok(toml_edit::de::from_str::<M>(&d.to_string())
		.context("meta")?
		.meta)
}

/// The `[page]` table. Unlike rules it holds no cascade: a merged stylesheet
/// overlays it field by field.
fn parse_page(doc: &mut toml_edit::DocumentMut) -> Result<PageStyle> {
	let Some(item) = doc.remove("page") else {
		return Ok(PageStyle::default());
	};
	let mut d = toml_edit::DocumentMut::new();
	d["page"] = item;
	#[derive(Deserialize)]
	struct P {
		page: PageStyle,
	}
	let page = toml_edit::de::from_str::<P>(&d.to_string())
		.context("page")?
		.page;
	page.validate().context("page")?;
	Ok(page)
}

/// The `[mermaid]` table. Like `[page]` it holds no cascade: a merged
/// stylesheet overlays it field by field.
fn parse_mermaid(doc: &mut toml_edit::DocumentMut) -> Result<MermaidStyle> {
	let Some(item) = doc.remove("mermaid") else {
		return Ok(MermaidStyle::default());
	};
	let mut d = toml_edit::DocumentMut::new();
	d["mermaid"] = item;
	#[derive(Deserialize)]
	struct M {
		mermaid: MermaidStyle,
	}
	let mermaid = toml_edit::de::from_str::<M>(&d.to_string())
		.context("mermaid")?
		.mermaid;
	mermaid.validate().context("mermaid")?;
	Ok(mermaid)
}

/// Split one `[[rule]]` table into its condition set and style fields.
fn split_rule(
	table: &toml_edit::Table,
) -> Result<(ConditionSet, toml_edit::DocumentMut)> {
	let mut fields = toml_edit::DocumentMut::new();
	let mut when = None;
	for (key, value) in table.iter() {
		if key == "when" {
			when = Some(parse_when(value)?);
		} else {
			if value.is_table_like() {
				bail!("rule.{key}: expected a value");
			}
			fields[key] = value.clone();
		}
	}
	let conditions = when.context("rule: missing when")?;
	if fields.is_empty() {
		bail!("rule [{}]: declares no fields", conditions.display());
	}
	Ok((conditions, fields))
}
fn parse_when(value: &toml_edit::Item) -> Result<ConditionSet> {
	let names = value
		.as_array()
		.context("rule.when: expected an array of condition names")?;
	let mut conditions = ConditionSet::EMPTY;
	for name in names.iter() {
		let name = name
			.as_str()
			.context("rule.when: expected condition names")?;
		let condition = Condition::parse(name).with_context(|| {
			format!("rule.when: unknown condition {name:?}")
		})?;
		if conditions.contains(condition) {
			bail!("rule.when: duplicate condition {name:?}");
		}
		conditions = conditions.with(condition);
	}
	if conditions.is_empty() {
		bail!("rule.when: expected at least one condition");
	}
	Ok(conditions)
}
fn validate_numbers(name: &str, rule: &Rule) -> Result<()> {
	for (field, values) in [
		(
			"border_edges",
			rule.border_edges.as_ref().map(|v| v.as_slice()),
		),
		(
			"corner_radii",
			rule.corner_radii.as_ref().map(|v| v.as_slice()),
		),
		(
			"heading_marker",
			rule.heading_marker.as_ref().map(|v| v.as_slice()),
		),
	] {
		if values.is_some_and(|v| v.iter().any(|v| !v.is_finite() || *v < 0.0))
		{
			bail!("rule [{name}].{field}: expected finite nonnegative values");
		}
	}
	if rule.letter_spacing.is_some_and(|v| !v.is_finite()) {
		bail!("rule [{name}].letter_spacing: expected a finite number");
	}
	if rule.orphans == Some(0) || rule.widows == Some(0) {
		bail!("rule [{name}]: orphans and widows must be positive integers");
	}
	for (field, value, positive) in [
		("size", rule.size, true),
		("line_height", rule.line_height, true),
		("space_before", rule.space_before, false),
		("space_after", rule.space_after, false),
		("indent", rule.indent, false),
		("border_width", rule.border_width, false),
		("radius", rule.radius, false),
		("thickness", rule.thickness, true),
		("thickness_hover", rule.thickness_hover, true),
		("overflow_thickness", rule.overflow_thickness, true),
		(
			"overflow_thickness_hover",
			rule.overflow_thickness_hover,
			true,
		),
		("gutter", rule.gutter, false),
	] {
		if value.is_some_and(|v| {
			!v.is_finite() || if positive { v <= 0. } else { v < 0. }
		}) {
			bail!(
				"rule [{name}].{field}: expected finite {}number",
				if positive {
					"positive "
				} else {
					"nonnegative "
				}
			);
		}
	}
	if rule
		.padding
		.as_ref()
		.is_some_and(|p| p.sides().iter().any(|v| !v.is_finite() || *v < 0.))
	{
		bail!("rule [{name}].padding: expected finite nonnegative values");
	}
	if rule.weight.is_some_and(|w| !(1..=1000).contains(&w)) {
		bail!("rule [{name}].weight: expected 1..1000");
	}
	Ok(())
}
fn validate_field(conditions: ConditionSet, key: &str) -> Result<()> {
	use Condition as K;
	let has = |condition| conditions.contains(condition);
	let allowed = if has(K::Scrollbar) {
		matches!(
			key,
			"track"
				| "thumb" | "thumb_hover"
				| "thickness"
				| "thickness_hover"
				| "overflow_thickness"
				| "overflow_thickness_hover"
				| "gutter"
		)
	} else if has(K::Selection) {
		key == "background"
	} else if has(K::Caption) {
		matches!(
			key,
			"source"
				| "align" | "color"
				| "font" | "weight"
				| "size" | "decoration"
				| "background"
				| "line_height"
				| "space_before"
				| "space_after"
		)
	} else if has(K::Placeholder) {
		matches!(
			key,
			"color" | "font" | "weight" | "size" | "decoration" | "background"
		)
	} else if has(K::Image) {
		matches!(
			key,
			"background"
				| "border_color"
				| "border_width"
				| "padding" | "align"
		)
	} else if has(K::Hr) {
		matches!(
			key,
			"color" | "border_width" | "space_before" | "space_after"
		)
	} else if has(K::Math) && !has(K::Error) {
		matches!(key, "color" | "size")
	} else if has(K::Page)
		&& !has(K::PageHeader)
		&& !has(K::PageFooter)
		&& !has(K::PageNumber)
	{
		key == "background"
	} else if has(K::PageHeader) || has(K::PageFooter) || has(K::PageNumber) {
		matches!(key, "color" | "font" | "weight" | "size" | "decoration")
	} else if has(K::Error) {
		matches!(
			key,
			"show"
				| "color" | "font"
				| "weight" | "size"
				| "decoration"
				| "background"
				| "line_height"
		)
	} else {
		match key {
			"wrap" => has(K::CodeBlock) && !has(K::Label),
			"show" => {
				has(K::Label) || conditions == ConditionSet::of(K::FrontMatter)
			}
			"letter_spacing" => true,
			"border_edges" | "corner_radii" => conditions.container(),
			"heading_marker" | "marker_color" => {
				[K::H1, K::H2, K::H3, K::H4, K::H5, K::H6]
					.into_iter()
					.any(has)
			}
			"orphans" | "widows" | "keep_together" => conditions.container(),
			"color" | "font" | "weight" | "decoration" => true,
			"size" => !has(K::Body),
			"background" => true,
			"line_height" | "space_before" | "space_after" => {
				conditions.has_block() || has(K::Caption)
			}
			"indent" => has(K::List) || has(K::Enum),
			"align" => {
				has(K::Marker)
					|| has(K::TaskMarker)
					|| conditions == ConditionSet::of(K::Enum)
			}
			"shape" => has(K::Marker),
			"numbering" => conditions == ConditionSet::of(K::Enum),
			// A container pads its content; an inline code run pads its chip.
			"padding" => conditions.container() || has(K::Code),
			// A task checkbox is a small box, so a theme may round it and set
			// its outline width, but it still reserves no padding.
			"border_width" | "radius" => {
				conditions.container() || has(K::TaskMarker)
			}
			"border_color" => {
				conditions.container() || conditions.ui() || has(K::TaskMarker)
			}
			"muted" | "error" => conditions.ui(),
			// A task marker's accent fills a completed checkbox.
			"accent" => conditions.ui() || has(K::TaskMarker),
			"shadow" | "scrim" => has(K::Ui),
			"hover_background" | "active_background" | "disabled_color"
			| "focus_color" => has(K::Button),
			"theme" => conditions == ConditionSet::of(K::CodeBlock),
			_ => false,
		}
	};
	if !allowed {
		bail!("rule [{}].{key}: unsupported field", conditions.display());
	}
	Ok(())
}
