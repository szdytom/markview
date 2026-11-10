//! Oracles for the fuzz targets.
//!
//! Tier 1 (O1, O2): no panic, abort, overflow, or budget breach — enforced by
//! the harness shape plus [`crate::budget`]. Tier 3 (O5): the fingerprints
//! below give field-by-field equality of documents and layout snapshots, and
//! the finiteness assertions check I7 (every geometry value finite and
//! bounded).

use std::{
	collections::{HashMap, hash_map::DefaultHasher},
	hash::{Hash, Hasher},
};

use markview_commonmark::MarkdownParse;
use markview_document::{Block, BlockKind, Inline};
use markview_layout::scene::{BlockLayout, Draw, LayoutSnapshot, Rect};

/// A three-way field digest used by the differential targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Fingerprint(pub u64, pub u64, pub u64);

pub struct Hashers {
	digests: (DefaultHasher, DefaultHasher, DefaultHasher),
	fonts: HashMap<u64, Fingerprint>,
}

pub fn hashers() -> Hashers {
	Hashers {
		digests: (
			DefaultHasher::new(),
			DefaultHasher::new(),
			DefaultHasher::new(),
		),
		fonts: HashMap::new(),
	}
}
fn hash3(h: &mut Hashers, v: &impl Hash) {
	for h in [&mut h.digests.0, &mut h.digests.1, &mut h.digests.2] {
		v.hash(h);
	}
}
pub fn finish(h: Hashers) -> Fingerprint {
	Fingerprint(
		h.digests.0.finish(),
		h.digests.1.finish(),
		h.digests.2.finish(),
	)
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
pub fn document(doc: &MarkdownParse) -> Fingerprint {
	fn rich(
		h: &mut Hashers,
		doc: &MarkdownParse,
		block: &Block,
		text: &[Inline],
	) {
		for inline in text {
			hash3(
				h,
				&(
					&inline.kind,
					&inline.style,
					inline
						.link
						.as_ref()
						.map(|link| (link.as_str(), link.occurrence())),
					inline_source(doc, block, inline),
				),
			);
		}
	}
	fn hash_blocks(h: &mut Hashers, doc: &MarkdownParse, list: &[Block]) {
		for block in list {
			hash3(h, &(doc.block_source(block), block.content_key()));
			match block.kind() {
				BlockKind::Paragraph(text) => rich(h, doc, block, text),
				BlockKind::Heading { text, .. } => rich(h, doc, block, text),
				BlockKind::Quote { blocks, .. }
				| BlockKind::Footnote { blocks, .. } => hash_blocks(h, doc, blocks),
				BlockKind::Details {
					summary, blocks, ..
				} => {
					rich(h, doc, block, summary);
					hash_blocks(h, doc, blocks);
				}
				BlockKind::FrontMatter { blocks, .. }
				| BlockKind::Figure { blocks, .. } => hash_blocks(h, doc, blocks),
				BlockKind::List { items, .. } => {
					for item in items {
						hash_blocks(h, doc, &item.blocks);
					}
				}
				BlockKind::Table(table) => {
					if let Some(caption) = &table.caption {
						rich(h, doc, block, caption);
					}
					for child in table.cell_blocks() {
						hash_blocks(h, doc, std::slice::from_ref(child));
					}
				}
				BlockKind::DefinitionList(items) => {
					for item in items {
						for terms in &item.terms {
							rich(h, doc, block, terms);
						}
						for blocks in &item.definitions {
							hash_blocks(h, doc, blocks);
						}
					}
				}
				BlockKind::Code { .. } | BlockKind::Rule => {}
			}
		}
	}
	let mut h = hashers();
	hash3(&mut h, &doc.document.content_key());
	hash_blocks(&mut h, doc, doc.document.blocks());
	finish(h)
}

/// Every source range a document carries must be ordered, in bounds and on a
/// character boundary, so that the range addresses its source and a downstream
/// `&source[range]` is sound.
///
/// This is [`markview_document::SourceMap::validate`]'s contract, and the
/// parser keeps it even where a range is derived from a re-parsed snippet:
/// `<details>` bodies and the blocks a closing tag spills into the top level
/// are remapped onto document offsets, so an offset off a boundary for *this*
/// source is a defect rather than an artifact of the snippet.
///
/// The Markdown producer records every parsed block, including generated
/// blocks with no source. A present zero-width range is a valid position.
pub fn assert_source_ranges(doc: &MarkdownParse) {
	doc.map
		.validate(&doc.source, &doc.document)
		.expect("source map is valid");
	for block in doc.document.blocks() {
		markview_document::visit_blocks(block, &mut |block| {
			assert!(
				doc.map.block(block.id()).is_some(),
				"parsed block {} has no source-map record",
				block.id()
			);
		});
	}
}

/// The source range of one inline run.
///
/// Source-derived oracles require a mapped Markdown inline. Absence is
/// explicit; a zero-width source position remains valid.
pub fn inline_source(
	doc: &MarkdownParse,
	block: &Block,
	inline: &Inline,
) -> std::ops::Range<usize> {
	let range = doc
		.inline_source(block, inline)
		.expect("source-backed Markdown inline");
	assert!(
		range.start <= range.end,
		"inline range {range:?} is inverted"
	);
	range
}

/// A prefix parse must be the opening of the full parse: every block it
/// returns but the last must equal the one a full parse puts at that index,
/// field for field (source range, content key, kind). The last may differ
/// because it is the block the cut lands inside, which a prefix is allowed to
/// show up to the cut.
///
/// `BlockId` is deliberately not compared. Ids are allocated per parse, and a
/// block lowered from a raw HTML snippet takes its id from that snippet's own
/// producer, so the same block legitimately carries different ids in the two
/// parses. The reader never matches prefix blocks by id: it shows the prefix,
/// then replaces it with the full document and reuses layout by content key.
pub fn assert_prefix_consistent(full: &MarkdownParse, prefix: &MarkdownParse) {
	assert!(
		prefix.document.blocks().len() <= full.document.blocks().len(),
		"prefix has {} blocks, the full parse {}",
		prefix.document.blocks().len(),
		full.document.blocks().len()
	);
	if prefix.document.blocks().is_empty() {
		return;
	}
	let keep = prefix.document.blocks().len() - 1;
	for i in 0..keep {
		let from_prefix = &prefix.document.blocks()[i];
		let from_full = &full.document.blocks()[i];
		assert_eq!(
			prefix.block_source(from_prefix),
			full.block_source(from_full),
			"prefix block {i} of {keep} has a different source range than the \
			 full parse"
		);
		assert_eq!(
			from_prefix.content_key(),
			from_full.content_key(),
			"prefix block {i} of {keep} has a different content key than the \
			 full parse"
		);
		assert_eq!(
			kind_tag(from_prefix.kind()),
			kind_tag(from_full.kind()),
			"prefix block {i} of {keep} is a different construct than the full \
			 parse: {:?} versus {:?}",
			from_prefix.kind(),
			from_full.kind()
		);
		assert_eq!(
			from_prefix, from_full,
			"prefix block {i} of {keep} diverges from the full parse"
		);
	}
	if let Some(next) = full.document.blocks().get(keep) {
		let last = &prefix.document.blocks()[keep];
		if prefix.block_source(last).expect("parsed prefix block").end
			> full.block_source(next).expect("parsed full block").start
		{
			assert_eq!(
				kind_tag(last.kind()),
				kind_tag(next.kind()),
				"the prefix's last block is a different construct than the full \
				 parse puts there: {:?} versus {:?}",
				last.kind(),
				next.kind()
			);
		}
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
		BlockKind::DefinitionList(_) => 10,
		BlockKind::Figure { .. } => 11,
	}
}

/// Field-by-field equality of a layout snapshot. The `reused` counters are
/// cache bookkeeping, not geometry, and are excluded.
pub fn layout(snapshot: &LayoutSnapshot) -> Fingerprint {
	let snapshot = snapshot.flattened();
	let mut h = hashers();
	hash3(&mut h, &(F::new(snapshot.width), F::new(snapshot.height)));
	hash3(
		&mut h,
		&(
			snapshot.presentation_key,
			snapshot.degraded,
			snapshot.math_errors,
			snapshot.blocks.len(),
			snapshot.document_box.is_some(),
		),
	);
	if let Some(draw) = &snapshot.document_box {
		hash_draw(draw, &mut h);
	}
	for block in &snapshot.blocks {
		hash3(&mut h, &(block.id, block.content, F::new(block.y)));
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
		// Include search bindings, source images, selection groups and boundaries.
		let (images, group, boundaries) = node.reading_metadata();
		hash3(
			h,
			&(node.search_field, &node.search_ranges, group, boundaries),
		);
		for (ordinal, cluster) in images {
			hash3(
				h,
				&(
					ordinal,
					&cluster.range,
					hash_rect(cluster.rect),
					cluster.rtl,
					cluster.atomic,
					F::new(cluster.mixed_spacing.0),
					F::new(cluster.mixed_spacing.1),
				),
			);
			if let Some(draw) = block.draws.get(cluster.command) {
				hash_draw(&draw, h);
			}
		}
		hash3(h, &(node.text.as_str(), node.separator));
		for c in &node.clusters {
			hash3(
				h,
				&(
					&c.range,
					hash_rect(c.rect),
					c.rtl,
					c.atomic,
					F::new(c.mixed_spacing.0),
					F::new(c.mixed_spacing.1),
				),
			);
			if let Some(draw) = block.draws.get(c.command) {
				hash_draw(&draw, h);
			}
		}
	}
	for draw in &block.draws {
		hash_draw(&draw, h);
	}
	for o in &block.overflow {
		hash3(
			h,
			&(
				hash_rect(o.rect),
				F::new(o.content_width),
				o.commands.len(),
				F::new(o.gutter),
			),
		);
	}
	for o in &block.overflow {
		hash3(h, &block.draws.iter_range(o.commands.clone()).is_some());
		for draw in block
			.draws
			.iter_range(o.commands.clone())
			.into_iter()
			.flatten()
		{
			hash_draw(&draw, h);
		}
	}
	for l in &block.links {
		hash3(
			h,
			&(
				l.commands.len(),
				l.id,
				hash_rect(l.rect),
				&l.url,
				target_record(l.target),
			),
		);
		hash3(h, &block.draws.iter_range(l.commands.clone()).is_some());
		for draw in block
			.draws
			.iter_range(l.commands.clone())
			.into_iter()
			.flatten()
		{
			hash_draw(&draw, h);
		}
	}
	for a in &block.anchors {
		hash3(h, &(&a.anchor, F::new(a.y)));
	}
	for (command, rows) in &block.inline_decorations {
		hash_draw(&block.draws.at(*command), h);
		hash3(h, &(F::new(rows.start), F::new(rows.end)));
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
			// Blob IDs only key the memo; comparisons use font content.
			let font = *h.fonts.entry(g.font.data.id()).or_insert_with(|| {
				let mut font = hashers();
				hash3(&mut font, &g.font.data.data());
				finish(font)
			});
			hash3(
				h,
				&(
					2u8,
					g.font.index,
					font,
					&*g.coords,
					g.id,
					F::new(g.size),
					F::new(g.x),
					F::new(g.y),
					g.synthetic_italic,
					g.synthetic_bold,
					g.paint,
				),
			);
		}
		Draw::Rect(r, paint) => {
			hash3(h, &(3u8, hash_rect(*r), *paint));
		}
		Draw::FadedRect {
			rect,
			paint,
			opacity,
		} => {
			hash3(h, &(8u8, hash_rect(*rect), *paint, F::new(*opacity)));
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
						.as_deref()
						.map(|d| (d.edges.map(F::new), d.corners.map(F::new))),
				),
			);
		}
		Draw::Math { math, paint, x, y } => {
			hash3(h, &serde_json::to_string(&math.display).unwrap());
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
			for path in *paths {
				hash3(
					h,
					&(
						path.fill,
						F::new(path.stroke_width),
						serde_json::to_string(path.commands).unwrap(),
					),
				);
			}
			hash3(
				h,
				&(
					7u8,
					*paint,
					F::new(*x),
					F::new(*y),
					F::new(*size),
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
	for block in &snapshot.flattened().blocks {
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
		assert_draw_finite(&draw);
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
		Draw::FadedRect { rect, opacity, .. } => {
			assert_rect4("faded", *rect);
			assert_bounded("faded.opacity", *opacity);
		}
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

/// Whether two fitted edges still fit their extent.
///
/// `fit_edges` scales a pair so that `a + b <= extent`, but each scaled
/// operand rounds first: the sum can miss the extent by a few ulp of the
/// *scaled operands*, not of the extent. Opposite edges that nearly cancel
/// make that rounding arbitrarily large relative to a small extent, so the
/// tolerance carries the operands' magnitude.
pub fn fitted_edges_within(extent: f32, pair: [f32; 2]) -> bool {
	let sum = pair[0] + pair[1];
	if !sum.is_finite() {
		return true;
	}
	let rounding =
		4.0 * f32::EPSILON * (pair[0].abs() + pair[1].abs()).max(extent.abs());
	sum <= extent + rounding
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
pub fn shrunk_limits(seed: u128) -> markview_layout::limits::Limits {
	let pick = |shift: u32, n: u128| (seed >> shift) % n;
	markview_layout::limits::Limits {
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

fn target_record(target: &markview_layout::scene::LinkTarget) -> (u8, String) {
	use markview_layout::scene::LinkTarget::*;
	match target {
		Url(v) => (0, v.to_string()),
		FootnoteDefinition(v) => (1, v.as_str().to_owned()),
		FootnoteReturn(v) => (2, v.as_str().to_owned()),
		Disclosure(v) => (3, v.to_string()),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::panic::{AssertUnwindSafe, catch_unwind};

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
		assert!(shrunk_limits(u128::MAX).highlight_line_bytes > 64);
	}

	/// Every block a real parse produces has a mapping record, including
	/// generated blocks and valid zero-width source positions.
	#[test]
	fn every_parsed_block_has_a_source_map_record() {
		for source in [
			"| a | b |\n| - | - |\n| c | d |\n",
			"Term\n\n: definition\n",
			"> quote\n\n- item\n",
			"<details><summary>s</summary>body</details>\n",
			"<details></details>\n",
			"| | |\n| - | - |\n| | |\n",
			"![ref]\n\n[ref]: /url\n",
			"```rust\ncode\n```\n\n---\n",
			"![img](a.png)\n\n<figure>\n\n![in](b.png)\n\n</figure>\n",
		] {
			assert_source_ranges(&markview_commonmark::parse(source));
		}
	}

	/// A forgotten Markdown mapping record must remain distinguishable from a
	/// generated block or a real zero-width source position.
	#[test]
	fn an_omitted_block_source_record_is_caught() {
		let mut doc = markview_commonmark::parse("First.\n\nSecond.\n");
		assert_source_ranges(&doc);
		let first = doc.document.blocks()[0].id();
		let mut blocks = doc.map.blocks().to_vec();
		blocks.retain(|block| block.block != first);
		doc.map =
			markview_document::SourceMap::new(doc.document.revision(), blocks);
		assert!(
			catch_unwind(AssertUnwindSafe(|| assert_source_ranges(&doc)))
				.is_err()
		);
	}

	/// `Block` equality ignores ids and source ranges, so the prefix oracle must
	/// compare them itself: a prefix that shifted a block would otherwise pass.
	#[test]
	fn a_shifted_prefix_source_range_is_caught() {
		let source: std::sync::Arc<str> =
			std::sync::Arc::from("First.\n\nSecond.\n\nThird.\n");
		let full = markview_commonmark::parse(&*source);
		let prefix = markview_commonmark::parse_prefix(&source, 15)
			.expect("a prefix inside `Second.`");
		assert!(prefix.document.blocks().len() >= 2);
		assert_prefix_consistent(&full, &prefix);
		let first = prefix.document.blocks()[0].id();
		let mut blocks = prefix.map.blocks().to_vec();
		let entry = blocks
			.iter_mut()
			.find(|block| block.block == first)
			.expect("the first block has an entry");
		entry.source = Some(1..entry.source.as_ref().unwrap().end + 1);
		let spliced = MarkdownParse {
			document: prefix.document,
			source: prefix.source,
			map: markview_document::SourceMap::new(
				markview_document::DocumentRevision::new(1),
				blocks,
			),
			dialect: prefix.dialect,
		};
		let caught = catch_unwind(AssertUnwindSafe(|| {
			assert_prefix_consistent(&full, &spliced)
		}));
		assert!(
			caught.is_err(),
			"a prefix with a shifted source range must be caught"
		);
	}

	/// Fitted edges owe the extent only the rounding of the scaled operands.
	/// Both recorded `geometry` findings were opposite edges of `1e38`
	/// magnitude that nearly cancel, so the sum missed a tiny extent by a
	/// fraction of a percent — several ulp of the operands, and far beyond the
	/// pre-fix tolerance of `extent * 1e-4`.
	// The literals keep the decimal form the finding printed, not the shortest
	// form that round-trips, so the case still reads as recorded.
	#[allow(clippy::excessive_precision)]
	#[test]
	fn fitted_edges_allow_cancellation_rounding() {
		let cases = [
			(
				Rect {
					x: 1.0158336341824417e-34,
					y: 1.0158336341824417e-34,
					w: 1.0158336341824417e-34,
					h: 1.0350538014394208e30,
				},
				[
					-3.3961256062618747e38,
					2.4936932569232397e38,
					3.3961513649220713e38,
					1.1707410063014513e-38,
				],
			),
			(
				Rect {
					x: 7.275957180502557e-12,
					y: 9.403954246058914e-38,
					w: 133955584.0,
					h: 8547335680.0,
				},
				[
					3.3961513649220713e38,
					1.701415688262517e38,
					2.9176167113455474e38,
					-1.7014118346046923e38,
				],
			),
		];
		let old = |extent: f32, pair: [f32; 2]| {
			pair[0] + pair[1] <= extent + extent * 1e-4 + 1e-3
		};
		for (rect, edges) in cases {
			let fitted = markview_layout::scene::fit_edges(rect, edges);
			let vertical = [fitted[0], fitted[2]];
			let horizontal = [fitted[1], fitted[3]];
			assert!(
				fitted_edges_within(rect.h, vertical),
				"{vertical:?} must fit {rect:?}"
			);
			assert!(
				fitted_edges_within(rect.w, horizontal),
				"{horizontal:?} must fit {rect:?}"
			);
			assert!(
				!old(rect.h, vertical) || !old(rect.w, horizontal),
				"the recorded finding must fail the pre-fix tolerance"
			);
		}
	}

	/// The tolerance must not swallow a fitter that failed to scale at all.
	#[test]
	fn unscaled_edges_are_still_a_finding() {
		assert!(!fitted_edges_within(100.0, [1e6, 1e6]));
		assert!(fitted_edges_within(100.0, [40.0, 60.0]));
	}
}
