use super::*;
fn shaper() -> TextShaper {
	let mut s = TextShaper::new();
	s.font_context().collection = parley::fontique::Collection::new(
		parley::fontique::CollectionOptions {
			system_fonts: false,
			..Default::default()
		},
	);
	for (family, file) in [
		("Primary", "KaTeX_Main-Italic.ttf"),
		("Fallback", "KaTeX_AMS-Regular.ttf"),
	] {
		let data = ratex_katex_fonts::ttf_bytes(file).unwrap().into_owned();
		s.font_context().collection.register_fonts(
			data.into(),
			Some(parley::fontique::FontInfoOverride {
				family_name: Some(family),
				// KaTeX uses separate slanted outlines but labels them Normal.
				style: Some(if family == "Primary" {
					FontStyle::Italic
				} else {
					FontStyle::Normal
				}),
				..Default::default()
			}),
		);
	}
	let mut style = (*Stylesheet::bundled(false)).clone();
	style.merge(&Stylesheet::parse("format_version=2\nversion=1\n[[rule]]\nwhen=['em']\nfont=[{family='Primary',variant='italic'},{family='Fallback'}]").unwrap());
	s.set_stylesheet(Arc::new(style));
	s
}
#[test]
fn candidates_have_independent_real_faces_and_cluster_coverage() {
	let mut s = shaper();
	let appearance = s.stylesheet.inline(
		&s.appearance,
		&crate::document::TextStyle {
			italic: true,
			..Default::default()
		},
	);
	let latin = s.choose_font("a", &appearance).unwrap();
	assert_eq!(latin.family, "Primary");
	assert_eq!(latin.style, FontStyle::Italic);
	let other = (0x20..0x3000)
		.filter_map(char::from_u32)
		.find(|c| {
			let primary = swash::FontRef::from_index(
				latin.font.data.data(),
				latin.font.index as usize,
			)
			.unwrap();
			primary.charmap().map(*c) == 0
				&& s.choose_font(&c.to_string(), &appearance)
					.is_some_and(|f| f.family == "Fallback")
		})
		.expect("the AMS fixture has symbols absent from Main Italic");
	let fallback = s.choose_font(&other.to_string(), &appearance).unwrap();
	assert_eq!(fallback.family, "Fallback");
	assert_eq!(fallback.style, FontStyle::Normal);
	let text = format!("a{other}");
	let clusters = s.shape(
		&text,
		&[Span {
			range: 0..text.len(),
			style: crate::document::TextStyle {
				italic: true,
				..Default::default()
			},
		}],
		18.,
		false,
	);
	assert!(
		clusters[0]
			.glyphs
			.iter()
			.all(|g| g.font.data.id() == latin.font.data.id())
	);
	assert!(
		clusters
			.last()
			.unwrap()
			.glyphs
			.iter()
			.all(|g| g.font.data.id() == fallback.font.data.id())
	);
	// A candidate must cover the entire combining cluster, never just its base.
	if let Some(face) = s.choose_font("a\u{301}", &appearance) {
		let font = swash::FontRef::from_index(
			face.font.data.data(),
			face.font.index as usize,
		)
		.unwrap();
		assert_ne!(font.charmap().map('a'), 0);
		assert_ne!(font.charmap().map('\u{301}'), 0);
	}
}
#[test]
fn unavailable_variant_weight_and_family_are_skipped() {
	let mut s = shaper();
	let appearance = TextAppearance {
		font: vec![
			Font {
				family: "Missing".into(),
				variant: Variant::Normal,
				weight: None,
				synthetic_italic: false,
			},
			Font {
				family: "Fallback".into(),
				variant: Variant::Italic,
				weight: None,
				synthetic_italic: false,
			},
			Font {
				family: "Primary".into(),
				variant: Variant::Italic,
				weight: Some(700),
				synthetic_italic: false,
			},
			Font {
				family: "Primary".into(),
				variant: Variant::Italic,
				weight: Some(400),
				synthetic_italic: false,
			},
		],
		..Default::default()
	};
	let face = s.choose_font("a", &appearance).unwrap();
	assert_eq!(face.family, "Primary");
	assert_eq!(face.weight, 400);
	assert_eq!(face.style, FontStyle::Italic);
}

