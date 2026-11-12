use super::{
	BULLET_SIDE, BlockContext, LayoutOptions, MARKER_COLUMN, MARKER_GAP,
	TASK_BORDER, TASK_BOX, anchor_only, check_points, marker_min_width,
	marker_offset, marker_points, outer_spacing, rounded_square,
	rounded_square_ring,
};
use crate::{
	document::ListItem,
	scene::{BlockLayout, BoxDecoration, Draw, Paint, Rect},
	shaping::TextShaper,
	style::{ColorField, Condition, Rule, TextAlign, TextAppearance},
	text::{TextCluster, TextNode},
};
use std::{ops::Range, sync::Arc};

#[cfg(test)]
mod tests;

struct TaskMarker {
	appearance: TextAppearance,
	side: f32,
	align: TextAlign,
	border: f32,
	fill: Arc<[[f32; 2]]>,
	ring: Arc<[[f32; 2]]>,
	check: Option<Arc<[[f32; 2]]>>,
}

impl TaskMarker {
	fn new(
		parent: &TextAppearance,
		items: &[ListItem],
		opts: &LayoutOptions,
	) -> Self {
		let appearance = opts.stylesheet.text(parent, Condition::TaskMarker);
		let rule = opts
			.stylesheet
			.element_rule(appearance.chain, Condition::TaskMarker);
		let side = opts.font_size * appearance.size * TASK_BOX;
		let border = rule.border_width.unwrap_or(TASK_BORDER);
		let radius = rule.radius.unwrap_or(0.);
		Self {
			appearance,
			side,
			align: opts.stylesheet.marker_align(true),
			border,
			fill: Arc::from(rounded_square(side, radius)),
			ring: Arc::from(rounded_square_ring(side, radius, border)),
			check: items
				.iter()
				.any(|item| item.checked == Some(true))
				.then(|| check_points(side)),
		}
	}

	fn draw(&self, center: [f32; 2], checked: bool, out: &mut BlockLayout) {
		// A completed box paints the accent over its surface fill.
		let fill = |field| Draw::Polygon {
			center,
			points: self.fill.clone(),
			paint: Paint::Scoped(
				self.appearance.chain,
				Condition::TaskMarker,
				field,
			),
		};
		out.draws.push(fill(ColorField::Background));
		if checked {
			out.draws.push(fill(ColorField::Accent));
		}
		if self.border > 0. {
			out.draws.push(Draw::Polygon {
				center,
				points: self.ring.clone(),
				paint: Paint::Scoped(
					self.appearance.chain,
					Condition::TaskMarker,
					ColorField::BorderColor,
				),
			});
		}
		if checked && let Some(points) = &self.check {
			out.draws.push(Draw::Polygon {
				center,
				points: points.clone(),
				paint: self.appearance.paint,
			});
		}
	}
}

/// Shared appearance and marker geometry for every item in one list.
struct ListStyle {
	size: f32,
	appearance: TextAppearance,
	rule: Rule,
	padding: [f32; 4],
	bullet: TextAppearance,
	bullet_side: f32,
	bullet_points: Arc<[[f32; 2]]>,
	task: TaskMarker,
	column: f32,
}

impl ListStyle {
	fn new(
		parent: &TextAppearance,
		ordered: bool,
		items: &[ListItem],
		depth: usize,
		opts: &LayoutOptions,
	) -> Self {
		let appearance = opts.stylesheet.text(parent, Condition::ListItem);
		let rule = opts
			.stylesheet
			.element_rule(appearance.chain, Condition::ListItem);
		let padding = rule
			.padding
			.as_ref()
			.map(|p| p.sides().map(|v| v * opts.font_size))
			.unwrap_or([0.; 4]);
		let bullet = opts.stylesheet.text(&appearance, Condition::Marker);
		let bullet_side = opts.font_size * bullet.size * BULLET_SIDE;
		let shapes = opts.stylesheet.marker_shapes();
		let bullet_points =
			marker_points(shapes[depth % shapes.len()], bullet_side);
		let task = TaskMarker::new(&appearance, items, opts);
		let role = if ordered {
			Condition::Enum
		} else {
			Condition::List
		};
		let gap = MARKER_GAP * opts.font_size * bullet.size;
		let mut column = opts
			.stylesheet
			.element_rule(parent.chain, role)
			.marker_width
			.map_or(MARKER_COLUMN, |v| v * opts.font_size);
		if items.iter().any(|item| item.checked.is_some()) {
			column = column.max(marker_min_width(task.align, task.side, gap));
		}
		if items.iter().any(|item| item.checked.is_none()) {
			column = column.max(marker_min_width(
				opts.stylesheet.marker_align(false),
				bullet_side,
				gap,
			));
		}
		Self {
			size: opts.font_size * parent.size,
			appearance,
			rule,
			padding,
			bullet,
			bullet_side,
			bullet_points,
			task,
			column,
		}
	}

