//! Where the shaper gets its faces.
//!
//! A document normally shapes with the host's fonts. A caller that wants a
//! reproducible result, such as a PDF export that must not depend on what the
//! machine happens to have installed, names the directories to read instead
//! and can turn the system set off entirely.
use parley::FontContext;
use parley::fontique::{
	Blob, Collection, CollectionOptions, FamilyId, FontInfo, Script,
	SourceCache,
};
use std::{
	path::{Path, PathBuf},
	sync::{Arc, Mutex, OnceLock},
};

/// Which faces the shaper may use.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct FontConfig {
	/// Read only `directories`, never the host's own font set.
	pub ignore_system_fonts: bool,
	/// Font directories to scan. The collection is built once per distinct
	/// configuration and shared by every shaper that asks for it.
	pub directories: Vec<PathBuf>,
	/// Bumped when a directory's contents change, such as after a download.
	///
	/// Identity, not the paths alone, keys the collection cache: the same
	/// directories at a new revision are a different configuration and are
	/// scanned again, so a newly stored face is not hidden by an older scan.
	pub revision: u64,
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
/// A collection owns the font blobs `register` loaded, so an unbounded cache
/// keeps another copy of every downloaded file for every revision the process
/// has seen. A handful covers the configurations in use at once while still
/// reusing a repeated one.
const CACHE_CAP: usize = 4;

fn collection(config: &FontConfig) -> Collection {
	type Cache = Mutex<Vec<(FontConfig, Arc<OnceLock<Collection>>)>>;
	static CACHE: OnceLock<Cache> = OnceLock::new();
	let cache = CACHE.get_or_init(|| Mutex::new(Vec::new()));
	let slot =
		cached_slot(&mut cache.lock().expect("font collection cache"), config);
	// The build reads and parses files outside the cache lock, so a panic in
	// a font backend cannot poison it. The slot's own lock still makes exactly
	// one caller do the work.
	slot.get_or_init(|| build(config)).clone()
}

/// The cache slot for `config`, retiring the least recently used entry past
/// [`CACHE_CAP`].
///
/// A hit refreshes its entry, so a repeated configuration keeps the collection
/// it already built and the entry dropped is the one unused the longest.
fn cached_slot(
	cache: &mut Vec<(FontConfig, Arc<OnceLock<Collection>>)>,
	config: &FontConfig,
) -> Arc<OnceLock<Collection>> {
	if let Some(index) = cache.iter().position(|(key, _)| key == config) {
		let (_, slot) = cache.remove(index);
		cache.push((config.clone(), slot.clone()));
		return slot;
	}
	let slot = Arc::new(OnceLock::new());
	cache.push((config.clone(), slot.clone()));
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
	for directory in &config.directories {
		for (family, fonts) in register(&mut collection, directory) {
			// A family with several faces is reported once per file, and the
			// fallback order must not depend on the directory's own order.
			if !cjk.contains(&family)
				&& fonts.iter().any(|info| covers_cjk(info, &mut source_cache))
			{
				cjk.push(family);
			}
		}
	}
	// The stylesheet's `[cjk]` definition is the intended route, but a
	// document that leaves the convention unset falls back by script, and a
	// collection without system fonts has no fallback of its own. The loaded
	// directories supply one so CJK text still reaches a face that covers it.
	if !cjk.is_empty() {
		collection.set_fallbacks(Script::from_bytes(*b"Hani"), cjk.into_iter());
	}
	collection
}

/// Registers every font file directly inside `directory`, returning the
/// families added so the caller can recognize a CJK face.
fn register(
	collection: &mut Collection,
	directory: &Path,
) -> Vec<(FamilyId, Vec<FontInfo>)> {
	let entries = match std::fs::read_dir(directory) {
		Ok(entries) => entries,
		Err(error) => {
			log::warn!(
				"Font: cannot read directory {}: {error}",
				directory.display()
			);
			return Vec::new();
		}
	};
	let mut paths: Vec<PathBuf> =
		entries.flatten().map(|entry| entry.path()).collect();
	// Sorting keeps the registration order, and so the fallback order, the
	// same on every filesystem.
	paths.sort();
	let mut added = Vec::new();
	for path in paths {
		if !is_font_file(&path) {
			continue;
		}
		match std::fs::read(&path) {
			Ok(bytes) => {
				added.extend(
					collection.register_fonts(Blob::new(Arc::new(bytes)), None),
				);
			}
			Err(error) => {
				log::warn!("Font: cannot read {}: {error}", path.display())
			}
		}
	}
	added
}

