use markview_document::{BlockKind, Document, visit_blocks};
use proptest::prelude::*;

// Expected languages are independent of the production metadata parser.
fn declaration() -> impl Strategy<Value = (&'static str, Option<&'static str>)>
{
	prop::sample::select(vec![
		("", None),
		(r#"class="language-" data-language=" ""#, None),
		(r#"class="language-rust""#, Some("rust")),
		(r#"class="extra lang-javascript token""#, Some("js")),
		(r#"class="highlight-source-python3""#, Some("py")),
		(r#"data-language=" TypeScript ""#, Some("ts")),
		(r#"data-lang="c++""#, Some("cpp")),
		(r#"class="language-none""#, Some("text")),
		(r#"class="language-plaintext""#, Some("text")),
		(
			r#"class="nohighlight language-rust" data-lang="python""#,
			Some("text"),
		),
		(
			r#"class="no-highlight" data-language="typescript""#,
			Some("text"),
		),
		(
			r#"class="language-rust" data-lang="ruby" data-language="BASH""#,
			Some("sh"),
		),
		(
			r#"class="language-rust" data-language=" " data-lang="ruby""#,
			Some("rb"),
		),
		(r#"class="language-unrecognized""#, Some("unrecognized")),
	])
}

fn extract_codes(body: &str) -> Vec<(String, String)> {
	let prose = "<p>This tutorial explains the following examples in detail, including their inputs, results and practical applications. All examples are part of the article and illustrate the discussion.</p>";
	let html = format!(
		"<html><head><title>Code examples</title></head><body><article><h1>Code examples</h1>{prose}{body}{prose}</article></body></html>"
	);
	let document: Document =
		markview_webpage::extract(&html, "https://example.org/tutorial")
			.unwrap()
			.document;
	let mut codes = Vec::new();
	for block in document.blocks() {
		visit_blocks(block, &mut |block| {
			if let BlockKind::Code { language, text } = block.kind() {
				codes.push((language.clone(), text.clone()));
			}
		});
	}
	codes
}

fn escape(text: &str) -> String {
	text.replace('&', "&amp;")
		.replace('<', "&lt;")
		.replace('>', "&gt;")
}

proptest! {
	#[test]
	fn nearest_language_wins_without_leaking_to_siblings_and_token_spans_preserve_text(
		outer in declaration(),
		wrappers in prop::collection::vec(declaration(), 0..8),
		pre in declaration(),
		code in declaration(),
		pieces in prop::collection::vec("[a-zA-Z0-9 &<>\"'\t\n中文]{0,16}", 1..8),
	) {
		let mut body = format!("<div {}>", outer.0);
		for (attributes, _) in &wrappers {
			body.push_str(&format!("<div {attributes}>"));
		}
		body.push_str(&format!("<pre {}><code {}>let value = ", pre.0, code.0));
		for piece in &pieces {
			body.push_str(&format!("<span class=\"token string\">{}</span>", escape(piece)));
		}
		body.push_str(";</code></pre>");
		body.push_str(&"</div>".repeat(wrappers.len()));
		body.push_str("<pre><code>sibling</code></pre></div><pre>outside</pre>");
		let nearest = code.1.or(pre.1)
			.or_else(|| wrappers.iter().rev().find_map(|(_, expected)| *expected))
			.or(outer.1).unwrap_or("");
		let expected = vec![
			(nearest.to_owned(), format!("let value = {};", pieces.concat())),
			(outer.1.unwrap_or("").to_owned(), "sibling".into()),
			(String::new(), "outside".into()),
		];
		prop_assert_eq!(extract_codes(&body), expected);
	}

	#[test]
	fn explicit_metadata_overrides_shebangs_and_shebangs_require_the_first_line(
		metadata in declaration(),
		(interpreter, language) in prop::sample::select(vec![
			("python3", "py"), ("bash", "sh"), ("ruby", "rb"),
			("node", "js"), ("perl", "pl"), ("unknown-interpreter", ""),
		]),
		invocation in 0..3usize,
		leading_newline in any::<bool>(),
		arguments in prop::sample::select(vec!["", " -u", " --version"]),
	) {
		let command = match invocation {
			0 => format!("/usr/bin/{interpreter}"),
			1 => format!("/usr/bin/env {interpreter}"),
			_ => format!("/usr/bin/env -S {interpreter}"),
		};
		let text = format!("{}#!{command}{arguments}\nprint('hello')", if leading_newline { "\n" } else { "" });
		// A `code` child avoids HTML's special removal of the first `pre` newline.
		let body = format!("<pre><code {}>{}</code></pre>", metadata.0, escape(&text));
		let expected = metadata.1.unwrap_or(if leading_newline { "" } else { language });
		prop_assert_eq!(extract_codes(&body), vec![(expected.to_owned(), text)]);
	}
}
