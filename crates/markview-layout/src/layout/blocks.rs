mod list;

use super::{BlockContext, LayoutOptions};
use crate::{
	document::{
		Block, BlockKind, CalloutKind, CellAlign, Inline, InlineKind, RichText,
		TextStyle,
	},
	scene::{BlockLayout, Draw, HeadingAnchor, LinkId, LinkRect, Paint, Rect},
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

/// The side of a task checkbox, in multiples of the task marker's size.
const TASK_BOX: f32 = 0.72;

/// The outline a task checkbox draws without its own `border_width`, in
/// logical pixels.
const TASK_BORDER: f32 = 1.0;

/// The segments that approximate each corner of a task checkbox.
const TASK_CORNERS: usize = 4;

/// The column a `<details>` summary reserves before its text, in multiples of
/// the summary's own size.
const DETAILS_COLUMN: f32 = 1.3;

/// The side of the disclosure triangle, in multiples of the summary's size.
const DETAILS_MARKER: f32 = 0.42;

/// The space the marker keeps from the element's left edge, in logical pixels.
const DETAILS_INSET: f32 = 2.0;

/// The gap between a summary line and its body, in multiples of the summary's
/// size.
const DETAILS_GAP: f32 = 0.45;

/// A disclosure triangle relative to its center: pointing right while the body
/// is collapsed and down while it is expanded.
fn disclosure_points(expanded: bool, side: f32) -> Arc<[[f32; 2]]> {
	let r = side / 2.;
	Arc::from(if expanded {
		vec![[-r, -r], [r, -r], [0., r]]
	} else {
		vec![[-r, -r], [r, 0.], [-r, r]]
	})
}

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

/// Reserve both marker geometry and the alignment's minimum inset.
fn marker_min_width(align: TextAlign, width: f32, gap: f32) -> f32 {
	width + gap + marker_offset(align, width, width)
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

/// A completed task's check, as one filled polygon relative to the box center.
/// The mark is geometry rather than a glyph, so no font can substitute a
/// different shape, color, or size.
pub(super) fn check_points(side: f32) -> Arc<[[f32; 2]]> {
	// Half the stroke, and the segment ends, in units of the box side.
	let t = side * 0.075;
	let at = |x: f32, y: f32| [x * side, y * side];
	let (start, elbow, end) =
		(at(-0.26, 0.02), at(-0.06, 0.22), at(0.28, -0.24));
	let unit = |v: [f32; 2]| {
		let len = (v[0] * v[0] + v[1] * v[1]).sqrt();
		[v[0] / len, v[1] / len]
	};
	let normal = |v: [f32; 2]| [-v[1], v[0]];
	let first = unit([elbow[0] - start[0], elbow[1] - start[1]]);
	let second = unit([end[0] - elbow[0], end[1] - elbow[1]]);
	let (n1, n2) = (normal(first), normal(second));
	let along =
		|p: [f32; 2], n: [f32; 2], d: f32| [p[0] + n[0] * d, p[1] + n[1] * d];
	// Both joints land on the bisector; a sharp turn would spike, so the
	// miter is bounded.
	let bisector = unit([n1[0] + n2[0], n1[1] + n2[1]]);
	let cos = (n1[0] * bisector[0] + n1[1] * bisector[1]).max(0.5);
	let miter = (t / cos).min(3.0 * t);
	let outer = along(elbow, bisector, miter);
	let inner = along(elbow, bisector, -miter);
	Arc::from([
		along(start, n1, t),
		outer,
		along(end, n2, t),
		along(end, n2, -t),
		inner,
		along(start, n1, -t),
	])
}

/// A rounded square centered on the origin, traced clockwise. `TASK_CORNERS`
/// segments approximate each corner, which is enough at a checkbox's size and
/// keeps every box in a list on one cached raster. Both endpoints of every
/// quarter-circle are present, so each side stays axis-aligned.
pub(super) fn rounded_square(side: f32, radius: f32) -> Vec<[f32; 2]> {
	let half = side / 2.;
	let corner = radius.clamp(0., half);
	if corner < 0.5 {
		return vec![
			[-half, -half],
			[half, -half],
			[half, half],
			[-half, half],
		];
	}
	let center = half - corner;
	let mut points = Vec::with_capacity(4 * (TASK_CORNERS + 1));
	for (cx, cy, start) in [
		(center, -center, -90.0),
		(center, center, 0.0),
		(-center, center, 90.0),
		(-center, -center, 180.0),
	] {
		for i in 0..=TASK_CORNERS {
			let angle =
				(start + 90.0 * i as f32 / TASK_CORNERS as f32).to_radians();
			points.push([cx + corner * angle.cos(), cy + corner * angle.sin()]);
		}
	}
	points
}

/// A checkbox outline: the outer contour with the inner one cut into it and
/// wound the other way, so the box stays hollow when a theme sets no fill.
pub(super) fn rounded_square_ring(
	side: f32,
	radius: f32,
	border: f32,
) -> Vec<[f32; 2]> {
	let inner_side = (side - 2.0 * border).max(0.0);
	let inner_radius = (radius - border).max(0.0);
	let mut points = rounded_square(side, radius);
	// Close the outer loop before cutting, so the cut is one radial slit
	// rather than a detour across an edge.
	points.push(points[0]);
	let inner = rounded_square(inner_side, inner_radius);
	points.push(inner[0]);
	points.extend(inner.iter().rev());
	points
}

/// The stylesheet family that colors a callout type, after Obsidian's
/// palette: blue notes, cyan tips, green successes, orange warnings, red
/// dangers, purple examples and gray quotes.
pub(crate) fn callout_family(kind: CalloutKind) -> Condition {
	use CalloutKind as K;
	match kind {
		K::Note | K::Info | K::Todo => Condition::CalloutNote,
		K::Abstract | K::Tip => Condition::CalloutTip,
		K::Success => Condition::CalloutSuccess,
		K::Question | K::Warning => Condition::CalloutWarning,
		K::Caution | K::Failure | K::Danger | K::Bug => {
			Condition::CalloutDanger
		}
		K::Important | K::Example => Condition::CalloutExample,
		K::Quote => Condition::CalloutQuote,
	}
}

/// The element a block's box belongs to.
fn block_role(block: &Block) -> Condition {
	match &block.kind() {
		BlockKind::Paragraph(_) => Condition::P,
		BlockKind::Heading { level, .. } => Condition::heading(*level),
		BlockKind::Code { .. } => Condition::CodeBlock,
		BlockKind::Quote {
			callout: Some(callout),
			..
		} => callout_family(callout.kind),
		BlockKind::Quote { .. } => Condition::Blockquote,
		BlockKind::List { start, .. } => {
			if start.is_some() {
				Condition::Enum
			} else {
				Condition::List
			}
		}
		BlockKind::Table(_) => Condition::Table,
		BlockKind::Figure { .. } => Condition::Image,
		BlockKind::DefinitionList(_) => Condition::P,
		BlockKind::Footnote { .. } => Condition::Footnote,
		BlockKind::Details { .. } => Condition::Details,
		BlockKind::FrontMatter { .. } => Condition::FrontMatter,
		BlockKind::Rule => Condition::Hr,
	}
}

fn block_rule(
	opts: &LayoutOptions,
	chain: u128,
	role: Condition,
) -> crate::style::Rule {
	let mut rule = opts.stylesheet.element_rule(chain, role);
	// Callouts keep a decorated frame with the theme's quote line width.
	if role.element_path().contains(Condition::Callout) {
		rule.border_edges.get_or_insert([
			0.,
			0.,
			0.,
			rule.border_width.unwrap_or(0.),
		]);
	}
	rule
}

pub(super) fn anchor_only(block: &Block) -> bool {
	matches!(&block.kind(), BlockKind::Paragraph(text) if text.iter().all(|i| matches!(i.kind, InlineKind::Anchor(_))))
}

/// The spacing a block reserves outside its box, given the appearance its
/// parent established.
fn outer_spacing(
	block: &Block,
	parent: &TextAppearance,
	opts: &LayoutOptions,
) -> (f32, f32) {
	if anchor_only(block) {
		return (0., 0.);
	}
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
			InlineKind::Anchor(_) => {}
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
		owner: Option<markview_document::BlockId>,
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
				owner, rich, x, y, width, size, sans, align, justify, indent,
				opts, out,
			)
		})
	}

	#[expect(
		clippy::too_many_arguments,
		reason = "Text style and block geometry are independent layout inputs"
	)]
	fn rich_inner(
		&mut self,
		owner: Option<markview_document::BlockId>,
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
		let field = owner.and_then(|owner| self.fields.next(owner));
		let first_node = out.text.len();
		let mut start = 0;
		let mut height = 0.;
		for (i, inline) in rich.iter().enumerate() {
			if let InlineKind::Math { display: true, .. } = inline.kind {
				if i > start {
					let search_node = out.text.len();
					let h = self.paragraph(
						&rich[start..i],
						x,
						y + height,
						width,
						size,
						sans,
						align,
						justify,
						indent && start == 0,
						opts,
						out,
					);
					bind_rich_source(out, search_node, rich, start, field);
					height += h;
				}
				height += size * 0.5;
				let search_node = out.text.len();
				let h = self.paragraph(
					&rich[i..i + 1],
					x,
					y + height,
					width,
					size,
					false,
					CellAlign::Center,
					false,
					false,
					opts,
					out,
				);
				bind_rich_source(out, search_node, rich, i, field);
				height += h;
				height += size * 0.5;
				start = i + 1;
			}
		}
		if start < rich.len() {
			let search_node = out.text.len();
			let h = self.paragraph(
				&rich[start..],
				x,
				y + height,
				width,
				size,
				sans,
				align,
				justify,
				indent && start == 0,
				opts,
				out,
			);
			bind_rich_source(out, search_node, rich, start, field);
			height += h;
		}
		if let Some(node) = out.text.get_mut(first_node) {
			node.separator = "\n\n";
		}
		for node in &mut out.text[first_node..] {
			node.selection_group = Some(first_node);
		}
		height
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
		let mut height = 0.;
		let parent = self.shaper.appearance.clone();
		let count = blocks.iter().filter(|b| !anchor_only(b)).count();
		let mut index = 0;
		for block in blocks {
			if (self.cancelled)() {
				break;
			}
			if anchor_only(block) {
				self.shaper.appearance = parent.clone();
				height +=
					self.cached_child(block, [x, y + height], width, opts, out);
				continue;
			}
			self.shaper.appearance =
				opts.stylesheet.child(&parent, index, count);
			height +=
				self.cached_child(block, [x, y + height], width, opts, out);
			index += 1;
		}
		self.shaper.appearance = parent;
		height
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
		let (lead, trail) = match (
			blocks.iter().find(|b| !anchor_only(b)),
			blocks.iter().rfind(|b| !anchor_only(b)),
		) {
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
		if (self.cancelled)() {
			return 0.;
		}
		out.begin_scene(x, y);
		let height = self.measured_block(block, x, y, width, opts, out);
		out.end_scene(height, f32::NEG_INFINITY);
		height
	}

	fn measured_block(
		&mut self,
		block: &Block,
		x: f32,
		y: f32,
		width: f32,
		opts: &LayoutOptions,
		out: &mut BlockLayout,
	) -> f32 {
		if append_anchor_only(block, y, out) {
			return 0.;
		}
		// An export draws the document's text, and front matter is the reader's
		// aid, so it takes no page there.
		if opts.hide_front_matter
			&& matches!(block.kind(), BlockKind::FrontMatter { .. })
		{
			return 0.;
		}
		let role = block_role(block);
		let previous = self.shaper.appearance.clone();
		// A callout is a block quote first, so a theme's quote rules still
		// shape it; `callout` and its family refine them in that order.
		let appearance = if role.element_path().contains(Condition::Callout) {
			[Condition::Blockquote, Condition::Callout, role]
				.into_iter()
				.fold(previous.clone(), |a, c| opts.stylesheet.text(&a, c))
		} else {
			opts.stylesheet.text(&previous, role)
		};
		let chain = appearance.chain;
		let rule = block_rule(opts, chain, role);
		// A hidden block draws nothing, box included, so a stylesheet can keep
		// a document's metadata parsed while keeping it off the page.
		if rule.show == Some(false) {
			self.shaper.appearance = previous;
			return 0.;
		}
		let left_only = role.element_path().contains(Condition::Blockquote);
		self.shaper.appearance = appearance;
		self.shaper.appearance.background = None;
		let before = rule.space_before.unwrap_or(0.) * opts.font_size;
		let after = rule.space_after.unwrap_or(0.) * opts.font_size;
		let pad = block_padding(&rule, opts.font_size);
		let marker = rule.heading_marker.filter(|m| m[0] > 0.0 && m[1] > 0.0);
		let marker_width =
			marker.map_or(0.0, |m| (m[0] + m[2]) * opts.font_size);
		let line_height = opts.font_size
			* self.shaper.appearance.size
			* self.shaper.appearance.line_height;
		let marker_extra = marker
			.map_or(0.0, |m| (m[1] * opts.font_size - line_height).max(0.0));
		let inner_y = y + before + pad[0] + marker_extra * 0.5;
		let decoration =
			crate::scene::BoxDecoration::from_rule(&rule, left_only);
		let placeholder = out.draws.len();
		out.draws.push(Draw::Box {
			rect: Rect::default(),
			chain,
			condition: role,
			radius: rule.radius.unwrap_or(0.),
			border: rule.border_width.unwrap_or(0.),
			left_only,
			decoration: decoration.clone(),
		});
		for anchor in block.anchors() {
			out.anchors.push(HeadingAnchor {
				anchor: crate::document::AnchorId::Fragment(anchor.clone()),
				y: y + before,
			});
		}
		if let BlockKind::Heading { anchor, .. } = &block.kind() {
			// A link to this heading lands on the top of its box.
			out.anchors.push(HeadingAnchor {
				anchor: crate::document::AnchorId::Fragment(anchor.clone()),
				y: y + before,
			});
		}
		if let BlockKind::Footnote { id, .. } = &block.kind() {
			// A footnote reference lands on the top of the note's box.
			out.anchors.push(HeadingAnchor {
				anchor: crate::document::AnchorId::FootnoteDefinition(
					id.clone(),
				),
				y: y + before,
			});
		}
		let height = self.block_inner(
			block,
			x + pad[3] + marker_width,
			inner_y,
			(width - pad[1] - pad[3] - marker_width).max(1.),
			opts,
			out,
		);
		let box_height = pad[0] + height + pad[2] + marker_extra;
		if rule.orphans.is_some()
			|| rule.widows.is_some()
			|| rule.keep_together == Some(true)
		{
			out.page_constraints.push(crate::scene::PageConstraint {
				top: y + before,
				bottom: y + before + box_height,
				orphans: rule.orphans,
				widows: rule.widows,
				keep_together: rule.keep_together.unwrap_or(false),
			});
		}
		if let Some([w, h, _]) = marker {
			out.draws.push(Draw::Rect(
				Rect {
					x: x + pad[3],
					y: inner_y + (line_height - h * opts.font_size) * 0.5,
					w: w * opts.font_size,
					h: h * opts.font_size,
				},
				Paint::Scoped(
					chain,
					role,
					crate::style::ColorField::MarkerColor,
				),
			));
		}
		*out.draws.get_mut(placeholder).unwrap() = Draw::Box {
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
			left_only,
			decoration,
		};
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
		let height = match &block.kind() {
			BlockKind::Paragraph(text) => self.rich(
				Some(block.id()),
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
				Some(block.id()),
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
				let node = out.text.len();
				let h = self.code(language, text, x, y, width, size, opts, out);
				out.text[node].search_field = self.fields.next(block.id());
				out.text[node].search_ranges =
					vec![(0..text.len(), 0..text.len())].into();
				h
			}
			// The body is `yaml` source, so the disclosure below supplies the
			// appearance and the highlighter takes the block from there.
			BlockKind::FrontMatter { blocks, .. } => {
				let label = RichText::from([Inline {
					link: None,
					kind: InlineKind::Text(opts.front_matter_label.clone()),
					style: TextStyle::default(),
				}]);
				self.disclosure(
					block, None, &label, blocks, None, x, y, width, opts, out,
				)
			}
			BlockKind::Quote {
				callout: None,
				blocks,
			} => {
				return self.framed_children(blocks, x, y, width, opts, out);
			}
			BlockKind::Quote {
				callout: Some(callout),
				blocks,
			} if callout.fold.is_some() => {
				return self.folded_callout(
					callout.kind,
					blocks,
					[x, y],
					width,
					opts,
					out,
				);
			}
			BlockKind::Quote {
				callout: Some(callout),
				blocks,
			} => self.callout(
				block.id(),
				callout,
				blocks,
				[x, y],
				width,
				opts,
				out,
			),
			BlockKind::List {
				start,
				reversed,
				items,
				..
			} => self.list(*start, *reversed, items, [x, y], width, opts, out),
			BlockKind::Table(table) => {
				self.table(block.id(), table, [x, y], width, opts, out)
			}
			BlockKind::DefinitionList(items) => self.definition_list(
				block.id(),
				items,
				[x, y],
				width,
				opts,
				out,
			),
			BlockKind::Figure { blocks, caption } => self.figure(
				block.id(),
				blocks,
				caption.as_ref(),
				[x, y],
				width,
				opts,
				out,
			),
			BlockKind::Footnote { id, number, blocks } => {
				self.footnote(id, *number, blocks, [x, y], width, opts, out)
			}
			BlockKind::Details {
				summary, blocks, ..
			} => self.disclosure(
				block,
				Some(block.id()),
				summary,
				blocks,
				None,
				x,
				y,
				width,
				opts,
				out,
			),
		};
		out.height = out.height.max(y + height);
		out.width = out.width.max(x + width);
		height
	}

	#[expect(
		clippy::too_many_arguments,
		reason = "Callout content and geometry"
	)]
	fn callout(
		&mut self,
		owner: crate::document::BlockId,
		callout: &crate::document::Callout,
		blocks: &[Block],
		[x, y]: [f32; 2],
		width: f32,
		opts: &LayoutOptions,
		out: &mut BlockLayout,
	) -> f32 {
		let size = opts.font_size * self.shaper.appearance.size;
		let header = self.callout_header(
			Some(owner),
			callout.kind,
			callout.title.as_ref().expect("a plain callout has a title"),
			x,
			y,
			width,
			opts,
			out,
		);
		let gap = if blocks.is_empty() {
			0.0
		} else {
			size * DETAILS_GAP
		};
		header
			+ gap + self.framed_children(
			blocks,
			x,
			y + header + gap,
			width,
			opts,
			out,
		)
	}

	fn folded_callout(
		&mut self,
		kind: CalloutKind,
		blocks: &[Block],
		[x, y]: [f32; 2],
		width: f32,
		opts: &LayoutOptions,
		out: &mut BlockLayout,
	) -> f32 {
		let disclosure = &blocks[0];
		let BlockKind::Details {
			summary, blocks, ..
		} = disclosure.kind()
		else {
			unreachable!("foldable callouts contain a disclosure");
		};
		self.disclosure(
			disclosure,
			Some(disclosure.id()),
			summary,
			blocks,
			Some(kind),
			x,
			y,
			width,
			opts,
			out,
		)
	}

	fn definition_list(
		&mut self,
		owner: crate::document::BlockId,
		items: &[crate::document::DefinitionItem],
		[x, y]: [f32; 2],
		width: f32,
		opts: &LayoutOptions,
		out: &mut BlockLayout,
	) -> f32 {
		let size = opts.font_size * self.shaper.appearance.size;
		let indent = (opts.font_size * 1.5).min(width * 0.5);
		let mut height = 0.;
		for item in items {
			for terms in &item.terms {
				height += self.rich(
					Some(owner),
					terms,
					x,
					y + height,
					width,
					size,
					false,
					CellAlign::Left,
					false,
					false,
					opts,
					out,
				);
			}
			for blocks in &item.definitions {
				height += self.children(
					blocks,
					x + indent,
					y + height,
					(width - indent).max(1.0),
					opts,
					0.,
					out,
				);
			}
		}
		height
	}

	#[expect(
		clippy::too_many_arguments,
		reason = "Figure content and geometry"
	)]
	fn figure(
		&mut self,
		owner: crate::document::BlockId,
		blocks: &[Block],
		caption: Option<&RichText>,
		[x, y]: [f32; 2],
		width: f32,
		opts: &LayoutOptions,
		out: &mut BlockLayout,
	) -> f32 {
		let parent = self.shaper.appearance.clone();
		let mut height = 0.;
		for block in blocks {
			height +=
				self.cached_child(block, [x, y + height], width, opts, out);
		}
		if let Some(caption) = caption {
			self.shaper.appearance =
				opts.stylesheet.text(&parent, Condition::Caption);
			let caption_size = opts.font_size * self.shaper.appearance.size;
			let gap = caption_size * 0.5;
			height += gap;
			height += self.rich(
				Some(owner),
				caption,
				x,
				y + height,
				width,
				caption_size,
				false,
				CellAlign::Center,
				false,
				false,
				opts,
				out,
			);
			self.shaper.appearance = parent;
		}
		height
	}

	#[expect(
		clippy::too_many_arguments,
		reason = "Footnote identity, content and geometry"
	)]
	fn footnote(
		&mut self,
		note_id: &crate::document::FootnoteId,
		number: u32,
		blocks: &[Block],
		[x, y]: [f32; 2],
		width: f32,
		opts: &LayoutOptions,
		out: &mut BlockLayout,
	) -> f32 {
		let size = opts.font_size * self.shaper.appearance.size;
		let text = format!("[{number}]");
		let paint = Paint::Styled(Condition::FootnoteRef, ColorField::Color);
		// Every note reserves the same marker column, so their bodies
		// start at one x even when the numbers differ in width. The
		// number itself is shaped at the origin and moved onto the
		// body's first baseline below.
		let (mut draws, label_width) =
			self.shaper
				.label_measured(&text, opts.font_size, x, 0.0, paint);
		let reserved =
			format!("[{}]", "0".repeat(self.footnote_column as usize));
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
		// Visible flow resolves the baseline after disclosure placement.
		let baseline = y + size * 1.15;
		for draw in &mut draws {
			draw.translate(0.0, baseline);
		}
		let command = out.draws.len();
		out.draws.extend(draws);
		let commands = command..out.draws.len();
		out.retain_footnote(body_start..command, commands.clone(), baseline);
		// The number is the way back to the reference that opened the
		// note, so it is a link with the note's own identity.
		let id = LinkId::Special(out.links.len());
		let target = crate::scene::LinkTarget::FootnoteReturn(note_id.clone());
		out.links.push(LinkRect {
			commands,
			id,
			rect: Rect {
				x,
				y: baseline - size,
				w: label_width.max(1.0),
				h: size * 1.4,
			},
			url: target.display_url(),
			target,
		});
		body
	}

	#[expect(
		clippy::too_many_arguments,
		reason = "Header geometry and layout inputs"
	)]
	fn callout_header(
		&mut self,
		owner: Option<markview_document::BlockId>,
		kind: CalloutKind,
		title: &RichText,
		x: f32,
		y: f32,
		width: f32,
		opts: &LayoutOptions,
		out: &mut BlockLayout,
	) -> f32 {
		use super::callout_icons as icons;
		use CalloutKind as K;
		let paths = match kind {
			K::Note => icons::NOTE,
			K::Abstract => icons::ABSTRACT,
			K::Info => icons::INFO,
			K::Todo => icons::TODO,
			K::Tip => icons::TIP,
			K::Important => icons::IMPORTANT,
			K::Success => icons::SUCCESS,
			K::Question => icons::QUESTION,
			K::Warning => icons::WARNING,
			K::Caution => icons::CAUTION,
			K::Failure => icons::FAILURE,
			K::Danger => icons::DANGER,
			K::Bug => icons::BUG,
			K::Example => icons::EXAMPLE,
			K::Quote => icons::QUOTE,
		};
		let parent = self.shaper.appearance.clone();
		self.shaper.appearance =
			opts.stylesheet.text(&parent, Condition::Label);
		let size = opts.font_size * self.shaper.appearance.size;
		let line = size * self.shaper.appearance.line_height;
		out.draws.push(Draw::Icon {
			paths,
			paint: self.shaper.appearance.paint,
			x,
			y: y + (line - size) * 0.5,
			size,
		});
		let column = size * 1.45;
		let height = self
			.rich(
				owner,
				title,
				x + column,
				y,
				(width - column).max(1.0),
				size,
				false,
				CellAlign::Left,
				false,
				false,
				opts,
				out,
			)
			.max(line);
		self.shaper.appearance = parent;
		height
	}

	/// A summary line that toggles the blocks under it: front matter and a raw
	/// `<details>` element are the same disclosure with different content.
	#[expect(
		clippy::too_many_arguments,
		reason = "Disclosure geometry and layout inputs"
	)]
	fn disclosure(
		&mut self,
		block: &Block,
		summary_owner: Option<markview_document::BlockId>,
		summary: &RichText,
		blocks: &[Block],
		icon: Option<CalloutKind>,
		x: f32,
		y: f32,
		width: f32,
		opts: &LayoutOptions,
		out: &mut BlockLayout,
	) -> f32 {
		let open = match &block.kind() {
			BlockKind::Details { default_open, .. }
			| BlockKind::FrontMatter { default_open, .. } => *default_open,
			_ => false,
		};
		let parent = self.shaper.appearance.clone();
		self.shaper.appearance = opts.stylesheet.text(
			&parent,
			if icon.is_some() {
				Condition::Label
			} else {
				Condition::Summary
			},
		);
		let size = opts.font_size * self.shaper.appearance.size;
		let line = size * self.shaper.appearance.line_height;
		let paint = self.shaper.appearance.paint;
		let side = size * DETAILS_MARKER;
		let column = size * DETAILS_COLUMN;
		// The marker is geometry rather than a glyph, so no font can
		// change its shape.
		let marker = out.draws.len();
		out.draws.push(Draw::Polygon {
			center: [x + DETAILS_INSET + side * 0.5, y + line * 0.5],
			points: disclosure_points(true, side),
			paint,
		});
		let links_start = out.links.len();
		let mut height = if let Some(kind) = icon {
			self.shaper.appearance = parent.clone();
			self.callout_header(
				summary_owner,
				kind,
				summary,
				x + column,
				y,
				(width - column).max(1.0),
				opts,
				out,
			)
		} else {
			self.rich(
				summary_owner,
				summary,
				x + column,
				y,
				(width - column).max(1.0),
				size,
				false,
				CellAlign::Left,
				false,
				false,
				opts,
				out,
			)
			.max(line)
		};
		let summary_height = height;
		// The body is the element's ordinary content, so it must not
		// inherit the summary's weight, color or condition chain.
		self.shaper.appearance = parent;
		// The whole summary line toggles the element. It is hit like a
		// link, so a non-drag release and the hover state are enough.
		// Its range stops at the first link inside the summary or at the
		// body, so neither borrows the summary's hover.
		let summary_end = out
			.links
			.iter()
			.skip(links_start)
			.map(|link| link.commands.start)
			.min()
			.unwrap_or(out.draws.len());
		let id = LinkId::Special(out.links.len());
		let target = crate::scene::LinkTarget::Disclosure(block.id());
		out.links.push(LinkRect {
			commands: marker..summary_end,
			id,
			rect: Rect {
				x,
				y,
				w: width,
				h: summary_height,
			},
			url: target.display_url(),
			target,
		});
		let body = size * DETAILS_GAP
			+ self.framed_children(
				blocks,
				x,
				y + height + size * DETAILS_GAP,
				width,
				opts,
				out,
			);
		out.retain_disclosure(block.id(), open, marker, body);
		height += body;
		height
	}
}

