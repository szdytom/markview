use super::*;
use crate::security::{Origin, Security, Trust};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn serve(
	path: &str,
	content_type: &str,
	body: &[u8],
	redirect: Option<&str>,
) -> (String, tokio::task::JoinHandle<()>) {
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let url = format!("http://{}{path}", listener.local_addr().unwrap());
	let body = body.to_vec();
	let content_type = content_type.to_owned();
	let redirect = redirect.map(str::to_owned);
	let task = tokio::spawn(async move {
		for hop in 0..=usize::from(redirect.is_some()) {
			let (mut socket, _) = listener.accept().await.unwrap();
			let mut request = [0; 8192];
			let n = socket.read(&mut request).await.unwrap();
			assert!(n > 0);
			if hop == 0
				&& let Some(location) = &redirect
			{
				socket.write_all(format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
				continue;
			}
			socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).await.unwrap();
			socket.write_all(&body).await.unwrap();
		}
	});
	(url, task)
}

async fn fetch(url: &str) -> Result<Article> {
	load(
		url,
		false,
		&reqwest::header::HeaderMap::new(),
		Security::web(url.to_owned()),
		Default::default(),
		crate::lang::Lang::En,
	)
	.await
}

#[tokio::test]
async fn raw_markdown_uses_the_clipboard_parser_and_untrusted_permissions() {
	let source = "# Raw Markdown\n\n- [x] task\n\n```rust\nfn main() {}\n```\n\n$E=mc^2$\n\n![relative](image.png)\n";
	for (path, mime) in [
		("/file.md?download=1", "application/octet-stream"),
		("/FILE.MARKDOWN", "application/octet-stream"),
		("/file.mdown", "application/octet-stream"),
		("/file%2Emd", "application/octet-stream"),
		("/raw", "text/markdown; charset=UTF-8"),
		("/raw", "text/plain; charset=utf-8"),
		("/raw", "TEXT/X-MARKDOWN"),
	] {
		let (url, server) = serve(path, mime, source.as_bytes(), None).await;
		let article = fetch(&url).await.unwrap();
		server.await.unwrap();
		let pasted = LoadedDocument::markdown(Arc::new(
			markview_commonmark::parse(source),
		));
		assert_eq!(article.document.source(), Some(source));
		assert_eq!(article.document.content_key(), pasted.content_key());
		assert_eq!(
			article.document.input_fingerprint(),
			pasted.input_fingerprint()
		);
		assert_eq!(
			article.document.frontend_version(),
			pasted.frontend_version()
		);
		assert_eq!(article.title, "Raw Markdown");
		assert_eq!(article.security.origin, Origin::Web(url.clone()));
		assert_eq!(article.security.origin.trust(), Trust::Untrusted);
		assert!(
			article
				.security
				.check_file(std::path::Path::new("/tmp/secret.png"))
				.is_err()
		);
		let private = url::Url::parse("http://192.168.1.1/image.png").unwrap();
		assert!(
			article
				.security
				.check_address(&private, "192.168.1.1".parse().unwrap())
				.is_err()
		);
		let origin = url::Url::parse(&url).unwrap();
		assert!(
			article
				.security
				.check_address(&origin, "127.0.0.1".parse().unwrap())
				.is_ok()
		);
		assert!(
			article
				.security
				.for_content("changed")
				.check_address(&origin, "127.0.0.1".parse().unwrap())
				.is_err()
		);
		assert_eq!(
			crate::link::resolve_for(
				"file:///tmp/secret.md",
				None,
				&article.security
			),
			None
		);
	}
}

#[tokio::test]
async fn redirects_preserve_markdown_detection_and_the_final_resource_base() {
	for (path, destination) in
		[("/start", "/docs/file.md"), ("/file.md", "/docs/raw")]
	{
		let (url, server) = serve(
			path,
			"application/octet-stream",
			b"# Moved\n",
			Some(destination),
		)
		.await;
		let article = fetch(&url).await.unwrap();
		server.await.unwrap();
		assert_eq!(article.document.source(), Some("# Moved\n"));
		let final_url =
			url::Url::parse(&url).unwrap().join(destination).unwrap();
		assert_eq!(article.security.origin, Origin::Web(final_url.to_string()));
		assert_eq!(
			crate::link::resolve_for("next.md", None, &article.security),
			Some(crate::link::Target::Remote(
				final_url.join("next.md").unwrap().to_string()
			))
		);
	}
}

#[tokio::test]
async fn redirect_to_another_local_origin_is_blocked_before_connecting() {
	let destination = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
	destination.set_nonblocking(true).unwrap();
	let target =
		format!("http://{}/private.md", destination.local_addr().unwrap());
	let (url, server) =
		serve("/file.md", "text/markdown", b"", Some(&target)).await;
	let error = fetch(&url).await.unwrap_err();
	server.abort();
	let _ = server.await;
	assert!(
		error
			.downcast_ref::<crate::security::PermissionRequired>()
			.is_some()
	);
	assert_eq!(
		destination.accept().unwrap_err().kind(),
		std::io::ErrorKind::WouldBlock
	);
}

