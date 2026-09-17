//! Microtypography: how much of its advance a cluster gives up or takes on
//! when its line is justified, and the East Asian conventions that decide it.
//!
//! The East Asian rules follow the W3C Requirements for Chinese Text Layout:
//! section 3.1.6.3 compresses punctuation at a line start or end, and section
//! 3.2.2 spaces CJK apart from Latin. The overall model — one adjustability per
//! cluster, spent by a single ratio, with the leftover slack shared evenly over
//! the justifiable clusters — is the one Typst uses.
use crate::{
	shaping::{Cluster, Span},
	style::CjkType,
};

/// Space may grow to `spacing_max` and shrink to `spacing_min` of its own
/// width; character-level justification may add `tracking_max` or remove
/// `-tracking_min` em from every glyph.
///
/// The defaults follow Typst's word spacing limits (two thirds to one and a
/// half) with the tracking Typst's documentation recommends for narrow
/// columns. Typst leaves tracking off until a document asks for it; Markview
/// has no per-document typography to ask with, so the recommended amount is the
/// default and the whole thing is a reader setting.
#[derive(
	Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize,
)]
#[serde(default)]
pub struct JustificationLimits {
	pub spacing_min: f32,
	pub spacing_max: f32,
	pub tracking_min: f32,
	pub tracking_max: f32,
}
impl Default for JustificationLimits {
	fn default() -> Self {
		Self {
			spacing_min: 2.0 / 3.0,
			spacing_max: 1.5,
			tracking_min: -0.01,
			tracking_max: 0.01,
		}
	}
}
impl JustificationLimits {
	/// Whether the numbers make sense as lower and upper bounds.
	pub fn is_valid(&self) -> bool {
		let all = [
			self.spacing_min,
			self.spacing_max,
			self.tracking_min,
			self.tracking_max,
		];
		all.iter().all(|v| v.is_finite())
			&& self.spacing_min > 0.0
			&& self.spacing_min <= self.spacing_max
			&& self.spacing_max <= 4.0
			&& self.tracking_min <= 0.0
			&& self.tracking_min >= -1.0
			&& self.tracking_max >= 0.0
			&& self.tracking_max <= 1.0
	}

	/// The bit patterns that identify these bounds for layout caching.
	pub(crate) fn bits(&self) -> [u32; 4] {
		[
			self.spacing_min.to_bits(),
			self.spacing_max.to_bits(),
			self.tracking_min.to_bits(),
			self.tracking_max.to_bits(),
		]
	}
}

/// The quarter em that CLReq 3.2.2 inserts between CJK and Latin. Half of it
/// stays available for compression, so a mixed line can tighten again.
const MIXED_GAP: f32 = 0.25;

/// The reader's typographic choices, as `LayoutOptions` carries them into
/// layout: how far justification may move spacing, and which CJK convention
/// decides where punctuation sits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Typography {
	pub(crate) limits: JustificationLimits,
	pub(crate) cjk: CjkType,
}
impl Default for Typography {
	fn default() -> Self {
		Self {
			limits: JustificationLimits::default(),
			cjk: CjkType::None,
		}
	}
}

/// How far a cluster's advance may be stretched or shrunk on each side.
///
/// The sides are separate because the blank half of a CJK punctuation mark lies
/// on only one of them, and which one decides whether the mark may hang into a
/// line start or hug a line end.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Adjust {
	pub(crate) stretch: (f32, f32),
	pub(crate) shrink: (f32, f32),
}
impl Adjust {
	/// The advance this cluster may gain.
	pub(crate) fn stretch(self) -> f32 {
		self.stretch.0 + self.stretch.1
	}

	/// The advance this cluster may lose.
	pub(crate) fn shrink(self) -> f32 {
		self.shrink.0 + self.shrink.1
	}
}

