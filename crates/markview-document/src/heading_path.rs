use crate::OutlineEntry;

/// Resolves an Obsidian heading path to its existing, unique fragment.
/// Each successive match must be inside the preceding heading's section.
pub fn heading_path_anchor<'a>(
	outline: &'a [OutlineEntry],
	path: &str,
) -> Option<&'a str> {
	let parts: Vec<_> = path
		.split('#')
		.filter(|part| !part.is_empty())
		.map(key)
		.collect();
	let mut levels = Vec::new();
	for entry in outline {
		while levels.last().is_some_and(|level| entry.level <= *level) {
			levels.pop();
		}
		if parts
			.get(levels.len())
			.is_some_and(|wanted| key(&entry.text) == *wanted)
		{
			levels.push(entry.level);
			if levels.len() == parts.len() {
				return Some(&entry.anchor);
			}
		}
	}
	None
}

/// Obsidian compares headings without case, with link punctuation and runs
/// of whitespace replaced by a single space.
pub(crate) fn key(text: &str) -> String {
	text.to_lowercase()
		.replace(
			[
				'!', '"', '#', '$', '%', '&', '(', ')', '*', '+', ',', '.',
				':', ';', '<', '=', '>', '?', '@', '^', '`', '{', '|', '}',
				'~', '/', '[', ']', '\\',
			],
			" ",
		)
		.split_whitespace()
		.collect::<Vec<_>>()
		.join(" ")
}
