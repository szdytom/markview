//! Markview Stylesheet v2: conditions, field-wise cascading and semantic text styles.
//!
//! A rule is keyed by a *set* of conditions rather than by one hierarchical
//! role name. A condition is one thing that is true of a rendered run: the
//! containing blocks, the part of a block, the inline markup and its state.
//! Combination is therefore data: `["em", "strong", "code"]` needs no new
//! vocabulary, and declaration order inside the set is irrelevant.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One condition a rendered run can satisfy.
///
/// The declaration order is the canonical bit order: a later variant is more
/// local than an earlier one, so a part follows its base (`Cell` before
/// `Header`) and every inline condition follows the blocks that contain it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Condition {
	#[default]
	Body,
	Blockquote,
	List,
	Enum,
	Table,
	Footnote,
	Details,
	CodeBlock,
	ListItem,
	Hr,
	P,
	H1,
	H2,
	H3,
	H4,
	H5,
	H6,
	Image,
	Selection,
	Scrollbar,
	Ui,
	Cell,
	Header,
	Label,
	Marker,
	TaskMarker,
	Caption,
	Placeholder,
	Summary,
	Toolbar,
	Statusbar,
	Panel,
	Button,
	Math,
	Em,
	Strong,
	Link,
	Del,
	Sup,
	FootnoteRef,
	Code,
	Error,
	Hover,
	/// The sheet of paper: only its background is significant.
	Page,
	/// Page furniture, drawn in the margins by the PDF export.
	PageHeader,
	PageFooter,
	PageNumber,
}
impl Condition {
	pub const ALL: &'static [(Self, &'static str)] = &[
		(Self::Body, "body"),
		(Self::Blockquote, "blockquote"),
		(Self::List, "list"),
		(Self::Enum, "enum"),
		(Self::Table, "table"),
		(Self::Footnote, "footnote"),
		(Self::Details, "details"),
		(Self::CodeBlock, "code_block"),
		(Self::ListItem, "list_item"),
		(Self::Hr, "hr"),
		(Self::P, "p"),
		(Self::H1, "h1"),
		(Self::H2, "h2"),
		(Self::H3, "h3"),
		(Self::H4, "h4"),
		(Self::H5, "h5"),
		(Self::H6, "h6"),
		(Self::Image, "img"),
		(Self::Selection, "selection"),
		(Self::Scrollbar, "scrollbar"),
		(Self::Ui, "ui"),
		(Self::Cell, "cell"),
		(Self::Header, "header"),
		(Self::Label, "label"),
		(Self::Marker, "marker"),
		(Self::TaskMarker, "task_marker"),
		(Self::Caption, "caption"),
		(Self::Placeholder, "placeholder"),
		(Self::Summary, "summary"),
		(Self::Toolbar, "toolbar"),
		(Self::Statusbar, "statusbar"),
		(Self::Panel, "panel"),
		(Self::Button, "button"),
		(Self::Math, "math"),
		(Self::Em, "em"),
		(Self::Strong, "strong"),
		(Self::Link, "link"),
		(Self::Del, "del"),
		(Self::Sup, "sup"),
		(Self::FootnoteRef, "footnote_ref"),
		(Self::Code, "code"),
		(Self::Error, "error"),
		(Self::Hover, "hover"),
		(Self::Page, "page"),
		(Self::PageHeader, "page_header"),
		(Self::PageFooter, "page_footer"),
		(Self::PageNumber, "page_number"),
	];
	pub fn name(self) -> &'static str {
		Self::ALL.iter().find(|(r, _)| *r == self).unwrap().1
	}
	pub fn parse(name: &str) -> Option<Self> {
		Self::ALL.iter().find(|(_, n)| *n == name).map(|(r, _)| *r)
	}
	pub const COUNT: usize = Self::ALL.len();
	pub const fn bit(self) -> u64 {
		1 << self as u32
	}
	/// The chain a single-condition paint resolves through, oldest first. A
	/// paint usually carries the exact nesting instead; this is only the default
	/// for isolated paints such as the reader chrome.
	pub fn chain(self) -> u128 {
		use Condition::*;
		match self {
			Body => chain_of(&[Body]),
			Blockquote => chain_of(&[Body, Blockquote]),
			List => chain_of(&[Body, List]),
			Enum => chain_of(&[Body, Enum]),
			ListItem => chain_of(&[Body, ListItem]),
			Table => chain_of(&[Body, Table]),
			Footnote => chain_of(&[Body, Footnote]),
			Details => chain_of(&[Body, Details]),
			Summary => chain_of(&[Body, Details, Summary]),
			CodeBlock => chain_of(&[Body, CodeBlock]),
			Hr => chain_of(&[Body, Hr]),
			P => chain_of(&[Body, P]),
			H1 | H2 | H3 | H4 | H5 | H6 => chain_of(&[Body, self]),
			Image => chain_of(&[Body, Image]),
			Ui => chain_of(&[Body, Ui]),
			Toolbar | Statusbar | Panel | Button => chain_of(&[Body, Ui, self]),
			Cell => chain_of(&[Body, Table, Cell]),
			Header => chain_of(&[Body, Table, Cell, Header]),
			Label => chain_of(&[Body, CodeBlock, Label]),
			Marker => chain_of(&[Body, ListItem, Marker]),
			TaskMarker => chain_of(&[Body, ListItem, TaskMarker]),
			Caption | Placeholder => chain_of(&[Body, Image, self]),
			Math => chain_of(&[Body, Math]),
			Em | Strong | Link | Del | Sup | FootnoteRef | Code => {
				chain_of(&[Body, self])
			}
			Error => chain_of(&[Body, Math, Error]),
			Hover => chain_of(&[Body, Link, Hover]),
			// Page furniture is set in the document's own fonts unless the
			// stylesheet says otherwise, so it inherits from the body.
			PageHeader | PageFooter | PageNumber => chain_of(&[Body, self]),
			Page | Selection | Scrollbar => chain_of(&[self]),
		}
	}
	pub fn ui(self) -> bool {
		matches!(
			self,
			Self::Ui
				| Self::Toolbar
				| Self::Statusbar
				| Self::Panel
				| Self::Button
		)
	}
	/// An inline markup condition, as opposed to a block or a block part.
	pub fn inline(self) -> bool {
		matches!(
			self,
			Self::Em
				| Self::Strong
				| Self::Link | Self::Del
				| Self::Sup | Self::FootnoteRef
				| Self::Code | Self::Math
		)
	}
	/// The conditions whose rules own this element's box: the element itself
	/// plus the specializations it is built on. Generic ancestors are excluded,
	/// so a container's geometry and background do not reach its children.
	pub fn element_path(self) -> ConditionSet {
		use Condition::*;
		match self {
			Header => ConditionSet::of(Cell).with(Header),
			Toolbar | Statusbar | Panel | Button => {
				ConditionSet::of(Ui).with(self)
			}
			Hover => ConditionSet::of(Link).with(Hover),
			_ => ConditionSet::of(self),
		}
	}
	/// A condition that carries block geometry.
	pub fn block(self) -> bool {
		matches!(
			self,
			Self::Body
				| Self::P | Self::H1
				| Self::H2 | Self::H3
				| Self::H4 | Self::H5
				| Self::H6 | Self::Blockquote
				| Self::List | Self::Enum
				| Self::ListItem
				| Self::Footnote
				| Self::Details
				| Self::CodeBlock
				| Self::Table
				| Self::Cell
		)
	}
	pub fn heading(level: u8) -> Self {
		[Self::H1, Self::H2, Self::H3, Self::H4, Self::H5, Self::H6]
			[level.clamp(1, 6) as usize - 1]
	}
}

