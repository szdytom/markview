use super::*;
use crate::settings::ExportSettings;

fn parse(text: &str) -> Arc<LoadedDocument> {
	LoadedDocument::markdown(Arc::new(markview_commonmark::parse_with(
		text.to_owned(),
		Dialect {
			ofm: true,
			..Default::default()
		},
	)))
}

fn text(document: &Document) -> String {
	let mut text = String::new();
	for block in document.blocks() {
		markview_document::visit_fields(
			block,
			&mut Vec::new(),
			&|| false,
			&mut |_, _, field, _| {
				if let markview_document::FieldText::Rich(field) = field {
					text.push_str(&markview_document::plain_text(field));
					text.push('\n');
				}
			},
		);
	}
	text
}

fn embed_from_b(root: &Path, body: &str, host: &str) -> Arc<LoadedDocument> {
	fs::write(root.join("B.md"), body).unwrap();
	Loader::default()
		.prepare(
			parse(host),
			InputPath::File(&root.join("A.md")),
			None,
			&Security::local(Trust::Trusted),
			|| false,
		)
		.unwrap()
}

#[test]
fn note_embed_shares_rebased_link_addresses_across_occurrences() {
	use markview_document::BlockKind;

	let dir = crate::test_support::TestDir::new();
	let root = dir.path();
	let query = "x".repeat(64 * 1024);
	let body = format!("# Section\n\n[*la*bel](relative.md?q={query})");
	let source = parse(&body);
	let BlockKind::Paragraph(inlines) = source.blocks()[1].kind() else {
		panic!("source link paragraph missing");
	};
	assert_eq!(
		inlines[0].link.as_ref().unwrap().as_str(),
		format!("relative.md?q={query}")
	);
	let relative = root.join("relative.md");
	fs::write(&relative, "Destination").unwrap();
	let count = 513;
	let expanded =
		embed_from_b(root, &body, &"![[B#Section]]\n\n".repeat(count));
	let destination =
		Url::from_file_path(relative.canonicalize().unwrap()).unwrap();
	let expected = format!("{destination}?q={query}");
	let mut addresses = HashMap::new();
	let mut occurrences = HashSet::new();
	for block in expanded.blocks() {
		if let BlockKind::Paragraph(inlines) = block.kind() {
			for link in inlines.iter().filter_map(|inline| inline.link.as_ref())
			{
				assert_eq!(link.as_str(), expected);
				addresses.insert(link.shared().as_ptr(), link.as_str().len());
				occurrences.insert(link.occurrence());
			}
		}
	}
	assert_eq!(occurrences.len(), count);
	let bytes = addresses.values().sum::<usize>();
	assert!(
		bytes <= markview_document::DocumentLimits::default().max_text_bytes,
		"retained {bytes} bytes of rebased link addresses"
	);
}

#[test]
fn note_embed_includes_all_paragraphs_of_an_anchored_list_item() {
	use markview_document::BlockKind;

	let dir = crate::test_support::TestDir::new();
	let body =
		"- First item lead\n\n  First item tail ^pick\n\n- Second item marker";
	let source = parse(body);
	let BlockKind::List { items, .. } = source.blocks()[0].kind() else {
		panic!("source list missing");
	};
	assert_eq!(items.len(), 2);
	assert_eq!(items[0].blocks.len(), 2);
	assert_eq!(
		source.anchor_block("^pick").unwrap().id(),
		items[0].blocks[0].id()
	);
	let expanded = embed_from_b(dir.path(), body, "![[B#^pick]]");
	let reading = text(&expanded);
	assert!(reading.contains("First item lead"), "{reading}");
	assert!(reading.contains("First item tail"), "{reading}");
	assert!(!reading.contains("Second item marker"), "{reading}");
}

#[test]
fn note_embed_selects_an_entire_list_by_its_standalone_id() {
	use markview_document::BlockKind;

	let dir = crate::test_support::TestDir::new();
	let body = "4. Whole first\n5. Whole second\n\n^whole\n\nOutside marker";
	let source = parse(body);
	let reading = text(&source);
	assert!(reading.contains("Outside marker"));
	assert!(
		matches!(
			source.anchor_block("^whole").map(|block| block.kind()),
			Some(BlockKind::List { start: Some(4), items, .. }) if items.len() == 2
		),
		"the source ID must point at the ordered list"
	);
	assert!(!reading.contains("^whole"));
	let expanded = embed_from_b(dir.path(), body, "![[B#^whole]]");
	assert!(matches!(
		expanded.blocks()[0].kind(),
		BlockKind::List { start: Some(4), items, .. } if items.len() == 2
	));
	assert_eq!(text(&expanded), "Whole first\nWhole second\n");
}

