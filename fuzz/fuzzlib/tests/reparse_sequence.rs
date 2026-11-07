use mvfuzz::{mutators, reparse_sequence};

fn sequence(body: &[u8], pool: &[u8], records: &[(u32, u32, u16)]) -> Vec<u8> {
	let mut out = b"RPS1".to_vec();
	out.push(records.len() as u8);
	out.extend((body.len() as u32).to_le_bytes());
	out.extend((pool.len() as u32).to_le_bytes());
	for (start, delete, insert) in records {
		out.extend(start.to_le_bytes());
		out.extend(delete.to_le_bytes());
		out.extend(insert.to_le_bytes());
	}
	out.extend(pool);
	out.extend(body);
	out
}

#[test]
fn structured_sequences_match_a_full_parse_at_every_step() {
	let body = b"# title\n\nparagraph **bold**\n\n> quote\n\n- item\n\n```rust\nlet x = 1;\n```\n";
	let pool =
		b"## heading\n\n- inserted\n\n[ref]: https://example.org\n\nalpha";
	assert!(reparse_sequence::check(&sequence(
		body,
		pool,
		&[(3, 0, 9), (20, 5, 8), (40, 2, 6), (0, 1, 4)],
	)));
	// A truncated header is rejected without panicking.
	assert!(!reparse_sequence::check(b"RPS1\x01"));
}

#[test]
fn structured_header_reports_the_body_length() {
	let body = b"# title\n\nparagraph **bold**\n";
	let pool = b"[ref]: https://example.org\n\nalpha";
	let input = sequence(body, pool, &[(3, 0, 9), (20, 5, 8)]);
	let required = 13 + 2 * 9 + pool.len() + body.len();
	assert_eq!(mutators::reparse_required(&input), Some(required));
	assert_eq!(mutators::reparse_required(b"RPS1\x01"), None);
}
