//! Composes parsed notes from targets supplied by the caller.
use crate::{
	AnchorId, Block, BlockId, BlockKind, Document, DocumentBuilder,
	DocumentLimits, DocumentRevision, FieldText, FootnoteId, InlineKind, Link,
	LinkOccurrenceId, ListItem, RichText, image::ResourceBase, visit_blocks,
	visit_fields,
};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq)]
pub struct SourceId(pub u32);

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum Selector {
	Block(String),
	Section(String),
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct NoteEmbed {
	/// The encoded wiki address, or empty for the source note itself.
	pub note: String,
	pub selector: Selector,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Request {
	pub from: SourceId,
	pub note: String,
}

#[derive(Clone, Copy, Debug)]
pub struct Source<'a> {
	pub id: SourceId,
	pub document: &'a Document,
}

/// A completed lookup. The absence of a key means the lookup is still needed.
#[derive(Clone, Copy, Debug)]
pub enum Target<'a> {
	Loaded(Source<'a>),
	Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Origin {
	pub source: SourceId,
	pub block: BlockId,
}

/// A link still using its source note's address space.
pub struct SourceLink {
	pub source: SourceId,
	pub link: Link,
}

pub struct Expanded {
	pub document: Document,
	pub origins: HashMap<BlockId, Origin>,
	pub source_links: Vec<SourceLink>,
}

pub enum Expansion {
	Ready(Box<Expanded>),
	NeedsTargets(Vec<Request>),
}

const MAX_DEPTH: usize = 8;
const MAX_TARGETS: usize = 128;

#[cfg(test)]
thread_local! {
	static SECTION_START_PROBES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
	static HEADING_TITLE_PROBES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
	static FRAGMENT_ROOT_PROBES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The selected heading and its following blocks within the same container,
/// limited to a prefix of at most `max_blocks` blocks.
pub fn section_prefix<'a>(
	document: &'a Document,
	node: &Block,
	max_blocks: usize,
) -> &'a [Block] {
	let (index, parent_path) =
		document.path_of(node.id()).unwrap().split_last().unwrap();
	let (siblings, start) = if parent_path.is_empty() {
		(document.blocks(), *index)
	} else {
		let parent = document.block_at_path(parent_path).unwrap();
		crate::index::siblings_at(parent, *index).unwrap()
	};
	#[cfg(test)]
	{
		SECTION_START_PROBES.with(|probes| probes.set(probes.get() + 1));
		assert_eq!(siblings[start].id(), node.id());
	}
	let end = start.saturating_add(max_blocks).min(siblings.len());
	let section = &siblings[start..end];
	let BlockKind::Heading { level, .. } = node.kind() else {
		return &section[..section.len().min(1)];
	};
	let following = section.iter().skip(1).position(|block| {
		matches!(block.kind(), BlockKind::Heading { level: next, .. } if next <= level)
	});
	&section[..following.map_or(section.len(), |offset| 1 + offset)]
}

#[derive(Clone, Copy)]
struct Selection<'a> {
	blocks: &'a [Block],
	item: Option<ItemList>,
}

#[derive(PartialEq, Eq, Hash)]
struct SelectionKey {
	source: SourceId,
	first: BlockId,
	last: BlockId,
	list_item: bool,
}

#[derive(Clone, Copy)]
struct ItemList {
	source: BlockId,
	start: Option<isize>,
	tight: bool,
	checked: Option<bool>,
}

impl ItemList {
	fn wrap(self, blocks: Vec<Block>) -> Block {
		Block {
			id: self.source,
			anchors: Vec::new(),
			list_item_anchor: None,
			embed: None,
			content_key: Default::default(),
			kind: BlockKind::List {
				start: self.start,
				reversed: false,
				tight: self.tight,
				items: vec![ListItem {
					checked: self.checked,
					number: None,
					blocks,
				}],
			},
		}
	}
}

