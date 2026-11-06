use super::*;
use crate::{
	document::{AnchorId, BlockId},
	scene::{
		HeadingAnchor, LayoutSnapshot, LinkId, LinkRect, LinkTarget, Paint,
	},
	text::{TextCluster, TextNode},
};
use proptest::prelude::*;

#[derive(Clone, Debug)]
struct Tree {
	header: u8,
	tail: u8,
	gap: u8,
	indent: u8,
	disclosure: Option<bool>,
	children: Vec<Tree>,
}

fn trees() -> impl Strategy<Value = Tree> {
	let dimensions = (
		3u8..10,
		0u8..4,
		0u8..4,
		0u8..8,
		prop::option::of(any::<bool>()),
	);
	let leaf = dimensions.clone().prop_map(
		|(header, tail, gap, indent, disclosure)| Tree {
			header,
			tail,
			gap,
			indent,
			disclosure,
			children: Vec::new(),
		},
	);
	leaf.prop_recursive(3, 16, 3, move |child| {
		(dimensions.clone(), prop::collection::vec(child, 1..4)).prop_map(
			|((header, tail, gap, indent, disclosure), children)| Tree {
				header,
				tail,
				gap,
				indent,
				disclosure,
				children,
			},
		)
	})
}

impl Tree {
	fn height(&self) -> f32 {
		f32::from(self.header + self.tail)
			+ self
				.children
				.iter()
				.map(|child| f32::from(child.gap) + child.height())
				.sum::<f32>()
	}
}

struct Node {
	id: BlockId,
	tree: Tree,
	box_command: usize,
	command: usize,
	marker: Option<usize>,
	children: Vec<Node>,
}

fn build(
	layout: &mut BlockLayout,
	tree: Tree,
	x: f32,
	y: f32,
	next: &mut u32,
) -> Node {
	let id = BlockId::new(*next).unwrap();
	*next += 1;
	layout.begin_scene(x, y);
	let box_command = layout.draws.len();
	layout.draws.push(Draw::Box {
		rect: Rect {
			x,
			y,
			w: 40.0,
			h: tree.height(),
		},
		chain: 0,
		condition: crate::style::Condition::Body,
		radius: 0.0,
		border: 1.0,
		left_only: false,
		decoration: None,
	});
	let command = layout.draws.len();
	let rect = Rect {
		x: x + 1.0,
		y: y + 1.0,
		w: 2.0,
		h: 1.0,
	};
	layout.draws.push(Draw::Rect(rect, Paint::Text));
	let mut text = TextNode::new(format!("{id}"), "\n");
	text.push(TextCluster {
		mixed_spacing: (0.0, 0.0),
		range: 0..text.text.len(),
		rect,
		rtl: false,
		atomic: false,
		command,
	});
	layout.text.push(text);
	layout.anchors.push(HeadingAnchor {
		anchor: AnchorId::Fragment(format!("{id}")),
		y: y + 1.0,
	});
	layout.links.push(LinkRect {
		commands: command..command + 1,
		id: LinkId::Special(command),
		rect,
		url: "https://a.test".into(),
		target: LinkTarget::Url("https://a.test".into()),
	});
	let marker = tree.disclosure.map(|declared| {
		let marker = layout.draws.len();
		layout.draws.push(Draw::Polygon {
			center: [x + 2.0, y + 1.0],
			points: Arc::from([[1.0, 0.0], [0.0, 1.0], [-1.0, 0.0]]),
			paint: Paint::Text,
		});
		layout.links.push(LinkRect {
			commands: marker..marker + 1,
			id: LinkId::Special(marker),
			rect,
			url: "".into(),
			target: LinkTarget::Disclosure(id),
		});
		layout.retain_disclosure(
			id,
			declared,
			marker,
			tree.height() - f32::from(tree.header + tree.tail),
		);
		marker
	});
	let mut top = y + f32::from(tree.header);
	let children = tree
		.children
		.iter()
		.map(|child| {
			top += f32::from(child.gap);
			let node = build(
				layout,
				child.clone(),
				x + f32::from(child.indent),
				top,
				next,
			);
			top += child.height();
			node
		})
		.collect();
	layout.end_scene(tree.height(), 0.0);
	Node {
		id,
		tree,
		box_command,
		command,
		marker,
		children,
	}
}

struct Visible<'a> {
	node: &'a Node,
	x: f32,
	y: f32,
	height: f32,
	expanded: bool,
}

