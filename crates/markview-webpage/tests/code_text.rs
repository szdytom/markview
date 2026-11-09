use markview_document::{BlockKind, visit_blocks};
use proptest::prelude::*;

fn code_blocks(body: &str) -> Vec<(String, String)> {
	let html = format!(
		"<html><head><title>Code examples</title></head><body><article><h1>Code examples</h1><p>This tutorial explains the following examples, their inputs and their results.</p>{body}</article></body></html>"
	);
	let article =
		markview_webpage::extract(&html, "https://example.org/code").unwrap();
	let mut codes = Vec::new();
	for block in article.document.blocks() {
		visit_blocks(block, &mut |block| {
			if let BlockKind::Code { language, text } = block.kind() {
				codes.push((language.clone(), text.clone()));
			}
		});
	}
	codes
}

#[test]
fn docusaurus_token_lines_keep_breaks_blank_lines_and_indentation() {
	let html = r#"<div class="language-python"><pre class="prism-code language-python"><code class="codeBlockLines_UUn8"><span class="token-line"><span class="token keyword">def</span><span class="token plain"> greet():</span><br></span><span class="token-line"><span class="token plain" style="display:inline-block"></span><br></span><span class="token-line"><span class="token plain">    </span><span class="token keyword">return</span><span class="token string"> "&lt;hello&gt;&amp;"</span><br></span></code></pre></div>"#;
	assert_eq!(
		code_blocks(html),
		vec![(
			"py".into(),
			"def greet():\n\n    return \"<hello>&\"\n".into()
		)],
	);
}

#[test]
fn preformatted_breaks_survive_readability_without_changing_literal_newlines() {
	let html = "<pre><code><br><br>first<br><br>\tsecond\nthird<br></code></pre><pre><code>\nplain\n\n  indented\n</code></pre>";
	assert_eq!(
		code_blocks(html),
		vec![
			(String::new(), "\n\nfirst\n\n\tsecond\nthird\n".into()),
			(String::new(), "\nplain\n\n  indented\n".into()),
		],
	);
}

#[test]
fn line_breaks_are_preserved_before_shebang_language_detection() {
	assert_eq!(
		code_blocks("<pre>#!/usr/bin/python3<br>print(1)<br></pre>"),
		vec![("py".into(), "#!/usr/bin/python3\nprint(1)\n".into())],
	);
}

proptest! {
	#[test]
	fn code_text_matches_literal_source_with_mixed_break_encodings(
		pieces in prop::collection::vec(
			("[a-zA-Z0-9 &<>\"'\t中文]{0,24}", any::<bool>(), 0usize..4),
			1..16,
		),
	) {
		let mut html = String::from("<pre><code>start");
		let mut expected = String::from("start");
		for (text, use_br, breaks) in pieces {
			let escaped = text.replace('&', "&amp;")
				.replace('<', "&lt;").replace('>', "&gt;");
			html.push_str(&format!("<span class=\"token-line\"><span class=\"token plain\">{escaped}</span>"));
			html.push_str(&if use_br { "<br>" } else { "\n" }.repeat(breaks));
			html.push_str("</span>");
			expected.push_str(&text);
			expected.push_str(&"\n".repeat(breaks));
		}
		html.push_str("</code></pre>");
		prop_assert_eq!(code_blocks(&html), vec![(String::new(), expected)]);
	}
}
