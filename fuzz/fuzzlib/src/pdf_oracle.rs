//! Structural oracles over a PDF Markview produced (G6, Tier 3).
//!
//! The `pdf` target's Tier-1 oracle — "bytes came back, nothing panicked" —
//! cannot see a page the exporter dropped, a link annotation that points at
//! a page that is not there, or a page whose content stream lost the text the
//! document carries. These oracles re-parse the export with `lopdf`, the same
//! way a viewer would, and compare the structure against the document, the
//! pagination, and the link rectangles the export was built from.
//!
//! Everything here fails loudly. An oracle the exporter cannot satisfy is a
//! finding, not an environment problem: the export runs on the committed test
//! fonts against a fixed print stylesheet, so it is reproducible.

use std::collections::{HashMap, HashSet};

use lopdf::{Document, Object, ObjectId};
use markview_core::{
	layout::LayoutSnapshot,
	paginate::{PT_PER_PX, PageGeometry, PageItem, Pagination},
	scene::Rect,
};

/// How far a link annotation's rectangle may drift from the link rectangle
/// the layout recorded. Both are page points by the time they are compared;
/// only the float round trip through the exporter's transform can differ.
const RECT_TOLERANCE: f32 = 0.5;

/// The structural view of one exported PDF.
pub struct Structure {
	pdf: Document,
	/// The pages in reading order, as `lopdf` resolves the page tree.
	pages: Vec<ObjectId>,
	/// Every object reachable from the trailer, by following references
	/// through dictionaries, arrays and stream dictionaries.
	reachable: HashSet<ObjectId>,
}

impl Structure {
	/// Parses and walks one export.
	pub fn load(bytes: &[u8]) -> Result<Self, String> {
		let pdf = Document::load_mem(bytes)
			.map_err(|e| format!("the export does not parse as a PDF: {e}"))?;
		assert!(
			!pdf.objects.is_empty(),
			"the export parses but holds no objects"
		);
		let pages = pdf.get_pages().into_values().collect::<Vec<_>>();
		let reachable = reachable_from_root(&pdf);
		Ok(Self {
			pdf,
			pages,
			reachable,
		})
	}

	pub fn page_count(&self) -> usize {
		self.pages.len()
	}

	/// 1. Well-formedness: the trailer names a catalog, the catalog names a
	///    page tree whose `/Count` agrees with the pages reached, every page
	///    object is reachable, and no reference dangles.
	pub fn assert_well_formed(&self, pages_expected: usize) {
		let catalog_id = self
			.pdf
			.trailer
			.get(b"Root")
			.and_then(Object::as_reference)
			.unwrap_or_else(|e| {
				panic!(
					"the trailer has no usable /Root ({e}): {:?}",
					self.pdf.trailer
				)
			});
		let catalog = self
			.pdf
			.get_dictionary(catalog_id)
			.expect("the trailer /Root resolves");
		assert_eq!(
			catalog.get(b"Type").and_then(Object::as_name).ok(),
			Some(b"Catalog".as_slice()),
			"the trailer /Root is not a /Catalog"
		);
		let tree_id = catalog
			.get(b"Pages")
			.and_then(Object::as_reference)
			.expect("the catalog names a page tree");
		let tree = self
			.pdf
			.get_dictionary(tree_id)
			.expect("the page tree resolves");
		assert_eq!(
			tree.get(b"Type").and_then(Object::as_name).ok(),
			Some(b"Pages".as_slice()),
			"the catalog's /Pages is not a /Pages node"
		);
		let declared = tree
			.get(b"Count")
			.and_then(Object::as_i64)
			.expect("the page tree declares a /Count");
		assert_eq!(
			declared as usize, pages_expected,
			"the page tree declares {declared} pages and the paginator made \
			 {pages_expected}"
		);
		let page_objects = self
			.pdf
			.objects
			.iter()
			.filter(|(id, object)| {
				self.reachable.contains(id)
					&& object
						.as_dict()
						.map(|d| {
							d.get(b"Type").and_then(Object::as_name).ok()
								== Some(b"Page".as_slice())
						})
						.unwrap_or(false)
			})
			.count();
		assert_eq!(
			page_objects, pages_expected,
			"{page_objects} /Page objects are reachable from /Root and the \
			 paginator made {pages_expected}"
		);
		let dangling = self
			.pdf
			.objects
			.values()
			.flat_map(references)
			.filter(|id| *id != (0, 0) && !self.pdf.objects.contains_key(id))
			.collect::<Vec<_>>();
		assert!(
			dangling.is_empty(),
			"{} references point at objects that are not in the file: {:?}",
			dangling.len(),
			&dangling[..dangling.len().min(8)]
		);
	}

