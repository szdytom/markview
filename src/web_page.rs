//! Experimental web article extraction into the native Markdown pipeline.
use anyhow::{Context, Result, bail};
use dom_smoothie::{Config, Readability, TextMode};
use std::time::Duration;

pub(crate) fn validate_url(value: &str) -> Result<url::Url> {
	let url = url::Url::parse(value).context("Invalid web page URL")?;
	if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
		bail!("Web pages require an HTTP(S) URL");
	}
	Ok(url)
}

pub(crate) fn shared_url(text: &str) -> Option<String> {
	let mut lines = text.trim_matches(['\r', '\n']).lines();
	let first = lines.next()?;
	let candidate = match (lines.next(), lines.next()) {
		(None, None) => first,
		(Some(url), None)
			if plain_share_title(first)
				&& validate_url(first.trim()).is_err() =>
		{
			url
		}
		_ => return None,
	};
	if candidate.starts_with("    ")
		|| candidate.trim_start_matches(' ').starts_with('\t')
	{
		return None;
	}
	let candidate = candidate.trim();
	if candidate.chars().any(char::is_whitespace) {
		return None;
	}
	validate_url(candidate).ok().map(|url| url.to_string())
}

fn plain_share_title(title: &str) -> bool {
	use markview_commonmark::parse;
	use markview_document::{BlockKind, InlineKind, plain_text};
	let document = parse(title.to_owned());
	let [block] = document.blocks.as_slice() else {
		return false;
	};
	let BlockKind::Paragraph(text) = &block.kind else {
		return false;
	};
	plain_text(text) == title.trim()
		&& text.iter().all(|inline| {
			matches!(inline.kind, InlineKind::Text(_))
				&& markview_layout::style::text_conditions(&inline.style)
					.next()
					.is_none()
		})
}

#[derive(Debug)]
pub(crate) struct Article {
	pub(crate) markdown: String,
	pub(crate) security: crate::security::Security,
}

pub(crate) async fn load(
	url: &str,
	offline: bool,
	headers: &reqwest::header::HeaderMap,
	mut security: crate::security::Security,
) -> Result<Article> {
	if offline {
		bail!("Web pages are unavailable with --offline");
	}
	validate_url(url)?;
	crate::net::grant_target(url, &mut security).await?;
	let fetched = tokio::time::timeout(
		Duration::from_secs(30),
		crate::net::get(
			url,
			&crate::net::Validators::default(),
			crate::file::MAX_FILE_BYTES,
			"Web page",
			headers,
			&security,
		),
	)
	.await
	.context("Web page download timed out")??;
	let html = String::from_utf8(fetched.body)
		.context("Experimental web reading currently requires UTF-8 HTML")?;
	security.origin = crate::security::Origin::Web(fetched.final_url.clone());
	let markdown =
		tokio::task::spawn_blocking(move || extract(&html, &fetched.final_url))
			.await??;
	security.bind(&markdown);
	Ok(Article { markdown, security })
}

fn extract(html: &str, url: &str) -> Result<String> {
	let config = Config {
		text_mode: TextMode::Markdown,
		max_elements_to_parse: 100_000,
		..Default::default()
	};
	let document = dom_query::Document::from(html);
	let preserved = preserve_markup(&document);
	let article =
		Readability::with_document(document, Some(url), Some(config))?
			.parse()?;
	if article.text_content.trim().is_empty() {
		bail!("No readable article found on this page");
	}
	// A single plain heading names the temporary tab; source links stay absolute.
	let title = article
		.title
		.split_whitespace()
		.collect::<Vec<_>>()
		.join(" ");
	let title = if title.is_empty() { url } else { &title };
	let mut heading = String::with_capacity(title.len());
	for c in title.chars() {
		if c.is_ascii_punctuation() {
			use std::fmt::Write;
			write!(heading, "&#{};", c as u32).unwrap();
		} else {
			heading.push(c);
		}
	}
	let mut content = literal_delimiters(&article.text_content);
	for (token, replacement) in preserved {
		content = content.replace(&token, &replacement);
	}
	let markdown = format!("# {heading}\n\nSource: <{url}>\n\n{}\n", content);
	if !crate::paste::looks_like_markdown(&markdown) {
		bail!("Extracted web article exceeds the supported Markdown limits");
	}
	Ok(markdown)
}

