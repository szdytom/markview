//! Direct DOM-to-document lowering.
#[cfg(test)]
mod proptests;
use crate::{Error, Profile};
use dom_smoothie::dom_query::NodeRef;
use markview_document::{
	Anchors, Block, BlockId, BlockKind, CellAlign, CellKind, DefinitionItem,
	Document, DocumentBuilder, DocumentLimit, DocumentLimits, DocumentRevision,
	ImageSpec, Inline, InlineKind, LineBreak, Link, LinkOccurrenceId, ListItem,
	RichText, Table, TableCell, TableColumn, TableGroup, TableGroupKind,
	TableRow, TextStyle, image, plain_text,
};

fn document_limits(profile: &Profile) -> DocumentLimits {
	DocumentLimits {
		max_blocks: profile.max_blocks,
		max_depth: profile.max_semantic_depth,
		max_text_bytes: profile.max_text_bytes,
		max_table_cells: profile.max_table_cells,
	}
}

fn limit_error(limit: DocumentLimit) -> Error {
	match limit {
		DocumentLimit::Blocks { blocks, limit } => {
			Error::TooManyBlocks { blocks, limit }
		}
		DocumentLimit::Depth { .. } => Error::DepthExceeded,
		DocumentLimit::Text { bytes, limit } => {
			Error::TooManyText { bytes, limit }
		}
		DocumentLimit::Table { .. } => Error::TableTooLarge,
	}
}

/// One link occurrence allocated from the lowering's own counter.
fn allocated_link(url: impl Into<std::sync::Arc<str>>, counter: usize) -> Link {
	let id = u32::try_from(counter.saturating_add(1))
		.ok()
		.and_then(LinkOccurrenceId::new)
		.expect("the link counter exceeds the occurrence space");
	Link::new(url, id)
}

pub(crate) struct Lower<'a> {
	root: NodeRef<'a>,
	base: url::Url,
	profile: Profile,
	builder: DocumentBuilder,
	anchors: Anchors,
	explicit_ids: std::collections::HashSet<String>,
	next_link: usize,
	/// The run state an enclosing inline wrapper passes to deferred blocks.
	inherited: RunStyle,
	title_block: Option<BlockId>,
	blocks: usize,
	text_bytes: usize,
	table_rows: usize,
	table_cells: usize,
	diagnostics: Vec<String>,
}

impl<'a> Lower<'a> {
	pub(crate) fn new(
		extracted: &'a dom_smoothie::Extracted,
		base: url::Url,
		profile: Profile,
	) -> Self {
		let base = effective_base(extracted, base);
		Self {
			root: extracted.root(),
			base,
			builder: DocumentBuilder::new(DocumentRevision::fresh())
				.with_limits(document_limits(&profile)),
			profile,
			anchors: Anchors::default(),
			explicit_ids: Default::default(),
			next_link: 0,
			inherited: RunStyle::default(),
			title_block: None,
			blocks: 0,
			text_bytes: 0,
			table_rows: 0,
			table_cells: 0,
			diagnostics: Vec::new(),
		}
	}

	pub(crate) fn run(
		mut self,
		title: &str,
		url: &str,
	) -> Result<(Document, Vec<String>), Error> {
		let root = self.root;
		for node in root.descendants_it() {
			if let Some(id) = node.attr("id") {
				let id = id.to_string();
				if !id.trim().is_empty() {
					self.explicit_ids.insert(id.clone());
					self.anchors.reserve(id);
				}
			}
		}
		let mut blocks = Vec::new();
		self.blocks(&root, 0, &mut blocks)?;
		// Only a leading title heading replaces the synthetic document title;
		// a marked heading after any body block is content.
		let title_heading = self.title_block.is_some_and(|id| {
			blocks.first().is_some_and(|block| block.id() == id)
		});
		if !has_readable_content(&blocks) {
			return Err(Error::NoReadableContent);
		}
		let document = with_title_and_source(
			blocks,
			self.builder,
			title,
			url,
			title_heading,
			self.next_link,
		)?;
		Ok((document, self.diagnostics))
	}

	fn make(
		&mut self,
		anchors: Vec<String>,
		kind: BlockKind,
	) -> Result<Block, Error> {
		self.blocks += 1;
		if self.blocks > self.profile.max_blocks {
			return Err(Error::TooManyBlocks {
				blocks: self.blocks,
				limit: self.profile.max_blocks,
			});
		}
		Ok(self.builder.block(anchors, kind))
	}

	fn count_text(&mut self, text: &str) -> Result<(), Error> {
		self.text_bytes = self.text_bytes.saturating_add(text.len());
		if self.text_bytes > self.profile.max_text_bytes {
			return Err(Error::TooManyText {
				bytes: self.text_bytes,
				limit: self.profile.max_text_bytes,
			});
		}
		Ok(())
	}

	fn blocks(
		&mut self,
		parent: &NodeRef<'a>,
		depth: usize,
		out: &mut Vec<Block>,
	) -> Result<(), Error> {
		if depth > self.profile.max_semantic_depth {
			return Err(Error::DepthExceeded);
		}
		if presentation_only(parent) {
			return Ok(());
		}
		let style = self.inherited.clone();
		let mut pending = RichText::new();
		for child in parent.children_it(false) {
			if child.is_text() {
				let text = child.text().to_string();
				self.append_text(&mut pending, &text, &style)?;
				continue;
			}
			if !child.is_element() {
				continue;
			}
			let name = local_name(&child);
			if name == "br" {
				self.append_inline(
					&mut pending,
					InlineKind::LineBreak(LineBreak::Forced),
					&style,
				);
				continue;
			}
			if is_block_element(&name) || has_block_children(&child) {
				self.flush(&mut pending, out)?;
				self.lower_block(&child, &name, depth, out)?;
			} else {
				self.inline_element(
					&child,
					&name,
					&style,
					&mut pending,
					depth,
				)?;
			}
		}
		self.flush(&mut pending, out)
	}

	fn flush(
		&mut self,
		pending: &mut RichText,
		out: &mut Vec<Block>,
	) -> Result<(), Error> {
		if pending.is_empty() {
			return Ok(());
		}
		let text = trim_rich(std::mem::take(pending));
		if !text.is_empty() {
			let block = self.make(Vec::new(), BlockKind::Paragraph(text))?;
			out.push(block);
		}
		Ok(())
	}

