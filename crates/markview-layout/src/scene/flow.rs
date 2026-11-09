//! Retained local geometry and its inexpensive presentation index.
#[cfg(test)]
mod proptests;

use super::{BlockLayout, Draw, PlacedBlock, Rect};
use markview_document::FieldId;
use std::{
	collections::{BTreeMap, HashMap},
	ops::Range,
	sync::Arc,
};

#[derive(Clone, Debug, Default)]
pub struct Scene(Arc<SceneData>);
#[doc(hidden)]
#[derive(Clone, Debug, Default)]
pub struct SceneData {
	nodes: Vec<Node>,
	spans: Vec<Span>,
	footnotes: Vec<Footnote>,
	active: Option<usize>,
	cursor: Cursor,
	leaves: Vec<Leaf>,
	pub(crate) reused_leaves: usize,
}

impl std::ops::Deref for Scene {
	type Target = SceneData;
	fn deref(&self) -> &SceneData {
		&self.0
	}
}
impl std::ops::DerefMut for Scene {
	fn deref_mut(&mut self) -> &mut SceneData {
		Arc::make_mut(&mut self.0)
	}
}

#[derive(Clone, Debug)]
struct Leaf {
	key: u64,
	span: usize,
	height: f32,
	degraded: usize,
	math_errors: usize,
	field: Option<markview_document::FieldId>,
	separator: Option<&'static str>,
}

/// A leaf owns only its geometry, never the enclosing container or scene tree.
#[derive(Clone, Debug)]
pub(crate) struct LeafGeometry {
	leaf: Leaf,
	origin: Cursor,
	draws: super::DrawBuffer,
	text: Box<[crate::text::TextNode]>,
	links: Box<[super::LinkRect]>,
	occurrences: Box<[markview_document::LinkOccurrenceId]>,
	overflow: Box<[super::Overflow]>,
	anchors: Box<[super::HeadingAnchor]>,
	constraints: Box<[super::PageConstraint]>,
	decorations: Box<[(usize, Range<f32>)]>,
}

#[derive(Clone, Debug)]
struct Node {
	parent: Option<usize>,
	children: Vec<usize>,
	/// Local x and the gap after the preceding sibling.
	offset: [f32; 2],
	height: f32,
	minimum: f32,
	boxes: Vec<usize>,
	disclosure: Option<Disclosure>,
	/// Placed at the parent's origin rather than stacked after its siblings.
	parallel: bool,
}

#[derive(Clone, Debug)]
struct Footnote {
	node: usize,
	body: Range<usize>,
	marker: Range<usize>,
	baseline: f32,
}

#[derive(Clone, Debug)]
struct Disclosure {
	id: crate::document::BlockId,
	declared: bool,
	marker: usize,
	body: f32,
	closed: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct Cursor {
	draws: usize,
	text: usize,
	links: usize,
	occurrences: usize,
	overflow: usize,
	anchors: usize,
	constraints: usize,
	decorations: usize,
}

#[derive(Clone, Debug)]
struct Span {
	local: bool,
	node: usize,
	draws: Range<usize>,
	text: Range<usize>,
	links: Range<usize>,
	occurrences: Range<usize>,
	overflow: Range<usize>,
	anchors: Range<usize>,
	constraints: Range<usize>,
	decorations: Range<usize>,
}

#[derive(Clone, Copy, Debug)]
struct Placement {
	offset: [f32; 2],
	height: f32,
	expanded: bool,
}

/// Only positions and container decorations change when a disclosure toggles.
#[derive(Debug, Default)]
pub struct FlowIndex {
	force_open: bool,
	nodes: Vec<Option<Placement>>,
	draws: BTreeMap<usize, Draw>,
	marker_offsets: BTreeMap<usize, f32>,
	spans: Vec<(Range<usize>, [f32; 2])>,
	pub height: f32,
}

impl Scene {
	pub(super) fn heap_bytes(&self) -> usize {
		use super::memory::vec_bytes;
		size_of::<SceneData>()
			+ 2 * size_of::<usize>()
			+ vec_bytes(&self.nodes)
			+ vec_bytes(&self.spans)
			+ vec_bytes(&self.footnotes)
			+ vec_bytes(&self.leaves)
			+ self
				.nodes
				.iter()
				.map(|n| vec_bytes(&n.children) + vec_bytes(&n.boxes))
				.sum::<usize>()
	}
}

impl FlowIndex {
	pub(super) fn heap_bytes(&self) -> usize {
		use super::memory::vec_bytes;
		vec_bytes(&self.nodes)
			+ vec_bytes(&self.spans)
			+ self.draws.len() * (std::mem::size_of::<Draw>() + 128) * 2
			+ self.draws.values().map(Draw::heap_bytes).sum::<usize>()
			+ self.marker_offsets.len() * 128
	}
}

impl BlockLayout {
	fn cursor(&self) -> Cursor {
		Cursor {
			draws: self.draws.len(),
			text: self.text.len(),
			links: self.links.len(),
			occurrences: self.link_occurrences.len(),
			overflow: self.overflow.len(),
			anchors: self.anchors.len(),
			constraints: self.page_constraints.len(),
			decorations: self.inline_decorations.len(),
		}
	}