/// Protect semantic HTML that the Markdown serializer treats as plain text.
fn preserve_markup(document: &dom_query::Document) -> Vec<(String, String)> {
	let text = document.root().text();
	let mut prefix = "MARKVIEWPRESERVED".to_string();
	while text.contains(&prefix) {
		prefix.push('X');
	}
	let mut preserved = Vec::new();
	for node in document
		.select(".math.inline, .math.display, script[type^='math/tex']")
		.nodes()
	{
		if node.ancestors_it(None).any(|parent| parent.is("pre, code")) {
			continue;
		}
		let text = node.text();
		let mut latex = text.trim();
		for (open, close) in
			[(r"\(", r"\)"), (r"\[", r"\]"), ("$$", "$$"), ("$", "$")]
		{
			if let Some(inner) =
				latex.strip_prefix(open).and_then(|s| s.strip_suffix(close))
			{
				latex = inner;
				break;
			}
		}
		let in_table =
			node.ancestors_it(None).any(|parent| parent.is("td, th"));
		// TeX delimiters keep literal pipes from splitting Markdown table cells.
		let latex = if in_table {
			latex.replace(r"\|", r"\Vert ").replace('|', r"\vert ")
		} else {
			latex.to_string()
		};
		let display = node.has_class("display")
			|| node
				.attr("type")
				.is_some_and(|kind| kind.contains("mode=display"));
		let replacement = if display && !in_table {
			format!("\n\n\\[{latex}\\]\n\n")
		} else {
			format!("\\({latex}\\)")
		};
		let token = format!("{prefix}{}END", preserved.len());
		if node.is("script") {
			node.rename("span");
		}
		node.set_text(token.as_str());
		preserved.push((token, replacement));
	}
	// Empty headings otherwise produce an empty, invalid table separator cell.
	for node in document.select("th").nodes() {
		if node.inner_html().trim().is_empty() {
			let token = format!("{prefix}{}END", preserved.len());
			node.set_text(token.as_str());
			preserved.push((token, String::new()));
		}
	}
	preserved
}