	/// 2. The number of pages the exporter wrote equals what
	///    `markview_core::paginate` reported for the same document.
	pub fn assert_page_count(&self, pagination: &Pagination) {
		assert_eq!(
			self.pages.len(),
			pagination.pages.len(),
			"the exporter wrote {} pages and the paginator made {}",
			self.pages.len(),
			pagination.pages.len()
		);
	}

	/// 4. Text round trip: the document's characters reach the pages.
	///
	/// The check is on the *multiset of characters*. `extract_text` rebuilds
	/// a page from glyph positions and re-segments it by font run, so a style
	/// change splits a word, punctuation starts its own chunk, and
	/// whitespace is inferred from advances; none of that loses characters.
	/// A dropped or truncated run — the symptom a pagination disagreement
	/// produces — does.
	///
	/// A node is only compared when every character in it is one the pinned
	/// test faces can draw. Three things otherwise make the page carry fewer
	/// characters than the layout, and none is text the exporter dropped:
	///
	/// - A **ligature** sets several characters with one glyph, so the page
	///   names fewer characters than the layout has.
	/// - An **undrawable** character — a C0/C1 control character, or one no
	///   pinned face covers, such as Arabic in a CJK subset — has no glyph.
	/// - The character **directly after** an undrawable one goes with it:
	///   `lopdf` 0.45 decodes a `/ToUnicode` CMap by greedy variable-length
	///   matching and never consults its `codespacerange`, so a code with no
	///   `bfchar` entry runs on and eats the next code. The export itself is
	///   correct; the fault is in the extractor, so it is an oracle limit
	///   rather than a defect to report.
	///
	/// The first is a real transformation of the text; the other two are
	/// oracle limits. Excluding a node that mixes them keeps the check strict
	/// for ordinary prose — the pages of a document that dropped a run still
	/// fail — while never inventing a defect the export does not have.
	pub fn assert_text_round_trip(&self, snapshot: &LayoutSnapshot) {
		let mut available: HashMap<char, usize> = HashMap::new();
		for index in 0..self.pages.len() {
			let text = self
				.pdf
				.extract_text(&[(index + 1) as u32])
				.unwrap_or_else(|e| {
					panic!("the text of page {index} does not extract: {e}")
				});
			for character in text.chars() {
				if !character.is_whitespace() {
					*available.entry(character).or_insert(0) += 1;
				}
			}
		}
		let mut owed: HashMap<char, usize> = HashMap::new();
		for block in &snapshot.blocks {
			for node in &block.layout.text {
				if !node.text.chars().all(is_comparable) {
					continue;
				}
				// An atomic cluster is a formula or a drawn image: one box
				// whose text is the LaTeX or the alt text it copies, not a
				// glyph the page carries. The math path draws it separately.
				if node.clusters.iter().any(|cluster| cluster.atomic) {
					continue;
				}
				// One glyph per cluster, so a ligature can account for the
				// difference between the characters and the clusters. Only
				// the characters a cluster cannot cover are owed.
				let mut merges = node
					.text
					.chars()
					.filter(|c| !c.is_whitespace())
					.count()
					.saturating_sub(node.clusters.len());
				for character in node.text.chars() {
					if character.is_whitespace() {
						continue;
					}
					if merges > 0 {
						merges -= 1;
						continue;
					}
					*owed.entry(character).or_insert(0) += 1;
				}
			}
		}
		let mut missing = Vec::new();
		for (character, count) in owed {
			let have = available.get(&character).copied().unwrap_or(0);
			if have < count {
				missing.push(format!("{character:?} x{}", count - have));
			}
		}
		missing.sort();
		assert!(
			missing.is_empty(),
			"{} characters in the document do not survive the export's \
			 text extraction: {:?}",
			missing.len(),
			&missing[..missing.len().min(12)]
		);
	}