/// Which regional convention decides where a CJK punctuation mark sits in its
/// em box, and so which half of it is blank.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CjkPunctStyle {
	/// Mainland China, Singapore and Malaysia, following GB/T 15834.
	Gb,
	/// Taiwan, Hong Kong and Macao, where the comma-like marks are centered.
	Cns,
	/// Japan.
	Jis,
}
impl From<CjkType> for CjkPunctStyle {
	fn from(cjk: CjkType) -> Self {
		match cjk {
			// A reader who turned the CJK font variant off still reads CJK
			// text, so the punctuation keeps the common mainland convention.
			CjkType::Tc => Self::Cns,
			CjkType::Jp => Self::Jis,
			CjkType::Sc | CjkType::None => Self::Gb,
		}
	}
}

/// Which half of a full-width CJK punctuation mark's em box carries no ink.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CjkPunct {
	/// Ink on the left, blank on the right: the mark hugs a line end.
	Left,
	/// Ink on the right, blank on the left: the mark hangs into a line start.
	Right,
	/// Ink in the middle: a quarter em on either side.
	Center,
}

/// One cluster's justification metrics, with the line's end already trimmed.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Fit {
	/// The advance this cluster may gain inside the line.
	pub(crate) stretch: f32,
	/// The advance this cluster may lose.
	pub(crate) shrink: f32,
	/// Whether this cluster takes a share of the leftover slack.
	pub(crate) share: bool,
}

/// How a line is closed to its full measure.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Justify {
	/// The fraction of the natural stretchability that is spent.
	pub(crate) ratio: f32,
	/// The advance handed to each sharing cluster once the natural
	/// stretchability is exhausted.
	pub(crate) extra: f32,
}

/// Solve a line of `natural` width for the full `target` measure.
///
/// Word spaces and tracking are spent first, up to a ratio of one; whatever is
/// left over is then shared evenly over the clusters that can take it. That
/// second step is what closes a CJK line, which has no word spaces to stretch.
pub(crate) fn solve(
	natural: f32,
	target: f32,
	stretch: f32,
	shrink: f32,
	shares: usize,
) -> Justify {
	let delta = target - natural;
	if delta < 0.0 {
		return Justify {
			ratio: if shrink > 0.0 { delta / shrink } else { -1.0 },
			extra: 0.0,
		};
	}
	if stretch > 0.0 {
		let ratio = delta / stretch;
		if ratio <= 1.0 {
			return Justify { ratio, extra: 0.0 };
		}
	}
	Justify {
		ratio: if stretch > 0.0 { 1.0 } else { 0.0 },
		extra: (delta - stretch).max(0.0) / shares.max(1) as f32,
	}
}

/// The advance a line of the given totals gains from `justify`. A negative
/// ratio spends the shrinkability instead, which is what an overfull line does.
pub(crate) fn gained(
	stretch: f32,
	shrink: f32,
	shares: usize,
	justify: Justify,
) -> f32 {
	if justify.ratio < 0.0 {
		justify.ratio * shrink
	} else {
		justify.ratio * stretch + justify.extra * shares as f32
	}
}

/// Measure every cluster of a line, in order, for justification.
///
/// `text` is the text the cluster ranges index into, which is the whole
/// paragraph rather than the line, so that a cluster at a line edge still knows
/// its neighbours.
pub(crate) fn fit(
	clusters: &[Cluster],
	text: &str,
	size: f32,
	typo: Typography,
) -> Vec<Fit> {
	let mut fits = Vec::with_capacity(clusters.len());
	for (i, cluster) in clusters.iter().enumerate() {
		let slice = text.get(cluster.range.clone()).unwrap_or("-");
		let (adjust, justifiable) = adjust(
			slice,
			cluster.width,
			size,
			cluster.mixed,
			cluster.glyphs.len(),
			typo,
		);
		// The last cluster of a line carries no slack: stretching it would add
		// space after the final glyph rather than inside the line.
		let last = i + 1 == clusters.len();
		fits.push(Fit {
			stretch: if last { 0.0 } else { adjust.stretch() },
			shrink: adjust.shrink(),
			share: !last && justifiable,
		});
	}
	fits
}