	#[cfg_attr(
		any(),
		markview::allow_complexity(
			reason = "Keep the exhaustive HTML block dispatch in one ordered walk; each element owns its semantic lowering."
		)
	)]
	fn lower_block(
		&mut self,
		node: &NodeRef<'a>,
		name: &str,
		depth: usize,
		out: &mut Vec<Block>,
	) -> Result<(), Error> {
		if depth > self.profile.max_semantic_depth {
			return Err(Error::DepthExceeded);
		}
		if presentation_only(node) {
			return Ok(());
		}
		let style = self.inherited.clone();
		let start = out.len();
		let explicit = self.block_anchor(node);
		if !style.code
			&& (name == "math"
				|| node.has_class("math")
				|| node.attr("data-mv-math").is_some())
		{
			if let Some(math) = math_from(node, name) {
				let kind = BlockKind::Paragraph(vec![Inline {
					link: style.link.clone(),
					kind: InlineKind::Math {
						latex: math.0,
						display: math.1,
					},
					style: style.visual.clone(),
				}]);
				let block = self.make(explicit.into_iter().collect(), kind)?;
				out.push(block);
				return Ok(());
			}
			let fallback = node.text().to_string();
			if !fallback.trim().is_empty() {
				self.diagnostics
					.push("MathML without TeX annotation kept as text".into());
				let kind = BlockKind::Paragraph(vec![Inline {
					link: style.link.clone(),
					kind: InlineKind::Text(fallback),
					style: style.visual.clone(),
				}]);
				let block = self.make(explicit.into_iter().collect(), kind)?;
				out.push(block);
				return Ok(());
			}
		}
		if matches!(name, "h1" | "h2" | "h3" | "h4" | "h5" | "h6") {
			return self.heading(node, name, explicit, depth, out);
		}
		let kind = match name {
			"p" | "address" | "figcaption" => {
				let text = self.inlines(node, &style, depth + 1)?;
				BlockKind::Paragraph(trim_rich(text))
			}
			"hr" => BlockKind::Rule,
			"pre" => self.code_block(node)?,
			"blockquote" => {
				let mut blocks = Vec::new();
				self.blocks(node, depth + 1, &mut blocks)?;
				BlockKind::Quote {
					callout: None,
					blocks,
				}
			}
			"ul" | "ol" => self.list(node, name == "ol", depth)?,
			"table" => self.table(node, depth)?,
			"dl" => self.definition_list(node, depth)?,
			"details" => self.details(node, depth)?,
			"figure" => self.figure(node, depth)?,
			_ => {
				// Transparent containers keep the blocks of their children.
				let link = (name == "a")
					.then(|| node.attr("href"))
					.flatten()
					.and_then(|href| self.link_url(&href))
					.map(|url| {
						let link = allocated_link(url, self.next_link);
						self.next_link += 1;
						link
					});
				let result = self.blocks(node, depth + 1, out);
				if let Some(link) = link {
					for block in &mut out[start..] {
						apply_link(block, &link);
					}
				}
				return result.and_then(|_| {
					if let Some(id) = explicit {
						if out.len() == start {
							out.push(self.make(
								vec![id],
								BlockKind::Paragraph(Vec::new()),
							)?);
						} else {
							self.attach_anchor(&mut out[start], id);
						}
					}
					Ok(())
				});
			}
		};
		let block = self.make(explicit.into_iter().collect(), kind)?;
		out.push(block);
		Ok(())
	}

	/// Lowers a heading, keeping any block content nested inside it (a
	/// malformed page can leave a heading open over the rest of the article)
	/// as blocks after the heading instead of flattening it into the text.
	fn heading(
		&mut self,
		node: &NodeRef<'a>,
		name: &str,
		explicit: Option<String>,
		depth: usize,
		out: &mut Vec<Block>,
	) -> Result<(), Error> {
		if depth > self.profile.max_semantic_depth {
			return Err(Error::DepthExceeded);
		}
		let title = node.attr("data-mv-title").is_some();
		let level = if title {
			1
		} else {
			name[1..].parse::<u8>().expect("hN")
		};
		let (text, nested) = self.heading_parts(node, depth)?;
		let text = trim_rich(text);
		// A heading's explicit `id` is the heading's own anchor; keeping it
		// as a block anchor too would declare the same fragment twice.
		let anchor = match explicit {
			Some(id) => id,
			None => self.anchors.unique(&plain_text(&text)),
		};
		let block = self.make(
			Vec::new(),
			BlockKind::Heading {
				level,
				text,
				anchor,
			},
		)?;
		if title && self.title_block.is_none() {
			self.title_block = Some(block.id());
		}
		out.push(block);
		for (child, style) in nested {
			// The wrapper's link and style enclose the deferred blocks too.
			let outer = std::mem::replace(&mut self.inherited, style);
			let child_name = local_name(&child);
			let result = self.lower_block(&child, &child_name, depth + 1, out);
			self.inherited = outer;
			result?;
		}
		Ok(())
	}

	/// Splits a heading's children into its inline text and the block
	/// elements nested inside it.
	fn heading_parts(
		&mut self,
		node: &NodeRef<'a>,
		depth: usize,
	) -> Result<(RichText, Vec<(NodeRef<'a>, RunStyle)>), Error> {
		let mut text = RichText::new();
		let mut nested = Vec::new();
		let style = self.inherited.clone();
		self.heading_children(node, &style, depth, &mut text, &mut nested)?;
		Ok((text, nested))
	}

	/// Walks one heading level: inline content joins the heading text, with
	/// the style of any enclosing inline wrapper, and block children become
	/// blocks after the heading.
	fn heading_children(
		&mut self,
		node: &NodeRef<'a>,
		style: &RunStyle,
		depth: usize,
		text: &mut RichText,
		nested: &mut Vec<(NodeRef<'a>, RunStyle)>,
	) -> Result<(), Error> {
		if depth > self.profile.max_semantic_depth {
			return Err(Error::DepthExceeded);
		}
		for child in node.children_it(false) {
			if child.is_text() {
				self.append_text(text, child.text().as_ref(), style)?;
			} else if child.is_element() {
				if presentation_only(&child) {
					continue;
				}
				let name = local_name(&child);
				if is_block_element(&name) {
					nested.push((child, style.clone()));
				} else if has_block_children(&child)
					&& !delegated_inline(&child, &name)
				{
					// A mixed inline wrapper: its text is the heading's,
					// its blocks belong to the body.
					self.inline_anchor(&child, text, style);
					let style = self.run_style(&child, &name, style);
					self.heading_children(
						&child,
						&style,
						depth + 1,
						text,
						nested,
					)?;
				} else {
					self.inline_element(&child, &name, style, text, depth + 1)?;
				}
			}
		}
		Ok(())
	}

	/// Records an inline element's `id` as an invisible fragment target.
	fn inline_anchor(
		&mut self,
		node: &NodeRef<'a>,
		out: &mut RichText,
		style: &RunStyle,
	) {
		let Some(id) = node.attr("id") else {
			return;
		};
		let id = id.to_string();
		if id.trim().is_empty() {
			return;
		}
		if self.explicit_ids.remove(&id) {
			self.append_inline(out, InlineKind::Anchor(id), style);
		} else if !self.anchors.contains(&id) {
			self.anchors.reserve(id.clone());
			self.append_inline(out, InlineKind::Anchor(id), style);
		}
	}

	/// The run state an inline element passes to its children.
	fn run_style(
		&mut self,
		node: &NodeRef<'a>,
		name: &str,
		style: &RunStyle,
	) -> RunStyle {
		let mut style = style.clone();
		match name {
			"strong" | "b" => style.bold = true,
			"em" | "i" | "cite" | "dfn" | "var" => style.italic = true,
			"del" | "s" | "strike" => style.strike = true,
			"mark" => style.mark = true,
			"code" | "kbd" | "samp" | "tt" => style.code = true,
			"sup" => style.superscript = true,
			"sub" => style.subscript = true,
			"a" => {
				if let Some(href) = node.attr("href")
					&& let Some(url) = self.link_url(&href)
				{
					style.link = Some(allocated_link(url, self.next_link));
					self.next_link += 1;
				}
			}
			_ => {}
		}
		style
	}

	fn attach_anchor(&self, block: &mut Block, id: String) {
		block.add_anchor(id);
	}

	fn block_anchor(&mut self, node: &NodeRef<'a>) -> Option<String> {
		let id = node.attr("id")?.to_string();
		if id.trim().is_empty() {
			return None;
		}
		if self.explicit_ids.remove(&id) {
			return Some(id);
		}
		if self.anchors.contains(&id) {
			self.diagnostics.push(format!("duplicate HTML id: {id}"));
			return None;
		}
		self.anchors.reserve(id.clone());
		Some(id)
	}

	fn code_block(&mut self, node: &NodeRef<'a>) -> Result<BlockKind, Error> {
		let text = node.text().to_string();
		self.count_text(&text)?;
		let code = node
			.children_it(false)
			.find(|child| child.is_element() && is_name(child, "code"))
			.unwrap_or(*node);
		let language = code_language(&code)
			.or_else(|| code_language(node))
			.or_else(|| crate::language::shebang(&text).map(str::to_owned))
			.unwrap_or_default();
		// A code wrapper makes its content literal, including a Mermaid block.
		if language == "mermaid" && !self.inherited.code {
			let style = self.inherited.clone();
			return Ok(BlockKind::Paragraph(vec![Inline {
				link: style.link,
				kind: InlineKind::Image(Box::new(ImageSpec {
					source: image::ImageSource::Diagram(image::Diagram {
						language: "mermaid".into(),
						source: text,
					}),
					alt: String::new(),
					title: String::new(),
					width: None,
					height: None,
				})),
				style: style.visual,
			}]));
		}
		Ok(BlockKind::Code { language, text })
	}

	fn list(
		&mut self,
		node: &NodeRef<'a>,
		ordered: bool,
		depth: usize,
	) -> Result<BlockKind, Error> {
		let explicit_start: Option<isize> =
			node.attr("start").and_then(|value| value.parse().ok());
		let reversed = ordered && node.attr("reversed").is_some();
		let loose = node.children_it(false).any(|item| {
			item.is_element()
				&& item.children_it(false).any(|child| {
					is_name(&child, "p")
						|| is_name(&child, "div")
						|| is_name(&child, "blockquote")
				})
		});
		let mut items = Vec::new();
		for child in node.children_it(false) {
			if !is_name(&child, "li") {
				continue;
			}
			let item_anchor = self.block_anchor(&child);
			let number =
				child.attr("value").and_then(|value| value.parse().ok());
			// Readability removes `input` elements, so the state has to be on
			// the item already; see `preserve_checkbox`.
			let checked = child
				.attr("data-mv-checked")
				.map(|value| value.as_ref() != "0");
			let mut blocks = Vec::new();
			self.blocks(&child, depth + 1, &mut blocks)?;
			if blocks.is_empty() {
				blocks.push(
					self.make(Vec::new(), BlockKind::Paragraph(Vec::new()))?,
				);
			}
			if let Some(anchor) = item_anchor {
				blocks[0].add_anchor(anchor);
			}
			items.push(ListItem {
				checked,
				number,
				blocks,
			});
		}
		let start = if ordered {
			Some(explicit_start.unwrap_or(if reversed {
				items.len() as isize
			} else {
				1
			}))
		} else {
			None
		};
		Ok(BlockKind::List {
			start,
			reversed,
			tight: !loose,
			items,
		})
	}

	fn details(
		&mut self,
		node: &NodeRef<'a>,
		depth: usize,
	) -> Result<BlockKind, Error> {
		let open = node.attr("open").is_some();
		let style = self.inherited.clone();
		let mut summary = RichText::new();
		let mut blocks = Vec::new();
		let mut pending = RichText::new();
		for child in node.children_it(false) {
			if child.is_element() && is_name(&child, "summary") {
				self.flush(&mut pending, &mut blocks)?;
				summary = trim_rich(self.inlines(&child, &style, depth + 1)?);
			} else if child.is_text() {
				self.append_text(&mut pending, child.text().as_ref(), &style)?;
			} else if child.is_element() {
				let name = local_name(&child);
				if is_block_element(&name) || has_block_children(&child) {
					self.flush(&mut pending, &mut blocks)?;
					self.lower_block(&child, &name, depth + 1, &mut blocks)?;
				} else {
					self.inline_element(
						&child,
						&name,
						&style,
						&mut pending,
						depth + 1,
					)?;
				}
			}
		}
		self.flush(&mut pending, &mut blocks)?;
		Ok(BlockKind::Details {
			default_open: open,
			summary,
			blocks,
		})
	}

	fn figure(
		&mut self,
		node: &NodeRef<'a>,
		depth: usize,
	) -> Result<BlockKind, Error> {
		let style = self.inherited.clone();
		let mut blocks = Vec::new();
		let mut caption = None;
		let mut pending = RichText::new();
		for child in node.children_it(false) {
			if child.is_element() && is_name(&child, "figcaption") {
				self.flush(&mut pending, &mut blocks)?;
				let text =
					trim_rich(self.inlines(&child, &style, depth + 1)?);
				caption = (!text.is_empty()).then_some(text);
			} else if child.is_text() {
				self.append_text(&mut pending, child.text().as_ref(), &style)?;
			} else if child.is_element() {
				let name = local_name(&child);
				if is_block_element(&name) || has_block_children(&child) {
					self.flush(&mut pending, &mut blocks)?;
					self.lower_block(&child, &name, depth + 1, &mut blocks)?;
				} else {
					self.inline_element(
						&child,
						&name,
						&style,
						&mut pending,
						depth + 1,
					)?;
				}
			}
		}
		self.flush(&mut pending, &mut blocks)?;
		Ok(BlockKind::Figure { blocks, caption })
	}

	fn definition_list(
		&mut self,
		node: &NodeRef<'a>,
		depth: usize,
	) -> Result<BlockKind, Error> {
		let mut items: Vec<DefinitionItem> = Vec::new();
		self.definition_entries(node, depth, &mut items)?;
		Ok(BlockKind::DefinitionList(items))
	}

	/// Collects `<dt>`/`<dd>` in document order, descending through wrappers.
	fn definition_entries(
		&mut self,
		node: &NodeRef<'a>,
		depth: usize,
		items: &mut Vec<DefinitionItem>,
	) -> Result<(), Error> {
		if depth > self.profile.max_semantic_depth {
			return Err(Error::DepthExceeded);
		}
		for child in node.children_it(false) {
			if is_name(&child, "dt") {
				self.definition_term(&child, depth, items)?;
			} else if is_name(&child, "dd") {
				self.definition_body(&child, depth, items)?;
			} else if child.is_element() {
				// A wrapper owns its own visibility, like any other container.
				if presentation_only(&child) {
					continue;
				}
				self.definition_entries(&child, depth + 1, items)?;
			}
		}
		Ok(())
	}

	/// Appends one `<dt>` term, opening an item when the last one has a body.
	fn definition_term(
		&mut self,
		child: &NodeRef<'a>,
		depth: usize,
		items: &mut Vec<DefinitionItem>,
	) -> Result<(), Error> {
		if presentation_only(child) {
			return Ok(());
		}
		let anchor = self.block_anchor(child);
		let style = self.inherited.clone();
		let mut term = trim_rich(self.inlines(child, &style, depth + 1)?);
		if let Some(anchor) = anchor {
			term.insert(
				0,
				Inline {
					link: style.link.clone(),
					kind: InlineKind::Anchor(anchor),
					style: style.visual.clone(),
				},
			);
		}
		match items.last_mut() {
			Some(item) if item.definitions.is_empty() => item.terms.push(term),
			_ => items.push(DefinitionItem {
				terms: vec![term],
				definitions: Vec::new(),
			}),
		}
		Ok(())
	}

	/// Appends one `<dd>` body to the open item, opening one when needed.
	fn definition_body(
		&mut self,
		child: &NodeRef<'a>,
		depth: usize,
		items: &mut Vec<DefinitionItem>,
	) -> Result<(), Error> {
		let anchor = self.block_anchor(child);
		// A term may carry several definitions, so a second `<dd>` extends the
		// item that owns the term instead of opening a term-less one.
		if items.last().is_none_or(|item| item.terms.is_empty()) {
			items.push(DefinitionItem {
				terms: Vec::new(),
				definitions: Vec::new(),
			});
		}
		let mut blocks = Vec::new();
		self.blocks(child, depth + 1, &mut blocks)?;
		if let Some(anchor) = anchor {
			if blocks.is_empty() {
				blocks.push(
					self.make(Vec::new(), BlockKind::Paragraph(Vec::new()))?,
				);
			}
			blocks[0].add_anchor(anchor);
		}
		items
			.last_mut()
			.expect("a definition item")
			.definitions
			.push(blocks);
		Ok(())
	}

	#[cfg_attr(
		any(),
		markview::allow_complexity(
			reason = "Keep table grid construction, span clipping and cell lowering in one ordered pass."
		)
	)]
	fn table(
		&mut self,
		node: &NodeRef<'a>,
		depth: usize,
	) -> Result<BlockKind, Error> {
		let mut caption = None;
		let mut raw_groups: Vec<(TableGroupKind, Vec<NodeRef<'a>>)> =
			Vec::new();
		let style = self.inherited.clone();
		for child in node.children_it(false) {
			let name = local_name(&child);
			match name.as_str() {
				"caption" => {
					let text =
						trim_rich(self.inlines(&child, &style, depth + 1)?);
					caption = (!text.is_empty()).then_some(text);
				}
				"thead" | "tbody" | "tfoot" => {
					if presentation_only(&child) {
						continue;
					}
					let kind = match name.as_str() {
						"thead" => TableGroupKind::Head,
						"tfoot" => TableGroupKind::Foot,
						_ => TableGroupKind::Body,
					};
					raw_groups.push((
						kind,
						child
							.children_it(false)
							.filter(|row| {
								row.is_element()
									&& is_name(row, "tr")
									&& !presentation_only(row)
							})
							.collect(),
					));
				}
				"tr" => {
					if presentation_only(&child) {
						continue;
					}
					let kind = TableGroupKind::Body;
					match raw_groups.last_mut() {
						Some((kind, rows)) if *kind == TableGroupKind::Body => {
							rows.push(child)
						}
						_ => raw_groups.push((kind, vec![child])),
					}
				}
				_ => {}
			}
		}
		let total_rows: usize =
			raw_groups.iter().map(|(_, rows)| rows.len()).sum();
		self.table_rows = self.table_rows.saturating_add(total_rows);
		if self.table_rows > self.profile.max_table_rows {
			return Err(Error::TableTooLarge);
		}
		// HTML renders the head, then every body, then the foot, so the
		// emitted groups follow that order instead of source order.
		raw_groups.sort_by_key(|(kind, _)| match kind {
			TableGroupKind::Head => 0,
			TableGroupKind::Body => 1,
			TableGroupKind::Foot => 2,
		});
		let mut columns = 0usize;
		for (_, rows) in &raw_groups {
			let mut occupied = vec![Vec::<bool>::new(); rows.len()];
			for (row_index, row) in rows.iter().enumerate() {
				let mut col = 0usize;
				for cell in row.children_it(false) {
					if !is_name(&cell, "td") && !is_name(&cell, "th") {
						continue;
					}
					self.table_cells = self.table_cells.saturating_add(1);
					if self.table_cells > self.profile.max_table_cells {
						return Err(Error::TableTooLarge);
					}
					let colspan = span(&cell, "colspan", 1).max(1);
					let raw_rowspan = span_raw(&cell, "rowspan").unwrap_or(1);
					let rowspan = if raw_rowspan == 0 {
						rows.len() - row_index
					} else {
						raw_rowspan.min(rows.len() - row_index).max(1)
					};
					col.checked_add(colspan)
						.filter(|width| {
							*width <= self.profile.max_table_columns
						})
						.ok_or(Error::TableTooLarge)?;
					let Some((start, rowspan, colspan)) = fit_span(
						col,
						rowspan,
						colspan,
						self.profile.max_table_columns,
						|col, rowspan, colspan| {
							(row_index..row_index + rowspan).all(|r| {
								(col..col + colspan).all(|c| {
									!occupied[r]
										.get(c)
										.copied()
										.unwrap_or(false)
								})
							})
						},
					) else {
						return Err(Error::TableTooLarge);
					};
					let width = start + colspan;
					for row in occupied
						.iter_mut()
						.take(row_index + rowspan)
						.skip(row_index)
					{
						if row.len() < width {
							row.resize(width, false);
						}
						for slot in row.iter_mut().take(width).skip(start) {
							*slot = true;
						}
					}
					columns = columns.max(width);
					col = width;
				}
			}
		}
		if columns > self.profile.max_table_columns
			|| columns.saturating_mul(total_rows) > self.profile.max_table_cells
		{
			return Err(Error::TableTooLarge);
		}
		let mut groups = Vec::new();
		let mut column_aligns = vec![None::<CellAlign>; columns];
		let mut column_conflict = vec![false; columns];
		for (kind, rows) in raw_groups {
			let row_count = rows.len();
			let mut grid = vec![vec![None::<usize>; columns]; row_count];
			let mut placed: Vec<(usize, usize, usize, usize, usize)> =
				Vec::new();
			let mut placed_nodes: Vec<NodeRef<'a>> = Vec::new();
			for (row_index, row) in rows.iter().enumerate() {
				let mut col = 0usize;
				for cell in row.children_it(false) {
					let header = is_name(&cell, "th");
					if !header && !is_name(&cell, "td") {
						continue;
					}
					if col >= columns {
						break;
					}
					let colspan =
						span(&cell, "colspan", 1).min(columns - col).max(1);
					let raw_rowspan = span_raw(&cell, "rowspan").unwrap_or(1);
					let rowspan = if raw_rowspan == 0 {
						row_count - row_index
					} else {
						raw_rowspan.min(row_count - row_index).max(1)
					};
					let Some((start, rowspan, colspan)) = fit_span(
						col,
						rowspan,
						colspan,
						columns,
						|col, rowspan, colspan| {
							(row_index..row_index + rowspan).all(|r| {
								(col..col + colspan)
									.all(|c| grid[r][c].is_none())
							})
						},
					) else {
						break;
					};
					let id = placed.len();
					placed.push((row_index, start, rowspan, colspan, id));
					placed_nodes.push(cell);
					for row in grid
						.iter_mut()
						.take(row_index + rowspan)
						.skip(row_index)
					{
						for slot in
							row.iter_mut().take(start + colspan).skip(start)
						{
							*slot = Some(id);
						}
					}
					col = start + colspan;
				}
			}
			let mut by_row = vec![Vec::new(); row_count];
			for (index, cell) in placed.iter().enumerate() {
				by_row[cell.0].push(index);
			}
			let mut table_rows = Vec::new();
			for row in &by_row {
				let mut cells = Vec::new();
				for &index in row {
					let (_start_row, start_col, rowspan, colspan, id) =
						placed[index];
					let node = placed_nodes[id];
					let cell_anchor = self.block_anchor(&node);
					let mut blocks = Vec::new();
					self.blocks(&node, depth + 2, &mut blocks)?;
					if blocks.is_empty() {
						blocks.push(self.make(
							Vec::new(),
							BlockKind::Paragraph(Vec::new()),
						)?);
					}
					if let Some(anchor) = cell_anchor {
						blocks[0].add_anchor(anchor);
					}
					let align = cell_align(&node);
					if let Some(align) = align {
						for column in start_col..start_col + colspan {
							match column_aligns[column] {
								Some(known) if known != align => {
									column_conflict[column] = true;
								}
								None => column_aligns[column] = Some(align),
								_ => {}
							}
						}
					}
					cells.push(TableCell {
						kind: if is_name(&node, "th") {
							CellKind::Header
						} else {
							CellKind::Data
						},
						rowspan: rowspan as u32,
						colspan: colspan as u32,
						align,
						blocks,
					});
				}
				table_rows.push(TableRow { cells });
			}
			groups.push(TableGroup {
				kind,
				rows: table_rows,
			});
		}
		let columns = column_aligns
			.into_iter()
			.zip(column_conflict)
			.map(|(align, conflict)| TableColumn {
				align: if conflict { None } else { align },
			})
			.collect();
		Ok(BlockKind::Table(Table {
			caption,
			columns,
			groups,
		}))
	}

	fn inlines(
		&mut self,
		node: &NodeRef<'a>,
		style: &RunStyle,
		depth: usize,
	) -> Result<RichText, Error> {
		if depth > self.profile.max_semantic_depth {
			return Err(Error::DepthExceeded);
		}
		// A caption, summary or term owns its own visibility, so a hidden root
		// must not lower its children as text of their own.
		if presentation_only(node) {
			return Ok(RichText::new());
		}
		let mut out = RichText::new();
		for child in node.children_it(false) {
			if child.is_text() {
				let text = child.text().to_string();
				self.append_text(&mut out, &text, style)?;
			} else if child.is_element() {
				let name = local_name(&child);
				self.inline_element(&child, &name, style, &mut out, depth)?;
			}
		}
		Ok(out)
	}

	#[cfg_attr(
		any(),
		markview::allow_complexity(
			reason = "Keep the exhaustive inline HTML dispatch together; style, link and math handling share one traversal."
		)
	)]
	fn inline_element(
		&mut self,
		node: &NodeRef<'a>,
		name: &str,
		style: &RunStyle,
		out: &mut RichText,
		depth: usize,
	) -> Result<(), Error> {
		if depth > self.profile.max_semantic_depth {
			return Err(Error::DepthExceeded);
		}
		if presentation_only(node) {
			return Ok(());
		}
		self.inline_anchor(node, out, style);
		match name {
			"script" => {
				if !style.code
					&& let Some(math) = tex_script(node)
				{
					self.append_inline(
						out,
						InlineKind::Math {
							latex: math.0,
							display: math.1,
						},
						style,
					);
				}
				return Ok(());
			}
			name if is_skipped_inline(name) => {
				return Ok(());
			}
			"br" => {
				self.append_inline(
					out,
					InlineKind::LineBreak(LineBreak::Forced),
					style,
				);
				return Ok(());
			}
			"img" => {
				if let Some(image) = self.image(node) {
					self.count_text(&image.alt)?;
					self.count_text(&image.title)?;
					self.append_inline(
						out,
						InlineKind::Image(Box::new(image)),
						style,
					);
				}
				return Ok(());
			}
			"svg" => {
				if let Some(image) = svg_image(node) {
					self.count_text(image.address())?;
					self.append_inline(
						out,
						InlineKind::Image(Box::new(image)),
						style,
					);
				}
				return Ok(());
			}
			_ => {}
		}
		if !style.code
			&& (name == "math"
				|| node.has_class("math")
				|| node.attr("data-mv-math").is_some())
		{
			if let Some(math) = math_from(node, name) {
				self.count_text(&math.0)?;
				self.append_inline(
					out,
					InlineKind::Math {
						latex: math.0,
						display: math.1,
					},
					style,
				);
				return Ok(());
			}
			let fallback = node.text().to_string();
			if !fallback.trim().is_empty() {
				self.diagnostics
					.push("MathML without TeX annotation kept as text".into());
				self.append_text(out, &fallback, style)?;
			}
			return Ok(());
		}
		let style = self.run_style(node, name, style);
		let children = self.inlines(node, &style, depth + 1)?;
		for inline in children {
			self.append_inline(
				out,
				inline.kind,
				&RunStyle {
					visual: inline.style,
					link: inline.link,
				},
			);
		}
		Ok(())
	}

	fn link_url(&self, href: &str) -> Option<String> {
		let href = href.trim();
		if href.is_empty() {
			return None;
		}
		if href.starts_with('#') {
			return Some(href.to_owned());
		}
		Some(
			self.base
				.join(href)
				.map(|url| url.to_string())
				.unwrap_or_else(|_| href.to_owned()),
		)
	}

	fn image(&self, node: &NodeRef<'a>) -> Option<ImageSpec> {
		let raw = node
			.attr("src")
			.or_else(|| node.attr("data-src"))?
			.to_string();
		let src = if raw.starts_with("data:") {
			raw
		} else {
			self.base
				.join(raw.as_str())
				.map(|url| url.to_string())
				.unwrap_or(raw)
		};
		Some(ImageSpec {
			source: image::ImageSource::Resource(image::ResourceRef::document(
				src,
			)),
			alt: node.attr("alt").map(String::from).unwrap_or_default(),
			title: node.attr("title").map(String::from).unwrap_or_default(),
			width: node.attr("width").and_then(|v| v.parse().ok()),
			height: node.attr("height").and_then(|v| v.parse().ok()),
		})
	}

	fn append_text(
		&mut self,
		out: &mut RichText,
		text: &str,
		style: &RunStyle,
	) -> Result<(), Error> {
		self.count_text(text)?;
		self.append_inline(
			out,
			InlineKind::Text(collapse_inline_space(text)),
			style,
		);
		Ok(())
	}

	fn append_inline(
		&mut self,
		out: &mut RichText,
		kind: InlineKind,
		style: &RunStyle,
	) {
		let mut kind = kind;
		if let InlineKind::Text(text) = &mut kind {
			if out.last().is_some_and(|inline| {
				matches!(&inline.kind, InlineKind::Text(previous) if previous.ends_with(' '))
			}) {
				let trimmed = text.trim_start_matches(' ').to_owned();
				*text = trimmed;
			}
			if text.is_empty() {
				return;
			}
			if let Some(Inline {
				kind: InlineKind::Text(previous),
				style: previous_style,
				link: previous_link,
			}) = out.last_mut()
				&& *previous_style == style.visual
				&& match (previous_link.as_ref(), style.link.as_ref()) {
					(Some(a), Some(b)) => a.same_occurrence(b),
					(None, None) => true,
					_ => false,
				} {
				previous.push_str(text);
				return;
			}
		}
		out.push(Inline {
			kind,
			style: style.visual.clone(),
			link: style.link.clone(),
		});
	}
}

