//! Where the shaper gets its faces.
//!
//! A document normally shapes with the host's fonts. Two callers do not, and
//! both say so here. One wants a reproducible result, such as a PDF export that
//! must not depend on what the machine happens to have installed: it names the
//! directories to read and turns the system set off. The other has no
//! filesystem at all: it hands its faces over as bytes.
//!
//! Reading a directory is the only thing here that needs a filesystem, so it
//! is the only thing the `font-directories` feature carries. A front end with
//! none builds this crate without it and uses [`FontConfig::from_faces`].
mod diagram;
#[cfg(feature = "font-directories")]
mod scan;
mod validate;

pub use diagram::{DiagramFace, DiagramFonts, FaceData};
pub use validate::{is_font, is_font_file, is_postscript_outline};

use crate::sync::cache;
use parley::FontContext;
use parley::fontique::{
	Blob, Collection, CollectionOptions, FamilyId, FontInfo, Script,
	SourceCache,
};
use std::{
	collections::HashSet,
	fmt,
	hash::{Hash, Hasher},
	path::PathBuf,
	sync::{Arc, Mutex, OnceLock},
};

#[cfg(feature = "font-directories")]
use scan::register;

/// Faces a host already holds in memory.
///
/// A front end without a filesystem hands its faces over here instead of naming
/// directories. The tag is what the collection cache compares: a host that
/// re-sends the same faces must send the same tag, so the collection is built
/// and cached once, and a host that sends different faces must send a new one.
/// A tag already in use for other faces is served the collection those built.
#[derive(Clone, Default)]
pub struct HostFaces {
	/// `None` until a host supplies faces, which is deliberately not a tag: a
	/// configuration that names no faces must not share a cache entry with one
	/// whose faces happen to be tagged zero, or the first would serve the
	/// second the empty collection it built.
	tag: Option<u64>,
	faces: Vec<Blob<u8>>,
}

impl HostFaces {
	/// Faces identified by `tag`.
	pub fn new(tag: u64, faces: Vec<Blob<u8>>) -> Self {
		Self {
			tag: Some(tag),
			faces,
		}
	}
}

// Only the tag is identity. The bytes are compared by nothing: a host that
// re-sends the same faces would otherwise rebuild a collection for no reason,
// and the whole point of the tag is that the host already knows they match.
impl PartialEq for HostFaces {
	fn eq(&self, other: &Self) -> bool {
		self.tag == other.tag
	}
}
impl Eq for HostFaces {}
impl Hash for HostFaces {
	fn hash<H: Hasher>(&self, state: &mut H) {
		self.tag.hash(state);
	}
}
impl fmt::Debug for HostFaces {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("HostFaces")
			.field("tag", &self.tag)
			.field("faces", &self.faces.len())
			.finish()
	}
}

/// Which faces the shaper may use.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct FontConfig {
	/// Read only `host` and `directories`, never the host's own font set.
	pub ignore_system_fonts: bool,
	/// Font directories to scan. The collection is built once per distinct
	/// configuration and shared by every shaper that asks for it.
	pub directories: Vec<PathBuf>,
	/// Faces the host supplies instead of, or beside, a directory.
	pub host: HostFaces,
	/// Bumped when a directory's contents change, such as after a download.
	///
	/// Identity, not the paths alone, keys the collection cache: the same
	/// directories at a new revision are a different configuration and are
	/// scanned again, so a newly stored face is not hidden by an older scan.
	pub revision: u64,
}

impl FontConfig {
	/// A configuration whose only faces are the ones the host supplies.
	///
	/// This is what a front end without a filesystem uses: it has no system
	/// fonts to find and no directory to scan.
	pub fn from_faces(tag: u64, faces: Vec<Blob<u8>>) -> Self {
		Self {
			ignore_system_fonts: true,
			host: HostFaces::new(tag, faces),
			..Self::default()
		}
	}
}

/// The collection `config` names, built once per distinct configuration so
/// repeated engines share one scan.
pub(crate) fn context(config: &FontConfig) -> FontContext {
	FontContext {
		collection: collection(config),
		source_cache: SourceCache::default(),
	}
}
/// How many built collections stay cached.
///
/// A collection owns the font blobs it registered, so an unbounded cache keeps
/// another copy of every downloaded file for every revision the process has
/// seen. A handful covers the configurations in use at once while still
/// reusing a repeated one.
const CACHE_CAP: usize = 4;

