use super::*;
use proptest::prelude::*;
use std::net::{Ipv4Addr, Ipv6Addr};

const IPV4_ROUTES: &[(u32, u32, AddressClass)] = &[
	(0x00000000, 8, AddressClass::Invalid),
	(0xe0000000, 4, AddressClass::Invalid),
	(0xf0000000, 4, AddressClass::Invalid),
	(0xc0000200, 24, AddressClass::Invalid),
	(0xc6336400, 24, AddressClass::Invalid),
	(0xcb007100, 24, AddressClass::Invalid),
	(0x7f000000, 8, AddressClass::Loopback),
	(0xa9fe0000, 16, AddressClass::LinkLocal),
	(0x0a000000, 8, AddressClass::Private),
	(0xac100000, 12, AddressClass::Private),
	(0xc0a80000, 16, AddressClass::Private),
	(0x64400000, 10, AddressClass::Private),
];

fn ipv4_class(value: u32) -> AddressClass {
	IPV4_ROUTES
		.iter()
		.find(|&&(network, prefix, _)| {
			value >> (32 - prefix) == network >> (32 - prefix)
		})
		.map_or(AddressClass::Public, |&(_, _, class)| class)
}

fn ipv6_class(value: u128) -> AddressClass {
	if value >> 32 == 0xffff {
		return ipv4_class(value as u32);
	}
	if value == 0 || value >> 120 == 0xff || value >> 96 == 0x20010db8 {
		AddressClass::Invalid
	} else if value == 1 {
		AddressClass::Loopback
	} else if value >> 118 == 0x3fa {
		AddressClass::LinkLocal
	} else if value >> 121 == 0x7e {
		AddressClass::Private
	} else {
		AddressClass::Public
	}
}

proptest! {
	#[test]
	fn address_classification_matches_numeric_prefix_partitions(v4 in any::<u32>(), v6 in any::<u128>()) {
		let mut values = vec![v4];
		for &(network, prefix, _) in IPV4_ROUTES {
			let end = u64::from(network) + (1u64 << (32 - prefix)) - 1;
			values.extend([network, end as u32]);
			if network > 0 { values.push(network - 1); }
			if end < u64::from(u32::MAX) { values.push(end as u32 + 1); }
		}
		for value in values {
			let ip = Ipv4Addr::from(value);
			prop_assert_eq!(AddressClass::of(ip.into()), ipv4_class(value));
			prop_assert_eq!(AddressClass::of(ip.to_ipv6_mapped().into()), ipv4_class(value));
		}
		for value in [v6, 0, 1, (0xffu128 << 120) | (v6 & ((1u128 << 120) - 1)), (0x20010db8u128 << 96) | (v6 & ((1u128 << 96) - 1)), (0xfe80u128 << 112) | (v6 & ((1u128 << 118) - 1)), (0xfcu128 << 120) | (v6 & ((1u128 << 121) - 1))] {
			prop_assert_eq!(AddressClass::of(Ipv6Addr::from(value).into()), ipv6_class(value));
		}
	}

	#[test]
	fn authority_sequences_keep_grants_scoped_to_origin_class_source_and_content(
		origin in 0u8..4, operations in prop::collection::vec((0u8..7, 0usize..10), 0..40),
	) {
		use AddressClass::*;
		let mut security = match origin { 0 => Security::local(Trust::Trusted), 1 => Security::local(Trust::Untrusted), 2 => Security::web("https://article.test".to_owned()), _ => Security::default() };
		let trusted = origin == 0;
		let original_origin = security.origin.clone();
		let contents = ["alpha", "中文", ""];
		let urls: Vec<_> = ["http://a.test:8080/image", "http://a.test:8080/other", "https://a.test:8080/image"].into_iter().map(|url| url::Url::parse(url).unwrap()).collect();
		let origins = ["http://a.test:8080", "https://a.test:8080"];
		let destinations: Vec<(IpAddr, AddressClass)> = [("1.1.1.1", Public), ("10.0.0.1", Private), ("127.0.0.1", Loopback), ("169.254.1.1", LinkLocal), ("0.0.0.0", Invalid)].into_iter().map(|(ip, class)| (ip.parse().unwrap(), class)).collect();
		let files: Vec<_> = (0..3).map(|i| PathBuf::from(format!("file-{i}"))).collect();
		let sources = ["first", "second", "third"];
		let mut file_grants = [false; 3];
		let mut network_grants = [[false; 5]; 2];
		let mut selected = [false; 3];
		let mut current = 0;
		security.bind(contents[current]);
		let directed = [(0, 0), (1, 1), (1, 4), (5, 0), (4, 1), (2, 0), (3, 0)];
		for (operation, index) in directed.into_iter().chain(operations) {
			match operation {
				0 => {
					security.bind(contents[current]);
					security.grant(Resource::File(files[index % 3].clone()));
					file_grants[index % 3] = true;
				}
				1 => {
					security.bind(contents[current]);
					security.grant(Resource::Network { origin: origins[index / 5].to_owned(), class: destinations[index % 5].1 });
					network_grants[index / 5][index % 5] = true;
				}
				2 => {
					security.revoke();
					file_grants = [false; 3]; network_grants = [[false; 5]; 2]; selected = [false; 3];
				}
				3 => security.bind(contents[current]),
				4 => {
					let next = index % 3;
					security = security.for_content(contents[next]);
					if next != current { file_grants = [false; 3]; network_grants = [[false; 5]; 2]; selected = [false; 3]; }
					current = next;
					security.bind(contents[current]);
				}
				5 => {
					security.bind(contents[current]);
					security.grant(Resource::SelectedImage { source: sources[index % 3].to_owned(), path: PathBuf::from(format!("selected-{}", index % 3)) });
					selected[index % 3] = true;
				}
				_ => {
					let copy = security.for_content(contents[current]);
					prop_assert_eq!(copy.cache_key(), security.cache_key());
				}
			}
			prop_assert_eq!(&security.origin, &original_origin);
			prop_assert_eq!(security.origin.trust() == Trust::Trusted, trusted);
			for (i, file) in files.iter().enumerate() { prop_assert_eq!(security.check_file(file).is_ok(), trusted || file_grants[i]); }
			for (ui, url) in urls.iter().enumerate() {
				for (ci, &(ip, class)) in destinations.iter().enumerate() {
					let expected = class != Invalid && (class == Public || (trusted && matches!(class, Private | Loopback)) || network_grants[usize::from(ui == 2)][ci]);
					let result = security.check_address(url, ip);
					prop_assert_eq!(result.is_ok(), expected);
					if !expected && class != Invalid { prop_assert!(result.unwrap_err().downcast_ref::<PermissionRequired>().is_some()); }
				}
			}
			for (i, source) in sources.iter().enumerate() {
				let expected = selected[i].then(|| PathBuf::from(format!("selected-{i}")));
				prop_assert_eq!(security.selected_image(source), expected.as_deref());
			}
		}
	}
}
