use super::{BlockContext, LayoutOptions};
use crate::text::{TextCluster, TextNode};
use crate::{
	document::{
		Block, BlockKind, CellAlign, Inline, InlineKind, RichText, footnote,
	},
	scene::{BlockLayout, Draw, HeadingAnchor, LinkRect, Paint, Rect},
	style::{ColorField, Condition},
};

/// A paragraph earns a first-line indent only when its first visible content is
/// text. A leading image or display formula is a centered figure or block, so
/// indenting it would shift it away from its own margin.
fn starts_with_text(rich: &[Inline]) -> bool {
	for inline in rich {
		match &inline.kind {
			InlineKind::Text(text) if text.trim().is_empty() => {}
			InlineKind::Text(_)
			| InlineKind::Math { display: false, .. }
			| InlineKind::FootnoteRef(_) => {
				return true;
			}
			// A break on its own does not make a paragraph start with text.
			InlineKind::LineBreak { .. } => {}
			InlineKind::Image(_) | InlineKind::Math { display: true, .. } => {
				return false;
			}
		}
	}
	false
}

impl BlockContext<'_> {
	#[expect(
		clippy::too_many_arguments,
		reason = "Text style and block geometry are independent layout inputs"
	)]
	pub(super) fn rich(
		&mut self,
		rich: &RichText,
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
		crate::profile::span(crate::profile::Stage::Rich, || {
			self.rich_inner(
				rich, x, y, width, size, sans, align, justify, indent, opts,
				out,
			)
		})
	}

	#[expect(
		clippy::too_many_arguments,
		reason = "Text style and block geometry are independent layout inputs"
	)]
	fn rich_inner(
		&mut self,
		rich: &RichText,
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
		let first_node = out.text.len();
		let mut start = 0;
		let mut cursor = y;
		for (i, inline) in rich.iter().enumerate() {
			if let InlineKind::Math { display: true, .. } = inline.kind {
				if i > start {
					cursor += self.paragraph(
						&rich[start..i],
						x,
						cursor,
						width,
						size,
						sans,
						align,
						justify,
						indent && start == 0,
						opts,
						out,
					);
				}
				cursor += size * 0.5;
				cursor += self.paragraph(
					&rich[i..i + 1],
					x,
					cursor,
					width,
					size,
					false,
					CellAlign::Center,
					false,
					false,
					opts,
					out,
				);
				cursor += size * 0.5;
				start = i + 1;
			}
		}
		if start < rich.len() {
			cursor += self.paragraph(
				&rich[start..],
				x,
				cursor,
				width,
				size,
				sans,
				align,
				justify,
				indent && start == 0,
				opts,
				out,
			);
		}
		if let Some(node) = out.text.get_mut(first_node) {
			node.separator = "\n\n";
		}
		cursor - y
	}

	#[expect(
		clippy::too_many_arguments,
		reason = "Recursive block geometry and spacing"
	)]
	pub(super) fn children(
		&mut self,
		blocks: &[Block],
		x: f32,
		y: f32,
		width: f32,
		opts: &LayoutOptions,
		_gap: f32,
		out: &mut BlockLayout,
	) -> f32 {
		let mut cursor = y;
		for block in blocks {
			cursor += self.block(block, x, cursor, width, opts, out);
		}
		cursor - y
	}

	pub(super) fn block(
		&mut self,
		block: &Block,
		x: f32,
		y: f32,
		width: f32,
		opts: &LayoutOptions,
		out: &mut BlockLayout,
	) -> f32 {
		let role = match &block.kind {
			BlockKind::Paragraph(_) => Condition::P,
			BlockKind::Heading { level, .. } => Condition::heading(*level),
			BlockKind::Code { .. } => Condition::CodeBlock,
			BlockKind::Quote { .. } => Condition::Blockquote,
			BlockKind::List { start, .. } => {
				if start.is_some() {
					Condition::Enum
				} else {
					Condition::List
				}
			}
			BlockKind::Table { .. } => Condition::Table,
			BlockKind::Footnote { .. } => Condition::Footnote,
			BlockKind::Rule => Condition::Hr,
		};
		let previous = self.shaper.appearance.clone();
		let appearance = opts.stylesheet.text(&previous, role);
		let chain = appearance.chain;
		let rule = opts.stylesheet.element_rule(chain, role);
		self.shaper.appearance = appearance;
		self.shaper.appearance.background = None;
		let before = rule.space_before.unwrap_or(0.) * opts.font_size;
		let after = rule.space_after.unwrap_or(0.) * opts.font_size;
		let pad = rule
			.padding
			.as_ref()
			.map(|p| p.sides().map(|v| v * opts.font_size))
			.unwrap_or([0.; 4]);
		let inner_y = y + before + pad[0];
		let placeholder = out.draws.len();
		out.draws.push(Draw::Box {
			rect: Rect::default(),
			chain,
			condition: role,
			radius: rule.radius.unwrap_or(0.),
			border: rule.border_width.unwrap_or(0.),
			left_only: role == Condition::Blockquote,
		});
		let height = self.block_inner(
			block,
			x + pad[3],
			inner_y,
			(width - pad[1] - pad[3]).max(1.),
			opts,
			out,
		);
		let box_height = pad[0] + height + pad[2];
		out.draws[placeholder] = Draw::Box {
			rect: Rect {
				x,
				y: y + before,
				w: width,
				h: box_height,
			},
			chain,
			condition: role,
			radius: rule.radius.unwrap_or(0.),
			border: if role == Condition::Hr || role == Condition::Table {
				0.
			} else {
				rule.border_width.unwrap_or(0.)
			},
			left_only: role == Condition::Blockquote,
		};
		if let BlockKind::Heading { anchor, .. } = &block.kind {
			// A link to this heading lands on the top of its box.
			out.anchors.push(HeadingAnchor {
				anchor: anchor.clone(),
				y: y + before,
			});
		}
		if let BlockKind::Footnote { label, .. } = &block.kind {
			// A footnote reference lands on the top of the note's box.
			out.anchors.push(HeadingAnchor {
				anchor: footnote::anchor(label),
				y: y + before,
			});
		}
		self.shaper.appearance = previous;
		let total = before + box_height + after;
		out.height = out.height.max(y + total);
		out.width = out.width.max(x + width);
		total
	}
	pub(super) fn block_inner(
		&mut self,
		block: &Block,
		x: f32,
		y: f32,
		width: f32,
		opts: &LayoutOptions,
		out: &mut BlockLayout,
	) -> f32 {
		let size = opts.font_size * self.shaper.appearance.size;
		let width = width.max(40.0);
		let height = match &block.kind {
			BlockKind::Paragraph(text) => self.rich(
				text,
				x,
				y,
				width,
				size,
				false,
				CellAlign::Left,
				opts.justify,
				starts_with_text(text),
				opts,
				out,
			),
			BlockKind::Heading { text, .. } => self.rich(
				text,
				x,
				y,
				width,
				size,
				true,
				CellAlign::Left,
				false,
				false,
				opts,
				out,
			),
			BlockKind::Rule => {
				out.draws.push(Draw::Rect(
					Rect {
						x,
						y,
						w: width,
						h: opts
							.stylesheet
							.rule(Condition::Hr)
							.border_width
							.unwrap_or(1.),
					},
					Paint::Styled(Condition::Hr, ColorField::Color),
				));
				opts.stylesheet
					.rule(Condition::Hr)
					.border_width
					.unwrap_or(1.)
			}
			BlockKind::Code { language, text } => {
				self.code(language, text, x, y, width, size, opts, out)
			}
			BlockKind::Quote { label, blocks } => {
				let mut top = y;
				if let Some(label) = label {
					out.draws.extend(self.shaper.label(
						label,
						size * 0.8,
						x,
						top + size,
						Paint::Styled(Condition::Blockquote, ColorField::Color),
					));
					top += size * self.shaper.appearance.line_height;
				}
				top - y
					+ self.children(
						blocks,
						x,
						top,
						width,
						opts,
						size * 0.6,
						out,
					)
			}
			BlockKind::List {
				start,
				tight: _,
				items,
			} => {
				// A list indents as a whole, markers included, so its items line
				// up with the indented opening lines of paragraphs. The item
				// text does not indent again, and nested blocks inherit this
				// single shift. A theme may inset bullet and ordered lists by
				// different amounts on top of the reader's paragraph indent.
				let indent = (opts.indent(size, width)
					+ opts.stylesheet.list_indent(start.is_some())
						* opts.font_size)
					.min((width - size).max(0.0));
				let x = x + indent;
				let width = (width - indent).max(1.0);
				let item_opts = LayoutOptions {
					paragraph_indent: 0.0,
					..opts.clone()
				};
				let mut top = y;
				let list_appearance = self.shaper.appearance.clone();
				let item_appearance =
					opts.stylesheet.text(&list_appearance, Condition::ListItem);
				let item_rule = opts
					.stylesheet
					.element_rule(item_appearance.chain, Condition::ListItem);
				let padding = item_rule
					.padding
					.as_ref()
					.map(|p| p.sides().map(|v| v * opts.font_size))
					.unwrap_or([0.; 4]);
				for (i, item) in items.iter().enumerate() {
					self.shaper.appearance = item_appearance.clone();
					top +=
						item_rule.space_before.unwrap_or(0.) * opts.font_size;
					let box_y = top;
					let box_index = out.draws.len();
					out.draws.push(Draw::Rect(Rect::default(), Paint::Text));
					top += padding[0];
					let item_x = x + padding[3];
					let item_width = (width - padding[1] - padding[3]).max(1.);

					let marker = match item.checked {
						Some(true) => "[x] ".into(),
						Some(false) => "[ ] ".into(),
						None => start.map_or_else(
							|| "• ".into(),
							|n| format!("{}. ", n + i),
						),
					};
					let mut node = TextNode::new(marker.clone(), "\n");
					node.push(TextCluster {
						range: 0..marker.len(),
						rect: Rect {
							x,
							y: top,
							w: 25.0,
							h: size * self.shaper.appearance.line_height,
						},
						rtl: false,
						command: out.draws.len(),
					});
					out.text.push(node);
					let first_child = out.text.len();
					let indent = if start.is_some_and(|n| n + i >= 100) {
						48.0
					} else {
						30.0
					};
					if let Some(checked) = item.checked {
						let task = opts.stylesheet.text(
							&self.shaper.appearance,
							Condition::TaskMarker,
						);
						let marker_size = opts.font_size * task.size;
						let r = Rect {
							x: item_x + 2.,
							y: top + size * 0.5,
							w: marker_size * 0.7,
							h: marker_size * 0.7,
						};
						out.draws.push(Draw::Rect(
							r,
							Paint::Cascade(task.chain, ColorField::BorderColor),
						));
						out.draws.push(Draw::Rect(
							Rect {
								x: r.x + 1.,
								y: r.y + 1.,
								w: (r.w - 2.).max(0.),
								h: (r.h - 2.).max(0.),
							},
							Paint::Cascade(task.chain, ColorField::Background),
						));
						if checked {
							out.draws.extend(
								self.shaper
									.label_with(
										"✓",
										opts.font_size * 0.7,
										r.x,
										r.y + r.h,
										&task,
										task.paint,
										Some(Paint::Scoped(
											task.chain,
											Condition::TaskMarker,
											ColorField::Background,
										)),
									)
									.0,
							);
						}
					} else {
						let marker = start.map_or_else(
							|| "•".to_string(),
							|n| format!("{}.", n + i),
						);
						let bullet = opts
							.stylesheet
							.text(&self.shaper.appearance, Condition::Marker);
						out.draws.extend(
							self.shaper
								.label_with(
									&marker,
									opts.font_size,
									item_x + 2.0,
									top + size * 1.15,
									&bullet,
									bullet.paint,
									Some(Paint::Scoped(
										bullet.chain,
										Condition::Marker,
										ColorField::Background,
									)),
								)
								.0,
						);
					}
					top += self
						.children(
							&item.blocks,
							item_x + indent,
							top,
							(item_width - indent).max(1.),
							&item_opts,
							size * 0.6,
							out,
						)
						.max(size * self.shaper.appearance.line_height);
					top += padding[2];
					out.draws[box_index] = Draw::Box {
						rect: Rect {
							x,
							y: box_y,
							w: width,
							h: top - box_y,
						},
						chain: item_appearance.chain,
						condition: Condition::ListItem,
						radius: item_rule.radius.unwrap_or(0.),
						border: item_rule.border_width.unwrap_or(0.),
						left_only: false,
					};
					top += item_rule.space_after.unwrap_or(0.) * opts.font_size;
					if let Some(node) = out.text.get_mut(first_child) {
						node.separator = "";
					}
				}
				self.shaper.appearance = list_appearance;
				top - y
			}
			BlockKind::Table { align, rows } => {
				self.table(align, rows, x, y, width, opts, out)
			}
			BlockKind::Footnote { label, blocks } => {
				let text = format!("[{label}]");
				let label_size = size * 0.75;
				let command = out.draws.len();
				let (draws, label_width) = self.shaper.label_measured(
					&text,
					label_size,
					x,
					y + size,
					Paint::Styled(Condition::FootnoteRef, ColorField::Color),
				);
				out.draws.extend(draws);
				// The number is the way back to the reference that opened the
				// note, so it is a link with the note's own label.
				out.links.push(LinkRect {
					command,
					rect: Rect {
						x,
						y: y + size - label_size,
						w: label_width.max(1.0),
						h: label_size * 1.4,
					},
					url: footnote::back_url(label),
				});
				// The label leads the block, so its paragraphs stay flush.
				let body_opts = LayoutOptions {
					paragraph_indent: 0.0,
					..opts.clone()
				};
				self.children(
					blocks,
					x + 36.0,
					y,
					width - 36.0,
					&body_opts,
					size * 0.5,
					out,
				)
			}
		};
		out.height = out.height.max(y + height);
		out.width = out.width.max(x + width);
		height
	}
}
