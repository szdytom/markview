//! Block anchors supplied by Comrak's Obsidian marker nodes.
use comrak::nodes::{AstNode, NodeValue};

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
	let marker = trailing_marker(node)?;
	if node.ancestors().skip(1).any(|parent| {
		is_container(parent)
			&& trailing_marker(parent)
				.is_some_and(|other| std::ptr::eq(marker, other))
	}) {
		return None;
	}
	let NodeValue::BlockId(id) = &marker.data.borrow().value else {
		unreachable!();
	};
	Some(format!("^{}", id.name))
}

fn is_container(node: &AstNode<'_>) -> bool {
	matches!(
		node.data.borrow().value,
		NodeValue::BlockQuote
			| NodeValue::Callout(_)
			| NodeValue::Table(_)
			| NodeValue::GridTable(_)
			| NodeValue::Alert(_)
			| NodeValue::Item(_)
			| NodeValue::TaskItem(_)
	)
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
