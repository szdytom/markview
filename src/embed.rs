//! Native loading and dependency tracking for semantic note embeds.
use crate::{
	loaded::LoadedDocument,
	security::{Origin, Security, Trust},
};
use markview_commonmark::{Dialect, MarkdownParse};
use markview_document::{
	BlockId, Document, Link,
	embed::{self, Expansion, Request, SourceId, Target},
};
use markview_platform::{FileVersion, directory_version, file_version};
use std::{
	collections::{BTreeMap, HashMap, HashSet},
	fs,
	path::{Path, PathBuf},
	sync::{Arc, Mutex},
	time::Duration,
};
use url::Url;

pub(crate) const CHECK_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Clone, Debug, Default)]
pub(crate) struct Dependencies {
	files: BTreeMap<PathBuf, Option<FileVersion>>,
	locations: BTreeMap<PathBuf, Option<Location>>,
	directories: Arc<BTreeMap<PathBuf, DirectoryStamp>>,
}

#[derive(Debug)]
struct DirectoryStamp {
	stamp: Option<Mutex<FileVersion>>,
	entries: u64,
}

impl Dependencies {
	pub fn changed(&self) -> bool {
		self.files
			.iter()
			.any(|(path, stamp)| file_version(path) != *stamp)
			|| self
				.locations
				.iter()
				.any(|(path, target)| Location::read(path) != *target)
			|| self
				.directories
				.iter()
				.any(|(path, directory)| directory.changed(path))
	}

	pub fn is_empty(&self) -> bool {
		self.files.is_empty()
			&& self.locations.is_empty()
			&& self.directories.is_empty()
	}

	pub fn source_files(&self) -> impl Iterator<Item = &Path> {
		self.files.keys().map(PathBuf::as_path)
	}

	fn observe(&mut self, path: &Path) -> Option<FileVersion> {
		let stamp = file_version(path);
		self.files.entry(path.to_owned()).or_insert(stamp);
		stamp
	}
}

impl DirectoryStamp {
	fn changed(&self, path: &Path) -> bool {
		let Some(stamp) = &self.stamp else {
			return self.entries_changed(path);
		};
		let mut previous = stamp.lock().unwrap();
		let Some(current) = directory_version(path) else {
			return self.entries_changed(path);
		};
		if *previous == current {
			return false;
		}
		if self.entries_changed(path) {
			return true;
		}
		*previous = current;
		false
	}

	fn entries_changed(&self, path: &Path) -> bool {
		markview_document::fingerprint(&workspace_entries(path, || false))
			!= self.entries
	}
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Location {
	path: PathBuf,
	file: bool,
}

impl Location {
	fn read(path: &Path) -> Option<Self> {
		let path = path.canonicalize().ok()?;
		let file = path.is_file();
		Some(Self { path, file })
	}

	fn markdown(&self) -> bool {
		self.file && crate::link::is_markdown(&self.path)
	}
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lookup {
	Note,
	Link,
}

/// The source path of a file or the resource path supplied for a buffer.
#[derive(Clone, Copy)]
pub(crate) enum InputPath<'a> {
	File(&'a Path),
	Buffer(&'a Path),
}

impl<'a> InputPath<'a> {
	pub fn path(self) -> &'a Path {
		match self {
			Self::File(path) | Self::Buffer(path) => path,
		}
	}
}

pub(crate) type SourcePaths = HashMap<SourceId, PathBuf>;

#[derive(Debug)]
pub(crate) struct Composition {
	pub document: Document,
	pub origins: HashMap<BlockId, embed::Origin>,
	pub paths: Arc<SourcePaths>,
	sources: HashMap<SourceId, Arc<MarkdownParse>>,
	pub dependencies: Dependencies,
}

impl Composition {
	pub fn imported_size(&self) -> usize {
		self.sources
			.iter()
			.filter(|(id, _)| **id != SourceId(0))
			.map(|(_, parse)| parse.source.len())
			.sum()
	}

