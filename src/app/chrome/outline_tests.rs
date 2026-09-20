use super::*;
use crate::app::TOP;

fn entry(level: u8, text: &str) -> OutlineEntry {
	OutlineEntry {
		level,
		text: text.into(),
		anchor: text.to_lowercase(),
	}
}

fn glyphs(draws: &[Draw]) -> Vec<(f32, f32)> {
	draws
		.iter()
		.filter_map(|draw| match draw {
			Draw::Glyph(glyph) => Some((glyph.x, glyph.y)),
			_ => None,
		})
		.collect()
}

fn clipped(draws: &[Draw]) -> (Rect, &[Draw]) {
	draws
		.iter()
		.find_map(|draw| match draw {
			Draw::Clipped { rect, draws } => Some((*rect, draws.as_slice())),
			_ => None,
		})
		.expect("the entry list is clipped")
}

#[test]
fn entry_rows_indent_by_level_and_stay_inside_the_drawer() {
	let mut ui = crate::test_support::shaper();
	let drawer = rect(800.0, 600.0, TOP);
	let entries = [entry(1, "One"), entry(3, "Three")];
	let draws = draw(
		&mut ui,
		&InteractionState::default(),
		&entries,
		None,
		drawer,
	);
	let (clip, body) = clipped(&draws);
	let list = viewport(drawer);
	assert_eq!(
		(clip.x, clip.y, clip.w, clip.h),
		(list.x, list.y, list.w, list.h)
	);
	// The leftmost glyph of each row shows the indent that level produced.
	let mut left = [f32::INFINITY; 2];
	for (x, y) in glyphs(body) {
		let row = ((y - list.y) / ROW).floor().max(0.0) as usize;
		if row < left.len() {
			left[row] = left[row].min(x);
		}
	}
	assert!(
		left[0].is_finite() && left[1].is_finite(),
		"both rows wrote"
	);
	assert!((left[0] - (drawer.x + INSET)).abs() < 0.01);
	assert!(
		(left[1] - (drawer.x + INSET + 2.0 * LEVEL_INDENT)).abs() < 0.01,
		"a level-three entry is indented twice more than a level one"
	);
	assert!(left[1] > left[0]);
	for (x, _) in glyphs(body) {
		assert!(x >= drawer.x && x < drawer.x + drawer.w);
	}
	for button in buttons(drawer, entries.len(), 0.0) {
		assert!(button.rect.x >= drawer.x);
		assert!(button.rect.x + button.rect.w <= drawer.x + drawer.w);
		assert!(button.rect.y >= clip.y);
		assert!(button.rect.y + button.rect.h <= clip.y + clip.h);
	}
}

#[test]
fn tabbing_marks_the_row_keyboard_focus_lands_on() {
	let mut ui = crate::test_support::shaper();
	let drawer = rect(800.0, 600.0, TOP);
	let entries = [entry(1, "One"), entry(1, "Two"), entry(1, "Three")];
	let mut interaction = InteractionState::default();
	assert!(interaction.toggle_outline(entries.len(), Some(0)));
	// Tab walks from the toolbar toggle onto the second entry row.
	let buttons = [
		Command::Outline,
		Command::OutlineGoto(0),
		Command::OutlineGoto(1),
		Command::OutlineGoto(2),
	];
	interaction.focus = Some(Command::Outline);
	interaction.tab_focus(&buttons, false);
	assert_eq!(
		interaction.tab_focus(&buttons, false),
		Some(Command::OutlineGoto(1))
	);
	// The frame marks exactly the row that took the focus.
	let draws = draw(&mut ui, &interaction, &entries, None, drawer);
	let (list, body) = clipped(&draws);
	let marked: Vec<Rect> = body
		.iter()
		.filter_map(|draw| match draw {
			Draw::Rect(rect, Paint::Styled(Condition::Button, C::Accent)) => {
				Some(*rect)
			}
			_ => None,
		})
		.collect();
	assert_eq!(marked.len(), 1, "exactly one row carries the marker");
	let row = row_rect(list, 1, interaction.outline_scroll);
	assert_eq!((marked[0].x, marked[0].y), (row.x, row.y + 3.0));
}

#[test]
fn a_long_outline_only_offers_its_visible_rows() {
	let drawer = rect(800.0, 600.0, TOP);
	let entries: Vec<OutlineEntry> = (0..1000)
		.map(|i| entry(1, &format!("Heading {i}")))
		.collect();
	let rows = buttons(drawer, entries.len(), 0.0);
	assert!(!rows.is_empty());
	assert!(rows.len() < 30, "only the visible rows are clickable");
	let list = viewport(drawer);
	let max = max_scroll(drawer, entries.len());
	assert!(max > 0.0);
	let bottom = buttons(drawer, entries.len(), max);
	assert_eq!(
		bottom.last().map(|b| b.action),
		Some(Command::OutlineGoto(entries.len() - 1))
	);
	for button in bottom {
		assert!(list.intersect(button.rect).is_some());
	}
}

