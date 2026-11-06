//! Harness-only payload capacities; allocator totals include unlisted map buckets.
use crate::*;
pub type StorageVisitor<'a> =
	dyn FnMut(usize, &'static str, usize, usize, usize) + 'a;
fn array<T>(
	v: &Vec<T>,
	category: &'static str,
	visit: &mut StorageVisitor<'_>,
) {
	if v.capacity() != 0 {
		visit(
			v.as_ptr() as usize,
			category,
			v.capacity() * size_of::<T>(),
			v.len(),
			v.capacity(),
		);
	}
}
fn text(s: &String, visit: &mut StorageVisitor<'_>) {
	if s.capacity() != 0 {
		visit(
			s.as_ptr() as usize,
			"semantic_text",
			s.capacity(),
			s.len(),
			s.capacity(),
		);
	}
}
fn rich(r: &RichText, visit: &mut StorageVisitor<'_>) {
	array(r, "semantic_inlines", visit);
	for i in r {
		match &i.kind {
			InlineKind::Text(s)
			| InlineKind::Anchor(s)
			| InlineKind::Math { latex: s, .. } => text(s, visit),
			InlineKind::FootnoteRef(f) => text(&f.definition.0, visit),
			InlineKind::Image(i) => {
				visit(
					std::ptr::from_ref(&**i) as usize,
					"semantic_images",
					size_of::<image::ImageSpec>(),
					1,
					1,
				);
				text(&i.alt, visit);
				text(&i.title, visit);
				match &i.source {
					ImageSource::Resource(r) => text(&r.address, visit),
					ImageSource::Diagram(d) => {
						text(&d.language, visit);
						text(&d.source, visit);
					}
				}
			}
			InlineKind::LineBreak(_) => {}
		}
	}
}
fn blocks(b: &Vec<Block>, visit: &mut StorageVisitor<'_>) {
	array(b, "semantic_blocks", visit);
	for b in b {
		block(b, visit);
	}
}
fn callout_storage(callout: &Option<Callout>, visit: &mut StorageVisitor<'_>) {
	if let Some(c) = callout {
		text(&c.name, visit);
		if let Some(r) = &c.title {
			rich(r, visit);
		}
	}
}
fn table(t: &Table, visit: &mut StorageVisitor<'_>) {
	if let Some(r) = &t.caption {
		rich(r, visit);
	}
	array(&t.columns, "semantic_containers", visit);
	array(&t.groups, "semantic_containers", visit);
	for g in &t.groups {
		array(&g.rows, "semantic_containers", visit);
		for r in &g.rows {
			array(&r.cells, "semantic_containers", visit);
			for c in &r.cells {
				blocks(&c.blocks, visit);
			}
		}
	}
}
fn definitions(items: &Vec<DefinitionItem>, visit: &mut StorageVisitor<'_>) {
	array(items, "semantic_containers", visit);
	for i in items {
		array(&i.terms, "semantic_containers", visit);
		for t in &i.terms {
			rich(t, visit);
		}
		array(&i.definitions, "semantic_containers", visit);
		for b in &i.definitions {
			blocks(b, visit);
		}
	}
}
fn block(b: &Block, visit: &mut StorageVisitor<'_>) {
	array(&b.anchors, "semantic_anchors", visit);
	for a in &b.anchors {
		text(a, visit);
	}
	match &b.kind {
		BlockKind::Paragraph(r) => rich(r, visit),
		BlockKind::Heading {
			text: r, anchor, ..
		} => {
			rich(r, visit);
			text(anchor, visit);
		}
		BlockKind::Code { language, text: s } => {
			text(language, visit);
			text(s, visit);
		}
		BlockKind::Quote { callout, blocks: b } => {
			callout_storage(callout, visit);
			blocks(b, visit);
		}
		BlockKind::Footnote { id, blocks: b, .. } => {
			text(&id.0, visit);
			blocks(b, visit);
		}
		BlockKind::Details {
			summary, blocks: b, ..
		} => {
			rich(summary, visit);
			blocks(b, visit);
		}
		BlockKind::FrontMatter { blocks: b, .. } => blocks(b, visit),
		BlockKind::List { items, .. } => {
			array(items, "semantic_containers", visit);
			for i in items {
				blocks(&i.blocks, visit);
			}
		}
		BlockKind::Table(t) => table(t, visit),
		BlockKind::DefinitionList(items) => definitions(items, visit),
		BlockKind::Figure { blocks: b, caption } => {
			blocks(b, visit);
			if let Some(r) = caption {
				rich(r, visit);
			}
		}
		BlockKind::Rule => {}
	}
}
impl Document {
	pub fn visit_storage(&self, visit: &mut StorageVisitor<'_>) {
		blocks(&self.blocks, visit);
		array(&self.index.path_data, "semantic_paths", visit);
		array(&self.index.outline, "semantic_outline", visit);
		for o in &self.index.outline {
			text(&o.text, visit);
			text(&o.anchor, visit);
		}
		for path in self.index.enclosing.values() {
			array(path, "semantic_paths", visit);
		}
	}
}