#[test]
fn font_choices_are_scoped_and_invalidated_with_stylesheet() {
	let mut s = shaper();
	let appearance = s.stylesheet.inline(
		&s.appearance,
		&TextStyle {
			italic: true,
			..Default::default()
		},
	);
	assert_eq!(s.choose_font("a", &appearance).unwrap().family, "Primary");
	let index = s.resolve_fonts(&appearance);
	assert!(s.font_sets[index].choices.contains_key("a"));
	let missing = "\u{10ffff}";
	assert!(s.choose_font(missing, &appearance).is_none());
	assert_eq!(s.font_sets[index].choices.get(missing), Some(&None));
	let mut other = appearance.clone();
	other.font = vec![Font {
		family: "Fallback".into(),
		variant: Variant::Normal,
		weight: None,
		synthetic_italic: false,
	}];
	assert_ne!(index, s.resolve_fonts(&other));
	let mut style = (*s.stylesheet).clone();
	style.merge(
		&Stylesheet::parse(
			"format_version=2\nversion=1\n[[fontdef]]\nid='Primary'\nlookfor=['Missing']",
		)
		.unwrap(),
	);
	s.set_stylesheet(Arc::new(style));
	assert!(s.font_sets.is_empty());
	assert!(s.faces.is_empty());
	assert_ne!(
		s.choose_font("a", &appearance).map(|f| f.family),
		Some("Primary".into())
	);
}

#[test]
fn font_choice_cache_is_bounded() {
	let mut set = FontSet::default();
	for i in 0..5000 {
		assert_eq!(set.choose(&format!("word{i}")), None);
	}
	assert_eq!(set.choices.len(), 4096);
	let mut set = FontSet::default();
	set.choose(&"a".repeat(129));
	assert!(set.choices.is_empty());
}

#[test]
fn cached_choices_preserve_contextual_shaping() {
	let mut s = shaper();
	for text in [
		"office affine",
		"a\u{301} a\u{200d}",
		"中文 English",
		"مرحبا بالعالم",
		"אבג abc",
		"👩‍💻",
	] {
		for set in &mut s.font_sets {
			set.choices.clear();
		}
		let spans = [Span {
			range: 0..text.len(),
			style: TextStyle {
				italic: true,
				..Default::default()
			},
		}];
		let cold = s.shape(text, &spans, 18., false);
		let warm = s.shape(text, &spans, 18., false);
		assert_eq!(cold.len(), warm.len());
		for (a, b) in cold.iter().zip(&warm) {
			assert_eq!(
				(
					&a.range,
					a.rtl,
					a.width,
					a.ascent,
					a.descent,
					a.continuation
				),
				(
					&b.range,
					b.rtl,
					b.width,
					b.ascent,
					b.descent,
					b.continuation
				)
			);
			assert_eq!(a.glyphs.len(), b.glyphs.len());
			for (a, b) in a.glyphs.iter().zip(&b.glyphs) {
				assert_eq!(
					(
						a.font.data.id(),
						a.font.index,
						a.id,
						a.size,
						a.x,
						a.y,
						&a.coords
					),
					(
						b.font.data.id(),
						b.font.index,
						b.id,
						b.size,
						b.x,
						b.y,
						&b.coords
					)
				);
			}
		}
	}
}

