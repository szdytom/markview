use super::{BlockContext, LineGeometry, ParagraphLayout, ShapedLine};
use crate::{
	document::{AnchorId, FootnoteReference},
	image::ImageSpec,
	interval, microtype,
	scene::{
		BlockLayout, Draw, HeadingAnchor, LinkId, LinkRect, LinkTarget,
		Overflow, Paint, Rect,
	},
	shaping::Cluster,
	style::{ColorField, Condition, Decoration},
	text::TextCluster,
};
use std::ops::Range;

struct LinkRun {
	target: LinkTarget,
	id: LinkId,
	x: f32,
	command: usize,
}

impl LinkRun {
	fn finish(self, cursor: f32, row: Rect, out: &mut BlockLayout) {
		out.links.push(LinkRect {
			commands: self.command..out.draws.len(),
			id: self.id,
			// The whole line box keeps leading inside the link's hit target.
			rect: Rect {
				x: self.x,
				y: row.y,
				w: cursor - self.x,
				h: row.h,
			},
			url: self.target.display_url(),
			target: self.target,
		});
	}
}

struct BackgroundRun {
	command: usize,
	paint: Paint,
	fragment: Option<Range<usize>>,
	rect: Rect,
}

impl BackgroundRun {
	/// A fragment owns one background even when fallback fonts have
	/// different ink extents.
	fn push(
		run: &mut Option<Self>,
		paint: Paint,
		fragment: Option<Range<usize>>,
		rect: Rect,
		row: Rect,
		out: &mut BlockLayout,
	) {
		if let Some(previous) = run.as_mut()
			&& previous.paint == paint
			&& previous.fragment == fragment
			&& rect.x <= previous.rect.x + previous.rect.w + 0.01
		{
			let span = &mut previous.rect;
			let bottom = (span.y + span.h).max(rect.y + rect.h);
			span.y = span.y.min(rect.y);
			span.h = bottom - span.y;
			span.w = (rect.x + rect.w).max(span.x + span.w) - span.x;
			*out.draws.get_mut(previous.command).unwrap() =
				Draw::Rect(*span, paint);
			return;
		}
		*run = Some(Self {
			command: out.draws.len(),
			paint,
			fragment,
			rect,
		});
		out.inline_decorations
			.push((out.draws.len(), row.y..row.y + row.h));
		out.draws.push(Draw::Rect(rect, paint));
	}
}

struct LinePainter<'a, 'p> {
	paragraph: &'a ParagraphLayout<'p>,
	node: usize,
	geometry: LineGeometry,
	cursor: f32,
	link: Option<LinkRun>,
	background: Option<BackgroundRun>,
	out: &'a mut BlockLayout,
}

impl BlockContext<'_> {
	pub(super) fn emit_line(
		&mut self,
		paragraph: &ParagraphLayout<'_>,
		node: usize,
		shaped: ShapedLine,
		geometry: LineGeometry,
		out: &mut BlockLayout,
	) -> f32 {
		let start_draw = out.draws.len();
		let mut painter = LinePainter {
			paragraph,
			node,
			cursor: geometry.rect.x + geometry.offset,
			geometry,
			link: None,
			background: None,
			out,
		};
		for (cluster, fit) in shaped.clusters.into_iter().zip(shaped.fits) {
			painter.cluster(self, cluster, fit);
		}
		painter.finish(start_draw)
	}
}

impl LinePainter<'_, '_> {
	fn cluster(
		&mut self,
		context: &mut BlockContext<'_>,
		c: Cluster,
		fit: microtype::Fit,
	) {
		let p = self.paragraph.prepared;
		let range = p.reading_range(c.range.clone());
		// Only the first digit registers the reference's return anchor.
		let note = p.note_at(c.range.start);
		if let Some((reference, true)) = note {
			self.out.anchors.push(HeadingAnchor {
				anchor: AnchorId::FootnoteReference {
					definition: reference.definition.clone(),
					occurrence: reference.occurrence,
				},
				y: self.geometry.rect.y,
			});
		}
		if let Some(image) = p.images.get(&c.range.start) {
			self.image_cluster(context, &c, image, range);
			self.cursor += c.width;
		} else {
			self.text_cluster(
				context,
				c,
				fit,
				range,
				note.map(|(reference, _)| reference),
			);
		}
	}