	fn measure_numbers(
		&mut self,
		shaper: &mut TextShaper,
		items: &[ListItem],
		values: &[isize],
		depth: usize,
		opts: &LayoutOptions,
	) {
		let gap = MARKER_GAP * opts.font_size * self.bullet.size;
		for (item, value) in items.iter().zip(values) {
			if item.checked.is_some() {
				continue;
			}
			let label = opts
				.stylesheet
				.enum_numbering()
				.number(depth, *value as i64);
			let (_, width) = shaper.label_with(
				&label,
				opts.font_size,
				0.,
				0.,
				&self.bullet,
				self.bullet.paint,
				None,
			);
			self.column = self.column.max(marker_min_width(
				opts.stylesheet.enum_align(),
				width,
				gap,
			));
		}
	}
}

fn list_values(
	start: Option<isize>,
	reversed: bool,
	items: &[ListItem],
) -> Vec<isize> {
	let Some(mut value) = start else {
		return Vec::new();
	};
	items
		.iter()
		.map(|item| {
			if let Some(number) = item.number {
				value = number;
			}
			let current = value;
			value = if reversed {
				value.saturating_sub(1)
			} else {
				value.saturating_add(1)
			};
			current
		})
		.collect()
}

fn closing_space(
	item: &ListItem,
	appearance: &TextAppearance,
	opts: &LayoutOptions,
) -> f32 {
	let Some(last) = item.blocks.iter().rfind(|b| !anchor_only(b)) else {
		return 0.;
	};
	let count = item.blocks.iter().filter(|b| !anchor_only(b)).count();
	let parent = opts.stylesheet.child(appearance, count - 1, count);
	outer_spacing(last, &parent, opts).1
}

