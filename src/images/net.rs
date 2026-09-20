//! The one HTTP client for document-controlled URLs.
//!
//! Resolution happens first and the surviving address is pinned, so the client
//! cannot re-resolve behind the check; redirects are followed here and every
//! hop repeats that policy. The raw response headers are returned so the disk
//! cache can decide freshness without a second client.
use super::source::bounded_to;
use anyhow::{Context, Result, bail};
use reqwest::header::HeaderMap;
use std::{
	net::{IpAddr, SocketAddr, ToSocketAddrs},
	time::{Duration, SystemTime, UNIX_EPOCH},
};
/// Redirect hops followed before a request is abandoned.
const MAX_REDIRECTS: usize = 5;

/// Conditional-request validators from a stored entry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Validators {
	pub(super) etag: Option<String>,
	pub(super) last_modified: Option<String>,
	/// The absolute URL the validators were stored for. A validator is only
	/// meaningful for the resource that supplied it, so it is attached to a
	/// request for this URL alone; every other hop is unconditional.
	pub(super) url: Option<String>,
}

impl Validators {
	/// Whether these validators answer for `url`.
	fn applies_to(&self, url: &str) -> bool {
		self.url.as_deref() == Some(url)
	}
}

/// The response headers that decide freshness, parsed once.
#[derive(Clone, Debug, Default)]
pub(super) struct Headers {
	pub(super) etag: Option<String>,
	pub(super) last_modified: Option<String>,
	/// The `Vary` field values joined. A `*` anywhere means external factors
	/// select the representation.
	pub(super) vary: Option<String>,
	pub(super) max_age: Option<u64>,
	pub(super) no_store: bool,
	pub(super) no_cache: bool,
	/// Whether a `Cache-Control` header was present at all, so a `304` can be
	/// told apart from one that only left the stored directives in place.
	pub(super) has_cache_control: bool,
	pub(super) expires: Option<SystemTime>,
	pub(super) date: Option<SystemTime>,
}

impl Headers {
	/// The explicit freshness lifetime this response grants, measured from its
	/// own `Date` (or from `now` when it omitted one). `None` means the
	/// response said nothing about how long it stays fresh.
	pub(super) fn lifetime(&self, now: SystemTime) -> Option<u64> {
		let reference = self.date.unwrap_or(now);
		self.max_age.or_else(|| {
			self.expires.and_then(|at| {
				at.duration_since(reference).ok().map(|life| life.as_secs())
			})
		})
	}

	/// Whether the response grants a lifetime of its own, so it can stand in
	/// for the original URL without asking the server again.
	fn grants_freshness(&self, now: SystemTime) -> bool {
		self.lifetime(now).is_some_and(|age| age > 0)
	}

	/// The absolute instant this response's own freshness ends, or `None` when
	/// it states no lifetime. It is measured from the response's own `Date`, so
	/// a hop dated earlier than the final response expires earlier instead of
	/// having its lifetime extended by the later date.
	fn expires_at(&self, now: SystemTime) -> Option<SystemTime> {
		let reference = self.date.unwrap_or(now);
		self.max_age
			.map(|age| reference + Duration::from_secs(age))
			.or(self.expires)
	}

	/// Whether `Vary` lists `*`, meaning factors outside the request headers
	/// select the representation, so a constant request shape cannot justify
	/// reuse.
	pub(super) fn varies_wildcard(&self) -> bool {
		self.vary.as_deref().is_some_and(|value| {
			value.split(',').any(|field| field.trim() == "*")
		})
	}
}

/// What a redirect chain allows a stored body to do.
///
/// Every hop must allow storage and grant a freshness of its own, or the final
/// body cannot stand in for the original URL. The stored body may also not
/// stay fresh past the hop that expires first, because that hop may move to
/// another resource sooner than the final response expires.
struct Chain {
	cacheable: bool,
	/// The earliest absolute expiry any hop stated, so the stored body cannot
	/// stay fresh past a hop that may move to another resource.
	expires_at: Option<SystemTime>,
}

impl Default for Chain {
	fn default() -> Self {
		// A response with no redirect hops is storable on its own.
		Self {
			cacheable: true,
			expires_at: None,
		}
	}
}

