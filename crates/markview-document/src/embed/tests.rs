use super::*;
use crate::{FootnoteReference, Inline, inline_link_origins, plain_text};
use proptest::prelude::*;

fn inline(kind: InlineKind) -> Inline {
	Inline {
		kind,
		link: None,
		style: Default::default(),
	}
}

fn text(value: &str) -> RichText {
	vec![inline(InlineKind::Text(value.into()))]
}

fn paragraph(value: &str) -> BlockKind {
	BlockKind::Paragraph(text(value))
}

fn heading(name: &str, level: u8) -> BlockKind {
	BlockKind::Heading {
		level,
		text: text(name),
		anchor: name.to_lowercase(),
	}
}

fn document(kinds: impl IntoIterator<Item = BlockKind>) -> Document {
	let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
	let blocks = kinds
		.into_iter()
		.map(|kind| builder.block(Vec::new(), kind))
		.collect();
	builder.finish(blocks)
}

fn request(
	builder: &mut DocumentBuilder,
	note: &str,
	selector: Selector,
) -> Block {
	let block = builder.block(Vec::new(), paragraph("fallback"));
	block.with_note_embed(NoteEmbed {
		note: note.into(),
		selector,
	})
}

fn target(from: u32, note: &str) -> Request {
	Request {
		from: SourceId(from),
		note: note.into(),
	}
}

fn source(id: u32, document: &Document) -> Source<'_> {
	Source {
		id: SourceId(id),
		document,
	}
}

fn ready(
	root: &Source<'_>,
	targets: &HashMap<Request, Target<'_>>,
) -> Expanded {
	let Expansion::Ready(expanded) = expand(root, targets) else {
		panic!("unexpected lookup");
	};
	*expanded
}

fn needs(
	root: &Source<'_>,
	targets: &HashMap<Request, Target<'_>>,
) -> Vec<Request> {
	let Expansion::NeedsTargets(requests) = expand(root, targets) else {
		panic!("expected a lookup");
	};
	requests
}

fn reading(document: &Document) -> Vec<String> {
	let mut fields = Vec::new();
	for root in document.blocks() {
		visit_fields(
			root,
			&mut Vec::new(),
			&|| false,
			&mut |_, _, field, _| {
				if let FieldText::Rich(text) = field {
					fields.push(plain_text(text));
				}
			},
		);
	}
	fields
}

#[test]
fn selects_sections_and_requests_only_reachable_notes() {
	let mut b = DocumentBuilder::new(DocumentRevision::new(1));
	let include =
		request(&mut b, "B", Selector::Section("parent#child".into()));
	let a = b.finish(vec![include]);
	let mut b = DocumentBuilder::new(DocumentRevision::new(1));
	let outside =
		request(&mut b, "Outside", Selector::Section("unused".into()));
	let parent = b.block(Vec::new(), heading("Parent", 1));
	let child = b.block(Vec::new(), heading("Child", 2));
	let deeper = b.block(Vec::new(), heading("Deeper", 3));
	let inside = request(&mut b, "C", Selector::Block("selected".into()));
	let after = b.block(Vec::new(), heading("After", 2));
	let b = b.finish(vec![outside, parent, child, deeper, inside, after]);
	let mut c = DocumentBuilder::new(DocumentRevision::new(1));
	let selected =
		c.block(vec!["^selected".into()], paragraph("selected paragraph"));
	let c = c.finish(vec![selected]);

	let a = source(0, &a);
	let mut loaded = HashMap::new();
	assert_eq!(needs(&a, &loaded), [target(0, "B")]);
	loaded.insert(target(0, "B"), Target::Loaded(source(1, &b)));
	assert_eq!(needs(&a, &loaded), [target(1, "C")]);
	loaded.insert(target(1, "C"), Target::Loaded(source(2, &c)));
	let expanded = ready(&a, &loaded);
	assert_eq!(
		reading(&expanded.document),
		["Child", "Deeper", "selected paragraph"]
	);
	assert!(expanded.document.outline_entries().is_empty());
	assert_eq!(
		expanded.origins[&expanded.document.blocks()[2].id()].source,
		SourceId(2)
	);
}