fn anchored_block<'a>(
	document: &'a Document,
	anchor: &str,
) -> Option<Selection<'a>> {
	let anchor = document.resolve_anchor(anchor);
	if let Some(position) = document.list_item_position(&anchor) {
		return list_item(document.block(position.list)?, position);
	}
	let block = document.anchor_block(anchor)?;
	Some(Selection {
		blocks: std::slice::from_ref(block),
		item: None,
	})
}

fn list_item(
	list: &Block,
	position: crate::index::ListItemPosition,
) -> Option<Selection<'_>> {
	let BlockKind::List { tight, items, .. } = list.kind() else {
		return None;
	};
	let item = items.get(position.index)?;
	Some(Selection {
		blocks: &item.blocks,
		item: Some(ItemList {
			source: list.id(),
			start: position.number,
			tight: *tight,
			checked: item.checked,
		}),
	})
}

struct HeadingIndex<'a> {
	titles: HashMap<&'a str, BlockId>,
	slugs: Option<HashMap<String, BlockId>>,
}

impl<'a> HeadingIndex<'a> {
	fn new(document: &'a Document) -> Self {
		let mut titles = HashMap::new();
		for entry in document.outline_entries() {
			#[cfg(test)]
			HEADING_TITLE_PROBES.with(|probes| probes.set(probes.get() + 1));
			titles.entry(entry.text.as_str()).or_insert(entry.block);
		}
		Self {
			titles,
			slugs: None,
		}
	}

	fn find(
		&mut self,
		document: &'a Document,
		anchor: &str,
	) -> Option<&'a Block> {
		if let Some(id) = self.titles.get(anchor) {
			return document.block(*id);
		}
		let slug = crate::heading_slug(anchor);
		if let Some(block) = document.anchor_block(slug.as_str())
			&& matches!(block.kind(), BlockKind::Heading { .. })
		{
			return Some(block);
		}
		let slugs = self.slugs.get_or_insert_with(|| {
			let mut slugs = HashMap::new();
			for entry in document.outline_entries() {
				slugs
					.entry(crate::heading_slug(&entry.text))
					.or_insert(entry.block);
			}
			slugs
		});
		document.block(*slugs.get(&slug)?)
	}
}

struct SectionExtent<'a> {
	container: &'a [usize],
	end: usize,
	level: u8,
}

impl SectionExtent<'_> {
	fn contains(&self, path: &[usize]) -> bool {
		path.starts_with(self.container)
			&& path
				.get(self.container.len())
				.is_some_and(|index| *index <= self.end)
	}
}

fn compound_heading<'a>(
	document: &'a Document,
	anchor: &str,
) -> Option<&'a Block> {
	let parts: Vec<_> =
		anchor.split('#').filter(|part| !part.is_empty()).collect();
	match_heading_path(document, &parts, |title, part| title == part).or_else(
		|| {
			let normalized: Vec<_> = parts
				.iter()
				.map(|part| crate::heading_path::key(part))
				.collect();
			let parts: Vec<_> = normalized.iter().map(String::as_str).collect();
			match_heading_path(document, &parts, |title, part| {
				crate::heading_path::key(title) == part
			})
		},
	)
}

fn match_heading_path<'a>(
	document: &'a Document,
	parts: &[&str],
	matches: impl Fn(&str, &str) -> bool,
) -> Option<&'a Block> {
	let mut prefixes: Vec<SectionExtent<'_>> = Vec::new();
	for entry in document.outline_entries() {
		let path = document.path_of(entry.block).unwrap();
		while prefixes
			.last()
			.is_some_and(|section| !section.contains(path))
		{
			prefixes.pop();
		}
		if prefixes
			.last()
			.is_some_and(|section| entry.level <= section.level)
			|| !parts
				.get(prefixes.len())
				.is_some_and(|part| matches(&entry.text, part))
		{
			continue;
		}
		let heading = document.block(entry.block).unwrap();
		if prefixes.len() + 1 == parts.len() {
			return Some(heading);
		}
		let last = section_prefix(document, heading, usize::MAX)
			.last()
			.unwrap();
		let end = document.path_of(last.id()).unwrap()[path.len() - 1];
		prefixes.push(SectionExtent {
			container: &path[..path.len() - 1],
			end,
			level: entry.level,
		});
	}
	None
}

