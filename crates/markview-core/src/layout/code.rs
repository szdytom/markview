//! Code block geometry preserves source whitespace while applying syntax colors after shaping.
use super::{BlockContext, LayoutOptions, expand_tabs_mapped};
use crate::{
	document::TextStyle,
	scene::{BlockLayout, Draw, Overflow, Paint, Rect},
	shaping::Span,
	style::{ColorField, Condition},
	text::{TextCluster, TextNode},
};
use std::sync::Arc;
impl BlockContext<'_> {
	#[expect(
		clippy::too_many_arguments,
		reason = "Code source, block geometry and layout inputs"
	)]
	pub(super) fn code(
		&mut self,
		language: &str,
		text: &str,
		x: f32,
		y: f32,
		width: f32,
		size: f32,
		opts: &LayoutOptions,
		out: &mut BlockLayout,
	) -> f32 {
		let node = out.text.len();
		out.text.push(TextNode::new(text.to_owned(), "\n\n"));
		let mut line_offset = 0;

		let mut cursor = y;
		if !language.is_empty() {
			let label = opts
				.stylesheet
				.text(&self.shaper.appearance, Condition::Label);
			let rule =
				opts.stylesheet.element_rule(label.chain, Condition::Label);
			cursor += rule.space_before.unwrap_or(0.) * opts.font_size;
			let label_size = opts.font_size * label.size;
			let label_height = label_size * label.line_height;
			out.draws.extend(
				self.shaper
					.label_with(
						language,
						opts.font_size,
						x,
						cursor + label_size,
						&label,
						label.paint,
						Some(Paint::Scoped(
							label.chain,
							Condition::Label,
							ColorField::Background,
						)),
					)
					.0,
			);
			cursor +=
				label_height + rule.space_after.unwrap_or(0.) * opts.font_size;
		}
		let content_start = out.draws.len();
		let mut natural = 0.0_f32;
		let theme = opts.codeblock_theme_override.as_deref().or(opts
			.stylesheet
			.rule(Condition::CodeBlock)
			.theme
			.as_deref());
		let highlight_key =
			crate::document::fingerprint(&(language, text, theme));
		let highlighted = self
			.highlight_cache
			.get(&highlight_key)
			.cloned()
			.unwrap_or_else(|| {
				Arc::new(vec![
					Vec::new();
					text.trim_end_matches('\n').split('\n').count()
				])
			});
		for (line_index, line) in
			text.trim_end_matches('\n').split('\n').enumerate()
		{
			let original = line;
			let (line, offsets) = expand_tabs_mapped(line, 4);
			// Syntax colors are applied after shaping. Keeping the shaper input
			// plain means highlighting cannot affect font selection, shaping,
			// line breaking, or any geometry used by selection.
			let spans = [Span {
				range: 0..line.len(),
				style: TextStyle::default(),
			}];
			let clusters = self.shaper.shape(&line, &spans, size, false);
			let line_ascent =
				clusters.iter().map(|c| c.ascent).fold(size * 0.8, f32::max);
			let line_descent = clusters
				.iter()
				.map(|c| c.descent)
				.fold(size * 0.2, f32::max);
			let line_height = (size * self.shaper.appearance.line_height)
				.max(line_ascent + line_descent);
			let baseline_at = |top: f32| {
				top + (line_height - line_ascent - line_descent) * 0.5
					+ line_ascent
			};
			let mut left = x;
			let mut baseline = baseline_at(cursor);
			for c in clusters {
				// Clusters wider than the whole column stay on their own line
				// rather than wrapping forever.
				if opts.codeblock_wrap && left > x && left + c.width > x + width
				{
					cursor += line_height;
					left = x;
					baseline = baseline_at(cursor);
				}
				let color = highlighted[line_index]
					.iter()
					.find(|(range, _)| {
						range.start <= c.range.start
							&& c.range.start < range.end
					})
					.and_then(|(_, color)| *color);
				let start = offsets[c.range.start];
				let end = offsets[c.range.end];
				let end = if start == end {
					start
						+ original[start..]
							.chars()
							.next()
							.map_or(0, char::len_utf8)
				} else {
					end
				};
				out.text[node].push(TextCluster {
					range: line_offset + start..line_offset + end,
					rect: Rect {
						x: left,
						y: cursor,
						w: c.width.max(1.0),
						h: line_height,
					},
					rtl: c.rtl,
					command: out.draws.len(),
				});
				for mut g in c.glyphs {
					if let Some(color) = color {
						g.paint = Paint::Color(color);
					}
					g.x += left;
					g.y += baseline;
					out.draws.push(Draw::Glyph(g));
				}
				left += c.width;
			}
			natural = natural.max(left - x);
			cursor += line_height;
			line_offset += original.len() + 1;
		}
		let mut h = cursor - y;
		if natural > width {
			let gutter = opts.stylesheet.scrollbar_gutter();
			out.overflow.push(Overflow {
				rect: Rect { x, y, w: width, h },
				content_width: natural,
				commands: content_start..out.draws.len(),
				gutter,
			});
			h += gutter;
		}
		h
	}
}