	fn flush_scene(&mut self) {
		let end = self.cursor();
		if let Some(node) = self.scene.active {
			let start = self.scene.cursor;
			self.scene.spans.push(Span {
				local: false,
				node,
				draws: start.draws..end.draws,
				text: start.text..end.text,
				links: start.links..end.links,
				occurrences: start.occurrences..end.occurrences,
				overflow: start.overflow..end.overflow,
				anchors: start.anchors..end.anchors,
				constraints: start.constraints..end.constraints,
				decorations: start.decorations..end.decorations,
			});
		}
		self.scene.cursor = end;
	}

	/// Replace nested scroll regions with their enclosing table's region.
	pub(crate) fn truncate_overflow(&mut self, start: usize) {
		self.flush_scene();
		// A leaf with discarded scroll geometry must be measured again on reuse.
		let scene = &mut *self.scene;
		scene
			.leaves
			.retain(|leaf| scene.spans[leaf.span].overflow.end <= start);
		for span in &mut self.scene.spans {
			span.overflow.start = span.overflow.start.min(start);
			span.overflow.end = span.overflow.end.min(start);
		}
		self.overflow.truncate(start);
		self.scene.cursor.overflow = start;
	}

	pub(crate) fn begin_scene(&mut self, x: f32, y: f32) {
		self.flush_scene();
		let parent = self.scene.active;
		let index = self.scene.nodes.len();
		self.scene.nodes.push(Node {
			parent,
			children: Vec::new(),
			offset: [x, y],
			height: 0.,
			minimum: f32::NEG_INFINITY,
			boxes: Vec::new(),
			disclosure: None,
			parallel: false,
		});
		if let Some(parent) = parent {
			self.scene.nodes[parent].children.push(index);
		}
		self.scene.active = Some(index);
	}

	/// Opens a scene region placed at the parent's origin, so sibling regions
	/// such as table cells do not stack vertically.
	pub(crate) fn begin_parallel_scene(&mut self) {
		self.begin_scene(0., 0.);
		let active = self.scene.active.expect("a scene was just opened");
		self.scene.nodes[active].parallel = true;
	}

	pub(crate) fn end_scene(&mut self, height: f32, minimum: f32) {
		self.flush_scene();
		let active = self.scene.active.unwrap();
		let node = &mut self.scene.nodes[active];
		node.height = height;
		node.minimum = minimum;
		if let Some(disclosure) = &mut node.disclosure {
			disclosure.closed = height - disclosure.body;
		}
		self.scene.active = node.parent;
	}

	/// Closes a region whose content was built at its own origin, so sealing
	/// must not subtract the placement again.
	pub(crate) fn end_local_scene(&mut self, height: f32) {
		self.end_scene(height, f32::NEG_INFINITY);
		self.scene
			.spans
			.last_mut()
			.expect("a scene was just closed")
			.local = true;
	}

	pub(crate) fn retain_disclosure(
		&mut self,
		id: crate::document::BlockId,
		declared: bool,
		marker: usize,
		body: f32,
	) {
		let active = self.scene.active.unwrap();
		self.scene.nodes[active].disclosure = Some(Disclosure {
			id,
			declared,
			marker,
			body,
			closed: 0.,
		});
	}

