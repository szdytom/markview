//! Document provenance and explicit, session-scoped resource authority.
use serde::{Deserialize, Serialize};
use std::{
	fmt,
	net::IpAddr,
	path::{Path, PathBuf},
};

#[derive(
	Clone,
	Copy,
	Debug,
	Default,
	PartialEq,
	Eq,
	Hash,
	Serialize,
	Deserialize,
	clap::ValueEnum,
)]
pub enum Trust {
	Trusted,
	#[default]
	Untrusted,
}

#[derive(
	Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum Origin {
	Local(Trust),
	#[default]
	Clipboard,
	Web(String),
}
impl Origin {
	pub fn trust(&self) -> Trust {
		match self {
			Self::Local(trust) => *trust,
			Self::Clipboard | Self::Web(_) => Trust::Untrusted,
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AddressClass {
	Public,
	Private,
	Loopback,
	LinkLocal,
	Invalid,
}
impl AddressClass {
	pub fn of(ip: IpAddr) -> Self {
		if let IpAddr::V6(v6) = ip
			&& let Some(v4) = v6.to_ipv4_mapped()
		{
			return Self::of(IpAddr::V4(v4));
		}
		match ip {
			IpAddr::V4(v4) => Self::of_ipv4(v4),
			IpAddr::V6(v6) => Self::of_ipv6(v6),
		}
	}
	fn of_ipv4(v4: std::net::Ipv4Addr) -> Self {
		let o = v4.octets();
		if v4.is_unspecified()
			|| v4.is_broadcast()
			|| v4.is_multicast()
			|| v4.is_documentation()
			|| o[0] == 0
			|| o[0] >= 240
		{
			Self::Invalid
		} else if v4.is_loopback() {
			Self::Loopback
		} else if v4.is_link_local() {
			Self::LinkLocal
		} else if v4.is_private() || (o[0] == 100 && (64..=127).contains(&o[1]))
		{
			Self::Private
		} else {
			Self::Public
		}
	}

	fn of_ipv6(v6: std::net::Ipv6Addr) -> Self {
		if v6.is_unspecified()
			|| v6.is_multicast()
			|| (v6.segments()[0] == 0x2001 && v6.segments()[1] == 0xdb8)
		{
			Self::Invalid
		} else if v6.is_loopback() {
			Self::Loopback
		} else if v6.is_unicast_link_local() {
			Self::LinkLocal
		} else if v6.is_unique_local() {
			Self::Private
		} else {
			Self::Public
		}
	}
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Resource {
	SelectImage(String),
	SelectedImage { source: String, path: PathBuf },
	File(PathBuf),
	Network { origin: String, class: AddressClass },
}
impl fmt::Display for Resource {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::SelectImage(source) => write!(f, "{source}"),
			Self::SelectedImage { path, .. } | Self::File(path) => {
				write!(f, "{}", path.display())
			}
			Self::Network { origin, class } => {
				write!(f, "{origin} ({class:?})")
			}
		}
	}
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionRequired(pub Resource);
impl fmt::Display for PermissionRequired {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "Permission required: {}", self.0)
	}
}
impl std::error::Error for PermissionRequired {}

#[derive(
	Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct Security {
	pub origin: Origin,
	grants: Vec<Resource>,
	content: Option<String>,
}
impl Security {
	pub fn local(trust: Trust) -> Self {
		Self {
			origin: Origin::Local(trust),
			grants: Vec::new(),
			content: None,
		}
	}
	pub fn web(url: String) -> Self {
		Self {
			origin: Origin::Web(url),
			grants: Vec::new(),
			content: None,
		}
	}
	pub fn grant(&mut self, resource: Resource) {
		if !self.grants.contains(&resource) {
			self.grants.push(resource);
		}
	}
	pub fn revoke(&mut self) {
		self.grants.clear();
		self.content = None;
	}
	pub(crate) fn selected_image(&self, source: &str) -> Option<&Path> {
		self.grants.iter().find_map(|grant| match grant {
			Resource::SelectedImage {
				source: selected,
				path,
			} if selected == source => Some(path.as_path()),
			_ => None,
		})
	}
	pub(crate) fn bind(&mut self, source: &str) {
		self.content = Some(Self::source_key(source));
	}
	pub(crate) fn for_content(&self, source: &str) -> Self {
		let mut security = self.clone();
		if self
			.content
			.as_ref()
			.is_some_and(|key| *key != Self::source_key(source))
		{
			security.revoke();
		}
		security
	}
	fn source_key(source: &str) -> String {
		use sha2::{Digest, Sha256};
		format!("{:x}", Sha256::digest(source.as_bytes()))
	}
	pub(crate) fn cache_key(&self) -> String {
		use sha2::{Digest, Sha256};
		#[derive(Serialize)]
		struct Key<'a> {
			origin: &'a Origin,
			grants: Vec<serde_json::Value>,
			content: &'a Option<String>,
		}
		// Native path bytes preserve grants that are not UTF-8.
		let grants = self
			.grants
			.iter()
			.map(|grant| match grant {
				Resource::File(path) => serde_json::json!({
					"File": path.as_os_str().as_encoded_bytes(),
				}),
				Resource::SelectedImage { source, path } => serde_json::json!({
					"SelectedImage": {
						"source": source,
						"path": path.as_os_str().as_encoded_bytes(),
					},
				}),
				Resource::SelectImage(_) | Resource::Network { .. } => {
					serde_json::to_value(grant).unwrap()
				}
			})
			.collect();
		let key = Key {
			origin: &self.origin,
			grants,
			content: &self.content,
		};
		format!("{:x}", Sha256::digest(serde_json::to_vec(&key).unwrap()))
	}
	pub fn check_file(&self, path: &Path) -> anyhow::Result<()> {
		let resource = Resource::File(path.to_owned());
		if self.origin.trust() == Trust::Trusted
			|| self.grants.contains(&resource)
		{
			Ok(())
		} else {
			Err(PermissionRequired(resource).into())
		}
	}
	pub fn check_address(
		&self,
		url: &url::Url,
		ip: IpAddr,
	) -> anyhow::Result<()> {
		let class = AddressClass::of(ip);
		if class == AddressClass::Invalid {
			anyhow::bail!("Invalid network destination");
		}
		let resource = Resource::Network {
			origin: url.origin().ascii_serialization(),
			class,
		};
		match (self.origin.trust(), class) {
			(_, AddressClass::Public)
			| (
				Trust::Trusted,
				AddressClass::Private | AddressClass::Loopback,
			) => Ok(()),
			_ if self.grants.contains(&resource) => Ok(()),
			_ => Err(PermissionRequired(resource).into()),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn grants_do_not_expand_trust_origin_address_class_or_content() {
		let url = url::Url::parse("http://nas.local:8080/image").unwrap();
		let private = "192.168.1.2".parse().unwrap();
		let loopback = "127.0.0.1".parse().unwrap();
		let mut untrusted = Security::default();
		assert!(untrusted.check_address(&url, private).is_err());
		untrusted.grant(Resource::Network {
			origin: url.origin().ascii_serialization(),
			class: AddressClass::Private,
		});
		untrusted.bind("original document");
		assert!(untrusted.check_address(&url, private).is_ok());
		assert!(untrusted.check_address(&url, loopback).is_err());
		assert!(
			untrusted
				.check_address(&url, "::ffff:127.0.0.1".parse().unwrap())
				.is_err()
		);
		assert!(
			untrusted
				.check_address(
					&url::Url::parse("http://nas.local:8081/image").unwrap(),
					private
				)
				.is_err()
		);
		assert_eq!(untrusted.origin.trust(), Trust::Untrusted);
		assert!(
			untrusted
				.for_content("replacement document")
				.check_address(&url, private)
				.is_err()
		);
		let trusted = Security::local(Trust::Trusted);
		assert!(trusted.check_address(&url, private).is_ok());
		assert!(trusted.check_address(&url, loopback).is_ok());
		assert!(
			trusted
				.check_address(&url, "169.254.169.254".parse().unwrap())
				.is_err()
		);
		for ip in ["0.0.0.0", "224.0.0.1", "::", "ff02::1", "2001:db8::1"] {
			assert!(trusted.check_address(&url, ip.parse().unwrap()).is_err());
		}
	}
	#[test]
	fn address_classes_preserve_trusted_and_untrusted_resource_policy() {
		use AddressClass::*;
		let url = url::Url::parse("http://resource.example/image").unwrap();
		let trusted = Security::local(Trust::Trusted);
		let untrusted = Security::default();
		for (address, class) in [
			("1.1.1.1", Public),
			("2001:4860::1", Public),
			("192.168.1.1", Private),
			("100.64.0.1", Private),
			("100.127.255.255", Private),
			("100.128.0.1", Public),
			("fc00::1", Private),
			("127.0.0.1", Loopback),
			("::1", Loopback),
			("::ffff:127.0.0.1", Loopback),
			("169.254.1.1", LinkLocal),
			("fe80::1", LinkLocal),
			("255.255.255.255", Invalid),
			("192.0.2.1", Invalid),
			("240.0.0.1", Invalid),
			("2001:db8::1", Invalid),
		] {
			let ip = address.parse().unwrap();
			assert_eq!(AddressClass::of(ip), class, "{address}");
			assert_eq!(
				trusted.check_address(&url, ip).is_ok(),
				matches!(class, Public | Private | Loopback),
				"{address}"
			);
			assert_eq!(
				untrusted.check_address(&url, ip).is_ok(),
				class == Public,
				"{address}"
			);
		}
	}
}
