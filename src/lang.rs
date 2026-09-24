//! The language the interface is drawn in.
//!
//! The text itself lives in `assets/locales/*.toml`, which the macro below
//! turns into methods on [`Lang`] at compile time: each method hands back a
//! `&'static str`, so drawing a label costs no lookup and no allocation.
use serde::{Deserialize, Serialize};

/// The language the interface is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Lang {
	En,
	ZhHans,
	ZhHant,
	Ja,
}
impl Default for Lang {
	/// The system's language, chosen the same way the CJK convention is. Only
	/// an explicit choice is stored, so a locale change follows through.
	///
	/// The locale is asked once and then read as the language it names: drawing
	/// asks repeatedly, and resolving it again would clone and normalize a tag
	/// for an answer that cannot change while the process runs.
	fn default() -> Self {
		static RESOLVED: std::sync::OnceLock<Lang> = std::sync::OnceLock::new();
		*RESOLVED.get_or_init(|| {
			system_locale()
				.map_or(Self::En, |locale| Self::from_locale(&locale))
		})
	}
}
impl Lang {
	/// The language a locale tag names.
	///
	/// A tag this reader has no translation for falls back to English. A Chinese
	/// tag takes the script its own subtag names, and one this list does not name
	/// takes the Simplified text: a reader of one Chinese script is better served
	/// by the other than by none, and most `zh` tags are Simplified.
	pub fn from_locale(locale: &str) -> Self {
		let locale = normalize(locale);
		if locale == "ja" || locale.starts_with("ja-") {
			Self::Ja
		} else if locale == "zh" || locale.starts_with("zh-") {
			// The Chinese tags that name Traditional script; every other `zh`
			// spelling is Simplified, including a tag this list does not name.
			const TRADITIONAL: [&str; 4] =
				["zh-hant", "zh-tw", "zh-hk", "zh-mo"];
			if TRADITIONAL.iter().any(|tag| locale.starts_with(tag)) {
				Self::ZhHant
			} else {
				Self::ZhHans
			}
		} else {
			Self::En
		}
	}
}

/// The system's locale tag, lowercased and joined with `-`.
///
/// The platform is asked once: the answer reaches every frame that draws a
/// label whose language is still whatever the system says. Tests pin this to
/// `en-US`, so neither the language nor the CJK convention a test shapes with
/// depends on the machine that runs it.
pub(crate) fn system_locale() -> Option<String> {
	static LOCALE: std::sync::OnceLock<Option<String>> =
		std::sync::OnceLock::new();
	LOCALE.get_or_init(read_locale).clone()
}

fn read_locale() -> Option<String> {
	#[cfg(test)]
	let locale = Some("en-US".to_owned());
	#[cfg(not(test))]
	let locale = sys_locale::get_locale();
	locale.map(|locale| normalize(&locale))
}

fn normalize(locale: &str) -> String {
	locale.to_ascii_lowercase().replace('_', "-")
}

markview_i18n::locales!(
	"assets/locales",
	En = "en.toml",
	ZhHans = "zh-Hans.toml",
	ZhHant = "zh-Hant.toml",
	Ja = "ja.toml"
);

#[cfg(test)]
#[path = "lang/tests.rs"]
mod tests;