	#[cfg(test)]
	pub fn origin(
		&self,
		block: BlockId,
	) -> Option<(&Path, &MarkdownParse, BlockId)> {
		let origin = self.origins.get(&block)?;
		Some((
			&self.paths[&origin.source],
			&self.sources[&origin.source],
			origin.block,
		))
	}
}

#[derive(Default)]
pub(crate) struct Loader {
	cache: HashMap<PathBuf, Cached>,
	workspace: Option<Workspace>,
	prepared: Option<Prepared>,
}

struct Prepared {
	path: PathBuf,
	file: bool,
	workspace: Option<PathBuf>,
	trusted: bool,
	document: Arc<LoadedDocument>,
}

struct Cached {
	stamp: FileVersion,
	parse: Arc<MarkdownParse>,
}

struct Workspace {
	root: PathBuf,
	notes: Vec<PathBuf>,
	dependencies: Dependencies,
}

impl Loader {
	pub fn prepare(
		&mut self,
		input: Arc<LoadedDocument>,
		input_path: InputPath<'_>,
		workspace: Option<&Path>,
		security: &Security,
		cancelled: impl Fn() -> bool,
	) -> Option<Arc<LoadedDocument>> {
		if cancelled() {
			return None;
		}
		let Some(root) = input
			.markdown_parse()
			.filter(|parse| parse.document.has_note_embeds())
		else {
			self.cache.clear();
			self.prepared = None;
			self.workspace = None;
			return Some(input);
		};
		let trusted = matches!(security.origin, Origin::Local(Trust::Trusted));
		let path = input_path.path();
		let file = matches!(input_path, InputPath::File(_));
		if let Some(prepared) = &self.prepared
			&& prepared.path == path
			&& prepared.file == file
			&& prepared.workspace.as_deref() == workspace
			&& prepared.trusted == trusted
			&& Arc::ptr_eq(prepared.document.markdown_parse().unwrap(), root)
			&& prepared
				.document
				.composition()
				.is_none_or(|composition| !composition.dependencies.changed())
		{
			return (!cancelled()).then(|| prepared.document.clone());
		}
		let document = self.expand(
			root.clone(),
			input_path,
			workspace,
			security,
			trusted,
			cancelled,
		)?;
		self.prepared = Some(Prepared {
			path: path.to_owned(),
			file,
			workspace: workspace.map(Path::to_owned),
			trusted,
			document: document.clone(),
		});
		Some(document)
	}

