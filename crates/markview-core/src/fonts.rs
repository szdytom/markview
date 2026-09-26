//! Where the shaper gets its faces.
//!
//! A document normally shapes with the host's fonts. A caller that wants a
//! reproducible result, such as a PDF export that must not depend on what the
//! machine happens to have installed, names the directories to read instead
//! and can turn the system set off entirely.
use parley::FontContext;
use parley::fontique::{
	Blob, Collection, CollectionOptions, FamilyId, FontInfo, FontStyle,
	FontWeight, FontWidth, GenericFamily, Script, SourceCache,
};
use std::{
	collections::HashMap,
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
		match map_font(&path) {
			Ok(Some(bytes)) => {
				added.extend(
					collection.register_fonts(Blob::new(Arc::new(bytes)), None),
				);
			}
			Ok(None) => {}
			Err(error) => {
				log::warn!("Font: cannot read {}: {error}", path.display())
			}
		}
	}
	added
}

/// Maps a font file instead of copying it into memory.
///
/// The collection owns the mapping and the shaper pages in only the tables the
/// document touches, so a directory of downloaded faces costs what is drawn
/// rather than the whole directory. An empty file holds no face, so it is
/// skipped instead of reported.
#[allow(unsafe_code)]
fn map_font(path: &Path) -> std::io::Result<Option<memmap2::Mmap>> {
	let file = std::fs::File::open(path)?;
	if file.metadata()?.len() == 0 {
		return Ok(None);
	}
	// SAFETY: the mapping is read-only, and a face is installed under its
	// final name by renaming a fresh file into place, so the bytes behind a
	// mapping are never rewritten or shortened in place.
	unsafe { memmap2::Mmap::map(&file) }.map(Some)
}

/// Tables a renderable face must have.
const REQUIRED_TABLES: [[u8; 4]; 6] =
	[*b"head", *b"maxp", *b"hhea", *b"hmtx", *b"cmap", *b"name"];
/// The outline data itself: a TrueType face has `glyf`, a PostScript one
/// `CFF `, and a variable PostScript one `CFF2`. The shaper renders all three,
/// so any one of them is enough.
const OUTLINE_TABLES: [[u8; 4]; 3] = [*b"glyf", *b"CFF ", *b"CFF2"];

/// Whether the directory names pixels the shaper can draw.
///
/// A color emoji face may carry no outline at all: `Noto Color Emoji` stores
/// its images as `CBDT` strikes located by `CBLC`, and some platforms use an
/// `sbix` table. The rasterizer draws both, so they count beside the outlines.
fn has_drawable_glyphs(tables: &[[u8; 4]]) -> bool {
	OUTLINE_TABLES.iter().any(|tag| tables.contains(tag))
		|| (tables.contains(b"CBDT") && tables.contains(b"CBLC"))
		|| tables.contains(b"sbix")
}

/// Whether `bytes` is a font file the shaper can load.
///
/// A downloaded body is checked before it is stored, so an error page or a
/// truncated transfer never becomes a registered face. Naming two tables is
/// not enough on its own: a body cut short can keep the early `head` and
/// `cmap` records while losing the outlines, metrics and names that sit later
/// in the file. The whole table directory is read instead, every record must
/// lie inside `bytes`, the tables a drawable face needs must be present, and
/// the character map must resolve at least one code point.
pub fn is_font(bytes: &[u8]) -> bool {
	let Some(tables) = table_tags(bytes) else {
		return false;
	};
	if !REQUIRED_TABLES.iter().all(|tag| tables.contains(tag))
		|| !has_drawable_glyphs(&tables)
	{
		return false;
	}
	swash::FontRef::from_index(bytes, 0)
		.is_some_and(|font| maps_a_character(&font))
}

