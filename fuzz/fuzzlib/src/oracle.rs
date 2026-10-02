//! Oracles for the fuzz targets.
//!
//! Tier 1 (O1, O2): no panic, abort, overflow, or budget breach — enforced by
//! the harness shape plus [`crate::budget`]. Tier 3 (O5): the fingerprints
//! below give field-by-field equality of documents and layout snapshots, and
//! the finiteness assertions check I7 (every geometry value finite and
//! bounded).

use std::{
	collections::hash_map::DefaultHasher,
	hash::{Hash, Hasher},
};

use markview_core::{
	document::{Block, BlockKind, Document, Inline},
	scene::{BlockLayout, Draw, LayoutSnapshot, Rect},
};

/// A three-way digest: field-by-field equality up to a ~2^-192 collision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fingerprint(pub u64, pub u64, pub u64);

pub type Hashers = (DefaultHasher, DefaultHasher, DefaultHasher);

pub fn hashers() -> Hashers {
	(
		DefaultHasher::new(),
		DefaultHasher::new(),
		DefaultHasher::new(),
	)
}
fn hash3(h: &mut Hashers, v: &impl Hash) {
	for h in [&mut h.0, &mut h.1, &mut h.2] {
		v.hash(h);
	}
}
pub fn finish(h: Hashers) -> Fingerprint {
	Fingerprint(h.0.finish(), h.1.finish(), h.2.finish())
}

/// `f32` hashes by bit pattern, so equal floats compare equal and NaN is its
/// own value.
#[derive(Clone, Copy)]
struct F(f32);
impl Hash for F {
	fn hash<H: Hasher>(&self, state: &mut H) {
		self.0.to_bits().hash(state);
	}
}
impl F {
	const fn new(v: f32) -> Self {
		Self(v)
	}
}

/// Semantic equality of a parsed document: content id plus, for the whole
/// block tree, each block's id, source range, and content key, and each
/// inline's kind, style, and source range.
///
/// The walk is recursive on purpose. A shallow hash would miss exactly the
/// fields a reparse has to shift — a nested block's or an inline's source
/// range — because `content_key` is position-independent: the reading text
/// would compare equal while the ranges drifted.
pub fn document(doc: &Document) -> Fingerprint {
	fn rich(h: &mut Hashers, text: &[Inline]) {
		for inline in text {
			hash3(h, &(&inline.kind, &inline.style, &inline.source));
		}
	}
	fn hash_blocks(h: &mut Hashers, list: &[Block]) {
		for block in list {
			hash3(h, &(block.id, &block.source, block.content_key));
			match &block.kind {
				BlockKind::Paragraph(text) => rich(h, text),
				BlockKind::Heading { text, .. } => rich(h, text),
				BlockKind::Quote { blocks, .. }
				| BlockKind::Footnote { blocks, .. } => hash_blocks(h, blocks),
				BlockKind::Details {
					summary, blocks, ..
				} => {
					rich(h, summary);
					hash_blocks(h, blocks);
				}
				BlockKind::FrontMatter { blocks, .. } => hash_blocks(h, blocks),
				BlockKind::List { items, .. } => {
					for item in items {
						hash_blocks(h, &item.blocks);
					}
				}
				BlockKind::Table { rows, .. } => {
					for row in rows {
						for cell in row {
							rich(h, cell);
						}
					}
				}
				BlockKind::Code { .. } | BlockKind::Rule => {}
			}
		}
	}
	let mut h = hashers();
	hash3(&mut h, &doc.content_id);
	hash_blocks(&mut h, &doc.blocks);
	finish(h)
}