fn visible<'a>(
	node: &'a Node,
	open: &BTreeMap<BlockId, bool>,
	force: bool,
	x: f32,
	y: f32,
	out: &mut Vec<Visible<'a>>,
) -> f32 {
	let expanded = node.tree.disclosure.is_none_or(|declared| {
		force || open.get(&node.id).copied().unwrap_or(declared)
	});
	let mut height = f32::from(node.tree.header + node.tree.tail);
	let slot = out.len();
	out.push(Visible {
		node,
		x,
		y,
		height,
		expanded,
	});
	if expanded {
		let mut top = y + f32::from(node.tree.header);
		for child in &node.children {
			top += f32::from(child.tree.gap);
			let measured = visible(
				child,
				open,
				force,
				x + f32::from(child.tree.indent),
				top,
				out,
			);
			top += measured;
			height += f32::from(child.tree.gap) + measured;
		}
	}
	out[slot].height = height;
	height
}

proptest! {
	#[test]
	fn parallel_regions_share_an_origin_and_follow_preceding_disclosure_and_minimum_heights(
		cells in prop::collection::vec((1u8..65, 0u8..65, 0u8..8), 0..7), body in 1u8..33, gap in 0u8..8, left in 0u8..16,
	) {
		let mut layout = BlockLayout::default();
		let id = BlockId::new(1).unwrap();
		let preceding = 3.0 + f32::from(body);
		let table_top = preceding + f32::from(gap);
		let table_height = cells.iter().map(|&(height, _, _)| f32::from(height)).fold(0.0, f32::max);
		let table_minimum = cells.iter().map(|&(_, minimum, _)| f32::from(minimum)).fold(0.0, f32::max);
		let measured_table = cells.iter().map(|&(height, minimum, _)| f32::from(height.max(minimum))).fold(0.0, f32::max);
		layout.begin_scene(0.0, 0.0);
		layout.begin_scene(0.0, 0.0);
		layout.draws.push(Draw::Polygon { center: [1.0, 1.0], points: Arc::from([[1.0, 0.0], [0.0, 1.0], [-1.0, 0.0]]), paint: Paint::Text });
		layout.retain_disclosure(id, true, 0, f32::from(body));
		layout.begin_scene(0.0, 3.0);
		layout.draws.push(Draw::Rect(Rect { x: 0.0, y: 3.0, w: 2.0, h: f32::from(body) }, Paint::Text));
		layout.end_scene(f32::from(body), 0.0);
		layout.end_scene(preceding, 0.0);
		layout.begin_scene(f32::from(left), table_top);
		let mut expected = Vec::new();
		let mut x = f32::from(left);
		for &(height, _, indent) in &cells {
			x += f32::from(indent);
			layout.begin_parallel_scene();
			let command = layout.draws.len();
			let rect = Rect { x, y: table_top, w: 2.0, h: f32::from(height) };
			layout.draws.push(Draw::Rect(rect, Paint::Text));
			layout.end_scene(f32::from(height), f32::NEG_INFINITY);
			expected.push((command, rect));
			x += 2.0;
		}
		layout.end_scene(table_height, table_minimum);
		let footer_top = table_top + table_height + f32::from(gap);
		layout.begin_scene(0.0, footer_top);
		let footer = layout.draws.len();
		layout.draws.push(Draw::Rect(Rect { x: 0.0, y: footer_top, w: 2.0, h: 3.0 }, Paint::Text));
		layout.end_scene(3.0, 0.0);
		layout.height = footer_top + 3.0;
		layout.end_scene(layout.height, 0.0);
		layout.seal_scene();
		let layout = Arc::new(layout);
		for open in [true, false, true] {
			let visible_preceding = if open { preceding } else { 3.0 };
			let table_top = visible_preceding + f32::from(gap);
			let flow = Arc::new(layout.resolve_flow(&BTreeMap::from([(id, open)]), false));
			let placed = PlacedBlock { id, content: Default::default(), y: 0.0, layout: layout.clone(), flow };
			prop_assert_eq!(placed.height(), table_top + measured_table + f32::from(gap) + 3.0);
			prop_assert_eq!(placed.command_offset(1).is_some(), open);
			for &(command, rect) in &expected {
				prop_assert_eq!(placed.rect(command, rect), Some(Rect { y: table_top, ..rect }));
			}
			let Draw::Rect(rect, _) = &layout.draws.at(footer) else { unreachable!() };
			prop_assert_eq!(placed.rect(footer, *rect).unwrap().y, table_top + measured_table + f32::from(gap));
		}
	}

	#[test]
	fn disclosure_sequences_match_visible_tree_geometry_without_mutating_retained_content(
		tree in trees(), changes in prop::collection::vec((any::<u16>(), prop::option::of(any::<bool>()), any::<bool>()), 0..12),
		leading in 0u8..8, trailing in 0u8..8,
	) {
		let mut layout = BlockLayout::default();
		let mut next = 1;
		let root = build(&mut layout, tree.clone(), 0.0, 0.0, &mut next);
		layout.height = tree.height();
		layout.seal_scene();
		let layout = Arc::new(layout);
		let flow = Arc::new(layout.resolve_flow(&BTreeMap::new(), false));
		let initial = flow.height;
		let mut snapshot = LayoutSnapshot {
			blocks: vec![PlacedBlock { id: root.id, content: Default::default(), y: f32::from(leading), layout: layout.clone(), flow }, PlacedBlock { id: BlockId::new(next).unwrap(), content: Default::default(), y: f32::from(leading) + initial, layout: Arc::new(BlockLayout { height: 7.0, ..Default::default() }), flow: Arc::default() }],
			height: f32::from(leading + trailing) + initial + 7.0, ..Default::default()
		};
		let reading = snapshot.full_reading_text();
		let mut open = BTreeMap::new();
		for (index, value, force) in [(0, None, false), (0, None, true)].into_iter().chain(changes) {
			let id = BlockId::new(1 + u32::from(index) % next).unwrap();
			if let Some(value) = value { open.insert(id, value); } else { open.remove(&id); }
			let mut expected = Vec::new();
			let height = visible(&root, &open, force, 0.0, 0.0, &mut expected);
			snapshot.set_disclosures(&open, force);
			prop_assert_eq!(snapshot.height, f32::from(leading + trailing) + height + 7.0);
			prop_assert_eq!(snapshot.blocks[1].y, f32::from(leading) + height);
			prop_assert!(Arc::ptr_eq(&snapshot.blocks[0].layout, &layout));
			prop_assert_eq!(snapshot.full_reading_text(), reading.clone());
			let placed = &snapshot.blocks[0];
			prop_assert_eq!(placed.height(), height);
			prop_assert_eq!(placed.clusters().count(), expected.len());
			let expected_links: usize = expected.iter().map(|view| 1 + usize::from(view.node.marker.is_some() && !force)).sum();
			prop_assert_eq!(placed.links().count(), expected_links);
			let flat = snapshot.flattened();
			let flat = &flat.blocks[0];
			prop_assert_eq!(flat.height(), height);
			let anchors: Vec<_> = placed.anchor_positions().map(|(id, y)| (id.clone(), y)).collect();
			prop_assert_eq!(anchors, expected.iter().map(|view| (AnchorId::Fragment(format!("{}", view.node.id)), view.y + 1.0)).collect::<Vec<_>>());
			let visible_commands: Vec<_> = expected.iter().flat_map(|view| [Some(view.node.box_command), Some(view.node.command), view.node.marker].into_iter().flatten()).collect();
			prop_assert_eq!(placed.draws().map(|(command, _, _)| command).collect::<Vec<_>>(), visible_commands.clone());
			for command in 0..layout.draws.len() { prop_assert_eq!(placed.command_offset(command).is_some(), visible_commands.contains(&command)); }
			for view in expected {
				prop_assert_eq!(placed.command_offset(view.node.command), Some([view.x, view.y]));
				let Draw::Box { rect, .. } = placed.draw(view.node.box_command) else { unreachable!() };
				prop_assert_eq!(rect.h, view.height);
				let Draw::Rect(rect, _) = &flat.layout.draws.at(view.node.command) else { unreachable!() };
				prop_assert_eq!(*rect, Rect { x: view.x + 1.0, y: view.y + 1.0, w: 2.0, h: 1.0 });
				if let Some(marker) = view.node.marker {
					let Draw::Polygon { points, .. } = placed.draw(marker) else { unreachable!() };
					prop_assert_eq!(&*points, if view.expanded { &[[1.0, 0.0], [0.0, 1.0], [-1.0, 0.0]] } else { &[[0.0, -1.0], [1.0, 0.0], [0.0, 1.0]] });
				}
			}
		}
	}
}

