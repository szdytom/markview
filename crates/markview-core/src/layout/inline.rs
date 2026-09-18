use super::{BlockContext, Prepared};
use crate::{
	document::{Inline, InlineKind, TextStyle},
	linebreak::{Break, Unit},
	microtype,
	scene::BlockLayout,
	shaping::{Cluster, Span},
	style::Condition,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;

/// Demerits for breaking a word with a hyphen, before the distance from the
/// word's edges is taken into account.
const HYPHEN_PENALTY: f64 = 50.0;

/// Demerits for splitting a word inside inline code, such as breaking
/// `identifier` between two letters. Code is set exactly as written, so it has
/// no hyphenation dictionary and often no word spaces to break at; every
/// boundary inside the run is offered instead. A break that separates tokens —
/// where a text editor's whole-word selection stops, as in `foo|=|bar()` — is
/// free; only a break that splits a word carries this cost, small enough that an
/// ordinary word space or a hyphenation point still wins when one is available.
pub(super) const CODE_BREAK_PENALTY: f64 = 10.0;

/// Demerits for a hyphenation break, graded by how close it lands to either
/// edge of the word. A hyphen a character or two from the start or end reads as
/// a mistake rather than a convenience, so it should be worth avoiding even
/// when the line it produces is otherwise better.
pub(super) fn hyphen_penalty(before: usize, after: usize) -> f64 {
	const EDGE: usize = 5;
	let steps = EDGE.saturating_sub(before) + EDGE.saturating_sub(after);
	HYPHEN_PENALTY * (1.0 + 0.15 * steps as f64)
}
impl BlockContext<'_> {
	pub(super) fn prepare(
		&mut self,
		rich: &[Inline],
		size: f32,
		out: &mut BlockLayout,
	) -> Prepared {
		let mut p = Prepared {
			images: BTreeMap::new(),
			reading: String::new(),
			mapping: Vec::new(),
			text: String::new(),
			spans: Vec::new(),
			padding: Vec::new(),
			math: BTreeMap::new(),
			notes: BTreeMap::new(),
			breaks: std::collections::BTreeSet::new(),
		};
		let mut i = 0;
		while i < rich.len() {
			let inline = &rich[i];
			let start = p.text.len();
			let reading_start = p.reading.len();
			// Consecutive references share one bracket pair and one comma, so
			// only their numbers stay clickable.
			let run = if matches!(inline.kind, InlineKind::FootnoteRef(_)) {
				footnote_run(rich, i)
			} else {
				i + 1
			};
			if run > i + 1 {
				footnote_group(&mut p, &rich[i..run]);
				// A footnote reference is never code, so its chip is unpadded.
				p.padding.push([0.0; 4]);
				p.mapping.push((
					start..p.text.len(),
					reading_start..p.reading.len(),
					false,
				));
				i = run;
				continue;
			}
			match &inline.kind {
				InlineKind::Image(image) => p.reading.push_str(
					&self
						.image_placeholder(image)
						.unwrap_or_else(|| image.alt.clone()),
				),
				InlineKind::Text(t) => p.reading.push_str(t),
				InlineKind::Math { latex, .. } => p.reading.push_str(latex),
				InlineKind::FootnoteRef(n) => {
					p.reading.push_str(&format!("[{n}]"));
				}
				InlineKind::LineBreak { .. } => p.reading.push('\n'),
			}
			let mut style = inline.style.clone();
			match &inline.kind {
				InlineKind::Image(image) => {
					p.images.insert(start, image.clone());
					p.text.push('\u{fffc}');
				}
				InlineKind::Text(t) => p.text.push_str(t),
				InlineKind::FootnoteRef(n) => {
					let label = n.to_string();
					// The whole label is one link; `notes` carries only the
					// number's own range, which registers the return anchor.
					let at = start + 1;
					p.notes.insert(at, (*n, at + label.len()));
					p.text.push('[');
					p.text.push_str(&label);
					p.text.push(']');
				}
				InlineKind::LineBreak { justify } => {
					if *justify {
						p.breaks.insert(start);
					}
					p.text.push('\n');
				}
				InlineKind::Math { latex, display } => {
					let laid_out = crate::profile::span(
						crate::profile::Stage::Math,
						|| {
							self.math.layout(
								latex,
								*display,
								size * self
									.shaper
									.stylesheet
									.rule(Condition::Math)
									.size
									.unwrap_or(1.),
							)
						},
					);
					match laid_out {
						Ok(m) => {
							p.math.insert(start, m);
							p.text.push('\u{fffc}');
						}
						Err(error) => {
							p.text.push_str(latex);
							// `["error"]` and `["math", "error"]` both apply.
							let show_error = self
								.shaper
								.stylesheet
								.element_rule(
									crate::style::chain_of(&[
										Condition::Math,
										Condition::Error,
									]),
									Condition::Error,
								)
								.show
								.unwrap_or(true);
							if show_error {
								let diagnostic =
									format!(" [Math error: {error}]");
								p.reading.push_str(&diagnostic);
								p.text.push_str(&diagnostic);
								style.math_error = true;
							}
							style.code = true;
							out.math_errors += 1;
						}
					}
				}
			}
			p.mapping.push((
				start..p.text.len(),
				reading_start..p.reading.len(),
				matches!(
					inline.kind,
					InlineKind::Math { .. } | InlineKind::Image(_)
				),
			));
			let padding = self.code_padding(&style, size);
			p.spans.push(Span {
				range: start..p.text.len(),
				style,
			});
			p.padding.push(padding);
			i += 1;
		}
		p
	}

	/// The padding an inline code chip adds around its run, in logical pixels
	/// in the canonical top, right, bottom, left order. Only rules that name
	/// `code` apply, so a containing block's own padding never reaches a chip.
	fn code_padding(&self, style: &TextStyle, size: f32) -> [f32; 4] {
		if !style.code {
			return [0.0; 4];
		}
		let mut chain = self.shaper.appearance.chain;
		for condition in style.conditions() {
			chain = crate::style::chain_push(chain, condition);
		}
		self.shaper
			.stylesheet
			.element_rule(chain, Condition::Code)
			.padding
			.as_ref()
			.map_or([0.0; 4], |padding| padding.sides().map(|v| v * size))
	}

	pub(super) fn units(
		&mut self,
		p: &Prepared,
		size: f32,
		sans: bool,
		hyphenate: bool,
		available: f32,
		typo: microtype::Typography,
	) -> Vec<Unit> {
		let mut clusters =
			crate::profile::span(crate::profile::Stage::ShapeClusters, || {
				self.shaper.shape(&p.text, &p.spans, size, sans)
			});
		clusters.sort_by_key(|c| c.range.start);
		let segmenter =
			icu_segmenter::LineSegmenter::new_auto(Default::default());
		let breaks: HashSet<usize> = segmenter.segment_str(&p.text).collect();
		// Where a word may be split, and how many characters would be left on
		// either side, which decides how much the break costs.
		let mut hyphens: HashMap<usize, (usize, usize)> = HashMap::new();
		if hyphenate {
			let mut word_start = None;
			for (i, c) in p
				.text
				.char_indices()
				.chain(std::iter::once((p.text.len(), ' ')))
			{
				if c.is_ascii_alphabetic() {
					word_start.get_or_insert(i);
				} else if let Some(start) = word_start.take() {
					// The run is ASCII, so byte and character counts agree.
					let word = &p.text[start..i];
					if word.len() >= 6
						&& !p.spans.iter().any(|s| {
							s.range.contains(&start)
								&& (s.style.code || s.style.link.is_some())
						}) {
						let mut offset = start;
						for syllable in
							hypher::hyphenate(word, hypher::Lang::English)
						{
							offset += syllable.len();
							if offset - start >= 2 && i - offset >= 3 {
								hyphens.insert(
									offset,
									(offset - start, i - offset),
								);
							}
						}
					}
				}
			}
		}
		let hyphen_width: f32 = self
			.shaper
			.shape("-", &[], size, sans)
			.iter()
			.map(|c| c.width)
			.sum();
		microtype::space_mixed_scripts(&mut clusters, &p.text, &p.spans, size);
		let code = code_clusters(&clusters, &p.spans);
		let chip = chip_padding(&clusters, &p.spans, &p.padding);
		let mut units = Vec::new();
		for (i, c) in clusters.iter().enumerate() {
			let t = &p.text[c.range.clone()];
			let whitespace = microtype::is_space(t);
			let hard = t.contains('\n');
			let soft_hyphen = t == "\u{ad}";
			let math = p.math.get(&c.range.start);
			let next = clusters.get(i + 1);
			// A continuation carries no glyph of its own, so a break before it
			// would split one grapheme cluster.
			let attachable = !next.is_some_and(|c| c.continuation);
			let legal = (breaks.contains(&c.range.end) && attachable)
				|| microtype::quote_edge_break(&clusters, &p.text, i, size);
			// Only a boundary between two code characters belongs to the code
			// run. Its edges belong to the surrounding text, so the segmenter's
			// rules still decide them and a following comma or closing bracket
			// is never left to start the next line.
			let in_code = code[i] && code.get(i + 1).copied().unwrap_or(false);
			let after = if hard {
				// A break the author asked to justify still ends a line, but
				// the line it ends is set flush like any other.
				Some(Break {
					justify: p.breaks.contains(&c.range.start),
					..Break::FORCED
				})
			} else if soft_hyphen {
				// The document asked for this break, so it carries only the
				// base cost of a hyphen.
				Some(Break {
					penalty: HYPHEN_PENALTY,
					hyphen_width,
					..Break::NORMAL
				})
			} else if let Some(&(before, after)) = hyphens.get(&c.range.end) {
				Some(Break {
					penalty: hyphen_penalty(before, after),
					hyphen_width,
					..Break::NORMAL
				})
			} else if legal {
				Some(Break::NORMAL)
			} else if attachable && in_code {
				// Inline code has its own breaking rule: a boundary between two
				// code characters is always legal. A token edge is free, the
				// way a whole-word selection stops there; a split inside a word
				// carries a small penalty that still prefers a real word space.
				Some(if splits_word(&p.text, c.range.end) {
					Break {
						penalty: CODE_BREAK_PENALTY,
						..Break::NORMAL
					}
				} else {
					Break::NORMAL
				})
			} else {
				None
			};
			let width = if hard || soft_hyphen {
				0.0
			} else if let Some(image) = p.images.get(&c.range.start) {
				self.image_size(image, available, size).0
			} else {
				math.map_or(c.width, |m| m.width)
			};
			// Measure the advance the line really draws, which is zero for a
			// soft hyphen and the box's own width for an image or formula.
			let (adjust, justifiable) = microtype::adjust(
				t,
				width,
				size,
				c.mixed,
				c.glyphs.len(),
				typo,
			);
			// A code chip's horizontal padding is part of the advance the line
			// breaks against, but it is rigid, so `adjust` sees only the glyphs.
			let pad = chip[i];
			units.push(Unit {
				source: c.range.clone(),
				width: width + pad[1] + pad[3],
				stretch: adjust.stretch(),
				shrink: adjust.shrink(),
				justifiable,
				discard: whitespace,
				after,
			});
		}
		units
	}

	#[expect(
		clippy::too_many_arguments,
		reason = "Text style and block geometry are independent layout inputs"
	)]
	pub(super) fn line_clusters(
		&mut self,
		p: &Prepared,
		range: Range<usize>,
		hyphen: bool,
		size: f32,
		sans: bool,
		available: f32,
		typo: microtype::Typography,
	) -> Vec<Cluster> {
		crate::profile::span(crate::profile::Stage::LineClusters, || {
			self.line_clusters_inner(
				p, range, hyphen, size, sans, available, typo,
			)
		})
	}

	#[expect(
		clippy::too_many_arguments,
		reason = "Text style and block geometry are independent layout inputs"
	)]
	fn line_clusters_inner(
		&mut self,
		p: &Prepared,
		range: Range<usize>,
		hyphen: bool,
		size: f32,
		sans: bool,
		available: f32,
		typo: microtype::Typography,
	) -> Vec<Cluster> {
		let mut text = p.text[range.clone()].to_string();
		if hyphen {
			text.push('-');
		}
		let mut spans: Vec<Span> = p
			.spans
			.iter()
			.filter_map(|s| {
				let start = s.range.start.max(range.start);
				let end = s.range.end.min(range.end);
				(start < end).then(|| Span {
					range: start - range.start..end - range.start,
					style: s.style.clone(),
				})
			})
			.collect();
		if hyphen && let Some(s) = spans.last_mut() {
			s.range.end = text.len();
		}
		let mut clusters =
			crate::profile::span(crate::profile::Stage::ShapeClusters, || {
				self.shaper.shape(&text, &spans, size, sans)
			});
		for c in &mut clusters {
			c.range = (c.range.start + range.start).min(range.end)
				..(c.range.end + range.start).min(range.end);
			if let Some(image) = p.images.get(&c.range.start) {
				let (w, h) = self.image_size(image, available, size);
				c.width = w;
				c.ascent = h;
				c.descent = 0.;
				c.glyphs.clear();
			}
			if let Some(m) = p.math.get(&c.range.start) {
				c.width = m.width;
				c.ascent = m.ascent;
				c.descent = m.descent;
				c.glyphs.clear();
			}
			if p.text.get(c.range.clone()) == Some("\u{ad}") {
				c.width = 0.0;
				c.glyphs.clear();
			}
		}
		// Mixed CJK and Latin spacing needs both neighbours, so a gap that a
		// line break separates is never inserted, and the punctuation at the
		// two ends of this line is compressed against the measure.
		microtype::space_mixed_scripts(&mut clusters, &p.text, &p.spans, size);
		microtype::compress_line_edges(&mut clusters, &p.text, size, typo.cjk);
		// The chip's horizontal padding widens the run and insets its glyphs,
		// and its vertical padding makes every cluster of the run as tall as
		// the chip, so the background stays one rectangle.
		let chip = chip_padding(&clusters, &p.spans, &p.padding);
		for (c, pad) in clusters.iter_mut().zip(chip) {
			if pad == [0.0; 4] {
				continue;
			}
			c.width += pad[1] + pad[3];
			c.ascent += pad[0];
			c.descent += pad[2];
			for glyph in &mut c.glyphs {
				glyph.x += pad[3];
			}
		}
		clusters
	}
}