#[tokio::test]
async fn remote_markdown_refuses_an_oversized_response_before_reading_its_body()
{
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let url = format!("http://{}/large.md", listener.local_addr().unwrap());
	let server = tokio::spawn(async move {
		let (mut socket, _) = listener.accept().await.unwrap();
		let mut request = [0; 8192];
		assert!(socket.read(&mut request).await.unwrap() > 0);
		socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/markdown\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", crate::file::MAX_FILE_BYTES + 1).as_bytes()).await.unwrap();
	});
	let error = fetch(&url).await.unwrap_err();
	server.await.unwrap();
	assert!(format!("{error:#}").contains("exceeds its byte limit"));
}

#[tokio::test]
async fn html_pages_with_markdown_names_still_use_article_extraction() {
	let (url, server) = serve(
		"/blob/README.md",
		"text/html; charset=utf-8",
		include_bytes!("../../tests/fixtures/web-article.html"),
		None,
	)
	.await;
	let article = fetch(&url).await.unwrap();
	server.await.unwrap();
	assert!(article.document.source().is_none());
	assert_eq!(
		article.document.frontend_version(),
		100 + markview_webpage::PROFILE_VERSION
	);
}

#[tokio::test]
async fn remote_markdown_handles_utf8_bom_and_rejects_invalid_utf8() {
	for (body, expected) in [
		("\u{feff}# Heading\n".as_bytes(), Some("# Heading\n")),
		(&b"\xff\xfe"[..], None),
	] {
		let (url, server) = serve("/file.md", "text/plain", body, None).await;
		let result = fetch(&url).await;
		server.await.unwrap();
		match expected {
			Some(source) => {
				assert_eq!(result.unwrap().document.source(), Some(source))
			}
			None => assert!(result.unwrap_err().to_string().contains("UTF-8")),
		}
	}
}

#[tokio::test]
async fn remote_markdown_uses_the_selected_dialect_on_its_first_parse() {
	let source = "# Notes\n\n==highlight== %%hidden%%\n\nsoft\nwrapped\n";
	let (url, server) =
		serve("/file.md", "text/plain", source.as_bytes(), None).await;
	let dialect = markview_commonmark::Dialect {
		hard_breaks: true,
		highlight_marks: true,
		ofm: true,
	};
	let article = load(
		&url,
		false,
		&reqwest::header::HeaderMap::new(),
		Security::web(url.clone()),
		dialect,
		crate::lang::Lang::ZhHans,
	)
	.await
	.unwrap();
	server.await.unwrap();
	let LoadedDocument::Markdown(parse) = article.document.as_ref() else {
		panic!("expected Markdown");
	};
	assert_eq!(parse.dialect, dialect);
	let pasted = LoadedDocument::markdown(Arc::new(
		markview_commonmark::parse_with(source, dialect),
	));
	assert_eq!(article.document.content_key(), pasted.content_key());
	assert_eq!(
		article.document.frontend_version(),
		pasted.frontend_version()
	);
}

proptest::proptest! {
	#[test]
	fn markdown_detection_uses_only_the_path_extension(
		name in "[a-zA-Z0-9_-]{1,30}",
		query in "[a-zA-Z0-9._-]{0,40}",
		fragment in "[a-zA-Z0-9._-]{0,40}",
		extension in proptest::sample::select(vec!["md", "MD", "markdown", "MdOwN"]),
	) {
		let url = format!("https://example.org/{name}.{extension}?q={query}#{fragment}");
		proptest::prop_assert!(is_markdown(&url, None));
		proptest::prop_assert!(!is_markdown(&url, Some("text/html")));
		let html = format!("https://example.org/{name}.html?q={query}.md#{fragment}.md");
		proptest::prop_assert!(!is_markdown(&html, None));
	}

	#[test]
	fn mime_detection_is_independent_of_filename_and_ascii_case(
		name in "[a-zA-Z0-9_-]{1,30}",
		upper in proptest::prelude::any::<bool>(),
		parameters in proptest::sample::select(vec!["", "; charset=utf-8", " ; charset=UTF-8"]),
	) {
		let ordinary = format!("https://example.org/{name}.bin");
		let markdown = format!("https://example.org/{name}.md");
		for mime in ["text/markdown", "text/x-markdown", "text/plain"] {
			let mime = if upper { mime.to_ascii_uppercase() } else { mime.to_owned() };
			let content_type = format!("{mime}{parameters}");
			proptest::prop_assert!(is_markdown(&ordinary, Some(&content_type)));
		}
		for mime in ["text/html", "application/xhtml+xml"] {
			let mime = if upper { mime.to_ascii_uppercase() } else { mime.to_owned() };
			let content_type = format!("{mime}{parameters}");
			proptest::prop_assert!(!is_markdown(&markdown, Some(&content_type)));
		}
		proptest::prop_assert!(!is_markdown(&ordinary, Some("application/octet-stream")));
	}
}