	/// The first reading box in builder coordinates, including reused leaves.
	pub(crate) fn first_text_rect(&self, start: usize) -> Option<Rect> {
		for (text, node) in self.text.iter().enumerate().skip(start) {
			let mut rect: Option<Rect> = None;
			for cluster in node.clusters.iter().chain(
				node.source_images
					.iter()
					.map(|(_, cluster)| cluster.clone()),
			) {
				let candidate = cluster.rect;
				// An image with alt text has a reading cluster whose box is the
				// image, not its line. The line's own text cluster shares the
				// line top and is taller, so prefer it there.
				if rect.is_none_or(|rect| {
					candidate.y < rect.y - 0.01
						|| ((candidate.y - rect.y).abs() <= 0.01
							&& candidate.h > rect.h)
				}) {
					rect = Some(candidate);
				}
			}
			let Some(mut rect) = rect else {
				continue;
			};
			let index = self
				.scene
				.spans
				.partition_point(|span| span.text.end <= text);
			if let Some(span) = self.scene.spans.get(index)
				&& span.local
				&& span.text.contains(&text)
			{
				let [x, y] = self.scene.nodes[span.node].offset;
				rect.x += x;
				rect.y += y;
			}
			return Some(rect);
		}
		None
	}

	pub(crate) fn retain_footnote(
		&mut self,
		body: Range<usize>,
		marker: Range<usize>,
		baseline: f32,
	) {
		let node = self.scene.active.unwrap();
		let baseline = baseline - self.scene.nodes[node].offset[1];
		self.scene.footnotes.push(Footnote {
			node,
			body,
			marker,
			baseline,
		});
	}

	fn seal_buffers(&mut self) {
		macro_rules! seal {($($field:ident),*)=>{$(if self.$field.capacity()!=self.$field.len() {self.$field.shrink_to_fit();})*};}
		seal!(
			text,
			page_constraints,
			inline_decorations,
			overflow,
			link_occurrences,
			anchors
		);
	}

	/// Convert the builder's temporary coordinates into node-local geometry once.
	pub(crate) fn seal_scene(&mut self) {
		fn localize_text(text: &mut crate::text::TextNode, x: f32, y: f32) {
			for cluster in text.clusters.iter_mut().chain(
				text.source_images.iter_mut().map(|(_, cluster)| cluster),
			) {
				cluster.rect.x -= x;
				cluster.rect.y -= y;
			}
		}
		self.seal_buffers();
		if self.scene.nodes.is_empty() {
			for text in &mut self.text {
				text.seal();
			}
			self.links.seal();
			self.draws.seal();
			return;
		}
		self.scene.nodes[0].height = self.height;
		let scene = &mut *self.scene;
		for span in &scene.spans {
			let [x, y] = scene.nodes[span.node].offset;
			for command in span.draws.clone() {
				if matches!(self.draws.other(command), Some(Draw::Box { .. })) {
					scene.nodes[span.node].boxes.push(command);
				}
				if !span.local {
					self.draws.get_mut(command).unwrap().translate(-x, -y);
				}
			}
			if span.local {
				continue;
			}
			for text in &mut self.text[span.text.clone()] {
				localize_text(text, x, y);
			}
			for link in self.links.range_mut(span.links.clone()) {
				link.rect.x -= x;
				link.rect.y -= y;
			}
			for overflow in &mut self.overflow[span.overflow.clone()] {
				overflow.rect.x -= x;
				overflow.rect.y -= y;
			}
			for anchor in &mut self.anchors[span.anchors.clone()] {
				anchor.y -= y;
			}
			for constraint in
				&mut self.page_constraints[span.constraints.clone()]
			{
				constraint.top -= y;
				constraint.bottom -= y;
			}
			for (_, rows) in
				&mut self.inline_decorations[span.decorations.clone()]
			{
				rows.start -= y;
				rows.end -= y;
			}
		}
		self.localize_child_offsets();
		self.scene.seal_buffers();
		for text in &mut self.text {
			text.seal();
		}
		self.links.seal();
		self.draws
			.seal_spans(self.scene.spans.iter().map(|s| s.draws.clone()));
	}

	fn localize_child_offsets(&mut self) {
		// Children become relative to their parent and the preceding sibling's end.
		for parent in (0..self.scene.nodes.len()).rev() {
			let [x, mut y] = self.scene.nodes[parent].offset;
			let base = y;
			for child in self.scene.nodes[parent].children.clone() {
				let node = &mut self.scene.nodes[child];
				let next = node.offset[1] + node.height;
				node.offset[0] -= x;
				node.offset[1] -= if node.parallel { base } else { y };
				if !node.parallel {
					y = next;
				}
			}
		}
	}