	fn image_cluster(
		&mut self,
		context: &mut BlockContext<'_>,
		c: &Cluster,
		image: &ImageSpec,
		range: Range<usize>,
	) {
		let ParagraphLayout {
			prepared: p,
			size,
			width,
			opts,
			..
		} = *self.paragraph;
		let cursor = self.cursor;
		let baseline = self.geometry.baseline;
		let node = self.node;
		let out = &mut *self.out;
		let rect = Rect {
			x: cursor,
			y: baseline - c.ascent,
			w: c.width,
			h: c.ascent,
		};
		let command = out.draws.len();
		out.text[node].source_images.push((
			p.image_indices[&c.range.start],
			TextCluster {
				mixed_spacing: (0.0, 0.0),
				range: range.clone(),
				rect,
				rtl: false,
				atomic: true,
				command,
			},
		));
		// A drawn image's whole box is selectable for its `alt`.
		// A placeholder is instead selected character by
		// character, so its visible message copies as shown.
		if !range.is_empty() && context.image_placeholder(image).is_none() {
			out.text[node].push(TextCluster {
				mixed_spacing: (0.0, 0.0),
				range: range.clone(),
				rect,
				rtl: false,
				atomic: true,
				command,
			});
		}
		let span = interval::containing(&p.spans, c.range.start, |s| &s.range);
		let link_id = p.link_id(c.range.start, span);
		let link_url = span
			.and_then(|index| p.spans[index].link.as_ref())
			.map(|link| LinkTarget::Url(link.shared().clone()));
		for mut cluster in
			context.draw_image(image, rect, size, width, opts, out)
		{
			cluster.range.start += range.start;
			cluster.range.end += range.start;
			out.text[node].push(cluster);
		}
		if let (Some(target), Some(id)) = (link_url, link_id) {
			out.links.push(LinkRect {
				commands: command..out.draws.len(),
				id,
				rect,
				url: target.display_url(),
				target,
			});
		}
	}

	fn text_cluster(
		&mut self,
		context: &mut BlockContext<'_>,
		c: Cluster,
		fit: microtype::Fit,
		range: Range<usize>,
		note: Option<&FootnoteReference>,
	) {
		let ParagraphLayout {
			prepared: p, size, ..
		} = *self.paragraph;
		let LineGeometry { rect, solve, .. } = self.geometry;
		let (y_cursor, height) = (rect.y, rect.h);
		let cursor = self.cursor;
		let node = self.node;
		let out = &mut *self.out;
		// The cluster's real advance: justification stretches spaces and
		// CJK glue, and backgrounds and decorations must cover it too.
		// Compression moves the ink with the blank half it spends on the
		// left, so an opening mark is pulled against what precedes it
		// instead of being overlapped by what follows.
		let pulled = if solve.ratio < 0.0 {
			fit.shrink.0 * -solve.ratio
		} else {
			0.0
		};
		let advance = if solve.ratio < 0.0 {
			c.width - fit.shrink() * -solve.ratio
		} else {
			c.width
				+ fit.stretch * solve.ratio
				+ if fit.share { solve.extra } else { 0.0 }
		};
		if !range.is_empty() {
			out.text[node].push(TextCluster {
				mixed_spacing: (
					if c.mixed.0 {
						size * microtype::MIXED_GAP - pulled
					} else {
						0.0
					},
					if c.mixed.1 {
						size * microtype::MIXED_GAP
							+ fit.shrink.1 * solve.ratio.min(0.0)
					} else {
						0.0
					},
				),
				range,
				rect: Rect {
					x: cursor,
					y: y_cursor,
					w: advance.max(1.0),
					h: height,
				},
				rtl: c.rtl,
				// A formula is a single drawn box, like an image.
				atomic: p.math.contains_key(&c.range.start),
				command: out.draws.len(),
			});
		}

		let span = interval::containing(&p.spans, c.range.start, |s| &s.range);
		self.update_link(c.range.start, span, note);
		self.paint_cluster(context, c, advance, pulled, span);
		self.cursor += advance;
	}