	/// 6. Degenerate geometry: every page is a sane rectangle and its content
	///    stream decodes.
	pub fn assert_page_geometry(&self) {
		for (index, id) in self.pages.iter().enumerate() {
			let dict = self.pdf.get_dictionary(*id).expect("the page resolves");
			let media = dict
				.get(b"MediaBox")
				.and_then(Object::as_array)
				.expect("the page has a /MediaBox")
				.iter()
				.map(|v| v.as_float().expect("the /MediaBox is numeric"))
				.collect::<Vec<_>>();
			assert_eq!(media.len(), 4, "the /MediaBox is not four numbers");
			let (w, h) =
				((media[2] - media[0]).abs(), (media[3] - media[1]).abs());
			assert!(
				w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0,
				"page {index} has a degenerate /MediaBox {media:?}"
			);
			assert!(
				w < 1e6 && h < 1e6,
				"page {index} has an implausible /MediaBox {media:?}"
			);
			let content = self.pdf.get_page_content(*id);
			assert!(
				content.len() < 64 * 1024 * 1024,
				"page {index} has an implausible content stream"
			);
		}
	}

	/// A page the paginator filled must not export as an empty content
	/// stream: that is the blank page a page-count disagreement shows up as.
	pub fn assert_pages_have_content(&self, pagination: &Pagination) {
		for (index, id) in self.pages.iter().enumerate() {
			let items = pagination.pages.get(index).map_or(0, Vec::len);
			if items == 0 {
				continue;
			}
			assert!(
				!self.pdf.get_page_content(*id).is_empty(),
				"page {index} carries {items} paginated items but its content \
				 stream is empty"
			);
		}
	}