#[test]
fn same_file_sections_allow_disjoint_embeds_and_stop_cycles() {
	let mut b = DocumentBuilder::new(DocumentRevision::new(1));
	let top = request(&mut b, "", Selector::Section("a".into()));
	let a = b.block(Vec::new(), heading("A", 1));
	let to_b = request(&mut b, "", Selector::Section("b".into()));
	let b_heading = b.block(Vec::new(), heading("B", 1));
	let to_a = request(&mut b, "", Selector::Section("a".into()));
	let root = b.finish(vec![top, a, to_b, b_heading, to_a]);

	let expanded = ready(&source(0, &root), &HashMap::new());
	assert_eq!(&reading(&expanded.document)[..3], ["A", "B", "fallback"]);
	assert_eq!(expanded.document.outline_entries(), root.outline_entries());
	assert_eq!(
		expanded.document.anchor_block("b").unwrap().id(),
		root.blocks()[3].id()
	);
}

#[test]
fn excessive_expansions_fall_back_in_place() {
	let mut documents = Vec::new();
	for depth in 0..=MAX_DEPTH + 1 {
		let mut b = DocumentBuilder::new(DocumentRevision::new(1));
		let heading = b.block(Vec::new(), heading("Section", 1));
		let include =
			request(&mut b, "next", Selector::Section("section".into()));
		let label = b.block(Vec::new(), paragraph(&format!("level {depth}")));
		documents.push(b.finish(vec![heading, label, include]));
	}
	let targets = documents
		.iter()
		.enumerate()
		.skip(1)
		.map(|(depth, doc)| {
			(
				target((depth - 1) as u32, "next"),
				Target::Loaded(source(depth as u32, doc)),
			)
		})
		.collect();
	let expanded = ready(&source(0, &documents[0]), &targets);
	let fields = reading(&expanded.document);
	assert!(fields.contains(&format!("level {MAX_DEPTH}")));
	assert!(!fields.contains(&format!("level {}", MAX_DEPTH + 1)));
	assert_eq!(fields.last().unwrap(), "fallback");
}

proptest! {
	#[test]
	fn repeated_fragments_own_distinct_links_anchors_and_footnotes(repeats in 1usize..18) {
		let mut b = DocumentBuilder::new(DocumentRevision::new(1));
		let blocks = (0..repeats).map(|_| request(&mut b, "B", Selector::Section("selected".into()))).collect();
		let root = b.finish(blocks);
		let note = FootnoteId::new("n");
		let mut contents = text("in-section link ");
		contents[0].link = Some(Link::new("#selected", LinkOccurrenceId::new(1).unwrap()));
		contents.push(inline(InlineKind::FootnoteRef(FootnoteReference {
			definition: note.clone(),
			number: 7,
			occurrence: LinkOccurrenceId::new(2).unwrap(),
		})));
		let mut b = DocumentBuilder::new(DocumentRevision::new(1));
		let title = b.block(Vec::new(), heading("Selected", 1));
		let body = b.block(Vec::new(), BlockKind::Paragraph(contents));
		let outside = b.block(Vec::new(), heading("Outside", 1));
		let definition = b.block(Vec::new(), paragraph("definition"));
		let definition = b.block(Vec::new(), BlockKind::Footnote { id: note, number: 7, blocks: vec![definition] });
		let leaf = b.finish(vec![title, body, outside, definition]);

		let expanded = ready(&source(0, &root), &HashMap::from([(target(0, "B"), Target::Loaded(source(1, &leaf)))]));
		let mut ids = HashSet::new();
		let mut anchors = HashSet::new();
		for blocks in expanded.document.blocks().as_chunks::<3>().0 {
			for block in blocks {
				prop_assert!(ids.insert(block.id()));
			}
			let BlockKind::Heading { anchor, .. } = blocks[0].kind() else { panic!("missing heading"); };
			prop_assert!(anchors.insert(anchor));
			let BlockKind::Paragraph(text) = blocks[1].kind() else { panic!("missing body"); };
			let decoded = percent_encoding::percent_decode_str(text[0].link_url().unwrap().strip_prefix('#').unwrap()).decode_utf8().unwrap();
			prop_assert_eq!(decoded.as_ref(), anchor);
			prop_assert_eq!(expanded.document.anchor_block(anchor).unwrap().id(), blocks[0].id());
			let InlineKind::FootnoteRef(reference) = &text[1].kind else { panic!("missing footnote reference"); };
			prop_assert_eq!(reference.number, 7);
			prop_assert_eq!(expanded.document.anchor_block(AnchorId::FootnoteDefinition(reference.definition.clone())).unwrap().id(), blocks[2].id());
			prop_assert_eq!(expanded.origins[&blocks[1].id()].block, leaf.blocks()[1].id());
		}
		let unique: HashSet<_> = expanded.document.blocks().iter().flat_map(|b| {
			if let BlockKind::Paragraph(text) = b.kind() { inline_link_origins(text) } else { Vec::new() }
		}).collect();
		prop_assert_eq!(unique.len(), repeats * 2);
		prop_assert_eq!(expanded.document.blocks().len(), repeats * 3);
	}
}