/// Composes the selected fragments, or returns the outstanding lookups in reading order.
pub fn expand(
	root: &Source<'_>,
	targets: &HashMap<Request, Target<'_>>,
) -> Expansion {
	let mut remaining = DocumentLimits::default();
	if !root.document.has_note_embeds()
		|| !charge(
			&mut remaining,
			&root.document.blocks().iter().collect::<Vec<_>>(),
		) {
		return unchanged(root.document);
	}
	let mut next_occurrence = 1;
	for block in root.document.blocks() {
		visit_fields(
			block,
			&mut Vec::new(),
			&|| false,
			&mut |_, _, field, _| {
				if let FieldText::Rich(text) = field {
					for inline in text {
						if let Some(link) = &inline.link {
							next_occurrence = next_occurrence
								.max(link.occurrence().get() + 1);
						}
						if let InlineKind::FootnoteRef(note) = &inline.kind {
							next_occurrence =
								next_occurrence.max(note.occurrence.get() + 1);
						}
					}
				}
			},
		);
	}
	let mut state = Composer {
		targets,
		selections: HashMap::new(),
		headings: HashMap::new(),
		rejected: HashSet::new(),
		needed: Vec::new(),
		requested: HashSet::new(),
		stack: Vec::new(),
		builder: DocumentBuilder::continuing(
			root.document.revision(),
			root.document.index.paths.keys().copied().max(),
		),
		next_occurrence,
		next_scope: 0,
		prefix: namespace_prefix(root.document),
		origins: HashMap::new(),
		source_links: Vec::new(),
		remaining,
	};
	let mut blocks = root.document.blocks().to_vec();
	state.list(&mut blocks, root, None, 0);
	if !state.needed.is_empty() {
		return Expansion::NeedsTargets(state.needed);
	}
	if state.origins.is_empty() {
		return unchanged(root.document);
	}
	let Ok(mut document) = state.builder.try_finish(blocks) else {
		return unchanged(root.document);
	};
	document
		.index
		.outline
		.retain(|entry| !state.origins.contains_key(&entry.block));
	set_revision(&mut document, root.document.revision());
	Expansion::Ready(Box::new(Expanded {
		document,
		origins: state.origins,
		source_links: state.source_links,
	}))
}

fn namespace_prefix(document: &Document) -> String {
	const PREFIX: &str = "mv-embed-";
	let occupied: HashSet<usize> = document
		.index
		.enclosing
		.keys()
		.filter_map(|anchor| {
			let name = match anchor {
				AnchorId::Fragment(name) => name.as_str(),
				AnchorId::FootnoteDefinition(id) => id.as_str(),
				_ => return None,
			};
			name.strip_prefix(PREFIX)?.split_once('-')?.0.parse().ok()
		})
		.collect();
	let number = (0..).find(|number| !occupied.contains(number)).unwrap();
	format!("{PREFIX}{number}-")
}

fn unchanged(document: &Document) -> Expansion {
	Expansion::Ready(Box::new(Expanded {
		document: document.clone(),
		origins: HashMap::new(),
		source_links: Vec::new(),
	}))
}

struct Composer<'a, 'document> {
	targets: &'a HashMap<Request, Target<'document>>,
	selections:
		HashMap<SourceId, HashMap<Selector, Option<Selection<'document>>>>,
	headings: HashMap<SourceId, HeadingIndex<'document>>,
	rejected: HashSet<SelectionKey>,
	needed: Vec<Request>,
	requested: HashSet<Request>,
	stack: Vec<SelectionKey>,
	builder: DocumentBuilder,
	next_occurrence: u32,
	next_scope: usize,
	prefix: String,
	origins: HashMap<BlockId, Origin>,
	source_links: Vec<SourceLink>,
	remaining: DocumentLimits,
}