impl Chain {
	/// Folds one redirect hop into the chain's constraints.
	fn note(&mut self, hop: &Headers, now: SystemTime) {
		if hop.no_store
			|| hop.no_cache
			|| hop.varies_wildcard()
			|| !hop.grants_freshness(now)
		{
			self.cacheable = false;
		}
		if let Some(at) = hop.expires_at(now) {
			self.expires_at =
				Some(self.expires_at.map_or(at, |cap| cap.min(at)));
		}
	}
}

pub(super) struct Fetched {
	pub(super) status: u16,
	pub(super) headers: Headers,
	pub(super) body: Vec<u8>,
	/// Whether a followed redirect chain may be represented by this response.
	/// A redirect hop that forbids storage, or that grants no reusable
	/// freshness, cannot be: a body stored under the original URL would
	/// bypass re-resolving it. A response with no redirects is storable.
	pub(super) redirects_cacheable: bool,
	/// The absolute URL that supplied `headers` and `body`. A `304` answers
	/// only for this resource, never for another redirect target.
	pub(super) final_url: String,
	/// The earliest absolute instant at which any redirect hop's freshness
	/// ends, if any, so the stored body cannot outlive a hop that may move
	/// sooner. It is an absolute instant because each hop measures its own
	/// lifetime from its own `Date`.
	pub(super) freshness_cap: Option<SystemTime>,
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
///
/// `validators` add the conditional headers a cached entry uses to ask for a
/// `304` instead of a body; they are scoped to the URL stored with them, so a
/// redirect to another resource is fetched unconditionally. The returned
/// [`Fetched::final_url`] names the resource that answered, and
/// [`Fetched::freshness_cap`] is the earliest instant the chain allows.
pub(super) fn get(url: &str, validators: &Validators) -> Result<Fetched> {
	get_with(url, validators, super::source::MAX_BYTES as u64, "Image")
}

/// An unconditional GET for a caller outside the image cache, such as a font
/// download. The caller chooses the body cap; the address policy and the
/// redirect handling are exactly those of an image request.
pub(crate) fn get_body(url: &str, max: u64) -> Result<Vec<u8>> {
	Ok(get_with(url, &Validators::default(), max, "Font file")?.body)
}

fn get_with(
	url: &str,
	validators: &Validators,
	max: u64,
	what: &str,
) -> Result<Fetched> {
	let mut current = url::Url::parse(url).context("Invalid image URL")?;
	let mut chain = Chain::default();
	for _ in 0..=MAX_REDIRECTS {
		let client = pinned_client(&current)?;
		let mut request = client.get(current.clone());
		// Validators answer for one resource only: they go to the URL that
		// supplied them, and a request to any other hop is unconditional.
		if validators.applies_to(current.as_str()) {
			if let Some(etag) = &validators.etag {
				request = request
					.header(reqwest::header::IF_NONE_MATCH, etag.as_str());
			}
			if let Some(modified) = &validators.last_modified {
				request = request.header(
					reqwest::header::IF_MODIFIED_SINCE,
					modified.as_str(),
				);
			}
		}
		let response = request.send()?;
		let status = response.status();
		// `304` is a redirection status but carries no `Location`; it is the
		// answer a conditional request is looking for.
		if status == reqwest::StatusCode::NOT_MODIFIED {
			return Ok(Fetched {
				status: 304,
				headers: headers(response.headers()),
				body: Vec::new(),
				redirects_cacheable: chain.cacheable,
				final_url: current.to_string(),
				freshness_cap: chain.expires_at,
			});
		}
		if status.is_redirection() {
			// The final body may only stand in for the original URL while
			// every hop on the way stays reusable, and its freshness may not
			// outlast the shortest-lived hop.
			chain.note(&headers(response.headers()), SystemTime::now());
			let location = response
				.headers()
				.get(reqwest::header::LOCATION)
				.and_then(|value| value.to_str().ok())
				.context("Redirect without a location")?;
			current =
				current.join(location).context("Invalid redirect target")?;
			continue;
		}
		let headers = headers(response.headers());
		let body = bounded_to(response.error_for_status()?, max, what)?;
		return Ok(Fetched {
			status: status.as_u16(),
			headers,
			body,
			redirects_cacheable: chain.cacheable,
			final_url: current.to_string(),
			freshness_cap: chain.expires_at,
		});
	}
	bail!("Image redirects to too many locations")
}

pub(super) fn headers(map: &HeaderMap) -> Headers {
	// A field may repeat and every value still applies, so all values are
	// joined before they are interpreted.
	let text = |name: reqwest::header::HeaderName| {
		let values: Vec<&str> = map
			.get_all(name)
			.iter()
			.filter_map(|value| value.to_str().ok())
			.collect();
		if values.is_empty() {
			None
		} else {
			Some(values.join(", "))
		}
	};
	let directives = text(reqwest::header::CACHE_CONTROL);
	let (max_age, no_store, no_cache) =
		directives.as_deref().map(cache_control).unwrap_or_default();
	Headers {
		etag: text(reqwest::header::ETAG),
		last_modified: text(reqwest::header::LAST_MODIFIED),
		vary: text(reqwest::header::VARY),
		max_age,
		no_store,
		no_cache,
		has_cache_control: directives.is_some(),
		expires: text(reqwest::header::EXPIRES)
			.and_then(|value| http_date(&value)),
		date: text(reqwest::header::DATE).and_then(|value| http_date(&value)),
	}
}

/// `max-age`, `no-store`, and `no-cache` from a `Cache-Control` value.
///
/// `must-revalidate` needs no flag: the cache never serves a stale entry while
/// online, so it is honored by construction.
fn cache_control(value: &str) -> (Option<u64>, bool, bool) {
	let mut max_age = None;
	let mut no_store = false;
	let mut no_cache = false;
	for directive in value.split(',') {
		let directive = directive.trim();
		let (name, value) = match directive.split_once('=') {
			Some((name, value)) => {
				(name.trim(), Some(value.trim().trim_matches('"')))
			}
			None => (directive, None),
		};
		match name.to_ascii_lowercase().as_str() {
			"max-age" => max_age = value.and_then(|v| v.parse().ok()),
			"no-store" => no_store = true,
			"no-cache" => no_cache = true,
			_ => {}
		}
	}
	(max_age, no_store, no_cache)
}

/// Parses the IMF-fixdate form of an HTTP date (`Sun, 06 Nov 1994 08:49:37 GMT`).
///
/// The obsolete forms are treated as absent: an unparsed `Expires` makes the
/// entry stale, which costs one revalidation and never a wrong body.
fn http_date(value: &str) -> Option<SystemTime> {
	let rest = value.trim().split_once(", ")?.1;
	let mut parts = rest.split(' ');
	let day: u32 = parts.next()?.parse().ok()?;
	let month = match parts.next()? {
		"Jan" => 1,
		"Feb" => 2,
		"Mar" => 3,
		"Apr" => 4,
		"May" => 5,
		"Jun" => 6,
		"Jul" => 7,
		"Aug" => 8,
		"Sep" => 9,
		"Oct" => 10,
		"Nov" => 11,
		"Dec" => 12,
		_ => return None,
	};
	let year: i64 = parts.next()?.parse().ok()?;
	let mut clock = parts.next()?.split(':');
	let hour: u64 = clock.next()?.parse().ok()?;
	let minute: u64 = clock.next()?.parse().ok()?;
	let second: u64 = clock.next()?.parse().ok()?;
	if !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
		return None;
	}
	let days = days_from_civil(year, month, day);
	if days < 0 {
		return None;
	}
	let seconds = days as u64 * 86_400 + hour * 3_600 + minute * 60 + second;
	Some(UNIX_EPOCH + Duration::from_secs(seconds))
}