	pub(crate) fn cached_leaves(
		&self,
	) -> impl Iterator<Item = (u64, Arc<LeafGeometry>)> + '_ {
		self.scene.leaves.iter().map(|leaf| {
			let span = &self.scene.spans[leaf.span];
			(
				leaf.key,
				Arc::new(LeafGeometry {
					leaf: leaf.clone(),
					origin: Cursor {
						draws: span.draws.start,
						text: span.text.start,
						..Default::default()
					},
					draws: self.draws.independent_range(span.draws.clone()),
					text: self.text[span.text.clone()].into(),
					links: self
						.links
						.iter()
						.skip(span.links.start)
						.take(span.links.len())
						.map(super::LinkFragment::owned)
						.collect(),
					occurrences: self.link_occurrences
						[span.occurrences.clone()]
					.into(),
					overflow: self.overflow[span.overflow.clone()].into(),
					anchors: self.anchors[span.anchors.clone()].into(),
					constraints: self.page_constraints
						[span.constraints.clone()]
					.into(),
					decorations: self.inline_decorations
						[span.decorations.clone()]
					.into(),
				}),
			)
		})
	}

	pub(crate) fn retain_leaf(
		&mut self,
		key: u64,
		height: f32,
		degraded: usize,
		math_errors: usize,
		field: Option<markview_document::FieldId>,
	) {
		let scene = &mut *self.scene;
		scene.leaves.push(Leaf {
			key,
			span: scene.spans.len() - 1,
			height,
			degraded,
			math_errors,
			field,
			separator: self.text[scene.spans.last().unwrap().text.clone()]
				.first()
				.map(|text| text.separator),
		});
	}

	/// Share a sealed leaf span and rebind indices while retaining local
	/// coordinates. `origins` are the link occurrences the leaf carries now,
	/// so a reused fragment never keeps an identity that has since moved.
	pub(crate) fn reuse_leaf(
		&mut self,
		previous: &LeafGeometry,
		position: [f32; 2],
		width: f32,
		field: Option<markview_document::FieldId>,
		origins: &[markview_document::LinkOccurrenceId],
	) -> f32 {
		let leaf = &previous.leaf;
		let origin = previous.origin;
		let [x, y] = position;
		self.begin_scene(x, y);
		let start = self.cursor();
		self.draws.append_shared(&previous.draws);
		self.text
			.extend(previous.text.iter().cloned().enumerate().map(
				|(index, mut text)| {
					if index == 0
						&& let Some(separator) = leaf.separator
					{
						text.separator = separator;
					}
					text.clusters.relocate(origin.draws, start.draws);
					for cluster in text.source_images.iter_mut().map(|(_, c)| c)
					{
						cluster.command =
							start.draws + cluster.command - origin.draws;
					}
					text.selection_group = text
						.selection_group
						.map(|group| start.text + group - origin.text);
					if let (Some(old), Some(new), Some(current)) =
						(leaf.field, field, text.search_field)
					{
						text.search_field = Some(FieldId::from_index(
							new.get() + current.get() - old.get(),
						));
					}
					text
				},
			));
		let links = &previous.links;
		let occurrences = &previous.occurrences;
		let mut remap = HashMap::new();
		if occurrences.len() == origins.len() {
			for (&old, &new) in occurrences.iter().zip(origins) {
				remap.insert(old, new);
			}
		}
		self.links.extend(links.iter().cloned().map(|mut link| {
			if let super::LinkId::Inline(origin) = link.id
				&& let Some(&origin) = remap.get(&origin)
			{
				link.id = super::LinkId::Inline(origin);
			}
			link.commands = (start.draws + link.commands.start - origin.draws)
				..(start.draws + link.commands.end - origin.draws);
			link
		}));
		for &origin in origins {
			if self.link_occurrences.last() != Some(&origin) {
				self.link_occurrences.push(origin);
			}
		}
		self.overflow.extend(previous.overflow.iter().cloned().map(
			|mut overflow| {
				overflow.commands = (start.draws + overflow.commands.start
					- origin.draws)
					..(start.draws + overflow.commands.end - origin.draws);
				overflow
			},
		));
		self.anchors.extend(previous.anchors.iter().cloned().map(
			|mut anchor| {
				if let markview_document::AnchorId::FootnoteReference {
					occurrence,
					..
				} = &mut anchor.anchor
					&& let Some(&origin) = remap.get(occurrence)
				{
					*occurrence = origin;
				}
				anchor
			},
		));
		self.page_constraints
			.extend_from_slice(&previous.constraints);
		self.inline_decorations
			.extend(previous.decorations.iter().map(|(command, rows)| {
				(start.draws + command - origin.draws, rows.clone())
			}));
		self.height = self.height.max(y + leaf.height);
		self.width = self.width.max(x + width);
		self.degraded += leaf.degraded;
		self.math_errors += leaf.math_errors;
		self.end_scene(leaf.height, f32::NEG_INFINITY);
		self.scene.spans.last_mut().unwrap().local = true;
		self.retain_leaf(
			leaf.key,
			leaf.height,
			leaf.degraded,
			leaf.math_errors,
			field,
		);
		self.scene.reused_leaves += 1;
		leaf.height
	}

