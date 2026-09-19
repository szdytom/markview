//! Bounded, browser-like disk cache for fetched image bodies.
//!
//! One file per URL key, holding a small JSON header and the body, installed
//! by writing a temporary file and renaming it, so a partially written body is
//! never visible under an entry name. The cache lives beside `settings.toml`
//! and holds at most [`MAX_BYTES`], dropping least-recently-used entries first.
//! A lookup reads the header only, so revalidating a stale entry never has to
//! read its body, and a freshly fetched response decodes exactly like the
//! cached one because the bytes are stored verbatim.
//!
//! The key is the absolute URL. The reader sends no user agent or `Accept` of
//! its own and the client's `Accept: */*` is constant, so no other request
//! header can select a different body.
use super::net::{Fetched, Headers, Validators};
use super::source::bounded;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
	collections::hash_map::DefaultHasher,
	fs,
	hash::{Hash, Hasher},
	io::{Read, Write},
	path::{Path, PathBuf},
	sync::atomic::{AtomicU64, Ordering},
	time::{Duration, SystemTime, UNIX_EPOCH},
};
/// Total bytes the cache keeps. 128 MiB holds hundreds of ordinary diagrams
/// and photographs while staying modest beside the document it serves.
pub(super) const MAX_BYTES: u64 = 128 * 1024 * 1024;
const MAGIC: &[u8] = b"MARKVIEW-CACHE/1\n";
/// The JSON header is tiny; anything larger is a corrupt file.
const MAX_HEADER: usize = 8 * 1024;
/// A temporary file older than this is debris from a crashed writer.
const TEMP_AGE: Duration = Duration::from_secs(3600);
static TEMP: AtomicU64 = AtomicU64::new(0);

/// What a stored response must remember to be reused or revalidated.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct Meta {
	url: String,
	/// The absolute URL that supplied `body` after redirects. Validators are
	/// only reused for this resource; a missing field predates redirects
	/// being recorded, so it falls back to `url`.
	#[serde(default)]
	final_url: String,
	etag: Option<String>,
	last_modified: Option<String>,
	/// Freshness lifetime in seconds, from `max-age` or `Expires - Date`, so a
	/// `304` that only replaces `Date` can remeasure it.
	lifetime: Option<u64>,
	/// Unix seconds of the response `Date`, the base the lifetime runs from.
	date: Option<u64>,
	/// The response said `no-cache`: revalidate before every use.
	no_cache: bool,
	/// Stored body length, so a truncated file is never served.
	bytes: u64,
}

impl Meta {
	/// The URL that supplied the stored body, which may follow redirects.
	fn final_url(&self) -> &str {
		if self.final_url.is_empty() {
			&self.url
		} else {
			&self.final_url
		}
	}

	/// Unix seconds at which the entry becomes stale, or `None` when the
	/// response carried no freshness information.
	fn expires_at(&self) -> Option<u64> {
		self.lifetime
			.map(|age| self.date.unwrap_or(0).saturating_add(age))
	}

	fn fresh(&self, now: u64) -> bool {
		!self.no_cache && self.expires_at().is_some_and(|at| now < at)
	}

	fn validators(&self) -> Validators {
		Validators {
			etag: self.etag.clone(),
			last_modified: self.last_modified.clone(),
			url: Some(self.final_url().to_owned()),
		}
	}
}

/// Builds the metadata a stored response needs from its headers.
fn metadata(
	url: &str,
	final_url: &str,
	headers: &Headers,
	body: &[u8],
	now: SystemTime,
) -> Meta {
	// Freshness is a lifetime measured from the response `Date`; `Expires` is
	// turned into one so a later `304` can remeasure it. A server that omits
	// `Date` costs at most the transfer latency of overestimated freshness.
	// `Age` is ignored: this is a private cache, not a shared proxy chain.
	let reference = headers.date.unwrap_or(now);
	Meta {
		url: url.to_owned(),
		final_url: final_url.to_owned(),
		etag: headers.etag.clone(),
		last_modified: headers.last_modified.clone(),
		lifetime: headers.lifetime(now),
		date: Some(unix(reference)),
		no_cache: headers.no_cache,
		bytes: body.len() as u64,
	}
}

/// Caps a stored lifetime at the earliest redirect hop expiry, so the entry
/// cannot outlive a hop that may move sooner. The hop expiry is an absolute
/// instant, independent of the final response's `Date`, so it is measured back
/// from the date the stored lifetime runs from.
fn capped(
	lifetime: Option<u64>,
	date: u64,
	cap: Option<SystemTime>,
) -> Option<u64> {
	match (lifetime, cap.and_then(time_unix)) {
		(Some(life), Some(cap)) => Some(life.min(cap.saturating_sub(date))),
		_ => lifetime,
	}
}

