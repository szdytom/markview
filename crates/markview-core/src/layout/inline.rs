use super::{BlockContext, Prepared};
use crate::{
	document::{Inline, InlineKind},
	linebreak::{Break, Unit},
	scene::BlockLayout,
	shaping::{Cluster, Span},
	style::Condition,
};
use std::collections::HashSet;
use std::{collections::BTreeMap, ops::Range};
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
			math: BTreeMap::new(),
			notes: BTreeMap::new(),
		};
		for inline in rich {
			let start = p.text.len();
			let reading_start = p.reading.len();
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
			}
			let mut style = inline.style.clone();
			match &inline.kind {
				InlineKind::Image(image) => {
					p.images.insert(start, image.clone());
					p.text.push('\u{fffc}');
				}
				InlineKind::Text(t) => p.text.push_str(t),
				InlineKind::FootnoteRef(n) => {
					p.notes.insert(start, *n);
					p.text.push_str(&format!("[{n}]"));
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
			p.spans.push(Span {
				range: start..p.text.len(),
				style,
			});
		}
		p
	}

	pub(super) fn units(
		&mut self,
		p: &Prepared,
		size: f32,
		sans: bool,
		hyphenate: bool,
		available: f32,
	) -> Vec<Unit> {
		let mut clusters =
			crate::profile::span(crate::profile::Stage::ShapeClusters, || {
				self.shaper.shape(&p.text, &p.spans, size, sans)
			});
		clusters.sort_by_key(|c| c.range.start);
		let segmenter =
			icu_segmenter::LineSegmenter::new_auto(Default::default());
		let breaks: HashSet<usize> = segmenter.segment_str(&p.text).collect();
		let mut hyphens = HashSet::new();
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
								hyphens.insert(offset);
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
		let mut units = Vec::new();
		for (i, c) in clusters.iter().enumerate() {
			let t = &p.text[c.range.clone()];
			let whitespace = t
				.chars()
				.all(|c| c == ' ' || c == '\t' || c == '\n' || c == '\r');
			let hard = t.contains('\n');
			let soft_hyphen = t == "\u{ad}";
			let math = p.math.get(&c.range.start);
			let cjk = t.chars().next().is_some_and(is_cjk);
			let next = clusters.get(i + 1);
			let legal = breaks.contains(&c.range.end)
				&& !next.is_some_and(|c| c.continuation);
			let after = if hard {
				Some(Break::FORCED)
			} else if soft_hyphen || hyphens.contains(&c.range.end) {
				Some(Break {
					penalty: 50.0,
					hyphen_width,
					forced: false,
				})
			} else if legal {
				Some(Break::NORMAL)
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
			units.push(Unit {
				source: c.range.clone(),
				width,
				stretch: if whitespace && !hard {
					width * 0.65
				} else if cjk {
					size * 0.08
				} else {
					0.0
				},
				shrink: if whitespace && !hard {
					width * 0.3
				} else {
					0.0
				},
				discard: whitespace,
				after,
			});
		}
		units
	}

	pub(super) fn line_clusters(
		&mut self,
		p: &Prepared,
		range: Range<usize>,
		hyphen: bool,
		size: f32,
		sans: bool,
		available: f32,
	) -> Vec<Cluster> {
		crate::profile::span(crate::profile::Stage::LineClusters, || {
			self.line_clusters_inner(p, range, hyphen, size, sans, available)
		})
	}

	fn line_clusters_inner(
		&mut self,
		p: &Prepared,
		range: Range<usize>,
		hyphen: bool,
		size: f32,
		sans: bool,
		available: f32,
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
		clusters
	}
}
pub(super) fn is_cjk(c: char) -> bool {
	matches!(c as u32, 0x2e80..=0x9fff | 0xf900..=0xfaff | 0x20000..=0x3134f)
}