/// Whether each cluster belongs to an inline code run, found by walking the
/// spans alongside the clusters, both of which are in reading order.
fn code_clusters(clusters: &[Cluster], spans: &[Span]) -> Vec<bool> {
	let mut code = Vec::with_capacity(clusters.len());
	let mut cursor = 0;
	for cluster in clusters {
		while cursor < spans.len()
			&& spans[cursor].range.end <= cluster.range.start
		{
			cursor += 1;
		}
		code.push(spans.get(cursor).is_some_and(|span| {
			span.style.code && span.range.contains(&cluster.range.start)
		}));
	}
	code
}

/// The chip padding of each cluster, in the canonical top, right, bottom, left
/// order. The run's left and right padding lands only on the cluster holding
/// that edge, so a run broken across lines keeps a flush fragment; the vertical
/// padding lands on every cluster, so the chip stays one rectangle.
fn chip_padding(
	clusters: &[Cluster],
	spans: &[Span],
	padding: &[[f32; 4]],
) -> Vec<[f32; 4]> {
	let mut chip = vec![[0.0; 4]; clusters.len()];
	let mut cursor = 0;
	for (i, cluster) in clusters.iter().enumerate() {
		while cursor < spans.len()
			&& spans[cursor].range.end <= cluster.range.start
		{
			cursor += 1;
		}
		let (Some(span), Some(pad)) = (spans.get(cursor), padding.get(cursor))
		else {
			continue;
		};
		if !span.style.code || !span.range.contains(&cluster.range.start) {
			continue;
		}
		chip[i] = [
			pad[0],
			if cluster.range.end >= span.range.end {
				pad[1]
			} else {
				0.0
			},
			pad[2],
			if cluster.range.start == span.range.start {
				pad[3]
			} else {
				0.0
			},
		];
	}
	chip
}