	/// 3. Links: every link the layout resolved became an annotation whose
	///    destination matches, every annotation covers a real area, and every
	///    in-document destination lands on a page that exists.
	pub fn assert_links(
		&self,
		snapshot: &LayoutSnapshot,
		geometry: &PageGeometry,
		pagination: &Pagination,
	) {
		let annotations = self.link_annotations();
		let page_count = self.pages.len();
		let mut by_uri: HashMap<&str, Vec<&LinkAnnotation>> = HashMap::new();
		for annotation in &annotations {
			let [x0, y0, x1, y1] = annotation.rect;
			assert!(
				x1 - x0 > 0.0 && y1 - y0 > 0.0,
				"a link annotation on page {} has a zero-area /Rect {:?}",
				annotation.page,
				annotation.rect
			);
			if let Some(uri) = &annotation.uri {
				by_uri.entry(uri.as_str()).or_default().push(annotation);
			}
		}
		// Every in-document link the layout resolved must have an annotation
		// whose destination names the page the paginator put that anchor on.
		// An anchor no link points at gets no annotation, which is correct.
		for block in &snapshot.blocks {
			for link in &block.layout.links {
				let Some(anchor) = link.url.strip_prefix('#') else {
					continue;
				};
				let anchor = percent_encoding::percent_decode_str(anchor)
					.decode_utf8_lossy();
				let Some((page, _)) = pagination.anchors.get(anchor.as_ref())
				else {
					// The paginator never placed this anchor, so the export
					// has no destination to write; `link::annotation`
					// returns `None` for exactly this case.
					continue;
				};
				let found = annotations.iter().any(
					|a| matches!(a.target, LinkTarget::Internal(dest) if dest == *page),
				);
				assert!(
					found,
					"the layout resolved the in-document link {anchor:?} to \
					 page {page} but no annotation names that page"
				);
			}
		}
		for annotation in &annotations {
			if let LinkTarget::Internal(page) = annotation.target {
				assert!(
					page < page_count,
					"a link annotation points at page {page} and the export \
					 has {page_count}"
				);
			}
		}
		// Each link rectangle the layout recorded must be covered by an
		// annotation with the same URI, on the page its item landed on.
		for (bi, block) in snapshot.blocks.iter().enumerate() {
			for (li, link) in block.layout.links.iter().enumerate() {
				// An in-document anchor carries a `/Dest`, not a `/URI`; its
				// destination was checked above, and its rectangle is checked
				// by the annotation's own geometry below.
				if link.url.is_empty() || link.url.starts_with('#') {
					continue;
				}
				let Some(item) = page_item(pagination, bi, link.rect) else {
					// The paginator dropped the link's band, so the export
					// has nothing to annotate; the page-count and text
					// oracles would already have complained if that mattered.
					continue;
				};
				let expected = page_rect(link.rect, item, geometry);
				let candidates = by_uri.get(&*link.url).unwrap_or_else(|| {
					panic!(
						"the layout resolved the link {url:?} (block {bi} \
							 link {li}) but the export has no annotation with \
							 that URI",
						url = link.url
					)
				});
				let best = candidates
					.iter()
					.map(|a| {
						a.rect
							.iter()
							.zip(expected.iter())
							.map(|(a, e)| (a - e).abs())
							.fold(0.0_f32, f32::max)
					})
					.fold(f32::INFINITY, f32::min);
				assert!(
					best <= RECT_TOLERANCE,
					"the annotation for {:?} (block {bi} link {li}) is off by \
					 {best} points: expected {expected:?}, found {:?}",
					link.url,
					candidates.iter().map(|a| a.rect).collect::<Vec<_>>()
				);
			}
		}
	}

	/// The rectangles and URIs of every link annotation, with the page index
	/// each one sits on.
	pub fn link_annotations(&self) -> Vec<LinkAnnotation> {
		let mut out = Vec::new();
		for (page_index, id) in self.pages.iter().enumerate() {
			let Some(Ok(dict)) = self.pdf.objects.get(id).map(Object::as_dict)
			else {
				continue;
			};
			let Ok(annots) = dict.get(b"Annots").and_then(Object::as_array)
			else {
				continue;
			};
			for annot in annots {
				let annot_id = annot
					.as_reference()
					.expect("a page's /Annots entries are references");
				let annot = self
					.pdf
					.get_dictionary(annot_id)
					.expect("the annotation resolves");
				assert_eq!(
					annot.get(b"Subtype").and_then(Object::as_name).ok(),
					Some(b"Link".as_slice()),
					"page {page_index} carries a non-link annotation"
				);
				let rect = annot
					.get(b"Rect")
					.and_then(Object::as_array)
					.expect("the annotation has a /Rect")
					.iter()
					.map(|v| v.as_float().expect("the /Rect is numeric"))
					.collect::<Vec<_>>();
				assert_eq!(rect.len(), 4, "the /Rect is not four numbers");
				let (x0, x1) = (rect[0].min(rect[2]), rect[0].max(rect[2]));
				let (y0, y1) = (rect[1].min(rect[3]), rect[1].max(rect[3]));
				let (uri, target) = if let Ok(Ok(action)) =
					annot.get(b"A").map(Object::as_dict)
				{
					(
						Some(
							action
								.get(b"URI")
								.and_then(Object::as_str)
								.map(<[u8]>::to_vec)
								.unwrap_or_else(|e| {
									panic!(
										"a link action has no /URI \
											 string ({e}): {action:?}"
									)
								}),
						),
						LinkTarget::External,
					)
				} else if let Ok(dest) = annot.get(b"Dest") {
					(None, LinkTarget::Internal(self.destination_page(dest)))
				} else {
					panic!("a link annotation has neither /A nor /Dest")
				};
				let uri = uri.map(|bytes| {
					String::from_utf8(bytes).unwrap_or_else(|e| {
						panic!("a link URI is not UTF-8: {e}")
					})
				});
				out.push(LinkAnnotation {
					page: page_index,
					rect: [x0, y0, x1, y1],
					uri,
					target,
				});
			}
		}
		out
	}