/// The field ids one top-level block's traversal produced, per owning block.
///
/// Layout asks each owning block for its fields in traversal order, so a
/// container that paints its own field before or after its children still
/// binds the id the semantic walk assigned.
#[derive(Default)]
pub(super) struct FieldCursor {
	fields: std::collections::HashMap<
		markview_document::BlockId,
		(Vec<markview_document::FieldId>, usize),
	>,
}

impl FieldCursor {
	pub(super) fn new(block: &Block) -> Self {
		let mut fields = Self::default();
		markview_document::visit_fields(
			block,
			&mut Vec::new(),
			&|| false,
			&mut |owner, id, _, _| {
				fields.fields.entry(owner).or_default().0.push(id);
			},
		);
		fields
	}

	/// The next field id for `owner`, or `None` once it has consumed them all.
	pub(super) fn next(
		&mut self,
		owner: markview_document::BlockId,
	) -> Option<markview_document::FieldId> {
		let consumed = self.fields.get_mut(&owner);
		debug_assert!(
			consumed.is_some(),
			"block {owner} consumes a field its traversal never produced"
		);
		let (fields, next) = consumed?;
		let id = fields.get(*next).copied();
		debug_assert!(
			id.is_some(),
			"block {owner} consumes more fields than its traversal produced"
		);
		*next += usize::from(id.is_some());
		id
	}
}

