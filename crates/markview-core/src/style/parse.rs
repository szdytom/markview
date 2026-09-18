//! Markview Stylesheet v2: strict parsing, field-wise cascading and semantic text styles.
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::BTreeMap;

use super::{
	CjkType, Condition, ConditionSet, FontDefinition, Metadata, PageStyle,
	Rule, Stylesheet,
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
		let fontdefs = parse_fontdefs(&mut doc)?;
		let meta = parse_meta(&mut doc)?;
		let page = parse_page(&mut doc)?;
		let mut out = Self {
			version,
			fontdefs: BTreeMap::new(),
			fontdef_variants: fontdefs,
			cjk_type: CjkType::None,
			meta,
			page,
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
			"color" | "font" | "weight" | "decoration" => true,
			"size" => !has(K::Body),
			"background" => true,
			"line_height" | "space_before" | "space_after" => {
				conditions.has_block() || has(K::Caption)
			}
			"indent" => has(K::List) || has(K::Enum),
			"padding" | "border_width" | "radius" => conditions.container(),
			"border_color" => {
				conditions.container() || conditions.ui() || has(K::TaskMarker)
			}
			"muted" | "accent" | "error" => conditions.ui(),
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