/// Whether the first face draws with PostScript outlines, which is what names
/// an extensionless download `.otf` rather than `.ttf`.
pub fn is_postscript_outline(bytes: &[u8]) -> bool {
	table_tags(bytes)
		.is_some_and(|tags| tags.contains(b"CFF ") || tags.contains(b"CFF2"))
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

/// What one font file on disk says about itself.
///
/// A downloaded family is only ever known through its own files: the reader
/// reads their names and attributes instead of trusting a manifest, so a file
/// copied in by hand is described exactly like one this application wrote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaceInfo {
	/// The file name inside its directory, which is what a reader shows.
	pub file: String,
	/// Family names the file declares, the typographic family first. Every
	/// localized spelling is kept, because one family may be named differently
	/// per language.
	pub families: Vec<String>,
	/// The attributes of the first face. A collection's faces differ from one
	/// another, and only the family names are needed to recognize a family.
	pub weight: u16,
	pub style: FaceStyle,
	/// How many faces the file holds; a collection holds several.
	pub faces: usize,
	pub bytes: u64,
}

/// The slant a face declares for itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceStyle {
	Normal,
	Italic,
	Oblique,
}

/// Reads every font file directly inside `directory` and reports what each one
/// says about itself, in file-name order. A file that cannot be read is
/// skipped, exactly as the shaper's own scan does.
pub fn describe(directory: &Path) -> Vec<FaceInfo> {
	let Ok(entries) = std::fs::read_dir(directory) else {
		return Vec::new();
	};
	let mut paths: Vec<PathBuf> =
		entries.flatten().map(|entry| entry.path()).collect();
	// Sorting keeps the report stable on every filesystem.
	paths.sort();
	paths.iter().filter_map(|path| describe_one(path)).collect()
}

fn describe_one(path: &Path) -> Option<FaceInfo> {
	if !is_font_file(path) {
		return None;
	}
	let file = path.file_name()?.to_str()?.to_owned();
	let bytes = std::fs::read(path).ok()?;
	let bytes_len = bytes.len() as u64;
	let data = swash::FontDataRef::new(&bytes)?;
	// The typographic family groups every weight of one family; the plain
	// family is what an older face declares instead. Every face contributes,
	// because a collection may hold several families.
	let mut typographic: Vec<String> = Vec::new();
	let mut plain: Vec<String> = Vec::new();
	let mut first: Option<(u16, FaceStyle)> = None;
	for font in data.fonts() {
		if first.is_none() {
			let attributes = font.attributes();
			let style = match attributes.style() {
				swash::Style::Italic => FaceStyle::Italic,
				swash::Style::Oblique(_) => FaceStyle::Oblique,
				swash::Style::Normal => FaceStyle::Normal,
			};
			first = Some((attributes.weight().0, style));
		}
		for string in font.localized_strings() {
			if !string.is_decodable() {
				continue;
			}
			let name = string.chars().collect::<String>();
			let name = name.trim();
			if name.is_empty() {
				continue;
			}
			let name = name.to_owned();
			match string.id() {
				swash::StringId::TypographicFamily => {
					push_name(&mut typographic, name)
				}
				swash::StringId::Family => push_name(&mut plain, name),
				_ => {}
			}
		}
	}
	typographic.extend(plain);
	let (weight, style) = first?;
	Some(FaceInfo {
		file,
		families: typographic,
		// A collection's faces differ; these describe its first face and are
		// shown for it alone.
		weight,
		style,
		faces: data.len(),
		bytes: bytes_len,
	})
}

