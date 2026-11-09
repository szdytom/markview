use super::{BlockContext, LayoutOptions};
use crate::{
	document::{CellAlign, Inline, InlineKind, TextStyle},
	scene::{BlockLayout, Draw, Paint, Rect},
	style::{ColorField, Condition},
	text::{TextCluster, TextNode},
};

impl BlockContext<'_> {
	/// The image box scaled into the paragraph measure, like `max-width: 100%`.
	pub(super) fn image_size(
		&self,
		image: &crate::image::ImageSpec,
		available: f32,
		size: f32,
	) -> (f32, f32) {
		let inset = self.image_insets(size, available);
		let info = self
			.images
			.key(&image.source)
			.and_then(|key| self.images.entries.get(key));
		let (w, h) = crate::image::image_size(
			image,
			info,
			(available - inset[1] - inset[3]).max(1.),
		);
		(w + inset[1] + inset[3], h + inset[0] + inset[2])
	}

	pub(super) fn image_placeholder(
		&self,
		image: &crate::image::ImageSpec,
	) -> Option<String> {
		let info = self
			.images
			.key(&image.source)
			.and_then(|key| self.images.entries.get(key));
		let message = match info {
			Some(i) if i.size.is_some() && i.error.is_none() && !i.loading => {
				return None;
			}
			Some(i) if i.error.is_some() => i.error.as_deref().unwrap(),
			_ => "Loading image…",
		};
		Some(if image.alt.is_empty() {
			message.to_owned()
		} else {
			format!("{} · {message}", image.alt)
		})
	}

	pub(super) fn image_insets(&self, size: f32, available: f32) -> [f32; 4] {
		let rule = self.shaper.stylesheet.rule(Condition::Image);
		let base = size / self.shaper.appearance.size;
		let border = rule.border_width.unwrap_or(0.);
		let mut inset = rule
			.padding
			.as_ref()
			.map(|p| p.sides().map(|v| v * base + border))
			.unwrap_or([border; 4]);
		let scale =
			((available - 1.).max(0.) / (inset[1] + inset[3]).max(1.)).min(1.);
		inset[1] *= scale;
		inset[3] *= scale;
		inset
	}

	pub(super) fn draw_image(
		&mut self,
		image: &crate::image::ImageSpec,
		rect: Rect,
		size: f32,
		available: f32,
		opts: &LayoutOptions,
		out: &mut BlockLayout,
	) -> Vec<TextCluster> {
		let mut text_clusters = Vec::new();
		let key = self.images.key(&image.source).unwrap_or_default();
		let info = self.images.entries.get(key);
		let inset = self.image_insets(size, available);
		let content = Rect {
			x: rect.x + inset[3],
			y: rect.y + inset[0],
			w: (rect.w - inset[1] - inset[3]).max(1.),
			h: (rect.h - inset[0] - inset[2]).max(1.),
		};
		out.draws.push(Draw::Image {
			src: key.to_owned(),
			version: info.map_or(0, |i| i.version),
			rect: content,
			title: image.title.clone(),
		});
		out.draws.push(Draw::Box {
			rect,
			chain: Condition::Image.chain(),
			condition: Condition::Image,
			radius: 0.,
			border: self
				.shaper
				.stylesheet
				.rule(Condition::Image)
				.border_width
				.unwrap_or(0.)
				.min(inset[1])
				.min(inset[3]),
			left_only: false,
			decoration: None,
		});
		if let Some(text) = self.image_placeholder(image) {
			let rect = content;
			out.draws.push(Draw::Rect(
				rect,
				Paint::Styled(Condition::Placeholder, ColorField::Background),
			));
			let old = self.shaper.appearance.clone();
			let base = size / old.size;
			let image = self.shaper.stylesheet.text(&old, Condition::Image);
			self.shaper.appearance =
				self.shaper.stylesheet.text(&image, Condition::Placeholder);
			let label_size = base * self.shaper.appearance.size;
			let pad = 6.;
			let width = (rect.w - 2. * pad).max(0.);
			let height = (rect.h - 2. * pad).max(0.);
			let step = label_size * self.shaper.appearance.line_height;
			let lines = if step > 0. && width > 0. {
				(height / step).floor() as usize
			} else {
				0
			};
			// The placeholder is typeset by the paragraph engine, so it wraps,
			// justifies and hyphenates like body text; `lines` bounds it to the
			// image box and the last line carries the ellipsis.
			if lines > 0 {
				let mut decoration = BlockLayout::default();
				self.paragraph_bounded(
					&[Inline {
						link: None,
						kind: InlineKind::Text(text),
						style: TextStyle::default(),
					}],
					rect.x + pad,
					rect.y + pad,
					width,
					label_size,
					false,
					CellAlign::Left,
					opts.justify,
					false,
					Some(lines),
					opts,
					&mut decoration,
				);
				let offset = out.draws.len();
				out.inline_decorations.extend(
					decoration
						.inline_decorations
						.into_iter()
						.map(|(command, row)| (command + offset, row)),
				);
				for node in decoration.text {
					for mut cluster in node.clusters {
						cluster.command += offset;
						text_clusters.push(cluster);
					}
				}
				out.draws.extend(decoration.draws);
				out.overflow.extend(decoration.overflow.into_iter().map(
					|mut o| {
						o.commands.start += offset;
						o.commands.end += offset;
						o
					},
				));
				out.degraded += decoration.degraded;
			}
			self.shaper.appearance = old;
		}
		out.width = out.width.max(rect.x + rect.w);
		text_clusters
	}

	pub(super) fn image_caption(
		&mut self,
		image: &crate::image::ImageSpec,
		x: f32,
		y: f32,
		width: f32,
		opts: &LayoutOptions,
		out: &mut BlockLayout,
	) -> f32 {
		let mut y_cursor = y;
		let captioned = opts
			.stylesheet
			.text(&self.shaper.appearance, Condition::Image);
		let captioned = opts.stylesheet.text(&captioned, Condition::Caption);
		let rule = opts
			.stylesheet
			.element_rule(captioned.chain, Condition::Caption);
		if let Some(caption) = rule.source.unwrap_or_default().text(image) {
			let old = self.shaper.appearance.clone();
			self.shaper.appearance = opts.stylesheet.text(
				&opts.stylesheet.text(&old, Condition::Image),
				Condition::Caption,
			);
			let caption_size = opts.font_size * self.shaper.appearance.size;
			y_cursor += rule.space_before.unwrap_or(0.) * opts.font_size;
			let mut decoration = BlockLayout::default();
			y_cursor += self.paragraph(
				&[Inline {
					link: None,
					kind: InlineKind::Text(caption.to_owned()),
					style: TextStyle::default(),
				}],
				x,
				y_cursor,
				width,
				caption_size,
				false,
				rule.align.map(Into::into).unwrap_or(CellAlign::Center),
				false,
				false,
				opts,
				&mut decoration,
			);
			let offset = out.draws.len();
			out.inline_decorations.extend(
				decoration
					.inline_decorations
					.into_iter()
					.map(|(command, row)| (command + offset, row)),
			);
			for mut node in decoration.text {
				node.separator = "\n";
				for cluster in &mut node.clusters {
					cluster.command += offset;
				}
				out.text.push(node);
			}
			out.draws.extend(decoration.draws);
			out.overflow.extend(decoration.overflow.into_iter().map(
				|mut o| {
					o.commands.start += offset;
					o.commands.end += offset;
					o
				},
			));
			out.degraded += decoration.degraded;
			y_cursor += rule.space_after.unwrap_or(0.) * opts.font_size;
			self.shaper.appearance = old;
		} else {
			// Reserve the caption's ordinal so toggling it cannot renumber
			// subsequent paragraphs or table cells in this cached block.
			out.text.push(TextNode::new(String::new(), "\n"));
		}
		y_cursor - y
	}
}
