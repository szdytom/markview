use super::*;
use proptest::prelude::*;

proptest! {
	#[test]
	fn metadata_freshness_and_redirect_caps_use_explicit_response_times(
		date in 0u64..200_000, now in 0u64..200_000, expires in 0u64..300_000,
		max_age in prop::option::of(0u64..100_000), has_date in any::<bool>(), no_cache in any::<bool>(),
		body in prop::collection::vec(any::<u8>(), 0..64), cap in 0u64..300_000,
	) {
		let headers = Headers { date: has_date.then(|| UNIX_EPOCH + Duration::from_secs(date)), expires: Some(UNIX_EPOCH + Duration::from_secs(expires)), max_age, no_cache, etag: Some("etag".to_owned()), ..Default::default() };
		let meta = metadata("https://a.test/image", "https://b.test/final", &headers, &body, UNIX_EPOCH + Duration::from_secs(now));
		let reference = if has_date { date } else { now };
		let lifetime = max_age.or_else(|| expires.checked_sub(reference));
		prop_assert_eq!(meta.lifetime, lifetime);
		prop_assert_eq!(meta.date, Some(reference));
		prop_assert_eq!(meta.bytes, body.len() as u64);
		let validators = meta.validators();
		prop_assert_eq!(validators.url.as_deref(), Some("https://b.test/final"));
		let expiry = lifetime.map(|lifetime| reference + lifetime);
		for query in [0, now, reference, expiry.unwrap_or(0).saturating_sub(1), expiry.unwrap_or(0), 400_000] {
			prop_assert_eq!(meta.fresh(query), !no_cache && expiry.is_some_and(|expiry| query < expiry));
		}
		let capped_lifetime = capped(lifetime, reference, Some(UNIX_EPOCH + Duration::from_secs(cap)));
		if let Some(lifetime) = lifetime {
			let candidate = capped_lifetime.unwrap();
			prop_assert!(candidate <= lifetime);
			prop_assert!(reference + candidate <= reference.max(cap));
			prop_assert!(candidate == lifetime || reference + candidate == reference.max(cap));
		} else { prop_assert!(capped_lifetime.is_none()); }
	}

	#[test]
	fn disk_entries_keep_exact_bytes_and_reject_truncation_and_security_mismatches(
		body in prop::collection::vec(any::<u8>(), 0..256), remove in any::<usize>(), key in "[a-z]{1,12}",
	) {
		let directory = tempfile::tempdir().unwrap();
		let cache = Cache::new(directory.path().to_owned());
		let url = format!("https://a.test/{key}");
		let meta = Meta { url: url.clone(), final_url: "https://b.test/final".to_owned(), bytes: body.len() as u64, ..Default::default() };
		cache.store(&url, &meta, &body);
		prop_assert_eq!(cache.load_entry(&url), Some((meta.clone(), body.clone())));
		let isolated = Cache::new(directory.path().to_owned()).with_security(&crate::security::Security::default());
		prop_assert!(isolated.load_body(&url).is_none());
		let path = cache.path(&url);
		let data = fs::read(&path).unwrap();
		let removed = 1 + remove % body.len().max(1);
		fs::write(&path, &data[..data.len() - removed]).unwrap();
		prop_assert!(cache.load_body(&url).is_none());
		cache.store(&url, &meta, &body);
		prop_assert_eq!(cache.load_body(&url), Some(body));
		cache.remove(&url);
		prop_assert!(cache.load_meta(&url).is_none());
		prop_assert!(cache.load_body(&url).is_none());
	}

	#[test]
	fn revalidation_without_expires_preserves_or_replaces_stored_cache_control(
		old_age in prop::option::of(0u64..1000), old_no_cache in any::<bool>(),
		updates in prop::collection::vec((0u64..100_000, prop::option::of(0u64..1000), any::<bool>(), any::<bool>(), any::<bool>(), any::<bool>(), prop::option::of(0u64..101_000)), 0..12),
	) {
		let mut current = Meta { url: "https://a.test/image".to_owned(), final_url: "https://b.test/image".to_owned(), etag: Some("initial".to_owned()), last_modified: Some("initial-date".to_owned()), lifetime: old_age, no_cache: old_no_cache, bytes: 4, date: Some(0), ..Default::default() };
		let mut model = current.clone();
		for (step, &(date, age, control, no_cache, etag, modified, cap)) in updates.iter().enumerate() {
			let headers = Headers { date: Some(UNIX_EPOCH + Duration::from_secs(date)), max_age: control.then_some(age).flatten(), has_cache_control: control, no_cache: control && no_cache, etag: etag.then(|| format!("etag-{step}")), last_modified: modified.then(|| format!("modified-{step}")), ..Default::default() };
			if etag { model.etag = Some(format!("etag-{step}")); }
			if modified { model.last_modified = Some(format!("modified-{step}")); }
			if control { model.no_cache = no_cache; model.lifetime = age; }
			if let (Some(lifetime), Some(cap)) = (&mut model.lifetime, cap) { *lifetime = (*lifetime).min(cap.saturating_sub(date)); }
			model.date = Some(date);
			current = refreshed(&current, "https://a.test/image", "https://b.test/image", &headers, b"body", UNIX_EPOCH, cap.map(|at| UNIX_EPOCH + Duration::from_secs(at)));
			prop_assert_eq!(&current, &model);
			for query in [date, date + 1000] {
				prop_assert_eq!(current.fresh(query), !model.no_cache && model.lifetime.is_some_and(|life| query < date + life));
			}
		}
	}

	#[test]
	#[ignore = "Known bug PBT-009: an Expires-only 304 overrides a retained max-age"]
	fn a_304_expires_update_preserves_the_precedence_of_a_stored_max_age(age in 1u64..301, extra in 1u64..301) {
		let old_headers = Headers { max_age: Some(age), has_cache_control: true, date: Some(UNIX_EPOCH), ..Default::default() };
		let old = metadata("https://a.test/image", "https://a.test/image", &old_headers, b"body", UNIX_EPOCH);
		let date = UNIX_EPOCH + Duration::from_secs(1000);
		let update = Headers { date: Some(date), expires: Some(date + Duration::from_secs(age + extra)), ..Default::default() };
		let refreshed = refreshed(&old, "https://a.test/image", "https://a.test/image", &update, b"body", date, None);
		prop_assert_eq!(refreshed.lifetime, Some(age));
		prop_assert!(!refreshed.fresh(1000 + age));
	}

	#[test]
	fn cached_fetch_sequences_model_offline_revalidation_and_replacement(
		operations in prop::collection::vec(0u8..9, 0..10), seed in prop::collection::vec(any::<u8>(), 0..32),
	) {
		let directory = tempfile::tempdir().unwrap();
		let cache = Cache::new(directory.path().to_owned());
		let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
		let url = "https://a.test/image";
		let mut model: Option<(Vec<u8>, String, String)> = None;
		for (step, &operation) in operations.iter().enumerate() {
			let offline = operation == 0;
			let revalidate = [2, 3, 4, 8].contains(&operation);
			let old = model.clone();
			let final_url = if operation == 3 { format!("https://changed.test/{step}") } else { old.as_ref().map_or_else(|| url.to_owned(), |(_, target, _)| target.clone()) };
			let etag = format!("etag-{step}");
			let mut replacement = seed.clone();
			replacement.extend([step as u8, operation]);
			let mut requests = Vec::new();
			let result = runtime.block_on(fetch_cached(&cache, url, offline, |validators| {
				requests.push(validators);
				let first = requests.len() == 1;
				if operation == 4 && first { cache.remove(url); }
				std::future::ready(Ok(Fetched {
					status: if revalidate && first { 304 } else { 200 },
					headers: Headers { no_cache: true, has_cache_control: true, etag: Some(etag.clone()), no_store: [5, 8].contains(&operation), vary: (operation == 6).then(|| "*".to_owned()), ..Default::default() },
					body: replacement.clone(), final_url: final_url.clone(), redirects_cacheable: operation != 7, freshness_cap: None,
				}))
			}));
			if offline {
				prop_assert!(requests.is_empty());
				match &old { Some((body, _, _)) => prop_assert_eq!(result.unwrap(), body.clone()), None => prop_assert!(result.is_err()) }
			} else {
				let reused = revalidate && old.is_some() && ![3, 4].contains(&operation);
				let expected_body = if reused { old.as_ref().unwrap().0.clone() } else { replacement };
				prop_assert_eq!(result.unwrap(), expected_body.clone());
				let expected_validators = old.as_ref().map(|(_, target, etag)| Validators { url: Some(target.clone()), etag: Some(etag.clone()), last_modified: None }).unwrap_or_default();
				prop_assert_eq!(&requests[0], &expected_validators);
				prop_assert_eq!(requests.len(), if revalidate && !reused { 2 } else { 1 });
				if requests.len() == 2 { prop_assert_eq!(&requests[1], &Validators::default()); }
				model = if [5, 6, 7, 8].contains(&operation) { None } else { Some((expected_body, final_url, etag)) };
			}
			prop_assert_eq!(cache.load_body(url), model.as_ref().map(|(body, _, _)| body.clone()));
			prop_assert_eq!(cache.load_meta(url).map(|meta| (meta.final_url, meta.etag)), model.as_ref().map(|(_, target, etag)| (target.clone(), Some(etag.clone()))));
		}
	}

	#[test]
	fn eviction_removes_the_oldest_prefix_until_the_byte_budget_is_met(
		lengths in prop::collection::vec(0usize..129, 0..7), raw_budget in any::<u16>(), touch in prop::option::of(any::<usize>()),
	) {
		let directory = tempfile::tempdir().unwrap();
		let cache = Cache::new(directory.path().to_owned());
		let mut entries = Vec::new();
		for (index, &length) in lengths.iter().enumerate() {
			let url = format!("https://a.test/{index}");
			cache.write(&url, &Meta { url: url.clone(), bytes: length as u64, ..Default::default() }, &vec![index as u8; length]).unwrap();
			let file = fs::OpenOptions::new().read(true).write(true).open(cache.path(&url)).unwrap();
			let touched = touch.is_some_and(|touch| index == touch % lengths.len());
			let stamp = if touched { 10_000 } else { 100 + index as u64 };
			file.set_modified(UNIX_EPOCH + Duration::from_secs(stamp)).unwrap();
			entries.push((stamp, url, file.metadata().unwrap().len()));
		}
		entries.sort_by_key(|&(stamp, _, _)| stamp);
		let total: u64 = entries.iter().map(|(_, _, length)| length).sum();
		let limit = u64::from(raw_budget) % (total + 2);
		let mut retained = total;
		let mut removed = 0;
		while retained > limit { retained -= entries[removed].2; removed += 1; }
		Cache::with_limit(directory.path().to_owned(), limit).evict();
		for (index, (_, url, _)) in entries.iter().enumerate() {
			prop_assert_eq!(cache.path(url).exists(), index >= removed);
		}
		let remaining: u64 = fs::read_dir(directory.path()).unwrap().map(|entry| entry.unwrap().metadata().unwrap().len()).sum();
		prop_assert_eq!(remaining, retained);
		prop_assert!(remaining <= limit);
	}
}
