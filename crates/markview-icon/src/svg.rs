//! Turns an SVG document into flat vector figures.
//!
//! Parsing is delegated to `usvg`, the simplifier behind `resvg`, which
//! resolves view boxes, transforms, basic shapes, `<use>` references and arc
//! commands. What remains is to flatten its tree into one command list per
//! painted figure and normalize the coordinates to a unit box.
use usvg::tiny_skia_path;

/// One painted figure of an icon, in a unit box.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Figure {
	pub(crate) commands: Vec<Command>,
	pub(crate) fill: bool,
	/// Stroke width in unit-box units; ignored when `fill`.
	pub(crate) stroke_width: f32,
}

/// An absolute outline command, mirroring `markview_core::scene::PathCommand`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Command {
	MoveTo {
		x: f64,
		y: f64,
	},
	LineTo {
		x: f64,
		y: f64,
	},
	QuadTo {
		x1: f64,
		y1: f64,
		x: f64,
		y: f64,
	},
	CubicTo {
		x1: f64,
		y1: f64,
		x2: f64,
		y2: f64,
		x: f64,
		y: f64,
	},
	Close,
}

pub(crate) fn parse(source: &str) -> Result<Vec<Figure>, String> {
	let tree = usvg::Tree::from_str(source, &usvg::Options::default())
		.map_err(|error| error.to_string())?;
	let size = tree.size();
	let side = size.width();
	if side <= 0.0 || (size.height() - side).abs() > 0.5 {
		return Err(format!(
			"icon must be square, got {}x{}",
			size.width(),
			size.height()
		));
	}
	let mut figures = Vec::new();
	collect(tree.root(), side, &mut figures);
	if figures.is_empty() {
		return Err("icon has no visible geometry".into());
	}
	Ok(figures)
}

fn collect(group: &usvg::Group, side: f32, out: &mut Vec<Figure>) {
	for node in group.children() {
		match node {
			usvg::Node::Group(group) => collect(group, side, out),
			usvg::Node::Path(path) => figure(path, side, out),
			_ => {}
		}
	}
}

fn figure(path: &usvg::Path, side: f32, out: &mut Vec<Figure>) {
	if !path.is_visible() {
		return;
	}
	let transform = path.abs_transform();
	let Some(data) = path.data().clone().transform(transform) else {
		return;
	};
	let commands: Vec<Command> = data
		.segments()
		.map(|segment| command(segment, side))
		.collect();
	if commands.is_empty() {
		return;
	}
	if path.fill().is_some() {
		out.push(Figure {
			commands: commands.clone(),
			fill: true,
			stroke_width: 0.0,
		});
	}
	if let Some(stroke) = path.stroke() {
		// `stroke-width` is in user units; the transform maps them onto the
		// canvas the commands were flattened into.
		let scale = (transform.sx.hypot(transform.ky)
			+ transform.kx.hypot(transform.sy))
			/ 2.0;
		out.push(Figure {
			commands,
			fill: false,
			stroke_width: stroke.width().get() * scale / side,
		});
	}
}

fn command(segment: tiny_skia_path::PathSegment, side: f32) -> Command {
	let scale = |value: f32| f64::from(value / side);
	match segment {
		tiny_skia_path::PathSegment::MoveTo(p) => Command::MoveTo {
			x: scale(p.x),
			y: scale(p.y),
		},
		tiny_skia_path::PathSegment::LineTo(p) => Command::LineTo {
			x: scale(p.x),
			y: scale(p.y),
		},
		tiny_skia_path::PathSegment::QuadTo(a, b) => Command::QuadTo {
			x1: scale(a.x),
			y1: scale(a.y),
			x: scale(b.x),
			y: scale(b.y),
		},
		tiny_skia_path::PathSegment::CubicTo(a, b, c) => Command::CubicTo {
			x1: scale(a.x),
			y1: scale(a.y),
			x2: scale(b.x),
			y2: scale(b.y),
			x: scale(c.x),
			y: scale(c.y),
		},
		tiny_skia_path::PathSegment::Close => Command::Close,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn square(body: &str) -> String {
		format!(
			r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2">{body}</svg>"##
		)
	}

	#[test]
	fn a_stroked_line_normalizes_into_the_unit_box() {
		let figures = parse(&square(r#"<path d="M0 0 24 12"/>"#)).unwrap();
		assert_eq!(figures.len(), 1);
		let figure = &figures[0];
		assert!(!figure.fill);
		assert_eq!(figure.stroke_width, 2.0 / 24.0);
		assert_eq!(
			figure.commands,
			vec![
				Command::MoveTo { x: 0.0, y: 0.0 },
				Command::LineTo { x: 1.0, y: 0.5 },
			]
		);
	}

	#[test]
	fn basic_shapes_and_transforms_flatten() {
		let source = r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8" viewBox="0 0 8 8" fill="#000">
			<rect x="1" y="1" width="6" height="6"/>
			<g transform="translate(4 4)"><path d="M0 0 4 0" fill="none" stroke="#000" stroke-width="1"/></g>
		</svg>"##;
		let figures = parse(source).unwrap();
		assert_eq!(figures.len(), 2);
		assert!(figures[0].fill);
		assert!(!figures[1].fill);
		assert_eq!(figures[1].stroke_width, 1.0 / 8.0);
		assert_eq!(
			figures[1].commands.last(),
			Some(&Command::LineTo { x: 1.0, y: 0.5 })
		);
	}

	#[test]
	fn a_scaled_viewport_keeps_the_stroke_weight() {
		let source = r##"<svg xmlns="http://www.w3.org/2000/svg" width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2"><path d="M0 0 24 24"/></svg>"##;
		let figures = parse(source).unwrap();
		assert_eq!(figures[0].stroke_width, 2.0 / 24.0);
		assert_eq!(
			figures[0].commands.last(),
			Some(&Command::LineTo { x: 1.0, y: 1.0 })
		);
	}

	#[test]
	fn a_filled_and_stroked_path_becomes_two_figures() {
		let source = r##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4" viewBox="0 0 4 4">
			<path d="M0 0 4 4" fill="#000" stroke="#000" stroke-width="1"/>
		</svg>"##;
		let figures = parse(source).unwrap();
		assert_eq!(figures.len(), 2);
		assert!(figures[0].fill);
		assert!(!figures[1].fill);
	}

	#[test]
	fn a_non_square_viewport_is_rejected() {
		let source = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="12" viewBox="0 0 24 12"><path d="M0 0 1 1"/></svg>"#;
		assert!(parse(source).unwrap_err().contains("square"));
	}

	#[test]
	fn an_empty_file_is_rejected() {
		assert!(parse("<svg/>").unwrap_err().contains("geometry"));
	}
}
