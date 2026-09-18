use super::{BlockContext, LayoutOptions};
use crate::text::{TextCluster, TextNode};
use crate::{
	document::{
		Block, BlockKind, CellAlign, Inline, InlineKind, RichText, footnote,
	},
	scene::{BlockLayout, Draw, HeadingAnchor, LinkRect, Paint, Rect},
	style::{ColorField, Condition, MarkerShape, TextAlign, TextAppearance},
};
use std::sync::Arc;

/// The diameter of a bullet shape, in multiples of the marker's own size. The
/// bundled marker size therefore draws a bullet about a third of an em.
const BULLET_SIDE: f32 = 0.36;

/// The column a list reserves before its item text, in logical pixels. An
/// ordered list widens it to fit the widest number its numbering pattern and
/// range produce, so a longer format never runs into the text.
const MARKER_COLUMN: f32 = 30.0;

/// The space a widened column leaves between a number and the text that
/// follows it, in multiples of the marker's own size.
const MARKER_GAP: f32 = 0.35;

/// The x a marker of `width` takes inside its reserved column, which runs from
/// the item's left edge to where its text begins.
fn marker_offset(align: TextAlign, column: f32, width: f32) -> f32 {
	// A left- or right-aligned marker keeps a hair of space at the edge.
	const INSET: f32 = 2.0;
	let free = (column - width).max(0.0);
	match align {
		TextAlign::Left => INSET,
		TextAlign::Center => free / 2.0,
		TextAlign::Right => (free - INSET).max(INSET),
	}
}

/// A bullet's vertices, relative to its center, fitting a square `side` wide.
fn marker_points(shape: MarkerShape, side: f32) -> Arc<[[f32; 2]]> {
	let radius = side / 2.;
	// The stroke of a plus or a minus, about a third of the shape's width.
	let arm = radius / 3.;
	let corner = |degrees: f32| {
		let angle = degrees.to_radians();
		[angle.cos() * radius, angle.sin() * radius]
	};
	let points = match shape {
		// Enough segments that the antialiased outline reads as a circle.
		MarkerShape::Disc => {
			(0..64).map(|i| corner(i as f32 * 360. / 64.)).collect()
		}
		MarkerShape::Square => {
			vec![
				[-radius, -radius],
				[radius, -radius],
				[radius, radius],
				[-radius, radius],
			]
		}
		MarkerShape::Triangle => {
			vec![[0., -radius], [radius, radius], [-radius, radius]]
		}
		MarkerShape::Diamond => {
			vec![[0., -radius], [radius, 0.], [0., radius], [-radius, 0.]]
		}
		MarkerShape::Plus => {
			vec![
				[-arm, -radius],
				[arm, -radius],
				[arm, -arm],
				[radius, -arm],
				[radius, arm],
				[arm, arm],
				[arm, radius],
				[-arm, radius],
				[-arm, arm],
				[-radius, arm],
				[-radius, -arm],
				[-arm, -arm],
			]
		}
		MarkerShape::Minus => {
			vec![
				[-radius, -arm],
				[radius, -arm],
				[radius, arm],
				[-radius, arm],
			]
		}
	};
	Arc::from(points)
}

/// The element a block's box belongs to.
fn block_role(block: &Block) -> Condition {
	match &block.kind {
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
	}
}

