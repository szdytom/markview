//! The one HTTP client for document-controlled URLs.
//!
//! Resolution happens first and the surviving address is pinned, so the client
//! cannot re-resolve behind the check; redirects are followed here and every
//! hop repeats that policy. The raw response headers are returned so the disk
//! cache can decide freshness without a second client.
use anyhow::{Context, Result, bail};
use reqwest::header::HeaderMap;
use std::{
	io::{Read, Write},
	net::{IpAddr, SocketAddr, ToSocketAddrs},
	path::Path,
	time::{Duration, SystemTime, UNIX_EPOCH},
};
/// Redirect hops followed before a request is abandoned.
const MAX_REDIRECTS: usize = 5;

/// The download client names itself: some mirrors refuse a request with no
/// `User-Agent`, and others refuse a browser one as hotlinking.
const USER_AGENT: &str = concat!("markview/", env!("CARGO_PKG_VERSION"));

/// Conditional-request validators from a stored entry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Validators {
	pub(crate) etag: Option<String>,
	pub(crate) last_modified: Option<String>,
	/// The absolute URL the validators were stored for. A validator is only
	/// meaningful for the resource that supplied it, so it is attached to a
	/// request for this URL alone; every other hop is unconditional.
	pub(crate) url: Option<String>,
}

impl Validators {
	/// Whether these validators answer for `url`.
	fn applies_to(&self, url: &str) -> bool {
		self.url.as_deref() == Some(url)
	}
}

/// The response headers that decide freshness, parsed once.
#[derive(Clone, Debug, Default)]
pub(crate) struct Headers {
	pub(crate) etag: Option<String>,
	pub(crate) last_modified: Option<String>,
	/// The `Vary` field values joined. A `*` anywhere means external factors
	/// select the representation.
	pub(crate) vary: Option<String>,
	pub(crate) max_age: Option<u64>,
	pub(crate) no_store: bool,
	pub(crate) no_cache: bool,
	/// Whether a `Cache-Control` header was present at all, so a `304` can be
	/// told apart from one that only left the stored directives in place.
	pub(crate) has_cache_control: bool,
	pub(crate) expires: Option<SystemTime>,
	pub(crate) date: Option<SystemTime>,
}