	/// The page index a `/Dest` names. `krilla` writes an internal
	/// destination as an indirect array (`4 0 R` holding `[16 0 R /XYZ …]`)
	/// rather than inlining it, so the reference has to be dereferenced
	/// before the array's first element names the page.
	fn destination_page(&self, dest: &Object) -> usize {
		fn array_of<'a>(pdf: &'a Document, object: &'a Object) -> &'a Object {
			match object {
				Object::Reference(id) => {
					pdf.objects.get(id).unwrap_or_else(|| {
						panic!("a /Dest reference {id:?} dangles")
					})
				}
				other => other,
			}
		}
		let resolved = array_of(&self.pdf, dest);
		let page = match resolved {
			Object::Array(array) => array
				.first()
				.expect("a /Dest array names a page")
				.as_reference()
				.unwrap_or_else(|e| {
					panic!(
						"a /Dest array's first element is not a page reference ({e}): {array:?}"
					)
				}),
			Object::Reference(id) => *id,
			other => panic!(
				"a /Dest resolves to neither an array nor a reference: {other:?}"
			),
		};
		self.pages
			.iter()
			.position(|id| *id == page)
			.unwrap_or_else(|| {
				panic!(
					"a /Dest names {page:?}, which is not one of the export's {} \
				 pages {:?}",
					self.pages.len(),
					self.pages
				)
			})
	}
}

/// Whether any face the harness pins could draw `character`. A C0/C1 control
/// character is not a glyph at all, and a character no pinned face covers
/// shapes to `.notdef` that `extract_text` reports as U+FFFD; neither is text
/// the exporter dropped.
fn is_comparable(character: char) -> bool {
	character != char::REPLACEMENT_CHARACTER
		&& !character.is_control()
		// A zero-width formatting character draws nothing, so the page is
		// not expected to carry it. U+FEFF reaches the layout from a byte
		// order mark in the middle of a document.
		&& !matches!(character, '\u{feff}' | '\u{200b}' | '\u{200c}' | '\u{200d}')
		&& covered(character)
}

/// Whether any committed test face maps `character` to a real glyph. Glyph 0
/// is `.notdef`.
fn covered(character: char) -> bool {
	use std::sync::OnceLock;
	static COVERAGE: OnceLock<HashSet<u32>> = OnceLock::new();
	COVERAGE
		.get_or_init(|| {
			let mut covered = HashSet::new();
			let directories = crate::pipeline::pinned_fonts().directories;
			let Some(directory) = directories.first() else {
				return covered;
			};
			let Ok(entries) = std::fs::read_dir(directory) else {
				return covered;
			};
			for entry in entries.flatten() {
				let Ok(bytes) = std::fs::read(entry.path()) else {
					continue;
				};
				let Some(font) = swash::FontRef::from_index(&bytes, 0) else {
					continue;
				};
				let charmap = font.charmap();
				// The subset test faces cover ASCII plus their own ranges; a
				// sweep over the BMP is cheap next to one export.
				for code in 0u32..=0xffff {
					if let Some(c) = char::from_u32(code)
						&& charmap.map(c) != 0
					{
						covered.insert(code);
					}
				}
			}
			covered
		})
		.contains(&(character as u32))
}

/// The page item `rect` (block-local) falls in.
fn page_item(
	pagination: &Pagination,
	block: usize,
	rect: Rect,
) -> Option<&PageItem> {
	pagination.pages.iter().flatten().find(|item| {
		item.block == block
			&& rect.y + rect.h > item.top
			&& rect.y < item.bottom
	})
}

