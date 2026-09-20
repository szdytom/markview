use super::wheel_pixels;
use crate::platform::{WheelAmount, WheelNotch};
use winit::{
	dpi::PhysicalPosition,
	event::MouseScrollDelta::{LineDelta, PixelDelta},
};

/// Both axes driven by the same desktop value.
fn even(lines: f32) -> WheelNotch {
	WheelNotch {
		vertical: WheelAmount::Lines(lines),
		horizontal: WheelAmount::Lines(lines),
	}
}

#[test]
fn a_notch_follows_the_desktop_lines_and_the_reader_speed() {
	// Three lines a notch is the Windows default, at the reader's own speed.
	assert_eq!(
		wheel_pixels(LineDelta(0.0, 1.0), even(3.0), 1.0, 1.0, (600.0, 800.0)),
		(0.0, 126.0)
	);
	// The reader's multiplier scales whatever the desktop chose.
	assert_eq!(
		wheel_pixels(LineDelta(0.0, 1.0), even(3.0), 0.5, 1.0, (600.0, 800.0)),
		(0.0, 63.0)
	);
	// A sideways notch answers the same way.
	assert_eq!(
		wheel_pixels(LineDelta(1.0, 0.0), even(2.0), 2.0, 1.0, (600.0, 800.0)),
		(168.0, 0.0)
	);
}

#[test]
fn each_axis_takes_its_own_desktop_value() {
	// Windows sets the two axes apart: a vertical page notch must not drag
	// the horizontal one along with it, nor the other way round.
	let notch = WheelNotch {
		vertical: WheelAmount::Page,
		horizontal: WheelAmount::Lines(2.0),
	};
	let (dx, dy) =
		wheel_pixels(LineDelta(1.0, 1.0), notch, 1.0, 1.0, (600.0, 800.0));
	assert_eq!(dx, 84.0);
	assert!((dy - 720.0).abs() < 0.001, "{dy}");

	// A horizontal page travels the viewport's width, not its height.
	let notch = WheelNotch {
		vertical: WheelAmount::Lines(3.0),
		horizontal: WheelAmount::Page,
	};
	let (dx, dy) =
		wheel_pixels(LineDelta(1.0, 1.0), notch, 1.0, 1.0, (600.0, 800.0));
	assert!((dx - 540.0).abs() < 0.001, "{dx}");
	assert_eq!(dy, 126.0);
}

#[test]
fn a_touchpad_delta_is_logical_and_scaled() {
	// A physical delta on a 2x display is halved, then sped up 2x.
	assert_eq!(
		wheel_pixels(
			PixelDelta(PhysicalPosition::new(0.0, 100.0)),
			even(1.0),
			2.0,
			2.0,
			(600.0, 800.0)
		),
		(0.0, 100.0)
	);
}