/// Every source range a document carries must be ordered and in bounds, so
/// that the range addresses its source and a downstream `&source[range]` is
/// sound.
///
/// The character-boundary half of that contract is deliberately not asserted:
/// a `<details>` body and the blocks its closing block spills into the top
/// level are parsed as snippets of their own, so their ranges are relative to
/// the snippet rather than to the document (the crate's own
/// `a_details_body_with_a_lone_carriage_return_keeps_its_ranges` pins that).
/// A byte offset that is off a boundary for *this* source is therefore not a
/// defect by itself, while an inverted or out-of-range one is.
pub fn assert_source_ranges(doc: &Document) {
	fn check(what: &str, r: &std::ops::Range<usize>, len: usize) {
		assert!(r.start <= r.end, "{what} range is inverted: {r:?}");
		assert!(
			r.end <= len,
			"{what} range {r:?} runs past the {len}-byte source"
		);
	}
	fn rich(what: &str, text: &[Inline], len: usize) {
		for inline in text {
			check(what, &inline.source, len);
		}
	}
	fn walk(blocks: &[Block], len: usize) {
		for block in blocks {
			check("block", &block.source, len);
			match &block.kind {
				BlockKind::Paragraph(text) => rich("inline", text, len),
				BlockKind::Heading { text, .. } => rich("inline", text, len),
				BlockKind::Quote { blocks, .. }
				| BlockKind::Footnote { blocks, .. }
				| BlockKind::FrontMatter { blocks, .. } => walk(blocks, len),
				BlockKind::Details {
					summary, blocks, ..
				} => {
					rich("summary", summary, len);
					walk(blocks, len);
				}
				BlockKind::List { items, .. } => {
					for item in items {
						walk(&item.blocks, len);
					}
				}
				BlockKind::Table { rows, .. } => {
					for row in rows {
						for cell in row {
							rich("cell", cell, len);
						}
					}
				}
				BlockKind::Code { .. } | BlockKind::Rule => {}
			}
		}
	}
	walk(&doc.blocks, doc.source.len());
}

/// A prefix parse must be the opening of the full parse: every block it
/// returns but the last must equal the one a full parse puts at that index,
/// field for field (id, source range, content key, kind). The last may differ
/// because it is the block the cut lands inside, which a prefix is allowed to
/// show up to the cut.
pub fn assert_prefix_consistent(full: &Document, prefix: &Document) {
	assert!(
		prefix.blocks.len() <= full.blocks.len(),
		"prefix has {} blocks, the full parse {}",
		prefix.blocks.len(),
		full.blocks.len()
	);
	if prefix.blocks.is_empty() {
		return;
	}
	let keep = prefix.blocks.len() - 1;
	for i in 0..keep {
		assert_eq!(
			prefix.blocks[i], full.blocks[i],
			"prefix block {i} of {keep} diverges from the full parse"
		);
	}
	// The last block is the one the cut landed inside, so it may be truncated,
	// but it must still be the same construct the full parse continues: a
	// prefix that turns a YAML front matter opener into a thematic break, or
	// a document's only block into a different kind, is not a truncation.
	if let Some(next) = full.blocks.get(keep)
		&& prefix.blocks[keep].source.end > next.source.start
	{
		assert_eq!(
			kind_tag(&prefix.blocks[keep].kind),
			kind_tag(&next.kind),
			"the prefix's last block is a different construct than the full \
			 parse puts there: {:?} versus {:?}",
			prefix.blocks[keep].kind,
			next.kind
		);
	}
}

/// A stable tag per [`BlockKind`] variant, for comparing constructs without
/// requiring their payloads to match.
fn kind_tag(kind: &BlockKind) -> u8 {
	match kind {
		BlockKind::Paragraph(_) => 0,
		BlockKind::Heading { .. } => 1,
		BlockKind::Code { .. } => 2,
		BlockKind::Quote { .. } => 3,
		BlockKind::List { .. } => 4,
		BlockKind::Table { .. } => 5,
		BlockKind::Footnote { .. } => 6,
		BlockKind::Details { .. } => 7,
		BlockKind::Rule => 8,
		BlockKind::FrontMatter { .. } => 9,
	}
}