struct Scope {
	prefix: String,
	selected: HashSet<BlockId>,
	occurrences: HashMap<LinkOccurrenceId, LinkOccurrenceId>,
	local_links: HashMap<String, Link>,
}

enum ScopeLimit {
	Depth,
	Resources,
}

impl<'document> Composer<'_, 'document> {
	fn list(
		&mut self,
		blocks: &mut Vec<Block>,
		source: &Source<'document>,
		mut scope: Option<&mut Scope>,
		depth: usize,
	) {
		let mut result = Vec::with_capacity(blocks.len());
		for mut block in std::mem::take(blocks) {
			if let Some(mut replacement) = block
				.note_embed()
				.and_then(|embed| self.embed(source, embed, depth))
			{
				for anchor in block.anchors {
					let anchor = scope.as_ref().map_or_else(
						|| anchor.clone(),
						|scope| format!("{}{anchor}", scope.prefix),
					);
					replacement[0].add_anchor(anchor);
				}
				result.extend(replacement);
				continue;
			}
			if let Some(scope) = scope.as_deref_mut() {
				self.import(&mut block, source, scope);
			}
			block.embed = None;
			for_child_list(&mut block.kind, |children| {
				self.list(children, source, scope.as_deref_mut(), depth + 1)
			});
			result.push(block);
		}
		*blocks = result;
	}

	fn embed(
		&mut self,
		source: &Source<'document>,
		embed: &NoteEmbed,
		depth: usize,
	) -> Option<Vec<Block>> {
		if self.stack.len() >= MAX_DEPTH {
			return None;
		}
		let target = if embed.note.is_empty() {
			source
		} else {
			let request = Request {
				from: source.id,
				note: embed.note.clone(),
			};
			match self.targets.get(&request) {
				Some(Target::Loaded(target)) => target,
				Some(Target::Unavailable) => return None,
				None => {
					if self.targets.len() + self.needed.len() < MAX_TARGETS
						&& self.requested.insert(request.clone())
					{
						self.needed.push(request);
					}
					return None;
				}
			}
		};
		let selection = self.select(target, &embed.selector)?;
		let key = SelectionKey {
			source: target.id,
			first: selection.blocks[0].id(),
			last: selection.blocks.last().unwrap().id(),
			list_item: selection.item.is_some(),
		};
		if self.rejected.contains(&key) || self.stack.contains(&key) {
			return None;
		}
		let mut selected: Vec<_> = selection
			.blocks
			.iter()
			.take_while(|block| {
				!matches!(block.kind(), BlockKind::Footnote { .. })
			})
			.collect();
		if selected.is_empty() {
			return None;
		}
		let item_blocks = selection.item.map_or(0, |_| selected.len());
		let mut notes = HashSet::new();
		let mut index = 0;
		while let Some(block) = selected.get(index).copied() {
			#[cfg(test)]
			FRAGMENT_ROOT_PROBES.with(|probes| probes.set(probes.get() + 1));
			index += 1;
			visit_fields(
				block,
				&mut Vec::new(),
				&|| false,
				&mut |_, _, field, _| {
					if let FieldText::Rich(text) = field {
						for inline in text {
							if let InlineKind::FootnoteRef(note) = &inline.kind
								&& notes.insert(note.definition.clone())
								&& let Some(definition) = target
									.document
									.anchor_block(AnchorId::FootnoteDefinition(
										note.definition.clone(),
									)) {
								selected.push(definition);
							}
						}
					}
				},
			);
		}
		let mut scope =
			match self.prepare_scope(target, &selected, depth, item_blocks) {
				Ok(scope) => scope,
				Err(limit) => {
					if matches!(limit, ScopeLimit::Resources) {
						self.rejected.insert(key);
					}
					return None;
				}
			};
		self.stack.push(key);
		let mut blocks: Vec<_> = selected.into_iter().cloned().collect();
		if let Some(item) = selection.item {
			let wrapper = item.wrap(blocks.drain(..item_blocks).collect());
			blocks.insert(0, wrapper);
		}
		self.list(&mut blocks, target, Some(&mut scope), depth);
		self.stack.pop();
		Some(blocks)
	}

	fn select(
		&mut self,
		source: &Source<'document>,
		selector: &Selector,
	) -> Option<Selection<'document>> {
		let selections = self.selections.entry(source.id).or_default();
		if let Some(selection) = selections.get(selector) {
			return *selection;
		}
		let selection = match selector {
			Selector::Block(id) => {
				anchored_block(source.document, &format!("^{id}"))
			}
			Selector::Section(anchor) => {
				let heading = if anchor.contains('#') {
					compound_heading(source.document, anchor)
				} else {
					self.headings
						.entry(source.id)
						.or_insert_with(|| HeadingIndex::new(source.document))
						.find(source.document, anchor)
				};
				heading.map(|heading| Selection {
					blocks: section_prefix(
						source.document,
						heading,
						usize::MAX,
					),
					item: None,
				})
			}
		};
		selections.insert(selector.clone(), selection);
		selection
	}

	fn prepare_scope(
		&mut self,
		target: &Source<'_>,
		selected: &[&Block],
		depth: usize,
		item_blocks: usize,
	) -> Result<Scope, ScopeLimit> {
		let mut remaining = self.remaining;
		remaining.max_blocks = remaining
			.max_blocks
			.checked_sub(usize::from(item_blocks > 0))
			.ok_or(ScopeLimit::Resources)?;
		if !charge(&mut remaining, selected) {
			return Err(ScopeLimit::Resources);
		}
		let mut scope = Scope {
			prefix: format!("{}{}-", self.prefix, self.next_scope + 1),
			selected: HashSet::new(),
			occurrences: HashMap::new(),
			local_links: HashMap::new(),
		};
		let mut fits = true;
		for (index, block) in selected.iter().enumerate() {
			let depth = depth + usize::from(index < item_blocks);
			let source_depth =
				target.document.path_of(block.id()).unwrap().len();
			visit_blocks(block, &mut |block| {
				let block_depth =
					target.document.path_of(block.id()).unwrap().len();
				fits &=
					depth + (block_depth - source_depth) <= remaining.max_depth;
				scope.selected.insert(block.id());
			});
		}
		if !fits {
			return Err(ScopeLimit::Depth);
		}
		let link_bytes = local_link_bytes(selected, target, &scope);
		remaining.max_text_bytes = remaining
			.max_text_bytes
			.checked_sub(link_bytes)
			.ok_or(ScopeLimit::Resources)?;
		self.remaining = remaining;
		self.next_scope += 1;
		Ok(scope)
	}

	fn import(
		&mut self,
		block: &mut Block,
		source: &Source<'_>,
		scope: &mut Scope,
	) {
		let origin = Origin {
			source: source.id,
			block: block.id(),
		};
		let namespace = |name: &str| format!("{}{name}", scope.prefix);
		let anchors =
			block.anchors.iter().map(|name| namespace(name)).collect();
		match &mut block.kind {
			BlockKind::Heading { anchor, .. } => *anchor = namespace(anchor),
			BlockKind::Footnote { id, .. } => {
				*id = FootnoteId::new(namespace(id.as_str()))
			}
			_ => {}
		}
		let occurrence = |old, next: &mut u32, map: &mut HashMap<_, _>| {
			*map.entry(old).or_insert_with(|| {
				let new = LinkOccurrenceId::new(*next).unwrap();
				*next += 1;
				new
			})
		};
		for_rich_field(&mut block.kind, |text| {
			for inline in text {
				if let Some(link) = &mut inline.link {
					let first =
						!scope.occurrences.contains_key(&link.occurrence());
					let id = occurrence(
						link.occurrence(),
						&mut self.next_occurrence,
						&mut scope.occurrences,
					);
					if let Some(fragment) =
						local_fragment(link.as_str(), source, &scope.selected)
					{
						let address = scope
							.local_links
							.entry(fragment)
							.or_insert_with_key(|fragment| {
								let anchor = namespace(fragment);
								let encoded =
									percent_encoding::utf8_percent_encode(
										&anchor,
										percent_encoding::NON_ALPHANUMERIC,
									);
								Link::new(format!("#{encoded}"), id)
							});
						*link = address.with_occurrence(id);
					} else {
						*link = link.with_occurrence(id);
						if first {
							self.source_links.push(SourceLink {
								source: source.id,
								link: link.clone(),
							});
						}
					}
				}
				match &mut inline.kind {
					InlineKind::Anchor(name) => *name = namespace(name),
					InlineKind::FootnoteRef(note) => {
						note.definition = FootnoteId::new(namespace(
							note.definition.as_str(),
						));
						note.occurrence = occurrence(
							note.occurrence,
							&mut self.next_occurrence,
							&mut scope.occurrences,
						);
					}
					InlineKind::Image(image) => {
						if let crate::image::ImageSource::Resource(resource) =
							&mut image.source
						{
							resource.base = match resource.base {
								ResourceBase::Document => {
									ResourceBase::Source(source.id)
								}
								ResourceBase::Vault => {
									ResourceBase::VaultSource(source.id)
								}
								base => base,
							};
						}
					}
					_ => {}
				}
			}
		});
		*block = self.builder.block(
			anchors,
			std::mem::replace(&mut block.kind, BlockKind::Rule),
		);
		self.origins.insert(block.id(), origin);
	}
}

