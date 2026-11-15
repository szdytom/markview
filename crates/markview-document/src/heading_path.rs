use crate::OutlineEntry;

/// Resolves an Obsidian heading path to its existing, unique fragment.
/// Each successive match must have a deeper heading level.
pub fn heading_path_anchor<'a>(
	outline: &'a [OutlineEntry],
	path: &str,
) -> Option<&'a str> {
	let mut parts = path.split('#').filter(|part| !part.is_empty());
	let mut wanted = key(parts.next()?);
	let mut level = 0;
	for entry in outline {
		if entry.level <= level || key(&entry.text) != wanted {
			continue;
		}
		let Some(next) = parts.next() else {
			return Some(&entry.anchor);
		};
		wanted = key(next);
		level = entry.level;
	}
	None
}

/// Obsidian compares headings without case, with link punctuation and runs
/// of whitespace replaced by a single space.
fn key(text: &str) -> String {
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