#[test]
fn nested_heading_range_stays_inside_its_list_item() {
	let mut b = DocumentBuilder::new(DocumentRevision::new(1));
	let title = b.block(Vec::new(), heading("Nested", 2));
	let body = b.block(Vec::new(), paragraph("Included"));
	let other = b.block(Vec::new(), paragraph("Different list item"));
	let list = b.block(
		Vec::new(),
		BlockKind::List {
			start: None,
			reversed: false,
			tight: false,
			items: vec![
				crate::ListItem {
					checked: None,
					number: None,
					blocks: vec![title, body],
				},
				crate::ListItem {
					checked: None,
					number: None,
					blocks: vec![other],
				},
			],
		},
	);
	let include = request(&mut b, "", Selector::Section("nested".into()));
	let root = b.finish(vec![include, list]);
	let expanded = ready(&source(0, &root), &HashMap::new());
	assert_eq!(
		reading(&expanded.document),
		[
			"Nested",
			"Included",
			"Nested",
			"Included",
			"Different list item"
		]
	);
}

#[test]
fn repeated_end_sections_use_the_indexed_start_block() {
	let counts = [2_000, 4_000];
	let measurements = counts.map(|count| {
		let mut b = DocumentBuilder::new(DocumentRevision::new(1));
		let mut blocks: Vec<_> = (0..count)
			.map(|_| request(&mut b, "", Selector::Section("End".into())))
			.collect();
		blocks.push(b.block(Vec::new(), heading("End", 1)));
		let root = b.try_finish(blocks).unwrap();
		SECTION_START_PROBES.with(|probes| probes.set(0));
		let expanded = ready(&source(0, &root), &HashMap::new());
		assert_eq!(expanded.origins.len(), count);
		SECTION_START_PROBES.with(std::cell::Cell::get)
	});
	assert!(
		measurements
			.into_iter()
			.zip(counts)
			.all(|(probes, count)| (1..=count).contains(&probes)),
		"{measurements:?}"
	);
}

#[test]
fn repeated_heading_embeds_resolve_the_title_once() {
	assert_eq!(heading_title_probes(false), [2_000, 4_000]);
}

#[test]
fn distinct_heading_embeds_build_the_title_index_once() {
	assert_eq!(heading_title_probes(true), [2_000, 4_000]);
}

fn heading_title_probes(distinct: bool) -> [usize; 2] {
	let counts = [2_000, 4_000];
	counts.map(|count| {
		let mut b = DocumentBuilder::new(DocumentRevision::new(1));
		let headings = (0..count)
			.map(|n| b.block(Vec::new(), heading(&format!("Topic {n}"), 1)))
			.collect();
		let leaf = b.try_finish(headings).unwrap();
		let mut b = DocumentBuilder::new(DocumentRevision::new(2));
		let blocks = (0..count)
			.map(|index| {
				let index = if distinct { index } else { count - 1 };
				request(
					&mut b,
					"B",
					Selector::Section(format!("Topic {index}")),
				)
			})
			.collect();
		let root = b.try_finish(blocks).unwrap();
		let targets =
			HashMap::from([(target(0, "B"), Target::Loaded(source(1, &leaf)))]);
		HEADING_TITLE_PROBES.with(|probes| probes.set(0));
		let expanded = ready(&source(0, &root), &targets);
		assert_eq!(expanded.origins.len(), count);
		HEADING_TITLE_PROBES.with(std::cell::Cell::get)
	})
}