/// Applies an enclosing block-level HTML link to every rich run.
fn apply_link(block: &mut Block, link: &Link) {
	fn rich(text: &mut RichText, link: &Link) {
		for inline in text {
			if inline.link.is_none() {
				inline.link = Some(link.clone());
			}
		}
	}
	fn optional(text: &mut Option<RichText>, link: &Link) {
		if let Some(text) = text {
			rich(text, link);
		}
	}
	fn all(blocks: &mut [Block], link: &Link) {
		for child in blocks {
			apply_link(child, link);
		}
	}
	match block.kind_mut() {
		BlockKind::Paragraph(text) | BlockKind::Heading { text, .. } => {
			rich(text, link)
		}
		BlockKind::Quote { blocks, callout } => {
			if let Some(title) =
				callout.as_mut().and_then(|callout| callout.title.as_mut())
			{
				rich(title, link);
			}
			all(blocks, link);
		}
		BlockKind::Footnote { blocks, .. }
		| BlockKind::FrontMatter { blocks, .. } => all(blocks, link),
		BlockKind::Details {
			summary, blocks, ..
		} => {
			rich(summary, link);
			all(blocks, link);
		}
		BlockKind::Figure { blocks, caption } => {
			optional(caption, link);
			all(blocks, link);
		}
		BlockKind::List { items, .. } => {
			for item in items {
				all(&mut item.blocks, link);
			}
		}
		BlockKind::Table(table) => {
			optional(&mut table.caption, link);
			for child in table.cell_blocks_mut() {
				apply_link(child, link);
			}
		}
		BlockKind::DefinitionList(items) => {
			for item in items {
				for terms in &mut item.terms {
					rich(terms, link);
				}
				for blocks in &mut item.definitions {
					all(blocks, link);
				}
			}
		}
		BlockKind::Code { .. } | BlockKind::Rule => {}
	}
}