/// Whether the boundary at `at` falls inside a word, which is a split of an
/// identifier or a number rather than an edge between tokens.
fn splits_word(text: &str, at: usize) -> bool {
	let before = text[..at].chars().next_back();
	let after = text[at..].chars().next();
	before.is_some_and(is_word_char) && after.is_some_and(is_word_char)
}

/// Whether `c` is part of a word, matching the characters an editor's
/// double-click selection spans: letters, digits and underscore.
fn is_word_char(c: char) -> bool {
	c.is_alphanumeric() || c == '_'
}

/// The end of the footnote-reference run that starts at `start`: adjacent
/// references sharing one style, with any whitespace between them included
/// because it reads as part of the same citation group.
fn footnote_run(rich: &[Inline], start: usize) -> usize {
	let style = &rich[start].style;
	let mut end = start + 1;
	loop {
		match rich.get(end) {
			Some(next) if matches!(&next.kind, InlineKind::FootnoteRef(_)) => {
				if !same_note_style(style, &next.style) {
					break;
				}
				end += 1;
			}
			Some(next) if matches!(&next.kind, InlineKind::Text(t) if t.trim().is_empty()) =>
			{
				let Some(after) = rich.get(end + 1) else {
					break;
				};
				if !matches!(&after.kind, InlineKind::FootnoteRef(_))
					|| !same_note_style(style, &after.style)
				{
					break;
				}
				end += 2;
			}
			_ => break,
		}
	}
	end
}