#[test]
fn explicit_regular_fallback_survives_bold_and_missing_primary() {
	let mut s = shaper();
	let mut appearance = TextAppearance {
		weight: 700,
		font: vec![
			Font {
				family: "Missing".into(),
				variant: Variant::Normal,
				weight: None,
				synthetic_italic: false,
			},
			Font {
				family: "Fallback".into(),
				variant: Variant::Normal,
				weight: None,
				synthetic_italic: false,
			},
		],
		..Default::default()
	};
	assert!(s.choose_font("A", &appearance).is_none());
	let unavailable = s.resolve_fonts(&appearance);
	assert!(s.fallback_warning(unavailable, "\u{fffc}").is_none());
	assert!(s.fallback_warning(unavailable, "\n").is_none());
	let warning = s.fallback_warning(unavailable, "A\u{1b}").unwrap();
	assert!(warning.contains("U+0041 U+001B"));
	assert!(!warning.contains('\u{1b}'));
	assert!(warning.contains("weight 700"));
	assert!(warning.contains("Available exact faces: []"));
	assert!(s.fallback_warning(unavailable, "B").is_none());
	// Reflows reset choices, but must not repeat terminal warnings.
	s.set_stylesheet(s.stylesheet.clone());
	let unavailable = s.resolve_fonts(&appearance);
	assert!(s.fallback_warning(unavailable, "A").is_none());
	appearance.font[1].weight = Some(400);
	let face = s.choose_font("A", &appearance).unwrap();
	assert_eq!(face.family, "Fallback");
	assert_eq!(face.weight, 400);
	assert_eq!(s.warned_fallbacks.len(), 1);
}

#[test]
fn fallback_warnings_are_bounded_and_allow_new_candidate_sets() {
	let mut s = shaper();
	for i in 0..80 {
		let appearance = TextAppearance {
			font: vec![Font {
				family: format!("Missing{i}"),
				variant: Variant::Normal,
				weight: None,
				synthetic_italic: false,
			}],
			..Default::default()
		};
		let fonts = s.resolve_fonts(&appearance);
		let warning = s.fallback_warning(fonts, "\u{10ffff}");
		assert_eq!(warning.is_some(), i < 64);
		if i == 63 {
			assert!(
				warning
					.unwrap()
					.contains("Further font fallback warnings suppressed")
			);
		}
	}
	assert_eq!(s.warned_fallbacks.len(), 64);
}

#[test]
fn shaping_warns_only_when_configured_candidates_are_exhausted() {
	let mut s = shaper();
	s.appearance.font = vec![Font {
		family: "Fallback".into(),
		variant: Variant::Normal,
		weight: Some(400),
		synthetic_italic: false,
	}];
	s.shape("A", &[], 18., false);
	assert!(s.warned_fallbacks.is_empty());
	s.shape("\u{10ffff}", &[], 18., false);
	assert_eq!(s.warned_fallbacks.len(), 1);
	s.shape("\u{10ffff}", &[], 18., false);
	assert_eq!(s.warned_fallbacks.len(), 1);
}

#[test]
fn a_selected_cjk_variant_supplies_the_configured_face() {
	// The bundled stylesheet defines `serif[cjk]` once per convention, and
	// `resolve_fontdefs` keeps only the selected one. A reader who picks SC, TC
	// or JP therefore gets their configured CJK family. The families are
	// registered fixtures, so the test does not depend on which fonts a machine
	// happens to have installed.
	let variants = [
		(
			crate::style::CjkType::Sc,
			"Noto Serif CJK SC",
			"KaTeX_Main-Regular.ttf",
		),
		(
			crate::style::CjkType::Tc,
			"Noto Serif CJK TC",
			"KaTeX_SansSerif-Regular.ttf",
		),
		(
			crate::style::CjkType::Jp,
			"Noto Serif CJK JP",
			"KaTeX_Typewriter-Regular.ttf",
		),
	];
	let mut s = TextShaper::new();
	s.font_context().collection = parley::fontique::Collection::new(
		parley::fontique::CollectionOptions {
			system_fonts: false,
			..Default::default()
		},
	);
	for (_, family, file) in variants {
		let data = ratex_katex_fonts::ttf_bytes(file).unwrap().into_owned();
		s.font_context().collection.register_fonts(
			data.into(),
			Some(parley::fontique::FontInfoOverride {
				family_name: Some(family),
				..Default::default()
			}),
		);
	}
	for (cjk, family, _) in variants {
		let mut sheet = (*Stylesheet::bundled(false)).clone();
		sheet.set_cjk_type(cjk);
		s.set_stylesheet(Arc::new(sheet));
		let appearance = s.appearance.clone();
		let index = s.resolve_fonts(&appearance);
		assert_eq!(
			s.font_sets[index]
				.faces
				.first()
				.map(|face| face.family.as_str()),
			Some(family),
			"{cjk:?} did not resolve its configured face"
		);
	}
	// Turning the variant off is deliberate rather than a gap: no configured
	// face covers CJK, so the shaper reports that and the system fallback in
	// layout takes over. The desktop reader selects a variant by default.
	let mut sheet = (*Stylesheet::bundled(false)).clone();
	sheet.set_cjk_type(crate::style::CjkType::None);
	s.set_stylesheet(Arc::new(sheet));
	let appearance = s.appearance.clone();
	let index = s.resolve_fonts(&appearance);
	assert!(s.font_sets[index].faces.is_empty());
}