fn collection(config: &FontConfig) -> Collection {
	type Cache = Mutex<Vec<(FontConfig, Arc<OnceLock<Collection>>)>>;
	static CACHE: OnceLock<Cache> = OnceLock::new();
	let cache_mutex = CACHE.get_or_init(|| Mutex::new(Vec::new()));
	let slot = cached_slot(
		&mut cache(cache_mutex, "Font collection cache"),
		config.clone(),
		|| Arc::new(OnceLock::new()),
	);
	// The build reads and parses files outside the cache lock, so a panic in
	// a font backend cannot poison it. The slot's own lock still makes exactly
	// one caller do the work.
	slot.get_or_init(|| build(config)).clone()
}

/// The families `config` can shape with, in alphabetical order.
///
/// `han` keeps only the families whose character map covers a Han ideograph,
/// which is what makes one a useful fallback for a `[cjk]` font definition.
/// Reading the maps loads every family's faces, so the answer is cached per
/// configuration exactly as the collection itself is. Every name is stored for
/// the process, so a caller can name one in a label that outlives the list.
pub fn families(config: &FontConfig, han: bool) -> Arc<[&'static str]> {
	type Cache =
		Mutex<Vec<((FontConfig, bool), Arc<OnceLock<Arc<[&'static str]>>>)>>;
	static CACHE: OnceLock<Cache> = OnceLock::new();
	let cache_mutex = CACHE.get_or_init(|| Mutex::new(Vec::new()));
	let slot = cached_slot(
		&mut cache(cache_mutex, "Font family cache"),
		(config.clone(), han),
		|| Arc::new(OnceLock::new()),
	);
	// The build reads and parses faces outside the cache lock, so a panic in
	// a font backend cannot poison it. The slot's own lock still makes exactly
	// one caller do the work.
	slot.get_or_init(|| build_families(config, han)).clone()
}

fn build_families(config: &FontConfig, han: bool) -> Arc<[&'static str]> {
	let mut collection = collection(config);
	let mut names: Vec<&'static str> =
		collection.family_names().map(interned).collect();
	if han {
		let mut source_cache = SourceCache::default();
		names.retain(|name| {
			collection.family_by_name(name).is_some_and(|family| {
				family
					.fonts()
					.iter()
					.any(|info| covers_cjk(info, &mut source_cache))
			})
		});
	}
	names.sort_unstable();
	names.dedup();
	names.into()
}

/// Stores `name` for the life of the process and returns it.
///
/// A chooser names a family in a label the interface draws every frame, so a
/// name is kept once however many lists, roles or frames mention it: one small
/// string per family the machine has, rather than one per option per frame.
fn interned(name: &str) -> &'static str {
	static NAMES: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
	let names = NAMES.get_or_init(|| Mutex::new(HashSet::new()));
	let mut names = names.lock().expect("font family names");
	if let Some(stored) = names.get(name) {
		return stored;
	}
	let stored: &'static str = Box::leak(name.to_owned().into_boxed_str());
	names.insert(stored);
	stored
}

/// The cache slot for `key`, retiring the least recently used entry past
/// [`CACHE_CAP`].
///
/// A hit refreshes its entry, so a repeated key keeps the value it already
/// built and the entry dropped is the one unused the longest.
fn cached_slot<K: Eq + Clone, V: Clone>(
	cache: &mut Vec<(K, V)>,
	key: K,
	value: impl FnOnce() -> V,
) -> V {
	if let Some(index) = cache.iter().position(|(k, _)| *k == key) {
		let (_, slot) = cache.remove(index);
		cache.push((key, slot.clone()));
		return slot;
	}
	let slot = value();
	cache.push((key, slot.clone()));
	if cache.len() > CACHE_CAP {
		cache.remove(0);
	}
	slot
}

fn build(config: &FontConfig) -> Collection {
	let mut collection = Collection::new(CollectionOptions {
		shared: true,
		system_fonts: !config.ignore_system_fonts,
	});
	let mut source_cache = SourceCache::default();
	let mut cjk: Vec<FamilyId> = Vec::new();
	let mut added: Vec<(FamilyId, Vec<FontInfo>)> = Vec::new();
	// The host's own faces come first: it named them explicitly, so a family
	// it supplies wins over one a directory happens to hold. A body that is
	// not a face is refused here rather than registered as an empty family.
	for blob in &config.host.faces {
		if is_font(blob.data()) {
			added.extend(collection.register_fonts(blob.clone(), None));
		} else {
			log::warn!("Font: the host supplied a body that is not a face");
		}
	}
	#[cfg(feature = "font-directories")]
	for directory in &config.directories {
		added.extend(register(&mut collection, directory));
	}
	// A build without directory support cannot honour a named directory.
	// Saying so beats ignoring it: a silently empty collection is the failure
	// this feature exists to make impossible in the first place.
	#[cfg(not(feature = "font-directories"))]
	for directory in &config.directories {
		log::warn!(
			"Font: cannot read {} without the font-directories feature",
			directory.display()
		);
	}
	for (family, fonts) in added {
		// A family with several faces is reported once per file, and the
		// fallback order must not depend on the order faces arrived in.
		if !cjk.contains(&family)
			&& fonts.iter().any(|info| covers_cjk(info, &mut source_cache))
		{
			cjk.push(family);
		}
	}
	// The stylesheet's `[cjk]` definition is the intended route, but a
	// document that leaves the convention unset falls back by script, and a
	// collection without system fonts has no fallback of its own. The
	// registered faces supply one so CJK text still reaches a face that
	// covers it.
	if !cjk.is_empty() {
		collection.set_fallbacks(Script::from_bytes(*b"Hani"), cjk.into_iter());
	}
	collection
}
/// Whether any style of a family maps a representative Han ideograph, which is
/// what makes it a useful script fallback.
fn covers_cjk(info: &FontInfo, source_cache: &mut SourceCache) -> bool {
	info.load(Some(source_cache)).is_some_and(|data| {
		swash::FontRef::from_index(data.as_ref(), info.index() as usize)
			.is_some_and(|font| font.charmap().map('中') != 0)
	})
}
/// The first of `names` that `config` can already shape with, if any.
///
/// This is how a download is skipped: when one of a family's own names is
/// installed, or reachable through `--fonts`, the family is already there.
pub fn provided_family(
	config: &FontConfig,
	names: &[String],
) -> Option<String> {
	let mut collection = collection(config);
	names
		.iter()
		.find(|name| collection.family_id(name).is_some())
		.cloned()
}

/// Leaves a real poisoned lock for recovery tests.
#[cfg(test)]
pub(crate) fn poison<T: Send>(lock: &Mutex<T>) {
	std::thread::scope(|scope| {
		assert!(
			scope
				.spawn(|| {
					let _state = lock.lock().unwrap();
					panic!("injected failure");
				})
				.join()
				.is_err()
		);
	});
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::path::Path;

	/// A collection owns its font blobs, so the cache must not keep one copy
	/// per revision forever; a repeated configuration still reuses its own.
	#[test]
	fn the_collection_cache_retires_obsolete_configurations() {
		let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts");
		let config = |revision| FontConfig {
			ignore_system_fonts: true,
			directories: vec![dir.clone()],
			revision,
			..Default::default()
		};
		let mut cache = Vec::new();
		for revision in 0..CACHE_CAP as u64 {
			cached_slot(&mut cache, config(revision), || Arc::new(0));
		}
		assert_eq!(cache.len(), CACHE_CAP);
		// A repeated configuration reuses its slot and becomes the newest.
		let slot = cached_slot(&mut cache, config(0), || Arc::new(0));
		assert_eq!(cache.len(), CACHE_CAP);
		assert!(Arc::ptr_eq(&slot, &cache.last().unwrap().1));
		// One more revision retires the least recently used entry, revision 1.
		cached_slot(&mut cache, config(CACHE_CAP as u64), || Arc::new(0));
		assert_eq!(cache.len(), CACHE_CAP);
		assert!(cache.iter().any(|(key, _)| key.revision == 0));
		assert!(!cache.iter().any(|(key, _)| key.revision == 1));
	}
	#[test]
	fn a_failed_collection_build_keeps_the_cache_usable() {
		let config = FontConfig {
			ignore_system_fonts: true,
			..Default::default()
		};
		let mutex = Mutex::new(Vec::new());
		let slot = cached_slot(
			&mut cache(&mutex, "Test font cache"),
			config.clone(),
			|| Arc::new(OnceLock::new()),
		);
		std::thread::scope(|scope| {
			assert!(
				scope
					.spawn(|| slot.get_or_init(|| {
						assert!(mutex.try_lock().is_ok());
						panic!("injected build failure");
					}))
					.join()
					.is_err()
			);
		});
		assert!(!mutex.is_poisoned());
		assert!(Arc::ptr_eq(
			&slot,
			&cached_slot(
				&mut cache(&mutex, "Test font cache"),
				config.clone(),
				|| Arc::new(OnceLock::new()),
			)
		));
		slot.get_or_init(|| build(&config));
		poison(&mutex);
		let replacement = cached_slot(
			&mut cache(&mutex, "Test font cache"),
			config.clone(),
			|| Arc::new(OnceLock::new()),
		);
		assert!(!Arc::ptr_eq(&slot, &replacement));
		assert!(!mutex.is_poisoned());
		replacement.get_or_init(|| build(&config));
	}

	/// A host with no filesystem hands over bytes instead of a directory, and
	/// gets the collection a scan would have built.
	#[test]
	fn a_host_supplies_faces_as_bytes() {
		let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts");
		let face = |name: &str| {
			Blob::new(Arc::new(std::fs::read(dir.join(name)).unwrap()))
		};
		// The tag must not collide with another test's: it is the whole
		// identity of a hosted configuration.
		let config = FontConfig::from_faces(
			0x1001,
			vec![
				face("NotoSerif-Regular-subset.otf"),
				face("NotoSansCJKsc-Regular-subset.otf"),
			],
		);
		assert!(config.directories.is_empty());
		let mut served = context(&config);
		assert!(served.collection.family_by_name("Noto Serif").is_some());
		// A hosted collection reaches Han text through the same fallback a
		// scanned one does.
		let fallback: Vec<_> = served
			.collection
			.fallback_families(Script::from_bytes(*b"Hani"))
			.collect();
		assert!(!fallback.is_empty());
		// The tag alone is identity, so a re-send reuses the collection while
		// a different tag builds another.
		assert_eq!(config, FontConfig::from_faces(0x1001, Vec::new()));
		assert_ne!(config, FontConfig::from_faces(0x1002, Vec::new()));
	}

	/// A body the host hands over that is not a face is refused, not
	/// registered as a family that draws nothing.
	#[test]
	fn a_host_body_that_is_not_a_face_is_refused() {
		let config = FontConfig::from_faces(
			0x2001,
			vec![Blob::new(Arc::new(b"<!doctype html>".to_vec()))],
		);
		let mut served = context(&config);
		assert_eq!(served.collection.family_names().count(), 0);
	}

	/// A configuration that names no faces is not the same cache entry as one
	/// whose faces happen to be tagged zero. If they were, a host that supplied
	/// real faces would be served the empty collection the other built.
	#[test]
	fn an_unconfigured_host_is_not_a_tagged_one() {
		let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts");
		let face = Blob::new(Arc::new(
			std::fs::read(dir.join("NotoSerif-Regular-subset.otf")).unwrap(),
		));
		let unconfigured = FontConfig {
			ignore_system_fonts: true,
			..Default::default()
		};
		let tagged = FontConfig::from_faces(0, vec![face]);
		// What the cache keys on has to tell them apart, because what the two
		// build is not interchangeable.
		assert_ne!(unconfigured, tagged);
		let mut empty = build(&unconfigured);
		let mut supplied = build(&tagged);
		assert_eq!(empty.family_names().count(), 0);
		assert!(supplied.family_by_name("Noto Serif").is_some());
		// And so a local cache gives them separate slots.
		let mut cache = Vec::new();
		let empty_slot = cached_slot(&mut cache, &unconfigured, || Arc::new(0));
		let supplied_slot = cached_slot(&mut cache, &tagged, || Arc::new(0));
		assert!(!Arc::ptr_eq(&empty_slot, &supplied_slot));
	}

	/// A chooser lists every family the configuration can shape with, and a
	/// Han chooser keeps only the ones that draw a Han ideograph.
	#[test]
	fn a_family_list_keeps_only_han_families_where_asked() {
		let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts");
		let config = FontConfig {
			ignore_system_fonts: true,
			directories: vec![dir],
			..Default::default()
		};
		let all = families(&config, false);
		let han = families(&config, true);
		assert!(all.contains(&"Noto Sans"), "{all:?}");
		assert!(!han.contains(&"Noto Sans"), "{han:?}");
		assert!(han.iter().any(|name| name.contains("CJK")), "{han:?}");
		// Both lists read in alphabetical order, and a Han family is a subset.
		assert!(all.windows(2).all(|pair| pair[0] <= pair[1]));
		assert!(han.windows(2).all(|pair| pair[0] <= pair[1]));
		assert!(han.iter().all(|name| all.contains(name)));
	}
}