/// A set of conditions that hold together. The empty set is the root.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConditionSet(u64);
impl ConditionSet {
	pub const EMPTY: Self = Self(0);
	pub const fn of(condition: Condition) -> Self {
		Self(condition.bit())
	}
	pub const fn with(self, condition: Condition) -> Self {
		Self(self.0 | condition.bit())
	}
	pub const fn without(self, condition: Condition) -> Self {
		Self(self.0 & !condition.bit())
	}
	pub const fn contains(self, condition: Condition) -> bool {
		self.0 & condition.bit() != 0
	}
	pub const fn is_empty(self) -> bool {
		self.0 == 0
	}
	pub const fn len(self) -> u32 {
		self.0.count_ones()
	}
	pub const fn is_subset_of(self, other: Self) -> bool {
		self.0 & !other.0 == 0
	}
	pub const fn intersects(self, other: Self) -> bool {
		self.0 & other.0 != 0
	}
	/// Whether the set names any inline markup condition.
	pub fn has_inline(self) -> bool {
		const INLINE: u64 = Condition::Em.bit()
			| Condition::Strong.bit()
			| Condition::Link.bit()
			| Condition::Del.bit()
			| Condition::Sup.bit()
			| Condition::FootnoteRef.bit()
			| Condition::Code.bit()
			| Condition::Math.bit();
		self.0 & INLINE != 0
	}
	pub const fn union(self, other: Self) -> Self {
		Self(self.0 | other.0)
	}
	pub fn iter(self) -> impl Iterator<Item = Condition> {
		Condition::ALL
			.iter()
			.map(|(c, _)| *c)
			.filter(move |c| self.contains(*c))
	}
	pub fn names(self) -> Vec<&'static str> {
		self.iter().map(Condition::name).collect()
	}
	/// The canonical spelling, for errors and cache keys.
	pub fn display(self) -> String {
		if self.is_empty() {
			return "root".into();
		}
		self.names().join("+")
	}
	pub fn ui(self) -> bool {
		self.iter().any(Condition::ui)
	}
	pub fn has_block(self) -> bool {
		self.iter().any(Condition::block)
	}
	/// A set that owns container geometry: a block without a refining part
	/// other than a table cell.
	pub fn container(self) -> bool {
		self.has_block()
			&& !self.contains(Condition::Label)
			&& !self.contains(Condition::Marker)
			&& !self.contains(Condition::TaskMarker)
			&& !self.contains(Condition::Caption)
			&& !self.contains(Condition::Placeholder)
	}
}
impl From<Condition> for ConditionSet {
	fn from(condition: Condition) -> Self {
		Self::of(condition)
	}
}