#[test]
fn a_synthetic_italic_candidate_keeps_an_upright_face() {
	let mut s = shaper();
	let appearance = |synthetic_italic| TextAppearance {
		font: vec![Font {
			family: "Fallback".into(),
			variant: Variant::Italic,
			weight: None,
			synthetic_italic,
		}],
		..Default::default()
	};
	// The fixture family ships one upright face. Without the opt-in the
	// candidate is skipped, as before; with it the shaper keeps the face and
	// records that the renderer must shear its outline.
	assert!(s.choose_font("A", &appearance(false)).is_none());
	let face = s.choose_font("A", &appearance(true)).unwrap();
	assert_eq!(face.family, "Fallback");
	assert_eq!(face.style, FontStyle::Normal);
	assert!(face.synthetic_italic);
	s.appearance.font = appearance(true).font;
	let clusters = s.shape("A", &[], 18., false);
	let glyphs: Vec<_> = clusters.iter().flat_map(|c| &c.glyphs).collect();
	assert!(!glyphs.is_empty());
	assert!(glyphs.iter().all(|g| g.synthetic_italic));
}

#[test]
fn bundled_emphasis_shears_cjk_but_keeps_a_real_latin_italic() {
	let mut s = TextShaper::new();
	let appearance = s.stylesheet.inline(
		&s.appearance,
		&TextStyle {
			italic: true,
			..Default::default()
		},
	);
	// A CJK family has no italic, so the bundled `em` rule opts into shear.
	let han = s.choose_font("中", &appearance).unwrap();
	assert_eq!(han.family, "Noto Serif CJK SC");
	assert!(han.synthetic_italic);
	// The Latin family does have one, so no shear is invented.
	let latin = s.choose_font("a", &appearance).unwrap();
	assert!(!latin.synthetic_italic);
	assert_eq!(latin.style, FontStyle::Italic);
}

#[test]
fn emoji_presentation_follows_the_unicode_defaults() {
	for emoji in [
		"\u{2705}",  // WHITE HEAVY CHECK MARK, `Emoji_Presentation=Yes`
		"\u{274c}",  // CROSS MARK
		"\u{2b50}",  // WHITE MEDIUM STAR
		"\u{1f600}", // GRINNING FACE
		"\u{1f469}\u{200d}\u{1f4bb}", // WOMAN TECHNOLOGIST
		"\u{1f1e8}\u{1f1f3}", // REGIONAL INDICATOR pair
		"1\u{fe0f}\u{20e3}", // KEYCAP: the selector decides
		"\u{26a0}\u{fe0f}", // WARNING SIGN with VS16
		"\u{a9}\u{fe0f}", // COPYRIGHT SIGN with VS16
	] {
		assert!(prefers_emoji(emoji), "{emoji:?}");
	}
	for text in [
		"\u{26a0}", // WARNING SIGN defaults to text presentation
		"\u{2764}", // HEAVY BLACK HEART
		"\u{2714}", // HEAVY CHECK MARK
		"\u{a9}",   // COPYRIGHT SIGN
		"\u{2122}", // TRADE MARK SIGN
		"\u{27a1}", // BLACK RIGHTWARDS ARROW
		"1",        // Keycap bases and components are not Emoji by default
		"1\u{20e3}",
		"#",
		"a",
		"中",
		"\u{2705}\u{fe0e}", // A VS15 overrides the default
	] {
		assert!(!prefers_emoji(text), "{text:?}");
	}
}

