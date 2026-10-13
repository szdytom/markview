//! Caption controls use the same rectangles as native hit testing.
use super::icons;
use crate::{
	app::frame::{Caption, Layout},
	layout::{Draw, Paint},
	settings::WindowLayout,
};
use markview_layout::{
	scene::IconPath,
	style::{Color, ColorField as C, Condition},
};

const MINIMIZE: &[IconPath] =
	markview_icon::icon!("assets/ui/window-minimize.svg");
const MAXIMIZE: &[IconPath] =
	markview_icon::icon!("assets/ui/window-maximize.svg");
const RESTORE: &[IconPath] =
	markview_icon::icon!("assets/ui/window-restore.svg");
const CIRCLE: &[IconPath] = markview_icon::icon!("assets/ui/window-circle.svg");
const LINUX_MINIMIZE: &[IconPath] =
	markview_icon::icon!("assets/ui/window-chevron-down.svg");
const LINUX_MAXIMIZE: &[IconPath] =
	markview_icon::icon!("assets/ui/window-chevron-up.svg");
const LINUX_RESTORE: &[IconPath] =
	markview_icon::icon!("assets/ui/window-diamond.svg");
const LINUX_CLOSE: &[IconPath] =
	markview_icon::icon!("assets/ui/window-cross.svg");

pub(super) fn draw(layout: Layout) -> Vec<Draw> {
	if layout.native_buttons || layout.fullscreen {
		return Vec::new();
	}
	let mut out = Vec::new();
	for (caption, rect) in layout.captions() {
		let hover = layout.hover == Some(caption);
		let pressed = layout.pressed == Some(caption) && hover;
		let paths = caption_paths(layout, caption);
		let mac = layout.style == WindowLayout::Macos;
		let Some(paint) =
			caption_paint(&mut out, layout, caption, rect, hover, pressed)
		else {
			continue;
		};
		let size = if mac { 14.0 } else { 18.0 };
		out.push(Draw::Icon {
			paths,
			paint,
			x: rect.x + (rect.w - size) / 2.0,
			y: rect.y + (rect.h - size) / 2.0,
			size,
		});
	}
	out
}

fn caption_paths(layout: Layout, caption: Caption) -> &'static [IconPath] {
	match caption {
		Caption::Close if layout.style == WindowLayout::Linux => LINUX_CLOSE,
		Caption::Close => icons::CLOSE,
		Caption::Minimize if layout.style == WindowLayout::Linux => {
			LINUX_MINIMIZE
		}
		Caption::Expand
			if layout.style == WindowLayout::Linux && layout.maximized =>
		{
			LINUX_RESTORE
		}
		Caption::Expand if layout.style == WindowLayout::Linux => {
			LINUX_MAXIMIZE
		}
		Caption::Minimize => MINIMIZE,
		Caption::Expand if layout.maximized => RESTORE,
		Caption::Expand => MAXIMIZE,
	}
}

fn caption_paint(
	out: &mut Vec<Draw>,
	layout: Layout,
	caption: Caption,
	rect: crate::layout::Rect,
	hover: bool,
	pressed: bool,
) -> Option<Paint> {
	let mac = layout.style == WindowLayout::Macos;
	Some(if mac {
		let color = if !layout.focused {
			0xa6a6a6ff
		} else {
			match caption {
				Caption::Close => 0xff5f57ff,
				Caption::Minimize => 0xffbd2eff,
				Caption::Expand => 0x28c840ff,
			}
		};
		out.push(Draw::Icon {
			paths: CIRCLE,
			paint: Paint::Color(Color(color)),
			x: rect.x + (rect.w - 12.0) / 2.0,
			y: 14.0,
			size: 12.0,
		});
		if !hover {
			return None;
		}
		Paint::Color(Color(if pressed { 0x000000ff } else { 0x000000b0 }))
	} else if hover {
		out.push(Draw::Rect(
			rect,
			if caption == Caption::Close {
				Paint::Color(Color(if pressed {
					0xa51d18ff
				} else {
					0xc42b1cff
				}))
			} else {
				Paint::Styled(
					Condition::Button,
					if pressed {
						C::ActiveBackground
					} else {
						C::HoverBackground
					},
				)
			},
		));
		if caption == Caption::Close {
			Paint::Color(Color(0xffffffff))
		} else {
			Paint::Styled(Condition::Toolbar, C::Color)
		}
	} else {
		Paint::Styled(
			Condition::Toolbar,
			if layout.focused { C::Color } else { C::Muted },
		)
	})
}
