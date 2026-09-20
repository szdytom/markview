use super::*;
use crate::state::Modal;

fn drawer() -> Rect {
	crate::app::chrome::outline::rect(800.0, 600.0, crate::app::TOP)
}

#[test]
fn an_open_panel_takes_the_presses_the_drawer_covers() {
	let drawer = drawer();
	// A point on the drawer, inside the region a centred panel also covers.
	let over = (drawer.x + 10.0, drawer.y + 60.0);
	assert!(drawer.contains(over.0, over.1));
	let mut interaction = InteractionState::default();
	assert!(interaction.toggle_outline(4, Some(0)));
	// Only the drawer is open: it claims the press, so the document does not
	// begin a selection between its rows.
	assert!(claims_pointer(&interaction, drawer, over.0, over.1));
	// Opening a panel leaves the drawer open but hands every press to the
	// panel, so its scrollbar drag and its outside-click dismissal answer.
	interaction.panel_open = true;
	assert!(!claims_pointer(&interaction, drawer, over.0, over.1));
	// A confirmation owns input in the same way.
	interaction.panel_open = false;
	interaction.modal = Some(Modal::OpenLocal {
		path: "local.bin".into(),
		dir: ".".into(),
		document_dir: None,
	});
	assert!(!claims_pointer(&interaction, drawer, over.0, over.1));
	// A point beside the drawer is never its press, open or not.
	interaction.modal = None;
	assert!(!claims_pointer(
		&interaction,
		drawer,
		drawer.x - 10.0,
		over.1
	));
}

#[test]
fn the_drawer_claims_presses_over_a_row_and_over_its_background() {
	// Both the left and the middle button route through this predicate, so a
	// middle press between the rows cannot open the document link or image
	// drawn underneath them.
	let drawer = drawer();
	let mut interaction = InteractionState::default();
	assert!(interaction.toggle_outline(4, Some(0)));
	for point in [
		// On an entry row.
		(drawer.x + 20.0, drawer.y + 60.0),
		// The drawer's own background, below the last entry.
		(drawer.x + drawer.w - 10.0, drawer.y + drawer.h - 10.0),
	] {
		assert!(drawer.contains(point.0, point.1));
		assert!(claims_pointer(&interaction, drawer, point.0, point.1));
	}
	// Closing the drawer hands the same points back to the document.
	interaction.close_outline();
	assert!(!claims_pointer(
		&interaction,
		drawer,
		drawer.x + 20.0,
		drawer.y + 60.0
	));
}
