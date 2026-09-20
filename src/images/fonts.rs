//! The reader's own faces for diagrams.
//!
//! A diagram is laid out by the renderer's own measurement and drawn by the
//! SVG rasterizer, so both stages need the same faces or the labels would not
//! fit the boxes around them. [`DiagramFonts`] resolves faces through the
//! shaper's collection, and hands them to the rasterizer's font database.
use markview_core::fonts::{DiagramFace, DiagramFonts as Policy, FontConfig};
use std::{
	collections::HashMap,
	sync::{Arc, Mutex, OnceLock},
};

/// How many font configurations stay resolved at once.
const CACHE_CAP: usize = 4;

/// The faces a diagram is measured and drawn with.
pub(super) struct DiagramFonts {
	/// Identity of the configuration and the Han list behind these faces.
	key: u64,
	policy: Policy,
	db: Arc<resvg::usvg::fontdb::Database>,
	/// Where the rasterizer finds a face the policy handed out.
	ids: HashMap<(u64, u32), resvg::usvg::fontdb::ID>,
}

impl DiagramFonts {
	/// The faces `config` names, with `han` as the fallback families for Han
	/// text. A configuration and its Han list resolve once.
	pub(super) fn get(config: &FontConfig, han: &[String]) -> Arc<Self> {
		type Cache = Mutex<Vec<((FontConfig, Vec<String>), Arc<DiagramFonts>)>>;
		static CACHE: OnceLock<Cache> = OnceLock::new();
		let cache = CACHE.get_or_init(|| Mutex::new(Vec::new()));
		let key = (config.clone(), han.to_vec());
		let mut cache = cache.lock().unwrap();
		if let Some((_, fonts)) = cache.iter().find(|(other, _)| *other == key)
		{
			return fonts.clone();
		}
		let fonts = Arc::new(Self::new(&key.0, &key.1));
		cache.push((key, fonts.clone()));
		if cache.len() > CACHE_CAP {
			cache.remove(0);
		}
		fonts
	}

	fn new(config: &FontConfig, han: &[String]) -> Self {
		let policy = Policy::new(config, han);
		let mut db = resvg::usvg::fontdb::Database::new();
		let mut ids = HashMap::new();
		for face in policy.faces() {
			let id = push(&mut db, &face);
			ids.insert(face.key(), id);
		}
		// The policy resolves the generic names a font list may carry, so a
		// rasterizer's `sans-serif` and a measurement's `sans-serif` are the
		// same face.
		for (generic, family) in policy.generics() {
			set_generic(&mut db, generic, family);
		}
		Self {
			key: crate::document::fingerprint(&(config.clone(), han.to_vec())),
			policy,
			db: Arc::new(db),
			ids,
		}
	}

	/// Every face of the reader's collection, which is what the rasterizer's
	/// database was built from. Tests read it back; the database itself is
	/// what production uses.
	#[cfg(test)]
	pub(super) fn faces(&self) -> Vec<DiagramFace> {
		self.policy.faces()
	}

	/// The face that draws one character for this family list.
	#[cfg(test)]
	pub(super) fn cover(
		&self,
		families: &str,
		ch: char,
	) -> Option<DiagramFace> {
		self.policy.cover(families, ch)
	}

	/// The generics the rasterizer's database maps, for a test that checks a
	/// measurement resolves them the same way.
	#[cfg(test)]
	pub(super) fn generics(&self) -> Vec<(&'static str, String)> {
		self.policy.generics()
	}

	/// Identity of these faces, so a scheduler can tell a redraw from a
	/// repeat when the configuration or the Han list changes.
	pub(super) fn key(&self) -> u64 {
		self.key
	}

	/// Gives `options` the faces and the per-character fallback a diagram was
	/// measured with. `families` is the theme's own font list.
	pub(super) fn apply(
		self: &Arc<Self>,
		options: &mut resvg::usvg::Options<'_>,
		families: &str,
	) {
		options.fontdb = self.db.clone();
		let fonts = Arc::clone(self);
		let list = families.to_owned();
		options.font_resolver = resvg::usvg::FontResolver {
			// The default resolves a family against the database above, so a
			// face the shaper knows is the face the rasterizer draws.
			select_font: resvg::usvg::FontResolver::default_font_selector(),
			select_fallback: Box::new(move |ch, exclude, _db| {
				let face = fonts.policy.cover(&list, ch)?;
				let id = *fonts.ids.get(&face.key())?;
				(!exclude.contains(&id)).then_some(id)
			}),
		};
	}
}

impl std::fmt::Debug for DiagramFonts {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("DiagramFonts")
			.field("faces", &self.db.len())
			.field("key", &self.key)
			.finish()
	}
}

impl mermaid_rs_renderer::TextMetrics for DiagramFonts {
	fn measure_text_width(
		&self,
		text: &str,
		font_size: f32,
		font_family: &str,
	) -> Option<f32> {
		self.policy.measure(font_family, text, font_size)
	}
}

/// Teaches the database what a generic name means, so text that names one is
/// resolved instead of skipped before character fallback ever runs.
fn set_generic(
	db: &mut resvg::usvg::fontdb::Database,
	generic: &str,
	family: String,
) {
	match generic {
		"serif" => db.set_serif_family(family),
		"sans-serif" => db.set_sans_serif_family(family),
		"monospace" => db.set_monospace_family(family),
		"cursive" => db.set_cursive_family(family),
		"fantasy" => db.set_fantasy_family(family),
		_ => {}
	}
}

/// Adds one face to a rasterizer's database.
fn push(
	db: &mut resvg::usvg::fontdb::Database,
	face: &DiagramFace,
) -> resvg::usvg::fontdb::ID {
	use resvg::usvg::fontdb;
	db.push_face_info(fontdb::FaceInfo {
		id: fontdb::ID::dummy(),
		source: fontdb::Source::Binary(Arc::new(face.data())),
		index: face.index(),
		families: vec![(
			face.family.clone(),
			fontdb::Language::English_UnitedStates,
		)],
		post_script_name: String::new(),
		style: if face.italic {
			fontdb::Style::Italic
		} else {
			fontdb::Style::Normal
		},
		weight: fontdb::Weight(face.weight.clamp(1, 1000)),
		stretch: fontdb::Stretch::Normal,
		monospaced: false,
	})
}