	fn update_link(
		&mut self,
		offset: usize,
		span: Option<usize>,
		note: Option<&FootnoteReference>,
	) {
		let p = self.paragraph.prepared;
		let mut link_id = p.link_id(offset, span);
		// A lone reference keeps its brackets in the hit target; every
		// number of a merged group resolves through `notes` instead.
		let span_note = span
			.and_then(|index| p.note_spans.get(&p.spans[index].range.start));
		let link_target = span
			.and_then(|index| p.spans[index].link.as_ref())
			.map(|link| LinkTarget::Url(link.shared().clone()))
			.or_else(|| {
				note.or(span_note).map(|reference| {
					LinkTarget::FootnoteDefinition(reference.definition.clone())
				})
			});
		if link_id.is_none()
			&& let Some(reference) = span_note
		{
			link_id = Some(LinkId::Inline(reference.occurrence));
		}
		// A link wraps as one run per line, so hit testing stays tight.
		let pair = link_target.as_ref().zip(link_id);
		if self.link.as_ref().map(|run| (&run.target, run.id)) != pair {
			if let Some(run) = self.link.take() {
				run.finish(self.cursor, self.geometry.rect, self.out);
			}
			if let (Some(target), Some(id)) = (link_target, link_id) {
				self.link = Some(LinkRun {
					target,
					id,
					x: self.cursor,
					command: self.out.draws.len(),
				});
			}
		}
	}

	fn paint_cluster(
		&mut self,
		context: &mut BlockContext<'_>,
		c: Cluster,
		advance: f32,
		pulled: f32,
		span: Option<usize>,
	) {
		let ParagraphLayout {
			prepared: p, size, ..
		} = *self.paragraph;
		let LineGeometry {
			rect: row,
			baseline,
			..
		} = self.geometry;
		let (y_cursor, height) = (row.y, row.h);
		let cursor = self.cursor;
		let out = &mut *self.out;
		let appearance = span
			.map(|index| {
				let span = &p.spans[index];
				context.shaper.stylesheet.inline_run(
					&context.shaper.appearance,
					&span.style,
					span.math_error,
					span.footnote_ref,
					span.link.is_some(),
				)
			})
			.unwrap_or_else(|| context.shaper.appearance.clone());
		if let Some(paint) = appearance.background {
			let rect = Rect {
				x: cursor,
				y: baseline - c.background.0,
				w: advance,
				h: (c.background.0 + c.background.1).max(0.),
			};
			let fragment = span.map(|index| p.spans[index].range.clone());
			BackgroundRun::push(
				&mut self.background,
				paint,
				fragment,
				rect,
				row,
				out,
			);
		} else {
			self.background = None;
		}
		if let Some(math) = p.math.get(&c.range.start) {
			out.draws.push(Draw::Math {
				math: math.clone(),
				paint: appearance
					.paint
					.cascade(Condition::Math, ColorField::Color),
				x: cursor,
				y: baseline - math.ascent,
			});
		} else {
			for mut g in c.glyphs {
				g.x += cursor - pulled;
				g.y += baseline;
				out.draws.push(Draw::Glyph(g));
			}
		}
		for decoration in &appearance.decoration {
			out.inline_decorations
				.push((out.draws.len(), y_cursor..y_cursor + height));
			out.draws.push(Draw::Rect(
				Rect {
					x: cursor,
					y: if *decoration == Decoration::Strike {
						baseline + c.baseline_shift
							- size * appearance.size * 0.3
					} else {
						baseline
							+ c.baseline_shift
							+ size * appearance.size * 0.12
					},
					w: advance,
					h: 1.0,
				},
				appearance.paint,
			));
		}
	}

	fn finish(self, start_draw: usize) -> f32 {
		let LineGeometry {
			rect,
			actual,
			target,
			..
		} = self.geometry;
		if let Some(run) = self.link {
			run.finish(self.cursor, rect, self.out);
		}
		let mut height = rect.h;
		if actual > target + 0.5 {
			let gutter = self.paragraph.opts.stylesheet.scrollbar_gutter();
			self.out.overflow.push(Overflow {
				rect,
				content_width: actual,
				commands: start_draw..self.out.draws.len(),
				gutter,
			});
			height += gutter;
		}
		self.out.width = self.out.width.max(rect.x + actual.min(rect.w));
		height
	}
}