fn local_fragment(
	url: &str,
	source: &Source<'_>,
	selected: &HashSet<BlockId>,
) -> Option<String> {
	let fragment = percent_encoding::percent_decode_str(url.strip_prefix('#')?)
		.decode_utf8()
		.ok()?;
	let anchor = source.document.resolve_anchor(fragment.as_ref());
	source
		.document
		.anchor_block(&anchor)
		.filter(|block| selected.contains(&block.id()))?;
	anchor.fragment().map(str::to_owned)
}

fn local_link_bytes(
	blocks: &[&Block],
	source: &Source<'_>,
	scope: &Scope,
) -> usize {
	let mut fragments = HashSet::new();
	for block in blocks {
		visit_fields(
			block,
			&mut Vec::new(),
			&|| false,
			&mut |_, _, field, _| {
				if let FieldText::Rich(inlines) = field {
					for link in
						inlines.iter().filter_map(|inline| inline.link.as_ref())
					{
						if let Some(fragment) = local_fragment(
							link.as_str(),
							source,
							&scope.selected,
						) {
							fragments.insert(fragment);
						}
					}
				}
			},
		);
	}
	let encoded_len = |part: &str| {
		percent_encoding::utf8_percent_encode(
			part,
			percent_encoding::NON_ALPHANUMERIC,
		)
		.map(str::len)
		.sum::<usize>()
	};
	let prefix_len = 1 + encoded_len(&scope.prefix);
	fragments
		.iter()
		.map(|fragment| prefix_len + encoded_len(fragment))
		.sum()
}