fn effective_base(
	extracted: &dom_smoothie::Extracted,
	mut base: url::Url,
) -> url::Url {
	let selection = extracted.document.select_single("base[href]");
	if let Some(node) = selection.nodes().first()
		&& let Some(href) = node.attr("href")
		&& let Ok(resolved) = base.join(href.as_ref())
	{
		base = resolved;
	}
	base
}

fn local_name(node: &NodeRef<'_>) -> String {
	node.node_name()
		.map(|name| name.to_ascii_lowercase())
		.unwrap_or_default()
}

fn is_name(node: &NodeRef<'_>, expected: &str) -> bool {
	node.node_name()
		.is_some_and(|name| name.eq_ignore_ascii_case(expected))
}

/// Whether the element is a visual duplicate for assistive technology.
///
/// Formula markup such as KaTeX and MathJax pairs a MathML copy with an
/// `aria-hidden` rendering of the same formula, so the hidden subtree must
/// not be lowered as text of its own. `role="presentation"` only removes
/// ARIA semantics, so it hides text inside formula markup and nowhere else.
fn presentation_only(node: &NodeRef<'_>) -> bool {
	if aria_hidden(node) {
		return true;
	}
	let attribute = |name: &str, value: &str| {
		node.attr(name)
			.is_some_and(|found| found.trim().eq_ignore_ascii_case(value))
	};
	let presentational = ["presentation", "none"]
		.iter()
		.any(|role| attribute("role", role));
	presentational && !is_math_source(node) && in_math_context(node)
}