/// The spacing a block reserves outside its box, given the appearance its
/// parent established.
fn outer_spacing(
	block: &Block,
	parent: &TextAppearance,
	opts: &LayoutOptions,
) -> (f32, f32) {
	let role = block_role(block);
	let appearance = opts.stylesheet.text(parent, role);
	let rule = opts.stylesheet.element_rule(appearance.chain, role);
	(
		rule.space_before.unwrap_or(0.) * opts.font_size,
		rule.space_after.unwrap_or(0.) * opts.font_size,
	)
}

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

	/// Lay out children that a box with visible edges frames. The opening space
	/// of the first child and the closing space of the last one are outer
	/// spacing, so they stay outside the box: keeping them would leave its
	/// padding and border lopsided around the content.
	pub(super) fn framed_children(
		&mut self,
		blocks: &[Block],
		x: f32,
		y: f32,
		width: f32,
		opts: &LayoutOptions,
		out: &mut BlockLayout,
	) -> f32 {
		let parent = self.shaper.appearance.clone();
		let (lead, trail) = match (blocks.first(), blocks.last()) {
			(Some(first), Some(last)) => (
				outer_spacing(first, &parent, opts).0,
				outer_spacing(last, &parent, opts).1,
			),
			_ => (0., 0.),
		};
		self.children(blocks, x, y - lead, width, opts, 0., out) - lead - trail
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
		let role = block_role(block);
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
				top - y + self.framed_children(blocks, x, top, width, opts, out)
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
				// Every item in the list draws the same bullet graphic. Its
				// shape is the cycle entry for this list's nesting depth.
				let bullet =
					opts.stylesheet.text(&item_appearance, Condition::Marker);
				let bullet_side = opts.font_size * bullet.size * BULLET_SIDE;
				let shapes = opts.stylesheet.marker_shapes();
				let bullet_points = marker_points(
					shapes[self.marker_depth % shapes.len()],
					bullet_side,
				);
				// A nested list inside an item is one bullet level deeper, and
				// one ordered level takes the next counting symbol. This list's
				// own items number at the depth it was entered at.
				let enum_depth = self.enum_depth;
				if start.is_none() {
					self.marker_depth += 1;
				} else {
					self.enum_depth += 1;
				}
				let numbering = opts.stylesheet.enum_numbering();
				let number_align = opts.stylesheet.enum_align();
				// One column holds every marker of the list, so the item text
				// starts at one x. An ordered list widens it to its own numbers.
				let mut column = MARKER_COLUMN;
				if let Some(start) = *start {
					let gap = MARKER_GAP * opts.font_size * bullet.size;
					for (i, item) in items.iter().enumerate() {
						if item.checked.is_some() {
							continue;
						}
						let label =
							numbering.number(enum_depth, (start + i) as u64);
						let (_, width) = self.shaper.label_with(
							&label,
							opts.font_size,
							0.,
							0.,
							&bullet,
							bullet.paint,
							None,
						);
						column = column.max(width + gap);
					}
				}
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

					// Only an ordered number is reading text. Bullets and task
					// checkboxes are drawn, so nothing about them is selectable.
					let numbered = item.checked.is_none() && start.is_some();
					if numbered {
						let label = numbering
							.number(enum_depth, (start.unwrap() + i) as u64);
						let (mut draws, width) = self.shaper.label_with(
							&label,
							opts.font_size,
							item_x,
							top + size * 1.15,
							&bullet,
							bullet.paint,
							Some(Paint::Scoped(
								bullet.chain,
								Condition::Marker,
								ColorField::Background,
							)),
						);
						// Shaping starts at the column's left edge; alignment
						// moves the finished label without reshaping it.
						let dx = marker_offset(number_align, column, width);
						for draw in &mut draws {
							draw.translate(dx, 0.);
						}
						let command = out.draws.len();
						out.draws.extend(draws);
						// The number copies as its own word before the item.
						let mut node = TextNode::new(format!("{label} "), "\n");
						node.push(TextCluster {
							range: 0..node.text.len(),
							rect: Rect {
								x: item_x + dx,
								y: top,
								w: width,
								h: size * self.shaper.appearance.line_height,
							},
							rtl: false,
							command,
						});
						out.text.push(node);
					}
					let first_child = out.text.len();
					if let Some(checked) = item.checked {
						let task = opts.stylesheet.text(
							&self.shaper.appearance,
							Condition::TaskMarker,
						);
						let marker_size = opts.font_size * task.size;
						let box_size = marker_size * 0.7;
						let r = Rect {
							x: item_x
								+ marker_offset(
									opts.stylesheet.marker_align(true),
									column,
									box_size,
								),
							y: top + size * 0.5,
							w: box_size,
							h: box_size,
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
					} else if !numbered {
						let left = item_x
							+ marker_offset(
								opts.stylesheet.marker_align(false),
								column,
								bullet_side,
							);
						out.draws.push(Draw::Polygon {
							center: [
								left + bullet_side / 2.,
								top + size * 0.88,
							],
							points: bullet_points.clone(),
							paint: bullet.paint,
						});
					}
					top += self
						.children(
							&item.blocks,
							item_x + column,
							top,
							(item_width - column).max(1.),
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
						// A number shares its line with the item text; an item
						// without one starts its own line.
						node.separator = if numbered { "" } else { "\n" };
					}
				}
				if start.is_none() {
					self.marker_depth -= 1;
				} else {
					self.enum_depth -= 1;
				}
				self.shaper.appearance = list_appearance;
				top - y
			}
			BlockKind::Table { align, rows } => {
				self.table(align, rows, x, y, width, opts, out)
			}
			BlockKind::Footnote {
				label,
				column,
				blocks,
			} => {
				let text = format!("[{label}]");
				let paint =
					Paint::Styled(Condition::FootnoteRef, ColorField::Color);
				// Every note reserves the same marker column, so their bodies
				// start at one x even when the numbers differ in width. The
				// number itself is shaped at the origin and moved onto the
				// body's first baseline below.
				let (mut draws, label_width) = self.shaper.label_measured(
					&text,
					opts.font_size,
					x,
					0.0,
					paint,
				);
				let reserved = format!("[{}]", "0".repeat(*column as usize));
				let (_, column_width) = self.shaper.label_measured(
					&reserved,
					opts.font_size,
					x,
					0.0,
					paint,
				);
				// The label leads the block, so its paragraphs stay flush.
				let body_opts = LayoutOptions {
					paragraph_indent: 0.0,
					..opts.clone()
				};
				let body_x = x + column_width.max(label_width) + size * 0.5;
				let body_start = out.draws.len();
				let body = self.children(
					blocks,
					body_x,
					y,
					width - (body_x - x),
					&body_opts,
					size * 0.5,
					out,
				);
				// The note opens with text on almost every document, and its
				// first glyph carries the baseline the number shares.
				let baseline = out.draws[body_start..]
					.iter()
					.find_map(|d| match d {
						Draw::Glyph(g) => Some(g.y),
						_ => None,
					})
					.unwrap_or(y + size * 1.15);
				for draw in &mut draws {
					draw.translate(0.0, baseline);
				}
				let command = out.draws.len();
				out.draws.extend(draws);
				// The number is the way back to the reference that opened the
				// note, so it is a link with the note's own label.
				out.links.push(LinkRect {
					command,
					rect: Rect {
						x,
						y: baseline - size,
						w: label_width.max(1.0),
						h: size * 1.4,
					},
					url: footnote::back_url(label),
				});
				body
			}
		};
		out.height = out.height.max(y + height);
		out.width = out.width.max(x + width);
		height
	}
}