/// How far a cluster may be adjusted to justify its line, and whether it also
/// takes a share of the leftover slack when the line is underfull.
pub(crate) fn adjust(
	text: &str,
	width: f32,
	size: f32,
	mixed: (bool, bool),
	glyphs: usize,
	typo: Typography,
) -> (Adjust, bool) {
	let limits = typo.limits;
	let c = text.chars().next().unwrap_or(' ');
	if width <= 0.0 {
		return (Adjust::default(), false);
	}
	// A glyph is adjusted only when it stands alone in its cluster, so a
	// ligature or a combining sequence is never pulled apart.
	let tracking =
		|amount: f32| if glyphs <= 1 { amount.max(0.0) } else { 0.0 };
	if is_space(text) {
		// Tracking applies to a space on top of its own spacing bounds, which
		// is how a word space and the letter spacing around it add up.
		let stretch = (width * (limits.spacing_max - 1.0)).max(0.0)
			+ tracking(size * limits.tracking_max);
		let shrink = (width * (1.0 - limits.spacing_min)).max(0.0)
			+ tracking(size * -limits.tracking_min);
		return (
			Adjust {
				stretch: (0.0, stretch),
				shrink: (0.0, shrink.min(width * 0.75)),
			},
			true,
		);
	}
	if let Some(punct) = cjk_punct(c, width, size, typo.cjk.into()) {
		let adjust = match punct {
			CjkPunct::Left => Adjust {
				shrink: (0.0, width * 0.5),
				..Adjust::default()
			},
			CjkPunct::Right => Adjust {
				shrink: (width * 0.5, 0.0),
				..Adjust::default()
			},
			CjkPunct::Center => Adjust {
				shrink: (width * 0.25, width * 0.25),
				..Adjust::default()
			},
		};
		return (adjust, true);
	}
	if is_han_kana(c) || is_gb_only_punct(c) {
		// An ideograph keeps its em box, and so do the exclamation and
		// question marks that only the mainland convention lets compress. An
		// underfull CJK line is closed by the even share of the leftover slack
		// rather than by stretching the glyphs.
		let gap = size * MIXED_GAP * 0.5;
		return (
			Adjust {
				shrink: (
					if mixed.0 { gap } else { 0.0 },
					if mixed.1 { gap } else { 0.0 },
				),
				..Adjust::default()
			},
			true,
		);
	}
	// An image or a formula is an atomic box, so it is left exactly as measured.
	if c == '\u{fffc}' {
		return (Adjust::default(), false);
	}
	// A glyph may never be compressed away: as an advance approaches zero the
	// cost of the line explodes and the break search stops making progress.
	let stretch = tracking(size * limits.tracking_max);
	let shrink = tracking(size * -limits.tracking_min);
	(
		Adjust {
			stretch: (0.0, stretch),
			shrink: (0.0, shrink.min(width * 0.75)),
		},
		false,
	)
}

/// Whether `c` is a mark that only the mainland convention shortens.
fn is_gb_only_punct(c: char) -> bool {
	matches!(c, '？' | '！')
}

/// Whether `text` is a run of spaces, tabs and line breaks.
pub(crate) fn is_space(text: &str) -> bool {
	!text.is_empty()
		&& text
			.chars()
			.all(|c| c == ' ' || c == '\t' || c == '\n' || c == '\r')
}

/// Whether `c` belongs to a script written without word spaces, which is what
/// makes it justifiable on its own. Hangul is excluded: Korean is word-spaced.
pub(crate) fn is_han_kana(c: char) -> bool {
	matches!(
		c as u32,
		0x3005..=0x3007       // 々 〆 〇
			| 0x3040..=0x30ff     // Hiragana and Katakana
			| 0x3100..=0x312f     // Bopomofo
			| 0x3400..=0x4dbf     // CJK Unified Ideographs Extension A
			| 0x4e00..=0x9fff     // CJK Unified Ideographs
			| 0xf900..=0xfaff     // CJK Compatibility Ideographs
			| 0x20000..=0x3134f   // CJK Unified Ideographs Extensions B to G
	)
}