#[test]
fn note_embed_an_item_preserves_its_number_and_task_state() {
	use markview_document::BlockKind;

	let dir = crate::test_support::TestDir::new();
	let body = "4. First item\n5. [x] Selected task ^pick\n6. Last item";
	let source = parse(body);
	assert!(matches!(
		source.blocks()[0].kind(),
		BlockKind::List { start: Some(4), items, .. } if items[1].checked == Some(true)
			&& Some(items[1].blocks[0].id()) == source.anchor_block("^pick").map(|block| block.id())
	));
	let expanded = embed_from_b(dir.path(), body, "![[B#^pick]]");
	assert_eq!(text(&expanded), "Selected task\n");
	assert!(matches!(
		expanded.blocks()[0].kind(),
		BlockKind::List { start: Some(5), items, .. } if items.len() == 1 && items[0].checked == Some(true)
	));
}

#[test]
fn note_embed_opening_bullet_id_preserves_the_nested_list() {
	use markview_document::BlockKind;

	let dir = crate::test_support::TestDir::new();
	let body = "- Parent ^pick\n  - Child\n- Sibling";
	let source = parse(body);
	assert!(matches!(
		source.blocks()[0].kind(),
		BlockKind::List { items, .. } if items.len() == 2
			&& matches!(items[0].blocks[1].kind(), BlockKind::List { .. })
	));
	let expanded = embed_from_b(dir.path(), body, "![[B#^pick]]");
	let reading = text(&expanded);
	assert_eq!(reading, "Parent\nChild\n");
	assert!(
		matches!(expanded.blocks()[0].kind(), BlockKind::List { items, .. } if items.len() == 1)
	);
}

#[test]
fn note_embed_section_can_embed_its_heading_by_block_id() {
	let dir = crate::test_support::TestDir::new();
	let body = "# Section ^title\n\n![[#^title]]\n\nEnd marker";
	let direct = Loader::default()
		.prepare(
			parse(body),
			InputPath::File(&dir.path().join("B.md")),
			None,
			&Security::local(Trust::Trusted),
			|| false,
		)
		.unwrap();
	let expected = "Section\nSection\nEnd marker\n";
	assert_eq!(text(&direct), expected);
	let expanded = embed_from_b(dir.path(), body, "![[B#Section]]");
	assert_eq!(text(&expanded), expected);
}

#[test]
fn note_embed_includes_content_after_a_standalone_item_id() {
	let dir = crate::test_support::TestDir::new();
	let body = "- First\n\n  ^pick\n\n  Continuation\n\n- Neighbor";
	let source = parse(body);
	let markview_document::BlockKind::List { items, .. } =
		source.blocks()[0].kind()
	else {
		panic!("expected the source list");
	};
	assert_eq!(items.len(), 2);
	assert!(items[0].blocks.iter().any(|block| matches!(
		block.kind(), markview_document::BlockKind::Paragraph(text)
			if markview_document::plain_text(text) == "Continuation"
	)));
	let expanded = embed_from_b(dir.path(), body, "![[B#^pick]]");
	assert_eq!(text(&expanded), "First\nContinuation\n");
	assert!(!text(&source).contains("^pick"));
}

#[test]
fn note_embed_counts_custom_callout_names_towards_the_copy_budget() {
	use markview_document::BlockKind;
	let dir = crate::test_support::TestDir::new();
	let length = 256 * 1024;
	let body = format!(
		"# Section\n\n> [!{}] Short title\n> Payload",
		"X".repeat(length)
	);
	let source = parse(&body);
	let BlockKind::Quote {
		callout: Some(callout),
		..
	} = source.blocks()[1].kind()
	else {
		panic!("expected custom callout");
	};
	assert_eq!(callout.name.len(), length);
	assert_eq!(
		markview_document::plain_text(callout.title.as_deref().unwrap()),
		"Short title"
	);
	let expanded =
		embed_from_b(dir.path(), &body, &"![[B#Section]]\n\n".repeat(160));
	let names: HashMap<_, _> = expanded
		.blocks()
		.iter()
		.filter_map(|block| match block.kind() {
			BlockKind::Quote {
				callout: Some(callout),
				..
			} => Some((callout.name.as_ptr(), callout.name.len())),
			_ => None,
		})
		.collect();
	assert!(!names.is_empty());
	let bytes = names.values().sum::<usize>();
	assert!(
		bytes <= markview_document::DocumentLimits::default().max_text_bytes,
		"retained {bytes} bytes of custom callout names"
	);
}

#[test]
fn note_embed_host_anchor_cannot_amplify_imported_namespaces() {
	use markview_document::BlockKind;

	let dir = crate::test_support::TestDir::new();
	let id = format!("mv-embed-{}", "-".repeat(128 * 1024));
	let reserved = "mv-embed-0-1-section";
	let host = format!(
		"Text <em id=\"{id}\">marker</em> <em id=\"{reserved}\">reserved</em>\n\n{}",
		"![[B#Section]]\n\n".repeat(300)
	);
	let source = parse(&host);
	assert!(source.anchor_block(id.as_str()).is_some());
	let expanded = embed_from_b(dir.path(), "# Section", &host);
	assert!(matches!(
		expanded.anchor_block(reserved).unwrap().kind(),
		BlockKind::Paragraph(_)
	));
	let anchors: Vec<_> = expanded
		.blocks()
		.iter()
		.filter_map(|block| {
			if let BlockKind::Heading { anchor, .. } = block.kind() {
				Some(anchor)
			} else {
				None
			}
		})
		.collect();
	assert_eq!(anchors.len(), 300);
	assert!(anchors.iter().all(|anchor| anchor.as_str() != reserved));
	let bytes = anchors.iter().map(|anchor| anchor.len()).sum::<usize>();
	assert!(
		bytes <= markview_document::DocumentLimits::default().max_text_bytes,
		"retained {bytes} bytes in imported heading anchors"
	);
}