	pub fn resolve_flow(
		&self,
		open: &BTreeMap<crate::document::BlockId, bool>,
		force_open: bool,
	) -> FlowIndex {
		let mut index = FlowIndex {
			force_open,
			nodes: vec![None; self.scene.nodes.len()],
			..Default::default()
		};
		if self.scene.nodes.is_empty() {
			index.height = self.height;
			return index;
		}
		index.height =
			self.scene.place(0, [0., 0.], open, force_open, &mut index);
		for span in &self.scene.spans {
			let Some(placement) = index.nodes[span.node] else {
				continue;
			};
			if !span.draws.is_empty() {
				index.spans.push((span.draws.clone(), placement.offset));
			}
		}
		for footnote in &self.scene.footnotes {
			let Some(placement) = index.nodes[footnote.node] else {
				continue;
			};
			let baseline = index
				.spans
				.iter()
				.find_map(|(commands, offset)| {
					let start = commands.start.max(footnote.body.start);
					let end = commands.end.min(footnote.body.end);
					(start..end).find_map(|command| {
						match &self.draws.at(command) {
							Draw::Glyph(glyph) => {
								Some(glyph.y + offset[1] - placement.offset[1])
							}
							_ => None,
						}
					})
				})
				.unwrap_or(footnote.baseline);
			for command in footnote.marker.clone() {
				index
					.marker_offsets
					.insert(command, baseline - footnote.baseline);
			}
		}
		for (node, placement) in self.scene.nodes.iter().zip(&index.nodes) {
			let Some(placement) = placement else {
				continue;
			};

			if placement.height != node.height {
				for &command in &node.boxes {
					let mut draw = self.draws.at(command);
					if let Draw::Box { rect, .. } = &mut draw {
						rect.h += placement.height - node.height;
					}
					index.draws.insert(command, draw);
				}
			}
			if let Some(disclosure) = &node.disclosure
				&& !placement.expanded
			{
				let mut marker = self.draws.at(disclosure.marker);
				if let Draw::Polygon { points, .. } = &mut marker {
					*points = points
						.iter()
						.map(|[x, y]| [*y, -x])
						.collect::<Vec<_>>()
						.into();
				}
				index.draws.insert(disclosure.marker, marker);
			}
		}
		index.spans.shrink_to_fit();
		index
	}
}

impl Scene {
	fn seal_buffers(&mut self) {
		let scene = &mut **self;
		scene.nodes.shrink_to_fit();
		scene.spans.shrink_to_fit();
		scene.footnotes.shrink_to_fit();
		scene.leaves.shrink_to_fit();
		for node in &mut scene.nodes {
			node.children.shrink_to_fit();
			node.boxes.shrink_to_fit();
		}
	}