/// Whether the element itself carries formula source.
fn is_math_source(node: &NodeRef<'_>) -> bool {
	is_name(node, "math")
		|| node.attr("data-mv-math").is_some()
		|| node.attr("class").is_some_and(|classes| {
			classes.split_whitespace().any(|class| class == "math")
		})
}

/// Whether the element is hidden from assistive technology.
fn aria_hidden(node: &NodeRef<'_>) -> bool {
	node.attr("aria-hidden")
		.is_some_and(|found| found.trim().eq_ignore_ascii_case("true"))
}

/// Whether the element sits inside formula markup.
fn in_math_context(node: &NodeRef<'_>) -> bool {
	std::iter::once(*node)
		.chain(node.ancestors_it(None))
		.any(|node| {
			is_name(&node, "math")
				|| node.attr("data-mv-math").is_some()
				|| node.attr("class").is_some_and(|classes| {
					classes.split_whitespace().any(|class| {
						class == "math"
							|| class.starts_with("katex")
							|| class.to_ascii_lowercase().contains("mathjax")
							|| class.starts_with("mjx")
					})
				})
		})
}

fn is_block_element(name: &str) -> bool {
	matches!(
		name,
		"p" | "div"
			| "section"
			| "article"
			| "main"
			| "header"
			| "footer"
			| "aside"
			| "nav"
			| "h1"
			| "h2"
			| "h3"
			| "h4"
			| "h5"
			| "h6"
			| "ul"
			| "ol"
			| "li"
			| "blockquote"
			| "pre"
			| "table"
			| "thead"
			| "tbody"
			| "tfoot"
			| "tr"
			| "td"
			| "th"
			| "caption"
			| "hr"
			| "figure"
			| "figcaption"
			| "details"
			| "summary"
			| "dl"
			| "dt"
			| "dd"
			| "form"
			| "fieldset"
			| "address"
			| "center"
	)
}