#[test]
fn note_embed_indented_id_selects_its_list_item() {
	use markview_document::BlockKind;

	let dir = crate::test_support::TestDir::new();
	let body = "- Inline task\n    $$a+b$$\n    ^inside\n- Sibling item";
	let source = parse(body);
	assert!(
		matches!(source.blocks()[0].kind(), BlockKind::List { items, .. } if items.len() == 2)
	);
	let expanded = embed_from_b(dir.path(), body, "![[B#^inside]]");
	let reading = text(&expanded);
	assert!(reading.contains("Inline task"), "{reading}");
	assert!(!reading.contains("Sibling item"), "{reading}");
	assert!(!reading.contains("^inside"), "{reading}");
}

#[test]
fn note_embed_selects_the_named_heading_when_titles_share_a_slug() {
	let dir = crate::test_support::TestDir::new();
	let body = "# A B\n\nWrong marker\n\n# A-B\n\nRight marker";
	let source = parse(body);
	assert_eq!(source.outline_entries()[0].text, "A B");
	assert_eq!(source.outline_entries()[1].text, "A-B");
	let expanded = embed_from_b(dir.path(), body, "![[B#A-B]]");
	let reading = text(&expanded);
	assert!(reading.contains("Right marker"), "{reading}");
	assert!(!reading.contains("Wrong marker"), "{reading}");
}

#[test]
fn note_embed_tracks_missing_and_workspace_candidates_without_observing_pdf_outputs()
 {
	let dir = crate::test_support::TestDir::new();
	let root = dir.path();
	let origin = root.join("draft.md");
	let source = parse("![[Topic#^leaf]]");
	let security = Security::local(Trust::Trusted);
	let mut loader = Loader::default();
	let prepare = |loader: &mut Loader| {
		loader
			.prepare(
				source.clone(),
				InputPath::File(&origin),
				Some(root),
				&security,
				|| false,
			)
			.unwrap()
	};

	let missing = prepare(&mut loader);
	let dependencies = &missing.composition().unwrap().dependencies;
	assert!(text(&missing).contains("Topic"));
	assert!(!dependencies.changed());
	let generated = root.join("output.pdf");
	fs::write(&generated, "output").unwrap();
	assert!(!dependencies.changed());

	let nested = root.join("topics");
	fs::create_dir(&nested).unwrap();
	let leaf = nested.join("Topic.md");
	fs::write(&leaf, "First copy ^leaf\n").unwrap();
	assert!(dependencies.changed());
	let loaded = prepare(&mut loader);
	assert_eq!(text(&loaded), "First copy\n");
	let path = leaf.canonicalize().unwrap();
	let (file, parse, block) = loaded
		.composition()
		.unwrap()
		.origin(loaded.blocks()[0].id())
		.unwrap();
	assert_eq!(file, path);
	let range = parse
		.block_source(parse.document.block(block).unwrap())
		.unwrap();
	assert!(parse.source[range].starts_with("First copy"));
	assert!(!loaded.composition().unwrap().dependencies.changed());
	assert!(Arc::ptr_eq(&loaded, &prepare(&mut loader)));

	fs::write(&leaf, "A second version ^leaf\n").unwrap();
	assert!(loaded.composition().unwrap().dependencies.changed());
	let changed = prepare(&mut loader);
	assert_eq!(text(&changed), "A second version\n");
	assert_ne!(changed.revision(), loaded.revision());
	assert!(Arc::ptr_eq(
		changed.markdown_parse().unwrap(),
		source.markdown_parse().unwrap()
	));

	let other = root.join("duplicate");
	fs::create_dir(&other).unwrap();
	fs::write(other.join("Topic.md"), "Another target ^leaf").unwrap();
	assert!(changed.composition().unwrap().dependencies.changed());
	let ambiguous = prepare(&mut loader);
	assert!(text(&ambiguous).contains("Topic"));
	fs::remove_dir_all(other).unwrap();
	assert_eq!(text(&prepare(&mut loader)), "A second version\n");
}

#[test]
fn note_embed_requires_local_trust_for_external_files_and_preserves_same_note_targets()
 {
	let dir = crate::test_support::TestDir::new();
	let secret = dir.path().join("Other.md");
	fs::write(&secret, "External marker ^leaf").unwrap();
	let input = "![[Other#^leaf]]\n\n![[#^local]]\n\nLocal marker ^local";
	let source = parse(input);
	let run = |security| {
		Loader::default()
			.prepare(
				source.clone(),
				InputPath::Buffer(&dir.path().join("unsaved.md")),
				None,
				&security,
				|| false,
			)
			.unwrap()
	};
	let mut untrusted = Security::local(Trust::Untrusted);
	untrusted.grant(crate::security::Resource::File(
		secret.canonicalize().unwrap(),
	));
	for security in
		[untrusted, Security::web("https://example.test/note".into())]
	{
		let document = run(security);
		assert!(!text(&document).contains("External marker"));
		assert_eq!(text(&document).matches("Local marker").count(), 2);
		assert!(document.composition().unwrap().dependencies.is_empty());
	}
	let trusted = run(Security::local(Trust::Trusted));
	assert!(text(&trusted).contains("External marker"));
	assert_eq!(trusted.source(), Some(input));
}