/// Refreshes a stored entry from a `304` response. A `304` may repeat only
/// some headers, and RFC 9111 keeps whatever it did not replace, so a missing
/// validator or freshness directive reuses the stored one. A stored lifetime
/// is remeasured from the new `Date`, so a header-light `304` does not leave
/// the entry already expired.
fn refreshed(
	old: &Meta,
	url: &str,
	final_url: &str,
	headers: &Headers,
	body: &[u8],
	now: SystemTime,
	cap: Option<SystemTime>,
) -> Meta {
	let mut new = metadata(url, final_url, headers, body, now);
	new.etag = new.etag.or_else(|| old.etag.clone());
	new.last_modified = new.last_modified.or_else(|| old.last_modified.clone());
	// A `304` that repeats `Cache-Control` replaces the stored directives;
	// one that omits it keeps them.
	if !headers.has_cache_control {
		new.no_cache = old.no_cache;
	}
	// When the response says nothing about freshness at all, the stored
	// lifetime carries over and runs from the new `Date`.
	if !headers.has_cache_control && headers.expires.is_none() {
		new.lifetime = old.lifetime;
	}
	new.lifetime = capped(new.lifetime, new.date.unwrap_or(0), cap);
	new
}

fn unix(time: SystemTime) -> u64 {
	time_unix(time).unwrap_or(0)
}

fn time_unix(time: SystemTime) -> Option<u64> {
	time.duration_since(UNIX_EPOCH)
		.ok()
		.map(|since| since.as_secs())
}

/// The image cache directory, beside `settings.toml`.
pub(super) fn directory() -> Option<PathBuf> {
	crate::settings::config_path()
		.and_then(|path| path.parent().map(|dir| dir.join("cache/images")))
}

/// A cache rooted at one directory. Nothing is created until the first store,
/// so a document without network images pays nothing.
#[derive(Clone)]
pub(super) struct Cache {
	root: PathBuf,
	limit: u64,
}

impl Cache {
	pub(super) fn new(root: PathBuf) -> Self {
		Self {
			root,
			limit: MAX_BYTES,
		}
	}

	#[cfg(test)]
	fn with_limit(root: PathBuf, limit: u64) -> Self {
		Self { root, limit }
	}

	/// Hashes the key to a file name. A collision would store another URL's
	/// body, so every read also checks the URL recorded in the header.
	fn path(&self, key: &str) -> PathBuf {
		let mut hasher = DefaultHasher::new();
		key.hash(&mut hasher);
		self.root.join(format!("{:016x}.img", hasher.finish()))
	}

	/// Reads only the header; a stale entry revalidates without its body.
	fn load_meta(&self, key: &str) -> Option<Meta> {
		let (_, meta) = open(&self.path(key)).ok()?;
		(meta.url == key).then_some(meta)
	}

	/// Reads a whole entry. The metadata and the body come from one open file,
	/// so a concurrent replacement can never pair another entry's bytes with
	/// this entry's validators.
	fn load_entry(&self, key: &str) -> Option<(Meta, Vec<u8>)> {
		let (mut file, meta) = open(&self.path(key)).ok()?;
		if meta.url != key {
			return None;
		}
		// The source byte cap applies to a cached body exactly as to a fetched
		// one, and a length mismatch means a truncated file.
		let body = bounded(&mut file).ok()?;
		if body.len() as u64 != meta.bytes {
			return None;
		}
		// The modification time is the LRU clock; a failed touch only makes
		// eviction approximate.
		let _ = file.set_modified(SystemTime::now());
		Some((meta, body))
	}

	fn load_body(&self, key: &str) -> Option<Vec<u8>> {
		self.load_entry(key).map(|(_, body)| body)
	}

	fn store(&self, key: &str, meta: &Meta, body: &[u8]) {
		if let Err(error) = self.write(key, meta, body) {
			log::warn!("Image cache write failed: {error}");
		}
		self.evict();
	}

	/// Drops a stored entry; a failed removal only leaves it for eviction.
	fn remove(&self, key: &str) {
		let _ = fs::remove_file(self.path(key));
	}

	fn write(&self, key: &str, meta: &Meta, body: &[u8]) -> Result<()> {
		fs::create_dir_all(&self.root).context("Create image cache")?;
		let header = serde_json::to_vec(meta).context("Encode cache header")?;
		let temp = self.root.join(format!(
			".tmp-{}-{}",
			std::process::id(),
			TEMP.fetch_add(1, Ordering::Relaxed)
		));
		let mut file = fs::File::create(&temp)?;
		let result = (|| -> Result<()> {
			file.write_all(MAGIC)?;
			file.write_all(&(header.len() as u32).to_le_bytes())?;
			file.write_all(&header)?;
			file.write_all(body)?;
			file.flush()?;
			fs::rename(&temp, self.path(key))
				.context("Install image cache entry")?;
			Ok(())
		})();
		if result.is_err() {
			let _ = fs::remove_file(&temp);
		}
		result
	}