/// Elements whose content the reader never lowers as text.
fn is_skipped_inline(name: &str) -> bool {
	matches!(
		name,
		"style"
			| "template"
			| "noscript"
			| "iframe"
			| "object"
			| "embed"
			| "video"
			| "audio"
			| "input"
			| "button"
			| "select"
			| "textarea"
	)
}

/// Inline elements with their own semantic lowering that a mixed wrapper
/// must not replace with a transparent split.
fn delegated_inline(node: &NodeRef<'_>, name: &str) -> bool {
	name == "script"
		|| is_skipped_inline(name)
		|| name == "math"
		|| node.has_class("math")
		|| node.attr("data-mv-math").is_some()
}

fn has_block_children(node: &NodeRef<'_>) -> bool {
	node.children_it(false).any(|child| {
		child.is_element() && is_block_element(&local_name(&child))
	})
}

fn trim_rich(mut text: RichText) -> RichText {
	fn trim_edge(text: &mut RichText, end: bool) {
		let mut leading = true;
		text.retain(|inline| {
			if !leading {
				return true;
			}
			match &inline.kind {
				InlineKind::Anchor(_) => true,
				InlineKind::Text(t) if html_blank(t) => false,
				_ => {
					leading = false;
					true
				}
			}
		});
		if let Some(Inline {
			kind: InlineKind::Text(t),
			..
		}) = text.iter_mut().find(|inline| match &inline.kind {
			InlineKind::Anchor(_) => false,
			InlineKind::Text(t) => !html_blank(t),
			_ => true,
		}) {
			if end {
				t.truncate(html_trim_end(t).len());
			} else {
				let start = html_trim_start(t).len();
				t.drain(..t.len() - start);
			}
		}
	}
	trim_edge(&mut text, false);
	text.reverse();
	trim_edge(&mut text, true);
	text.reverse();
	text
}

