//! Dump a generated web article, its expectation and the extracted blocks.
//! For triage: the output points at the first block that diverged.
//!
//! Run: `cargo run -p mvfuzz --bin webdump -- <file> [base-url]`

use markview_document::{Block, BlockKind, plain_text};
use mvfuzz::{webgen, webpage};

fn main() {
	let path = std::env::args().nth(1).expect("usage: webdump <file>");
	let data = std::fs::read(&path).unwrap();
	let raw = std::env::args().any(|arg| arg == "--html");
	let base = std::env::args()
		.nth(2)
		.filter(|arg| arg != "--html")
		.unwrap_or_else(|| webpage::base_for(&data).to_owned());
	if raw {
		let html = String::from_utf8_lossy(&data);
		eprintln!("base={base}");
		match markview_webpage::extract(&html, &base) {
			Ok(article) => {
				eprintln!("--- metadata ---\n{:#?}", article.metadata);
				eprintln!("--- blocks ---");
				for block in article.document.blocks() {
					dump(block, 0);
				}
				eprintln!(
					"--- reading text ---\n{}",
					webpage::reading_text(&article.document)
				);
			}
			Err(error) => eprintln!("extraction failed: {error:?}"),
		}
		return;
	}
	let generated = webgen::generate(&data);
	eprintln!("base={base}");
	eprintln!("--- html ---\n{}", generated.html);
	eprintln!("--- markers ---\n{:?}", generated.markers);
	eprintln!("--- expected ---");
	for block in &generated.blocks {
		eprintln!("{block:#?}");
	}
	match markview_webpage::extract(&generated.html, &base) {
		Ok(article) => {
			eprintln!("--- metadata ---\n{:#?}", article.metadata);
			eprintln!("--- blocks ---");
			for block in article.document.blocks() {
				dump(block, 0);
			}
			eprintln!(
				"--- reading text ---\n{}",
				webpage::reading_text(&article.document)
			);
		}
		Err(error) => eprintln!("extraction failed: {error:?}"),
	}
}

fn dump(block: &Block, depth: usize) {
	let pad = "  ".repeat(depth);
	eprintln!("{pad}anchors={:?} id={}", block.anchors(), block.id());
	match block.kind() {
		BlockKind::Paragraph(text) => {
			eprintln!("{pad}Paragraph {:?}", plain_text(text));
			for inline in text {
				if inline.style != Default::default() || inline.link.is_some() {
					eprintln!(
						"{pad}  inline {:?} style={:?} link={:?}",
						inline.kind,
						inline.style,
						inline.link_url()
					);
				}
			}
		}
		BlockKind::Heading {
			level,
			text,
			anchor,
		} => eprintln!(
			"{pad}Heading{level} {:?} anchor={anchor:?}",
			plain_text(text)
		),
		BlockKind::Code { language, text } => {
			eprintln!("{pad}Code {language:?} {text:?}")
		}
		BlockKind::Table(table) => {
			eprintln!(
				"{pad}Table caption={:?} columns={:?}",
				table.caption.as_ref().map(|c| plain_text(c)),
				table.columns
			);
			for group in &table.groups {
				eprintln!("{pad}  {:?}", group.kind);
				for row in &group.rows {
					for cell in &row.cells {
						eprintln!(
							"{pad}    cell {:?} rs={} cs={} align={:?} {:?}",
							cell.kind,
							cell.rowspan,
							cell.colspan,
							cell.align,
							cell.blocks
								.iter()
								.map(|block| match block.kind() {
									BlockKind::Paragraph(text) =>
										plain_text(text),
									other => format!("{other:?}"),
								})
								.collect::<Vec<_>>()
						);
					}
				}
			}
		}
		other => eprintln!("{pad}{other:?}"),
	}
	for child in children(block) {
		dump(child, depth + 1);
	}
}

fn children(block: &Block) -> Vec<&Block> {
	match block.kind() {
		BlockKind::Quote { blocks, .. }
		| BlockKind::Footnote { blocks, .. }
		| BlockKind::Details { blocks, .. }
		| BlockKind::FrontMatter { blocks, .. }
		| BlockKind::Figure { blocks, .. } => blocks.iter().collect(),
		BlockKind::List { items, .. } => {
			items.iter().flat_map(|item| item.blocks.iter()).collect()
		}
		BlockKind::Table(table) => table.cell_blocks().collect(),
		BlockKind::DefinitionList(items) => items
			.iter()
			.flat_map(|item| item.definitions.iter())
			.flat_map(|blocks| blocks.iter())
			.collect(),
		_ => Vec::new(),
	}
}