#[test]
fn note_embed_native_png_and_pdf_share_sections_source_paths_and_trust() {
	let dir = crate::test_support::TestDir::new();
	let root = dir.path();
	let relative = root.join("host");
	fs::create_dir(&relative).unwrap();
	let host = relative.join("source.md");
	let body = "# Root title\n\n![[Alpha#Selected]]\n\n![[Beta#^pick]]\n\n![[Beta#^whole]]";
	fs::write(&host, body).unwrap();
	for (folder, note, source, color) in [
		(
			"first",
			"Alpha",
			"# Before\n\nOutside before\n\n## Selected\n\nAlpha excerpt. [local](#child) [neighbor](next.md)\n\n![red](same.png)\n\n### Child\n\nInside child\n\n## After\n\nOutside after",
			[255, 0, 0, 255],
		),
		(
			"second",
			"Beta",
			"Beta excerpt ![blue](same.png) ^pick\n\nOutside block\n\n1. Whole first\n2. Whole second\n\n^whole",
			[0, 0, 255, 255],
		),
	] {
		let path = root.join(folder);
		fs::create_dir(&path).unwrap();
		fs::write(path.join(format!("{note}.md")), source).unwrap();
		image::RgbaImage::from_pixel(3, 2, image::Rgba(color))
			.save(path.join("same.png"))
			.unwrap();
	}
	let options = crate::export::Config::from_settings(
		&ExportSettings::default(),
		crate::test_support::fonts(),
		markview_layout::style::CjkType::Sc,
		&[],
	)
	.with_workspace(Some(root.to_owned()))
	.with_dialect(crate::settings::EffectiveDialect {
		hard_breaks: false,
		ofm: true,
		highlight_marks: true,
		grid_tables: true,
		math_dollars_block: true,
	})
	.prepare()
	.unwrap();
	let security = Security::local(Trust::Trusted);
	let services = Arc::new(crate::services::Services::new(4));
	let png = crate::export::png_snapshot_with_services_and_workspace(
		crate::export::PngSource::file(&host),
		options.options.clone(),
		true,
		services.clone(),
		security.clone(),
		Some(root.to_owned()),
	)
	.unwrap();
	let png_text = png.full_reading_text();
	let excerpts = [
		"Alpha excerpt",
		"Inside child",
		"Beta excerpt",
		"Whole first",
		"Whole second",
	];
	for expected in excerpts {
		assert!(png_text.contains(expected), "{png_text}");
	}
	assert!(!png_text.contains("Outside"), "{png_text}");
	assert_eq!(png.images.decoded().len(), 2);
	let urls: Vec<_> = png
		.blocks
		.iter()
		.flat_map(|block| block.links().map(|(link, _)| link.url.as_ref()))
		.collect();
	assert!(
		urls.iter().any(|url| url.contains("/first/next.md")),
		"{urls:?}"
	);
	let local = urls.iter().find_map(|url| url.strip_prefix('#')).unwrap();
	let local = percent_encoding::percent_decode_str(local)
		.decode_utf8()
		.unwrap();
	assert!(png.anchor_y(local.as_ref()).is_some());

	let mut pdf = options.pdf_request(host.clone(), root.join("out.pdf"), true);
	pdf.security = security;
	crate::pdf::export_once(&pdf).unwrap();
	let output = lopdf::Document::load(&pdf.output).unwrap();
	let pdf_text = output.extract_text(&[1]).unwrap();
	for expected in excerpts {
		assert!(pdf_text.contains(expected), "{pdf_text}");
	}
	assert!(!pdf_text.contains("Outside"), "{pdf_text}");

	pdf.path = relative.join("Unsaved.md");
	pdf.security = Security::local(Trust::Untrusted);
	crate::pdf::export_text(&pdf, body, services, || Ok(())).unwrap();
	let pdf_text = lopdf::Document::load(&pdf.output)
		.unwrap()
		.extract_text(&[1])
		.unwrap();
	assert!(!pdf_text.contains("Alpha excerpt"), "{pdf_text}");
	assert!(pdf_text.contains("Alpha"), "{pdf_text}");
}

