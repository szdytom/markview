//! RGBA color values carried by semantic text and appearance.
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color(pub u32);
impl Color {
	pub fn rgba(self) -> [f32; 4] {
		[
			((self.0 >> 24) & 255) as f32 / 255.,
			((self.0 >> 16) & 255) as f32 / 255.,
			((self.0 >> 8) & 255) as f32 / 255.,
			(self.0 & 255) as f32 / 255.,
		]
	}
}
impl<'de> Deserialize<'de> for Color {
	fn deserialize<D: serde::Deserializer<'de>>(
		d: D,
	) -> std::result::Result<Self, D::Error> {
		let s = String::deserialize(d)?;
		let h = s.strip_prefix('#').ok_or_else(|| {
			serde::de::Error::custom("expected #RRGGBB or #RRGGBBAA")
		})?;
		if !matches!(h.len(), 6 | 8)
			|| !h.bytes().all(|c| c.is_ascii_hexdigit())
		{
			return Err(serde::de::Error::custom(
				"expected #RRGGBB or #RRGGBBAA",
			));
		}
		let v = u32::from_str_radix(h, 16).map_err(serde::de::Error::custom)?;
		Ok(Self(if h.len() == 6 { v << 8 | 255 } else { v }))
	}
}

#[cfg(test)]
mod proptests {
	use super::*;
	use proptest::prelude::*;
	use serde::de::value::{Error, StrDeserializer};

	fn parse(text: &str) -> Result<Color, Error> {
		Color::deserialize(StrDeserializer::new(text))
	}

	proptest! {
		#[test]
		fn hex_colors_preserve_channels_and_default_to_opaque(channels in any::<[u8; 4]>()) {
			let [r, g, b, a] = channels;
			for (text, expected) in [
				(format!("#{r:02x}{g:02x}{b:02x}{a:02x}"), channels),
				(format!("#{r:02X}{g:02X}{b:02X}{a:02X}"), channels),
				(format!("#{r:02x}{g:02x}{b:02x}"), [r, g, b, 255]),
			] {
				let color = parse(&text).unwrap();
				prop_assert_eq!(color.0.to_be_bytes(), expected);
				for (normalized, byte) in color.rgba().into_iter().zip(expected) {
					prop_assert!((0.0..=1.0).contains(&normalized));
					// The reconstruction error is less than 1/1000 of one channel step.
					prop_assert!((normalized * 255.0 - f32::from(byte)).abs() < 0.001);
				}
			}
		}

		#[test]
		fn malformed_color_spelling_is_rejected(value in any::<u32>(), index in 0usize..8) {
			let valid = format!("#{value:08x}");
			let mut invalid = valid.clone();
			invalid.replace_range(index + 1..index + 2, "g");
			for text in [invalid, valid[1..].to_owned(), format!("{valid}0"), format!(" {valid}"), valid[..6].to_owned()] {
				prop_assert!(parse(&text).is_err(), "accepted invalid color: {text:?}");
			}
		}
	}
}