/// Where a link annotation sends the reader.
#[derive(Clone, Copy, Debug)]
pub enum LinkTarget {
	External,
	Internal(usize),
}

/// One link annotation read back out of the export, in page points with the
/// origin at the page's bottom-left.
#[derive(Clone, Debug)]
pub struct LinkAnnotation {
	pub page: usize,
	/// `[x0, y0, x1, y1]`, normalised so `x0 <= x1` and `y0 <= y1`.
	pub rect: [f32; 4],
	pub uri: Option<String>,
	pub target: LinkTarget,
}

/// Maps a block-local link rectangle onto the page, as the painter does: the
/// item's scale shrinks it, the item's page-local y places it, and PDF y runs
/// up from the page's bottom edge while the layout's runs down from the text
/// area's top.
fn page_rect(rect: Rect, item: &PageItem, geometry: &PageGeometry) -> [f32; 4] {
	let [left, top, width, text_height] = geometry.text_pt();
	let clipped_top = rect.y.max(item.top);
	let clipped_bottom = (rect.y + rect.h).min(item.bottom);
	let scale = item.scale * PT_PER_PX;
	let y = |block_local: f32| {
		let page_local = item.y + (block_local - item.top) * item.scale;
		top + text_height - page_local * PT_PER_PX
	};
	// The painter clamps the annotation to the text area: a long unbreakable
	// URL overflows the measure, and an annotation reaching into the margin
	// would not be clickable where it was drawn. The expectation follows the
	// same clamp.
	[
		(left + rect.x * scale).max(left),
		y(clipped_bottom).max(top),
		(left + (rect.x + rect.w) * scale).min(left + width),
		y(clipped_top).min(top + text_height),
	]
}

/// Every object reference an object holds, without following it.
fn references(object: &Object) -> Vec<ObjectId> {
	fn walk(object: &Object, out: &mut Vec<ObjectId>) {
		match object {
			Object::Reference(id) => out.push(*id),
			Object::Array(array) => array.iter().for_each(|v| walk(v, out)),
			Object::Dictionary(dict) => {
				dict.iter().for_each(|(_, v)| walk(v, out));
			}
			Object::Stream(stream) => {
				stream.dict.iter().for_each(|(_, v)| walk(v, out));
			}
			_ => {}
		}
	}
	let mut out = Vec::new();
	walk(object, &mut out);
	out
}

/// The object ids reachable from the trailer's `/Root`.
fn reachable_from_root(pdf: &Document) -> HashSet<ObjectId> {
	let mut seen = HashSet::new();
	let mut stack = pdf
		.trailer
		.iter()
		.map(|(_, value)| value.clone())
		.collect::<Vec<_>>();
	while let Some(object) = stack.pop() {
		match object {
			Object::Reference(id) => {
				if id != (0, 0)
					&& seen.insert(id)
					&& let Some(value) = pdf.objects.get(&id)
				{
					stack.push(value.clone());
				}
			}
			Object::Array(array) => stack.extend(array),
			Object::Dictionary(dict) => {
				stack.extend(dict.into_iter().map(|(_, v)| v));
			}
			Object::Stream(stream) => {
				stack.extend(stream.dict.into_iter().map(|(_, v)| v));
			}
			_ => {}
		}
	}
	seen
}

/// The text a viewer would read out of the export, page by page, for a
/// finding's record.
pub fn extracted_text(bytes: &[u8]) -> Result<String, String> {
	use std::fmt::Write as _;
	let pdf = Document::load_mem(bytes)
		.map_err(|e| format!("the export does not parse: {e}"))?;
	let mut out = String::new();
	for number in pdf.get_pages().keys() {
		let text = pdf
			.extract_text(&[*number])
			.map_err(|e| format!("page {number} does not extract: {e}"))?;
		let _ = writeln!(out, "--- page {number} ---\n{text}");
	}
	Ok(out)
}