/// A condition chain packs six bits per slot into a `u128`.
pub const MAX_CHAIN: usize = 21;

/// Append a condition to an ordered chain. A repeated condition replaces its
/// earlier occurrence, which keeps deeply nested containers compact.
pub fn chain_push(chain: u128, condition: Condition) -> u128 {
	let id = condition as u128 + 1;
	let mut remaining = chain;
	let mut compact = 0;
	let mut shift = 0;
	while remaining != 0 {
		let slot = remaining & 63;
		remaining >>= 6;
		if slot != id {
			compact |= slot << shift;
			shift += 6;
		}
	}
	(compact << 6) | id
}
pub fn chain_of(conditions: &[Condition]) -> u128 {
	conditions.iter().fold(0, |chain, c| chain_push(chain, *c))
}
/// The conditions of a chain, as a set, without allocating.
pub fn chain_set(chain: u128) -> ConditionSet {
	let mut set = ConditionSet::EMPTY;
	let mut remaining = chain;
	while remaining != 0 {
		let id = (remaining & 63) as usize;
		remaining >>= 6;
		if let Some((condition, _)) =
			id.checked_sub(1).and_then(|i| Condition::ALL.get(i))
		{
			set = set.with(*condition);
		}
	}
	set
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorField {
	Color,
	Background,
	BorderColor,
	Muted,
	Accent,
	Error,
	Shadow,
	Scrim,
	Track,
	Thumb,
	ThumbHover,
	HoverBackground,
	ActiveBackground,
	DisabledColor,
	FocusColor,
}
impl ColorField {
	pub fn name(self) -> &'static str {
		match self {
			Self::Color => "color",
			Self::Background => "background",
			Self::BorderColor => "border_color",
			Self::Muted => "muted",
			Self::Accent => "accent",
			Self::Error => "error",
			Self::Shadow => "shadow",
			Self::Scrim => "scrim",
			Self::Track => "track",
			Self::Thumb => "thumb",
			Self::ThumbHover => "thumb_hover",
			Self::HoverBackground => "hover_background",
			Self::ActiveBackground => "active_background",
			Self::DisabledColor => "disabled_color",
			Self::FocusColor => "focus_color",
		}
	}
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color(pub u32);
impl Color {
	pub fn rgba(self) -> [f32; 4] {
		[
			((self.0 >> 24) & 255) as f32 / 255.,
			((self.0 >> 16) & 255) as f32 / 255.,
			((self.0 >> 8) & 255) as f32 / 255.,
			(self.0 & 255) as f32 / 255.,
		]
	}
}
impl<'de> Deserialize<'de> for Color {
	fn deserialize<D: serde::Deserializer<'de>>(
		d: D,
	) -> std::result::Result<Self, D::Error> {
		let s = String::deserialize(d)?;
		let h = s.strip_prefix('#').ok_or_else(|| {
			serde::de::Error::custom("expected #RRGGBB or #RRGGBBAA")
		})?;
		if !matches!(h.len(), 6 | 8)
			|| !h.bytes().all(|c| c.is_ascii_hexdigit())
		{
			return Err(serde::de::Error::custom(
				"expected #RRGGBB or #RRGGBBAA",
			));
		}
		let v = u32::from_str_radix(h, 16).map_err(serde::de::Error::custom)?;
		Ok(Self(if h.len() == 6 { v << 8 | 255 } else { v }))
	}
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Variant {
	#[default]
	Normal,
	Italic,
	Oblique,
}
/// The slant Markview shears a face with when it has no italic or oblique of
/// its own. 14 degrees is the angle the CSS font matching algorithm uses.
pub const SYNTHETIC_ITALIC_ANGLE_DEG: f32 = 14.0;
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Font {
	pub family: String,
	#[serde(default)]
	pub variant: Variant,
	pub weight: Option<u16>,
	/// Shear the face when it carries no italic or oblique of its own. CJK
	/// families usually ship a single upright face, so their emphasis has to
	/// be faked.
	#[serde(default)]
	pub synthetic_italic: bool,
}
#[derive(
	Clone,
	Copy,
	Debug,
	Default,
	PartialEq,
	Eq,
	PartialOrd,
	Ord,
	Hash,
	Serialize,
	Deserialize,
)]
pub enum CjkType {
	#[serde(rename = "SC")]
	Sc,
	#[serde(rename = "TC")]
	Tc,
	#[serde(rename = "JP")]
	Jp,
	#[default]
	#[serde(rename = "none")]
	None,
}
impl CjkType {
	/// The spelling a setting or a command line accepts, shared with the
	/// `serde` names so a settings file and a flag agree.
	pub fn from_name(name: &str) -> Option<Self> {
		match name.to_ascii_lowercase().as_str() {
			"sc" => Some(Self::Sc),
			"tc" => Some(Self::Tc),
			"jp" => Some(Self::Jp),
			"none" => Some(Self::None),
			_ => None,
		}
	}
}

