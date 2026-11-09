mod paint;
#[cfg(test)]
mod tests;

use super::{BlockContext, LayoutOptions, Prepared, fitted_range};
use crate::{
	document::{CellAlign, Inline, InlineKind},
	linebreak, microtype,
	scene::{BlockLayout, HeadingAnchor, Rect},
	shaping::Cluster,
	style::Condition,
	text::TextNode,
};
use std::collections::VecDeque;

struct ShapedLine {
	clusters: Vec<Cluster>,
	fits: Vec<microtype::Fit>,
	natural: f32,
	overhang: f32,
}

impl ShapedLine {
	fn new(
		clusters: Vec<Cluster>,
		p: &Prepared,
		size: f32,
		typography: microtype::Typography,
	) -> Self {
		let fits = microtype::fit(&clusters, &p.text, size, typography);
		let overhang = microtype::overhang(&clusters, &p.text, &p.spans);
		let natural = clusters.iter().map(|c| c.width).sum();
		Self {
			clusters,
			fits,
			natural,
			overhang,
		}
	}

	fn source_end(
		&self,
		p: &Prepared,
		units: &[linebreak::Unit],
		discarded: std::ops::Range<usize>,
	) -> usize {
		let mut end = self
			.clusters
			.iter()
			.map(|c| c.range.end)
			.max()
			.unwrap_or(p.text.len());
		// Consumed whitespace separates targets on either side of a break.
		for unit in units[discarded].iter().take_while(|u| u.discard) {
			end = end.max(unit.source.end);
			if &p.text[unit.source.clone()] == "\n" {
				break;
			}
		}
		end
	}
}

#[derive(Clone, Copy)]
struct LineGeometry {
	rect: Rect,
	baseline: f32,
	offset: f32,
	actual: f32,
	target: f32,
	solve: microtype::Justify,
}

struct ParagraphLayout<'a> {
	prepared: &'a Prepared,
	size: f32,
	sans: bool,
	width: f32,
	justify: bool,
	opts: &'a LayoutOptions,
}