fn push_name(names: &mut Vec<String>, name: String) {
	if !names.contains(&name) {
		names.push(name);
	}
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

/// Whether any style of a family maps a representative Han ideograph, which is
/// what makes it a useful script fallback.
fn covers_cjk(info: &FontInfo, source_cache: &mut SourceCache) -> bool {
	info.load(Some(source_cache)).is_some_and(|data| {
		swash::FontRef::from_index(data.as_ref(), info.index() as usize)
			.is_some_and(|font| font.charmap().map('中') != 0)
	})
}

/// One face of the shaper's collection, for a caller that measures or draws
/// outside the shaper.
#[derive(Clone, Debug)]
pub struct DiagramFace {
	pub family: String,
	pub weight: u16,
	pub italic: bool,
	bytes: Blob<u8>,
	index: u32,
}

impl DiagramFace {
	/// A key that tells two faces of one file apart, so a rasterizer can find
	/// the face it already loaded.
	pub fn key(&self) -> (u64, u32) {
		(self.bytes.id(), self.index)
	}

	/// The face's own file data, shared with the shaper's collection.
	pub fn bytes(&self) -> &Blob<u8> {
		&self.bytes
	}

	/// The same data in the shape a rasterizer's font database stores it.
	pub fn data(&self) -> FaceData {
		FaceData(self.bytes.clone())
	}

	pub fn index(&self) -> u32 {
		self.index
	}
}

/// A face's data for a rasterizer that keeps its own font sources.
#[derive(Clone)]
pub struct FaceData(Blob<u8>);

impl AsRef<[u8]> for FaceData {
	fn as_ref(&self) -> &[u8] {
		self.0.data()
	}
}

/// The advance tables of one face, filled in as it is used.
struct FaceMetrics {
	face: DiagramFace,
	units_per_em: f32,
	glyphs: HashMap<char, Option<u16>>,
	advances: HashMap<u16, f32>,
}

impl FaceMetrics {
	fn new(face: DiagramFace) -> Self {
		let units_per_em =
			swash::FontRef::from_index(face.bytes.data(), face.index as usize)
				.map(|font| font.metrics(&[]).units_per_em as f32)
				.filter(|units| *units > 0.0)
				.unwrap_or(1000.0);
		Self {
			face,
			units_per_em,
			glyphs: HashMap::new(),
			advances: HashMap::new(),
		}
	}

	/// The glyph a character maps to, or `None` when the face has none.
	fn glyph(&mut self, ch: char) -> Option<u16> {
		if let Some(glyph) = self.glyphs.get(&ch) {
			return *glyph;
		}
		let glyph = swash::FontRef::from_index(
			self.face.bytes.data(),
			self.face.index as usize,
		)
		.map(|font| font.charmap().map(ch))
		.filter(|glyph| *glyph != 0);
		self.glyphs.insert(ch, glyph);
		glyph
	}

	/// The advance width of one character, or `None` when the face has no
	/// glyph for it.
	fn advance(&mut self, ch: char, size: f32) -> Option<f32> {
		let glyph = self.glyph(ch)?;
		let units = *self.advances.entry(glyph).or_insert_with(|| {
			swash::FontRef::from_index(
				self.face.bytes.data(),
				self.face.index as usize,
			)
			.map(|font| font.glyph_metrics(&[]).advance_width(glyph))
			.unwrap_or(0.0)
		});
		Some(units / self.units_per_em * size)
	}

	fn covers(&mut self, ch: char) -> bool {
		self.glyph(ch).is_some()
	}
}

struct DiagramInner {
	collection: Collection,
	sources: SourceCache,
	/// The families the stylesheet's Han font definitions name, in order.
	han: Vec<String>,
	/// Every face of one family, the shaper's first choice first.
	families: HashMap<String, Arc<Vec<Arc<Mutex<FaceMetrics>>>>>,
	/// Every face seen so far, by the key that identifies it.
	faces: HashMap<(u64, u32), Arc<Mutex<FaceMetrics>>>,
	/// Production Mermaid requests opt out of collection-wide fallback scans.
	restricted: bool,
	/// What the unrestricted compatibility path found for one character.
	scanned: HashMap<char, Option<(u64, u32)>>,
}

/// The generic family names a font list may carry, with the substring that
/// names a face when the collection maps none and a substring that rules one
/// out. A pinned directory has no generic map of its own, so a name hint is
/// the only thing a measurement and a rasterizer can share.
const GENERICS: [(&str, GenericFamily, &str, &str); 5] = [
	("serif", GenericFamily::Serif, "serif", ""),
	("sans-serif", GenericFamily::SansSerif, "sans", "mono"),
	("monospace", GenericFamily::Monospace, "mono", ""),
	("cursive", GenericFamily::Cursive, "", ""),
	("fantasy", GenericFamily::Fantasy, "", ""),
];

/// The faces a diagram may be measured and drawn with: the same collection,
/// built by the same scan, that the reader's own text is shaped with.
///
/// A caller that rasterizes the SVG itself needs the faces for its own font
/// database ([`Self::faces`]) and, per character, the face the stylesheet's
/// list resolves to ([`Self::cover`]); a caller that only lays the diagram out
/// asks for a width ([`Self::measure`]). Both resolve through this one policy,
/// so the boxes the layout computes match the text that is drawn.
pub struct DiagramFonts {
	inner: Mutex<DiagramInner>,
}

impl DiagramFonts {
	/// Reads the faces `config` names, with `han` as the fallback families for
	/// Han text when the requested list has none.
	pub fn new(config: &FontConfig, han: &[String]) -> Self {
		Self {
			inner: Mutex::new(DiagramInner {
				collection: collection(config),
				sources: SourceCache::default(),
				han: han.to_vec(),
				families: HashMap::new(),
				faces: HashMap::new(),
				restricted: false,
				scanned: HashMap::new(),
			}),
		}
	}

	/// The width of `text` at `size`, in the first family of the
	/// comma-separated `families` that has faces.
	///
	/// A character the resolved faces do not draw is estimated the way the
	/// renderer estimates it — one em for Han and emoji, a little over half an
	/// em otherwise — so a measurement is available while any family exists.
	pub fn measure(
		&self,
		families: &str,
		text: &str,
		size: f32,
	) -> Option<f32> {
		if text.is_empty() || size <= 0.0 {
			return Some(0.0);
		}
		let mut inner = self.inner.lock().unwrap();
		let base = inner.base_face(families)?;
		let faces = inner.resolved(families, text, &base);
		let mut width = 0.0;
		for (ch, face) in text.chars().zip(faces) {
			width += match face {
				Some(face) => face
					.lock()
					.unwrap()
					.advance(ch, size)
					.unwrap_or_else(|| estimate(ch, size)),
				None => estimate(ch, size),
			};
		}
		Some(width)
	}

	/// The face that draws `ch` for this family list, for a rasterizer that
	/// falls back per character.
	pub fn cover(&self, families: &str, ch: char) -> Option<DiagramFace> {
		self.inner
			.lock()
			.unwrap()
			.cover(families, ch)
			.map(|face| face.lock().unwrap().face.clone())
	}

	/// The families this collection resolves the generic names a font list may
	/// carry to, for a rasterizer that resolves `serif` or `sans-serif` by
	/// itself. [`Self::measure`] uses the same resolution, so the face it
	/// measures with is the face the rasterizer draws.
	pub fn generics(&self) -> Vec<(&'static str, String)> {
		let mut inner = self.inner.lock().unwrap();
		GENERICS
			.into_iter()
			.filter_map(|(name, generic, hint, avoid)| {
				inner
					.generic_name(generic, hint, avoid)
					.map(|family| (name, family))
			})
			.collect()
	}

	/// Resolves a configured family name to a family present in the collection.
	pub fn resolve_family(&self, name: &str) -> Option<String> {
		let mut inner = self.inner.lock().unwrap();
		let name = inner.family_name(name)?;
		inner
			.collection
			.family_by_name(&name)
			.map(|family| family.name().to_owned())
	}

	/// Faces for the Mermaid candidates and the reader's selected Han fallback.
	///
	/// Resolving a diagram must not materialize every installed family. The
	/// stylesheet has already declared the only candidates this renderer may
	/// use, so keep the font database bounded to those families.
	pub fn faces_for(&self, families: &[String]) -> Vec<DiagramFace> {
		let mut inner = self.inner.lock().unwrap();
		inner.restricted = true;
		let mut names = families.to_vec();
		names.extend(inner.han.iter().cloned());
		let mut seen = HashMap::new();
		let mut out = Vec::new();
		for name in names {
			for face in inner.family(&name).iter() {
				let face = face.lock().unwrap().face.clone();
				if seen.insert(face.key(), ()).is_none() {
					out.push(face);
				}
			}
		}
		out
	}

	/// Every face of the collection, for a rasterizer's own font database.
	///
	/// The order is the collection's own, so a rasterizer that resolves a
	/// family by itself sees the same faces the shaper does.
	pub fn faces(&self) -> Vec<DiagramFace> {
		let mut inner = self.inner.lock().unwrap();
		let names: Vec<String> =
			inner.collection.family_names().map(str::to_owned).collect();
		let mut out = Vec::new();
		for name in names {
			for face in inner.family(&name).iter() {
				out.push(face.lock().unwrap().face.clone());
			}
		}
		out
	}
}

impl DiagramInner {
	/// The collection's family for a generic name: its own mapping first, then
	/// a face whose name suggests it when the collection maps none.
	fn generic_name(
		&mut self,
		generic: GenericFamily,
		hint: &str,
		avoid: &str,
	) -> Option<String> {
		let ids: Vec<FamilyId> =
			self.collection.generic_families(generic).collect();
		if let Some(family) = ids
			.into_iter()
			.find_map(|id| self.collection.family_name(id).map(str::to_owned))
		{
			return Some(family);
		}
		if hint.is_empty() {
			return None;
		}
		self.collection
			.family_names()
			.map(str::to_owned)
			.find(|family| {
				let family = family.to_ascii_lowercase();
				family.contains(hint)
					&& (avoid.is_empty() || !family.contains(avoid))
			})
	}

	/// The collection family a list entry names, resolving a generic such as
	/// `monospace` to a real family.
	fn family_name(&mut self, name: &str) -> Option<String> {
		for (generic, family, hint, avoid) in GENERICS {
			if generic.eq_ignore_ascii_case(name) {
				return self.generic_name(family, hint, avoid);
			}
		}
		Some(name.to_owned())
	}

	/// The faces of one family, the shaper's first choice first.
	fn family(&mut self, name: &str) -> Arc<Vec<Arc<Mutex<FaceMetrics>>>> {
		if let Some(faces) = self.families.get(name) {
			return faces.clone();
		}
		let mut faces = Vec::new();
		if let Some(name) = self.family_name(name)
			&& let Some(family) = self.collection.family_by_name(&name)
		{
			// The family's own match order first, the rest after it.
			let mut infos: Vec<&FontInfo> = Vec::new();
			if let Some(best) = family.match_font(
				FontWidth::default(),
				FontStyle::Normal,
				FontWeight::default(),
				false,
			) {
				infos.push(best);
			}
			let rest: Vec<&FontInfo> = family
				.fonts()
				.iter()
				.filter(|info| {
					!infos.iter().any(|chosen| {
						chosen.source().id() == info.source().id()
							&& chosen.index() == info.index()
					})
				})
				.collect();
			infos.extend(rest);
			for info in infos {
				let Some(bytes) = self.sources.get(info.source()) else {
					continue;
				};
				faces.push(Arc::new(Mutex::new(FaceMetrics::new(
					DiagramFace {
						family: family.name().to_owned(),
						weight: info.weight().value() as u16,
						italic: !matches!(info.style(), FontStyle::Normal),
						bytes,
						index: info.index(),
					},
				))));
			}
		}
		let faces = Arc::new(faces);
		for face in faces.iter() {
			self.faces
				.insert(face.lock().unwrap().face.key(), face.clone());
		}
		self.families.insert(name.to_owned(), faces.clone());
		faces
	}

	/// The face a rasterizer's own selector picks for this list: the first
	/// family the collection has. A measurement starts from it, exactly as the
	/// rasterizer's `select_font` starts from the list.
	fn base_face(&mut self, families: &str) -> Option<Arc<Mutex<FaceMetrics>>> {
		split_families(families)
			.into_iter()
			.find_map(|name| self.family(&name).first().cloned())
	}

	/// The face each character of `text` is drawn with. The base face draws
	/// what it covers; a character it cannot draw falls back per character,
	/// but a fallback that covers the whole text replaces the base face
	/// everywhere, which is what the rasterizer's own fallback does.
	fn resolved(
		&mut self,
		families: &str,
		text: &str,
		base: &Arc<Mutex<FaceMetrics>>,
	) -> Vec<Option<Arc<Mutex<FaceMetrics>>>> {
		let chars: Vec<char> = text.chars().collect();
		let mut faces: Vec<Option<Arc<Mutex<FaceMetrics>>>> = {
			let mut metrics = base.lock().unwrap();
			chars
				.iter()
				.map(|ch| metrics.covers(*ch).then(|| base.clone()))
				.collect()
		};
		let mut used = vec![base.lock().unwrap().face.key()];
		while let Some(index) = faces.iter().position(Option::is_none) {
			let Some(fallback) = self.cover(families, chars[index]) else {
				break;
			};
			let key = fallback.lock().unwrap().face.key();
			if used.contains(&key) {
				break;
			}
			let covers_all = {
				let mut metrics = fallback.lock().unwrap();
				chars.iter().all(|ch| metrics.covers(*ch))
			};
			if covers_all {
				faces.fill(Some(fallback));
				break;
			}
			{
				let mut metrics = fallback.lock().unwrap();
				for (face, ch) in faces.iter_mut().zip(&chars) {
					if face.is_none() && metrics.covers(*ch) {
						*face = Some(fallback.clone());
					}
				}
			}
			used.push(key);
		}
		faces
	}

	/// The face that draws `ch`: the requested families in order, then the
	/// stylesheet's selected Han families.
	fn cover(
		&mut self,
		families: &str,
		ch: char,
	) -> Option<Arc<Mutex<FaceMetrics>>> {
		let mut names = split_families(families);
		names.extend(self.han.iter().cloned());
		if let Some(face) =
			names.iter().find_map(|name| self.covering_face(name, ch))
		{
			return Some(face);
		}
		if self.restricted {
			return None;
		}
		if let Some(cached) = self.scanned.get(&ch) {
			return cached.and_then(|key| self.faces.get(&key).cloned());
		}
		let rest: Vec<String> = self
			.collection
			.family_names()
			.map(str::to_owned)
			.filter(|name| !names.contains(name))
			.collect();
		let found = rest.iter().find_map(|name| self.covering_face(name, ch));
		self.scanned.insert(
			ch,
			found.as_ref().map(|face| face.lock().unwrap().face.key()),
		);
		found
	}

	/// The first face of `name` that draws `ch`.
	fn covering_face(
		&mut self,
		name: &str,
		ch: char,
	) -> Option<Arc<Mutex<FaceMetrics>>> {
		self.family(name)
			.iter()
			.find(|face| face.lock().unwrap().covers(ch))
			.cloned()
	}
}

/// The families of a comma-separated list, trimmed and without the quotes a
/// renderer's font list may carry.
fn split_families(families: &str) -> Vec<String> {
	families
		.split(',')
		.map(|name| name.trim().trim_matches(['"', '\'']).to_owned())
		.filter(|name| !name.is_empty())
		.collect()
}

/// The width a character takes when no resolved face draws it. Han, kana and
/// emoji are drawn about one em wide by whatever the system falls back to;
/// everything else averages a little over half an em.
fn estimate(ch: char, size: f32) -> f32 {
	if ch.is_control()
		|| matches!(
			ch as u32,
			0x200b..=0x200f | 0xfe00..=0xfe0f | 0xe0100..=0xe01ef
		) {
		return 0.0;
	}
	if crate::microtype::is_han_kana(ch)
		|| matches!(
			ch as u32,
			0x3000..=0x303f | 0xff00..=0xffef | 0x1f000..=0x1faff
		) {
		size
	} else {
		size * 0.56
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Packs single-face fonts into one collection, so a test can describe a
	/// file that holds more than one family.
	fn collection_of(faces: &[Vec<u8>]) -> Vec<u8> {
		let mut out = Vec::new();
		out.extend(b"ttcf");
		out.extend(1u32.to_be_bytes());
		out.extend((faces.len() as u32).to_be_bytes());
		out.extend(std::iter::repeat_n(0u8, 4 * faces.len()));
		for (index, face) in faces.iter().enumerate() {
			let start = out.len();
			let slot = 12 + index * 4;
			out[slot..slot + 4].copy_from_slice(&(start as u32).to_be_bytes());
			let count = u16::from_be_bytes([face[4], face[5]]) as usize;
			out.extend(&face[..12]);
			let mut tables = Vec::new();
			for table in 0..count {
				let record = 12 + table * 16;
				let at = u32::from_be_bytes(
					face[record + 8..record + 12].try_into().unwrap(),
				) as usize;
				let length = u32::from_be_bytes(
					face[record + 12..record + 16].try_into().unwrap(),
				) as usize;
				tables.push(face[at..at + length].to_vec());
				out.extend(&face[record..record + 8]);
				out.extend(0u32.to_be_bytes());
				out.extend((length as u32).to_be_bytes());
			}
			for (table, data) in tables.iter().enumerate() {
				while out.len() % 4 != 0 {
					out.push(0);
				}
				let at = out.len();
				let slot = start + 12 + table * 16 + 8;
				out[slot..slot + 4].copy_from_slice(&(at as u32).to_be_bytes());
				out.extend(data);
			}
		}
		out
	}

	/// A collection holds several families; describing only its first face
	/// would hide the rest from the download catalogue.
	#[test]
	fn a_collection_reports_every_family() {
		let dir = tempfile::tempdir().unwrap();
		let read = |name: &str| {
			std::fs::read(
				Path::new(env!("CARGO_MANIFEST_DIR"))
					.join("tests/fonts")
					.join(name),
			)
			.unwrap()
		};
		let faces = vec![
			read("NotoSerif-Regular-subset.otf"),
			read("NotoSans-Regular-subset.otf"),
		];
		let bytes = collection_of(&faces);
		std::fs::write(dir.path().join("both.ttc"), &bytes).unwrap();
		let described = describe(dir.path());
		assert_eq!(described.len(), 1);
		assert_eq!(described[0].file, "both.ttc");
		assert_eq!(described[0].faces, 2);
		assert_eq!(described[0].bytes, bytes.len() as u64);
		let names = &described[0].families;
		assert!(
			names.iter().any(|name| name.contains("Noto Serif")),
			"{names:?}"
		);
		assert!(
			names.iter().any(|name| name.contains("Noto Sans")),
			"{names:?}"
		);
	}

	#[test]
	fn a_face_is_described_from_its_own_tables() {
		let dir = tempfile::tempdir().unwrap();
		let bytes = std::fs::read(
			Path::new(env!("CARGO_MANIFEST_DIR"))
				.join("tests/fonts/NotoSerif-Regular-subset.otf"),
		)
		.unwrap();
		std::fs::write(dir.path().join("face.ttf"), &bytes).unwrap();
		// A file that is not a font is skipped rather than reported.
		std::fs::write(dir.path().join("notes.txt"), b"x").unwrap();
		let faces = describe(dir.path());
		assert_eq!(faces.len(), 1);
		assert_eq!(faces[0].file, "face.ttf");
		assert_eq!(faces[0].bytes, bytes.len() as u64);
		assert_eq!(faces[0].style, FaceStyle::Normal);
		assert!((1..=1000).contains(&faces[0].weight), "{}", faces[0].weight);
		assert!(
			faces[0]
				.families
				.iter()
				.any(|name| name.contains("Noto Serif")),
			"{:?}",
			faces[0].families
		);
		// An absent directory describes nothing instead of failing.
		assert!(describe(&dir.path().join("missing")).is_empty());
	}

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

	/// An empty file in a font directory holds no face, and one beside a real
	/// face must not cost it.
	#[test]
	fn an_empty_font_file_is_skipped_beside_a_real_one() {
		let dir = tempfile::tempdir().unwrap();
		std::fs::write(dir.path().join("empty.ttf"), b"").unwrap();
		std::fs::copy(
			Path::new(env!("CARGO_MANIFEST_DIR"))
				.join("tests/fonts/NotoSerif-Regular-subset.otf"),
			dir.path().join("face.otf"),
		)
		.unwrap();
		let config = FontConfig {
			ignore_system_fonts: true,
			directories: vec![dir.path().to_owned()],
			..Default::default()
		};
		let mut context = context(&config);
		assert!(context.collection.family_by_name("Noto Serif").is_some());
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
		// The collection the old configuration built before the download is
		// untouched. (Assert on `before` rather than re-asking the global
		// cache: a parallel test can evict the cached slot in between.)
		assert!(before.collection.family_by_name("Noto Sans").is_none());
	}

	#[test]
	fn only_a_parsable_font_is_a_font() {
		let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts");
		let font =
			std::fs::read(dir.join("NotoSerif-Regular-subset.otf")).unwrap();
		assert!(is_font(&font));
		assert!(!is_font(b"<!doctype html><html>404"));
		assert!(!is_font(&font[..64]));
		// A color emoji face keeps its pixels in `CBDT` strikes and has no
		// outline table, and the shaper draws it all the same.
		let emoji =
			std::fs::read(dir.join("NotoColorEmoji-subset.ttf")).unwrap();
		assert!(is_font(&emoji));
		assert!(!is_font(&emoji[..64]));
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

	/// A diagram measured outside the shaper resolves through the shaper's
	/// own collection, so the boxes it computes match the text that is drawn.
	#[test]
	fn diagram_fonts_measure_and_cover_from_the_shapers_collection() {
		let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts");
		let config = FontConfig {
			ignore_system_fonts: true,
			directories: vec![dir],
			revision: 0,
		};
		let fonts = DiagramFonts::new(&config, &[]);
		assert!(!fonts.faces().is_empty(), "no faces in the collection");
		assert_eq!(
			fonts.resolve_family("noto sans").as_deref(),
			Some("Noto Sans")
		);
		// A family the collection has measures, one it does not declines.
		let latin = fonts.measure("Noto Sans", "Hello", 16.0).unwrap();
		assert!(latin > 16.0, "{latin}");
		assert!(fonts.measure("No Such Family", "Hello", 16.0).is_none());
		// The face a character resolves to draws it.
		let covered = fonts.cover("Noto Sans", 'H').expect("H is covered");
		assert_eq!(covered.family, "Noto Sans");
		// Han text is estimated at an em when no named family draws it, and a
		// Han family is used when one is named.
		let estimated = fonts
			.measure("Noto Sans", "\u{4e2d}\u{5b57}", 16.0)
			.unwrap();
		assert!((estimated - 32.0).abs() < 0.01, "{estimated}");
		let han = fonts
			.cover("Noto Serif CJK SC", '\u{4e2d}')
			.expect("a Han face draws it");
		assert_eq!(han.family, "Noto Serif CJK SC");
		// Generic families resolve through the pinned collection too.
		let generic = |name: &str| {
			fonts
				.generics()
				.into_iter()
				.find(|(generic, _)| *generic == name)
				.map(|(_, family)| family)
		};
		let mono = generic("monospace").expect("a mono face in the directory");
		assert_eq!(
			fonts.measure("monospace", "Hello", 16.0),
			fonts.measure(&mono, "Hello", 16.0)
		);
		let sans = generic("sans-serif").expect("a sans face in the directory");
		assert_eq!(
			fonts.measure("sans-serif", "Hello", 16.0),
			fonts.measure(&sans, "Hello", 16.0)
		);
	}

	#[test]
	fn diagram_faces_are_limited_to_requested_families() {
		let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts");
		let config = FontConfig {
			ignore_system_fonts: true,
			directories: vec![dir],
			revision: 0,
		};
		let fonts = DiagramFonts::new(&config, &[]);
		let faces = fonts.faces_for(&["Noto Sans".into()]);
		assert!(!faces.is_empty());
		assert!(faces.iter().all(|face| face.family == "Noto Sans"));
	}

	/// A mixed-script label is measured with the face the rasterizer draws it
	/// with. The base face draws the Latin, a CJK face draws the Han, and
	/// because that fallback covers the Latin too the rasterizer redraws the
	/// whole label with it, so the measurement has to follow.
	#[test]
	fn a_whole_run_fallback_replaces_the_base_face() {
		let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts");
		let config = FontConfig {
			ignore_system_fonts: true,
			directories: vec![dir],
			revision: 0,
		};
		let fonts = DiagramFonts::new(&config, &[]);
		let fallback = fonts
			.cover("Noto Sans", '\u{4e2d}')
			.expect("a Han face draws it");
		let drawn = {
			let mut inner = fonts.inner.lock().unwrap();
			let base = inner.base_face("Noto Sans").expect("a base face");
			inner
				.resolved("Noto Sans", "A\u{4e2d}", &base)
				.into_iter()
				.map(|face| {
					face.expect("every character resolves")
						.lock()
						.unwrap()
						.face
						.family
						.clone()
				})
				.collect::<Vec<_>>()
		};
		assert_eq!(drawn, vec![fallback.family.clone(), fallback.family]);
		assert_eq!(
			fonts.measure("Noto Sans", "A\u{4e2d}", 16.0),
			fonts.measure(&drawn[0], "A\u{4e2d}", 16.0)
		);
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