	/// Drops least-recently-used entries until the cache is within its bound,
	/// and removes temporary files a crashed writer left behind. This runs
	/// after a store, so a cache hit never pays for it.
	fn evict(&self) {
		let Ok(dir) = fs::read_dir(&self.root) else {
			return;
		};
		let mut entries: Vec<(PathBuf, u64, SystemTime)> = Vec::new();
		let mut total = 0u64;
		let now = SystemTime::now();
		for entry in dir.flatten() {
			let Ok(meta) = entry.metadata() else {
				continue;
			};
			if !meta.is_file() {
				continue;
			}
			let name = entry.file_name();
			let Some(name) = name.to_str() else {
				continue;
			};
			if name.starts_with(".tmp-") {
				let abandoned = meta.modified().is_ok_and(|at| {
					now.duration_since(at).is_ok_and(|age| age > TEMP_AGE)
				});
				if abandoned {
					let _ = fs::remove_file(entry.path());
				}
				continue;
			}
			total += meta.len();
			entries.push((
				entry.path(),
				meta.len(),
				meta.modified().unwrap_or(UNIX_EPOCH),
			));
		}
		if total <= self.limit {
			return;
		}
		entries.sort_by_key(|(_, _, used)| *used);
		for (path, len, _) in entries {
			if total <= self.limit {
				break;
			}
			if fs::remove_file(&path).is_ok() {
				total -= len;
			}
		}
	}

	/// Stores a response as if it had just been fetched, for tests.
	#[cfg(test)]
	pub(super) fn put(&self, url: &str, headers: Headers, body: &[u8]) {
		self.store(
			url,
			&metadata(url, url, &headers, body, SystemTime::now()),
			body,
		);
	}
}

fn open(path: &Path) -> Result<(fs::File, Meta)> {
	let mut file = fs::File::open(path)?;
	let mut magic = [0u8; MAGIC.len()];
	file.read_exact(&mut magic)?;
	if magic != MAGIC {
		bail!("not a cache entry");
	}
	let mut length = [0u8; 4];
	file.read_exact(&mut length)?;
	let length = u32::from_le_bytes(length) as usize;
	if length == 0 || length > MAX_HEADER {
		bail!("invalid cache header");
	}
	let mut header = vec![0u8; length];
	file.read_exact(&mut header)?;
	let meta =
		serde_json::from_slice(&header).context("Decode cache header")?;
	Ok((file, meta))
}

/// Fetches `url` through the cache, revalidating a stale entry and reusing its
/// body on `304`. `offline` never calls `get`: it serves a cached body whether
/// fresh or stale, and otherwise fails the way the reader already does.
///
/// Serving stale offline deliberately overrides `no-cache` and
/// `must-revalidate`: there is no network to revalidate against, and a stored
/// image is exactly what offline reading wants.
pub(super) fn fetch(
	cache: &Cache,
	url: &str,
	offline: bool,
	get: &mut dyn FnMut(Validators) -> Result<Fetched>,
) -> Result<Vec<u8>> {
	if offline {
		return match cache.load_body(url) {
			Some(body) => Ok(body),
			None => bail!("Network images disabled (--offline)"),
		};
	}
	let now = SystemTime::now();
	let stored = cache.load_meta(url);
	if let Some(meta) = &stored
		&& meta.fresh(unix(now))
		&& let Some(body) = cache.load_body(url)
	{
		return Ok(body);
	}
	let validators = stored.as_ref().map(Meta::validators).unwrap_or_default();
	let fetched = get(validators)?;
	// A `304` is only an answer for the resource the stored validators belong
	// to. When the chain now ends at another URL, the response says nothing
	// about this entry, so the new resource is fetched in full.
	let revalidated = fetched.status == 304
		&& stored
			.as_ref()
			.is_some_and(|meta| meta.final_url() == fetched.final_url);
	if fetched.status == 304 && !revalidated {
		let fetched = get(Validators::default())?;
		return Ok(install(cache, url, fetched, now));
	}
	if revalidated {
		// The metadata and the body are read together, so a replacement that
		// landed while the request was in flight cannot pair its bytes with
		// the validators this response answered.
		if let Some((current, body)) = cache.load_entry(url) {
			// A revalidation that forbids storage, varies on external factors,
			// or that resolved through a redirect chain the cache cannot
			// represent, drops the entry instead of retaining it.
			if fetched.headers.no_store
				|| fetched.headers.varies_wildcard()
				|| !fetched.redirects_cacheable
			{
				cache.remove(url);
				return Ok(body);
			}
			// Only the entry the `304` actually answered may be refreshed. If
			// it changed under the request, the result is discarded rather
			// than stored against the wrong bytes.
			if stored.as_ref() == Some(&current) {
				let meta = refreshed(
					&current,
					url,
					&fetched.final_url,
					&fetched.headers,
					&body,
					now,
					fetched.freshness_cap,
				);
				cache.store(url, &meta, &body);
			}
			return Ok(body);
		}
		// The entry vanished between the two reads; fetch it in full rather
		// than fail a document over a revalidation.
		let fetched = get(Validators::default())?;
		return Ok(install(cache, url, fetched, now));
	}
	Ok(install(cache, url, fetched, now))
}

/// Stores a fetched response when it may be stored, and returns its body. A
/// response that must not be stored replaces any stale entry, so it cannot be
/// served from the cache later.
fn install(
	cache: &Cache,
	url: &str,
	fetched: Fetched,
	now: SystemTime,
) -> Vec<u8> {
	if fetched.redirects_cacheable
		&& !fetched.headers.no_store
		&& !fetched.headers.varies_wildcard()
	{
		let mut meta = metadata(
			url,
			&fetched.final_url,
			&fetched.headers,
			&fetched.body,
			now,
		);
		meta.lifetime = capped(
			meta.lifetime,
			meta.date.unwrap_or(0),
			fetched.freshness_cap,
		);
		cache.store(url, &meta, &fetched.body);
	} else {
		cache.remove(url);
	}
	fetched.body
}

