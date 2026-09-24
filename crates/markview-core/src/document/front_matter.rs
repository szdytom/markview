//! YAML front matter: the body between the delimiters, kept as source.
//!
//! Nothing here reads the YAML. A reader shows it as a `yaml` code block, as
//! the honest rendering of text whose structure is the author's, and parsing
//! it would put a whole map expansion between a document and the reader for a
//! result the page does not draw.

/// The language the front-matter source is drawn and highlighted as.
pub(crate) const LANGUAGE: &str = "yaml";

/// What a front-matter block holds.
pub(super) enum Content {
	/// No entries, so there is nothing to draw.
	Empty,
	/// The YAML between the delimiters, as source.
	Source(String),
}

/// Read the payload comrak hands over, delimiters included.
pub(super) fn parse(source: &str) -> Content {
	let yaml = strip(source).trim_end();
	if yaml.trim().is_empty() {
		Content::Empty
	} else {
		Content::Source(yaml.to_owned())
	}
}

/// The YAML between the delimiters, without the line ending the closing
/// delimiter leaves behind. Comrak keeps both delimiter lines and closes on
/// the first `---` line, so the body is what precedes it. A line that merely
/// opens with `---` is content — `---extra` closes nothing — so the delimiter
/// is matched the way comrak matched it, including a closing `---` that ends
/// the file with no line ending after it.
fn strip(source: &str) -> &str {
	let body = source.split_once('\n').map_or("", |(_, rest)| rest);
	body.match_indices("\n---")
		.find(|(index, _)| {
			// `None` is the end of the file, where comrak also closes.
			matches!(body.as_bytes().get(index + 4), None | Some(b'\n' | b'\r'))
		})
		.map_or("", |(index, _)| &body[..index + 1])
}
