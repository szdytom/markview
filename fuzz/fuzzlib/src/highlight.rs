//! Fenced-code preservation oracle for syntax highlighting.
use markview_commonmark::MarkdownParse;
use markview_document::BlockKind;

/// A fence longer than any run in `code`, so the block cannot close early.
pub fn fence(code: &str) -> String {
	let mut longest = 0;
	let mut run = 0;
	for byte in code.bytes() {
		if byte == b'`' {
			run += 1;
			longest = longest.max(run);
		} else {
			run = 0;
		}
	}
	"`".repeat((longest + 2).max(3))
}

pub fn assert_code(doc: &MarkdownParse, code: &str) {
	// Fenced code normalizes CR and CRLF line endings to LF.
	let expected = code.replace("\r\n", "\n").replace('\r', "\n");
	let found = doc
		.document
		.blocks()
		.iter()
		.find_map(|block| {
			if let BlockKind::Code { text, .. } = block.kind() {
				Some(text)
			} else {
				None
			}
		})
		.expect("the fenced input did not become a code block");
	assert_eq!(
		found.trim_end_matches('\n'),
		expected.trim_end_matches('\n')
	);
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn code_oracle_accepts_markdown_normalization() {
		for code in [
			"for valueni  one two; do echo $value; done\r",
			"first\rsecond\r\nthird\n",
			"a\0b\0",
			"\r\n\r\n",
			"",
			"  let x = \"文本\";\t\n```\n````",
		] {
			let fence = fence(code);
			let doc = markview_commonmark::parse(format!(
				"{fence}rust\n{code}\n{fence}\n"
			));
			assert_code(&doc, code);
		}
	}
}