/// Tables a renderable outline font must have.
const REQUIRED_TABLES: [[u8; 4]; 6] =
	[*b"head", *b"maxp", *b"hhea", *b"hmtx", *b"cmap", *b"name"];
/// The outline data itself: a TrueType face has `glyf`, a PostScript one
/// `CFF `, and a variable PostScript one `CFF2`. The shaper renders all three,
/// so any one of them is enough.
const OUTLINE_TABLES: [[u8; 4]; 3] = [*b"glyf", *b"CFF ", *b"CFF2"];

/// Whether `bytes` is a font file the shaper can load.
///
/// A downloaded body is checked before it is stored, so an error page or a
/// truncated transfer never becomes a registered face. Naming two tables is
/// not enough on its own: a body cut short can keep the early `head` and
/// `cmap` records while losing the outlines, metrics and names that sit later
/// in the file. The whole table directory is read instead, every record must
/// lie inside `bytes`, the tables an outline font needs must be present, and
/// the character map must resolve at least one code point.
pub fn is_font(bytes: &[u8]) -> bool {
	let Some(tables) = table_tags(bytes) else {
		return false;
	};
	if !REQUIRED_TABLES.iter().all(|tag| tables.contains(tag))
		|| !OUTLINE_TABLES.iter().any(|tag| tables.contains(tag))
	{
		return false;
	}
	swash::FontRef::from_index(bytes, 0)
		.is_some_and(|font| maps_a_character(&font))
}

/// The table tags of the first face in `bytes`, or `None` when a record does
/// not lie inside the body.
///
/// A collection names the first face's directory at an offset; a single font
/// starts at zero.
fn table_tags(bytes: &[u8]) -> Option<Vec<[u8; 4]>> {
	let base = if bytes.starts_with(b"ttcf") {
		u32_at(bytes, 12)? as usize
	} else {
		0
	};
	let count = u16_at(bytes, base.checked_add(4)?)? as usize;
	let start = base.checked_add(12)?;
	let mut tags = Vec::with_capacity(count);
	for index in 0..count {
		let record = start.checked_add(index.checked_mul(16)?)?;
		let end = record.checked_add(4)?;
		let tag: [u8; 4] = bytes.get(record..end)?.try_into().ok()?;
		let offset = u32_at(bytes, record.checked_add(8)?)? as usize;
		let length = u32_at(bytes, record.checked_add(12)?)? as usize;
		// Every record, not only the named tables, has to fit.
		if offset.checked_add(length)? > bytes.len() {
			return None;
		}
		tags.push(tag);
	}
	Some(tags)
}

fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
	let end = offset.checked_add(2)?;
	Some(u16::from_be_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
	let end = offset.checked_add(4)?;
	Some(u32::from_be_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

/// Whether the character map resolves at least one code point. A directory
/// can parse and still describe no usable character at all.
fn maps_a_character(font: &swash::FontRef<'_>) -> bool {
	let mut mapped = false;
	font.charmap().enumerate(|_, _| mapped = true);
	mapped
}

fn is_font_file(path: &Path) -> bool {
	path.extension()
		.and_then(|extension| extension.to_str())
		.is_some_and(|extension| {
			matches!(
				extension.to_ascii_lowercase().as_str(),
				"ttf" | "otf" | "ttc" | "otc"
			)
		})
}

/// Whether any style of a family maps a representative Han ideograph, which is
/// what makes it a useful script fallback.
fn covers_cjk(info: &FontInfo, source_cache: &mut SourceCache) -> bool {
	info.load(Some(source_cache)).is_some_and(|data| {
		swash::FontRef::from_index(data.as_ref(), info.index() as usize)
			.is_some_and(|font| font.charmap().map('中') != 0)
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_directory_supplies_families_and_a_cjk_fallback() {
		let config = FontConfig {
			ignore_system_fonts: true,
			directories: vec![
				Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts"),
			],
			..Default::default()
		};
		let mut context = context(&config);
		assert!(context.collection.family_by_name("Noto Serif").is_some());
		// No system fonts are loaded, so Han text only reaches a face through
		// the fallback the directory supplied.
		let fallback: Vec<_> = context
			.collection
			.fallback_families(Script::from_bytes(*b"Hani"))
			.collect();
		assert!(!fallback.is_empty());
	}

	#[test]
	fn an_unknown_directory_is_ignored_rather_than_fatal() {
		let config = FontConfig {
			ignore_system_fonts: true,
			directories: vec![Path::new("does-not-exist").to_owned()],
			..Default::default()
		};
		let mut context = context(&config);
		assert!(context.collection.family_by_name("Noto Serif").is_none());
	}

	/// A later download adds files to a directory already in the
	/// configuration, so the revision has to make the same paths read as a
	/// new collection.
	#[test]
	fn a_download_into_a_registered_directory_changes_the_collection() {
		let dir = tempfile::tempdir().unwrap();
		let download = |name: &str| {
			std::fs::copy(
				Path::new(env!("CARGO_MANIFEST_DIR"))
					.join("tests/fonts")
					.join(name),
				dir.path().join(name),
			)
			.unwrap();
		};
		// The directory is already registered and holds one face.
		download("NotoSerif-Regular-subset.otf");
		let config = FontConfig {
			ignore_system_fonts: true,
			directories: vec![dir.path().to_owned()],
			..Default::default()
		};
		let mut before = context(&config);
		assert!(before.collection.family_by_name("Noto Serif").is_some());
		assert!(before.collection.family_by_name("Noto Sans").is_none());
		// A second download lands another family without changing the paths.
		download("NotoSans-Regular-subset.otf");
		let mut downloaded = config.clone();
		downloaded.revision += 1;
		let mut after = context(&downloaded);
		assert!(after.collection.family_by_name("Noto Sans").is_some());
		// The old configuration keeps the collection it already built.
		let mut unchanged = context(&config);
		assert!(unchanged.collection.family_by_name("Noto Sans").is_none());
	}

	#[test]
	fn only_a_parsable_font_is_a_font() {
		let font = std::fs::read(
			Path::new(env!("CARGO_MANIFEST_DIR"))
				.join("tests/fonts/NotoSerif-Regular-subset.otf"),
		)
		.unwrap();
		assert!(is_font(&font));
		assert!(!is_font(b"<!doctype html><html>404"));
		assert!(!is_font(&font[..64]));
	}

	/// A body cut after `cmap` keeps the tables the old two-table check named
	/// while losing the outlines, metrics and name that follow them.
	#[test]
	fn a_truncated_font_is_rejected() {
		let font = std::fs::read(
			Path::new(env!("CARGO_MANIFEST_DIR"))
				.join("tests/fonts/NotoSerif-Regular-subset.otf"),
		)
		.unwrap();
		// The `cmap` record ends at 1,852 bytes, so `head` and `cmap` survive
		// the cut while `fpgm`, `glyf` and `name` do not.
		assert!(!is_font(&font[..1852]));
		assert!(is_font(&font));
	}

	/// A variable PostScript face stores its outlines in `CFF2` rather than
	/// `CFF `, and the shaper renders it, so a downloaded one must be accepted.
	/// The pinned test faces are static PostScript and TrueType, so only the
	/// whitelist itself can be asserted here.
	#[test]
	fn the_outline_whitelist_accepts_variable_postscript() {
		assert!(OUTLINE_TABLES.contains(b"CFF2"));
	}

	/// A collection owns its font blobs, so the cache must not keep one copy
	/// per revision forever; a repeated configuration still reuses its own.
	#[test]
	fn the_collection_cache_retires_obsolete_configurations() {
		let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts");
		let config = |revision| FontConfig {
			ignore_system_fonts: true,
			directories: vec![dir.clone()],
			revision,
		};
		let mut cache = Vec::new();
		for revision in 0..CACHE_CAP as u64 {
			cached_slot(&mut cache, &config(revision));
		}
		assert_eq!(cache.len(), CACHE_CAP);
		// A repeated configuration reuses its slot and becomes the newest.
		let slot = cached_slot(&mut cache, &config(0));
		assert_eq!(cache.len(), CACHE_CAP);
		assert!(Arc::ptr_eq(&slot, &cache.last().unwrap().1));
		// One more revision retires the least recently used entry, revision 1.
		cached_slot(&mut cache, &config(CACHE_CAP as u64));
		assert_eq!(cache.len(), CACHE_CAP);
		assert!(cache.iter().any(|(key, _)| key.revision == 0));
		assert!(!cache.iter().any(|(key, _)| key.revision == 1));
	}
}
