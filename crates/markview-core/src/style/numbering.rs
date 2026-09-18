//! Numbering patterns for ordered lists, in Typst's notation.
//!
//! A pattern is a sequence of literal prefixes, counting symbols, and one
//! suffix. A counting symbol is one character that names a numeral system,
//! such as `1`, `a`, `i`, `I`, `①`, or `一`; everything else is printed as it
//! stands. The number of counting symbols is the number of nesting levels the
//! pattern addresses, and the last one repeats for deeper lists.
use codex::numeral_systems::NamedNumeralSystem;
use serde::{Deserialize, Deserializer};
use std::{
	fmt::{self, Write},
	str::FromStr,
};

/// How many bytes of a numeral a marker may carry before it falls back to
/// decimal. Some systems grow with the number: `*` repeats a symbol every six
/// items, and an additive system such as Hebrew repeats its largest numeral for
/// however large the number is. A list that merely starts at `999999999.`
/// would otherwise spell out a label hundreds of megabytes long.
const MAX_NUMERAL: usize = 64;

/// How an ordered list writes its numbers. `1.` numbers items `1.`, `2.`, ...,
/// while `1.a.` numbers the first level `1.`, the second `a.`, and so on.
#[derive(Clone, Debug, PartialEq)]
pub struct NumberingPattern {
	pieces: Vec<(String, NamedNumeralSystem)>,
	suffix: String,
}
impl NumberingPattern {
	/// The number for an item at nesting level `depth`, counting from zero.
	///
	/// The depth picks the counting symbol; a depth past the last piece reuses
	/// that piece. A numeral system that cannot represent the number—an
	/// alphabetic zero, a circled number past fifty—falls back to decimal, as
	/// does one whose numeral would outgrow a marker.
	pub fn number(&self, depth: usize, number: u64) -> String {
		let (prefix, _) = &self.pieces[0];
		let (_, system) = self
			.pieces
			.get(depth)
			.or_else(|| self.pieces.last())
			.expect(
				"a numbering pattern declares at least one counting symbol",
			);
		let mut out = prefix.clone();
		out.push_str(&numeral(*system, number));
		out.push_str(&self.suffix);
		out
	}
}

/// One numeral, or its decimal form when the system cannot write the number or
/// would spell it out beyond what a marker can hold.
fn numeral(system: NamedNumeralSystem, number: u64) -> String {
	let Ok(represented) = system.system().represent(number) else {
		return number.to_string();
	};
	let mut out = Bounded {
		text: String::new(),
		limit: MAX_NUMERAL,
	};
	if write!(out, "{represented}").is_err() {
		return number.to_string();
	}
	out.text
}

/// A writer that refuses the byte past `limit`, so a numeral that grows with
/// the number stops before it allocates.
struct Bounded {
	text: String,
	limit: usize,
}
impl fmt::Write for Bounded {
	fn write_str(&mut self, s: &str) -> fmt::Result {
		if self.text.len() + s.len() > self.limit {
			return Err(fmt::Error);
		}
		self.text.push_str(s);
		Ok(())
	}
}
impl FromStr for NumberingPattern {
	type Err = &'static str;
	fn from_str(pattern: &str) -> Result<Self, Self::Err> {
		let mut pieces = Vec::new();
		let mut handled = 0;
		for (i, c) in pattern.char_indices() {
			let mut buffer = [0; 4];
			let Some(system) =
				NamedNumeralSystem::from_shorthand(c.encode_utf8(&mut buffer))
			else {
				continue;
			};
			pieces.push((pattern[handled..i].to_string(), system));
			handled = i + c.len_utf8();
		}
		if pieces.is_empty() {
			return Err("invalid numbering pattern");
		}
		Ok(Self {
			pieces,
			suffix: pattern[handled..].to_string(),
		})
	}
}
impl<'de> Deserialize<'de> for NumberingPattern {
	fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
		String::deserialize(d)?.parse().map_err(|_| {
			serde::de::Error::custom(
				"expected a numbering pattern such as \"1.\" or \"a)\"",
			)
		})
	}
}

/// The numbering an ordered list uses when a theme says nothing.
pub(super) const DEFAULT_NUMBERING: &str = "1.";