/// Days since 1970-01-01 for a proleptic Gregorian date.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
	let year = if month <= 2 { year - 1 } else { year };
	let era = year.div_euclid(400);
	let yoe = year - era * 400;
	let mp = i64::from((month + 9) % 12);
	let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
	let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
	era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn cache_control_directives_are_parsed() {
		let (max_age, no_store, no_cache) =
			cache_control("public, max-age=600, must-revalidate");
		assert_eq!(max_age, Some(600));
		assert!(!no_store && !no_cache);
		assert_eq!(cache_control("no-store"), (None, true, false));
		assert_eq!(cache_control("no-cache"), (None, false, true));
		assert_eq!(cache_control("max-age=\"60\"").0, Some(60));
		assert_eq!(cache_control("private").0, None);
	}

	#[test]
	fn http_dates_parse_the_imf_fixdate_form() {
		let epoch = UNIX_EPOCH;
		assert_eq!(http_date("Thu, 01 Jan 1970 00:00:00 GMT"), Some(epoch));
		assert_eq!(
			http_date("Sun, 06 Nov 1994 08:49:37 GMT"),
			Some(epoch + Duration::from_secs(784_111_777))
		);
		assert!(http_date("Wed, 21 Oct 2015 07:28:00 GMT").is_some());
		assert_eq!(http_date("not a date"), None);
		assert_eq!(http_date("Sun, 32 Nov 1994 08:49:37 GMT"), None);
		assert_eq!(http_date("Sun, 06 Xxx 1994 08:49:37 GMT"), None);
	}

	#[test]
	fn validators_only_apply_to_the_url_that_supplied_them() {
		let validators = Validators {
			etag: Some("\"v1\"".into()),
			url: Some("https://cdn.example.com/a.png".into()),
			..Default::default()
		};
		assert!(validators.applies_to("https://cdn.example.com/a.png"));
		// A redirect target that happens to share the ETag must be fetched
		// unconditionally instead of answering for the old resource.
		assert!(!validators.applies_to("https://cdn.example.com/b.png"));
		assert!(
			!Validators::default().applies_to("https://cdn.example.com/a.png")
		);
	}

	#[test]
	fn a_hop_lifetime_is_measured_from_its_own_date() {
		let now = UNIX_EPOCH + Duration::from_secs(1_000_000);
		let max_age = Headers {
			max_age: Some(30),
			date: Some(now),
			..Default::default()
		};
		assert_eq!(max_age.lifetime(now), Some(30));
		assert!(max_age.grants_freshness(now));
		let expires = Headers {
			expires: Some(now + Duration::from_secs(15)),
			date: Some(now),
			..Default::default()
		};
		assert_eq!(expires.lifetime(now), Some(15));
		assert!(expires.grants_freshness(now));
		// Neither directive, or an `Expires` already past, grants nothing.
		assert_eq!(Headers::default().lifetime(now), None);
		let stale = Headers {
			expires: Some(now - Duration::from_secs(1)),
			date: Some(now),
			..Default::default()
		};
		assert_eq!(stale.lifetime(now), None);
		assert!(!stale.grants_freshness(now));
	}

	#[test]
	fn every_hop_constraint_folds_into_the_chain() {
		let now = UNIX_EPOCH + Duration::from_secs(1_000_000);
		let hop = |max_age| Headers {
			max_age: Some(max_age),
			date: Some(now),
			..Default::default()
		};
		let mut chain = Chain::default();
		assert!(chain.cacheable);
		assert_eq!(chain.expires_at, None);
		chain.note(&hop(600), now);
		assert_eq!(chain.expires_at, Some(now + Duration::from_secs(600)));
		chain.note(&hop(30), now);
		// The soonest hop decides, even though the final response is longer.
		assert_eq!(chain.expires_at, Some(now + Duration::from_secs(30)));
		chain.note(&hop(900), now);
		assert_eq!(chain.expires_at, Some(now + Duration::from_secs(30)));
		// A hop dated before the final response keeps its own expiry instead
		// of having its lifetime extended by the later `Date`.
		chain.note(
			&Headers {
				max_age: Some(60),
				date: Some(now - Duration::from_secs(50)),
				..Default::default()
			},
			now,
		);
		assert_eq!(chain.expires_at, Some(now + Duration::from_secs(10)));
		assert!(chain.cacheable);
		// Wildcard variation selects the representation by factors outside
		// the request, so no hop may be reused from the cache.
		chain.note(
			&Headers {
				max_age: Some(600),
				vary: Some("*".into()),
				..Default::default()
			},
			now,
		);
		assert!(!chain.cacheable);
		// A hop that grants no freshness of its own makes the chain
		// unstorable, because it may move at any time.
		chain.note(&Headers::default(), now);
		assert!(!chain.cacheable);
	}
}