	fn place(
		&self,
		node: usize,
		parent: [f32; 2],
		open: &BTreeMap<crate::document::BlockId, bool>,
		force: bool,
		index: &mut FlowIndex,
	) -> f32 {
		let current = &self.nodes[node];
		let offset =
			[parent[0] + current.offset[0], parent[1] + current.offset[1]];
		let expanded = current.disclosure.as_ref().is_none_or(|d| {
			force || open.get(&d.id).copied().unwrap_or(d.declared)
		});
		let mut height = current.height;
		if expanded {
			let mut y = offset[1];
			for &child in &current.children {
				let node = &self.nodes[child];
				if node.parallel {
					let measured = self.place(
						child,
						[offset[0], offset[1]],
						open,
						force,
						index,
					);
					height = height.max(node.offset[1] + measured);
					continue;
				}
				let measured =
					self.place(child, [offset[0], y], open, force, index);
				y += node.offset[1] + measured;
				height += measured - node.height;
			}
			height = height.max(current.minimum);
		} else {
			height = current.disclosure.as_ref().unwrap().closed;
		}
		index.nodes[node] = Some(Placement {
			offset,
			height,
			expanded,
		});
		height
	}

	fn owner(
		&self,
		index: usize,
		range: impl Fn(&Span) -> &Range<usize>,
	) -> Option<usize> {
		self.spans
			.get(self.spans.partition_point(|span| range(span).end <= index))
			.map(|span| span.node)
	}
}

impl PlacedBlock {
	pub fn links(
		&self,
	) -> impl Iterator<Item = (super::LinkFragment<'_>, Rect)> {
		self.layout
			.links
			.iter()
			.filter(move |link| match &link.target {
				super::LinkTarget::Disclosure(_) => !self.flow.force_open,
				_ => !link.url.is_empty(),
			})
			.filter_map(move |link| {
				self.rect(link.commands.start, link.rect)
					.map(|rect| (link, rect))
			})
	}
	pub fn clusters(
		&self,
	) -> impl Iterator<Item = (usize, crate::text::TextCluster, Rect)> {
		self.layout
			.text
			.iter()
			.enumerate()
			.filter(move |(ni, _)| self.text_visible(*ni))
			.flat_map(move |(ni, node)| {
				node.clusters.iter().filter_map(move |c| {
					self.rect(c.command, c.rect).map(|rect| (ni, c, rect))
				})
			})
	}

	pub fn visible_ancestor_y(&self, command: usize) -> Option<f32> {
		let mut node = self.layout.scene.owner(command, |span| &span.draws)?;
		loop {
			if let Some(p) = self.flow.nodes[node] {
				return Some(p.offset[1]);
			}
			node = self.layout.scene.nodes[node].parent?;
		}
	}
	/// Materialize coordinates only at the paper-layout boundary.
	fn flat_layout(&self) -> Arc<BlockLayout> {
		if self.layout.scene.nodes.is_empty() {
			return self.layout.clone();
		}
		let mut out = BlockLayout {
			height: self.height(),
			width: self.layout.width,
			degraded: self.layout.degraded,
			math_errors: self.layout.math_errors,
			text: self.layout.text.clone(),
			draws: super::DrawBuffer::from_paint_iter(
				self.layout.draws.iter().enumerate().map(
					|(command, mut draw)| {
						if let Some([x, y]) = self.command_offset(command) {
							if let Some(override_draw) =
								self.flow.draws.get(&command)
							{
								draw = override_draw.clone();
							}
							draw.translate(x, y);
							draw
						} else {
							Draw::Rect(Rect::default(), super::Paint::Text)
						}
					},
				),
			),
			..Default::default()
		};
		for (ni, text) in out.text.iter_mut().enumerate() {
			if !self.text_visible(ni) {
				text.text = Arc::new(String::new());
				text.clusters.clear();
				text.source_images.clear();
				continue;
			}
			for cluster in text
				.clusters
				.iter_mut()
				.chain(text.source_images.iter_mut().map(|(_, c)| c))
			{
				cluster.rect =
					self.rect(cluster.command, cluster.rect).unwrap();
			}
		}
		out.links = self
			.links()
			.map(|(link, rect)| super::LinkRect {
				rect,
				..link.owned()
			})
			.collect();
		out.overflow = self
			.layout
			.overflow
			.iter()
			.enumerate()
			.filter_map(|(i, o)| {
				self.overflow_rect(i)
					.map(|rect| super::Overflow { rect, ..o.clone() })
			})
			.collect();
		out.anchors = self
			.anchor_positions()
			.map(|(anchor, y)| super::HeadingAnchor {
				anchor: anchor.clone(),
				y,
			})
			.collect();
		for span in &self.layout.scene.spans {
			let Some(p) = self.flow.nodes[span.node] else {
				continue;
			};
			for c in &self.layout.page_constraints[span.constraints.clone()] {
				out.page_constraints.push(super::PageConstraint {
					top: c.top + p.offset[1],
					bottom: c.bottom + p.offset[1] + p.height
						- self.layout.scene.nodes[span.node].height,
					..*c
				});
			}
			for (command, rows) in
				&self.layout.inline_decorations[span.decorations.clone()]
			{
				out.inline_decorations.push((
					*command,
					rows.start + p.offset[1]..rows.end + p.offset[1],
				));
			}
		}
		out.seal_scene();
		Arc::new(out)
	}
	pub fn height(&self) -> f32 {
		if self.layout.scene.nodes.is_empty() {
			self.layout.height
		} else {
			self.flow.height
		}
	}