#[test]
fn note_embed_vault_images_use_the_workspace_or_source_directory() {
	let dir = crate::test_support::TestDir::new();
	let root = dir.path();
	let target = root.join("notes");
	fs::create_dir(&target).unwrap();
	let host = root.join("Host.md");
	fs::write(&host, "![[notes/Target#Section]]").unwrap();
	fs::write(target.join("Target.md"), "# Section\n\n![[picture.png]]")
		.unwrap();
	for (folder, width) in [(root, 6), (target.as_path(), 3)] {
		image::RgbaImage::from_pixel(width, 2, image::Rgba([40, 50, 60, 255]))
			.save(folder.join("picture.png"))
			.unwrap();
	}
	let mut options = crate::test_support::options();
	options.ofm = true;
	let services = Arc::new(crate::services::Services::new(2));
	for (workspace, expected_width) in [(None, 3), (Some(root.to_owned()), 6)] {
		let snapshot = crate::export::png_snapshot_with_services_and_workspace(
			crate::export::PngSource::file(&host),
			options.clone(),
			true,
			services.clone(),
			Security::local(Trust::Trusted),
			workspace,
		)
		.unwrap();
		let images = snapshot.images.decoded();
		assert_eq!(images.len(), 1);
		assert_eq!(images.values().next().unwrap().width, expected_width);
	}
}

#[test]
fn note_embed_wikilinks_refresh_when_a_closer_note_appears() {
	let dir = crate::test_support::TestDir::new();
	let root = dir.path();
	let sub = root.join("sub");
	fs::create_dir(&sub).unwrap();
	let host = root.join("A.md");
	let source = parse("![[sub/B#Section]]");
	fs::write(sub.join("B.md"), "# Section\n\n[[Other]]").unwrap();
	fs::write(root.join("Other.md"), "Workspace copy").unwrap();
	let security = Security::local(Trust::Trusted);
	let mut loader = Loader::default();
	let mut prepare = || {
		loader
			.prepare(
				source.clone(),
				InputPath::File(&host),
				Some(root),
				&security,
				|| false,
			)
			.unwrap()
	};
	let target = |document: &Document| {
		let markview_document::BlockKind::Paragraph(text) =
			document.blocks()[1].kind()
		else {
			panic!("expected the linked paragraph")
		};
		let url = text[0].link_url().unwrap();
		let Some(crate::link::Target::Markdown(path)) =
			crate::link::resolve_for(url, Some(root), &security)
		else {
			panic!("expected a resolved note: {url}")
		};
		path
	};
	let first = prepare();
	assert_eq!(
		target(&first),
		root.join("Other.md").canonicalize().unwrap()
	);
	let closer = sub.join("Other.md");
	fs::write(&closer, "Source directory copy").unwrap();
	let closer = closer.canonicalize().unwrap();
	let direct =
		crate::link::workspace_candidates(Path::new("Other"), &sub, Some(root))
			.find(|path| path.is_file())
			.unwrap()
			.canonicalize()
			.unwrap();
	assert_eq!(direct, closer);
	let updated = prepare();
	assert_eq!(target(&updated), closer);
}

#[test]
fn note_embed_dependency_poll_reuses_supported_directory_metadata() {
	let dir = crate::test_support::TestDir::new();
	let root = dir.path();
	let sub = root.join("sub");
	fs::create_dir(&sub).unwrap();
	fs::write(sub.join("Topic.md"), "# Section").unwrap();
	let prepared = Loader::default()
		.prepare(
			parse("![[Topic#Section]]"),
			InputPath::File(&root.join("A.md")),
			Some(root),
			&Security::local(Trust::Trusted),
			|| false,
		)
		.unwrap();
	assert!(text(&prepared).contains("Section"));
	let dependencies = &prepared.composition().unwrap().dependencies;
	assert!(!dependencies.changed());
	let output = root.join("output.pdf");
	fs::write(&output, "unrelated export").unwrap();
	let stamp = dependencies.directories[root].stamp.as_ref();
	if let Some(stamp) = stamp {
		*stamp.lock().unwrap() = file_version(&output).unwrap();
	}

	DIRECTORY_READS.with(|reads| reads.set(0));
	for _ in 0..4 {
		assert!(!dependencies.changed());
	}
	let expected = if stamp.is_some() {
		1
	} else {
		4 * dependencies.directories.len()
	};
	assert_eq!(DIRECTORY_READS.with(|reads| reads.get()), expected);
}

#[test]
fn workspace_entries_are_checked_without_directory_metadata() {
	let dir = crate::test_support::TestDir::new();
	let root = dir.path();
	let directory = DirectoryStamp {
		stamp: None,
		entries: markview_document::fingerprint(&workspace_entries(
			root,
			|| false,
		)),
	};
	fs::write(root.join("output.pdf"), "unrelated export").unwrap();
	assert!(!directory.changed(root));
	let nested = root.join("topics");
	fs::create_dir(&nested).unwrap();
	fs::write(nested.join("Topic.md"), "# A new candidate").unwrap();
	assert!(directory.changed(root));
}

#[test]
fn note_embed_trailing_block_id_addresses_the_start_of_the_expansion() {
	let dir = crate::test_support::TestDir::new();
	let root = dir.path();
	fs::write(root.join("B.md"), "# Section\n\nEmbedded body").unwrap();
	let source = parse("![[B#Section]] ^here");
	assert!(source.anchor_block("^here").is_some());
	let expanded = Loader::default()
		.prepare(
			source,
			InputPath::File(&root.join("A.md")),
			None,
			&Security::local(Trust::Trusted),
			|| false,
		)
		.unwrap();
	let block = expanded.anchor_block("^here").unwrap();
	assert!(
		matches!(block.kind(), markview_document::BlockKind::Heading { text, .. } if markview_document::plain_text(text) == "Section")
	);
	assert_eq!(block.id(), expanded.blocks()[0].id());
	assert!(text(&expanded).contains("Embedded body"));
}

