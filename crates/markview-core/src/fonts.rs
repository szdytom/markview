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
}

/// The collection `config` names, built once per distinct configuration so
/// repeated engines share one scan.
pub(crate) fn context(config: &FontConfig) -> FontContext {
	FontContext {
		collection: collection(config),
		source_cache: SourceCache::default(),
	}
}

fn collection(config: &FontConfig) -> Collection {
	type Cache = Mutex<Vec<(FontConfig, Arc<OnceLock<Collection>>)>>;
	static CACHE: OnceLock<Cache> = OnceLock::new();
	let cache = CACHE.get_or_init(|| Mutex::new(Vec::new()));
	let slot = {
		let mut cache = cache.lock().expect("font collection cache");
		match cache.iter().find(|(key, _)| key == config) {
			Some((_, slot)) => slot.clone(),
			None => {
				// A process asks for very few distinct configurations, so
				// keeping every one is cheaper than rescanning on the next
				// call.
				let slot = Arc::new(OnceLock::new());
				cache.push((config.clone(), slot.clone()));
				slot
			}
		}
	};
	// The build reads and parses files outside the cache lock, so a panic in
	// a font backend cannot poison it. The slot's own lock still makes exactly
	// one caller do the work.
	slot.get_or_init(|| build(config)).clone()
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
		};
		let mut context = context(&config);
		assert!(context.collection.family_by_name("Noto Serif").is_none());
	}
}
