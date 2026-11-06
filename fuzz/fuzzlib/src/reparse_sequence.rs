//! A versioned Markdown edit sequence compared with a full parse at each step.
use crate::{coverage, oracle};
use std::sync::Arc;

#[derive(Debug)]
struct Record {
	start: usize,
	delete: usize,
	insert: usize,
}

fn word(bytes: &[u8], at: usize) -> Option<u32> {
	Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// `RPS1`, count, three lengths, then fixed records, the insert pool, and the
/// initial body. Removing a record while adjusting count and lengths shrinks a
/// failing sequence without rewriting its text.
pub fn check(data: &[u8]) -> bool {
	if !data.starts_with(b"RPS1") || data.len() < 13 {
		return false;
	}
	let count = usize::from(data[4]).min(16);
	let body_len = word(data, 5).unwrap_or(0) as usize;
	let pool_len = word(data, 9).unwrap_or(0) as usize;
	let records_at = 13;
	let pool_at = records_at + count * 9;
	let body_at = pool_at.saturating_add(pool_len);
	let Some(body) = data.get(body_at..body_at.saturating_add(body_len)) else {
		return false;
	};
	let Some(pool) = data.get(pool_at..body_at) else {
		return false;
	};
	if body.len() > 64 * 1024 || pool.len() > 64 * 1024 {
		return false;
	}
	let mut records = Vec::with_capacity(count);
	for index in 0..count {
		let at = records_at + index * 9;
		let Some(start) = word(data, at) else {
			return false;
		};
		let Some(delete) = word(data, at + 4) else {
			return false;
		};
		let Some(insert) = data.get(at + 8..at + 10) else {
			return false;
		};
		records.push(Record {
			start: start as usize,
			delete: delete as usize,
			insert: usize::from(u16::from_le_bytes(insert.try_into().unwrap())),
		});
	}
	let mut source = String::from_utf8_lossy(body).into_owned();
	let mut previous = markview_commonmark::parse(source.clone());
	let mut pool_at = 0;
	for record in records {
		if source.len() > 64 * 1024 {
			break;
		}
		let mut start = record.start % (source.len() + 1);
		while !source.is_char_boundary(start) {
			start -= 1;
		}
		let mut end = (start + record.delete).min(source.len());
		while !source.is_char_boundary(end) {
			end += 1;
		}
		let insert = if pool.is_empty() {
			""
		} else {
			let mut from = pool_at % pool.len();
			while !std::str::from_utf8(&pool[from..])
				.is_ok_and(|s| !s.is_empty())
				&& from + 1 < pool.len()
			{
				from += 1;
			}
			let from = from.min(pool.len());
			let len = record.insert.min(pool.len() - from);
			let mut to = from + len;
			while to > from && std::str::from_utf8(&pool[from..to]).is_err() {
				to -= 1;
			}
			pool_at = (pool_at + record.insert).max(from + 1);
			std::str::from_utf8(&pool[from..to]).unwrap()
		};
		source.replace_range(start..end, insert);
		let full = markview_commonmark::parse(source.clone());
		let incr =
			markview_commonmark::reparse(&previous, Arc::from(source.as_str()));
		assert_eq!(
			oracle::document(&full),
			oracle::document(&incr),
			"incremental sequence diverged after {record:?}"
		);
		previous = incr;
		coverage::hit(coverage::Hit::ReparseStep);
	}
	true
}