#[test]
fn note_embed_png_refuses_to_replace_an_embedded_note() {
	let dir = crate::test_support::TestDir::new();
	let root = dir.path();
	let host = root.join("A.md");
	let note = root.join("B.md");
	let body = "# Section\n\nProtected note";
	fs::write(&host, "![[B#Section]]").unwrap();
	fs::write(&note, body).unwrap();
	let mut options = crate::test_support::options();
	options.ofm = true;
	let result = crate::export::png_snapshot_with_services_and_workspace(
		crate::export::PngSource::file(&host).with_output(&note),
		options,
		true,
		Arc::new(crate::services::Services::new(2)),
		Security::local(Trust::Trusted),
		None,
	);
	assert!(
		result
			.unwrap_err()
			.to_string()
			.contains("embedded source file")
	);
	assert_eq!(fs::read_to_string(&note).unwrap(), body);
}

#[test]
fn note_embed_buffer_base_ignores_a_symlink_at_its_resource_filename() {
	if !markview_platform::test_support::symlinks() {
		return;
	}
	let dir = crate::test_support::TestDir::new();
	let base = dir.path().join("input");
	let elsewhere = dir.path().join("elsewhere");
	for folder in [&base, &elsewhere] {
		fs::create_dir(folder).unwrap();
	}
	fs::write(elsewhere.join("Actual.md"), "# Unrelated file").unwrap();
	fs::write(elsewhere.join("Other.md"), "# Section\n\nWrong source").unwrap();
	fs::write(base.join("Other.md"), "# Section\n\nExpected source").unwrap();
	let virtual_path = base.join("Untitled.md");
	markview_platform::test_support::symlink(
		elsewhere.join("Actual.md"),
		&virtual_path,
	)
	.unwrap();
	let expanded = Loader::default()
		.prepare(
			parse("![[Other#Section]]"),
			InputPath::Buffer(&virtual_path),
			None,
			&Security::local(Trust::Trusted),
			|| false,
		)
		.unwrap();
	assert!(text(&expanded).contains("Expected source"));
	assert!(!text(&expanded).contains("Wrong source"));
}

#[test]
fn note_embed_copies_at_most_the_budget_of_owned_diagram_source() {
	let dir = crate::test_support::TestDir::new();
	let root = dir.path();
	let bytes = 256 * 1024;
	let body = format!(
		"# Diagram\n\n{}`mermaid\n{}\n{}`",
		"``",
		"x".repeat(bytes - 1),
		"``"
	);
	let parsed = parse(&body);
	let mut images = Vec::new();
	for block in parsed.blocks() {
		block.images(&mut images);
	}
	assert_eq!(images[0].source.diagram().unwrap().source.len(), bytes);
	fs::write(root.join("B.md"), body).unwrap();
	let expanded = Loader::default()
		.prepare(
			parse(&"![[B#Diagram]]\n\n".repeat(129)),
			InputPath::File(&root.join("A.md")),
			None,
			&Security::local(Trust::Trusted),
			|| false,
		)
		.unwrap();
	let mut images = Vec::new();
	for block in expanded.blocks() {
		block.images(&mut images);
	}
	let unique: HashMap<_, _> = images
		.iter()
		.map(|image| {
			let source = &image.source.diagram().unwrap().source;
			(source.as_ptr(), source.len())
		})
		.collect();
	let retained = unique.values().sum::<usize>();
	assert!(retained >= bytes);
	assert!(
		retained <= markview_document::DocumentLimits::default().max_text_bytes,
		"retained {retained} bytes of copied diagrams"
	);
}

#[test]
fn note_embed_resolves_dotted_wikilink_names_from_the_source_note() {
	let dir = crate::test_support::TestDir::new();
	let root = dir.path();
	let sub = root.join("sub");
	fs::create_dir(&sub).unwrap();
	let dotted = "Meeting 2026.04.10";
	let note = sub.join(format!("{dotted}.md"));
	fs::write(&note, "Meeting notes").unwrap();
	fs::write(sub.join("B.md"), format!("# Section\n\n[[{dotted}]]")).unwrap();
	let note = note.canonicalize().unwrap();
	let direct =
		crate::link::workspace_candidates(Path::new(dotted), &sub, Some(root))
			.find(|path| path.is_file())
			.unwrap()
			.canonicalize()
			.unwrap();
	assert_eq!(direct, note);
	let security = Security::local(Trust::Trusted);
	let expanded = Loader::default()
		.prepare(
			parse("![[sub/B#Section]]"),
			InputPath::File(&root.join("A.md")),
			Some(root),
			&security,
			|| false,
		)
		.unwrap();
	let markview_document::BlockKind::Paragraph(text) =
		expanded.blocks()[1].kind()
	else {
		panic!("expected paragraph");
	};
	let url = text[0].link_url().unwrap();
	assert_eq!(
		crate::link::resolve_for(url, Some(root), &security),
		Some(crate::link::Target::Markdown(note)),
		"{url}"
	);
}

