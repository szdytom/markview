use super::*;
use crate::app::modal::{Message, Modal};

fn drawer() -> Rect {
	crate::app::chrome::outline::rect(800.0, 600.0, crate::app::TOP)
}

#[test]
fn a_document_change_drops_outgoing_row_indices() {
	let now = Instant::now();
	let mut model = Model::default();
	model.settle_rows(1, &[0, 1, 2]);
	model.retarget_rows(&[0, 1, 2], &[0], now);
	assert!(model.rows(&[0]).iter().any(|row| row.index == 1));
	model.settle_rows(2, &[0]);
	let rows = model.rows(&[0]);
	assert_eq!(rows.iter().map(|row| row.index).collect::<Vec<_>>(), [0]);
}

#[test]
fn closing_the_drawer_still_drops_outgoing_rows_on_a_new_document() {
	let now = Instant::now();
	let mut model = Model::default();
	model.settle_rows(1, &[0, 1, 2]);
	model.retarget_rows(&[0, 1, 2], &[0], now);
	model.settle_content(2);
	let rows = model.rows(&[0]);
	assert_eq!(rows.iter().map(|row| row.index).collect::<Vec<_>>(), [0]);
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
	interaction.show_panel(crate::state::PanelPage::Settings(
		crate::state::PanelTab::Generic,
	));
	assert!(!claims_pointer(&interaction, drawer, over.0, over.1));
	// A confirmation owns input in the same way.
	interaction.show_panel(crate::state::PanelPage::Closed);
	interaction
		.overlays
		.modal
		.send(Message::Open(Modal::OpenLocal {
			path: "local.bin".into(),
			dir: ".".into(),
			document_dir: None,
		}));
	assert!(!claims_pointer(&interaction, drawer, over.0, over.1));
	// A point beside the drawer is never its press, open or not.
	interaction.overlays.modal.send(Message::Dismiss);
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