	fn expand(
		&mut self,
		root: Arc<MarkdownParse>,
		input_path: InputPath<'_>,
		workspace: Option<&Path>,
		security: &Security,
		trusted: bool,
		cancelled: impl Fn() -> bool,
	) -> Option<Arc<LoadedDocument>> {
		self.workspace = self.workspace.take().filter(|index| {
			Some(index.root.as_path()) == workspace
				&& !index.dependencies.changed()
		});
		let mut sources = HashMap::from([(SourceId(0), root.clone())]);
		let mut ids = HashMap::new();
		let source_path = match input_path {
			InputPath::File(path) => {
				let canonical =
					fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
				ids.insert(canonical.clone(), SourceId(0));
				canonical
			}
			InputPath::Buffer(path) => path.to_owned(),
		};
		let mut paths = HashMap::from([(SourceId(0), source_path)]);
		let mut dependencies = Dependencies::default();
		let mut lookups = HashMap::<Request, Option<SourceId>>::new();
		let mut read_bytes = 0;
		let mut retained = HashSet::new();
		loop {
			if cancelled() {
				return None;
			}
			let targets = lookups
				.iter()
				.map(|(request, id)| {
					let result = id.map_or(Target::Unavailable, |id| {
						Target::Loaded(embed::Source {
							id,
							document: &sources[&id].document,
						})
					});
					(request.clone(), result)
				})
				.collect();
			let source = embed::Source {
				id: SourceId(0),
				document: &root.document,
			};
			match embed::expand(&source, &targets) {
				Expansion::NeedsTargets(requests) => {
					for request in requests {
						if cancelled() {
							return None;
						}
						let loaded = trusted
							.then(|| {
								self.find_path(
									&request.note,
									&paths[&request.from],
									workspace,
									&mut dependencies,
									&cancelled,
									Lookup::Note,
								)
							})
							.flatten()
							.and_then(|target| {
								if let Some(id) = ids.get(&target) {
									return Some(*id);
								}
								security.check_file(&target).ok()?;
								let stamp = dependencies.observe(&target)?;
								let remaining =
									crate::file::MAX_FILE_BYTES - read_bytes;
								if stamp.size > remaining {
									return None;
								}
								let cached =
									self.parse(&target, stamp, root.dialect)?;
								let bytes = cached.source.len() as u64;
								if bytes > remaining {
									return None;
								}
								read_bytes += bytes;
								let id = SourceId(sources.len() as u32);
								retained.insert(target.clone());
								ids.insert(target.clone(), id);
								paths.insert(id, target);
								sources.insert(id, cached);
								Some(id)
							});
						lookups.insert(request, loaded);
					}
				}
				Expansion::Ready(mut expanded) => {
					let mut addresses = HashMap::new();
					let mut resolved = HashMap::new();
					for source in &expanded.source_links {
						if cancelled() {
							return None;
						}
						// Host fragments address the assembled document.
						if source.source == SourceId(0)
							&& (!trusted
								|| source.link.as_str().starts_with(['#', '?']))
						{
							continue;
						}
						let occurrence = source.link.occurrence();
						let address = resolved
							.entry((source.source, source.link.as_str()))
							.or_insert_with(|| {
								self.rebase_link(
									source.link.as_str(),
									&paths[&source.source],
									workspace,
									&mut dependencies,
									&cancelled,
								)
								.map(|address| Link::new(address, occurrence))
							});
						if let Some(address) = address {
							addresses.insert(
								occurrence,
								address.with_occurrence(occurrence),
							);
						}
					}
					expanded.replace_link_addresses(&addresses);
					self.cache.retain(|path, _| retained.contains(path));
					if cancelled() {
						return None;
					}
					if expanded.origins.is_empty() && dependencies.is_empty() {
						return Some(LoadedDocument::markdown(root));
					}
					return Some(LoadedDocument::composed(
						root,
						Composition {
							document: expanded.document,
							origins: expanded.origins,
							paths: Arc::new(paths),
							sources,
							dependencies,
						},
					));
				}
			}
		}
	}

	fn parse(
		&mut self,
		path: &Path,
		stamp: FileVersion,
		dialect: Dialect,
	) -> Option<Arc<MarkdownParse>> {
		if let Some(cached) = self.cache.get(path)
			&& cached.stamp == stamp
			&& cached.parse.dialect == dialect
		{
			return Some(cached.parse.clone());
		}
		let source: Arc<str> = crate::file::read_document(path).ok()?.into();
		if file_version(path) != Some(stamp) {
			return None;
		}
		let parse = Arc::new(markview_commonmark::parse_with(source, dialect));
		self.cache.insert(
			path.to_owned(),
			Cached {
				stamp,
				parse: parse.clone(),
			},
		);
		Some(parse)
	}