#[test]
fn note_embed_heading_path_stays_within_its_named_parent() {
	let dir = crate::test_support::TestDir::new();
	let root = dir.path();
	fs::write(
		root.join("B.md"),
		"# Parent\n\n## Different\n\n# Unrelated\n\n## Child\n\nUnique child marker",
	)
	.unwrap();
	let expanded = Loader::default()
		.prepare(
			parse("![[B#Parent#Child]]\n\n![[B#Unrelated#Child]]"),
			InputPath::File(&root.join("A.md")),
			None,
			&Security::local(Trust::Trusted),
			|| false,
		)
		.unwrap();
	let reading = text(&expanded);
	assert!(reading.contains("Unique child marker"), "{reading}");
	assert_eq!(
		reading.matches("Unique child marker").count(),
		1,
		"{reading}"
	);
	assert!(reading.contains("B#Parent#Child"), "{reading}");
}

#[test]
fn note_embed_copies_at_most_the_budget_of_owned_inline_anchors() {
	let dir = crate::test_support::TestDir::new();
	let root = dir.path();
	let id = "x".repeat(256 * 1024);
	let body = format!("# Section\n\nText <em id=\"{id}\">marker</em>");
	let anchors = |document: &Document,
	               counts: &mut HashMap<*const u8, usize>| {
		for block in document.blocks() {
			markview_document::visit_fields(
				block,
				&mut Vec::new(),
				&|| false,
				&mut |_, _, field, _| {
					if let markview_document::FieldText::Rich(field) = field {
						for inline in field {
							if let markview_document::InlineKind::Anchor(
								anchor,
							) = &inline.kind
							{
								counts.insert(anchor.as_ptr(), anchor.len());
							}
						}
					}
				},
			);
		}
	};
	let parsed = parse(&body);
	assert_eq!(text(&parsed), "Section\nText marker\n");
	let mut source = HashMap::new();
	anchors(&parsed, &mut source);
	assert_eq!(source.values().copied().collect::<Vec<_>>(), [id.len()]);
	fs::write(root.join("B.md"), body).unwrap();
	let expanded = Loader::default()
		.prepare(
			parse(&"![[B#Section]]\n\n".repeat(160)),
			InputPath::File(&root.join("A.md")),
			None,
			&Security::local(Trust::Trusted),
			|| false,
		)
		.unwrap();
	let mut owned = HashMap::new();
	anchors(&expanded, &mut owned);
	assert!(!owned.is_empty());
	let retained = owned.values().sum::<usize>();
	assert!(
		retained <= markview_document::DocumentLimits::default().max_text_bytes,
		"retained {retained} bytes of copied anchors"
	);
}

#[test]
fn note_embed_trailing_id_inside_a_container_anchors_the_expansion() {
	let dir = crate::test_support::TestDir::new();
	let root = dir.path();
	fs::write(root.join("B.md"), "# Section\n\nEmbedded body").unwrap();
	for source in [
		"> Before\n>\n> ![[B#Section]] ^here",
		"- Before\n\n  ![[B#Section]] ^here",
	] {
		let source = parse(source);
		assert!(source.anchor_block("^here").is_some());
		let expanded = Loader::default()
			.prepare(
				source,
				InputPath::File(&root.join("A.md")),
				None,
				&Security::local(Trust::Trusted),
				|| false,
			)
			.unwrap();
		assert!(text(&expanded).contains("Embedded body"));
		let anchor = expanded.anchor_block("^here").unwrap();
		assert!(
			matches!(anchor.kind(), markview_document::BlockKind::Heading { text, .. } if markview_document::plain_text(text) == "Section")
		);
	}
}

#[test]
fn note_embed_selects_a_heading_whose_slug_is_reserved_by_an_html_anchor() {
	let dir = crate::test_support::TestDir::new();
	let root = dir.path();
	let body = "Text <span id=\"section\">anchor</span>\n\n# Section\n\nHeading body marker";
	let target = parse(body);
	assert_eq!(target.outline_entries()[0].text, "Section");
	assert!(matches!(
		target.anchor_block("section").unwrap().kind(),
		markview_document::BlockKind::Paragraph(_)
	));
	fs::write(root.join("B.md"), body).unwrap();
	let expanded = Loader::default()
		.prepare(
			parse("![[B#Section]]"),
			InputPath::File(&root.join("A.md")),
			None,
			&Security::local(Trust::Trusted),
			|| false,
		)
		.unwrap();
	assert!(text(&expanded).contains("Heading body marker"));
}

#[test]
fn note_embed_compound_paths_stay_inside_the_parent_container() {
	let dir = crate::test_support::TestDir::new();
	let root = dir.path();
	fs::write(
		root.join("B.md"),
		"> # Parent\n>\n> Earlier text\n\n## Child\n\nOutside marker",
	)
	.unwrap();
	let expanded = Loader::default()
		.prepare(
			parse("![[B#Parent#Child]]\n\n![[B#Child]]"),
			InputPath::File(&root.join("A.md")),
			None,
			&Security::local(Trust::Trusted),
			|| false,
		)
		.unwrap();
	let reading = text(&expanded);
	assert!(reading.contains("B#Parent#Child"), "{reading}");
	assert_eq!(reading.matches("Outside marker").count(), 1);
}