/// Whether `c` belongs to a script written with word spaces. The exact script
/// does not matter here; what matters is that a mixed CJK and Latin gap belongs
/// between the two.
pub(crate) fn is_word_spaced(c: char) -> bool {
	c.is_ascii_alphanumeric()
		|| matches!(c, '#' | '$' | '%' | '&')
		|| matches!(
			c as u32,
			0x00c0..=0x024f       // Latin letters through Latin Extended-B
				| 0x0370..=0x03ff     // Greek
				| 0x0400..=0x04ff     // Cyrillic
				| 0x1e00..=0x1eff     // Latin Extended Additional
				| 0xff10..=0xff19     // Fullwidth digits
				| 0xff21..=0xff3a     // Fullwidth Latin capitals
				| 0xff41..=0xff5a     // Fullwidth Latin small letters
		)
}

/// The blank side of `c`, when `c` is CJK punctuation set full width.
///
/// The quotation marks are shared with Latin, so they only count once the font
/// actually set them an em wide.
pub(crate) fn cjk_punct(
	c: char,
	width: f32,
	size: f32,
	style: CjkPunctStyle,
) -> Option<CjkPunct> {
	if width >= size * 0.9 {
		match c {
			'”' | '’' => return Some(CjkPunct::Left),
			'“' | '‘' => return Some(CjkPunct::Right),
			_ => {}
		}
	}
	// The comma-like marks are left aligned in the mainland and Japanese
	// conventions and centered in the Taiwanese one.
	if matches!(c, '，' | '。' | '．' | '、' | '：' | '；') {
		return Some(match style {
			CjkPunctStyle::Cns => CjkPunct::Center,
			CjkPunctStyle::Gb | CjkPunctStyle::Jis => CjkPunct::Left,
		});
	}
	if style == CjkPunctStyle::Gb && is_gb_only_punct(c) {
		return Some(CjkPunct::Left);
	}
	// CLReq A.3 and JLReq A.1 and A.2.
	if matches!(
		c,
		'》' | '）'
			| '』' | '」'
			| '】' | '〗'
			| '〕' | '〉'
			| '］' | '｝'
			| '｠' | '〙'
			| '〟'
	) {
		return Some(CjkPunct::Left);
	}
	if matches!(
		c,
		'《' | '（'
			| '『' | '「'
			| '【' | '〖'
			| '〔' | '〈'
			| '［' | '｛'
			| '｟' | '〘'
			| '〝'
	) {
		return Some(CjkPunct::Right);
	}
	if matches!(c, '\u{30fb}' | '\u{b7}') {
		return Some(CjkPunct::Center);
	}
	None
}

/// Insert the mixed CJK and Latin gap of CLReq 3.2.2.
///
/// The blank belongs to the CJK cluster, on the side facing the Latin one, so
/// that a line break between the two scripts simply drops it. No gap is
/// inserted across a change of baseline shift, because a superscript or a
/// subscript is already set apart from the text it follows.
pub(crate) fn space_mixed_scripts(
	clusters: &mut [Cluster],
	text: &str,
	spans: &[Span],
	size: f32,
) {
	let gap = size * MIXED_GAP;
	let shifts = shifts(clusters, spans);
	let mut prev: Option<(char, bool)> = None;
	for i in 0..clusters.len() {
		let c = first_char(text, &clusters[i]);
		let shifted = shifts[i];
		if is_han_kana(c) {
			let left =
				prev.is_some_and(|(c, s)| s == shifted && is_word_spaced(c));
			let right = i + 1 < clusters.len()
				&& shifts[i + 1] == shifted
				&& is_word_spaced(first_char(text, &clusters[i + 1]));
			let cluster = &mut clusters[i];
			cluster.mixed = (left, right);
			if left {
				cluster.width += gap;
				for glyph in &mut cluster.glyphs {
					glyph.x += gap;
				}
			}
			if right {
				cluster.width += gap;
			}
		}
		prev = Some((c, shifted));
	}
}

