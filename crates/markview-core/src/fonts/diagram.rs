//! Measuring and drawing with the same faces the shaper uses.
use super::{FontConfig, collection};
use crate::sync::available;
use parley::fontique::{
	Blob, Collection, FamilyId, FontInfo, FontStyle, FontWeight, FontWidth,
	GenericFamily, SourceCache,
};
use std::{
	collections::HashMap,
	sync::{Arc, Mutex},
};

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
/// Poisoned policy state declines queries; poisoned faces are skipped.
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
		let mut inner = available(&self.inner, "Diagram fonts")?;
		let base = inner.base_face(families)?;
		let faces = inner.resolved(families, text, &base);
		let mut width = 0.0;
		for (ch, face) in text.chars().zip(faces) {
			width += match face {
				Some(face) => available(&face, "Font metrics")
					.and_then(|mut metrics| metrics.advance(ch, size))
					.unwrap_or_else(|| estimate(ch, size)),
				None => estimate(ch, size),
			};
		}
		Some(width)
	}

	/// The face that draws `ch` for this family list, for a rasterizer that
	/// falls back per character.
	pub fn cover(&self, families: &str, ch: char) -> Option<DiagramFace> {
		let face =
			available(&self.inner, "Diagram fonts")?.cover(families, ch)?;
		Some(available(&face, "Font metrics")?.face.clone())
	}

	/// The families this collection resolves the generic names a font list may
	/// carry to, for a rasterizer that resolves `serif` or `sans-serif` by
	/// itself. [`Self::measure`] uses the same resolution, so the face it
	/// measures with is the face the rasterizer draws.
	pub fn generics(&self) -> Vec<(&'static str, String)> {
		let Some(mut inner) = available(&self.inner, "Diagram fonts") else {
			return Vec::new();
		};
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
		let mut inner = available(&self.inner, "Diagram fonts")?;
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
		let Some(mut inner) = available(&self.inner, "Diagram fonts") else {
			return Vec::new();
		};
		inner.restricted = true;
		let mut names = families.to_vec();
		names.extend(inner.han.iter().cloned());
		let mut seen = HashMap::new();
		let mut out = Vec::new();
		for name in names {
			for face in inner.family(&name).iter() {
				let Some(face) = available(face, "Font metrics")
					.map(|metrics| metrics.face.clone())
				else {
					continue;
				};
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
		let Some(mut inner) = available(&self.inner, "Diagram fonts") else {
			return Vec::new();
		};
		let names: Vec<String> =
			inner.collection.family_names().map(str::to_owned).collect();
		let mut out = Vec::new();
		for name in names {
			for face in inner.family(&name).iter() {
				if let Some(metrics) = available(face, "Font metrics") {
					out.push(metrics.face.clone());
				}
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
			if let Some(metrics) = available(face, "Font metrics") {
				self.faces.insert(metrics.face.key(), face.clone());
			}
		}
		self.families.insert(name.to_owned(), faces.clone());
		faces
	}

	/// The face a rasterizer's own selector picks for this list: the first
	/// family the collection has. A measurement starts from it, exactly as the
	/// rasterizer's `select_font` starts from the list.
	fn base_face(&mut self, families: &str) -> Option<Arc<Mutex<FaceMetrics>>> {
		split_families(families).into_iter().find_map(|name| {
			self.family(&name)
				.iter()
				.find(|face| available(face, "Font metrics").is_some())
				.cloned()
		})
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
		let (mut faces, mut used) = {
			let Some(mut metrics) = available(base, "Font metrics") else {
				return vec![None; chars.len()];
			};
			let faces: Vec<_> = chars
				.iter()
				.map(|ch| metrics.covers(*ch).then(|| base.clone()))
				.collect();
			(faces, vec![metrics.face.key()])
		};
		while let Some(index) = faces.iter().position(Option::is_none) {
			let Some(fallback) = self.cover(families, chars[index]) else {
				break;
			};
			let Some(key) = available(&fallback, "Font metrics")
				.map(|metrics| metrics.face.key())
			else {
				break;
			};
			if used.contains(&key) {
				break;
			}
			let covers_all = {
				let Some(mut metrics) = available(&fallback, "Font metrics")
				else {
					break;
				};
				chars.iter().all(|ch| metrics.covers(*ch))
			};
			if covers_all {
				faces.fill(Some(fallback));
				break;
			}
			{
				let Some(mut metrics) = available(&fallback, "Font metrics")
				else {
					break;
				};
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
		if let Some(key) = self.scanned.get(&ch).copied().flatten()
			&& let Some(face) = self.faces.get(&key)
			&& available(face, "Font metrics").is_some()
		{
			return Some(face.clone());
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
			found.as_ref().and_then(|face| {
				available(face, "Font metrics")
					.map(|metrics| metrics.face.key())
			}),
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
			.find(|face| {
				available(face, "Font metrics")
					.is_some_and(|mut metrics| metrics.covers(ch))
			})
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
	use crate::fonts::{FontConfig, poison};
	use std::path::Path;

	/// A diagram measured outside the shaper resolves through the shaper's
	/// own collection, so the boxes it computes match the text that is drawn.
	#[cfg(feature = "font-directories")]
	#[test]
	fn diagram_fonts_measure_and_cover_from_the_shapers_collection() {
		let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts");
		let config = FontConfig {
			ignore_system_fonts: true,
			directories: vec![dir],
			revision: 0,
			..Default::default()
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
	#[cfg(feature = "font-directories")]
	#[test]
	fn diagram_faces_are_limited_to_requested_families() {
		let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts");
		let config = FontConfig {
			ignore_system_fonts: true,
			directories: vec![dir],
			revision: 0,
			..Default::default()
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
	#[cfg(feature = "font-directories")]
	#[test]
	fn a_whole_run_fallback_replaces_the_base_face() {
		let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts");
		let config = FontConfig {
			ignore_system_fonts: true,
			directories: vec![dir],
			revision: 0,
			..Default::default()
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
	#[test]
	fn poisoned_diagram_state_declines_without_poisoning_the_collection() {
		let config = FontConfig {
			ignore_system_fonts: true,
			..Default::default()
		};
		let fonts = DiagramFonts::new(&config, &[]);
		poison(&fonts.inner);
		assert!(fonts.measure("serif", "text", 16.0).is_none());
		assert!(fonts.cover("serif", 'a').is_none());
		assert!(fonts.resolve_family("serif").is_none());
		assert!(fonts.generics().is_empty());
		assert!(fonts.faces().is_empty());
		assert!(fonts.faces_for(&["serif".into()]).is_empty());
		assert!(!DiagramFonts::new(&config, &[]).inner.is_poisoned());
	}
	#[cfg(feature = "font-directories")]
	#[test]
	fn poisoned_face_uses_a_healthy_fallback() {
		let config = FontConfig {
			ignore_system_fonts: true,
			directories: vec![
				Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts"),
			],
			..Default::default()
		};
		let fonts = DiagramFonts::new(&config, &[]);
		let face = fonts.inner.lock().unwrap().base_face("Noto Sans").unwrap();
		poison(&face);
		let fallback = fonts.cover("Noto Sans, Noto Serif", 'A').unwrap();
		assert_ne!(
			fallback.key(),
			face.lock().err().unwrap().into_inner().face.key()
		);
		assert!(
			fonts.measure("Noto Sans, Noto Serif", "A", 16.0).unwrap() > 0.0
		);
		assert!(!fonts.inner.is_poisoned());
	}
}