impl ParagraphLayout<'_> {
	fn shape(
		&self,
		context: &mut BlockContext<'_>,
		units: &[linebreak::Unit],
		line: &linebreak::Line,
	) -> ShapedLine {
		let range = units[line.units.start].source.start
			..units[line.units.end - 1].source.end;
		let clusters = context.line_clusters(
			self.prepared,
			range,
			line.hyphen,
			self.size,
			self.sans,
			self.width,
			self.opts.typography(),
		);
		ShapedLine::new(
			clusters,
			self.prepared,
			self.size,
			self.opts.typography(),
		)
	}

	/// An earlier break after boundary reshaping also reoptimizes the
	/// remaining paragraph.
	fn reshape_to_fit(
		&self,
		context: &mut BlockContext<'_>,
		units: &[linebreak::Unit],
		line: &mut linebreak::Line,
		lines: &mut VecDeque<linebreak::Line>,
		line_width: f32,
	) -> ShapedLine {
		let mut shaped = self.shape(context, units, line);
		loop {
			let target = line_width + shaped.overhang;
			let shrink: f32 =
				if shaped.natural > target || (self.justify && !line.last) {
					shaped.fits.iter().map(|f| f.shrink()).sum()
				} else {
					0.
				};
			if shaped.natural - shrink <= target + 0.1 {
				break;
			}
			let Some(end) = (line.units.start + 1..line.units.end)
				.rev()
				.find(|&end| units[end - 1].after.is_some())
			else {
				break;
			};
			let br = units[end - 1].after.unwrap();
			line.units.end = end;
			while line.units.end > line.units.start
				&& units[line.units.end - 1].discard
			{
				line.units.end -= 1;
			}
			if line.units.is_empty() {
				break;
			}
			line.hyphen = br.hyphen_width > 0.;
			line.last = br.forced;
			shaped = self.shape(context, units, line);
			let tail = if self.opts.greedy {
				linebreak::greedy(&units[end..], self.width, &self.opts.limits)
			} else {
				linebreak::break_lines(
					&units[end..],
					self.width,
					self.size,
					self.justify,
					&self.opts.limits,
				)
			};
			*lines = tail
				.lines
				.into_iter()
				.map(|mut l| {
					l.units.start += end;
					l.units.end += end;
					l
				})
				.collect();
		}
		shaped
	}

	/// The final visible line maps its elided ranges back to the full source.
	fn elide(
		&self,
		context: &mut BlockContext<'_>,
		start: usize,
	) -> ShapedLine {
		let full = &self.prepared.text[start..];
		let shown = context.shaper.fit(
			full,
			self.size / context.shaper.appearance.size,
			self.width,
		);
		let mut clusters =
			context.shaper.shape(&shown, &[], self.size, self.sans);
		for c in &mut clusters {
			let elided = fitted_range(full, &shown, c.range.clone());
			c.range = start + elided.start..start + elided.end;
		}
		ShapedLine::new(
			clusters,
			self.prepared,
			self.size,
			self.opts.typography(),
		)
	}

	fn geometry(
		&self,
		context: &mut BlockContext<'_>,
		shaped: &ShapedLine,
		mut rect: Rect,
		last: bool,
		align: CellAlign,
	) -> LineGeometry {
		let Self {
			prepared: p,
			size,
			justify,
			..
		} = *self;
		let ShapedLine {
			clusters,
			fits,
			natural,
			overhang,
		} = shaped;
		let (natural, overhang) = (*natural, *overhang);
		let ink = if clusters.is_empty() {
			context.shaper.empty_metrics(size)
		} else {
			(
				clusters.iter().map(|c| c.ascent).fold(0., f32::max),
				clusters.iter().map(|c| c.descent).fold(0., f32::max),
			)
		};
		// A formula or picture can reach below the text it shares a line
		// with. Center the text when it fits, so that depth does not lift
		// every other glyph; only a box taller than the line owns the
		// baseline.
		let mut text_metrics: Option<(f32, f32)> = None;
		for cluster in clusters.iter().filter(|c| {
			!p.math.contains_key(&c.range.start)
				&& !p.images.contains_key(&c.range.start)
		}) {
			let (a, d) = text_metrics.get_or_insert((0., 0.));
			*a = (*a).max(cluster.ascent);
			*d = (*d).max(cluster.descent);
		}
		let height = (size * context.shaper.appearance.line_height)
			.max(ink.0 + ink.1 + size * 0.18);
		let (ascent, descent) = text_metrics.unwrap_or(ink);
		let mut baseline = rect.y + (height - ascent - descent) * 0.5 + ascent;
		// A box that reaches past the line still centers the ink it
		// shares it with.
		if baseline - ink.0 < rect.y || baseline + ink.1 > rect.y + height {
			baseline = rect.y + (height - ink.0 - ink.1) * 0.5 + ink.0;
		}
		// Justification spends the word spaces and the tracking first, then
		// shares whatever is left over between the clusters that can take
		// it. Solving it here from the same totals the break search used
		// keeps the drawn line the width that was chosen for it.
		let stretch: f32 = fits.iter().map(|f| f.stretch).sum();
		let shrink: f32 = fits.iter().map(|f| f.shrink()).sum();
		let shares = fits.iter().filter(|f| f.share).count();
		// A closing mark hangs into the end margin, so the line is solved
		// against the measure plus that much.
		let target = rect.w + overhang;
		// An overfull line is compressed wherever it sits in the
		// paragraph; only stretching is limited to justified lines.
		let solve = if natural > target || (justify && !last) {
			microtype::solve(natural, target, stretch, shrink, shares)
		} else {
			microtype::Justify::default()
		};
		let solve = microtype::Justify {
			ratio: solve.ratio.clamp(-1.0, 1.0),
			extra: solve.extra,
		};
		let actual =
			natural + microtype::gained(stretch, shrink, shares, solve);
		let offset = match align {
			CellAlign::Left => 0.0,
			CellAlign::Center => ((target - actual) * 0.5).max(0.0),
			CellAlign::Right => (target - actual).max(0.0),
		};

		rect.h = height;
		LineGeometry {
			rect,
			baseline,
			offset,
			actual,
			target,
			solve,
		}
	}
}