fn charge(remaining: &mut DocumentLimits, selected: &[&Block]) -> bool {
	let mut blocks = 0;
	let mut text = 0;
	let mut cells = 0;
	for root in selected {
		visit_blocks(root, &mut |block| {
			blocks += 1;
			text += block
				.anchors()
				.iter()
				.map(|anchor| 2 * anchor.len())
				.sum::<usize>();
			text += match block.kind() {
				BlockKind::Heading { anchor, .. } => 2 * anchor.len(),
				BlockKind::Footnote { id, .. } => 2 * id.as_str().len(),
				BlockKind::Code { language, .. } => language.len(),
				BlockKind::Quote { callout, .. } => {
					callout.as_ref().map_or(0, |callout| callout.name.len())
				}
				_ => 0,
			};
			if let BlockKind::Table(table) = block.kind() {
				cells += table
					.groups
					.iter()
					.flat_map(|group| &group.rows)
					.map(|row| row.cells.len())
					.sum::<usize>();
			}
		});
		visit_fields(
			root,
			&mut Vec::new(),
			&|| false,
			&mut |_, _, field, _| {
				text += field.len();
				if let FieldText::Rich(inlines) = field {
					text += inlines
						.iter()
						.filter_map(|inline| match &inline.kind {
							InlineKind::Image(image) => {
								Some(image.address().len() + image.title.len())
							}
							InlineKind::Anchor(anchor) => {
								Some(2 * anchor.len())
							}
							InlineKind::FootnoteRef(note) => {
								Some(2 * note.definition.as_str().len())
							}
							_ => None,
						})
						.sum::<usize>();
				}
			},
		);
	}
	if blocks > remaining.max_blocks
		|| text > remaining.max_text_bytes
		|| cells > remaining.max_table_cells
	{
		return false;
	}
	remaining.max_blocks -= blocks;
	remaining.max_text_bytes -= text;
	remaining.max_table_cells -= cells;
	true
}