impl BlockContext<'_> {
	#[expect(
		clippy::too_many_arguments,
		reason = "List numbering, content and geometry"
	)]
	pub(super) fn list(
		&mut self,
		start: Option<isize>,
		reversed: bool,
		items: &[ListItem],
		[x, y]: [f32; 2],
		width: f32,
		opts: &LayoutOptions,
		out: &mut BlockLayout,
	) -> f32 {
		let size = opts.font_size * self.shaper.appearance.size;
		// The whole list indents once, markers included; its paragraphs stay flush.
		let indent = (opts.indent(size, width)
			+ opts.stylesheet.list_indent(start.is_some()) * opts.font_size)
			.min((width - size).max(0.));
		let x = x + indent;
		let width = (width - indent).max(1.);
		let item_opts = LayoutOptions {
			paragraph_indent: 0.,
			..opts.clone()
		};
		let parent = self.shaper.appearance.clone();
		let mut style = ListStyle::new(
			&parent,
			start.is_some(),
			items,
			self.marker_depth,
			opts,
		);
		let enum_depth = self.enum_depth;
		if start.is_none() {
			self.marker_depth += 1;
		} else {
			self.enum_depth += 1;
		}
		let values = list_values(start, reversed, items);
		style.measure_numbers(self.shaper, items, &values, enum_depth, opts);
		let mut height = 0.;
		for (i, item) in items.iter().enumerate() {
			if (self.cancelled)() {
				break;
			}
			let label =
				(start.is_some() && item.checked.is_none()).then(|| {
					opts.stylesheet
						.enum_numbering()
						.number(enum_depth, values[i] as i64)
				});
			height += self.list_item(
				item,
				label.as_deref(),
				i + 1 == items.len(),
				[x, y + height],
				width,
				&style,
				&item_opts,
				out,
			);
		}
		if start.is_none() {
			self.marker_depth -= 1;
		} else {
			self.enum_depth -= 1;
		}
		self.shaper.appearance = parent;
		height
	}

	#[expect(
		clippy::too_many_arguments,
		reason = "Item content, marker and geometry"
	)]
	fn list_item(
		&mut self,
		item: &ListItem,
		label: Option<&str>,
		last: bool,
		[x, y]: [f32; 2],
		width: f32,
		style: &ListStyle,
		opts: &LayoutOptions,
		out: &mut BlockLayout,
	) -> f32 {
		out.begin_scene(x, y);
		self.shaper.appearance = style.appearance.clone();
		let before = style.rule.space_before.unwrap_or(0.) * opts.font_size;
		let after = style.rule.space_after.unwrap_or(0.) * opts.font_size;
		let box_y = y + before;
		let box_index = out.draws.len();
		out.draws.push(Draw::Rect(Rect::default(), Paint::Text));
		let content_y = box_y + style.padding[0];
		let item_x = x + style.padding[3];
		let item_width = (width - style.padding[1] - style.padding[3]).max(1.);

		// Only ordered numbers are reading text; bullets and checkboxes are geometry.
		let numbered = label.is_some();
		let marker_start = out.draws.len();
		if let Some(label) = label {
			self.list_number(label, item_x, content_y, style, opts, out);
		}
		let first_child = out.text.len();
		if let Some(checked) = item.checked {
			style.task.draw(
				[
					item_x
						+ marker_offset(
							style.task.align,
							style.column,
							style.task.side,
						)
						+ style.task.side * 0.5,
					0.,
				],
				checked,
				out,
			);
		} else if !numbered {
			let left = item_x
				+ marker_offset(
					opts.stylesheet.marker_align(false),
					style.column,
					style.bullet_side,
				);
			out.draws.push(Draw::Polygon {
				center: [left + style.bullet_side / 2., 0.],
				points: style.bullet_points.clone(),
				paint: style.bullet.paint,
			});
		}
		let marker_end = out.draws.len();
		let child_height = self.children(
			&item.blocks,
			item_x + style.column,
			content_y,
			(item_width - style.column).max(1.),
			opts,
			style.size * 0.6,
			out,
		);
		let first_line = out.first_text_rect(first_child).unwrap_or(Rect {
			x: item_x + style.column,
			y: content_y,
			w: 0.,
			h: style.size * style.appearance.line_height,
		});
		self.place_list_marker(
			out,
			marker_start..marker_end,
			numbered,
			first_child,
			first_line,
		);
		// The list owns its closing space; retain child spacing only
		// between items, so nested tails do not accumulate.
		let trailing = if last {
			closing_space(item, &style.appearance, opts)
		} else {
			0.
		};
		let content_height = (child_height - trailing)
			.max(style.size * self.shaper.appearance.line_height);
		if trailing > 0.0 {
			out.height = out.height.min(content_y + content_height);
		}
		let box_height = style.padding[0] + content_height + style.padding[2];
		*out.draws.get_mut(box_index).unwrap() = Draw::Box {
			rect: Rect {
				x,
				y: box_y,
				w: width,
				h: box_height,
			},
			chain: style.appearance.chain,
			condition: Condition::ListItem,
			radius: style.rule.radius.unwrap_or(0.),
			border: style.rule.border_width.unwrap_or(0.),
			left_only: false,
			decoration: BoxDecoration::from_rule(&style.rule, false),
		};
		let item_height = before + box_height + after;
		if let Some(node) = out.text.get_mut(first_child) {
			// A number shares its line with the item text; an item
			// without one starts its own line.
			node.separator = if numbered { "" } else { "\n" };
		}
		if numbered && first_child < out.text.len() {
			out.text[first_child - 1].selection_group = Some(
				out.text[first_child].selection_group.unwrap_or(first_child),
			);
		}
		out.end_scene(
			item_height,
			before
				+ style.padding[0]
				+ style.padding[2]
				+ style.size * self.shaper.appearance.line_height
				+ after,
		);
		item_height
	}

	fn list_number(
		&mut self,
		label: &str,
		item_x: f32,
		top: f32,
		style: &ListStyle,
		opts: &LayoutOptions,
		out: &mut BlockLayout,
	) {
		let (mut draws, ranges, width) = self.shaper.label_runs(
			label,
			opts.font_size,
			item_x,
			0.,
			&style.bullet,
			style.bullet.paint,
			Some(Paint::Scoped(
				style.bullet.chain,
				Condition::Marker,
				ColorField::Background,
			)),
		);
		// Shaping starts at the column's left edge; alignment
		// moves the finished label without reshaping it.
		let dx =
			marker_offset(opts.stylesheet.enum_align(), style.column, width);
		for draw in &mut draws {
			draw.translate(dx, 0.);
		}
		// The number copies as its own word before the item,
		// so the trailing space rides on its last glyph. Each
		// glyph keeps its own range, which is what lets a PDF
		// name every character instead of one span that leaves
		// the later glyphs unnamed.
		let text = format!("{label} ");
		let mut node = TextNode::new(text.clone(), "\n");
		let height = style.size * self.shaper.appearance.line_height;
		let command = out.draws.len();
		let glyphs: Vec<f32> = draws
			.iter()
			.filter_map(|draw| match draw {
				Draw::Glyph(glyph) => Some(glyph.x),
				_ => None,
			})
			.collect();
		let mut ranges = ranges.into_iter();
		let mut glyph = 0;
		for (offset, draw) in draws.iter().enumerate() {
			if !matches!(draw, Draw::Glyph(_)) {
				continue;
			}
			let mut range = ranges.next().unwrap_or(0..0);
			if glyph + 1 == glyphs.len() {
				range.end = text.len();
			}
			let x = glyphs[glyph];
			let end = glyphs
				.get(glyph + 1)
				.copied()
				.unwrap_or(item_x + dx + width);
			node.push(TextCluster {
				mixed_spacing: (0.0, 0.0),
				range,
				rect: Rect {
					x,
					y: top,
					w: (end - x).max(1.0),
					h: height,
				},
				rtl: false,
				atomic: false,
				command: command + offset,
			});
			glyph += 1;
		}
		out.draws.extend(draws);
		out.text.push(node);
	}

	fn place_list_marker(
		&mut self,
		out: &mut BlockLayout,
		commands: Range<usize>,
		numbered: bool,
		first_child: usize,
		first_line: Rect,
	) {
		let center_y = first_line.y + first_line.h * 0.5;
		let marker = &mut out.draws.builder_mut()[commands];
		if numbered {
			self.shaper.center_draws(marker, center_y);
			for cluster in &mut out.text[first_child - 1].clusters {
				cluster.rect.y = first_line.y;
				cluster.rect.h = first_line.h;
			}
		} else {
			for draw in marker {
				if let Draw::Polygon { center, .. } = draw {
					center[1] = center_y;
				}
			}
		}
	}
}
