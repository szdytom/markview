//! Retained local geometry and its inexpensive presentation index.
use super::{BlockLayout, Draw, PlacedBlock, Rect};
use std::{
	collections::{BTreeMap, HashMap},
	ops::Range,
	sync::Arc,
};

#[derive(Debug, Default)]
pub struct Scene {
	nodes: Vec<Node>,
	spans: Vec<Span>,
	footnotes: Vec<Footnote>,
	active: Option<usize>,
	cursor: Cursor,
}

#[derive(Debug)]
struct Node {
	parent: Option<usize>,
	children: Vec<usize>,
	/// Local x and the gap after the preceding sibling.
	offset: [f32; 2],
	height: f32,
	minimum: f32,
	boxes: Vec<usize>,
	disclosure: Option<Disclosure>,
}

#[derive(Debug)]
struct Footnote {
	node: usize,
	body: Range<usize>,
	marker: Range<usize>,
	baseline: f32,
}

#[derive(Debug)]
struct Disclosure {
	id: u64,
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
	overflow: usize,
	anchors: usize,
	constraints: usize,
	decorations: usize,
}

#[derive(Debug)]
struct Span {
	node: usize,
	draws: Range<usize>,
	text: Range<usize>,
	links: Range<usize>,
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

impl BlockLayout {
	fn cursor(&self) -> Cursor {
		Cursor {
			draws: self.draws.len(),
			text: self.text.len(),
			links: self.links.len(),
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
				node,
				draws: start.draws..end.draws,
				text: start.text..end.text,
				links: start.links..end.links,
				overflow: start.overflow..end.overflow,
				anchors: start.anchors..end.anchors,
				constraints: start.constraints..end.constraints,
				decorations: start.decorations..end.decorations,
			});
		}
		self.scene.cursor = end;
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
		});
		if let Some(parent) = parent {
			self.scene.nodes[parent].children.push(index);
		}
		self.scene.active = Some(index);
	}

	pub(crate) fn end_scene(&mut self, height: f32, minimum: f32) {
		self.flush_scene();
		let node = &mut self.scene.nodes[self.scene.active.unwrap()];
		node.height = height;
		node.minimum = minimum;
		if let Some(disclosure) = &mut node.disclosure {
			disclosure.closed = height - disclosure.body;
		}
		self.scene.active = node.parent;
	}

	pub(crate) fn retain_disclosure(
		&mut self,
		id: u64,
		declared: bool,
		marker: usize,
		body: f32,
	) {
		self.scene.nodes[self.scene.active.unwrap()].disclosure =
			Some(Disclosure {
				id,
				declared,
				marker,
				body,
				closed: 0.,
			});
	}

	pub(crate) fn retain_footnote(
		&mut self,
		body: Range<usize>,
		marker: Range<usize>,
		baseline: f32,
	) {
		let node = self.scene.active.unwrap();
		self.scene.footnotes.push(Footnote {
			node,
			body,
			marker,
			baseline: baseline - self.scene.nodes[node].offset[1],
		});
	}

	/// Convert the builder's temporary coordinates into node-local geometry once.
	pub(crate) fn seal_scene(&mut self) {
		if self.scene.nodes.is_empty() {
			return;
		}
		self.scene.nodes[0].height = self.height;
		for span in &self.scene.spans {
			let [x, y] = self.scene.nodes[span.node].offset;
			for command in span.draws.clone() {
				let draw = &mut self.draws[command];
				if matches!(draw, Draw::Box { .. }) {
					self.scene.nodes[span.node].boxes.push(command);
				}
				draw.translate(-x, -y);
			}
			for text in &mut self.text[span.text.clone()] {
				for cluster in text
					.clusters
					.iter_mut()
					.chain(text.source_images.iter_mut().map(|(_, c)| c))
				{
					cluster.rect.x -= x;
					cluster.rect.y -= y;
				}
			}
			for link in &mut self.links[span.links.clone()] {
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
		// Children become relative to their parent and the preceding sibling's end.
		for parent in (0..self.scene.nodes.len()).rev() {
			let [x, mut y] = self.scene.nodes[parent].offset;
			for child in self.scene.nodes[parent].children.clone() {
				let node = &mut self.scene.nodes[child];
				let next = node.offset[1] + node.height;
				node.offset[0] -= x;
				node.offset[1] -= y;
				y = next;
			}
		}
	}

	pub fn resolve_flow(
		&self,
		open: &BTreeMap<u64, bool>,
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
						match &self.draws[command] {
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
					let mut draw = self.draws[command].clone();
					if let Draw::Box { rect, .. } = &mut draw {
						rect.h += placement.height - node.height;
					}
					index.draws.insert(command, draw);
				}
			}
			if let Some(disclosure) = &node.disclosure
				&& !placement.expanded
			{
				let mut marker = self.draws[disclosure.marker].clone();
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
		index
	}
}

impl Scene {
	fn place(
		&self,
		node: usize,
		parent: [f32; 2],
		open: &BTreeMap<u64, bool>,
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
				let measured =
					self.place(child, [offset[0], y], open, force, index);
				y += self.nodes[child].offset[1] + measured;
				height += measured - self.nodes[child].height;
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
	pub fn links(&self) -> impl Iterator<Item = (&super::LinkRect, Rect)> {
		self.layout
			.links
			.iter()
			.filter(move |link| {
				!link.url.is_empty()
					&& !(self.flow.force_open
						&& link
							.url
							.starts_with(crate::document::DETAILS_SCHEME))
			})
			.filter_map(move |link| {
				self.rect(link.command, link.rect).map(|rect| (link, rect))
			})
	}
	pub fn clusters(
		&self,
	) -> impl Iterator<Item = (usize, &crate::text::TextCluster, Rect)> {
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
			draws: self.layout.draws.clone(),
			..Default::default()
		};
		for (command, draw) in out.draws.iter_mut().enumerate() {
			if let Some([x, y]) = self.command_offset(command) {
				*draw = self.draw(command).clone();
				draw.translate(x, y);
			} else {
				*draw = Draw::Rect(Rect::default(), super::Paint::Text);
			}
		}
		for (ni, text) in out.text.iter_mut().enumerate() {
			if !self.text_visible(ni) {
				text.text.clear();
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
				..link.clone()
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
				anchor: anchor.into(),
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

	pub fn draw(&self, command: usize) -> &Draw {
		self.flow
			.draws
			.get(&command)
			.unwrap_or(&self.layout.draws[command])
	}

	pub fn draws(&self) -> impl Iterator<Item = (usize, &Draw, [f32; 2])> {
		let retained =
			self.flow.spans.iter().flat_map(move |(commands, offset)| {
				commands.clone().map(move |command| {
					(
						command,
						self.draw(command),
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

	pub fn anchor_positions(&self) -> impl Iterator<Item = (&str, f32)> {
		self.layout
			.anchors
			.iter()
			.enumerate()
			.filter_map(|(i, anchor)| {
				self.placement(self.layout.scene.owner(i, |span| &span.anchors))
					.map(|p| (anchor.anchor.as_str(), p[1] + anchor.y))
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
		open: &BTreeMap<u64, bool>,
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
