//! Block anchors supplied by Comrak's Obsidian marker nodes.
use comrak::nodes::{AstNode, NodeBlockId, NodeValue};

pub(crate) fn has_marker<'a>(node: &'a AstNode<'a>) -> bool {
	node.last_child().is_some_and(|last| {
		matches!(last.data.borrow().value, NodeValue::BlockId(_))
	})
}

/// The marker labelling this node, unless a containing block owns it.
pub(crate) fn anchor<'a>(node: &'a AstNode<'a>) -> Option<String> {
	if !is_container(node) && !has_marker(node) {
		return None;
	}
	let marker = owned_marker(node)?;
	let owner = marker.parent().unwrap();
	if super::parse::note_embed(owner).is_some() {
		if !std::ptr::eq(node, owner) {
			return None;
		}
	} else if node.ancestors().skip(1).any(|parent| {
		is_container(parent)
			&& owned_marker(parent)
				.is_some_and(|other| std::ptr::eq(marker, other))
	}) {
		return None;
	}
	let NodeValue::BlockId(id) = &marker.data.borrow().value else {
		unreachable!();
	};
	Some(format!("^{}", id.name))
}

fn owned_marker<'a>(node: &'a AstNode<'a>) -> Option<&'a AstNode<'a>> {
	opening_item_marker(node).or_else(|| trailing_marker(node))
}

fn opening_item_marker<'a>(item: &'a AstNode<'a>) -> Option<&'a AstNode<'a>> {
	let data = item.data.borrow();
	if !matches!(data.value, NodeValue::Item(_) | NodeValue::TaskItem(_)) {
		return None;
	}
	let first = item.first_child()?;
	let marker = first.last_child()?;
	let marker_data = marker.data.borrow();
	(matches!(marker_data.value, NodeValue::BlockId(_))
		&& marker_data.sourcepos.start.line == data.sourcepos.start.line
		&& super::parse::note_embed(first).is_none())
	.then_some(marker)
}

fn is_container(node: &AstNode<'_>) -> bool {
	matches!(
		node.data.borrow().value,
		NodeValue::BlockQuote
			| NodeValue::Callout(_)
			| NodeValue::Table(_)
			| NodeValue::GridTable(_)
			| NodeValue::Alert(_)
			| NodeValue::List(_)
			| NodeValue::Item(_)
			| NodeValue::TaskItem(_)
	)
}

/// Attaches standalone list IDs before the tree is lowered to semantic blocks.
pub(crate) fn attach_list_markers<'a>(
	root: &'a AstNode<'a>,
	source: &str,
	lines: &[usize],
) {
	let placements: Vec<_> = root
		.descendants()
		.filter_map(|paragraph| {
			if !matches!(paragraph.data.borrow().value, NodeValue::Paragraph) {
				return None;
			}
			let marker = paragraph.last_child()?;
			let (owner, remove) = list_marker_position(paragraph, marker)?;
			let name = marker_name(marker, source, lines)?;
			Some((marker, owner, remove, name))
		})
		.collect();
	for (marker, owner, remove, name) in placements {
		marker.data.borrow_mut().value =
			NodeValue::BlockId(Box::new(NodeBlockId::new(name)));
		remove.detach();
		owner.append(marker);
	}
}

fn list_marker_position<'a>(
	paragraph: &'a AstNode<'a>,
	marker: &'a AstNode<'a>,
) -> Option<(&'a AstNode<'a>, &'a AstNode<'a>)> {
	let first_line = paragraph.data.borrow().sourcepos.start.line;
	if marker.previous_sibling().is_none()
		&& let Some(list) = paragraph.previous_sibling()
		&& matches!(list.data.borrow().value, NodeValue::List(_))
		&& first_line > list.data.borrow().sourcepos.end.line + 1
	{
		return Some((list, paragraph));
	}
	let item = paragraph.parent()?;
	let marker_start = marker.data.borrow().sourcepos.start;
	if marker_start.column <= item.data.borrow().sourcepos.start.column
		|| !matches!(
			item.data.borrow().value,
			NodeValue::Item(_) | NodeValue::TaskItem(_)
		) {
		return None;
	}
	match marker.previous_sibling() {
		Some(break_node)
			if paragraph.next_sibling().is_none()
				&& matches!(
					break_node.data.borrow().value,
					NodeValue::SoftBreak | NodeValue::LineBreak
				)
				&& marker_start.line > first_line =>
		{
			Some((paragraph, break_node))
		}
		None if paragraph.previous_sibling().is_some() => {
			Some((item, paragraph))
		}
		_ => None,
	}
}

fn marker_name<'a>(
	marker: &AstNode<'_>,
	source: &'a str,
	lines: &[usize],
) -> Option<&'a str> {
	let data = marker.data.borrow();
	let NodeValue::Text(_) = data.value else {
		return None;
	};
	let pos = data.sourcepos;
	if pos.start.line != pos.end.line {
		return None;
	}
	let start = *lines.get(pos.start.line.checked_sub(1)?)?;
	let end = lines.get(pos.start.line).copied().unwrap_or(source.len());
	let caret = start + pos.start.column.checked_sub(1)?;
	if !source
		.get(start..caret)?
		.bytes()
		.all(|byte| matches!(byte, b' ' | b'\t' | b'>'))
	{
		return None;
	}
	let tail = source.get(caret..end)?;
	let name = tail.trim_end_matches(['\r', '\n']).strip_prefix('^')?;
	(!name.is_empty()
		&& name
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'))
	.then_some(name)
}

fn trailing_marker<'a>(mut node: &'a AstNode<'a>) -> Option<&'a AstNode<'a>> {
	while let Some(last) = node.last_child() {
		match last.data.borrow().value {
			NodeValue::BlockId(_) => return Some(last),
			NodeValue::List(_)
			| NodeValue::Item(_)
			| NodeValue::TaskItem(_) => {
				return None;
			}
			_ => node = last,
		}
	}
	None
}
