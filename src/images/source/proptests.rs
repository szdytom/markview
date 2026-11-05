use super::*;
use crate::security::{Security, Trust};
use proptest::prelude::*;

proptest! {
	#[test]
	fn local_resource_bases_resolve_percent_encoded_paths_to_the_selected_root(
		name in "[a-z]{1,12}", encoded in any::<bool>(), vault in any::<bool>(), workspace in any::<bool>(),
	) {
		let directory = tempfile::tempdir().unwrap();
		let root = directory.path().canonicalize().unwrap();
		let document = root.join("documents/article.md");
		let workspace_path = root.join("vault");
		let relative = format!("figures/{name} 空.png");
		let base = if vault && workspace { workspace_path.clone() } else { document.parent().unwrap().to_owned() };
		let expected = base.join(&relative);
		fs::create_dir_all(expected.parent().unwrap()).unwrap();
		fs::write(&expected, b"image").unwrap();
		let address = if encoded { percent_encoding::utf8_percent_encode(&relative, percent_encoding::NON_ALPHANUMERIC).to_string() } else { relative };
		let reference = ResourceRef { address, base: if vault { ResourceBase::Vault } else { ResourceBase::Document } };
		let source = resolve_resource(&reference, &document, workspace.then_some(workspace_path.as_path()), &Security::local(Trust::Trusted)).unwrap();
		prop_assert_eq!(source, Source::File(expected.canonicalize().unwrap()));
	}

	#[test]
	fn web_sources_resolve_known_relative_address_forms_without_local_file_access(
		folder in "[a-z]{1,12}", name in "[a-z]{1,12}", query in "[a-z0-9]{0,12}",
	) {
		let security = Security::web(format!("https://a.test/{folder}/article.html"));
		for (relative, expected) in [
			(format!("{name}.png?q={query}"), format!("https://a.test/{folder}/{name}.png?q={query}")),
			(format!("../{name}.png"), format!("https://a.test/{name}.png")),
			(format!("/{name}.png"), format!("https://a.test/{name}.png")),
			(format!("//b.test/{name}.png"), format!("https://b.test/{name}.png")),
		] {
			prop_assert_eq!(resolve_at(&relative, Path::new("unused.md"), &security).unwrap(), Source::Http(expected));
		}
		for address in [format!("file:///{name}.png"), format!("ftp://a.test/{name}.png")] { prop_assert!(resolve_at(&address, Path::new("unused.md"), &security).is_err()); }
	}

	#[test]
	fn data_sources_keep_exact_bytes_for_base64_and_percent_encoded_payloads(
		bytes in prop::collection::vec(any::<u8>(), 0..257), upper in any::<bool>(),
	) {
		let theme = super::super::diagram::resolve(&markview_layout::style::Stylesheet::default(), None);
		let mime = if upper { "DATA:IMAGE/PNG" } else { "data:image/png" };
		let payload = base64::engine::general_purpose::STANDARD.encode(&bytes);
		let encoded = percent_encoding::percent_encode(&bytes, percent_encoding::NON_ALPHANUMERIC).to_string();
		for uri in [format!("{mime};base64,{payload}"), format!("{mime},{encoded}")] {
			let source = source(&uri, Path::new("unused.md")).unwrap();
			prop_assert_eq!(fetch(&source, true, None, &theme).unwrap(), bytes.clone());
		}
		let invalid = Source::Data(format!("data:text/plain;base64,{payload}"));
		prop_assert!(fetch(&invalid, true, None, &theme).is_err());
	}
}
