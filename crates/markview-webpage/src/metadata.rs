//! Structured article metadata kept separate from the reading document.

/// What the extraction learned about the page.
#[derive(Clone, Debug, Default)]
pub struct ArticleMetadata {
	/// The article title, or the final URL when the page declared none.
	pub title: String,
	pub byline: Option<String>,
	pub site_name: Option<String>,
	pub lang: Option<String>,
	pub dir: Option<String>,
	pub published_time: Option<String>,
	pub modified_time: Option<String>,
	pub excerpt: Option<String>,
	/// The lead image, resolved against the final URL.
	pub image: Option<String>,
	/// The page's canonical URL, resolved against the final URL: the declared
	/// `rel="canonical"` link, or the `og:url` metadata when it declares
	/// nothing.
	pub canonical_url: Option<String>,
	/// The URL the response finally came from.
	pub final_url: String,
	/// Conversion diagnostics, such as unsupported MathML.
	pub diagnostics: Vec<String>,
}

impl ArticleMetadata {
	pub(crate) fn from_extracted(
		extracted: &dom_smoothie::Extracted,
		base: &url::Url,
		canonical: Option<String>,
	) -> Self {
		let metadata = &extracted.metadata;
		let resolve = |value: &Option<String>| {
			value.as_deref().and_then(|value| {
				base.join(value).ok().map(|url| url.to_string())
			})
		};
		let title = collapse(&metadata.title);
		Self {
			title: if title.is_empty() {
				base.to_string()
			} else {
				title
			},
			byline: metadata.byline.as_deref().map(collapse_owned),
			site_name: metadata.site_name.as_deref().map(collapse_owned),
			lang: metadata.lang.clone(),
			dir: metadata.dir.clone(),
			published_time: metadata.published_time.clone(),
			modified_time: metadata.modified_time.clone(),
			excerpt: metadata
				.excerpt
				.as_deref()
				.map(collapse_owned)
				.filter(|text| !text.is_empty()),
			image: resolve(&metadata.image),
			canonical_url: canonical.or_else(|| resolve(&metadata.url)),
			final_url: base.to_string(),
			diagnostics: Vec::new(),
		}
	}

	pub(crate) fn with_diagnostics(
		mut self,
		mut diagnostics: Vec<String>,
	) -> Self {
		self.diagnostics.append(&mut diagnostics);
		self
	}
}

/// Collapses ASCII whitespace, leaving non-breaking spaces intact.
pub(crate) fn collapse_owned(text: impl AsRef<str>) -> String {
	collapse(text.as_ref())
}

pub(crate) fn collapse(text: &str) -> String {
	let mut out = String::with_capacity(text.len());
	let mut space = false;
	for c in text.chars() {
		if c.is_ascii_whitespace() {
			space = true;
			continue;
		}
		if space && !out.is_empty() {
			out.push(' ');
		}
		space = false;
		out.push(c);
	}
	out
}
