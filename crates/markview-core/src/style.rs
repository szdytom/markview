//! Stylesheet cascading and resolved semantic appearance.
mod parse;
mod types;
use crate::{
	document::TextStyle,
	scene::{Paint, SCROLLBAR_GUTTER, ScrollbarMetrics},
};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::{
	collections::BTreeMap,
	sync::{Arc, OnceLock},
};
pub use types::{
	CaptionSource, CjkType, Color, ColorField, Condition, ConditionSet,
	Decoration, Font, FontDefType, FontDefinition, MAX_CHAIN, Padding, Rule,
	TextAlign, Variant, chain_of, chain_push, chain_set,
};

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
	pub name: Option<String>,
	pub description: Option<String>,
	pub author: Option<String>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stylesheet {
	/// Version of the theme represented by this stylesheet.
	pub version: u64,
	pub fontdefs: BTreeMap<String, FontDefinition>,
	fontdef_variants: BTreeMap<(String, Option<FontDefType>), FontDefinition>,
	cjk_type: CjkType,
	pub meta: Metadata,
	/// Rules keyed by the canonical condition set they require.
	pub rules: BTreeMap<ConditionSet, Rule>,
	/// Rule keys grouped by condition, most specific first.
	/// Rule keys grouped by condition, most specific first.
	rule_index: Vec<Vec<ConditionSet>>,
}
impl Stylesheet {
	/// Rebuild the lookup index after the rule table changes.
	fn reindex(&mut self) {
		let mut index = vec![Vec::new(); Condition::COUNT];
		for key in self.rules.keys() {
			for condition in key.iter() {
				index[condition as usize].push(*key);
			}
		}
		for keys in &mut index {
			keys.sort_by(|a, b| b.len().cmp(&a.len()).then(b.cmp(a)));
		}
		self.rule_index = index;
	}
	/// The rule declared for exactly this condition, without fallbacks.
	pub fn rule(&self, condition: Condition) -> &Rule {
		static EMPTY: OnceLock<Rule> = OnceLock::new();
		self.rules
			.get(&ConditionSet::of(condition))
			.unwrap_or_else(|| EMPTY.get_or_init(Rule::default))
	}
	/// The rule stack that owns one element's box: the rules that name the
	/// element or a specialization of it, are satisfied by `chain`, and are
	/// listed oldest first. Container geometry and backgrounds do not inherit,
	/// so a rule for an ancestor block is not part of a child's box.
	pub fn element_rule(&self, chain: u128, condition: Condition) -> Rule {
		let mut out = Rule::default();
		self.for_each_element_key(chain, condition, |key| {
			out.overlay(&self.rules[key]);
		});
		out
	}
	/// Visit the rule keys that own an element's box, in application order.
	/// Allocation-free: this runs once per box on every rendered frame.
	fn for_each_element_key(
		&self,
		chain: u128,
		condition: Condition,
		mut visit: impl FnMut(&ConditionSet),
	) {
		let path = condition.element_path();
		let mut slots = [Condition::Body; MAX_CHAIN];
		let mut count = 0;
		let mut remaining = chain;
		while remaining != 0 {
			let id = (remaining & 63) as usize;
			remaining >>= 6;
			if let Some((c, _)) =
				id.checked_sub(1).and_then(|i| Condition::ALL.get(i))
				&& count < MAX_CHAIN
			{
				slots[count] = *c;
				count += 1;
			}
		}
		// `slots` is newest first, so walk it backwards to apply oldest first.
		let mut set = ConditionSet::EMPTY;
		for c in slots[..count].iter().rev() {
			set = set.with(*c);
			let Some(keys) = self.rule_index.get(*c as usize) else {
				continue;
			};
			// `rule_index[c]` is most specific first; a rule joins here only
			// when `c` is its most recent condition.
			for key in keys.iter().rev() {
				if key.contains(*c)
					&& key.is_subset_of(set)
					&& key.intersects(path)
				{
					visit(key);
				}
			}
		}
	}
	/// Resolve one color field for an element box, matching the same rules as
	/// its geometry.
	fn resolve_scoped(
		&self,
		chain: u128,
		condition: Condition,
		field: ColorField,
	) -> [f32; 4] {
		let mut color = None;
		self.for_each_element_key(chain, condition, |key| {
			if let Some(value) = self.rules[key].color(field) {
				color = Some(value);
			}
		});
		color.unwrap_or(Color(0)).rgba()
	}
	/// Resolve one color field for an ordered chain: the most recently applied
	/// rule that declares the field wins. `inline_only` keeps a text run's
	/// background from crossing into its containers' declarations.
	fn resolve(&self, chain: u128, field: ColorField) -> [f32; 4] {
		self.resolve_filtered(chain, field, false)
	}
	/// A text run's own background: compound rules may still match through the
	/// ancestry, but a bare container or body background does not paint it.
	fn resolve_inline_background(&self, chain: u128) -> [f32; 4] {
		self.resolve_filtered(chain, ColorField::Background, true)
	}
	fn resolve_filtered(
		&self,
		chain: u128,
		field: ColorField,
		inline_only: bool,
	) -> [f32; 4] {
		let set = chain_set(chain);
		let mut remaining = chain;
		while remaining != 0 {
			let id = (remaining & 63) as usize;
			remaining >>= 6;
			let Some(condition) =
				id.checked_sub(1).and_then(|i| Condition::ALL.get(i))
			else {
				continue;
			};
			let Some(keys) = self.rule_index.get(condition.0 as usize) else {
				continue;
			};
			for key in keys {
				if (!inline_only || key.has_inline())
					&& key.is_subset_of(set)
					&& let Some(color) = self.rules[key].color(field)
				{
					return color.rgba();
				}
			}
		}
		match field {
			ColorField::Color => self
				.rule(Condition::Body)
				.color
				.unwrap_or(Color(0x262b30ff))
				.rgba(),
			// Only the bare root gets the window's fallback background; a
			// cascaded run without a background stays transparent.
			ColorField::Background
				if !inline_only && set == ConditionSet::of(Condition::Body) =>
			{
				self.rule(Condition::Body)
					.background
					.unwrap_or(Color(0xfafaf8ff))
					.rgba()
			}
			_ => Color(0).rgba(),
		}
	}
	/// Thicknesses of the reader's vertical scrollbar.
	pub fn scrollbar_metrics(&self) -> ScrollbarMetrics {
		let rule = self.rule(Condition::Scrollbar);
		ScrollbarMetrics {
			thickness: rule
				.thickness
				.unwrap_or(ScrollbarMetrics::DOCUMENT.thickness),
			thickness_hover: rule
				.thickness_hover
				.unwrap_or(ScrollbarMetrics::DOCUMENT.thickness_hover),
		}
	}
	/// Thicknesses of a wide block's horizontal scrollbar. Setting both fields
	/// to the same value disables the thickening on hover.
	pub fn overflow_scrollbar_metrics(&self) -> ScrollbarMetrics {
		let rule = self.rule(Condition::Scrollbar);
		ScrollbarMetrics {
			thickness: rule
				.overflow_thickness
				.unwrap_or(ScrollbarMetrics::OVERFLOW.thickness),
			thickness_hover: rule
				.overflow_thickness_hover
				.unwrap_or(ScrollbarMetrics::OVERFLOW.thickness_hover),
		}
	}
	/// Space an overflowing block reserves below its content for its
	/// horizontal scrollbar.
	pub fn scrollbar_gutter(&self) -> f32 {
		self.rule(Condition::Scrollbar)
			.gutter
			.unwrap_or(SCROLLBAR_GUTTER)
	}
	/// Extra indent the theme adds to a list, in base-size units. Ordered lists
	/// use the `enum` condition, so a theme can inset the two kinds independently.
	pub fn list_indent(&self, ordered: bool) -> f32 {
		let condition = if ordered {
			Condition::Enum
		} else {
			Condition::List
		};
		self.rule(condition).indent.unwrap_or(0.0).max(0.0)
	}
	pub fn merge(&mut self, higher: &Self) {
		for (key, def) in &higher.fontdef_variants {
			self.fontdef_variants.insert(key.clone(), def.clone());
		}
		self.resolve_fontdefs();
		for (conditions, v) in &higher.rules {
			self.rules.entry(*conditions).or_default().overlay(v);
		}
		self.reindex();
	}
	pub(super) fn resolve_fontdefs(&mut self) {
		let mut resolved = BTreeMap::new();
		for ((id, ty), def) in &self.fontdef_variants {
			if ty.is_none() {
				resolved.insert(id.clone(), def.clone());
			}
		}
		if self.cjk_type != CjkType::None {
			let selected = match self.cjk_type {
				CjkType::Sc => FontDefType::Sc,
				CjkType::Tc => FontDefType::Tc,
				CjkType::Jp => FontDefType::Jp,
				CjkType::None => unreachable!(),
			};
			for ((id, ty), def) in &self.fontdef_variants {
				if *ty == Some(selected) {
					resolved.insert(id.clone(), def.clone());
				}
			}
		}
		self.fontdefs = resolved;
	}
	/// Which CJK convention this stylesheet resolved its `[cjk]` font
	/// definitions with, and so which one judges its punctuation.
	pub fn cjk_type(&self) -> CjkType {
		self.cjk_type
	}
	pub fn set_cjk_type(&mut self, cjk_type: CjkType) {
		self.cjk_type = cjk_type;
		self.resolve_fontdefs();
	}
	pub fn has_fontdef_variant(&self, id: &str) -> bool {
		self.fontdef_variants
			.keys()
			.any(|(candidate, _)| candidate == id)
	}
	pub fn apply_font_overrides(
		&mut self,
		overrides: &[(String, String)],
	) -> Result<()> {
		for (id, name) in overrides {
			let def = self.fontdefs.get_mut(id).with_context(|| {
				format!("fontdef override: unknown id {id:?}")
			})?;
			if name.trim().is_empty() {
				bail!("fontdef override {id:?}: empty font name");
			}
			def.lookfor = vec![name.clone()];
		}
		Ok(())
	}
	/// Raw bundled declarations, without implicitly merging light into dark.
	pub fn bundled_rules(dark: bool) -> Arc<Self> {
		if !dark {
			return Self::bundled(false);
		}
		static DARK: OnceLock<Arc<Stylesheet>> = OnceLock::new();
		DARK.get_or_init(|| {
			Arc::new(
				Self::parse(include_str!("../styles/dark.mvss.toml"))
					.expect("bundled dark stylesheet"),
			)
		})
		.clone()
	}
	pub fn bundled(dark: bool) -> Arc<Self> {
		static LIGHT: OnceLock<Arc<Stylesheet>> = OnceLock::new();
		static DARK: OnceLock<Arc<Stylesheet>> = OnceLock::new();
		if dark {
			DARK.get_or_init(|| {
				let mut s = (*Self::bundled(false)).clone();
				s.merge(&Self::bundled_rules(true));
				Arc::new(s)
			})
			.clone()
		} else {
			LIGHT
				.get_or_init(|| {
					let sheet =
						Self::parse(include_str!("../styles/light.mvss.toml"))
							.expect("bundled light stylesheet");
					// Unit tests pin the faces they shape with, so they also
					// select the CJK definition the reader uses by default.
					// Otherwise `[cjk]` faces are dropped and CJK falls back
					// to whatever the host happens to provide.
					#[cfg(test)]
					let sheet = {
						let mut sheet = sheet;
						sheet.set_cjk_type(CjkType::Sc);
						sheet
					};
					Arc::new(sheet)
				})
				.clone()
		}
	}
	pub fn paint(&self, paint: Paint) -> [f32; 4] {
		use ColorField as C;
		use Condition as K;
		if let Paint::Cascade(chain, field) = paint {
			return if field == C::Background {
				self.resolve_inline_background(chain)
			} else {
				self.resolve(chain, field)
			};
		}
		if let Paint::Scoped(chain, condition, field) = paint {
			return self.resolve_scoped(chain, condition, field);
		}
		if let Paint::Styled(condition, field) = paint {
			return self.resolve(condition.chain(), field);
		}
		let (condition, field) = match paint {
			Paint::Cascade(..) | Paint::Scoped(..) | Paint::Styled(..) => {
				unreachable!()
			}
			Paint::Color(color) => return color.rgba(),
			Paint::Text => (K::Body, C::Color),
			Paint::Background => (K::Body, C::Background),
			Paint::Muted => (K::Ui, C::Muted),
			Paint::Accent => (K::Ui, C::Accent),
			Paint::Border => (K::Ui, C::BorderColor),
			Paint::Panel => (K::Button, C::Background),
			Paint::Glass => (K::Panel, C::Background),
			Paint::Scrim => (K::Ui, C::Scrim),
			Paint::Shadow => (K::Ui, C::Shadow),
			Paint::Error => (K::Ui, C::Error),
		};
		self.resolve(condition.chain(), field)
	}
	pub fn color(&self, condition: Condition, field: ColorField) -> [f32; 4] {
		self.resolve(condition.chain(), field)
	}
	/// Colors are resolved by the renderer; only geometry-affecting declarations invalidate layout.
	pub fn layout_key(&self) -> u64 {
		let mut s = format!("{:?}", self.cjk_type);
		for (conditions, rule) in &self.rules {
			if !rule.layout_relevant() {
				continue;
			}
			s.push_str(&conditions.display());
			s.push_str(&format!(
				"{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}",
				rule.source,
				rule.align,
				rule.show,
				rule.font,
				rule.weight,
				rule.size,
				rule.decoration,
				rule.line_height,
				rule.space_before,
				rule.space_after,
				rule.indent,
				rule.padding,
				rule.border_width,
			));
			s.push_str(&format!("{:?}{:?}", rule.radius, rule.gutter));
		}
		crate::document::fingerprint(&s)
	}
	/// Overlay the appearance fields a rule declares.
	fn apply(&self, out: &mut TextAppearance, rule: &Rule) {
		if let Some(v) = &rule.font {
			out.font = v.clone();
		}
		if let Some(v) = rule.weight {
			out.weight = v;
		}
		if let Some(v) = rule.size {
			out.size = v;
		}
		if let Some(v) = rule.line_height {
			out.line_height = v;
		}
		if let Some(v) = &rule.decoration {
			out.decoration = v.clone();
		}
	}
	/// Enter one more condition, applying every rule that completes with it.
	fn enter(
		&self,
		parent: &TextAppearance,
		condition: Condition,
	) -> TextAppearance {
		let mut out = parent.clone();
		out.chain = chain_push(parent.chain, condition);
		let set = chain_set(out.chain);
		if let Some(keys) = self.rule_index.get(condition as usize) {
			// Least specific first, so the most specific declaration wins.
			for key in keys.iter().rev() {
				if key.is_subset_of(set) {
					self.apply(&mut out, &self.rules[key]);
				}
			}
		}
		if !matches!(out.paint, Paint::Color(_)) {
			out.paint = Paint::Cascade(out.chain, ColorField::Color);
		}
		out.background =
			Some(Paint::Cascade(out.chain, ColorField::Background));
		out
	}
	pub fn text(
		&self,
		parent: &TextAppearance,
		condition: Condition,
	) -> TextAppearance {
		self.enter(parent, condition)
	}
	pub fn inline(
		&self,
		parent: &TextAppearance,
		s: &TextStyle,
	) -> TextAppearance {
		let mut out = parent.clone();
		out.size = 1.;
		out.background = None;
		for condition in s.conditions() {
			out = self.enter(&out, condition);
		}
		if let Some(color) = s.color {
			out.paint = Paint::Color(color);
		}
		out
	}
}
#[derive(Clone, Debug)]
pub struct TextAppearance {
	pub font: Vec<Font>,
	pub weight: u16,
	pub size: f32,
	pub line_height: f32,
	pub paint: Paint,
	pub background: Option<Paint>,
	pub decoration: Vec<Decoration>,
	/// Conditions entered so far, oldest first, packed six bits each.
	pub chain: u128,
}
impl Default for TextAppearance {
	fn default() -> Self {
		Self {
			font: vec![Font {
				family: "serif".into(),
				variant: Variant::Normal,
				weight: None,
			}],
			weight: 400,
			size: 1.,
			line_height: 1.65,
			paint: Paint::Styled(Condition::Body, ColorField::Color),
			background: None,
			decoration: vec![],
			chain: chain_of(&[Condition::Body]),
		}
	}
}
#[cfg(test)]
mod tests;