impl Headers {
	/// The explicit freshness lifetime this response grants, measured from its
	/// own `Date` (or from `now` when it omitted one). `None` means the
	/// response said nothing about how long it stays fresh.
	pub(crate) fn lifetime(&self, now: SystemTime) -> Option<u64> {
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
	pub(crate) fn varies_wildcard(&self) -> bool {
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

pub(crate) struct Fetched {
	pub(crate) status: u16,
	pub(crate) headers: Headers,
	pub(crate) body: Vec<u8>,
	/// Whether a followed redirect chain may be represented by this response.
	/// A redirect hop that forbids storage, or that grants no reusable
	/// freshness, cannot be: a body stored under the original URL would
	/// bypass re-resolving it. A response with no redirects is storable.
	pub(crate) redirects_cacheable: bool,
	/// The absolute URL that supplied `headers` and `body`. A `304` answers
	/// only for this resource, never for another redirect target.
	pub(crate) final_url: String,
	/// The earliest absolute instant at which any redirect hop's freshness
	/// ends, if any, so the stored body cannot outlive a hop that may move
	/// sooner. It is an absolute instant because each hop measures its own
	/// lifetime from its own `Date`.
	pub(crate) freshness_cap: Option<SystemTime>,
}

/// Whether a document may reach this address.
///
/// Loopback, link-local, and private ranges are refused so a document cannot
/// use the reader as a request proxy against local services. The check runs on
/// every resolved address, and the chosen address is then pinned, so a rebind
/// between resolution and connection cannot slip a private address through.
pub(crate) fn permitted(ip: IpAddr) -> bool {
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
/// The host and the permitted addresses `url` resolves to.
///
/// `Url::host_str` keeps the brackets of an IPv6 literal, which does not
/// resolve; the address itself is what a lookup and a pin need.
fn resolved(url: &url::Url, what: &str) -> Result<(String, Vec<SocketAddr>)> {
	let host = match url.host() {
		Some(url::Host::Domain(domain)) => domain.to_owned(),
		Some(url::Host::Ipv4(addr)) => addr.to_string(),
		Some(url::Host::Ipv6(addr)) => addr.to_string(),
		None => bail!("{what} URL has no host"),
	};
	let port = url
		.port_or_known_default()
		.with_context(|| format!("{what} URL has no port"))?;
	let addrs: Vec<SocketAddr> = (host.as_str(), port)
		.to_socket_addrs()
		.with_context(|| format!("Cannot resolve {what} host"))?
		.collect();
	if addrs.is_empty() {
		bail!("{what} host has no address");
	}
	for addr in &addrs {
		if !permitted(addr.ip()) {
			bail!("{what} host resolves to a local or private address");
		}
	}
	Ok((host, addrs))
}

/// Whether `url` may be fetched at all, before any address is resolved.
fn check_scheme(url: &url::Url, what: &str) -> Result<()> {
	if !matches!(url.scheme(), "http" | "https") {
		bail!("Unsupported {what} URL scheme");
	}
	Ok(())
}

/// Builds a client whose connection can only go to a public address of `url`,
/// naming the fetched resource `what` in the errors.
fn pinned_client(
	url: &url::Url,
	what: &str,
) -> Result<reqwest::blocking::Client> {
	check_scheme(url, what)?;
	let (host, addrs) = resolved(url, what)?;
	reqwest::blocking::Client::builder()
		.timeout(Duration::from_secs(15))
		.connect_timeout(Duration::from_secs(5))
		.referer(false)
		.redirect(reqwest::redirect::Policy::none())
		.resolve_to_addrs(&host, &addrs)
		.build()
		.with_context(|| format!("{what} client"))
}

/// A transfer that may make no progress at all before it is abandoned.
///
/// A whole-request timeout cannot serve a font archive: the same client that
/// gives an image fifteen seconds would cut a hundred-megabyte transfer off in
/// the middle. `read_timeout` bounds the silence between bytes instead.
const STALL_TIMEOUT: Duration = Duration::from_secs(60);
/// How often a cancelled transfer is noticed, even while it is stalled.
const CANCEL_POLL: Duration = Duration::from_millis(150);

/// Streams a document-controlled URL into a file.
///
/// Only the async client carries a read timeout, so one small runtime drives
/// this path; the image cache keeps its blocking client and its own limits.
/// `what` names the resource the transfers are for, so a font failure is not
/// reported as an image one.
pub(crate) struct Downloader {
	runtime: tokio::runtime::Runtime,
	what: &'static str,
}

impl Downloader {
	pub(crate) fn new(what: &'static str) -> Result<Self> {
		let runtime = tokio::runtime::Builder::new_multi_thread()
			.worker_threads(1)
			.enable_all()
			.build()
			.context("Download runtime")?;
		Ok(Self { runtime, what })
	}

	/// Streams `url` into `path`, bounded by `cap` bytes, reporting the bytes
	/// written so far after every chunk. `cancel` is polled throughout, so a
	/// stalled transfer still stops promptly.
	pub(crate) fn fetch(
		&self,
		url: &str,
		path: &Path,
		cap: u64,
		progress: &mut dyn FnMut(u64),
		cancel: &dyn Fn() -> bool,
	) -> Result<()> {
		self.runtime
			.block_on(fetch_into(url, path, cap, progress, cancel, self.what))
	}

	/// How long `url` takes to answer a one-byte range request.
	pub(crate) fn probe(&self, url: &str) -> Result<Duration> {
		self.runtime.block_on(probe_once(url, self.what))
	}
}

/// A client pinned to the addresses `url`'s host resolved to, as
/// [`pinned_client`] does for the blocking paths.
fn pinned_async_client(
	url: &url::Url,
	read_timeout: Duration,
	total_timeout: Option<Duration>,
	what: &str,
) -> Result<reqwest::Client> {
	check_scheme(url, what)?;
	let (host, addrs) = resolved(url, what)?;
	let mut builder = reqwest::Client::builder()
		.connect_timeout(Duration::from_secs(5))
		.read_timeout(read_timeout)
		.referer(false)
		.user_agent(USER_AGENT)
		.redirect(reqwest::redirect::Policy::none())
		.resolve_to_addrs(&host, &addrs);
	if let Some(total) = total_timeout {
		builder = builder.timeout(total);
	}
	builder.build().with_context(|| format!("{what} client"))
}

/// Follows one redirect hop, or returns the target of the next one.
fn next_hop(
	current: &url::Url,
	response: &reqwest::Response,
) -> Result<url::Url> {
	let location = response
		.headers()
		.get(reqwest::header::LOCATION)
		.and_then(|value| value.to_str().ok())
		.context("Redirect without a location")?;
	current.join(location).context("Invalid redirect target")
}

async fn fetch_into(
	url: &str,
	path: &Path,
	cap: u64,
	progress: &mut dyn FnMut(u64),
	cancel: &dyn Fn() -> bool,
	what: &str,
) -> Result<()> {
	let mut current =
		url::Url::parse(url).with_context(|| format!("Invalid {what} URL"))?;
	for _ in 0..=MAX_REDIRECTS {
		// A cancelled transfer must not even open a connection, and a server
		// that accepts one and then stalls before its headers must not hold
		// the cancellation off until the stall timeout.
		if cancel() {
			bail!("Cancelled");
		}
		let client = pinned_async_client(&current, STALL_TIMEOUT, None, what)?;
		let response = tokio::select! {
			response = client.get(current.clone()).send() => response?,
			_ = wait_for_cancel(cancel) => bail!("Cancelled"),
		};
		if response.status().is_redirection() {
			current = next_hop(&current, &response)?;
			continue;
		}
		let response = response.error_for_status()?;
		let total = response.content_length();
		if let Some(total) = total
			&& total > cap
		{
			bail!("File exceeds {} MiB", cap / (1024 * 1024));
		}
		let body = stream_body(response, path, cap, total, progress);
		tokio::pin!(body);
		tokio::select! {
			result = &mut body => result?,
			_ = wait_for_cancel(cancel) => bail!("Cancelled"),
		}
		return Ok(());
	}
	bail!("{what} redirects to too many locations")
}

async fn stream_body(
	mut response: reqwest::Response,
	path: &Path,
	cap: u64,
	total: Option<u64>,
	progress: &mut dyn FnMut(u64),
) -> Result<()> {
	let mut file = std::fs::File::create(path)
		.with_context(|| format!("Cannot write {}", path.display()))?;
	let mut written = 0u64;
	while let Some(chunk) = response.chunk().await? {
		written = written.saturating_add(chunk.len() as u64);
		if written > cap {
			bail!("File exceeds {} MiB", cap / (1024 * 1024));
		}
		file.write_all(&chunk)?;
		progress(written);
	}
	file.sync_all()?;
	// A body shorter than its own announced length is a truncated transfer,
	// which must not be mistaken for a complete file.
	if let Some(total) = total
		&& written != total
	{
		bail!("Truncated transfer");
	}
	Ok(())
}

async fn wait_for_cancel(cancel: &dyn Fn() -> bool) {
	while !cancel() {
		tokio::time::sleep(CANCEL_POLL).await;
	}
}

async fn probe_once(url: &str, what: &str) -> Result<Duration> {
	let mut current =
		url::Url::parse(url).with_context(|| format!("Invalid {what} URL"))?;
	let started = std::time::Instant::now();
	for _ in 0..=MAX_REDIRECTS {
		let client = pinned_async_client(
			&current,
			Duration::from_secs(5),
			Some(Duration::from_secs(10)),
			what,
		)?;
		let response = client
			.get(current.clone())
			.header(reqwest::header::RANGE, "bytes=0-0")
			.send()
			.await?;
		if response.status().is_redirection() {
			current = next_hop(&current, &response)?;
			continue;
		}
		response.error_for_status()?;
		return Ok(started.elapsed());
	}
	bail!("{what} redirects to too many locations")
}

/// Fetches over HTTP(S), validating and re-pinning every redirect hop.
///
/// `validators` add the conditional headers a cached entry uses to ask for a
/// `304` instead of a body; they are scoped to the URL stored with them, so a
/// redirect to another resource is fetched unconditionally. The returned
/// [`Fetched::final_url`] names the resource that answered, and
/// [`Fetched::freshness_cap`] is the earliest instant the chain allows.
/// `what` names the resource being fetched, so an error names the transfer
/// that failed rather than the one this transport was first built for.
pub(crate) fn get(
	url: &str,
	validators: &Validators,
	max: u64,
	what: &str,
) -> Result<Fetched> {
	let mut current =
		url::Url::parse(url).with_context(|| format!("Invalid {what} URL"))?;
	let mut chain = Chain::default();
	for _ in 0..=MAX_REDIRECTS {
		let client = pinned_client(&current, what)?;
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
	bail!("{what} redirects to too many locations")
}

pub(crate) fn headers(map: &HeaderMap) -> Headers {
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

/// Reads at most `max` bytes, refusing a larger body. `what` names the body in
/// the error because the cap is the caller's own policy.
pub(crate) fn bounded_to(
	mut reader: impl Read,
	max: u64,
	what: &str,
) -> Result<Vec<u8>> {
	let mut bytes = Vec::new();
	reader.by_ref().take(max + 1).read_to_end(&mut bytes)?;
	if bytes.len() as u64 > max {
		bail!("{what} exceeds {} MiB", max / (1024 * 1024));
	}
	Ok(bytes)
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
	fn errors_name_the_resource_the_caller_asked_for() {
		// The transport is shared, so a font failure must not be reported
		// as an image one.
		let local = url::Url::parse("file:///fonts/a.ttf").unwrap();
		let message = pinned_client(&local, "Font").unwrap_err().to_string();
		assert!(message.contains("Font"), "{message}");
		assert!(!message.contains("Image"), "{message}");
		let message = get("notaurl", &Validators::default(), 1024, "Font")
			.err()
			.expect("an unparsable URL is refused")
			.to_string();
		assert!(message.contains("Font"), "{message}");
		assert!(!message.contains("Image"), "{message}");
		// Fonts use the streaming downloader rather than `get`, so the
		// name it was built with has to reach its own errors too.
		let downloader = Downloader::new("Font").unwrap();
		let message = downloader.probe("notaurl").unwrap_err().to_string();
		assert!(message.contains("Font"), "{message}");
		assert!(!message.contains("Image"), "{message}");
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
