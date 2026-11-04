use super::*;
use proptest::prelude::*;

proptest! {
	#[test]
	fn positions_follow_encoding_prefixes(chars in prop::collection::vec(any::<char>(), 0..64)) {
		let text: String = chars.iter().collect();
		let mut prefixes = vec![(0, 0)];
		for ch in chars {
			let (b, u) = *prefixes.last().unwrap();
			prefixes.push((b + ch.len_utf8(), u + ch.len_utf16()));
		}
		for byte in 0..=text.len() + 3 {
			let &(b, u) = prefixes.iter().rev().find(|&&(b, _)| b <= byte).unwrap();
			prop_assert_eq!(
				byte_to_utf16(&text, byte),
				u,
				"byte={}, boundary={}",
				byte,
				b
			);
		}
		for unit in 0..=prefixes.last().unwrap().1 + 3 {
			let &(b, _) = prefixes.iter().rev().find(|&&(_, u)| u <= unit).unwrap();
			prop_assert_eq!(utf16_to_byte(&text, unit), b, "unit={}", unit);
		}
	}
}

#[test]
fn encoding_categories() {
	for text in ["", "ascii", "中文", "😀", "e\u{301}", "\r\n", "a中😀\r\n"]
	{
		let mut units = 0;
		for (byte, ch) in text.char_indices() {
			assert_eq!(byte_to_utf16(text, byte), units);
			assert_eq!(utf16_to_byte(text, units), byte);
			units += ch.len_utf16();
		}
	}
}