#[derive(
	Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize,
)]
pub enum FontDefType {
	#[serde(rename = "SC")]
	Sc,
	#[serde(rename = "TC")]
	Tc,
	#[serde(rename = "JP")]
	Jp,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontDefinition {
	pub id: String,
	#[serde(default)]
	pub r#type: Option<FontDefType>,
	/// The family is the Emoji face. A cluster that asks for emoji presentation
	/// prefers it over the text candidates, however the rule orders them.
	#[serde(default)]
	pub emoji: bool,
	pub lookfor: Vec<String>,
}

/// One downloadable font family, such as Noto Sans CJK SC with its several
/// weights and subsets.
///
/// A family is independent of a [`FontDefinition`]: the definition says what a
/// short name like `serif` means, while this says how a concrete family is
/// obtained. Downloading is therefore nothing more than another `--fonts`
/// directory, and the names a rule can reach come from the font files
/// themselves. Nothing is fetched at parse or install time.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontFamily {
	/// The bookkeeping key: it selects this family on the command line and in
	/// the reader, and it is never a font name.
	pub id: String,
	/// Names the family may already report for itself, used as aliases. When
	/// any of them is available there is nothing to download.
	pub lookfor: Vec<String>,
	#[serde(default)]
	pub description: Option<String>,
	/// An SPDX identifier such as `OFL-1.1`.
	#[serde(default)]
	pub license: Option<String>,
	#[serde(default)]
	pub license_url: Option<String>,
	#[serde(default)]
	pub homepage: Option<String>,
	/// The ways to obtain the family. They are mirrors of one another: tried
	/// in order, and the first one that succeeds whole is the one used.
	pub source: Vec<FontSource>,
}
impl FontFamily {
	/// The name a list shows before the family is on disk.
	pub fn display_name(&self) -> &str {
		self.lookfor.first().map(String::as_str).unwrap_or(&self.id)
	}
}

/// One way to obtain a whole family: some direct files, some archives, or
/// both.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontSource {
	/// A label for the reader, such as the mirror's name.
	#[serde(default)]
	pub name: Option<String>,
	#[serde(default)]
	pub files: Vec<FontFile>,
	#[serde(default)]
	pub archives: Vec<FontArchive>,
}
impl FontSource {
	pub fn label(&self) -> Option<&str> {
		self.name.as_deref()
	}
	pub fn is_empty(&self) -> bool {
		self.files.is_empty() && self.archives.is_empty()
	}
}

/// One file a source downloads as it stands.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum FontFile {
	Url(String),
	Full {
		url: String,
		#[serde(default)]
		sha256: Option<String>,
	},
}
impl FontFile {
	pub fn url(&self) -> &str {
		match self {
			Self::Url(url) => url,
			Self::Full { url, .. } => url,
		}
	}
	pub fn sha256(&self) -> Option<&str> {
		match self {
			Self::Url(_) => None,
			Self::Full { sha256, .. } => sha256.as_deref(),
		}
	}
}

