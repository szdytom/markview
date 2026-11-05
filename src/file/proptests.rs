use super::*;
use proptest::prelude::*;

proptest! {
	#[test]
	fn local_files_preserve_utf8_strip_one_bom_and_classify_only_the_final_extension(
		characters in prop::collection::vec(any::<char>(), 0..128), bom in any::<bool>(), upper in any::<bool>(),
		kind in prop::sample::select(vec![("md", 0), ("markdown", 0), ("mdown", 0), ("png", 1), ("jpg", 1), ("jpeg", 1), ("gif", 1), ("webp", 1), ("bmp", 1), ("ico", 1), ("svg", 1), ("pdf", 2), ("txt", 2), ("exe", 3), ("sh", 3), ("unknown", 3)]),
	) {
		let directory = tempfile::tempdir().unwrap();
		let extension = if upper { kind.0.to_uppercase() } else { kind.0.to_owned() };
		let path = directory.path().join(format!("note.exe.{extension}"));
		let body: String = characters.into_iter().collect();
		let source = format!("{}{body}", if bom { "\u{feff}" } else { "" });
		fs::write(&path, &source).unwrap();
		prop_assert_eq!(read_document(&path).unwrap(), source.strip_prefix('\u{feff}').unwrap_or(&source));
		prop_assert_eq!(is_image(&path), kind.1 == 1);
		let expected = std::fs::canonicalize(&path).unwrap();
		let resolved = crate::link::resolve(&format!("note.exe.{extension}#section?query"), Some(directory.path())).unwrap();
		prop_assert_eq!(resolved, match kind.1 { 0 => crate::link::Target::Markdown(expected), 1 | 2 => crate::link::Target::OsDirect(expected), _ => crate::link::Target::Confirm(expected) });
		let mut invalid = vec![0xff]; invalid.extend(source.as_bytes());
		fs::write(&path, invalid).unwrap();
		prop_assert!(read_document(&path).is_err());
	}
}