impl Expanded {
	/// Applies link addresses already resolved by the caller.
	pub fn replace_link_addresses(
		&mut self,
		addresses: &HashMap<LinkOccurrenceId, Link>,
	) {
		if addresses.is_empty() {
			return;
		}
		for block in &mut self.document.blocks {
			map_links(block, &mut |link| {
				if let Some(address) = addresses.get(&link.occurrence()) {
					*link = address.with_occurrence(link.occurrence());
				}
			});
		}
		let revision = self.document.revision();
		let blocks = std::mem::take(&mut self.document.blocks);
		self.document = DocumentBuilder::new(revision).finish(blocks);
		self.document
			.index
			.outline
			.retain(|entry| !self.origins.contains_key(&entry.block));
		set_revision(&mut self.document, revision);
	}
}

fn set_revision(document: &mut Document, base: DocumentRevision) {
	document.revision = DocumentRevision::new(
		crate::fingerprint(&(base, document.content_key())) | (1 << 63),
	);
}

fn map_links(block: &mut Block, map: &mut impl FnMut(&mut Link)) {
	for_rich_field(&mut block.kind, |text| {
		for link in text.iter_mut().filter_map(|inline| inline.link.as_mut()) {
			map(link);
		}
	});
	for_child_list(&mut block.kind, |children| {
		for child in children {
			map_links(child, map);
		}
	});
}

fn for_rich_field(kind: &mut BlockKind, mut visit: impl FnMut(&mut RichText)) {
	match kind {
		BlockKind::Paragraph(text)
		| BlockKind::Heading { text, .. }
		| BlockKind::Details { summary: text, .. } => visit(text),
		BlockKind::Quote {
			callout: Some(callout),
			..
		} => callout.title.iter_mut().for_each(visit),
		BlockKind::Table(table) => table.caption.iter_mut().for_each(visit),
		BlockKind::Figure { caption, .. } => caption.iter_mut().for_each(visit),
		BlockKind::DefinitionList(items) => items
			.iter_mut()
			.flat_map(|item| &mut item.terms)
			.for_each(visit),
		_ => {}
	}
}

fn for_child_list(
	kind: &mut BlockKind,
	mut visit: impl FnMut(&mut Vec<Block>),
) {
	match kind {
		BlockKind::Quote { blocks, .. }
		| BlockKind::Footnote { blocks, .. }
		| BlockKind::Details { blocks, .. }
		| BlockKind::FrontMatter { blocks, .. }
		| BlockKind::Figure { blocks, .. } => visit(blocks),
		BlockKind::List { items, .. } => items
			.iter_mut()
			.map(|item| &mut item.blocks)
			.for_each(visit),
		BlockKind::Table(table) => table
			.groups
			.iter_mut()
			.flat_map(|group| &mut group.rows)
			.flat_map(|row| &mut row.cells)
			.map(|cell| &mut cell.blocks)
			.for_each(visit),
		BlockKind::DefinitionList(items) => items
			.iter_mut()
			.flat_map(|item| &mut item.definitions)
			.for_each(visit),
		_ => {}
	}
}

#[cfg(test)]
mod tests;