/// Keep CommonMark punctuation escapes from becoming Markview LaTeX delimiters.
fn literal_delimiters(markdown: &str) -> String {
	let arena = comrak::Arena::new();
	let root =
		comrak::parse_document(&arena, markdown, &comrak::Options::default());
	let mut lines = vec![0];
	lines.extend(markdown.match_indices('\n').map(|(offset, _)| offset + 1));
	let mut code = root
		.descendants()
		.filter_map(|node| {
			let data = node.data.borrow();
			matches!(
				data.value,
				comrak::nodes::NodeValue::Code(_)
					| comrak::nodes::NodeValue::CodeBlock(_)
			)
			.then(|| {
				let position = data.sourcepos;
				let start =
					lines[position.start.line - 1] + position.start.column - 1;
				let end = (lines[position.end.line - 1] + position.end.column)
					.min(markdown.len());
				start..end
			})
		})
		.peekable();
	let mut output = String::with_capacity(markdown.len());
	let mut offset = 0;
	while offset < markdown.len() {
		if let Some(range) = code.peek()
			&& offset == range.start
		{
			output.push_str(&markdown[range.clone()]);
			offset = range.end;
			code.next();
			continue;
		}
		let tail = &markdown[offset..];
		let mut chars = tail.chars();
		let c = chars.next().unwrap();
		if c == '\\'
			&& let Some(next) = chars.next()
		{
			match next {
				'(' => output.push_str("&#40;"),
				')' => output.push_str("&#41;"),
				'[' => output.push_str("&#91;"),
				']' => output.push_str("&#93;"),
				_ => {
					output.push(c);
					output.push(next);
				}
			}
			offset += c.len_utf8() + next.len_utf8();
		} else {
			output.push(c);
			offset += c.len_utf8();
		}
	}
	output
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn shared_links_preserve_url_prose() {
		let url = "https://example.com/article";
		for whitespace in [" ", "\t", "\u{a0}", "\u{2003}"] {
			let prose = format!("  {url}{whitespace}is worth reading ");
			assert_eq!(shared_url(&prose), None, "{prose:?}");
			assert_eq!(shared_url(&format!("Article title\n{prose}")), None);
		}
		for encoded in ["%20", "%09"] {
			let link = format!("{url}{encoded}name");
			assert_eq!(shared_url(&format!("  {link} \t")), Some(link.clone()));
			assert_eq!(
				shared_url(&format!("Article title\n{link}")),
				Some(link)
			);
		}
	}

	#[test]
	fn shared_links_preserve_indented_url_code_blocks() {
		let url = "https://example.com/article";
		for indentation in ["    ", "     ", "\t", " \t", "   \t"] {
			let code = format!("{indentation}{url} ");
			assert_eq!(shared_url(&code), None, "{code:?}");
			assert_eq!(shared_url(&format!("Article title\n{code}")), None);
			assert_eq!(shared_url(&format!("\r\n{code}\r\n")), None);
		}
		for padding in ["", " ", "  ", "   "] {
			let link = format!("{padding}{url} \t");
			assert_eq!(shared_url(&link), Some(url.into()), "{link:?}");
			assert_eq!(
				shared_url(&format!("Article title\n{link}")),
				Some(url.into())
			);
		}
	}

	#[test]
	fn shared_links_preserve_markdown_titles() {
		let url = "https://example.com/article?filter[tag]=rust";
		for title in [
			"- Read this article",
			"+ Read this article",
			"* Read this article",
			"1. Read this article",
			"1) Read this article",
			"- [ ] Read this article",
			"> Read this article",
			"# Read this article",
			"    Read this article",
			"\tRead this article",
			"**Read this article**",
			"_Read this article_",
			"~~Read this article~~",
			"`Read this article`",
			"$x^2$",
			"```rust",
			"[Read this article](https://example.com/)",
			"---",
		] {
			assert_eq!(shared_url(&format!("{title}\n{url}")), None, "{title}");
		}
		for title in [
			"Read this article",
			"Rust: notes (2026)",
			"#hashtag",
			"-word",
		] {
			assert_eq!(
				shared_url(&format!("{title}\n{url}")),
				Some(url.into()),
				"{title}"
			);
		}
		assert_eq!(shared_url(&format!("  {url}  ")), Some(url.into()));
		let ipv6 = "https://[2001:db8::1]/article";
		assert_eq!(shared_url(ipv6), Some(ipv6.into()));
	}

	#[test]
	fn web_math_and_empty_table_headers_reach_native_layout() {
		use crate::document::{BlockKind, InlineKind, plain_text};
		let markdown = extract(
			include_str!("../tests/fixtures/web-math-table.html"),
			"https://example.org/cabo",
		)
		.unwrap();
		assert!(!markdown.contains("MARKVIEWPRESERVED"));
		let document = markview_commonmark::parse(markdown);
		let math: Vec<_> = document
			.blocks
			.iter()
			.filter_map(|block| {
				if let BlockKind::Paragraph(text) = &block.kind {
					Some(text)
				} else {
					None
				}
			})
			.flatten()
			.filter_map(|inline| {
				if let InlineKind::Math { latex, display } = &inline.kind {
					Some((latex.as_str(), *display))
				} else {
					None
				}
			})
			.collect();
		assert!(math.contains(&("n=2", false)));
		assert!(math.contains(&("(k_1,k_2)", false)));
		assert!(math.contains(&(r"|\Delta|_{\max}\approx0.003", false)));
		assert!(math.contains(&("x < y", false)));
		assert_eq!(math.iter().filter(|(_, display)| *display).count(), 2);
		let rows = document
			.blocks
			.iter()
			.find_map(|block| {
				if let BlockKind::Table { rows, .. } = &block.kind {
					Some(rows)
				} else {
					None
				}
			})
			.expect("table with an empty heading");
		assert_eq!(rows.len(), 3);
		assert!(rows.iter().all(|row| row.len() == 5));
		assert!(rows[0][2].is_empty());
		assert_eq!(plain_text(&rows[1][1]), "0.840");
		assert_eq!(plain_text(&rows[1][4]), "0.372");
		assert!(
			matches!(&rows[0][0][0].kind, InlineKind::Math { latex, display: false } if latex == "(k_1,k_2)")
		);
		let empty_headers = document
			.blocks
			.iter()
			.filter_map(|block| {
				if let BlockKind::Table { rows, .. } = &block.kind {
					Some(rows)
				} else {
					None
				}
			})
			.nth(1)
			.expect("table with all headings empty");
		assert_eq!(empty_headers.len(), 2);
		assert!(empty_headers[0].iter().all(Vec::is_empty));
		assert!(
			matches!(&empty_headers[1][0][0].kind, InlineKind::Math { latex, .. } if latex == r"a\vert b")
		);
		assert!(
			matches!(&empty_headers[1][1][0].kind, InlineKind::Math { latex, .. } if latex == r"\Vert x\Vert ")
		);

		let snapshot = crate::layout::LayoutEngine::new()
			.layout(&document, &crate::test_support::options());
		assert_eq!(snapshot.math_errors, 0);
	}

	#[test]
	fn extracts_article_markdown_with_absolute_links_and_images() {
		let markdown = extract(
			include_str!("../tests/fixtures/web-article.html"),
			"https://example.org/articles/intro",
		)
		.unwrap();
		assert!(markdown.starts_with("# Native web reading\n"));
		assert!(
			markdown.contains("Source: <https://example.org/articles/intro>")
		);
		assert!(markdown.contains("**native typography**"));
		assert!(
			markdown.contains("[related article](https://example.org/related)")
		);
		assert!(
			markdown.contains(
				"![Diagram](https://example.org/articles/diagram.png)"
			)
		);
		assert!(!markdown.contains("Navigation link"));
		assert!(!markdown.contains("alert("));
		assert!(markdown.contains(r"\(keep code\)"));
		let document = markview_commonmark::parse(markdown.clone());
		assert_eq!(document.source.as_ref(), markdown);
		assert!(!document.blocks.is_empty());
		assert!(document.blocks.iter().any(|block| {
			let crate::document::BlockKind::Paragraph(inlines) = &block.kind
			else {
				return false;
			};
			crate::document::plain_text(inlines)
				== "中文 Try (rustup default beta) with [literal brackets] in prose."
		}));
		for block in &document.blocks {
			if let crate::document::BlockKind::Paragraph(inlines) = &block.kind
			{
				assert!(!inlines.iter().any(|inline| matches!(
					inline.kind,
					crate::document::InlineKind::Math { .. }
				)));
			}
		}
	}

	#[test]
	fn preserves_titles_and_uses_url_for_untitled_pages() {
		let html = include_str!("../tests/fixtures/web-article.html");
		let title = "Rust_Blog [2024] &amp; $5 &lt;notes&gt;";
		let html = html.replace("Native web reading", title);
		let markdown = extract(&html, "https://example.org/").unwrap();
		let document = markview_commonmark::parse(markdown.clone());
		let crate::document::BlockKind::Heading { text, .. } =
			&document.blocks[0].kind
		else {
			panic!("missing title")
		};
		assert_eq!(
			crate::document::plain_text(text),
			"Rust_Blog [2024] & $5 <notes>"
		);
		assert_eq!(
			crate::paste::title_for(&markdown, crate::lang::Lang::En),
			"Rust_Blog [2024] & $5 <notes>"
		);
		let html = html.replace(title, "");
		let markdown = extract(&html, "https://example.org/").unwrap();
		assert_eq!(
			crate::paste::title_for(&markdown, crate::lang::Lang::En),
			"https://example.org/"
		);
	}

	#[test]
	fn delimiter_conversion_preserves_nested_code_and_escaped_backslashes() {
		let markdown = "中文 \\(prose\\) \\[brackets\\] `\\(inline\\)`\n\n| Code |\n| --- |\n| `\\[table\\]` |\n\n- Example:\n\n  ```rust\n  \\(fenced\\)\n  ```\n\n\\\\(backslash)";
		let converted = literal_delimiters(markdown);
		assert!(converted.contains("&#40;prose&#41; &#91;brackets&#93;"));
		for code in [
			r"`\(inline\)`",
			r"`\[table\]`",
			r"\(fenced\)",
			r"\\(backslash)",
		] {
			assert!(converted.contains(code), "lost {code:?}: {converted}");
		}
	}

	#[tokio::test]
	async fn rejects_offline_and_invalid_requests_before_network_access() {
		assert!(
			load(
				"https://example.org/",
				true,
				&reqwest::header::HeaderMap::new(),
				crate::security::Security::default(),
			)
			.await
			.unwrap_err()
			.to_string()
			.contains("--offline")
		);
		assert!(
			load(
				"file:///tmp/article.html",
				false,
				&reqwest::header::HeaderMap::new(),
				crate::security::Security::default(),
			)
			.await
			.is_err()
		);
		assert!(
			extract("<html><body></body></html>", "https://example.org/")
				.is_err()
		);
	}
	#[tokio::test]
	async fn explicit_local_page_grants_its_origin_and_stays_untrusted() {
		use crate::security::{Origin, Security, Trust};
		use tokio::io::{AsyncReadExt, AsyncWriteExt};
		let listener =
			tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
		let url = format!("http://{}/article", listener.local_addr().unwrap());
		let response = tokio::spawn(async move {
			let (mut socket, _) = listener.accept().await.unwrap();
			let mut request = [0; 4096];
			let n = socket.read(&mut request).await.unwrap();
			assert!(n > 0);
			let html = include_str!("../tests/fixtures/web-article.html");
			socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}", html.len()).as_bytes()).await.unwrap();
		});
		let article = load(
			&url,
			false,
			&reqwest::header::HeaderMap::new(),
			Security::web(url.clone()),
		)
		.await
		.unwrap();
		response.await.unwrap();
		assert!(article.markdown.contains("Native web reading"));
		assert_eq!(article.security.origin, Origin::Web(url.clone()));
		assert_eq!(article.security.origin.trust(), Trust::Untrusted);
		assert!(
			article
				.security
				.check_address(
					&url::Url::parse(&url).unwrap(),
					"127.0.0.1".parse().unwrap()
				)
				.is_ok()
		);
		assert!(
			article
				.security
				.check_address(
					&url::Url::parse("http://127.0.0.1:12345/image").unwrap(),
					"127.0.0.1".parse().unwrap()
				)
				.is_err()
		);
		assert!(
			article
				.security
				.check_file(std::path::Path::new("/tmp/image.png"))
				.is_err()
		);
	}
}
