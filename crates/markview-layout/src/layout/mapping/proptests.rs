use super::*;
use markview_document::{FootnoteId, FootnoteReference, LinkOccurrenceId};
use proptest::prelude::*;

fn occurrence(start: usize) -> LinkOccurrenceId {
	LinkOccurrenceId::new(start as u32 + 1).unwrap()
}
fn note(start: usize) -> FootnoteReference {
	FootnoteReference {
		definition: FootnoteId::new(format!("note-{start}")),
		number: start as u32,
		occurrence: occurrence(start),
	}
}

fn prepared() -> Prepared {
	Prepared {
		anchors: vec![],
		images: BTreeMap::new(),
		image_indices: BTreeMap::new(),
		reading: String::new(),
		search_ranges: vec![],
		mapping: vec![],
		text: String::new(),
		spans: vec![],
		link_ids: vec![],
		link_occurrences: vec![],
		padding: vec![],
		math: BTreeMap::new(),
		notes: BTreeMap::new(),
		note_spans: BTreeMap::new(),
	}
}
proptest! {
	#[test]
	fn reading_lookup_matches_linear_segments_with_gaps(
		segments in prop::collection::vec((0usize..6, 0usize..9, any::<bool>(), 0usize..3), 0..24),
		queries in prop::collection::vec((0usize..256, 0usize..256), 1..64),
	) {
		let mut p = prepared();
		let (mut visual, mut logical) = (0, 0);
		for (length, reading_length, atomic, gap) in segments {
			visual += gap;
			let reading_length = if atomic { reading_length } else { length };
			p.mapping.push((visual..visual + length, logical..logical + reading_length, atomic));
			visual += length;
			logical += reading_length;
		}
		p.reading = "r".repeat(logical);
		for (a, b) in queries {
			let range = a.min(b)..a.max(b);
			let expected = p.mapping.iter().find(|(v, _, _)| v.contains(&range.start)).map_or(
				logical..logical,
				|(v, l, atomic)| {
					if *atomic { return l.clone(); }
					let end = p.mapping.iter().find(|(v, _, _)| v.start < range.end && range.end <= v.end)
						.map_or(logical, |(v, l, atomic)| if *atomic { l.end } else { l.start + range.end - v.start });
					l.start + range.start - v.start..end
				},
			);
			prop_assert_eq!(p.reading_range(range), expected);
		}
	}

	#[test]
	fn tabs_preserve_byte_origins(tokens in prop::collection::vec(prop::sample::select(vec!["a","中","😀","\t","\n"]),0..48), size in 1usize..17) {
		let text = tokens.concat();
		let (out, offsets) = expand_tabs_mapped(&text, size);
		let mut expected = String::new();
		let mut map = vec![0];
		let mut column = 0;
		for (i, ch) in text.char_indices() {
			if ch == '\t' {
				let stop = (column / size + 1) * size;
				while column < stop {
					expected.push(' ');
					column += 1;
					map.push(if column == stop { i + 1 } else { i });
				}
			} else {
				expected.push(ch);
				map.extend(i + 1..=i + ch.len_utf8());
				column += 1;
			}
		}
		prop_assert_eq!(&out, &expected);
		prop_assert_eq!(&offsets, &map);
		prop_assert_eq!(offsets.len(), out.len() + 1);
		prop_assert!(offsets.windows(2).all(|w| w[0] <= w[1]));
	}
	#[test]
	fn reading_segments_and_note_ranges(lengths in prop::collection::vec((1usize..9,1usize..9,any::<bool>()),1..12)) {
		let mut p = prepared();
		let mut starts = Vec::new();
		let (mut v, mut l) = (0, 0);
		for (visual, logical, atomic) in lengths {
			let logical = if atomic { logical } else { visual };
			p.mapping.push((v..v + visual, l..l + logical, atomic));
			starts.push((v, l, visual, logical, atomic));
			v += visual;
			l += logical;
		}
		p.reading = "r".repeat(l);
		let mut start_positions = Vec::new();
		let mut end_positions = vec![0];
		for &(vs, ls, vlen, llen, atomic) in &starts {
			for displacement in 0..vlen {
				start_positions
					.push((ls + displacement, atomic.then_some(ls..ls + llen)));
			}
			for displacement in 1..=vlen {
				end_positions.push(if atomic {
					ls + llen
				} else {
					ls + displacement
				});
			}
			p.notes
				.insert(vs, (note(vs), vs + vlen, LinkId::Inline(occurrence(vs))));
		}
		for (start, (logical, atomic)) in start_positions.iter().enumerate() {
			for (end, &logical_end) in
				end_positions.iter().enumerate().skip(start + 1)
			{
				let expected = atomic.clone().unwrap_or(*logical..logical_end);
				prop_assert_eq!(
					p.reading_range(start..end),
					expected,
					"visual={:?}",
					start..end
				);
			}
		}
		prop_assert_eq!(p.reading_range(v..v + 1), l..l);
		for offset in 0..=v {
			let expected = starts
				.iter()
				.find(|&&(s, _, n, _, _)| (s..s + n).contains(&offset));
			prop_assert_eq!(
				p.note_at(offset)
					.map(|(reference, first)| (reference.clone(), first)),
				expected.map(|&(s, _, _, _, _)| (note(s), offset == s))
			);
			prop_assert_eq!(
				p.link_id(offset, None),
				expected.map(|&(s, _, _, _, _)| LinkId::Inline(occurrence(s)))
			);
			p.link_ids = vec![Some(LinkId::Special(999))];
			prop_assert_eq!(p.link_id(offset, Some(0)), Some(LinkId::Special(999)));
			if let Some(&(s, ls, _, _, false)) = expected {
				prop_assert_eq!(p.reading_range(offset..v), ls + offset - s..l);
			}
		}
	}
}

#[test]
fn empty_atomic_and_unmapped_ranges() {
	let mut p = prepared();
	p.reading = "abcdef".into();
	p.mapping = vec![(0..2, 0..2, false), (4..5, 2..6, true)];
	assert_eq!(p.reading_range(1..1), 1..1);
	assert_eq!(p.reading_range(4..4), 2..6);
	assert_eq!(p.reading_range(2..4), 6..6);
	assert_eq!(p.reading_range(1..3), 1..6);
	assert_eq!(p.reading_range(1..5), 1..6);
}

#[test]
fn empty_segments_and_reverse_queries_preserve_endpoints() {
	let mut p = prepared();
	p.reading = "abcdefgh".into();
	p.mapping = vec![
		(0..0, 0..0, false),
		(0..2, 0..2, false),
		(2..2, 2..2, false),
		(4..5, 2..6, true),
		(5..5, 6..6, false),
		(5..7, 6..8, false),
	];
	for (query, expected) in [
		(5..6, 6..7),
		(4..4, 2..6),
		(1..5, 1..6),
		(0..2, 0..2),
		(2..4, 8..8),
		(0..0, 0..8),
	] {
		assert_eq!(p.reading_range(query), expected);
	}
}