fn code_language(node: &NodeRef<'_>) -> Option<String> {
	node.attr("data-mv-language").map(|value| value.to_string())
}

fn html_blank(text: &str) -> bool {
	text.chars().all(|c| c.is_ascii_whitespace())
}

fn html_trim_start(text: &str) -> &str {
	text.trim_start_matches(|c: char| c.is_ascii_whitespace())
}

fn html_trim_end(text: &str) -> &str {
	text.trim_end_matches(|c: char| c.is_ascii_whitespace())
}

fn collapse_inline_space(text: &str) -> String {
	let mut out = String::with_capacity(text.len());
	let mut space = false;
	let leading = text
		.chars()
		.next()
		.is_some_and(|c: char| c.is_ascii_whitespace());
	for c in text.chars() {
		if c.is_ascii_whitespace() {
			space = true;
			continue;
		}
		if space && (!out.is_empty() || leading) {
			out.push(' ');
		}
		space = false;
		out.push(c);
	}
	if space {
		out.push(' ');
	}
	out
}

/// The alignment a table cell declares, kept by `preserve_semantics`.
fn cell_align(node: &NodeRef<'_>) -> Option<CellAlign> {
	let value = node.attr("data-mv-align").or_else(|| node.attr("align"))?;
	match value.to_ascii_lowercase().as_str() {
		"center" => Some(CellAlign::Center),
		"right" => Some(CellAlign::Right),
		"left" => Some(CellAlign::Left),
		_ => None,
	}
}

fn span(node: &NodeRef<'_>, attr: &str, default: usize) -> usize {
	span_raw(node, attr)
		.map(|value| value.max(1))
		.unwrap_or(default)
}

fn span_raw(node: &NodeRef<'_>, attr: &str) -> Option<usize> {
	node.attr(attr)?.parse().ok()
}

/// Places a cell at the first free slot at or after the cursor, clipping its
/// declared span to the free rectangle that starts there.
///
/// A collision above the cursor therefore no longer demotes the cell to a
/// plain 1x1: the span is retried at the next free slot, which is where an
/// HTML table places it, and only the part of the span that does not fit is
/// dropped. Starting at the first free slot also keeps a row free of interior
/// holes.
///
/// `free` reports whether a span fits at a column. `None` means the row has
/// no free slot left for the cell.
fn fit_span(
	col: usize,
	rowspan: usize,
	colspan: usize,
	columns: usize,
	mut free: impl FnMut(usize, usize, usize) -> bool,
) -> Option<(usize, usize, usize)> {
	for start in col..columns {
		if !free(start, 1, 1) {
			continue;
		}
		let mut colspan = colspan.min(columns - start);
		while colspan > 1 && !free(start, 1, colspan) {
			colspan -= 1;
		}
		let mut rowspan = rowspan;
		while rowspan > 1 && !free(start, rowspan, colspan) {
			rowspan -= 1;
		}
		return Some((start, rowspan, colspan));
	}
	None
}

/// The TeX source a `math/tex` script carries, and whether it is display.
fn tex_script(node: &NodeRef<'_>) -> Option<(String, bool)> {
	let kind = node.attr("type")?.to_ascii_lowercase();
	if !kind.starts_with("math/tex") {
		return None;
	}
	let latex = node.text().to_string();
	let display =
		kind.contains("mode=display") || has_display_delimiters(&latex);
	Some((strip_delimiters(&latex).to_owned(), display))
}

fn math_from(node: &NodeRef<'_>, name: &str) -> Option<(String, bool)> {
	if name == "script" {
		return tex_script(node);
	}
	let display = node.has_class("display")
		|| node.attr("data-mv-display").is_some()
		|| node
			.attr("display")
			.is_some_and(|value| value.as_ref() == "block");
	// Delimiters state display on their own, so a stripped formula keeps it.
	let display = |latex: &str| display || has_display_delimiters(latex);
	if let Some(latex) = node.attr("data-mv-latex") {
		let latex = latex.to_string();
		return Some((strip_delimiters(&latex).to_owned(), display(&latex)));
	}
	let annotation = node.descendants_it().find(|child| {
		child.is_element()
			&& is_name(child, "annotation")
			&& child.attr("encoding").is_some_and(|encoding| {
				encoding.to_ascii_lowercase().contains("application/x-tex")
			})
	});
	if let Some(annotation) = annotation {
		let latex = annotation.text().to_string();
		return Some((strip_delimiters(&latex).to_owned(), display(&latex)));
	}
	if name == "math" {
		// MathML's own text is the rendered glyphs, not TeX, so only the
		// declared `alttext` may stand in for a missing TeX annotation.
		let alttext = node.attr("alttext").map(|text| text.to_string());
		let alttext = alttext.unwrap_or_default();
		let trimmed = alttext.trim();
		return (!trimmed.is_empty())
			.then(|| (strip_delimiters(trimmed).to_owned(), display(trimmed)));
	}
	let text = node.text().to_string();
	if node.has_class("math") || node.attr("data-mv-math").is_some() {
		let trimmed = text.trim();
		if trimmed.is_empty() {
			return None;
		}
		return Some((strip_delimiters(trimmed).to_owned(), display(trimmed)));
	}
	None
}

/// Whether TeX source is wrapped in display delimiters.
fn has_display_delimiters(text: &str) -> bool {
	let text = text.trim();
	(text.starts_with(r"\[") && text.ends_with(r"\]"))
		|| (text.starts_with("$$") && text.ends_with("$$"))
}