#[test]
fn distinct_list_item_embeds_use_indexed_item_positions() {
	use crate::index::ITEM_LOOKUP_PROBES;

	for ordered in [false, true] {
		let measurements = [1024, 2048].map(|count| {
			ITEM_LOOKUP_PROBES.with(|probes| probes.set(0));
			let mut b = DocumentBuilder::new(DocumentRevision::new(1));
			let items = (0..count).map(|index| {
				let mut block = b.block(Vec::new(), paragraph(&format!("Item {index}")));
				block.add_list_item_anchor(format!("^item-{index}"));
				ListItem { checked: None, number: None, blocks: vec![block] }
			}).collect();
			let list = b.block(Vec::new(), BlockKind::List {
				start: ordered.then_some(4), reversed: false, tight: true, items,
			});
			let leaf = b.try_finish(vec![list]).unwrap();
			let mut b = DocumentBuilder::new(DocumentRevision::new(2));
			let blocks = (0..count)
				.map(|index| request(&mut b, "B", Selector::Block(format!("item-{index}"))))
				.collect();
			let root = b.try_finish(blocks).unwrap();
			let targets = HashMap::from([(target(0, "B"), Target::Loaded(source(1, &leaf)))]);
			let expanded = ready(&source(0, &root), &targets);
			assert_eq!(expanded.document.blocks().len(), count);
			for (index, block) in expanded.document.blocks().iter().enumerate() {
				let BlockKind::List { start, items, .. } = block.kind() else {
					panic!("item did not retain its marker");
				};
				assert_eq!(*start, ordered.then_some(index as isize + 4));
				assert_eq!(items.len(), 1);
				assert!(matches!(
					items[0].blocks[0].kind(), BlockKind::Paragraph(text) if plain_text(text) == format!("Item {index}")
				));
			}
			ITEM_LOOKUP_PROBES.with(std::cell::Cell::get)
		});
		assert!(
			measurements[0] <= 3 * 1024 && measurements[1] <= 3 * 2048,
			"{measurements:?}"
		);
	}
}

#[test]
fn exhausted_embed_budget_does_not_rescan_the_same_fragment() {
	let count = DocumentLimits::default().max_blocks / 2 + 1;
	let mut b = DocumentBuilder::new(DocumentRevision::new(1));
	let mut blocks = vec![b.block(Vec::new(), heading("Large", 1))];
	blocks.extend((1..count).map(|_| b.block(Vec::new(), paragraph("Body"))));
	let leaf = b.try_finish(blocks).unwrap();
	let targets =
		HashMap::from([(target(0, "B"), Target::Loaded(source(1, &leaf)))]);
	let measurements = [32, 64].map(|repeats| {
		let mut b = DocumentBuilder::new(DocumentRevision::new(2));
		let blocks = (0..repeats)
			.map(|_| request(&mut b, "B", Selector::Section("Large".into())))
			.collect();
		let root = b.try_finish(blocks).unwrap();
		FRAGMENT_ROOT_PROBES.with(|probes| probes.set(0));
		let expanded = ready(&source(0, &root), &targets);
		assert_eq!(expanded.origins.len(), count);
		assert_eq!(expanded.document.blocks().len(), count + repeats - 1);
		FRAGMENT_ROOT_PROBES.with(std::cell::Cell::get)
	});
	assert!(
		measurements.into_iter().all(|probes| probes <= 2 * count),
		"{measurements:?}"
	);
}

#[test]
fn expansion_budgets_percent_encoded_local_addresses() {
	let anchor = "-".repeat(64 * 1024);
	let link =
		Link::new(format!("#{anchor}"), LinkOccurrenceId::new(1).unwrap());
	let mut b = DocumentBuilder::new(DocumentRevision::new(1));
	let title = b.block(Vec::new(), heading("Section", 1));
	let body = b.block(
		vec![anchor],
		BlockKind::Paragraph(vec![Inline {
			link: Some(link),
			..inline(InlineKind::Text("link".into()))
		}]),
	);
	let leaf = b.try_finish(vec![title, body]).unwrap();
	let mut b = DocumentBuilder::new(DocumentRevision::new(1));
	let blocks = (0..250)
		.map(|_| request(&mut b, "B", Selector::Section("Section".into())))
		.collect();
	let root = b.try_finish(blocks).unwrap();
	let targets =
		HashMap::from([(target(0, "B"), Target::Loaded(source(1, &leaf)))]);
	let expanded = ready(&source(0, &root), &targets);
	let mut addresses = HashMap::new();
	for block in expanded.document.blocks() {
		if let BlockKind::Paragraph(inlines) = block.kind()
			&& let Some(link) = &inlines[0].link
		{
			let address = link.as_str().strip_prefix('#').unwrap();
			let decoded = percent_encoding::percent_decode_str(address)
				.decode_utf8()
				.unwrap();
			assert_eq!(
				expanded
					.document
					.anchor_block(decoded.as_ref())
					.unwrap()
					.id(),
				block.id()
			);
			addresses.insert(link.shared().as_ptr(), link.as_str().len());
		}
	}
	assert!(!addresses.is_empty());
	let bytes = addresses.values().sum::<usize>();
	assert!(
		bytes <= DocumentLimits::default().max_text_bytes,
		"retained {bytes} bytes of encoded local link addresses"
	);
}

fn in_quotes(builder: &mut DocumentBuilder, mut block: Block) -> Block {
	for _ in 0..130 {
		block = builder.block(
			Vec::new(),
			BlockKind::Quote {
				callout: None,
				blocks: vec![block],
			},
		);
	}
	block
}

