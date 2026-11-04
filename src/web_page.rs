//! Experimental web article extraction into the semantic document pipeline.
use anyhow::{Context, Result, bail};
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
	let [block] = document.blocks() else {
		return false;
	};
	let BlockKind::Paragraph(text) = block.kind() else {
		return false;
	};
	plain_text(text) == title.trim()
		&& text.iter().all(|inline| {
			matches!(inline.kind, InlineKind::Text(_))
				&& markview_layout::style::text_conditions(
					&inline.style,
					inline.link.is_some(),
				)
				.next()
				.is_none()
		})
}

/// A converted page and the permissions its input granted.
#[derive(Debug)]
pub(crate) struct Article {
	pub(crate) article: markview_webpage::WebArticle,
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
	security.bind(&html);
	let final_url = fetched.final_url.clone();
	let article = tokio::task::spawn_blocking(move || {
		markview_webpage::extract(&html, &final_url)
	})
	.await??;
	Ok(Article { article, security })
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
		assert!(article.article.document.blocks().iter().any(
			|block| matches!(
				block.kind(),
				crate::document::BlockKind::Heading { text, .. }
					if crate::document::plain_text(text)
						.contains("Native web reading")
			)
		));
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

	#[test]
	fn markdown_and_web_documents_never_share_a_revision() {
		let markdown = markview_commonmark::parse("Body without a heading.\n");
		let article = markview_webpage::extract(
			include_str!("../tests/fixtures/web-plain-article.html"),
			"https://example.org/plain",
		)
		.unwrap();
		assert_ne!(markdown.document.revision(), article.document.revision());
		// Both front ends allocate from one counter, so the next revision is
		// newer than either of them.
		let next = markview_document::DocumentRevision::fresh();
		assert!(next > markdown.document.revision());
		assert!(next > article.document.revision());
	}

	#[test]
	fn web_extraction_limits_match_layout_limits() {
		let limits = markview_layout::limits::Limits::default();
		assert_eq!(
			markview_webpage::PROFILE_V1.max_table_rows,
			limits.table_rows
		);
		assert_eq!(
			markview_webpage::PROFILE_V1.max_table_columns,
			limits.table_columns
		);
		assert_eq!(
			markview_webpage::PROFILE_V1.max_table_cells,
			limits.table_cells
		);
	}

	#[test]
	fn the_stored_web_fixture_lays_out_all_of_its_math() {
		let html = include_str!("../tests/fixtures/web-math-table.html");
		let article =
			markview_webpage::extract(html, "https://example.org/math")
				.expect("a readable article");
		let document = crate::loaded::LoadedDocument::semantic(
			std::sync::Arc::new(article.document),
		);
		let settings = crate::settings::ReaderSettings::default();
		let options = settings.layout_options(
			760.0,
			false,
			&crate::test_support::fonts(),
		);
		let snapshot = markview_layout::layout::LayoutEngine::new()
			.layout(&document, &options);
		assert_eq!(snapshot.math_errors, 0);
	}
}
