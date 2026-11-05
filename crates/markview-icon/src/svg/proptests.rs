use crate::svg::*;
use proptest::prelude::*;

proptest! {
	#[test]
	fn icon_paths_flatten_nested_transforms_and_viewboxes_without_losing_curve_controls(
		power in 0u8..3, scale in 1u8..5, offset in prop::array::uniform2(8u8..17),
		translation in prop::array::uniform2(0u8..5), viewport in prop::array::uniform2(0u8..5),
		controls in prop::array::uniform4(0u8..17), mode in 1u8..4, stroke in 1u8..6,
	) {
		let side = 128u16 << power;
		let [dx, dy] = offset;
		let [tx, ty] = translation;
		let [vx, vy] = viewport;
		let [x1, y1, x2, y2] = controls;
		let fill = if mode & 1 != 0 { "black" } else { "none" };
		let line = if mode & 2 != 0 { "black" } else { "none" };
		let source = format!("<svg xmlns='http://www.w3.org/2000/svg' width='{side}' height='{side}' viewBox='{vx} {vy} {side} {side}'><g transform='translate({dx} {dy})'><g transform='scale({scale})'><path transform='translate({tx} {ty})' d='M0 0 L16 0 C{x1} {y1} {x2} {y2} 16 16 L0 16 Z' fill='{fill}' stroke='{line}' stroke-width='{stroke}'/></g></g></svg>");
		let figures = parse(&source).unwrap();
		let x = |x| (f64::from(scale) * (x + f64::from(tx)) + f64::from(dx) - f64::from(vx)) / f64::from(side);
		let y = |y| (f64::from(scale) * (y + f64::from(ty)) + f64::from(dy) - f64::from(vy)) / f64::from(side);
		let commands = vec![Command::MoveTo { x: x(0.0), y: y(0.0) }, Command::LineTo { x: x(16.0), y: y(0.0) }, Command::CubicTo { x1: x(f64::from(x1)), y1: y(f64::from(y1)), x2: x(f64::from(x2)), y2: y(f64::from(y2)), x: x(16.0), y: y(16.0) }, Command::LineTo { x: x(0.0), y: y(16.0) }, Command::Close];
		let mut expected = Vec::new();
		if mode & 1 != 0 { expected.push(Figure { commands: commands.clone(), fill: true, stroke_width: 0.0 }); }
		if mode & 2 != 0 { expected.push(Figure { commands, fill: false, stroke_width: f32::from(stroke) * f32::from(scale) / f32::from(side) }); }
		prop_assert_eq!(figures, expected);
	}

	#[test]
	fn icon_validation_requires_a_square_viewport_and_at_least_one_visible_figure(
		side in 1u16..257, extra in 1u16..65, visible in prop::collection::vec(any::<bool>(), 0..12),
	) {
		let nonsquare = format!("<svg xmlns='http://www.w3.org/2000/svg' width='{side}' height='{}'><path d='M0 0 L1 1' stroke='black'/></svg>", side + extra);
		prop_assert!(parse(&nonsquare).is_err());
		let mut source = "<svg xmlns='http://www.w3.org/2000/svg' width='64' height='64' fill='none' stroke='black'>".to_owned();
		for (index, &visible) in visible.iter().enumerate() { source.push_str(&format!("<path visibility='{}' d='M0 {index} L16 {}'/>", if visible { "visible" } else { "hidden" }, index + 1)); }
		source.push_str("</svg>");
		let result = parse(&source);
		let count = visible.iter().filter(|&&visible| visible).count();
		prop_assert_eq!(result.is_ok(), count > 0);
		if let Ok(figures) = result { prop_assert_eq!(figures.len(), count); }
	}
}