/// One archive a source downloads and extracts.
///
/// The container is recognized from its leading bytes, so a `.tar.gz` served
/// under a `.zip` URL still works; `members` selects what to take out, matched
/// against the member path with `/` as the separator, where `*` stops at a
/// separator, `**` crosses one, and `?` matches one character.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontArchive {
	pub url: String,
	#[serde(default)]
	pub sha256: Option<String>,
	pub members: Vec<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum Decoration {
	#[serde(rename = "underline")]
	Underline,
	#[serde(rename = "line-through")]
	Strike,
}
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Padding {
	All(f32),
	Sides([f32; 4]),
}
impl Padding {
	pub fn sides(&self) -> [f32; 4] {
		match self {
			Self::All(v) => [*v; 4],
			Self::Sides(v) => *v,
		}
	}
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptionSource {
	#[default]
	None,
	Title,
	Alt,
	TitleOrAlt,
}
impl CaptionSource {
	pub fn text(self, image: &crate::image::ImageSpec) -> Option<&str> {
		let text = match self {
			Self::None => return None,
			Self::Title => &image.title,
			Self::Alt => &image.alt,
			Self::TitleOrAlt if !image.title.trim().is_empty() => &image.title,
			Self::TitleOrAlt => &image.alt,
		};
		(!text.trim().is_empty()).then_some(text.trim())
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
	Left,
	Center,
	Right,
}
impl From<TextAlign> for crate::document::CellAlign {
	fn from(value: TextAlign) -> Self {
		match value {
			TextAlign::Left => Self::Left,
			TextAlign::Center => Self::Center,
			TextAlign::Right => Self::Right,
		}
	}
}

/// The graphic a bullet marker draws. Ordered numbers stay text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkerShape {
	#[default]
	Disc,
	Square,
	Triangle,
	Diamond,
	Plus,
	Minus,
}

/// The graphic a bullet marker draws: one shape, or a cycle indexed by the
/// bullet's nesting depth. `shape = ["plus", "minus"]` gives the first level a
/// plus and the second a minus, then repeats.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum MarkerShapes {
	One(MarkerShape),
	Many(Vec<MarkerShape>),
}
impl MarkerShapes {
	pub fn cycle(&self) -> &[MarkerShape] {
		match self {
			Self::One(shape) => std::slice::from_ref(shape),
			Self::Many(shapes) => shapes,
		}
	}
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
	pub show: Option<bool>,
	pub source: Option<CaptionSource>,
	pub align: Option<TextAlign>,
	/// The graphic, or depth-cycled graphics, a bullet marker draws instead
	/// of a text glyph.
	pub shape: Option<MarkerShapes>,
	/// How an ordered list writes its numbers. Only the `enum` condition
	/// reads it.
	pub numbering: Option<super::NumberingPattern>,
	pub color: Option<Color>,
	pub background: Option<Color>,
	pub border_color: Option<Color>,
	pub font: Option<Vec<Font>>,
	pub weight: Option<u16>,
	pub size: Option<f32>,
	pub decoration: Option<Vec<Decoration>>,
	pub line_height: Option<f32>,
	pub space_before: Option<f32>,
	pub space_after: Option<f32>,
	/// Extra indent a theme adds to a list, in base-size units.
	pub indent: Option<f32>,
	pub padding: Option<Padding>,
	pub border_width: Option<f32>,
	pub radius: Option<f32>,
	pub muted: Option<Color>,
	pub accent: Option<Color>,
	pub error: Option<Color>,
	pub shadow: Option<Color>,
	pub scrim: Option<Color>,
	pub track: Option<Color>,
	pub thumb: Option<Color>,
	pub thumb_hover: Option<Color>,
	pub thickness: Option<f32>,
	pub thickness_hover: Option<f32>,
	pub overflow_thickness: Option<f32>,
	pub overflow_thickness_hover: Option<f32>,
	pub gutter: Option<f32>,
	pub hover_background: Option<Color>,
	pub active_background: Option<Color>,
	pub disabled_color: Option<Color>,
	pub focus_color: Option<Color>,
	pub theme: Option<String>,
}
impl Rule {
	/// Whether a declaration can change layout geometry. Color-only rules must
	/// not invalidate cached layout.
	pub fn layout_relevant(&self) -> bool {
		self.show.is_some()
			|| self.source.is_some()
			|| self.align.is_some()
			|| self.shape.is_some()
			|| self.numbering.is_some()
			|| self.font.is_some()
			|| self.weight.is_some()
			|| self.size.is_some()
			|| self.decoration.is_some()
			|| self.line_height.is_some()
			|| self.space_before.is_some()
			|| self.space_after.is_some()
			|| self.indent.is_some()
			|| self.padding.is_some()
			|| self.border_width.is_some()
			|| self.radius.is_some()
			|| self.gutter.is_some()
	}
	pub fn overlay(&mut self, higher: &Self) {
		macro_rules! merge { ($($f:ident),*) => { $(if higher.$f.is_some(){self.$f=higher.$f.clone();})* }; }
		merge!(
			show,
			source,
			align,
			shape,
			numbering,
			color,
			background,
			border_color,
			font,
			weight,
			size,
			decoration,
			line_height,
			space_before,
			space_after,
			indent,
			padding,
			border_width,
			radius,
			muted,
			accent,
			error,
			shadow,
			scrim,
			track,
			thumb,
			thumb_hover,
			thickness,
			thickness_hover,
			overflow_thickness,
			overflow_thickness_hover,
			gutter,
			hover_background,
			active_background,
			disabled_color,
			focus_color,
			theme
		);
	}
	pub fn color(&self, field: ColorField) -> Option<Color> {
		match field {
			ColorField::Color => self.color,
			ColorField::Background => self.background,
			ColorField::BorderColor => self.border_color,
			ColorField::Muted => self.muted,
			ColorField::Accent => self.accent,
			ColorField::Error => self.error,
			ColorField::Shadow => self.shadow,
			ColorField::Scrim => self.scrim,
			ColorField::Track => self.track,
			ColorField::Thumb => self.thumb,
			ColorField::ThumbHover => self.thumb_hover,
			ColorField::HoverBackground => self.hover_background,
			ColorField::ActiveBackground => self.active_background,
			ColorField::DisabledColor => self.disabled_color,
			ColorField::FocusColor => self.focus_color,
		}
	}
}

