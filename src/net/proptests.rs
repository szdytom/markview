use super::*;
use proptest::prelude::*;
use reqwest::header::{CACHE_CONTROL, IF_MODIFIED_SINCE, IF_NONE_MATCH, VARY};

fn month_lengths(year: u16) -> [u8; 12] {
	let leap = year.is_multiple_of(4)
		&& (!year.is_multiple_of(100) || year.is_multiple_of(400));
	[
		31,
		28 + u8::from(leap),
		31,
		30,
		31,
		30,
		31,
		31,
		30,
		31,
		30,
		31,
	]
}

fn calendar_date(
	year: u16,
	month: usize,
	day: u8,
	clock: [u8; 3],
) -> (String, u64) {
	let days: u64 = (1970..year)
		.map(|year| month_lengths(year).into_iter().map(u64::from).sum::<u64>())
		.sum::<u64>()
		+ month_lengths(year)[..month]
			.iter()
			.copied()
			.map(u64::from)
			.sum::<u64>()
		+ u64::from(day - 1);
	let weekday =
		["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][days as usize % 7];
	let month = [
		"Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct",
		"Nov", "Dec",
	][month];
	let [hour, minute, second] = clock;
	(
		format!(
			"{weekday}, {day:02} {month} {year:04} {hour:02}:{minute:02}:{second:02} GMT"
		),
		days * 86_400
			+ u64::from(hour) * 3600
			+ u64::from(minute) * 60
			+ u64::from(second),
	)
}

proptest! {
	#[test]
	fn repeated_headers_preserve_cache_flags_and_wildcard_vary(
		age in prop::option::of(any::<u32>()), quoted in any::<bool>(), upper in any::<bool>(),
		directives in prop::collection::vec(0u8..4, 0..12), fields in prop::collection::vec(0u8..3, 0..8),
	) {
		let mut map = HeaderMap::new();
		if let Some(age) = age {
			let name = if upper { "MAX-AGE" } else { "max-age" };
			let value = if quoted { format!("{name} = \"{age}\"") } else { format!("{name}={age}") };
			map.append(CACHE_CONTROL, value.parse().unwrap());
		}
		for &directive in &directives {
			let value = ["no-store", "no-cache", "private", "unknown=7"][usize::from(directive)];
			let value = if upper { value.to_ascii_uppercase() } else { value.to_owned() };
			map.append(CACHE_CONTROL, format!(" {value} ").parse().unwrap());
		}
		for &field in &fields {
			map.append(VARY, ["Accept", "*", "Accept-Language"][usize::from(field)].parse().unwrap());
		}
		let parsed = headers(&map);
		prop_assert_eq!(parsed.max_age, age.map(u64::from));
		prop_assert_eq!(parsed.no_store, directives.contains(&0));
		prop_assert_eq!(parsed.no_cache, directives.contains(&1));
		prop_assert_eq!(parsed.has_cache_control, age.is_some() || !directives.is_empty());
		prop_assert_eq!(parsed.varies_wildcard(), fields.contains(&1));
		prop_assert_eq!(parsed.vary.is_some(), !fields.is_empty());
	}

	#[test]
	fn calendar_dates_match_an_enumerated_gregorian_calendar(
		year in 1970u16..2401, month in 0usize..12, day in any::<u8>(),
		hour in 0u8..24, minute in 0u8..60, second in 0u8..60,
	) {
		let day = 1 + day % month_lengths(year)[month];
		let (date, seconds) = calendar_date(year, month, day, [hour, minute, second]);
		let expected = Some(UNIX_EPOCH + Duration::from_secs(seconds));
		prop_assert_eq!(http_date(&date), expected);
		let mut map = HeaderMap::new();
		map.insert(reqwest::header::DATE, date.parse().unwrap());
		map.insert(reqwest::header::EXPIRES, calendar_date(year, month, day, [23, 59, 59]).0.parse().unwrap());
		let parsed = headers(&map);
		prop_assert_eq!(parsed.date, expected);
		prop_assert_eq!(parsed.lifetime(UNIX_EPOCH), Some(86_399 - seconds % 86_400));
	}

	#[test]
	fn redirect_constraints_intersect_storage_and_absolute_expiries(
		now in 1000u32..200_000,
		hops in prop::collection::vec((prop::option::of(0u32..200_000), prop::option::of(0u32..500), prop::option::of(0u32..200_000), any::<bool>(), any::<bool>(), any::<bool>()), 0..12),
	) {
		let mut constraints = Vec::new();
		let mut cacheable = true;
		let mut expiries = Vec::new();
		let mut chain = Chain::default();
		for &(date, age, expires, no_store, no_cache, wildcard) in &hops {
			let reference = date.unwrap_or(now);
			let lifetime = age.or_else(|| expires.and_then(|expiry| expiry.checked_sub(reference)));
			let expiry = age.map(|age| reference + age).or(expires);
			cacheable &= !no_store && !no_cache && !wildcard && lifetime.is_some_and(|life| life > 0);
			expiries.extend(expiry);
			let hop = Headers {
				date: date.map(|date| UNIX_EPOCH + Duration::from_secs(u64::from(date))),
				max_age: age.map(u64::from),
				expires: expires.map(|expires| UNIX_EPOCH + Duration::from_secs(u64::from(expires))),
				no_store, no_cache, vary: wildcard.then(|| "Accept, *".to_owned()), ..Default::default()
			};
			chain.note(&hop, UNIX_EPOCH + Duration::from_secs(u64::from(now)));
			constraints.push(hop);
			prop_assert_eq!(chain.cacheable, cacheable);
			prop_assert_eq!(chain.expires_at, expiries.iter().min().map(|&at| UNIX_EPOCH + Duration::from_secs(u64::from(at))));
		}
		let mut reversed = Chain::default();
		for hop in constraints.iter().rev() { reversed.note(hop, UNIX_EPOCH + Duration::from_secs(u64::from(now))); }
		prop_assert_eq!(reversed.cacheable, chain.cacheable);
		prop_assert_eq!(reversed.expires_at, chain.expires_at);
	}

	#[test]
	fn conditional_requests_scope_each_validator_to_its_recorded_url(
		path in "[a-z]{1,16}", etag in prop::option::of("[a-z0-9]{1,12}"), modified in prop::option::of("[A-Za-z0-9 ]{1,20}"),
	) {
		static CLIENT: std::sync::LazyLock<reqwest::Client> = std::sync::LazyLock::new(reqwest::Client::new);
		let target = format!("https://a.test/{path}");
		let validators = Validators { etag: etag.clone(), last_modified: modified.clone(), url: Some(target.clone()) };
		for url in [&target, &format!("https://b.test/{path}"), &format!("https://a.test/{path}/other")] {
			let request = conditional_request(CLIENT.get(url), url, &validators).build().unwrap();
			let expected_etag = (url == &target).then_some(etag.as_deref()).flatten();
			let expected_modified = (url == &target).then_some(modified.as_deref()).flatten();
			prop_assert_eq!(request.headers().get(IF_NONE_MATCH).map(|value| value.to_str().unwrap()), expected_etag);
			prop_assert_eq!(request.headers().get(IF_MODIFIED_SINCE).map(|value| value.to_str().unwrap()), expected_modified);
		}
	}

	#[test]
	fn bounded_reads_return_exact_bodies_or_a_named_size_error(
		body in prop::collection::vec(any::<u8>(), 0..257), cap in 0u16..258,
	) {
		let result = bounded_to(body.as_slice(), u64::from(cap), "Property resource");
		if body.len() <= usize::from(cap) { prop_assert_eq!(result.unwrap(), body); }
		else { prop_assert!(result.unwrap_err().to_string().contains("Property resource")); }
	}

	#[test]
	fn nonexistent_calendar_dates_are_rejected(year in 1970u16..2401, short_month in 0usize..5) {
		let month = [1, 3, 5, 8, 10][short_month];
		for invalid_day in month_lengths(year)[month] + 1..=31 {
			let (date, _) = calendar_date(year, month, invalid_day, [0, 0, 0]);
			prop_assert_eq!(http_date(&date), None, "{}", date);
			let mut map = HeaderMap::new();
			map.insert(reqwest::header::DATE, date.parse().unwrap());
			map.insert(reqwest::header::EXPIRES, date.parse().unwrap());
			let parsed = headers(&map);
			prop_assert_eq!(parsed.date, None);
			prop_assert_eq!(parsed.expires, None);
		}
	}
}