#[test]
fn an_over_depth_embed_does_not_discard_shallow_embeds() {
	let mut b = DocumentBuilder::new(DocumentRevision::new(1));
	let deep_request =
		request(&mut b, "Deep", Selector::Section("Deep".into()));
	let quotes = in_quotes(&mut b, deep_request);
	let shallow_request =
		request(&mut b, "Shallow", Selector::Section("Shallow".into()));
	let same_section_at_root =
		request(&mut b, "Deep", Selector::Section("Deep".into()));
	let root = b
		.try_finish(vec![quotes, shallow_request, same_section_at_root])
		.unwrap();
	let mut b = DocumentBuilder::new(DocumentRevision::new(1));
	let title = b.block(Vec::new(), heading("Deep", 1));
	let paragraph = b.block(Vec::new(), paragraph("Too deep marker"));
	let quotes = in_quotes(&mut b, paragraph);
	let deep = b.try_finish(vec![title, quotes]).unwrap();
	let shallow = document([heading("Shallow", 1)]);
	let targets = HashMap::from([
		(target(0, "Deep"), Target::Loaded(source(1, &deep))),
		(target(0, "Shallow"), Target::Loaded(source(2, &shallow))),
	]);
	let expanded = ready(&source(0, &root), &targets);
	assert_eq!(
		reading(&expanded.document),
		["fallback", "Shallow", "Deep", "Too deep marker"]
	);
}

#[test]
fn rewritten_local_links_share_the_address_across_runs() {
	let anchor = "x".repeat(64 * 1024);
	let url = format!("#{anchor}");
	let link = Link::new(url.as_str(), LinkOccurrenceId::new(1).unwrap());
	let label = Inline {
		link: Some(link),
		..inline(InlineKind::Text("link".into()))
	};
	let mut b = DocumentBuilder::new(DocumentRevision::new(1));
	let heading = b.block(Vec::new(), heading("Section", 1));
	let body = b.block(vec![anchor], BlockKind::Paragraph(vec![label; 32]));
	let leaf = b.finish(vec![heading, body]);
	let mut b = DocumentBuilder::new(DocumentRevision::new(1));
	let blocks = (0..20)
		.map(|_| request(&mut b, "B", Selector::Section("Section".into())))
		.collect();
	let root = b.finish(blocks);
	let targets =
		HashMap::from([(target(0, "B"), Target::Loaded(source(1, &leaf)))]);
	let expanded = ready(&source(0, &root), &targets);
	assert_eq!(expanded.document.blocks().len(), 40);
	let mut addresses = HashMap::new();
	for block in expanded.document.blocks() {
		if let BlockKind::Paragraph(inlines) = block.kind() {
			for link in inlines.iter().filter_map(|inline| inline.link.as_ref())
			{
				assert_ne!(link.as_str(), url);
				addresses.insert(link.shared().as_ptr(), link.as_str().len());
			}
		}
	}
	let bytes = addresses.values().sum::<usize>();
	assert!(
		bytes <= DocumentLimits::default().max_text_bytes,
		"retained {bytes} bytes of rewritten local link addresses"
	);
}

#[test]
fn compound_heading_prefers_the_exact_path_when_normalized_names_collide() {
	let leaf = document([
		heading("Parent?", 1),
		heading("Shared", 2),
		paragraph("Wrong marker"),
		heading("Parent!", 1),
		heading("Shared", 2),
		paragraph("Right marker"),
	]);
	let mut b = DocumentBuilder::new(DocumentRevision::new(1));
	let include =
		request(&mut b, "B", Selector::Section("Parent!#Shared".into()));
	let root = b.finish(vec![include]);
	let targets =
		HashMap::from([(target(0, "B"), Target::Loaded(source(1, &leaf)))]);
	let expanded = ready(&source(0, &root), &targets);
	assert_eq!(reading(&expanded.document), ["Shared", "Right marker"]);
}

#[test]
fn non_heading_fragment_cannot_embed_a_section() {
	let body = document([paragraph("plain paragraph")]);
	let mut b = DocumentBuilder::new(DocumentRevision::new(1));
	let include = request(&mut b, "B", Selector::Section("missing".into()));
	let root = b.finish(vec![include]);
	let expanded = ready(
		&source(0, &root),
		&HashMap::from([(target(0, "B"), Target::Loaded(source(1, &body)))]),
	);
	assert_eq!(reading(&expanded.document), ["fallback"]);
}