#[test]
fn the_marked_emoji_face_wins_only_for_emoji_clusters() {
	let mut s = TextShaper::new();
	let mut sheet = (*Stylesheet::bundled(false)).clone();
	// The marked face comes first, so only the Emoji group can explain a
	// text cluster that still takes a later candidate.
	sheet.merge(
		&Stylesheet::parse(concat!(
			"format_version=2\nversion=1\n",
			"[[fontdef]]\nid='first'\nemoji=true\nlookfor=['Noto Color Emoji']\n",
			"[[rule]]\nwhen=['body']\n",
			"font=[{family='first',weight=400},{family='serif'},{family='serif[cjk]'}]",
		))
		.unwrap(),
	);
	s.set_stylesheet(Arc::new(sheet));
	let appearance = s.appearance.clone();
	let family = |s: &mut TextShaper, text: &str| {
		s.choose_font(text, &appearance).unwrap().family
	};
	assert_eq!(family(&mut s, "1"), "Noto Serif");
	assert_eq!(family(&mut s, "\u{26a0}"), "Noto Serif CJK SC");
	assert_eq!(family(&mut s, "\u{2705}"), "Noto Color Emoji");
	assert_eq!(family(&mut s, "\u{26a0}\u{fe0f}"), "Noto Color Emoji");
}

#[test]
fn the_bundled_emoji_face_beats_a_text_candidate_covering_the_cluster() {
	let mut s = TextShaper::new();
	let appearance = s.appearance.clone();
	let family = |s: &mut TextShaper, text: &str| {
		s.choose_font(text, &appearance).unwrap().family
	};
	// The bundled body list names the Emoji face last, and the pinned Noto
	// Serif CJK face covers U+26A0 by itself, so only the Emoji group can
	// explain the styled variant.
	assert_eq!(family(&mut s, "\u{26a0}"), "Noto Serif CJK SC");
	assert_eq!(family(&mut s, "\u{26a0}\u{fe0e}"), "Noto Serif CJK SC");
	assert_eq!(family(&mut s, "\u{26a0}\u{fe0f}"), "Noto Color Emoji");
}

#[test]
fn unavailable_cjk_medium_keeps_an_explicit_regular_fallback() {
	let mut shaper = TextShaper::new();
	let mut sheet = (*Stylesheet::bundled(false)).clone();
	sheet.set_cjk_type(crate::style::CjkType::Sc);
	sheet.merge(&Stylesheet::parse("format_version=2\nversion=1\n[[rule]]\nwhen=['ui']\nfont=[{family='sans-serif'},{family='sans-serif[cjk]',weight=500},{family='sans-serif[cjk]'},{family='emoji',weight=400}]").unwrap());
	shaper.set_stylesheet(Arc::new(sheet));
	let appearance = shaper
		.stylesheet
		.text(&TextAppearance::default(), Condition::Ui);
	// Pinned CJK faces have 400 and 700, so 500 must not cause system fallback.
	let cjk = shaper.choose_font("中", &appearance).unwrap();
	assert_eq!(cjk.weight, 400);
	assert!(cjk.family.contains("CJK"));
	assert_eq!(shaper.choose_font("a", &appearance).unwrap().weight, 400);
}