fn bind_rich_source(
	out: &mut BlockLayout,
	node: usize,
	rich: &RichText,
	start: usize,
	field: Option<markview_document::FieldId>,
) {
	if let Some(node) = out.text.get_mut(node) {
		node.search_field = field;
		for (index, _) in &mut node.source_images {
			*index += start;
		}
		let offset = crate::document::plain_text(&rich[..start]).len();
		for (semantic, _) in &mut node.search_ranges {
			semantic.start += offset;
			semantic.end += offset;
		}
	}
}

fn append_anchor_only(block: &Block, y: f32, out: &mut BlockLayout) -> bool {
	if let BlockKind::Paragraph(text) = &block.kind()
		&& anchor_only(block)
	{
		for anchor in
			block.anchors().iter().chain(text.iter().filter_map(|i| {
				if let InlineKind::Anchor(a) = &i.kind {
					Some(a)
				} else {
					None
				}
			})) {
			out.anchors.push(HeadingAnchor {
				anchor: crate::document::AnchorId::Fragment(anchor.clone()),
				y,
			});
		}
		return true;
	}
	false
}

pub(super) fn block_padding(
	rule: &crate::style::Rule,
	font_size: f32,
) -> [f32; 4] {
	let mut pad = rule
		.padding
		.as_ref()
		.map(|p| p.sides().map(|v| v * font_size))
		.unwrap_or([0.; 4]);
	if let Some(edges) = rule.border_edges {
		for (pad, edge) in pad.iter_mut().zip(edges) {
			*pad += edge;
		}
	}
	pad
}
