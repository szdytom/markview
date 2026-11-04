use markview_document::BlockKind;

#[test]
fn an_unused_reference_definition_keeps_preceding_details_unchanged() {
	for newline in ["\n", "\r\n", "\r"] {
		for body in [
			"+ \n  [^1]: x\n      ",
			"+ [used]\n      ",
			"\n```\n[used]",
			"[\n\n<?L:|LL\n  </other><?L:|LL\n ",
			"<details>+ \n  [^1]: x\n      </details>",
		] {
			for definitions in ["", "[used]: /used\n"] {
				let source = format!(
					"<details>{body}</details>\n\nEnd\n\n{definitions}"
				)
				.replace('\n', newline);
				let before = markview_commonmark::parse(source.clone());
				let after = markview_commonmark::parse(format!(
					"{source}{newline}[unused]: /unused{newline}"
				));
				// The test owns the details block, not whatever follows it.
				assert_eq!(before.blocks()[0], after.blocks()[0], "{source:?}");
			}
		}
	}
}

#[test]
fn an_unused_reference_definition_keeps_preceding_table_unchanged() {
	let source = "|---|:-:|---:|\n      |---|:-:|---:|\n|---|:-:|---:|\n|---|:-:|---|\n#\n";
	for newline in ["\n", "\r\n", "\r"] {
		let source = source.replace('\n', newline);
		let before = markview_commonmark::parse(source.clone());
		let after = markview_commonmark::parse(format!(
			"{source}{newline}[unused]: /unused{newline}"
		));
		assert_eq!(before.blocks(), after.blocks());
		let BlockKind::Table(table) = before.blocks()[1].kind() else {
			panic!("expected the table")
		};
		let cell = &table.groups[1].rows[0].cells[2];
		let BlockKind::Paragraph(rich) = cell.blocks[0].kind() else {
			panic!("expected a cell paragraph")
		};
		assert_eq!(
			&source
				[before.inline_source(&before.blocks()[1], &rich[0]).unwrap()],
			"---"
		);
	}
}