/// Named paper sizes, in millimetres. An explicit `"WIDTHxHEIGHT"` (also mm)
/// is accepted wherever a name is.
pub fn parse_paper_size(name: &str) -> Option<(f32, f32)> {
	let named = match name.trim().to_ascii_lowercase().as_str() {
		"a3" => (297.0, 420.0),
		"a4" => (210.0, 297.0),
		"a5" => (148.0, 210.0),
		"a6" => (105.0, 148.0),
		"b5" => (176.0, 250.0),
		"letter" => (215.9, 279.4),
		"legal" => (215.9, 355.6),
		"tabloid" => (279.4, 431.8),
		_ => {
			let (width, height) = name.trim().split_once(['x', 'X', '*'])?;
			let width: f32 = width.trim().parse().ok()?;
			let height: f32 = height.trim().parse().ok()?;
			if !(width.is_finite() && height.is_finite())
				|| width < 20.0
				|| height < 20.0
				|| width > 2000.0
				|| height > 2000.0
			{
				return None;
			}
			return Some((width, height));
		}
	};
	Some(named)
}

/// The `[svg]` table: generic families used by SVG text.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SvgStyle {
	pub generic_font_family: BTreeMap<String, Vec<String>>,
}

impl SvgStyle {
	const GENERICS: [&'static str; 5] =
		["serif", "sans-serif", "monospace", "cursive", "fantasy"];

	pub fn overlay(&mut self, higher: &Self) {
		self.generic_font_family.extend(
			higher
				.generic_font_family
				.iter()
				.map(|(key, value)| (key.clone(), value.clone())),
		);
	}

	pub fn validate(&self) -> anyhow::Result<()> {
		for (generic, families) in &self.generic_font_family {
			if !Self::GENERICS.contains(&generic.as_str()) {
				anyhow::bail!(
					"svg.generic_font_family: unknown generic {generic:?}"
				);
			}
			if families.is_empty() {
				anyhow::bail!(
					"svg.generic_font_family.{generic}: must not be empty"
				);
			}
			for (index, family) in families.iter().enumerate() {
				if family.trim().is_empty() || family.contains(',') {
					anyhow::bail!(
						"svg.generic_font_family.{generic}[{index}]: expected a family name or a fontdef id"
					);
				}
			}
		}
		Ok(())
	}
}

/// The `[page]` table: paper, margins and page furniture. Lengths are
/// millimetres; the PDF layer converts them to points.
///
/// Every slot is a template. `{page}`, `{pages}`, `{title}` and `{path}` are
/// the supported placeholders; a slot holding a page number is styled by the
/// `page_number` condition and the rest by `page_header` or `page_footer`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageStyle {
	pub size: Option<String>,
	pub landscape: Option<bool>,
	/// Top, right, bottom, left, in millimetres: one value for all sides, two
	/// for top/bottom and left/right, or four in that order.
	pub margin: Option<Vec<f32>>,
	pub header_left: Option<String>,
	pub header_center: Option<String>,
	pub header_right: Option<String>,
	pub footer_left: Option<String>,
	pub footer_center: Option<String>,
	pub footer_right: Option<String>,
}

impl PageStyle {
	/// The margin a stylesheet that says nothing about margins gets.
	pub const DEFAULT_MARGIN_MM: [f32; 4] = [22.0, 20.0, 22.0, 20.0];
	/// The footer a stylesheet that says nothing about furniture gets.
	pub const DEFAULT_FOOTER_CENTER: &'static str = "{page} / {pages}";