/// Whether each cluster is set as a superscript, found by walking the spans
/// alongside the clusters, both of which are in reading order.
fn shifts(clusters: &[Cluster], spans: &[Span]) -> Vec<bool> {
	let mut shifted = Vec::with_capacity(clusters.len());
	let mut cursor = 0;
	for cluster in clusters {
		while cursor < spans.len()
			&& spans[cursor].range.end <= cluster.range.start
		{
			cursor += 1;
		}
		shifted.push(spans.get(cursor).is_some_and(|span| {
			span.range.contains(&cluster.range.start) && span.style.superscript
		}));
	}
	shifted
}

/// Give back the blank half of a CJK punctuation mark at a line edge, per CLReq
/// 3.1.6.3. A mark whose ink sits right hangs into the line start; one whose ink
/// sits left hugs the line end.
pub(crate) fn compress_line_edges(
	clusters: &mut [Cluster],
	text: &str,
	size: f32,
	cjk: CjkType,
) {
	if let Some(first) = clusters.first_mut()
		&& cjk_punct(first_char(text, first), first.width, size, cjk.into())
			== Some(CjkPunct::Right)
	{
		let amount = first.width * 0.5;
		first.width -= amount;
		for glyph in &mut first.glyphs {
			glyph.x -= amount;
		}
	}
	if let Some(last) = clusters.last_mut()
		&& cjk_punct(first_char(text, last), last.width, size, cjk.into())
			== Some(CjkPunct::Left)
	{
		last.width -= last.width * 0.5;
	}
}

/// How much of a line-edge glyph may hang into the end margin, as a fraction of
/// its own advance.
///
/// A closing mark keeps most of its blank on one side, so letting that side
/// fall outside the measure is what makes a line read as flush instead of
/// stopping short of the margin. The ratios are Typst's.
const OVERHANG: &[(char, f32)] = &[
	('–', 0.2),
	('—', 0.2),
	('-', 0.55),
	('\u{ad}', 0.55),
	('.', 0.8),
	(',', 0.8),
	(';', 0.3),
	(':', 0.3),
	('\u{60c}', 0.4),
	('\u{6d4}', 0.4),
];

/// The advance the last cluster of a line gives back by hanging into the end
/// margin, or zero when nothing hangs.
///
/// A line holding a single cluster has no text for the mark to be flush with,
/// an image or a formula has no ink to hang, and inline code is set exactly as
/// written.
pub(crate) fn overhang(
	clusters: &[Cluster],
	text: &str,
	spans: &[Span],
) -> f32 {
	if clusters.len() <= 1 {
		return 0.0;
	}
	let cluster = clusters.last().unwrap();
	let Some(c) = text
		.get(cluster.range.clone())
		.and_then(|t| t.chars().next_back())
	else {
		return 0.0;
	};
	let code = spans.iter().any(|span| {
		span.range.contains(&cluster.range.start) && span.style.code
	});
	if c == '\u{fffc}' || code {
		return 0.0;
	}
	OVERHANG
		.iter()
		.find(|(mark, _)| *mark == c)
		.map_or(0.0, |(_, ratio)| ratio * cluster.width)
}