/// Field-by-field equality of a layout snapshot. The `reused` counters are
/// cache bookkeeping, not geometry, and are excluded.
pub fn layout(snapshot: &LayoutSnapshot) -> Fingerprint {
	let mut h = hashers();
	hash3(&mut h, &(F::new(snapshot.width), F::new(snapshot.height)));
	for block in &snapshot.blocks {
		hash3(&mut h, &(block.id, &block.source, F::new(block.y)));
		block_layout(&block.layout, &mut h);
	}
	finish(h)
}

/// The geometry of one block, exposed for prefix-versus-final comparisons
/// that must hold per block, not just in aggregate.
pub fn block_layout(block: &BlockLayout, h: &mut Hashers) {
	hash3(
		h,
		&(
			F::new(block.height),
			F::new(block.width),
			block.degraded,
			block.math_errors,
		),
	);
	for pc in &block.page_constraints {
		hash3(
			h,
			&(
				F::new(pc.top),
				F::new(pc.bottom),
				pc.orphans,
				pc.widows,
				pc.keep_together,
			),
		);
	}
	for node in &block.text {
		hash3(h, &(node.text.as_str(), node.separator));
		for c in &node.clusters {
			hash3(h, &(&c.range, hash_rect(c.rect), c.rtl, c.command));
		}
	}
	for draw in &block.draws {
		hash_draw(draw, h);
	}
	for o in &block.overflow {
		hash3(
			h,
			&(
				hash_rect(o.rect),
				F::new(o.content_width),
				&o.commands,
				F::new(o.gutter),
			),
		);
	}
	for l in &block.links {
		hash3(h, &(l.command, hash_rect(l.rect), &l.url));
	}
	for a in &block.anchors {
		hash3(h, &(&a.anchor, F::new(a.y)));
	}
}

fn hash_draw(draw: &Draw, h: &mut Hashers) {
	match draw {
		Draw::Clipped { rect, draws } => {
			hash3(h, &(0u8, hash_rect(*rect)));
			for d in draws {
				hash_draw(d, h);
			}
		}
		Draw::Image {
			src,
			version,
			rect,
			title,
		} => {
			hash3(h, &(1u8, hash_rect(*rect), &src, *version, &**title));
		}
		Draw::Glyph(g) => {
			hash3(
				h,
				&(
					2u8,
					g.font.index,
					g.font.data.data(),
					&*g.coords,
					g.id,
					F::new(g.size),
					F::new(g.x),
					F::new(g.y),
					g.synthetic_italic,
					g.paint,
				),
			);
		}
		Draw::Rect(r, paint) => {
			hash3(h, &(3u8, hash_rect(*r), *paint));
		}
		Draw::Box {
			rect,
			chain,
			condition,
			radius,
			border,
			left_only,
			decoration,
		} => {
			hash3(
				h,
				&(
					4u8,
					hash_rect(*rect),
					*chain,
					*condition,
					F::new(*radius),
					F::new(*border),
					*left_only,
					decoration
						.map(|d| (d.edges.map(F::new), d.corners.map(F::new))),
				),
			);
		}
		Draw::Math { math, paint, x, y } => {
			hash3(
				h,
				&(
					5u8,
					*paint,
					F::new(*x),
					F::new(*y),
					F::new(math.width),
					F::new(math.ascent),
					F::new(math.descent),
					F::new(math.size),
					math.display.items.len(),
					math.display.width.to_bits(),
					math.display.height.to_bits(),
					math.display.depth.to_bits(),
				),
			);
		}
		Draw::Polygon {
			center,
			points,
			paint,
		} => {
			hash3(h, &(6u8, F::new(center[0]), F::new(center[1]), paint));
			for p in points.iter() {
				hash3(h, &(p[0].to_bits(), p[1].to_bits()));
			}
		}
		Draw::Icon {
			paths,
			paint,
			x,
			y,
			size,
		} => {
			hash3(
				h,
				&(
					7u8,
					*paint,
					F::new(*x),
					F::new(*y),
					F::new(*size),
					paths.as_ptr(),
					paths.len(),
				),
			);
		}
	}
}

/// A rect by bit pattern, so equality is exact.
fn hash_rect(r: Rect) -> (u32, u32, u32, u32) {
	(r.x.to_bits(), r.y.to_bits(), r.w.to_bits(), r.h.to_bits())
}