	/// The paper in millimetres, after landscape is applied.
	pub fn paper_mm(&self) -> Option<(f32, f32)> {
		let (width, height) =
			parse_paper_size(self.size.as_deref().unwrap_or("a4"))?;
		Some(if self.landscape.unwrap_or(false) {
			(height, width)
		} else {
			(width, height)
		})
	}

	/// The margins in millimetres, in the canonical top/right/bottom/left
	/// order.
	pub fn margin_mm(&self) -> Option<[f32; 4]> {
		let values = self.margin.as_ref()?;
		let list: [f32; 4] = match values.as_slice() {
			[] => Self::DEFAULT_MARGIN_MM,
			[all] => [*all; 4],
			[vertical, horizontal] => {
				[*vertical, *horizontal, *vertical, *horizontal]
			}
			[top, right, bottom, left] => [*top, *right, *bottom, *left],
			_ => return None,
		};
		list.iter()
			.all(|v| v.is_finite() && *v >= 0.0)
			.then_some(list)
	}

	/// The header or footer slots, left to right. A stylesheet with no
	/// furniture at all still gets the default page number.
	pub fn slots(&self, header: bool) -> [String; 3] {
		let (left, center, right) = if header {
			(&self.header_left, &self.header_center, &self.header_right)
		} else {
			(&self.footer_left, &self.footer_center, &self.footer_right)
		};
		let empty = self.header_left.is_none()
			&& self.header_center.is_none()
			&& self.header_right.is_none()
			&& self.footer_left.is_none()
			&& self.footer_center.is_none()
			&& self.footer_right.is_none();
		let center = match center {
			Some(text) => text.clone(),
			None if !header && empty => Self::DEFAULT_FOOTER_CENTER.into(),
			None => String::new(),
		};
		[
			left.clone().unwrap_or_default(),
			center,
			right.clone().unwrap_or_default(),
		]
	}

	pub fn overlay(&mut self, higher: &Self) {
		macro_rules! merge { ($($f:ident),*) => { $(if higher.$f.is_some(){self.$f=higher.$f.clone();})* }; }
		merge!(
			size,
			landscape,
			margin,
			header_left,
			header_center,
			header_right,
			footer_left,
			footer_center,
			footer_right
		);
	}

	/// Rejects a table the PDF layer could not honour, so a broken stylesheet
	/// fails at parse time rather than at export time.
	pub fn validate(&self) -> anyhow::Result<()> {
		if self.size.is_some() && self.paper_mm().is_none() {
			anyhow::bail!(
				"page.size: expected a paper name (a4, a5, letter, legal) or WIDTHxHEIGHT in millimetres"
			);
		}
		if self.margin.is_some() && self.margin_mm().is_none() {
			anyhow::bail!(
				"page.margin: expected 1, 2 or 4 nonnegative numbers in millimetres"
			);
		}
		for (slot, text) in self.slot_templates() {
			if !crate::paginate::template_is_valid(text) {
				anyhow::bail!(
					"page.{slot}: unknown placeholder; use {{page}}, {{pages}}, {{title}} or {{path}}"
				);
			}
		}
		Ok(())
	}

	fn slot_templates(&self) -> impl Iterator<Item = (&'static str, &String)> {
		[
			("header_left", &self.header_left),
			("header_center", &self.header_center),
			("header_right", &self.header_right),
			("footer_left", &self.footer_left),
			("footer_center", &self.footer_center),
			("footer_right", &self.footer_right),
		]
		.into_iter()
		.filter_map(|(name, value)| Some((name, value.as_ref()?)))
	}
}