	fn placement(&self, owner: Option<usize>) -> Option<[f32; 2]> {
		match owner {
			Some(node) => self.flow.nodes[node].map(|p| p.offset),
			None => Some([0., 0.]),
		}
	}

	pub fn command_offset(&self, command: usize) -> Option<[f32; 2]> {
		self.placement(self.layout.scene.owner(command, |span| &span.draws))
			.map(|offset| self.flow.offset(command, offset))
	}

	pub fn text_visible(&self, text: usize) -> bool {
		self.placement(self.layout.scene.owner(text, |span| &span.text))
			.is_some()
	}

	pub fn draw(&self, command: usize) -> Draw {
		self.flow
			.draws
			.get(&command)
			.cloned()
			.unwrap_or_else(|| self.layout.draws.at(command))
	}

	pub fn other_draw(&self, command: usize) -> Option<&Draw> {
		self.flow
			.draws
			.get(&command)
			.or_else(|| self.layout.draws.other(command))
	}
	pub fn draw_commands(&self) -> impl Iterator<Item = (usize, [f32; 2])> {
		let retained =
			self.flow.spans.iter().flat_map(move |(commands, offset)| {
				commands.clone().map(move |command| {
					(command, self.flow.offset(command, *offset))
				})
			});
		let len = if self.layout.scene.nodes.is_empty() {
			self.layout.draws.len()
		} else {
			0
		};
		retained.chain((0..len).map(|command| (command, [0., 0.])))
	}
	pub fn draws(&self) -> impl Iterator<Item = (usize, Draw, [f32; 2])> {
		let retained =
			self.flow.spans.iter().flat_map(move |(commands, offset)| {
				let start = commands.start;
				self.layout
					.draws
					.iter_range(commands.clone())
					.unwrap()
					.enumerate()
					.map(move |(i, draw)| {
						let command = start + i;
						(
							command,
							self.flow
								.draws
								.get(&command)
								.cloned()
								.unwrap_or(draw),
							self.flow.offset(command, *offset),
						)
					})
			});
		let identity = self
			.layout
			.draws
			.iter()
			.enumerate()
			.take(if self.layout.scene.nodes.is_empty() {
				self.layout.draws.len()
			} else {
				0
			})
			.map(|(command, draw)| (command, draw, [0., 0.]));
		retained.chain(identity)
	}

	pub fn anchor_positions(
		&self,
	) -> impl Iterator<Item = (&markview_document::AnchorId, f32)> {
		self.layout
			.anchors
			.iter()
			.enumerate()
			.filter_map(|(i, anchor)| {
				self.placement(self.layout.scene.owner(i, |span| &span.anchors))
					.map(|p| (&anchor.anchor, p[1] + anchor.y))
			})
	}

	pub fn overflow_rect(&self, index: usize) -> Option<Rect> {
		let [x, y] = self
			.placement(self.layout.scene.owner(index, |span| &span.overflow))?;
		let rect = self.layout.overflow[index].rect;
		Some(Rect {
			x: rect.x + x,
			y: rect.y + y,
			..rect
		})
	}

	pub fn rect(&self, command: usize, mut rect: Rect) -> Option<Rect> {
		let [x, y] = self.command_offset(command)?;
		rect.x += x;
		rect.y += y;
		Some(rect)
	}