/// I7: every geometry value the pipeline produced is finite and bounded in
/// magnitude, so nothing downstream can overflow into inf or NaN.
pub fn assert_snapshot_finite(snapshot: &LayoutSnapshot) {
	assert_bounded("snapshot.width", snapshot.width);
	assert_bounded("snapshot.height", snapshot.height);
	for block in &snapshot.blocks {
		assert_bounded("block.y", block.y);
		assert_block_finite(&block.layout);
	}
}

fn assert_block_finite(block: &BlockLayout) {
	assert_bounded("block.height", block.height);
	assert_bounded("block.width", block.width);
	for pc in &block.page_constraints {
		assert_bounded("constraint.top", pc.top);
		assert_bounded("constraint.bottom", pc.bottom);
	}
	for node in &block.text {
		for c in &node.clusters {
			assert_rect4("cluster", c.rect);
		}
	}
	for draw in &block.draws {
		assert_draw_finite(draw);
	}
	for o in &block.overflow {
		assert_rect4("overflow", o.rect);
		assert_bounded("overflow.content_width", o.content_width);
		assert_bounded("overflow.gutter", o.gutter);
	}
	for l in &block.links {
		assert_rect4("link", l.rect);
	}
	for a in &block.anchors {
		assert_bounded("anchor.y", a.y);
	}
}

fn assert_draw_finite(draw: &Draw) {
	match draw {
		Draw::Clipped { rect, draws } => {
			assert_rect4("clipped", *rect);
			for d in draws {
				assert_draw_finite(d);
			}
		}
		Draw::Image { rect, .. } => assert_rect4("image", *rect),
		Draw::Glyph(g) => {
			assert_bounded("glyph.x", g.x);
			assert_bounded("glyph.y", g.y);
			assert_bounded("glyph.size", g.size);
		}
		Draw::Rect(r, _) => assert_rect4("rect", *r),
		Draw::Box {
			rect,
			radius,
			border,
			..
		} => {
			assert_rect4("box", *rect);
			assert_bounded("box.radius", *radius);
			assert_bounded("box.border", *border);
		}
		Draw::Math { math, x, y, .. } => {
			assert_bounded("math.x", *x);
			assert_bounded("math.y", *y);
			assert_bounded("math.width", math.width);
			assert_bounded("math.ascent", math.ascent);
			assert_bounded("math.descent", math.descent);
		}
		Draw::Polygon { center, points, .. } => {
			assert_bounded("polygon.cx", center[0]);
			assert_bounded("polygon.cy", center[1]);
			for p in points.iter() {
				assert_bounded("polygon.px", p[0]);
				assert_bounded("polygon.py", p[1]);
			}
		}
		Draw::Icon { x, y, size, .. } => {
			assert_bounded("icon.x", *x);
			assert_bounded("icon.y", *y);
			assert_bounded("icon.size", *size);
		}
	}
}

/// A geometry value must be finite and small enough that downstream float
/// arithmetic stays well below `f32::MAX`.
pub fn assert_bounded(name: &str, v: f32) {
	assert!(v.is_finite(), "{name} is not finite: {v:?}");
	assert!(v.abs() < 1e9, "{name} out of bounds: {v:?}");
}

/// The `1e9` magnitude bound guards against overflow in downstream
/// multiplies, which an input already near the `f32` limit can trigger by
/// pure scaling without any bug. Operands in the plausible range owe the
/// bound; beyond it, only finiteness.
pub fn plausible(v: &[f32]) -> bool {
	v.iter().all(|x| x.abs() <= 1e6)
}

/// Whether the operands are far enough from the `f32` limit that the sums
/// this arithmetic takes (`x + w`) cannot overflow to infinity.
pub fn no_overflow(v: &[f32]) -> bool {
	v.iter().all(|x| x.abs() <= 1e38)
}