#[test]
fn note_embed_invalidates_a_timestamp_preserving_atomic_replacement() {
	use crate::watch::file_stamp;

	let dir = crate::test_support::TestDir::new();
	let root = dir.path();
	let note = root.join("B.md");
	let replacement = root.join("replacement.md");
	fs::write(&note, "# Section\n\nOld marker").unwrap();
	let parsed = parse("![[B#Section]]");
	let security = Security::local(Trust::Trusted);
	let mut loader = Loader::default();
	let host = root.join("A.md");
	let mut prepare = || {
		loader
			.prepare(
				parsed.clone(),
				InputPath::File(&host),
				None,
				&security,
				|| false,
			)
			.unwrap()
	};
	let before = prepare();
	assert!(text(&before).contains("Old marker"));
	let old_stamp = file_stamp(&note);
	let canonical = note.canonicalize().unwrap();
	fs::write(&replacement, "# Section\n\nNew marker").unwrap();
	fs::File::options()
		.write(true)
		.open(&replacement)
		.unwrap()
		.set_modified(fs::metadata(&note).unwrap().modified().unwrap())
		.unwrap();
	fs::rename(&replacement, &note).unwrap();
	assert_eq!(file_stamp(&note), old_stamp);
	assert_eq!(note.canonicalize().unwrap(), canonical);
	assert!(fs::read_to_string(&note).unwrap().contains("New marker"));
	assert!(before.composition().unwrap().dependencies.changed());
	assert!(text(&prepare()).contains("New marker"));
}

#[test]
fn note_embed_preserves_a_local_markdown_links_query() {
	use markview_document::BlockKind;
	let dir = crate::test_support::TestDir::new();
	let root = dir.path();
	let literal = root.join("child.md?mode=print");
	if let Err(error) = fs::write(&literal, "Literal name") {
		assert!(
			matches!(
				error.kind(),
				std::io::ErrorKind::InvalidInput
					| std::io::ErrorKind::InvalidFilename
			),
			"{error}"
		);
		return;
	}
	let normal = root.join("child.md");
	fs::write(&normal, "# Target").unwrap();
	let body = "# Section\n\n[Next](child.md?mode=print#target)";
	let link = |doc: &Document| match doc.blocks()[1].kind() {
		BlockKind::Paragraph(text) => text[0].link_url().unwrap().to_owned(),
		_ => panic!("link paragraph missing"),
	};
	let security = Security::local(Trust::Trusted);
	let direct = link(&parse(body));
	let target = crate::link::resolve_for(&direct, Some(root), &security);
	assert_eq!(
		target,
		Some(crate::link::Target::Markdown(
			normal.canonicalize().unwrap()
		))
	);
	let expanded = embed_from_b(root, body, "![[B#Section]]");
	let copied = link(&expanded);
	assert_eq!(
		crate::link::resolve_for(&copied, Some(root), &security),
		target
	);
	let url = Url::parse(&copied).unwrap();
	assert_eq!(url.query(), Some("mode=print"));
	assert_eq!(url.fragment(), Some("target"));
}

#[test]
fn note_embed_preserves_literal_question_marks_in_note_names() {
	let dir = crate::test_support::TestDir::new();
	let root = dir.path();
	let sub = root.join("sub");
	fs::create_dir(&sub).unwrap();
	let literal = sub.join("Plan?draft.md");
	if let Err(error) = fs::write(&literal, "Draft marker") {
		assert!(
			matches!(
				error.kind(),
				std::io::ErrorKind::InvalidInput
					| std::io::ErrorKind::InvalidFilename
			),
			"{error}"
		);
		return;
	}
	fs::write(sub.join("Plan.md"), "Other marker").unwrap();
	let body = "# Section\n\n[[Plan?draft.md]]";
	fs::write(sub.join("B.md"), body).unwrap();
	let link = |document: &Document| match document.blocks()[1].kind() {
		markview_document::BlockKind::Paragraph(text) => {
			text[0].link_url().unwrap().to_owned()
		}
		_ => panic!("expected paragraph"),
	};
	let direct = link(&parse(body));
	assert_eq!(direct, "Plan?draft.md");
	let selected =
		crate::link::workspace_candidates(Path::new(&direct), &sub, Some(root))
			.find(|path| path.is_file())
			.unwrap()
			.canonicalize()
			.unwrap();
	assert_eq!(selected, literal.canonicalize().unwrap());
	let security = Security::local(Trust::Trusted);
	let expanded = Loader::default()
		.prepare(
			parse("![[sub/B#Section]]"),
			InputPath::File(&root.join("A.md")),
			Some(root),
			&security,
			|| false,
		)
		.unwrap();
	assert_eq!(
		crate::link::resolve_for(&link(&expanded), Some(root), &security),
		Some(crate::link::Target::Markdown(selected))
	);
}
