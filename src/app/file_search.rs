//! Pure workspace path matching, ranking and grapheme-safe match snippets.
use nucleo_matcher::{
	Matcher, Utf32String,
	pattern::{CaseMatching, Normalization, Pattern},
};
use std::{
	collections::{HashMap, HashSet},
	ops::Range,
	path::{Path, PathBuf},
	sync::Arc,
};
use unicode_segmentation::UnicodeSegmentation;

const RESULTS: usize = 64;
pub(super) type Matches = (Arc<[Arc<File>]>, HashMap<usize, Snippet>);

#[derive(Debug)]
pub(super) struct File {
	pub id: usize,
	pub path: PathBuf,
	pub name: Arc<str>,
	pub parent: Arc<str>,
	text: Utf32String,
	depth: usize,
}
impl File {
	pub fn new(id: usize, root: &Path, path: &Path) -> Arc<Self> {
		let relative = path.strip_prefix(root).unwrap();
		Arc::new(Self {
			id,
			path: path.to_owned(),
			name: path.file_name().unwrap().to_string_lossy().as_ref().into(),
			parent: relative
				.parent()
				.unwrap()
				.to_string_lossy()
				.as_ref()
				.into(),
			text: relative
				.to_string_lossy()
				.replace('\\', "/")
				.as_str()
				.into(),
			depth: relative.components().count(),
		})
	}
	fn relative(&self) -> String {
		if self.parent.is_empty() {
			self.name.to_string()
		} else {
			format!("{}/{}", self.parent, self.name).replace('\\', "/")
		}
	}
}

/// Workspace entries retain their IDs; open files add unique paths after them.
pub(super) fn catalog<'a>(
	workspace: impl Iterator<Item = Arc<File>>,
	opened: impl Iterator<Item = &'a Path>,
) -> Arc<[Arc<File>]> {
	let mut files: Vec<_> = workspace.collect();
	let mut paths: HashSet<_> =
		files.iter().map(|file| file.path.clone()).collect();
	let mut next = files
		.iter()
		.map(|file| file.id)
		.max()
		.map_or(0, |id| id + 1);
	for path in opened {
		if paths.insert(path.to_owned()) {
			files.push(File::new(next, Path::new(""), path));
			next += 1;
		}
	}
	files.into()
}

#[derive(Debug)]
pub(super) struct Snippet {
	pub text: String,
	/// UTF-8 byte ranges covering whole matched graphemes.
	pub matched: Vec<Range<usize>>,
}
impl Snippet {
	fn new(path: &str, indices: &[u32]) -> Self {
		let graphemes: Vec<_> = path.graphemes(true).collect();
		let start = (indices[0] as usize).saturating_sub(6);
		let end = (indices.last().unwrap() + 13) as usize;
		let end = end.min(graphemes.len()).min(start + 96);
		let mut text = if start > 0 {
			"…".into()
		} else {
			String::new()
		};
		let mut matched: Vec<Range<usize>> = Vec::new();
		for (index, grapheme) in
			graphemes.iter().enumerate().take(end).skip(start)
		{
			let byte = text.len();
			text.push_str(grapheme);
			if indices.binary_search(&(index as u32)).is_ok() {
				if let Some(previous) = matched.last_mut()
					&& previous.end == byte
				{
					previous.end = text.len();
				} else {
					matched.push(byte..text.len());
				}
			}
		}
		if end < graphemes.len() {
			text.push('…');
		}
		Self { text, matched }
	}
}

pub(super) fn find(
	matcher: &mut Matcher,
	catalog: &[Arc<File>],
	query: &str,
	cancelled: impl Fn() -> bool,
) -> Option<Matches> {
	let query = query.replace('\\', "/");
	let pattern = Pattern::new(
		&query,
		CaseMatching::Ignore,
		Normalization::Smart,
		nucleo_matcher::pattern::AtomKind::Fuzzy,
	);
	let mut scored = Vec::new();
	for (index, file) in catalog.iter().enumerate() {
		if index % 128 == 0 && cancelled() {
			return None;
		}
		if let Some(score) = pattern.score(file.text.slice(..), matcher) {
			scored.push((score, index));
		}
	}
	let order = |a: &(u32, usize), b: &(u32, usize)| {
		b.0.cmp(&a.0)
			.then_with(|| catalog[a.1].depth.cmp(&catalog[b.1].depth))
			.then_with(|| catalog[a.1].name.cmp(&catalog[b.1].name))
			.then_with(|| a.1.cmp(&b.1))
	};
	let limit = if query.trim().is_empty() { 3 } else { RESULTS };
	if scored.len() > limit {
		scored.select_nth_unstable_by(limit, order);
		scored.truncate(limit);
	}
	scored.sort_unstable_by(order);
	let mut matches = Vec::with_capacity(scored.len());
	let mut snippets = HashMap::new();
	let mut indices = Vec::new();
	for (_, index) in scored {
		if cancelled() {
			return None;
		}
		let file = &catalog[index];
		if !query.trim().is_empty() {
			indices.clear();
			pattern
				.indices(file.text.slice(..), matcher, &mut indices)
				.unwrap();
			indices.sort_unstable();
			indices.dedup();
			let name_start =
				file.text.len() - file.name.graphemes(true).count();
			let snippet = if indices[0] as usize >= name_start {
				for index in &mut indices {
					*index -= name_start as u32;
				}
				Snippet::new(&file.name, &indices)
			} else {
				Snippet::new(&file.relative(), &indices)
			};
			snippets.insert(file.id, snippet);
		}
		matches.push(file.clone());
	}
	Some((matches.into(), snippets))
}

#[cfg(test)]
mod tests {
	use super::*;
	use nucleo_matcher::Config;
	use std::time::Instant;

	#[test]
	fn match_snippets_preserve_graphemes_and_skip_long_unmatched_prefixes() {
		let root = Path::new("workspace");
		let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
		for (name, query, expected) in [
			("cafe\u{301}-notes.md", "cafe", "cafe\u{301}"),
			("阅读-notes.md", "阅读", "阅读"),
			("CHANGELOG.md", "chlg", "CHLG"),
		] {
			let file = File::new(
				0,
				root,
				&root
					.join("a-long-unmatched-parent-directory/nested")
					.join(name),
			);
			let (_, snippets) =
				find(&mut matcher, &[file], query, || false).unwrap();
			let snippet = &snippets[&0];
			assert!(!snippet.text.contains("unmatched-parent"));
			let matched = snippet
				.matched
				.iter()
				.map(|range| &snippet.text[range.clone()])
				.collect::<String>();
			assert_eq!(matched, expected);
			assert!(snippet.text.graphemes(true).count() <= 98);
		}
	}

	#[test]
	#[ignore = "manual file matching performance measurement"]
	fn workspace_search_performance() {
		let root = Path::new("workspace");
		let started = Instant::now();
		let files: Vec<_> = (0..100_000)
			.map(|index| {
				File::new(
					index,
					root,
					&root.join(format!(
						"docs/group-{}/reading-{index}.md",
						index % 100
					)),
				)
			})
			.collect();
		eprintln!("Prepare 100000 paths: {:?}", started.elapsed());
		let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
		for query in
			["", "r", "read", "group-19", "read99999", "no_match_at_all"]
		{
			let started = Instant::now();
			let (result, _) =
				find(&mut matcher, &files, query, || false).unwrap();
			eprintln!(
				"{query:?}: {:?}, {} results",
				started.elapsed(),
				result.len()
			);
			assert!(result.len() <= RESULTS);
		}
		assert!(find(&mut matcher, &files, "read", || true).is_none());
	}
}