/// The `[mermaid]` table: how Mermaid diagrams are drawn. Each field mirrors
/// one field of the renderer's theme, and a field the table leaves out keeps
/// the value from the preset named by `theme`.
///
/// Colors are hex, so the renderer's derived palettes can be overridden but
/// not replaced by a CSS function. `git_colors`, `git_inv_colors`,
/// `git_branch_label_colors` and `pie_colors` replace a whole palette.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MermaidStyle {
	/// Built-in preset: `default`, `dark`, `forest`, `neutral` or `modern`.
	pub theme: Option<String>,
	/// Families in priority order. A name that matches a `fontdef` id is that
	/// definition's families, exactly as in a rule's `font`; any other name is
	/// a literal family.
	pub font_family: Option<Vec<String>>,
	pub font_size: Option<f32>,
	pub primary_color: Option<Color>,
	pub primary_text_color: Option<Color>,
	pub primary_border_color: Option<Color>,
	pub line_color: Option<Color>,
	pub secondary_color: Option<Color>,
	pub tertiary_color: Option<Color>,
	pub edge_label_background: Option<Color>,
	pub cluster_background: Option<Color>,
	pub cluster_border: Option<Color>,
	pub background: Option<Color>,
	pub sequence_actor_fill: Option<Color>,
	pub sequence_actor_border: Option<Color>,
	pub sequence_actor_line: Option<Color>,
	pub sequence_note_fill: Option<Color>,
	pub sequence_note_border: Option<Color>,
	pub sequence_activation_fill: Option<Color>,
	pub sequence_activation_border: Option<Color>,
	pub text_color: Option<Color>,
	pub git_colors: Option<[Color; 8]>,
	pub git_inv_colors: Option<[Color; 8]>,
	pub git_branch_label_colors: Option<[Color; 8]>,
	pub git_commit_label_color: Option<Color>,
	pub git_commit_label_background: Option<Color>,
	pub git_tag_label_color: Option<Color>,
	pub git_tag_label_background: Option<Color>,
	pub git_tag_label_border: Option<Color>,
	pub pie_colors: Option<[Color; 12]>,
	pub pie_title_text_size: Option<f32>,
	pub pie_title_text_color: Option<Color>,
	pub pie_section_text_size: Option<f32>,
	pub pie_section_text_color: Option<Color>,
	pub pie_legend_text_size: Option<f32>,
	pub pie_legend_text_color: Option<Color>,
	pub pie_stroke_color: Option<Color>,
	pub pie_stroke_width: Option<f32>,
	pub pie_outer_stroke_width: Option<f32>,
	pub pie_outer_stroke_color: Option<Color>,
	pub pie_opacity: Option<f32>,
}

impl MermaidStyle {
	/// The presets the renderer resolves by name. This is the renderer's own
	/// list; a name outside it is a typo the stylesheet should hear about.
	pub const PRESETS: &[&str] = &[
		"default", "base", "mermaid", "dark", "forest", "neutral", "modern",
	];

	/// The preset name, or the renderer's default when the table names none.
	pub fn preset(&self) -> &str {
		self.theme.as_deref().unwrap_or("modern")
	}

	pub fn overlay(&mut self, higher: &Self) {
		macro_rules! merge { ($($f:ident),*) => { $(if higher.$f.is_some(){self.$f=higher.$f.clone();})* }; }
		merge!(
			theme,
			font_family,
			font_size,
			primary_color,
			primary_text_color,
			primary_border_color,
			line_color,
			secondary_color,
			tertiary_color,
			edge_label_background,
			cluster_background,
			cluster_border,
			background,
			sequence_actor_fill,
			sequence_actor_border,
			sequence_actor_line,
			sequence_note_fill,
			sequence_note_border,
			sequence_activation_fill,
			sequence_activation_border,
			text_color,
			git_colors,
			git_inv_colors,
			git_branch_label_colors,
			git_commit_label_color,
			git_commit_label_background,
			git_tag_label_color,
			git_tag_label_background,
			git_tag_label_border,
			pie_colors,
			pie_title_text_size,
			pie_title_text_color,
			pie_section_text_size,
			pie_section_text_color,
			pie_legend_text_size,
			pie_legend_text_color,
			pie_stroke_color,
			pie_stroke_width,
			pie_outer_stroke_width,
			pie_outer_stroke_color,
			pie_opacity
		);
	}

	/// Rejects a table the renderer could not honour.
	pub fn validate(&self) -> anyhow::Result<()> {
		if let Some(name) = &self.theme
			&& !Self::PRESETS
				.contains(&name.trim().to_ascii_lowercase().as_str())
		{
			anyhow::bail!(
				"mermaid.theme: expected one of {}",
				Self::PRESETS.join(", ")
			);
		}
		if let Some(families) = &self.font_family {
			if families.is_empty() {
				anyhow::bail!("mermaid.font_family: must not be empty");
			}
			for (i, family) in families.iter().enumerate() {
				// The renderer's own list is comma separated, so a comma
				// could not survive one name.
				if family.trim().is_empty() || family.contains(',') {
					anyhow::bail!(
						"mermaid.font_family[{i}]: expected a family name or a fontdef id"
					);
				}
			}
		}
		for (field, value) in [
			("font_size", self.font_size),
			("pie_title_text_size", self.pie_title_text_size),
			("pie_section_text_size", self.pie_section_text_size),
			("pie_legend_text_size", self.pie_legend_text_size),
			("pie_stroke_width", self.pie_stroke_width),
			("pie_outer_stroke_width", self.pie_outer_stroke_width),
		] {
			if value.is_some_and(|v| !v.is_finite() || v <= 0.) {
				anyhow::bail!(
					"mermaid.{field}: expected a finite positive number"
				);
			}
		}
		if self
			.pie_opacity
			.is_some_and(|v| !v.is_finite() || !(0. ..=1.).contains(&v))
		{
			anyhow::bail!("mermaid.pie_opacity: expected a number in 0..=1");
		}
		Ok(())
	}
}