/// Whether two references read alike apart from the note they point at, which
/// is what lets them share one bracket pair.
fn same_note_style(a: &TextStyle, b: &TextStyle) -> bool {
	let clear = |s: &TextStyle| TextStyle {
		link: None,
		..s.clone()
	};
	clear(a) == clear(b)
}

/// Draw a run of references as one `[1,2]` group: every number is its own link,
/// and the brackets and commas are not. The whole group shares one appearance,
/// so it takes a single span; `notes` gives each number's digit range.
fn footnote_group(p: &mut Prepared, run: &[Inline]) {
	let start = p.text.len();
	p.text.push('[');
	p.reading.push('[');
	let mut first = true;
	for number in run.iter().filter_map(|inline| match &inline.kind {
		InlineKind::FootnoteRef(number) => Some(*number),
		_ => None,
	}) {
		if !first {
			p.text.push(',');
			p.reading.push(',');
		}
		first = false;
		let label = number.to_string();
		let at = p.text.len();
		p.notes.insert(at, (number, at + label.len()));
		p.text.push_str(&label);
		p.reading.push_str(&label);
	}
	p.text.push(']');
	p.reading.push(']');
	p.spans.push(Span {
		range: start..p.text.len(),
		style: TextStyle {
			link: None,
			..run[0].style.clone()
		},
	});
}