/// Fetches a remote image, through `cache` when one is configured and directly
/// otherwise. `offline` never opens a socket.
pub(super) fn fetch_http(
	url: &str,
	offline: bool,
	cache: Option<&Cache>,
) -> Result<Vec<u8>> {
	match cache {
		Some(cache) => fetch(cache, url, offline, &mut |validators| {
			super::net::get(url, &validators)
		}),
		None if offline => bail!("Network images disabled (--offline)"),
		None => Ok(super::net::get(url, &Validators::default())?.body),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn fetched(
		url: &str,
		status: u16,
		headers: Headers,
		body: &[u8],
	) -> Fetched {
		Fetched {
			status,
			headers,
			body: body.to_vec(),
			redirects_cacheable: true,
			// Without redirects the requested URL is the resource.
			final_url: url.to_owned(),
			freshness_cap: None,
		}
	}

	fn hit(etag: Option<&str>, max_age: Option<u64>) -> Headers {
		Headers {
			etag: etag.map(str::to_owned),
			max_age,
			// A `max-age` only ever comes from `Cache-Control`.
			has_cache_control: max_age.is_some(),
			..Default::default()
		}
	}

	/// Stores an immediately stale entry as if it had been fetched through a
	/// redirect from `url` to `final_url`.
	fn put_redirect(
		cache: &Cache,
		url: &str,
		final_url: &str,
		etag: &str,
		body: &[u8],
	) {
		let meta = Meta {
			url: url.into(),
			final_url: final_url.into(),
			etag: Some(etag.into()),
			lifetime: Some(0),
			date: Some(unix(SystemTime::now())),
			bytes: body.len() as u64,
			..Default::default()
		};
		cache.store(url, &meta, body);
	}

	#[test]
	fn fresh_hit_does_not_refetch() {
		let dir = tempfile::tempdir().unwrap();
		let cache = Cache::new(dir.path().to_owned());
		let url = "https://example.com/a.png";
		cache.put(url, hit(Some("\"v1\""), Some(600)), b"stored");
		let mut calls = 0;
		let body = fetch(&cache, url, false, &mut |_| {
			calls += 1;
			Ok(fetched(url, 200, Headers::default(), b"fresh"))
		})
		.unwrap();
		assert_eq!(body, b"stored");
		assert_eq!(calls, 0);
	}

	#[test]
	fn stale_hit_revalidates_and_reuses_on_304() {
		let dir = tempfile::tempdir().unwrap();
		let cache = Cache::new(dir.path().to_owned());
		let url = "https://example.com/a.png";
		// `max-age=0` makes the entry immediately stale.
		cache.put(url, hit(Some("\"v1\""), Some(0)), b"stored");
		let mut seen = Vec::new();
		let body = fetch(&cache, url, false, &mut |validators| {
			seen.push(validators.clone());
			Ok(fetched(url, 304, hit(Some("\"v1\""), Some(600)), b""))
		})
		.unwrap();
		assert_eq!(body, b"stored");
		assert_eq!(seen.len(), 1);
		assert_eq!(seen[0].etag.as_deref(), Some("\"v1\""));
		// The refreshed metadata makes the next read a fresh hit.
		let mut calls = 0;
		let body = fetch(&cache, url, false, &mut |_| {
			calls += 1;
			Ok(fetched(url, 200, Headers::default(), b"changed"))
		})
		.unwrap();
		assert_eq!(body, b"stored");
		assert_eq!(calls, 0);
	}

	#[test]
	fn a_304_without_headers_keeps_the_stored_validator() {
		let dir = tempfile::tempdir().unwrap();
		let cache = Cache::new(dir.path().to_owned());
		let url = "https://example.com/a.png";
		cache.put(url, hit(Some("\"v1\""), Some(0)), b"stored");
		fetch(&cache, url, false, &mut |_| {
			Ok(fetched(url, 304, Headers::default(), b""))
		})
		.unwrap();
		// The stored `ETag` is still offered on the next revalidation.
		let mut seen = None;
		fetch(&cache, url, false, &mut |validators| {
			seen = validators.etag;
			Ok(fetched(url, 304, Headers::default(), b""))
		})
		.unwrap();
		assert_eq!(seen.as_deref(), Some("\"v1\""));
	}

	#[test]
	fn a_header_light_304_remeasures_the_stored_lifetime() {
		let dir = tempfile::tempdir().unwrap();
		let cache = Cache::new(dir.path().to_owned());
		let url = "https://example.com/a.png";
		// Fresh for 600 seconds from a `Date` that is long past.
		let old_date = UNIX_EPOCH + Duration::from_secs(1_000_000);
		let headers = Headers {
			etag: Some("\"v1\"".into()),
			max_age: Some(600),
			date: Some(old_date),
			..Default::default()
		};
		cache.put(url, headers, b"stored");
		assert!(!cache.load_meta(url).unwrap().fresh(unix(SystemTime::now())));
		// The `304` only refreshes `Date`; the stored lifetime must run from
		// that date instead of keeping the expired one.
		let mut calls = 0;
		let body = fetch(&cache, url, false, &mut |_| {
			calls += 1;
			let headers = Headers {
				date: Some(SystemTime::now()),
				..Default::default()
			};
			Ok(fetched(url, 304, headers, b""))
		})
		.unwrap();
		assert_eq!(body, b"stored");
		assert_eq!(calls, 1);
		// The next open is served from disk without a network request.
		let mut calls = 0;
		let body = fetch(&cache, url, false, &mut |_| {
			calls += 1;
			Ok(fetched(url, 200, Headers::default(), b"changed"))
		})
		.unwrap();
		assert_eq!(body, b"stored");
		assert_eq!(calls, 0);
		// The same holds when the stored freshness came from `Expires`.
		let cache = Cache::new(dir.path().to_owned());
		let url = "https://example.com/b.png";
		let headers = Headers {
			etag: Some("\"v2\"".into()),
			date: Some(old_date),
			expires: Some(old_date + Duration::from_secs(600)),
			..Default::default()
		};
		cache.put(url, headers, b"stored");
		assert!(!cache.load_meta(url).unwrap().fresh(unix(SystemTime::now())));
		fetch(&cache, url, false, &mut |_| {
			let headers = Headers {
				date: Some(SystemTime::now()),
				..Default::default()
			};
			Ok(fetched(url, 304, headers, b""))
		})
		.unwrap();
		let mut calls = 0;
		let body = fetch(&cache, url, false, &mut |_| {
			calls += 1;
			Ok(fetched(url, 200, Headers::default(), b"changed"))
		})
		.unwrap();
		assert_eq!(body, b"stored");
		assert_eq!(calls, 0);
	}

	#[test]
	fn a_304_that_says_no_store_drops_the_entry() {
		let dir = tempfile::tempdir().unwrap();
		let cache = Cache::new(dir.path().to_owned());
		let url = "https://example.com/a.png";
		cache.put(url, hit(Some("\"v1\""), Some(0)), b"stored");
		let body = fetch(&cache, url, false, &mut |_| {
			let mut headers = hit(Some("\"v1\""), None);
			headers.no_store = true;
			Ok(fetched(url, 304, headers, b""))
		})
		.unwrap();
		assert_eq!(body, b"stored");
		assert!(!cache.path(url).exists());
		// `--offline` can no longer serve what the server forbade storing.
		let mut calls = 0;
		let error = fetch(&cache, url, true, &mut |_| {
			calls += 1;
			Ok(fetched(url, 200, Headers::default(), b"online"))
		})
		.unwrap_err()
		.to_string();
		assert!(error.contains("--offline"), "{error}");
		assert_eq!(calls, 0);
	}

	#[test]
	fn a_304_that_repeats_cache_control_replaces_the_stored_directives() {
		let dir = tempfile::tempdir().unwrap();
		let cache = Cache::new(dir.path().to_owned());
		let url = "https://example.com/a.png";
		let no_cache = Headers {
			no_cache: true,
			has_cache_control: true,
			..Default::default()
		};
		cache.put(url, no_cache, b"stored");
		let fresh = Headers {
			max_age: Some(600),
			has_cache_control: true,
			..Default::default()
		};
		fetch(&cache, url, false, &mut |_| {
			Ok(fetched(url, 304, fresh.clone(), b""))
		})
		.unwrap();
		// The repeated `Cache-Control` makes the entry fresh again.
		let mut calls = 0;
		let body = fetch(&cache, url, false, &mut |_| {
			calls += 1;
			Ok(fetched(url, 200, Headers::default(), b"changed"))
		})
		.unwrap();
		assert_eq!(body, b"stored");
		assert_eq!(calls, 0);
	}

	#[test]
	fn changed_etag_refetches() {
		let dir = tempfile::tempdir().unwrap();
		let cache = Cache::new(dir.path().to_owned());
		let url = "https://example.com/a.png";
		cache.put(url, hit(Some("\"v1\""), Some(0)), b"old");
		let mut calls = 0;
		let body = fetch(&cache, url, false, &mut |validators| {
			calls += 1;
			assert_eq!(validators.etag.as_deref(), Some("\"v1\""));
			Ok(fetched(url, 200, hit(Some("\"v2\""), Some(0)), b"new"))
		})
		.unwrap();
		assert_eq!(body, b"new");
		assert_eq!(calls, 1);
		// The replacement carries the new validator into the next revalidation.
		let mut seen = None;
		fetch(&cache, url, false, &mut |validators| {
			seen = validators.etag;
			Ok(fetched(url, 304, hit(Some("\"v2\""), Some(600)), b""))
		})
		.unwrap();
		assert_eq!(seen.as_deref(), Some("\"v2\""));
	}

	#[test]
	fn no_cache_headers_are_revalidated_every_time() {
		let dir = tempfile::tempdir().unwrap();
		let cache = Cache::new(dir.path().to_owned());
		let url = "https://example.com/a.png";
		let headers = Headers {
			etag: Some("\"v1\"".into()),
			max_age: Some(600),
			no_cache: true,
			..Default::default()
		};
		cache.put(url, headers, b"stored");
		let mut calls = 0;
		fetch(&cache, url, false, &mut |_| {
			calls += 1;
			Ok(fetched(url, 304, Headers::default(), b""))
		})
		.unwrap();
		assert_eq!(calls, 1);
	}

	#[test]
	fn no_store_is_never_written() {
		let dir = tempfile::tempdir().unwrap();
		let cache = Cache::new(dir.path().to_owned());
		let url = "https://example.com/a.png";
		let headers = Headers {
			no_store: true,
			..Default::default()
		};
		let body = fetch(&cache, url, false, &mut |_| {
			Ok(fetched(url, 200, headers.clone(), b"secret"))
		})
		.unwrap();
		assert_eq!(body, b"secret");
		assert!(!cache.path(url).exists());
		assert!(fs::read_dir(dir.path()).unwrap().next().is_none());
	}

	#[test]
	fn every_cache_control_field_is_interpreted() {
		use reqwest::header::{CACHE_CONTROL, HeaderMap, HeaderValue};
		let mut map = HeaderMap::new();
		map.append(CACHE_CONTROL, HeaderValue::from_static("max-age=600"));
		map.append(CACHE_CONTROL, HeaderValue::from_static("no-store"));
		let headers = super::super::net::headers(&map);
		assert_eq!(headers.max_age, Some(600));
		assert!(headers.no_store, "a later field still forbids storage");
		// A response that both grants a lifetime and forbids storage is never
		// written, even though the first field alone would have allowed it.
		let dir = tempfile::tempdir().unwrap();
		let cache = Cache::new(dir.path().to_owned());
		let url = "https://example.com/a.png";
		let body = fetch(&cache, url, false, &mut |_| {
			Ok(fetched(url, 200, headers.clone(), b"secret"))
		})
		.unwrap();
		assert_eq!(body, b"secret");
		assert!(!cache.path(url).exists());
	}

	#[test]
	fn an_uncacheable_redirect_is_not_stored_under_the_original_url() {
		let dir = tempfile::tempdir().unwrap();
		let cache = Cache::new(dir.path().to_owned());
		let url = "https://example.com/latest.png";
		// A stale entry from an earlier open must not survive either.
		cache.put(url, hit(None, Some(0)), b"old");
		let mut calls = 0;
		let body = fetch(&cache, url, false, &mut |_| {
			calls += 1;
			let mut response =
				fetched(url, 200, hit(None, Some(600)), b"final");
			// The redirect hop forbids caching, so the final body cannot stand
			// in for the original URL.
			response.redirects_cacheable = false;
			Ok(response)
		})
		.unwrap();
		assert_eq!(body, b"final");
		assert_eq!(calls, 1);
		assert!(!cache.path(url).exists());
		// Nothing is stored, so the next open resolves the redirect again.
		let body = fetch(&cache, url, false, &mut |_| {
			calls += 1;
			let mut response =
				fetched(url, 200, hit(None, Some(600)), b"final");
			response.redirects_cacheable = false;
			Ok(response)
		})
		.unwrap();
		assert_eq!(body, b"final");
		assert_eq!(calls, 2);
	}

	#[test]
	fn a_redirect_hop_with_a_shorter_lifetime_caps_the_stored_freshness() {
		let dir = tempfile::tempdir().unwrap();
		let cache = Cache::new(dir.path().to_owned());
		let url = "https://example.com/latest.png";
		let date = UNIX_EPOCH + Duration::from_secs(1_000_000);
		let headers = Headers {
			etag: Some("\"v1\"".into()),
			max_age: Some(600),
			has_cache_control: true,
			date: Some(date),
			..Default::default()
		};
		let body = fetch(&cache, url, false, &mut |_| {
			let mut response = fetched(url, 200, headers.clone(), b"final");
			response.final_url = "https://cdn.example.com/a.png".into();
			// The shortest hop of the chain stays fresh for only five seconds,
			// so the final response's ten-minute lifetime must not be stored.
			response.freshness_cap = Some(date + Duration::from_secs(5));
			Ok(response)
		})
		.unwrap();
		assert_eq!(body, b"final");
		let meta = cache.load_meta(url).unwrap();
		assert_eq!(meta.lifetime, Some(5));
		// The stored lifetime runs from the response `Date`.
		let date = meta.date.unwrap();
		assert!(meta.fresh(date + 4));
		assert!(!meta.fresh(date + 5));
	}

	#[test]
	fn a_redirect_hop_dated_earlier_caps_the_stored_expiry() {
		let dir = tempfile::tempdir().unwrap();
		let cache = Cache::new(dir.path().to_owned());
		let url = "https://example.com/latest.png";
		// The redirect hop was dated fifty seconds ago with `max-age=60`, so
		// it expires in ten seconds from now. The final response is dated now
		// with `max-age=600`; carrying the hop's lifetime onto that date would
		// keep the entry fresh for another minute instead of ten seconds.
		let hop_date = UNIX_EPOCH + Duration::from_secs(1_000_000);
		let date = hop_date + Duration::from_secs(50);
		let headers = Headers {
			etag: Some("\"v1\"".into()),
			max_age: Some(600),
			has_cache_control: true,
			date: Some(date),
			..Default::default()
		};
		let body = fetch(&cache, url, false, &mut |_| {
			let mut response = fetched(url, 200, headers.clone(), b"final");
			response.final_url = "https://cdn.example.com/a.png".into();
			// `net` folds the hop's `Date + lifetime` into an absolute cap.
			response.freshness_cap = Some(hop_date + Duration::from_secs(60));
			Ok(response)
		})
		.unwrap();
		assert_eq!(body, b"final");
		let meta = cache.load_meta(url).unwrap();
		let expiry = unix(hop_date + Duration::from_secs(60));
		assert_eq!(meta.expires_at(), Some(expiry));
		assert!(meta.fresh(expiry - 1));
		assert!(!meta.fresh(expiry));
	}

	#[test]
	fn a_wildcard_vary_response_is_never_stored() {
		use reqwest::header::{CACHE_CONTROL, HeaderMap, HeaderValue, VARY};
		let mut map = HeaderMap::new();
		map.append(CACHE_CONTROL, HeaderValue::from_static("max-age=600"));
		map.append(VARY, HeaderValue::from_static("*"));
		let headers = super::super::net::headers(&map);
		assert!(headers.varies_wildcard());
		let dir = tempfile::tempdir().unwrap();
		let cache = Cache::new(dir.path().to_owned());
		let url = "https://example.com/a.png";
		// A stale entry from an earlier open must not survive either.
		cache.put(url, hit(None, Some(0)), b"old");
		let mut calls = 0;
		let body = fetch(&cache, url, false, &mut |_| {
			calls += 1;
			Ok(fetched(url, 200, headers.clone(), b"fresh"))
		})
		.unwrap();
		assert_eq!(body, b"fresh");
		assert_eq!(calls, 1);
		assert!(!cache.path(url).exists());
		// Nothing was stored, so the next open must go online instead of
		// serving the response from disk.
		let body = fetch(&cache, url, false, &mut |_| {
			calls += 1;
			Ok(fetched(url, 200, headers.clone(), b"fresh"))
		})
		.unwrap();
		assert_eq!(body, b"fresh");
		assert_eq!(calls, 2);
	}

	#[test]
	fn a_304_for_a_changed_redirect_target_does_not_serve_the_old_body() {
		let dir = tempfile::tempdir().unwrap();
		let cache = Cache::new(dir.path().to_owned());
		let url = "https://example.com/latest.png";
		let first = "https://cdn.example.com/a.png";
		let second = "https://cdn.example.com/b.png";
		put_redirect(&cache, url, first, "\"v1\"", b"old");
		let mut calls = 0;
		let body = fetch(&cache, url, false, &mut |validators| {
			calls += 1;
			if calls == 1 {
				// The stored validators are offered for the old target only.
				assert_eq!(validators.url.as_deref(), Some(first));
				// The chain now ends at another resource that happens to use
				// the same `ETag`, so a `304` is not an answer for this entry.
				let mut response =
					fetched(url, 304, hit(Some("\"v1\""), Some(600)), b"");
				response.final_url = second.into();
				Ok(response)
			} else {
				// The resource is fetched in full instead of reusing A's body.
				assert!(validators.url.is_none() && validators.etag.is_none());
				let mut response =
					fetched(url, 200, hit(Some("\"v1\""), Some(600)), b"new");
				response.final_url = second.into();
				Ok(response)
			}
		})
		.unwrap();
		assert_eq!(body, b"new");
		assert_eq!(calls, 2);
		let meta = cache.load_meta(url).unwrap();
		assert_eq!(meta.final_url, second);
		assert_eq!(meta.etag.as_deref(), Some("\"v1\""));
		assert_eq!(cache.load_body(url).unwrap(), b"new");
	}

	#[test]
	fn a_replacement_during_a_conditional_request_is_not_refreshed() {
		let dir = tempfile::tempdir().unwrap();
		let cache = Cache::new(dir.path().to_owned());
		let url = "https://example.com/a.png";
		cache.put(url, hit(Some("\"v1\""), Some(0)), b"old");
		let body = fetch(&cache, url, false, &mut |_| {
			// Another reader replaces the entry while the request is in
			// flight; the `304` still answers the entry that was opened.
			cache.put(url, hit(Some("\"v2\""), Some(600)), b"newer");
			Ok(fetched(url, 304, hit(Some("\"v1\""), Some(600)), b""))
		})
		.unwrap();
		// The replacement keeps its own metadata instead of being paired with
		// the validators and freshness the `304` answered with.
		assert_eq!(body, b"newer");
		let meta = cache.load_meta(url).unwrap();
		assert_eq!(meta.etag.as_deref(), Some("\"v2\""));
		assert_eq!(meta.lifetime, Some(600));
		assert_eq!(meta.bytes, 5);
		assert_eq!(cache.load_body(url).unwrap(), b"newer");
	}

	#[test]
	fn lru_eviction_respects_the_byte_bound() {
		let dir = tempfile::tempdir().unwrap();
		let cache = Cache::new(dir.path().to_owned());
		let base = UNIX_EPOCH + Duration::from_secs(1_000_000);
		let store = |url: &str, at: SystemTime| {
			cache.put(url, Headers::default(), b"aaaa");
			fs::File::open(cache.path(url))
				.unwrap()
				.set_modified(at)
				.unwrap();
		};
		store("https://example.com/a", base);
		store("https://example.com/b", base + Duration::from_secs(1));
		store("https://example.com/c", base + Duration::from_secs(2));
		store("https://example.com/d", base + Duration::from_secs(3));
		let size = |url: &str| fs::metadata(cache.path(url)).unwrap().len();
		// A bound that exactly fits the two newest entries drops the two least
		// recently used.
		let limit =
			size("https://example.com/c") + size("https://example.com/d");
		Cache::with_limit(dir.path().to_owned(), limit).evict();
		let total: u64 = fs::read_dir(dir.path())
			.unwrap()
			.flatten()
			.map(|entry| entry.metadata().unwrap().len())
			.sum();
		assert!(total <= limit, "{total} bytes");
		assert!(!cache.path("https://example.com/a").exists());
		assert!(!cache.path("https://example.com/b").exists());
		assert!(cache.path("https://example.com/c").exists());
		assert!(cache.path("https://example.com/d").exists());
	}

	#[test]
	fn partial_or_truncated_entry_is_never_served() {
		let dir = tempfile::tempdir().unwrap();
		let cache = Cache::new(dir.path().to_owned());
		let url = "https://example.com/a.png";
		cache.put(url, Headers::default(), b"complete body");
		// Truncate the body without touching the header.
		let path = cache.path(url);
		let full = fs::read(&path).unwrap();
		fs::write(&path, &full[..full.len() - 4]).unwrap();
		assert!(cache.load_body(url).is_none());
		let mut calls = 0;
		let body = fetch(&cache, url, false, &mut |_| {
			calls += 1;
			Ok(fetched(url, 200, Headers::default(), b"fresh body"))
		})
		.unwrap();
		assert_eq!(body, b"fresh body");
		assert_eq!(calls, 1);
	}

	#[test]
	fn a_colliding_key_does_not_serve_another_url() {
		let dir = tempfile::tempdir().unwrap();
		let cache = Cache::new(dir.path().to_owned());
		cache.put("https://example.com/a", Headers::default(), b"a body");
		let path = cache.path("https://example.com/a");
		// Point the same file at another key by rewriting only the header.
		let meta = Meta {
			url: "https://example.com/b".into(),
			bytes: 6,
			..Default::default()
		};
		let json = serde_json::to_vec(&meta).unwrap();
		let mut bytes = Vec::new();
		bytes.extend_from_slice(MAGIC);
		bytes.extend_from_slice(&(json.len() as u32).to_le_bytes());
		bytes.extend_from_slice(&json);
		bytes.extend_from_slice(b"a body");
		fs::write(&path, &bytes).unwrap();
		assert!(cache.load_meta("https://example.com/a").is_none());
		assert!(cache.load_body("https://example.com/a").is_none());
	}

	#[test]
	fn offline_serves_stale_and_fails_without_an_entry() {
		let dir = tempfile::tempdir().unwrap();
		let cache = Cache::new(dir.path().to_owned());
		let url = "https://example.com/a.png";
		cache.put(url, hit(None, Some(0)), b"stale body");
		let mut calls = 0;
		let body = fetch(&cache, url, true, &mut |_| {
			calls += 1;
			Ok(fetched(url, 200, Headers::default(), b"online"))
		})
		.unwrap();
		assert_eq!(body, b"stale body");
		assert_eq!(calls, 0, "offline must not touch the network");
		let error =
			fetch(&cache, "https://example.com/missing", true, &mut |_| {
				calls += 1;
				Ok(fetched(url, 200, Headers::default(), b"online"))
			})
			.unwrap_err()
			.to_string();
		assert!(error.contains("--offline"), "{error}");
		assert_eq!(calls, 0);
	}

	#[test]
	fn expires_and_date_decide_freshness() {
		let now = UNIX_EPOCH + Duration::from_secs(100_000);
		let headers = Headers {
			date: Some(UNIX_EPOCH + Duration::from_secs(99_000)),
			max_age: Some(600),
			..Default::default()
		};
		let meta = metadata(
			"https://example.com/a",
			"https://example.com/a",
			&headers,
			b"body",
			now,
		);
		assert_eq!(meta.bytes, 4);
		// Fresh until the response `Date` plus `max-age`.
		assert!(meta.fresh(99_500));
		assert!(!meta.fresh(99_600));
		let headers = Headers {
			expires: Some(UNIX_EPOCH + Duration::from_secs(200_000)),
			..Default::default()
		};
		let meta = metadata(
			"https://example.com/a",
			"https://example.com/a",
			&headers,
			b"body",
			now,
		);
		assert!(meta.fresh(100_000));
		assert!(!meta.fresh(200_000));
		// No expiry at all: immediately stale, as before the cache.
		let meta = metadata(
			"https://example.com/a",
			"https://example.com/a",
			&Headers::default(),
			b"body",
			now,
		);
		assert!(!meta.fresh(unix(now)));
	}
}