#[test]
fn one_cached_leaf_does_not_retain_its_parent_or_large_sibling() {
	let source = format!(
		"<details open>\n<summary>Summary</summary>\n\n{}\n\nSmall leaf.\n\n</details>\n",
		"Large sibling words. ".repeat(2000)
	);
	let document = markview_commonmark::parse(source);
	let mut engine = crate::layout::LayoutEngine::new();
	let snapshot =
		engine.layout(&document, &crate::layout::LayoutOptions::default());
	let parent = Arc::downgrade(&snapshot.blocks[0].layout);
	let mut leaves: Vec<_> = snapshot.blocks[0]
		.layout
		.cached_leaves()
		.map(|(_, g)| g)
		.collect();
	leaves.sort_by_key(|g| g.draws.len());
	let small = leaves.remove(0);
	assert!(small.draws.len() < 100);
	let large = leaves.pop().unwrap();
	assert!(large.draws.len() > 1000);
	let super::super::DrawBuffer::Packed(p) = &large.draws else {
		panic!("packed leaf")
	};
	let old_large = Arc::downgrade(p);
	drop(large);
	drop(leaves);
	drop(snapshot);
	engine.release_document();
	assert!(parent.upgrade().is_none());
	assert!(old_large.upgrade().is_none());
	assert!(small.text.iter().any(|t| t.text.as_str() == "Small leaf."));
}
