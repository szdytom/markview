//! Source resolution and bounded reads; independent of decoding and scheduling.
use anyhow::{Context, Result, bail};
use base64::Engine;
use std::{
	fs,
	io::Read,
	net::{IpAddr, SocketAddr, ToSocketAddrs},
	path::{Path, PathBuf},
	time::{Duration, SystemTime},
};
const MAX_BYTES: usize = 32 * 1024 * 1024;
/// Redirect hops followed before a request is abandoned.
const MAX_REDIRECTS: usize = 5;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum Source {
	File(PathBuf),
	Http(String),
	Data(String),
}

pub(super) fn source(
	src: &str,
	document: &Path,
	offline: bool,
) -> Result<Source> {
	if src.is_empty() {
		bail!("Missing image source");
	}
	if let Ok(url) = url::Url::parse(src) {
		return match url.scheme() {
			"http" | "https" if !offline => Ok(Source::Http(url.to_string())),
			"http" | "https" => {
				anyhow::bail!("Network images disabled (--offline)")
			}
			"data" => Ok(Source::Data(src.to_owned())),
			_ => anyhow::bail!("Unsupported image URL scheme"),
		};
	}
	let decoded = percent_encoding::percent_decode_str(src)
		.decode_utf8()
		.context("Invalid path encoding")?;
	let path = Path::new(decoded.as_ref());
	// Only paths relative to the document are reachable. `..` is allowed: it
	// names another relative location, and the reader cannot exfiltrate what
	// it reads. See `docs/security.md` for the accepted residual.
	if rooted(path) {
		bail!("Absolute image paths are not allowed");
	}
	let path = document.parent().unwrap_or(Path::new(".")).join(path);
	Ok(Source::File(fs::canonicalize(&path).unwrap_or(path)))
}

/// Whether a path names an absolute location, including the Windows forms
/// `\foo` (rooted, not `is_absolute`) and `C:foo` (drive-relative).
pub(super) fn rooted(path: &Path) -> bool {
	if path.is_absolute() {
		return true;
	}
	matches!(
		path.components().next(),
		Some(std::path::Component::Prefix(_) | std::path::Component::RootDir)
	)
}

/// Whether a document may reach this address.
///
/// Loopback, link-local, and private ranges are refused so a document cannot
/// use the reader as a request proxy against local services. The check runs on
/// every resolved address, and the chosen address is then pinned, so a rebind
/// between resolution and connection cannot slip a private address through.
pub(super) fn permitted(ip: IpAddr) -> bool {
	if let IpAddr::V6(v6) = ip
		&& let Some(v4) = v6.to_ipv4_mapped()
	{
		return permitted(IpAddr::V4(v4));
	}
	match ip {
		IpAddr::V4(v4) => {
			let o = v4.octets();
			!(v4.is_private()
				|| v4.is_loopback()
				|| v4.is_link_local()
				|| v4.is_broadcast()
				|| v4.is_unspecified()
				|| v4.is_documentation()
				|| v4.is_multicast()
				|| o[0] == 0
				|| o[0] >= 240
				// Carrier-grade NAT, 100.64.0.0/10.
				|| (o[0] == 100 && (64..=127).contains(&o[1])))
		}
		IpAddr::V6(v6) => {
			!(v6.is_loopback()
				|| v6.is_unspecified()
				|| v6.is_unique_local()
				|| v6.is_unicast_link_local()
				|| v6.is_multicast())
		}
	}
}

fn bounded(mut reader: impl Read) -> Result<Vec<u8>> {
	let mut bytes = Vec::new();
	reader
		.by_ref()
		.take((MAX_BYTES + 1) as u64)
		.read_to_end(&mut bytes)?;
	if bytes.len() > MAX_BYTES {
		bail!("Image exceeds 32 MiB");
	}
	Ok(bytes)
}

/// Builds a client whose connection can only go to a public address of `url`.
///
/// Resolution happens here rather than inside the client so the addresses are
/// inspected first and then pinned; the client cannot re-resolve behind us.
fn pinned_client(url: &url::Url) -> Result<reqwest::blocking::Client> {
	if !matches!(url.scheme(), "http" | "https") {
		bail!("Unsupported image URL scheme");
	}
	// `Url::host_str` keeps the brackets of an IPv6 literal, which does not
	// resolve; the address itself is what a lookup and a pin need.
	let host = match url.host() {
		Some(url::Host::Domain(domain)) => domain.to_owned(),
		Some(url::Host::Ipv4(addr)) => addr.to_string(),
		Some(url::Host::Ipv6(addr)) => addr.to_string(),
		None => bail!("Image URL has no host"),
	};
	let port = url
		.port_or_known_default()
		.context("Image URL has no port")?;
	let addrs: Vec<SocketAddr> = (host.as_str(), port)
		.to_socket_addrs()
		.context("Cannot resolve image host")?
		.collect();
	if addrs.is_empty() {
		bail!("Image host has no address");
	}
	for addr in &addrs {
		if !permitted(addr.ip()) {
			bail!("Image host resolves to a local or private address");
		}
	}
	reqwest::blocking::Client::builder()
		.timeout(Duration::from_secs(15))
		.connect_timeout(Duration::from_secs(5))
		.referer(false)
		.redirect(reqwest::redirect::Policy::none())
		.resolve_to_addrs(&host, &addrs)
		.build()
		.context("Image client")
}

/// Fetches over HTTP(S), validating and re-pinning every redirect hop.
fn http_get(url: &str) -> Result<Vec<u8>> {
	let mut current = url::Url::parse(url).context("Invalid image URL")?;
	for _ in 0..=MAX_REDIRECTS {
		let client = pinned_client(&current)?;
		let response = client.get(current.clone()).send()?;
		if response.status().is_redirection() {
			let location = response
				.headers()
				.get(reqwest::header::LOCATION)
				.and_then(|value| value.to_str().ok())
				.context("Redirect without a location")?;
			current =
				current.join(location).context("Invalid redirect target")?;
			continue;
		}
		return bounded(response.error_for_status()?);
	}
	bail!("Image redirects to too many locations")
}

pub(super) fn fetch(source: &Source) -> Result<Vec<u8>> {
	match source {
		Source::File(path) => {
			let file = fs::File::open(path).context("Cannot open image")?;
			if !file.metadata()?.is_file() {
				bail!("Image is not a regular file");
			}
			bounded(file)
		}
		Source::Http(url) => http_get(url),
		Source::Data(uri) => {
			let (header, data) =
				uri.split_once(',').context("Invalid data URI")?;
			if !header.to_ascii_lowercase().starts_with("data:image/") {
				bail!("Data URI must contain an image");
			}
			if data.len() > MAX_BYTES * 3 {
				bail!("Image exceeds 32 MiB");
			}
			let data =
				percent_encoding::percent_decode_str(data).collect::<Vec<_>>();
			let bytes = if header.to_ascii_lowercase().ends_with(";base64") {
				base64::engine::general_purpose::STANDARD.decode(data)?
			} else {
				data
			};
			if bytes.len() > MAX_BYTES {
				bail!("Image exceeds 32 MiB");
			}
			Ok(bytes)
		}
	}
}

pub(super) fn stamp(source: &Source) -> Option<(u64, Option<SystemTime>)> {
	if let Source::File(path) = source {
		fs::metadata(path)
			.ok()
			.map(|m| (m.len(), m.modified().ok()))
	} else {
		None
	}
}