	pub fn command_view(
		&self,
		command: usize,
		block: usize,
		horizontal: &HashMap<(usize, usize), f32>,
	) -> (f32, Option<Rect>) {
		self.layout
			.overflow
			.iter()
			.enumerate()
			.find(|(_, o)| o.commands.contains(&command))
			.map(|(oi, o)| {
				(
					horizontal
						.get(&(block, oi))
						.copied()
						.unwrap_or(0.)
						.clamp(0., (o.content_width - o.rect.w).max(0.)),
					self.overflow_rect(oi),
				)
			})
			.unwrap_or((0., None))
	}
}

impl FlowIndex {
	fn offset(&self, command: usize, mut offset: [f32; 2]) -> [f32; 2] {
		offset[1] += self.marker_offsets.get(&command).copied().unwrap_or(0.);
		offset
	}
}

impl super::LayoutSnapshot {
	pub fn flattened(&self) -> Self {
		let mut snapshot = self.clone();
		for block in &mut snapshot.blocks {
			block.layout = block.flat_layout();
			block.flow = Arc::default();
		}
		snapshot
	}
	/// Rebuild presentation from retained geometry without invoking a text engine.
	pub fn set_disclosures(
		&mut self,
		open: &BTreeMap<crate::document::BlockId, bool>,
		force_open: bool,
	) {
		let key = crate::document::fingerprint(&(open, force_open));
		if self.presentation_key == key {
			return;
		}
		self.presentation_key = key;
		let opening = self.blocks.first().map_or(self.height, |b| b.y);
		let trailing = self
			.blocks
			.last()
			.map_or(0., |b| self.height - b.y - b.height());
		let mut y = opening;
		for block in &mut self.blocks {
			block.flow = Arc::new(block.layout.resolve_flow(open, force_open));
			block.y = y;
			y += block.height();
		}
		self.height = y + trailing;
		if let Some(Draw::Box { rect, .. }) = &mut self.document_box {
			rect.h = self.height;
		}
	}
}

#[cfg(feature = "fuzz")]
impl Scene {
	pub(super) fn visit_storage(
		&self,
		visit: &mut super::memory::StorageVisitor<'_>,
	) {
		use super::memory::allocation;
		visit(
			Arc::as_ptr(&self.0) as usize,
			"scene",
			size_of::<SceneData>() + 2 * size_of::<usize>(),
			0,
			0,
		);
		allocation(&self.nodes, self.nodes.capacity(), "scene", visit);
		allocation(&self.spans, self.spans.capacity(), "scene", visit);
		allocation(&self.leaves, self.leaves.capacity(), "scene", visit);
		allocation(&self.footnotes, self.footnotes.capacity(), "scene", visit);
		for node in &self.nodes {
			allocation(
				&node.children,
				node.children.capacity(),
				"scene",
				visit,
			);
			allocation(&node.boxes, node.boxes.capacity(), "scene", visit);
		}
	}
}
#[cfg(feature = "fuzz")]
impl LeafGeometry {
	pub(crate) fn visit_storage(
		&self,
		visit: &mut super::memory::StorageVisitor<'_>,
	) {
		use super::memory::allocation;
		visit(
			std::ptr::from_ref(self) as usize,
			"cached_leaves",
			size_of::<Self>() + 2 * size_of::<usize>(),
			1,
			1,
		);
		self.draws.visit_storage(visit);
		allocation(&self.text, self.text.len(), "text_nodes", visit);
		for node in &self.text {
			node.visit_storage(visit);
		}
		allocation(&self.links, self.links.len(), "link_geometry", visit);
		allocation(
			&self.occurrences,
			self.occurrences.len(),
			"occurrences",
			visit,
		);
		allocation(&self.overflow, self.overflow.len(), "overflow", visit);
		allocation(&self.anchors, self.anchors.len(), "anchors", visit);
		allocation(
			&self.constraints,
			self.constraints.len(),
			"page_constraints",
			visit,
		);
		allocation(
			&self.decorations,
			self.decorations.len(),
			"decorations",
			visit,
		);
	}
}

#[cfg(feature = "fuzz")]
impl FlowIndex {
	pub(super) fn visit_storage(
		&self,
		visit: &mut super::memory::StorageVisitor<'_>,
	) {
		use super::memory::allocation;
		visit(
			std::ptr::from_ref(self) as usize,
			"flow",
			size_of::<Self>() + 2 * size_of::<usize>(),
			0,
			0,
		);
		allocation(&self.nodes, self.nodes.capacity(), "flow", visit);
		allocation(&self.spans, self.spans.capacity(), "flow", visit);
	}
}