/// Whether the line may break after cluster `i` around a CJK quotation mark.
///
/// An opening quote belongs at the start of a line and a closing quote at the
/// end, so a CJK run has to be able to break before `“` and after `”`. UAX #14
/// rule LB19 forbids a break on either side of a quotation mark, which instead
/// glues a quoted phrase to the text around it and leaves a narrow CJK column
/// with no break at all. Typst gets the same result by reloading ICU with
/// U+201C and U+201D reclassified from `QU` to `OP` and `CP`, which is exactly
/// what the full-width CJK brackets already are.
pub(crate) fn quote_edge_break(
	clusters: &[Cluster],
	text: &str,
	i: usize,
) -> bool {
	if clusters
		.get(i + 1)
		.is_some_and(|c| matches!(first_char(text, c), '“' | '‘'))
	{
		return clusters
			.get(i + 2)
			.is_some_and(|c| is_han_kana(first_char(text, c)));
	}
	if matches!(first_char(text, &clusters[i]), '”' | '’') {
		return i
			.checked_sub(1)
			.is_some_and(|j| is_han_kana(first_char(text, &clusters[j])));
	}
	false
}

/// The first character `cluster` covers.
fn first_char(text: &str, cluster: &Cluster) -> char {
	text.get(cluster.range.clone())
		.and_then(|t| t.chars().next())
		.unwrap_or(' ')
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::document::TextStyle;
	use std::ops::Range;

	/// A cluster covering `range` of the paragraph, with no ink.
	fn cluster(range: Range<usize>, width: f32) -> Cluster {
		Cluster {
			rtl: false,
			range,
			width,
			mixed: (false, false),
			ascent: 0.0,
			descent: 0.0,
			glyphs: Vec::new(),
			continuation: false,
		}
	}

	/// The clusters of `text`, one per character, each an em wide.
	fn per_char(text: &str, size: f32) -> (String, Vec<Cluster>) {
		let mut clusters = Vec::new();
		for (i, c) in text.char_indices() {
			clusters.push(cluster(i..i + c.len_utf8(), size));
		}
		(text.to_owned(), clusters)
	}

	/// The default choices with a given CJK convention.
	fn typo(cjk: CjkType) -> Typography {
		Typography {
			cjk,
			..Typography::default()
		}
	}

	#[test]
	fn spaces_take_and_give_space() {
		let (space, share) =
			adjust(" ", 5.0, 18.0, (false, false), 1, typo(CjkType::Sc));
		assert!(share);
		// The defaults are Typst's: two thirds to one and a half, plus the
		// letter spacing that applies to a space as well.
		let tracking = 18.0 * JustificationLimits::default().tracking_max;
		assert!((space.stretch.1 - (5.0 * 0.5 + tracking)).abs() < 0.001);
		assert!((space.shrink.1 - (5.0 / 3.0 + tracking)).abs() < 0.001);
	}

	#[test]
	fn the_limits_decide_how_far_a_space_moves() {
		let wide = Typography {
			limits: JustificationLimits {
				spacing_min: 0.5,
				spacing_max: 2.0,
				tracking_min: 0.0,
				tracking_max: 0.0,
			},
			cjk: CjkType::Sc,
		};
		let (space, _) = adjust(" ", 10.0, 10.0, (false, false), 1, wide);
		assert_eq!(space.stretch, (0.0, 10.0));
		assert_eq!(space.shrink, (0.0, 5.0));
	}

	#[test]
	fn tracking_can_be_turned_off() {
		let limits = JustificationLimits {
			tracking_min: 0.0,
			tracking_max: 0.0,
			..Default::default()
		};
		let typo = Typography {
			limits,
			cjk: CjkType::Sc,
		};
		let (letter, share) = adjust("a", 9.0, 18.0, (false, false), 1, typo);
		assert!(!share);
		assert_eq!(letter, Adjust::default());
		// The glyph still keeps its own advance, so nothing collapses.
		let (space, _) = adjust(" ", 9.0, 18.0, (false, false), 1, typo);
		assert!(space.stretch.1 > 0.0);
	}

	#[test]
	fn latin_tracking_stays_small_and_never_erases_a_glyph() {
		let (letter, share) =
			adjust("a", 18.0, 18.0, (false, false), 1, typo(CjkType::Sc));
		assert!(!share);
		assert_eq!(letter.stretch, (0.0, 18.0 * 0.01));
		assert_eq!(letter.shrink, (0.0, 18.0 * 0.01));
		// Three quarters of the advance is the most any glyph may lose.
		let (narrow, _) =
			adjust("i", 0.05, 18.0, (false, false), 1, typo(CjkType::Sc));
		assert_eq!(narrow.shrink, (0.0, 0.05 * 0.75));
	}

	#[test]
	fn a_ligature_is_never_pulled_apart() {
		// A cluster holding more than one glyph gets no letter spacing, so a
		// ligature or a combining sequence is never separated.
		let (single, _) =
			adjust("fi", 18.0, 18.0, (false, false), 1, typo(CjkType::Sc));
		assert!(single.stretch.1 > 0.0);
		let (ligature, _) =
			adjust("fi", 18.0, 18.0, (false, false), 2, typo(CjkType::Sc));
		assert_eq!(ligature, Adjust::default());
	}

	#[test]
	fn punctuation_follows_the_regional_convention() {
		use CjkPunctStyle::*;
		// The comma-like marks are left aligned on the mainland and in Japan,
		// and centered in Taiwan.
		assert_eq!(cjk_punct('，', 18.0, 18.0, Gb), Some(CjkPunct::Left));
		assert_eq!(cjk_punct('，', 18.0, 18.0, Jis), Some(CjkPunct::Left));
		assert_eq!(cjk_punct('，', 18.0, 18.0, Cns), Some(CjkPunct::Center));
		// Only the mainland convention shortens exclamations and questions.
		assert_eq!(cjk_punct('？', 18.0, 18.0, Gb), Some(CjkPunct::Left));
		assert_eq!(cjk_punct('？', 18.0, 18.0, Cns), None);
		assert_eq!(cjk_punct('？', 18.0, 18.0, Jis), None);
		// The brackets are the same everywhere.
		for style in [Gb, Cns, Jis] {
			assert_eq!(
				cjk_punct('（', 18.0, 18.0, style),
				Some(CjkPunct::Right)
			);
			assert_eq!(
				cjk_punct('》', 18.0, 18.0, style),
				Some(CjkPunct::Left)
			);
			assert_eq!(
				cjk_punct('・', 18.0, 18.0, style),
				Some(CjkPunct::Center)
			);
		}
		// A mark that only one convention shortens still takes a share of the
		// line, which is how Typst's own justifiability test reads it.
		let (adjust, share) =
			adjust("？", 18.0, 18.0, (false, false), 1, typo(CjkType::Tc));
		assert!(share);
		assert_eq!(adjust, Adjust::default());
	}

	#[test]
	fn fullwidth_punctuation_gives_up_its_blank_half() {
		let sc = typo(CjkType::Sc);
		let (left, _) = adjust("，", 18.0, 18.0, (false, false), 1, sc);
		assert_eq!(left.shrink, (0.0, 9.0));
		let (right, _) = adjust("（", 18.0, 18.0, (false, false), 1, sc);
		assert_eq!(right.shrink, (9.0, 0.0));
		let (center, _) = adjust("・", 18.0, 18.0, (false, false), 1, sc);
		assert_eq!(center.shrink, (4.5, 4.5));
		// A shared quotation mark only counts once it is set full width.
		assert_eq!(
			cjk_punct('”', 18.0, 18.0, CjkPunctStyle::Gb),
			Some(CjkPunct::Left)
		);
		assert_eq!(cjk_punct('”', 6.0, 18.0, CjkPunctStyle::Gb), None);
		assert_eq!(cjk_punct('中', 18.0, 18.0, CjkPunctStyle::Gb), None);
	}

	#[test]
	fn cjk_keeps_its_box_and_gives_back_the_mixed_gap() {
		let (next_to_latin, share) =
			adjust("中", 18.0, 18.0, (true, false), 1, typo(CjkType::Sc));
		assert!(share);
		let gap = 18.0 * MIXED_GAP * 0.5;
		assert_eq!(next_to_latin.shrink, (gap, 0.0));
		// An ideograph never gives up its own box.
		assert_eq!(next_to_latin.stretch, (0.0, 0.0));
	}

	#[test]
	fn leftover_slack_is_shared_over_the_justifiable_clusters() {
		let solve = solve(300.0, 318.0, 0.0, 0.0, 15);
		assert_eq!(
			solve,
			Justify {
				ratio: 0.0,
				extra: 1.2
			}
		);
		assert!((gained(0.0, 0.0, 15, solve) - 18.0).abs() < 0.001);
	}

	#[test]
	fn overfull_lines_spend_their_shrinkability() {
		let solve = solve(110.0, 100.0, 0.0, 20.0, 0);
		assert_eq!(
			solve,
			Justify {
				ratio: -0.5,
				extra: 0.0
			}
		);
		assert!((gained(0.0, 20.0, 0, solve) + 10.0).abs() < 0.001);
	}

	#[test]
	fn natural_stretchability_is_spent_before_the_even_share() {
		let solve = solve(90.0, 100.0, 8.0, 0.0, 4);
		assert_eq!(
			solve,
			Justify {
				ratio: 1.0,
				extra: 0.5
			}
		);
		assert!((gained(8.0, 0.0, 4, solve) - 10.0).abs() < 0.001);
	}

	#[test]
	fn mixed_scripts_are_spaced_apart_on_the_cjk_side() {
		let (text, mut clusters) = per_char("汉a汉", 18.0);
		space_mixed_scripts(&mut clusters, &text, &[], 18.0);
		let gap = 18.0 * MIXED_GAP;
		assert_eq!(clusters[0].width, 18.0 + gap);
		assert_eq!(clusters[1].width, 18.0);
		assert_eq!(clusters[2].width, 18.0 + gap);
		assert_eq!(clusters[0].mixed, (false, true));
		assert_eq!(clusters[2].mixed, (true, false));
	}

	#[test]
	fn a_shift_change_does_not_take_the_mixed_gap() {
		let gap = 18.0 * MIXED_GAP;
		// A superscript number is already set apart from the text it follows,
		// so the gap would only add space that is not needed.
		let (text, mut clusters) = per_char("中1中", 18.0);
		let spans = vec![Span {
			range: clusters[1].range.clone(),
			style: TextStyle {
				superscript: true,
				..Default::default()
			},
		}];
		space_mixed_scripts(&mut clusters, &text, &spans, 18.0);
		assert_eq!(clusters[0].width, 18.0);
		assert_eq!(clusters[0].mixed, (false, false));

		// On the baseline the same pair does gain the gap.
		let (text, mut clusters) = per_char("中1中", 18.0);
		space_mixed_scripts(&mut clusters, &text, &[], 18.0);
		assert_eq!(clusters[0].width, 18.0 + gap);
	}

	#[test]
	fn a_line_edge_hugs_its_punctuation() {
		let (text, mut clusters) = per_char("。中。", 18.0);
		compress_line_edges(&mut clusters, &text, 18.0, CjkType::Sc);
		assert_eq!(clusters[0].width, 18.0);
		assert_eq!(clusters[1].width, 18.0);
		assert_eq!(clusters[2].width, 9.0);

		let (text, mut clusters) = per_char("（中", 18.0);
		compress_line_edges(&mut clusters, &text, 18.0, CjkType::Sc);
		assert_eq!(clusters[0].width, 9.0);
	}

	#[test]
	fn only_the_clusters_that_can_share_do() {
		let (text, clusters) = per_char("a中 b", 18.0);
		let fits = fit(&clusters, &text, 18.0, typo(CjkType::Sc));
		assert_eq!(fits.len(), 4);
		// The last cluster of the line carries no stretch and takes no share,
		// though it may still be compressed if the line turns out overfull.
		assert_eq!(fits[3].stretch, 0.0);
		assert!(!fits[3].share);
		assert!(fits[1].share);
		assert!(fits[2].share);
		assert!(!fits[0].share);
	}
}