	fn find_path(
		&mut self,
		address: &str,
		from: &Path,
		workspace: Option<&Path>,
		dependencies: &mut Dependencies,
		cancelled: &impl Fn() -> bool,
		lookup: Lookup,
	) -> Option<PathBuf> {
		let decoded = crate::link::workspace_target(address)?;
		let name = Path::new(decoded.as_ref());
		for candidate in
			crate::link::workspace_candidates(name, from.parent()?, workspace)
		{
			if lookup == Lookup::Note && !crate::link::is_markdown(&candidate) {
				continue;
			}
			let target = Location::read(&candidate);
			dependencies
				.locations
				.entry(candidate)
				.or_insert_with(|| target.clone());
			if let Some(target) = target
				&& (lookup == Lookup::Link || target.markdown())
			{
				return Some(target.path);
			}
		}
		let workspace = workspace?;
		if self.workspace.is_none() {
			self.workspace = Workspace::scan(workspace, cancelled);
		}
		let index = self.workspace.as_ref()?;
		dependencies.directories = index.dependencies.directories.clone();
		crate::link::unique_workspace_match(
			name,
			index.notes.iter().map(PathBuf::as_path),
		)
		.and_then(|path| path.canonicalize().ok())
		.filter(|path| crate::link::is_markdown(path))
	}

	fn rebase_link(
		&mut self,
		url: &str,
		from: &Path,
		workspace: Option<&Path>,
		dependencies: &mut Dependencies,
		cancelled: &impl Fn() -> bool,
	) -> Option<String> {
		if Url::parse(url).is_ok() || url.starts_with("//") {
			return None;
		}
		let fragment = url.find('#').unwrap_or(url.len());
		let end = url.find(['#', '?']).unwrap_or(url.len());
		let (address, suffix) = url.split_at(end);
		if address.is_empty() {
			return Url::from_file_path(from)
				.ok()
				.map(|base| format!("{base}{suffix}"));
		}
		if Path::new(address).is_absolute() {
			return None;
		}
		let literal =
			(workspace.is_some() && fragment != end).then_some(fragment);
		for end in literal.into_iter().chain(std::iter::once(end)) {
			let (address, suffix) = url.split_at(end);
			if let Some(target) = self.find_path(
				address,
				from,
				workspace,
				dependencies,
				cancelled,
				Lookup::Link,
			) {
				return Url::from_file_path(target)
					.ok()
					.map(|base| format!("{base}{suffix}"));
			}
		}
		Url::from_file_path(from)
			.ok()?
			.join(url)
			.ok()
			.map(Into::into)
	}
}

impl Workspace {
	fn scan(root: &Path, cancelled: &impl Fn() -> bool) -> Option<Self> {
		let mut index = Self {
			root: root.to_owned(),
			notes: Vec::new(),
			dependencies: Dependencies::default(),
		};
		let mut pending = vec![root.to_owned()];
		let mut directories = BTreeMap::new();
		while let Some(path) = pending.pop() {
			if cancelled() {
				return None;
			}
			let stamp = directory_version(&path).map(Mutex::new);
			let entries = workspace_entries(&path, cancelled);
			if cancelled() {
				return None;
			}
			directories.insert(
				path,
				DirectoryStamp {
					stamp,
					entries: markview_document::fingerprint(&entries),
				},
			);
			for (path, directory) in entries {
				if directory {
					pending.push(path);
				} else {
					index.notes.push(path);
				}
			}
		}
		index.dependencies.directories = Arc::new(directories);
		Some(index)
	}
}

#[cfg(test)]
thread_local! {
	static DIRECTORY_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn workspace_entries(
	path: &Path,
	cancelled: impl Fn() -> bool,
) -> Vec<(PathBuf, bool)> {
	#[cfg(test)]
	DIRECTORY_READS.with(|reads| reads.set(reads.get() + 1));
	let mut entries: Vec<_> = fs::read_dir(path)
		.into_iter()
		.flatten()
		.flatten()
		.take_while(|_| !cancelled())
		.filter_map(|entry| {
			let kind = entry.file_type().ok()?;
			let path = entry.path();
			let directory = kind.is_dir();
			(!crate::link::workspace_excluded(&path, directory)
				&& (directory
					|| (kind.is_file() && crate::link::is_markdown(&path))))
			.then_some((path, directory))
		})
		.collect();
	entries.sort_unstable();
	entries
}

#[cfg(test)]
mod tests;