#[test]
fn row_hit_targets_stop_at_the_list_viewport() {
	let drawer = rect(800.0, 600.0, TOP);
	let entries = 100;
	let list = viewport(drawer);
	// Half a row is scrolled away, so the first row is only partly visible and
	// the extra row below the list must be dropped; buttons match what is drawn.
	let scroll = ROW / 2.0;
	let rows = buttons(drawer, entries, scroll);
	assert!(!rows.is_empty());
	for button in &rows {
		let hit = list
			.intersect(button.rect)
			.expect("a row button is inside the list");
		assert_eq!(
			(hit.x, hit.y, hit.w, hit.h),
			(button.rect.x, button.rect.y, button.rect.w, button.rect.h),
			"a row button is already clipped to the list"
		);
	}
	// The Contents header above the list is not a row.
	assert!(
		!rows
			.iter()
			.any(|b| b.rect.contains(list.x + 5.0, list.y - 1.0)),
		"the header must not activate a row"
	);
	// Nor is the strip just below the list.
	assert!(
		!rows
			.iter()
			.any(|b| b.rect.contains(list.x + 5.0, list.y + list.h + 1.0)),
		"nothing below the list must activate a row"
	);
	// The half-visible first row is still clickable where it is drawn.
	assert_eq!(rows[0].action, Command::OutlineGoto(0));
	assert!(rows[0].rect.contains(list.x + 5.0, list.y + 1.0));
	assert_eq!(rows[0].rect.y, list.y);
	assert!((rows[0].rect.h - scroll).abs() < 0.01);
}

#[test]
fn a_shorter_document_pulls_the_drawer_back_into_range() {
	let drawer = rect(800.0, 600.0, TOP);
	let list = viewport(drawer);
	let long = 1000;
	let mut interaction = InteractionState::default();
	assert!(interaction.toggle_outline(long, Some(0)));
	// The drawer was scrolled to the bottom of a long outline.
	interaction.outline_scroll = max_scroll(drawer, long);
	interaction.outline_selection = Some(long - 1);
	// A reload drops most headings, so the old offsets no longer fit.
	let short = 100;
	normalize(drawer, short, &mut interaction);
	assert_eq!(interaction.outline_scroll, max_scroll(drawer, short));
	let rows = buttons(drawer, short, interaction.outline_scroll);
	assert!(!rows.is_empty(), "the shorter outline still shows rows");
	let selected = interaction.outline_selection.unwrap();
	assert!(
		rows.iter()
			.any(|b| b.action == Command::OutlineGoto(selected)),
		"the selection is a visible row"
	);
	// A document short enough to fit whole shows every row and scrolls back.
	let tiny = 4;
	normalize(drawer, tiny, &mut interaction);
	assert_eq!(interaction.outline_scroll, 0.0);
	assert!(interaction.outline_selection.unwrap() < tiny);
	assert_eq!(visible(list, tiny, 0.0).len(), tiny);
	// A headingless document has nothing to select.
	normalize(drawer, 0, &mut interaction);
	assert_eq!(interaction.outline_selection, None);
	assert_eq!(interaction.outline_scroll, 0.0);
}

#[test]
fn the_empty_outline_draws_a_short_empty_state() {
	let mut ui = crate::test_support::shaper();
	let drawer = rect(800.0, 600.0, TOP);
	let draws = draw(&mut ui, &InteractionState::default(), &[], None, drawer);
	assert!(buttons(drawer, 0, 0.0).is_empty());
	assert!(
		!draws
			.iter()
			.any(|draw| matches!(draw, Draw::Clipped { .. })),
		"there are no rows to clip"
	);
	let list = viewport(drawer);
	assert!(
		glyphs(&draws)
			.iter()
			.any(|(_, y)| *y >= list.y && *y <= list.y + ROW),
		"the empty state is written in the list area"
	);
}

#[test]
fn revealing_an_entry_scrolls_it_into_the_list() {
	let drawer = rect(800.0, 600.0, TOP);
	let entries = 100;
	let list = viewport(drawer);
	let max = max_scroll(drawer, entries);
	// An entry above the list pulls it up to the top of the viewport.
	assert_eq!(reveal(drawer, entries, 300.0, 2), 2.0 * ROW);
	// An entry below the list brings its bottom edge into view.
	let bottom = reveal(drawer, entries, 0.0, 40);
	assert!((bottom - (41.0 * ROW - list.h)).abs() < 0.01);
	// An entry already visible leaves the offset alone.
	assert_eq!(reveal(drawer, entries, 0.0, 1), 0.0);
	assert!(reveal(drawer, entries, max, entries - 1) <= max);
}

#[test]
fn the_wheel_over_the_list_scrolls_it_and_not_the_document() {
	let drawer = rect(800.0, 600.0, TOP);
	let mut interaction = InteractionState::default();
	assert!(interaction.toggle_outline(50, Some(0)));
	let max = max_scroll(drawer, 50);
	assert!(max > 0.0);
	// A point over the drawer routes to the list, which moves by the wheel.
	let over = (drawer.x + 10.0, drawer.y + 60.0);
	assert!(drawer.contains(over.0, over.1));
	interaction.scroll_outline(42.0, max);
	assert_eq!(interaction.outline_scroll, 42.0);
	// A point beside it does not, so the document keeps its own scroll.
	let beside = (drawer.x - 10.0, drawer.y + 60.0);
	assert!(!drawer.contains(beside.0, beside.1));
	assert_eq!(interaction.outline_scroll, 42.0);
}