impl BlockContext<'_> {
	#[expect(
		clippy::too_many_arguments,
		reason = "Text style and block geometry are independent layout inputs"
	)]
	pub(super) fn paragraph(
		&mut self,
		rich: &[Inline],
		x: f32,
		y: f32,
		width: f32,
		size: f32,
		sans: bool,
		align: CellAlign,
		justify: bool,
		indent: bool,
		opts: &LayoutOptions,
		out: &mut BlockLayout,
	) -> f32 {
		self.paragraph_bounded(
			rich, x, y, width, size, sans, align, justify, indent, None, opts,
			out,
		)
	}

	/// A paragraph that draws at most `max_lines` lines. When the text continues
	/// past the bound, the last line is elided, so an image placeholder keeps its
	/// reason inside the image box instead of spilling out of it.
	#[expect(
		clippy::too_many_arguments,
		reason = "Text style and block geometry are independent layout inputs"
	)]
	pub(super) fn paragraph_bounded(
		&mut self,
		rich: &[Inline],
		x: f32,
		y: f32,
		width: f32,
		size: f32,
		sans: bool,
		align: CellAlign,
		justify: bool,
		indent: bool,
		max_lines: Option<usize>,
		opts: &LayoutOptions,
		out: &mut BlockLayout,
	) -> f32 {
		let p = crate::profile::span(crate::profile::Stage::Prepare, || {
			self.prepare(rich, size, out)
		});
		register_link_occurrences(&p, out);
		if p.text.is_empty() {
			for (_, anchor) in p.anchors {
				out.anchors.push(HeadingAnchor {
					anchor: crate::document::AnchorId::Fragment(anchor),
					y,
				});
			}
			return if rich
				.iter()
				.all(|i| matches!(i.kind, InlineKind::Anchor(_)))
			{
				0.
			} else {
				size * self.shaper.appearance.line_height
			};
		}
		let node = out.text.len();
		out.text.push(TextNode::new(p.reading.clone(), ""));
		out.text.last_mut().unwrap().search_ranges =
			p.search_ranges.clone().into();
		// The indent consumes part of the first line's measure, and never enough
		// to leave the opening line without room for a character.
		let indent = if indent {
			opts.indent(size, width)
		} else {
			0.0
		};
		let first_width = (width - indent).max(1.0);
		let units = crate::profile::span(crate::profile::Stage::Units, || {
			self.units(
				&p,
				size,
				sans,
				opts.hyphenate && !sans,
				width,
				opts.typography(),
			)
		});
		let solution =
			crate::profile::span(crate::profile::Stage::LineBreak, || {
				if opts.greedy {
					linebreak::greedy_with_first(
						&units,
						width,
						first_width,
						opts.limits.linebreak_evaluations,
					)
				} else {
					linebreak::break_lines_with_first(
						&units,
						width,
						first_width,
						size,
						justify,
						&opts.limits,
					)
				}
			});
		out.degraded += usize::from(solution.degraded && !opts.greedy);
		// Height stays local so cached paragraphs can move without changing it.
		let mut paragraph_height = 0.;
		let mut anchor_index = 0;
		let mut register_anchors =
			|end: usize, last: bool, y: f32, out: &mut BlockLayout| {
				let count = p.anchors[anchor_index..]
					.partition_point(|(offset, _)| *offset < end || last);
				for (_, anchor) in
					&p.anchors[anchor_index..anchor_index + count]
				{
					out.anchors.push(HeadingAnchor {
						anchor: crate::document::AnchorId::Fragment(
							anchor.clone(),
						),
						y,
					});
				}
				anchor_index += count;
			};
		let (only_images, align) = figure_alignment(rich, align, opts);
		let paragraph = ParagraphLayout {
			prepared: &p,
			size,
			sans,
			width,
			justify,
			opts,
		};
		let mut lines: VecDeque<_> = solution.lines.into();
		let mut first_line = true;
		let mut drawn = 0;
		while let Some(mut line) = lines.pop_front() {
			let y_cursor = y + paragraph_height;
			if line.units.is_empty() {
				let end = units
					.get(line.units.start)
					.map_or(p.text.len(), |unit| unit.source.start);
				register_anchors(end, lines.is_empty(), y_cursor, out);
				paragraph_height += size * self.shaper.appearance.line_height;
				drawn += 1;
				continue;
			}
			// Only the line that opens the paragraph is narrowed by the indent;
			// wrapped lines keep the full measure.
			let (line_x, line_width) = if first_line {
				(x + indent, first_width)
			} else {
				(x, width)
			};
			first_line = false;
			let line_start = units[line.units.start].source.start;
			let mut shaped = paragraph.reshape_to_fit(
				self, &units, &mut line, &mut lines, line_width,
			);
			if let Some(max_lines) = max_lines
				&& drawn + 1 == max_lines
				&& !line.last
			{
				shaped = paragraph.elide(self, line_start);
				line.last = true;
				line.hyphen = false;
				lines.clear();
			}
			let geometry = paragraph.geometry(
				self,
				&shaped,
				Rect {
					x: line_x,
					y: y_cursor,
					w: line_width,
					h: 0.,
				},
				line.last,
				align,
			);
			let consumed_end =
				lines.front().map_or(units.len(), |l| l.units.start);
			let end =
				shaped.source_end(&p, &units, line.units.end..consumed_end);
			let last = lines.is_empty();
			register_anchors(end, last, y_cursor, out);
			paragraph_height +=
				self.emit_line(&paragraph, node, shaped, geometry, out);
			drawn += 1;
		}
		if only_images && p.images.len() == 1 {
			paragraph_height = self.image_caption(
				p.images.values().next().unwrap(),
				[x, y],
				paragraph_height,
				width,
				opts,
				out,
			);
		}
		paragraph_height
	}
}

fn register_link_occurrences(p: &Prepared, out: &mut BlockLayout) {
	// Record occurrences before line breaking; a run it discards still has
	// to hold its place so cached ids match the runs painted after it.
	for &origin in &p.link_occurrences {
		if out.link_occurrences.last() != Some(&origin) {
			out.link_occurrences.push(origin);
		}
	}
}

fn figure_alignment(
	rich: &[Inline],
	align: CellAlign,
	opts: &LayoutOptions,
) -> (bool, CellAlign) {
	// An image alone in its block is a centered figure; mixed with text it
	// is an ordinary atomic inline box in the line flow.
	let only_images = !rich.is_empty()
		&& rich.iter().all(|i| {
			matches!(&i.kind, InlineKind::Image(_) | InlineKind::Anchor(_))
				|| matches!(&i.kind, InlineKind::Text(t) if t.trim().is_empty())
		});
	let align = if only_images && align == CellAlign::Left {
		opts.stylesheet
			.rule(Condition::Image)
			.align
			.map(Into::into)
			.unwrap_or(CellAlign::Center)
	} else {
		align
	};
	(only_images, align)
}