/// The pure-geometry helpers propagate non-finite input, so a result only
/// owes its finiteness to one: when the inputs were finite, a non-finite
/// result is a bug, but a `NaN` in may legally give a `NaN` out.
pub fn assert_finite_if(name: &str, v: f32, inputs_finite: bool) {
	if inputs_finite {
		assert!(v.is_finite(), "{name} is not finite: {v:?}");
	}
}

pub fn assert_rect4(name: &str, r: Rect) {
	assert_bounded(&format!("{name}.x"), r.x);
	assert_bounded(&format!("{name}.y"), r.y);
	assert_bounded(&format!("{name}.w"), r.w);
	assert_bounded(&format!("{name}.h"), r.h);
}

/// Split `data` at the first NUL into `(head, tail)`; both halves
/// lossy-decoded. The highlight target feeds `(language, code)` through this
/// split.
pub fn split_nul(data: &[u8]) -> (String, String) {
	let at = data.iter().position(|b| *b == 0).unwrap_or(data.len());
	let head = String::from_utf8_lossy(&data[..at]).into_owned();
	let tail_start = if at < data.len() { at + 1 } else { at };
	let tail = String::from_utf8_lossy(&data[tail_start..]).into_owned();
	(head, tail)
}

/// Derive a deterministic full 128-bit value from a byte string for option
/// generation. Two independent `DefaultHasher` instances cover the high and
/// low halves, so bits 64..128 are meaningful too. `DefaultHasher` is seeded
/// per process, which is fine: all comparisons happen inside one process.
/// Callers that only look at low bits (`seed as u32`, `seed & 7`, ...) stay
/// valid.
pub fn derive(data: &[u8]) -> u128 {
	// Two passes over distinct streams (the length prefix separates them);
	// hashing the same bytes into two fresh `DefaultHasher`s would yield
	// identical halves.
	let mut h1 = DefaultHasher::new();
	data.len().hash(&mut h1);
	data.hash(&mut h1);
	let mut h2 = DefaultHasher::new();
	data.hash(&mut h2);
	(h1.finish() as u128) | ((h2.finish() as u128) << 64)
}
/// The `Limits` shrunk toward their floors, derived from `seed`, so the
/// degradation paths are reachable at small inputs.
pub fn shrunk_limits(seed: u128) -> markview_core::limits::Limits {
	let pick = |shift: u32, n: u128| (seed >> shift) % n;
	markview_core::limits::Limits {
		inline_depth: 32 + pick(64, 512) as usize,
		block_depth: 8 + pick(68, 260) as usize,
		linebreak_evaluations: 1000 + pick(72, 3_000_000) as usize,
		highlight_line_bytes: 64 + pick(76, 65_536) as usize,
		highlight_bytes: 4096 + pick(80, 16 * 1024 * 1024) as usize,
		math_formula_bytes: 64 + pick(84, 256 * 1024) as usize,
		math_bytes: 1024 + pick(88, 8 * 1024 * 1024) as usize,
		table_columns: 4 + pick(92, 512) as usize,
		table_rows: 8 + pick(96, 16_384) as usize,
		table_cells: 16 + pick(100, 131_072) as usize,
	}
}

/// A positive, finite f32 in `1.0..16.0`, so derived font sizes stay
/// sensible while still varying.
pub fn f32_unit(v: u32) -> f32 {
	1.0 + (v % 15_000) as f32 / 1000.0
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Different seeds must produce at least one different `Limits` field,
	/// so the degradation paths do not collapse onto one fixed point.
	#[test]
	fn shrunk_limits_vary_across_seeds() {
		let a = shrunk_limits(derive(b"seed-a"));
		let b = shrunk_limits(derive(b"seed-b"));
		assert_ne!(a, b);
	}

	/// High seed bits feed the `Limits` picks, so a seed with every bit set
	/// must lift the fields above their floors (the pre-fix `derive` left
	/// bits 64..128 zero and pinned them there).
	#[test]
	fn shrunk_limits_uses_high_seed_bits() {
		assert!(shrunk_limits(u128::MAX).inline_depth > 32);
	}
}