fn strip_delimiters(text: &str) -> &str {
	for (open, close) in
		[(r"\(", r"\)"), (r"\[", r"\]"), ("$$", "$$"), ("$", "$")]
	{
		if let Some(inner) =
			text.strip_prefix(open).and_then(|s| s.strip_suffix(close))
		{
			return inner;
		}
	}
	text
}

fn svg_image(node: &NodeRef<'_>) -> Option<ImageSpec> {
	let html = node.html().to_string();
	let encoded = percent_encoding::utf8_percent_encode(
		&html,
		percent_encoding::NON_ALPHANUMERIC,
	)
	.to_string();
	Some(ImageSpec {
		source: image::ImageSource::Resource(image::ResourceRef::document(
			format!("data:image/svg+xml,{encoded}"),
		)),
		alt: String::new(),
		title: String::new(),
		width: node.attr("width").and_then(|v| v.parse().ok()),
		height: node.attr("height").and_then(|v| v.parse().ok()),
	})
}

/// Whether the lowered body contains anything a reader would call content.
pub(crate) fn has_readable_content(blocks: &[Block]) -> bool {
	fn block_has(block: &Block) -> bool {
		match block.kind() {
			BlockKind::Paragraph(text) | BlockKind::Heading { text, .. } => {
				rich_has(text)
			}
			BlockKind::Code { text, .. } => !text.trim().is_empty(),
			BlockKind::Rule => false,
			BlockKind::Quote { blocks, .. }
			| BlockKind::Footnote { blocks, .. }
			| BlockKind::Details { blocks, .. }
			| BlockKind::FrontMatter { blocks, .. }
			| BlockKind::Figure { blocks, .. } => {
				blocks.iter().any(block_has)
					|| matches!(block.kind(), BlockKind::Details { summary, .. } if rich_has(summary))
					|| matches!(block.kind(), BlockKind::Figure { caption: Some(c), .. } if rich_has(c))
			}
			BlockKind::List { items, .. } => {
				items.iter().any(|item| item.blocks.iter().any(block_has))
			}
			BlockKind::Table(table) => {
				table.caption.as_ref().is_some_and(rich_has)
					|| table.cell_blocks().any(block_has)
			}
			BlockKind::DefinitionList(items) => items.iter().any(|item| {
				item.terms.iter().any(rich_has)
					|| item
						.definitions
						.iter()
						.any(|blocks| blocks.iter().any(block_has))
			}),
		}
	}
	fn rich_has(text: &RichText) -> bool {
		text.iter().any(|inline| match &inline.kind {
			InlineKind::Text(text) => !text.trim().is_empty(),
			InlineKind::Image(_)
			| InlineKind::Math { .. }
			| InlineKind::FootnoteRef(_) => true,
			InlineKind::LineBreak(_) | InlineKind::Anchor(_) => false,
		})
	}
	blocks.iter().any(block_has)
}

/// Prepends the visible title and source line when the body lacks them.
pub(crate) fn with_title_and_source(
	mut body: Vec<Block>,
	mut builder: DocumentBuilder,
	title: &str,
	url: &str,
	title_heading: bool,
	next_link: usize,
) -> Result<Document, Error> {
	let mut anchors = Anchors::default();
	reserve_document_anchors(&body, &mut anchors);
	let mut out = Vec::new();
	if !title_heading {
		let anchor = anchors.unique(title);
		out.push(builder.block(
			Vec::new(),
			BlockKind::Heading {
				level: 1,
				text: vec![Inline {
					link: None,
					kind: InlineKind::Text(title.to_owned()),
					style: TextStyle::default(),
				}],
				anchor,
			},
		));
	}
	// The source line follows every body link, so its occurrence cannot
	// collide with one from the article.
	let link = allocated_link(url.to_owned(), next_link);
	out.push(builder.block(
		Vec::new(),
		BlockKind::Paragraph(vec![
			Inline {
				link: None,
				kind: InlineKind::Text("Source: ".into()),
				style: TextStyle::default(),
			},
			Inline {
				link: Some(link),
				kind: InlineKind::Text(url.to_owned()),
				style: TextStyle {
					..TextStyle::default()
				},
			},
		]),
	));
	out.append(&mut body);
	builder.try_finish(out).map_err(limit_error)
}

fn reserve_document_anchors(blocks: &[Block], anchors: &mut Anchors) {
	for root in blocks {
		markview_document::visit_blocks(root, &mut |block| {
			for anchor in block.anchors() {
				anchors.reserve(anchor.clone());
			}
			if let BlockKind::Heading { anchor, .. } = block.kind() {
				anchors.reserve(anchor.clone());
			}
		});
		let mut enclosing = Vec::new();
		markview_document::visit_fields(
			root,
			&mut enclosing,
			&|| false,
			&mut |_, _, field, _| {
				if let markview_document::FieldText::Rich(text) = field {
					for inline in text {
						if let InlineKind::Anchor(anchor) = &inline.kind {
							anchors.reserve(anchor.clone());
						}
					}
				}
			},
		);
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use markview_document::{Callout, CalloutKind};

	fn text(value: &str) -> Inline {
		Inline {
			link: None,
			kind: InlineKind::Text(value.to_owned()),
			style: TextStyle::default(),
		}
	}

	#[test]
	fn a_block_link_reaches_a_callout_title() {
		let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
		let body =
			builder.block(Vec::new(), BlockKind::Paragraph(vec![text("Body")]));
		let mut quote = builder.block(
			Vec::new(),
			BlockKind::Quote {
				callout: Some(Callout {
					kind: CalloutKind::Note,
					name: "note".to_owned(),
					title: Some(vec![text("Title")]),
					fold: None,
				}),
				blocks: vec![body],
			},
		);
		let url = "https://example.org/target";
		apply_link(
			&mut quote,
			&Link::new(
				url,
				markview_document::LinkOccurrenceId::new(1).unwrap(),
			),
		);
		let BlockKind::Quote {
			callout: Some(callout),
			blocks,
		} = quote.kind()
		else {
			panic!("a callout quote");
		};
		let title = callout.title.as_ref().expect("a plain callout title");
		assert!(
			title.iter().all(|inline| inline.link_url() == Some(url)),
			"{title:?}"
		);
		let BlockKind::Paragraph(body) = blocks[0].kind() else {
			panic!("a body paragraph");
		};
		assert!(
			body.iter().all(|inline| inline.link_url() == Some(url)),
			"{body:?}"
		);
	}
}

/// Inherited producer state; navigation is separated when emitting an inline.
#[derive(Clone, Debug, Default)]
struct RunStyle {
	visual: TextStyle,
	link: Option<Link>,
}
impl std::ops::Deref for RunStyle {
	type Target = TextStyle;
	fn deref(&self) -> &TextStyle {
		&self.visual
	}
}
impl std::ops::DerefMut for RunStyle {
	fn deref_mut(&mut self) -> &mut TextStyle {
		&mut self.visual
	}
}
